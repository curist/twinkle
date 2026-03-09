use super::env::{LocalEnv, TypeEnv, ValueEnv};
use super::error::TypeError;
use super::patterns::PatternChecker;
use super::ty::{
    CELL_TYPE_ID, ITER_ITEM_TYPE_ID, ITERATOR_TYPE_ID, MonoType, OPTION_TYPE_ID, RANGE_TYPE_ID,
    RESULT_TYPE_ID, UNFOLD_STEP_TYPE_ID, contains_meta, method_receiver_type_id, zonk_ty,
};
use super::type_map::TypeMap;
use crate::module::artifacts::TypedModule;
use crate::syntax::ast::{
    BinOp, Block, Expr, ExprId, ExprKind, FunctionDecl, Item, Literal, Pattern, SourceFile, Stmt,
    StringPart, Type as AstType, UnOp,
};
use crate::syntax::span::Span;
use std::collections::{HashMap, HashSet};

/// Bidirectional type checker
///
/// Uses synthesis mode (infer type) and checking mode (validate against expected type)
pub struct TypeChecker {
    type_env: TypeEnv,
    value_env: ValueEnv,
    local_env: LocalEnv,
    errors: Vec<TypeError>,
    type_map: TypeMap,

    // Track current function's return type for return statement checking
    current_function_ret: Option<MonoType>,

    // Module aliases (for cross-module call resolution)
    module_aliases: HashSet<String>,

    // Type variable scope — names in scope resolve to MonoType::Var
    type_var_scope: Vec<String>,

    // True when type-checking at module scope (top-level lets/stmts)
    at_module_scope: bool,

    // Unification engine: MetaVar counter and solved assignments
    next_meta: u32,
    meta_subst: HashMap<u32, MonoType>,

    // Internal host intrinsics are only callable from stdlib/prelude modules.
    allow_internal_host_builtins: bool,
}

impl TypeChecker {
    /// Type-check a complete module (source file).
    ///
    /// Takes accumulated `type_env` and `value_env` from the resolver, plus the
    /// set of known module aliases. Returns `(TypeMap, TypeEnv, ValueEnv)` so the
    /// caller can thread the updated environments to the next stage.
    pub fn check_module(
        ast: &SourceFile,
        type_env: TypeEnv,
        value_env: ValueEnv,
        module_aliases: HashSet<String>,
    ) -> Result<TypedModule, Vec<TypeError>> {
        Self::check_module_with_options(ast, type_env, value_env, module_aliases, false)
    }

    /// Type-check a module with internal host builtin access control.
    pub fn check_module_with_options(
        ast: &SourceFile,
        type_env: TypeEnv,
        value_env: ValueEnv,
        module_aliases: HashSet<String>,
        allow_internal_host_builtins: bool,
    ) -> Result<TypedModule, Vec<TypeError>> {
        let mut checker = TypeChecker {
            type_env,
            value_env,
            local_env: LocalEnv::new(),
            errors: Vec::new(),
            type_map: TypeMap::new(),
            current_function_ret: None,
            module_aliases,
            type_var_scope: Vec::new(),
            at_module_scope: true,
            next_meta: 0,
            meta_subst: HashMap::new(),
            allow_internal_host_builtins,
        };

        // Pass 1: Check all top-level lets and add to ValueEnv
        // This makes them available to all functions
        for item in &ast.items {
            if let Item::Stmt(stmt) = item {
                if let Stmt::Let {
                    pattern,
                    ty,
                    value,
                    span,
                    ..
                } = stmt
                {
                    // Only simple identifier patterns for top-level lets
                    if let Pattern::Ident(name, _) = pattern {
                        // Determine the expected type
                        let value_ty = if let Some(ann_ty) = ty {
                            // Type annotation provided - check mode
                            let expected = match checker.resolve_type(ann_ty) {
                                Ok(t) => t,
                                Err(()) => continue, // Error already recorded
                            };
                            match checker.check_expr(value, &expected) {
                                Ok(()) => expected,
                                Err(()) => continue, // Error already recorded
                            }
                        } else {
                            // No annotation - synthesis mode
                            let t = match checker.synth_expr(value) {
                                Ok(t) => t,
                                Err(()) => continue, // Error already recorded
                            };
                            let t = checker.zonk(&t);
                            if contains_meta(&t) {
                                checker.errors.push(TypeError::AmbiguousType {
                                    name: name.clone(),
                                    span: value.span,
                                    note: "type cannot be inferred; add a type annotation"
                                        .to_string(),
                                });
                                continue;
                            }
                            t
                        };

                        // Add to ValueEnv so it's accessible from functions
                        checker.value_env.add_value(name.clone(), value_ty);
                    } else {
                        checker.errors.push(TypeError::UnsupportedFeature {
                            feature: "pattern matching in top-level let bindings",
                            span: *span,
                            note: "Only simple identifiers are supported for top-level lets"
                                .to_string(),
                        });
                    }
                } else {
                    // For loops and other side-effectful statements at top-level
                    checker.check_top_level_stmt(stmt);
                }
            }
        }

        // Pass 2: Type-check all functions
        // Functions can now reference top-level lets
        for item in &ast.items {
            match item {
                Item::TypeDecl(_) | Item::Import(_) => {
                    // Already handled by resolver
                }
                Item::Function(decl) => {
                    checker.check_function(decl);
                }
                Item::Stmt(_) => {
                    // Already checked in Pass 1
                }
            }
        }

        // Final zonk: resolve any MetaVars from top-level stmt checking
        let meta_subst = std::mem::take(&mut checker.meta_subst);
        checker.type_map.zonk(&meta_subst);

        if checker.errors.is_empty() {
            Ok(TypedModule {
                type_map: checker.type_map,
                type_env: checker.type_env,
                value_env: checker.value_env,
            })
        } else {
            Err(checker.errors)
        }
    }

    //
    // Top-level checking
    //

    fn check_function(&mut self, decl: &FunctionDecl) {
        // Push type variable scope for generic functions
        let saved_type_vars = std::mem::replace(&mut self.type_var_scope, decl.type_params.clone());
        let saved_module_scope = std::mem::replace(&mut self.at_module_scope, false);

        // Push a new scope for the function body
        self.local_env.push_scope();

        // Get the function signature from ValueEnv (clone to avoid borrowing issues)
        let sig = match self.value_env.get_function(&decl.name) {
            Some(s) => s.clone(),
            None => {
                // Should not happen - resolver should have caught this
                self.errors.push(TypeError::UndefinedVariable {
                    name: decl.name.clone(),
                    span: decl.span,
                });
                self.local_env.pop_scope();
                return;
            }
        };

        // Bind parameters in local environment
        for (param, param_ty) in decl.params.iter().zip(sig.params.iter()) {
            self.local_env.bind(param.name.clone(), param_ty.clone());
        }

        // Set current function return type
        self.current_function_ret = sig.ret.clone();

        // Type-check the function body
        // The body is a Block, which should evaluate to the return type
        if let Some(expected_ret) = &sig.ret {
            // Explicit return type — use bidirectional check so that the
            // expected type flows into the last expression (e.g. anonymous
            // record literals in return position).
            let _ = self.check_block(&decl.body, expected_ret);
        } else {
            // No explicit return type - infer from body
            match self.synth_block(&decl.body) {
                Ok(body_ty) => {
                    let body_ty = self.zonk(&body_ty);
                    if contains_meta(&body_ty) {
                        // Return type contains unsolved MetaVars — the body holds a
                        // generic reference that was never called.  Reject it to
                        // prevent MetaVars from escaping into the TypeMap / lowered IR.
                        self.errors.push(TypeError::AmbiguousType {
                            name: format!("return type of `{}`", decl.name),
                            span: decl.body.span,
                            note: "return type cannot be inferred; add a type annotation or call the generic value".to_string(),
                        });
                    } else {
                        // Update the function signature with the inferred return type
                        let mut updated_sig = sig.clone();
                        updated_sig.ret = Some(body_ty);
                        self.value_env.update_function(updated_sig);
                    }
                }
                Err(()) => {
                    // Type checking failed, can't infer return type
                }
            }
        }

        // Zonk all TypeMap entries for this function, then clear per-function state
        let meta_subst = std::mem::take(&mut self.meta_subst);
        self.type_map.zonk(&meta_subst);

        // Clean up
        self.current_function_ret = None;
        self.local_env.pop_scope();
        self.type_var_scope = saved_type_vars;
        self.at_module_scope = saved_module_scope;
    }

    //
    // Unification engine — MetaVar management
    //

    /// Allocate a fresh MetaVar id.
    fn fresh_meta(&mut self) -> MonoType {
        let id = self.next_meta;
        self.next_meta += 1;
        MonoType::MetaVar(id)
    }

    /// Replace each named type parameter with a fresh MetaVar.
    /// Instantiate type-parameter variables with fresh MetaVars.
    ///
    /// Returns `(instantiated_type, var_to_meta)` where `var_to_meta` maps each
    /// type parameter name to the MetaVar created for it. After unification
    /// solves the MetaVars, callers can zonk these to get concrete type args.
    fn instantiate_vars(
        &mut self,
        type_params: &[String],
        ty: &MonoType,
    ) -> (MonoType, Vec<(String, MonoType)>) {
        if type_params.is_empty() {
            return (ty.clone(), vec![]);
        }
        let var_to_meta: HashMap<String, MonoType> = type_params
            .iter()
            .map(|p| (p.clone(), self.fresh_meta()))
            .collect();
        let ordered: Vec<(String, MonoType)> = type_params
            .iter()
            .map(|p| (p.clone(), var_to_meta[p].clone()))
            .collect();
        (apply_subst(ty, &var_to_meta), ordered)
    }

    /// Occurs check: does MetaVar `id` appear in `ty` (following chains)?
    fn occurs(&self, id: u32, ty: &MonoType) -> bool {
        match ty {
            MonoType::MetaVar(other_id) => {
                if *other_id == id {
                    return true;
                }
                if let Some(resolved) = self.meta_subst.get(other_id) {
                    let resolved = resolved.clone();
                    self.occurs(id, &resolved)
                } else {
                    false
                }
            }
            MonoType::Vector(e) => self.occurs(id, e),
            MonoType::Dict(k, v) => self.occurs(id, k) || self.occurs(id, v),
            MonoType::Function { params, ret } => {
                params.iter().any(|p| self.occurs(id, p)) || self.occurs(id, ret)
            }
            MonoType::Named { args, .. } => args.iter().any(|a| self.occurs(id, a)),
            _ => false,
        }
    }

    /// Solve `MetaVar(id) = ty`, with occurs check to prevent infinite types.
    ///
    /// Note: In the current type system this check is unreachable at the source
    /// level because all lambda parameters require explicit type annotations,
    /// preventing self-application (`fn(f) { f(f) }`) from creating circular
    /// MetaVar constraints. The guard is kept as a safety net for when
    /// unannotated parameters are introduced in a future stage.
    fn solve_meta(&mut self, id: u32, ty: MonoType, span: Span) -> Result<(), ()> {
        let zonked = self.zonk(&ty);
        if let MonoType::MetaVar(other) = &zonked {
            if *other == id {
                return Ok(()); // trivial self-unification
            }
        }
        if self.occurs(id, &zonked) {
            self.errors.push(TypeError::OccursCheckFailed { span });
            return Err(());
        }
        self.meta_subst.insert(id, zonked);
        Ok(())
    }

    /// Apply the current meta substitution to a type.
    fn zonk(&self, ty: &MonoType) -> MonoType {
        zonk_ty(ty, &self.meta_subst)
    }

    /// Type-check a top-level statement that is not a let binding.
    /// Allows for-loops, expression statements, break, continue, and return.
    fn check_top_level_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(e) => {
                let _ = self.synth_expr(e);
            }
            Stmt::For {
                pattern,
                index_pattern,
                iter,
                body,
                ..
            } => {
                self.check_for_stmt(pattern, index_pattern.as_ref(), iter, body);
            }
            Stmt::ForCond { cond, body, .. } => {
                let _ = self.check_expr(cond, &MonoType::Bool);
                let saved_scope = std::mem::replace(&mut self.at_module_scope, false);
                let _ = self.synth_block(body);
                self.at_module_scope = saved_scope;
            }
            Stmt::Break { value, .. } => {
                if let Some(val) = value {
                    let _ = self.synth_expr(val);
                }
            }
            Stmt::Continue { .. } => {}
            Stmt::Return { value, span } => {
                if let Some(val) = value {
                    let _ = self.synth_expr(val);
                }
                // Return at top-level is technically invalid but we'll let the
                // lowerer handle it (it becomes part of __init__)
                let _ = span;
            }
            Stmt::Defer { expr, span } => {
                let deferred_ty = self.synth_expr(expr).unwrap_or(MonoType::Void);
                if deferred_ty == MonoType::Never {
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "never-typed expression in defer",
                        span: *span,
                        note: "A defer body cannot diverge (return, break, continue, or \
                               error(...)). These control-flow effects would be ambiguous \
                               when executed at scope exit."
                            .to_string(),
                    });
                }
            }
            Stmt::Let { .. } => {
                // Should not happen here; handled in Pass 1
            }
        }
    }

    fn synth_function_expr(
        &mut self,
        fe: &crate::syntax::ast::FunctionExpr,
        span: Span,
    ) -> Result<MonoType, ()> {
        // Resolve param types — annotations required for lambdas in Stage 5
        let mut param_types = Vec::new();
        for p in &fe.params {
            match &p.ty {
                Some(ann) => {
                    let t = self.resolve_type(ann)?;
                    param_types.push(t);
                }
                None => {
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "lambda with unannotated parameters",
                        span,
                        note: "Lambda parameters can be inferred only when a contextual function type is available; add parameter annotations here"
                            .to_string(),
                    });
                    return Err(());
                }
            }
        }

        let expected_ret = match &fe.return_type {
            Some(t) => Some(self.resolve_type(t)?),
            None => None,
        };

        self.local_env.push_scope();
        for (p, ty) in fe.params.iter().zip(&param_types) {
            self.local_env.bind(p.name.clone(), ty.clone());
        }
        let saved = self.current_function_ret.take();
        self.current_function_ret = expected_ret.clone();
        let saved_scope = std::mem::replace(&mut self.at_module_scope, false);

        let body_ty = match &expected_ret {
            Some(exp) => {
                self.check_expr(&fe.body, exp)?;
                exp.clone()
            }
            None => self.synth_expr(&fe.body)?,
        };

        self.local_env.pop_scope();
        self.current_function_ret = saved;
        self.at_module_scope = saved_scope;

        Ok(MonoType::Function {
            params: param_types,
            ret: Box::new(body_ty),
        })
    }

    /// Resolve an AST type annotation to a MonoType
    /// Delegates to TypeEnv's shared implementation
    fn resolve_type(&mut self, ty: &AstType) -> Result<MonoType, ()> {
        // Check type var scope first
        if let AstType::Named { name, args, .. } = ty {
            if args.is_empty() && self.type_var_scope.contains(name) {
                return Ok(MonoType::Var(name.clone()));
            }
        }
        self.type_env.resolve_type(ty, &mut self.errors)
    }

    //
    // Synthesis mode: infer type of expression
    //

    fn synth_expr(&mut self, expr: &Expr) -> Result<MonoType, ()> {
        let ty = self.synth_expr_inner(expr)?;
        // Record the type in the TypeMap
        self.type_map.set_expr_type(expr.id, ty.clone());
        Ok(ty)
    }

    fn synth_expr_inner(&mut self, expr: &Expr) -> Result<MonoType, ()> {
        match &expr.kind {
            ExprKind::Literal(lit) => Ok(self.synth_literal(lit)),

            ExprKind::Ident(name) => {
                // Local env first (function parameters, let bindings)
                if let Some(ty) = self.local_env.lookup(name) {
                    return Ok(ty.clone());
                }
                // Generic function in value env: instantiate type params with MetaVars
                if let Some(sig) = self.value_env.get_function(name).cloned() {
                    let fn_ty = MonoType::Function {
                        params: sig.params.clone(),
                        ret: Box::new(sig.ret.clone().unwrap_or(MonoType::Void)),
                    };
                    let (inst_ty, _var_to_meta) = self.instantiate_vars(&sig.type_params, &fn_ty);
                    // Don't record instantiation here — MetaVars aren't solved yet.
                    // They'll be recorded at the call site after unification.
                    return Ok(inst_ty);
                }
                if !self.allow_internal_host_builtins
                    && self.value_env.is_visible_internal_host_builtin(name)
                {
                    self.errors.push(TypeError::UndefinedVariable {
                        name: name.clone(),
                        span: expr.span,
                    });
                    return Err(());
                }
                // Non-function values (top-level lets, builtins)
                if let Some(ty) = self.value_env.lookup(name) {
                    return Ok(ty);
                }
                self.errors.push(TypeError::UndefinedVariable {
                    name: name.clone(),
                    span: expr.span,
                });
                Err(())
            }

            ExprKind::Binary { op, left, right } => self.synth_binary(*op, left, right, expr.span),

            ExprKind::Unary { op, expr: inner } => self.synth_unary(*op, inner, expr.span),

            ExprKind::Call { callee, args } => self.synth_call(callee, args, expr.span),

            ExprKind::Block(block) => self.synth_block(block),

            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.synth_if(cond, then_branch, else_branch.as_deref(), expr.span),

            ExprKind::FieldAccess { base, field } => {
                if let ExprKind::Ident(alias) = &base.kind {
                    if self.module_aliases.contains(alias.as_str()) {
                        let qualified = format!("{}.{}", alias, field);
                        if let Some(ty) = self.value_env.lookup(&qualified) {
                            // Plain pub value or monomorphic function: synthesize directly
                            if !matches!(ty, MonoType::Function { .. }) {
                                self.type_map.set_expr_type(expr.id, ty.clone());
                                return Ok(ty);
                            }
                            // Monomorphic function: can infer without annotation
                            if let Some(sig) = self.value_env.get_function(&qualified) {
                                if sig.type_params.is_empty() {
                                    let fn_ty = ty.clone();
                                    self.type_map.set_expr_type(expr.id, fn_ty.clone());
                                    return Ok(fn_ty);
                                }
                            }
                        }
                        // Polymorphic function ref: require a type annotation
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "module method reference without type annotation",
                            span: expr.span,
                            note: format!(
                                "'{}.{}' as a value requires a type annotation, \
                                 e.g. `f : fn(...) ... = {}.{}`",
                                alias, field, alias, field
                            ),
                        });
                        return Err(());
                    }
                }
                self.synth_field_access(base, field, expr.span)
            }

            ExprKind::Index { base, index } => self.synth_index(base, index, expr.span),

            ExprKind::Array { elements } => self.synth_array(elements, expr.span),

            ExprKind::RecordLit { name, fields } => {
                self.synth_record_lit(name.as_deref(), fields, expr.span)
            }

            ExprKind::VariantLit { name, fields } => {
                self.synth_variant_lit(name, fields, expr.span)
            }

            ExprKind::Case { scrutinee, arms } => self.synth_case(scrutinee, arms, expr.span),

            ExprKind::StringInterpolation { parts } => {
                for part in parts {
                    if let StringPart::Interpolation(e) = part {
                        self.check_interpolation_expr(e)?;
                    }
                }
                Ok(MonoType::String)
            }

            ExprKind::Function(fe) => self.synth_function_expr(fe, expr.span),

            ExprKind::Collect {
                pattern,
                index_pattern,
                iter,
                body,
            } => self.synth_collect(pattern, index_pattern.as_ref(), iter, body, expr.span),

            ExprKind::CollectWhile { cond, body } => {
                self.synth_collect_while(cond, body, expr.span)
            }

            ExprKind::Try { expr: inner } => {
                let inner_ty = self.synth_expr(inner)?;
                match &inner_ty {
                    MonoType::Named { type_id, args } if *type_id == RESULT_TYPE_ID => {
                        // try Result<T,E> → extracts T; propagates Err(E) via early return
                        Ok(args.first().cloned().unwrap_or(MonoType::Void))
                    }
                    _ => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Named {
                                type_id: RESULT_TYPE_ID,
                                args: vec![MonoType::Void, MonoType::Void],
                            },
                            actual: inner_ty,
                            span: expr.span,
                            note: None,
                        });
                        Err(())
                    }
                }
            }
        }
    }

    //
    // Checking mode: validate expression against expected type
    //

    fn check_expr(&mut self, expr: &Expr, expected: &MonoType) -> Result<(), ()> {
        let result = match &expr.kind {
            // Anonymous record literals REQUIRE checking mode
            ExprKind::RecordLit { name: None, fields } => {
                self.check_anon_record_lit(fields, expected, expr.span)
            }

            // Variant literals can be checked against expected sum type
            ExprKind::VariantLit { name, fields } => {
                self.check_variant_lit(name, fields, expected, expr.span)
            }

            // Blocks: thread expected type into the last expression
            ExprKind::Block(block) => self.check_block(block, expected),

            // If expressions: check both branches against expected type
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check_expr(cond, &MonoType::Bool)?;
                self.check_expr(then_branch, expected)?;
                if let Some(else_b) = else_branch {
                    self.check_expr(else_b, expected)?;
                } else {
                    let _ = self.unify(&MonoType::Void, expected, expr.span);
                }
                Ok(())
            }

            // Case: check each arm body against expected type
            ExprKind::Case { scrutinee, arms } => {
                let scrut_ty = self.synth_expr(scrutinee)?;
                let is_primitive_match =
                    matches!(scrut_ty, MonoType::Int | MonoType::Bool | MonoType::String);
                if !is_primitive_match && !scrut_ty.is_sum(&self.type_env) {
                    self.errors.push(TypeError::CaseScrutineeNotSumType {
                        actual_type: scrut_ty.clone(),
                        span: scrutinee.span,
                    });
                    return Err(());
                }
                if arms.is_empty() {
                    self.errors.push(TypeError::NonExhaustiveMatch {
                        missing: vec!["(all patterns)".to_string()],
                        span: expr.span,
                    });
                    return Err(());
                }
                PatternChecker::check_exhaustiveness(
                    &self.type_env,
                    &mut self.errors,
                    &scrut_ty,
                    arms,
                    expr.span,
                )?;
                for arm in arms {
                    self.local_env.push_scope();
                    let mut pc =
                        PatternChecker::new(&self.type_env, &mut self.local_env, &mut self.errors);
                    pc.check_pattern(&arm.pattern, &scrut_ty)?;
                    drop(pc);
                    self.check_expr(&arm.body, expected)?;
                    self.local_env.pop_scope();
                }
                Ok(())
            }

            // Lambda: use expected Function type to supply unannotated param types
            ExprKind::Function(fe) => {
                if let MonoType::Function {
                    params: expected_params,
                    ret: expected_ret,
                } = expected
                {
                    if fe.params.len() != expected_params.len() {
                        self.errors.push(TypeError::WrongArity {
                            expected: expected_params.len(),
                            actual: fe.params.len(),
                            span: expr.span,
                        });
                        return Err(());
                    }
                    let mut param_types = Vec::new();
                    for (p, exp_ty) in fe.params.iter().zip(expected_params.iter()) {
                        match &p.ty {
                            Some(ann) => {
                                let ann_ty = self.resolve_type(ann)?;
                                self.unify(&ann_ty, exp_ty, p.span)?;
                                param_types.push(self.zonk(exp_ty));
                            }
                            None => param_types.push(exp_ty.clone()),
                        }
                    }
                    let ret_ty = match &fe.return_type {
                        Some(ann) => {
                            let ann_ret = self.resolve_type(ann)?;
                            self.unify(&ann_ret, expected_ret.as_ref(), ann.span())?;
                            self.zonk(expected_ret.as_ref())
                        }
                        None => expected_ret.as_ref().clone(),
                    };
                    self.local_env.push_scope();
                    for (p, ty) in fe.params.iter().zip(&param_types) {
                        self.local_env.bind(p.name.clone(), ty.clone());
                    }
                    let saved = self.current_function_ret.take();
                    self.current_function_ret = Some(ret_ty.clone());
                    let saved_scope = std::mem::replace(&mut self.at_module_scope, false);
                    let result = self.check_expr(&fe.body, &ret_ty);
                    self.local_env.pop_scope();
                    self.current_function_ret = saved;
                    self.at_module_scope = saved_scope;
                    result
                } else {
                    let actual = self.synth_expr(expr)?;
                    self.unify(&actual, expected, expr.span)
                }
            }

            // Vector literals: check each element against the expected element type
            ExprKind::Array { elements } => {
                if let MonoType::Vector(elem_ty) = expected {
                    let elem_ty = *elem_ty.clone();
                    for elem in elements {
                        self.check_expr(elem, &elem_ty)?;
                    }
                    Ok(())
                } else {
                    let actual = self.synth_array(elements, expr.span)?;
                    self.unify(&actual, expected, expr.span)
                }
            }

            // Dict.new() — type comes entirely from context annotation
            ExprKind::Call { callee, args } if args.is_empty() => {
                if let ExprKind::FieldAccess { base, field } = &callee.kind {
                    if let ExprKind::Ident(alias) = &base.kind {
                        if alias == "Dict" && field == "new" {
                            if let MonoType::Dict(_, _) = expected {
                                self.type_map.set_expr_type(expr.id, expected.clone());
                                self.type_map.set_expr_type(callee.id, expected.clone());
                                self.type_map.set_expr_type(base.id, expected.clone());
                                return Ok(());
                            }
                        }
                    }
                }
                let actual = self.synth_expr(expr)?;
                self.unify(&actual, expected, expr.span)
            }

            // First-class module method reference: Vector.len, String.concat, etc.
            ExprKind::FieldAccess { base, field } => {
                if let ExprKind::Ident(alias) = &base.kind {
                    if self.module_aliases.contains(alias.as_str()) {
                        let alias = alias.clone();
                        let field = field.clone();
                        return self
                            .check_module_func_ref(&alias, &field, expected, expr.id, expr.span);
                    }
                }
                let actual = self.synth_expr(expr)?;
                self.unify(&actual, expected, expr.span)
            }

            // For most expressions, synthesize and unify
            _ => {
                let actual = self.synth_expr(expr)?;
                self.unify(&actual, expected, expr.span)
            }
        };

        // Record the expected type in the TypeMap if checking succeeded
        if result.is_ok() {
            self.type_map.set_expr_type(expr.id, expected.clone());
        }

        result
    }

    //
    // Literal synthesis
    //

    fn synth_literal(&self, lit: &Literal) -> MonoType {
        match lit {
            Literal::Int(_) => MonoType::Int,
            Literal::Float(_) => MonoType::Float,
            Literal::Bool(_) => MonoType::Bool,
            Literal::String(_) => MonoType::String,
        }
    }

    //
    // Binary operators
    //

    fn synth_binary(
        &mut self,
        op: BinOp,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Result<MonoType, ()> {
        match op {
            // Arithmetic: Int × Int → Int, Float × Float → Float
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                let left_raw = self.synth_expr(left)?;
                let right_raw = self.synth_expr(right)?;
                let left_ty = self.zonk(&left_raw);
                let right_ty = self.zonk(&right_raw);

                match (&left_ty, &right_ty) {
                    (MonoType::Int, MonoType::Int) => Ok(MonoType::Int),
                    (MonoType::Float, MonoType::Float) => Ok(MonoType::Float),
                    // Allow numeric constraints to solve metas during inference
                    (MonoType::MetaVar(id), MonoType::Int) => {
                        self.solve_meta(*id, MonoType::Int, left.span)?;
                        Ok(MonoType::Int)
                    }
                    (MonoType::Int, MonoType::MetaVar(id)) => {
                        self.solve_meta(*id, MonoType::Int, right.span)?;
                        Ok(MonoType::Int)
                    }
                    (MonoType::MetaVar(id), MonoType::Float) => {
                        self.solve_meta(*id, MonoType::Float, left.span)?;
                        Ok(MonoType::Float)
                    }
                    (MonoType::Float, MonoType::MetaVar(id)) => {
                        self.solve_meta(*id, MonoType::Float, right.span)?;
                        Ok(MonoType::Float)
                    }
                    _ => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: left_ty.clone(),
                            actual: right_ty,
                            span: right.span,
                            note: None,
                        });
                        Err(())
                    }
                }
            }

            // Comparison: T × T → Bool (for primitive types)
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let left_ty = self.synth_expr(left)?;
                let right_ty = self.synth_expr(right)?;

                self.unify(&left_ty, &right_ty, right.span)?;
                Ok(MonoType::Bool)
            }

            // Logical: Bool × Bool → Bool
            BinOp::And | BinOp::Or => {
                self.check_expr(left, &MonoType::Bool)?;
                self.check_expr(right, &MonoType::Bool)?;
                Ok(MonoType::Bool)
            }

            // Assignment / rebinding operators
            BinOp::Assign => self.synth_assign(left, right, span),
        }
    }

    //
    // Unary operators
    //

    fn synth_unary(&mut self, op: UnOp, expr: &Expr, _span: Span) -> Result<MonoType, ()> {
        match op {
            UnOp::Neg => {
                let ty = self.synth_expr(expr)?;
                match &ty {
                    MonoType::Int => Ok(MonoType::Int),
                    MonoType::Float => Ok(MonoType::Float),
                    _ => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Int,
                            actual: ty,
                            span: expr.span,
                            note: None,
                        });
                        Err(())
                    }
                }
            }
            UnOp::Not => {
                self.check_expr(expr, &MonoType::Bool)?;
                Ok(MonoType::Bool)
            }
        }
    }

    //
    // Function calls
    //

    fn synth_call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> Result<MonoType, ()> {
        // Special case: field-access calls — handles both module-qualified
        // calls (module.func(args)) and method calls (receiver.method(args)).
        if let ExprKind::FieldAccess {
            base,
            field: method_name,
        } = &callee.kind
        {
            // Check for module-qualified call FIRST (before synthesising base type)
            if let ExprKind::Ident(alias) = &base.kind {
                if self.module_aliases.contains(alias.as_str()) {
                    let alias = alias.clone();
                    let method_name = method_name.clone();
                    let callee_id = callee.id;
                    return self.synth_module_call(&alias, &method_name, args, span, callee_id);
                }

                // TypeName.Variant(args) — variant construction with type prefix
                if let Some(type_id) = self.type_env.lookup_type(alias) {
                    if let Some(variant_idx) = self.type_env.get_variant_index(type_id, method_name)
                    {
                        // Build named_ty with Var args for generic types
                        let type_var_args: Vec<MonoType> = self
                            .type_env
                            .get_def(type_id)
                            .map(|d| {
                                d.type_params()
                                    .iter()
                                    .map(|p| MonoType::Var(p.clone()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let named_ty = MonoType::Named {
                            type_id,
                            args: type_var_args,
                        };
                        self.type_map.set_expr_type(base.id, named_ty.clone());
                        let variants = self
                            .type_env
                            .get_variants(type_id)
                            .expect("variant exists, variants must exist");
                        let variant_fields = variants[variant_idx].fields.clone();
                        // Check arity
                        if variant_fields.len() != args.len() {
                            self.errors.push(TypeError::WrongArity {
                                expected: variant_fields.len(),
                                actual: args.len(),
                                span,
                            });
                            return Err(());
                        }
                        // Instantiate type params with MetaVars (no-op for non-generic types)
                        let type_params: Vec<String> = self
                            .type_env
                            .get_def(type_id)
                            .map(|d| d.type_params().to_vec())
                            .unwrap_or_default();
                        let inst_map: HashMap<String, MonoType> = type_params
                            .iter()
                            .map(|p| (p.clone(), self.fresh_meta()))
                            .collect();
                        let inst_fields: Vec<MonoType> = variant_fields
                            .iter()
                            .map(|f| apply_subst(f, &inst_map))
                            .collect();
                        let inst_named_ty = apply_subst(&named_ty, &inst_map);
                        // Check each arg against instantiated field type
                        for (arg, expected_ty) in args.iter().zip(inst_fields.iter()) {
                            if let Err(()) = self.check_expr(arg, expected_ty) {
                                return Err(());
                            }
                        }
                        // Record callee type (constructor function) and return
                        let ctor_ty = if inst_fields.is_empty() {
                            inst_named_ty.clone()
                        } else {
                            MonoType::Function {
                                params: inst_fields,
                                ret: Box::new(inst_named_ty.clone()),
                            }
                        };
                        self.type_map.set_expr_type(callee.id, self.zonk(&ctor_ty));
                        return Ok(self.zonk(&inst_named_ty));
                    }
                }
            }

            // Method call on a value: synthesise base type, then dispatch
            let base_ty = self.synth_expr(base)?;
            let method_name = method_name.clone();
            let callee_id = callee.id;
            return self.synth_method_call(base, base_ty, &method_name, args, span, callee_id);
        }

        // Normal function call
        let callee_ty = self.synth_expr(callee)?;

        match callee_ty {
            MonoType::Function { params, ret } => {
                // Check arity
                if params.len() != args.len() {
                    self.errors.push(TypeError::WrongArity {
                        expected: params.len(),
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }

                // Check each argument; MetaVar params are solved by unify inside check_expr.
                // On failure, patch in call context for better error messages.
                for (idx, (arg, expected_ty)) in args.iter().zip(params.iter()).enumerate() {
                    if let Err(()) = self.check_expr(arg, expected_ty) {
                        if let Some(TypeError::TypeMismatch { note, .. }) = self.errors.last_mut() {
                            if note.is_none() {
                                let label = if let ExprKind::Ident(n) = &callee.kind {
                                    format!("argument {} of call to `{}`", idx + 1, n)
                                } else {
                                    format!("argument {} of call", idx + 1)
                                };
                                *note = Some(label);
                            }
                        }
                        return Err(());
                    }
                }

                Ok(self.zonk(&*ret))
            }
            _ => {
                self.errors.push(TypeError::NotAFunction {
                    ty: callee_ty,
                    span: callee.span,
                });
                Err(())
            }
        }
    }

    /// Handle module-qualified calls: `module.func(args)`.
    fn synth_module_call(
        &mut self,
        alias: &str,
        func_name: &str,
        args: &[Expr],
        span: Span,
        _callee_id: ExprId,
    ) -> Result<MonoType, ()> {
        // Special: Cell, Dict, Iterator, Vector, and String modules provide polymorphic operations.
        if alias == "Cell" {
            return self.synth_cell_call(func_name, args, span);
        }
        if alias == "Dict" {
            return self.synth_dict_module_call(func_name, args, span);
        }
        if alias == "Iterator" {
            return self.synth_iterator_call(func_name, args, span);
        }
        if alias == "Vector" {
            return self.synth_vector_call(func_name, args, span);
        }
        if alias == "String" {
            return self.synth_string_call(func_name, args, span);
        }
        if alias == "Byte" {
            return self.synth_byte_call(func_name, args, span);
        }

        self.synth_qualified_call(alias, func_name, args, span)
    }

    fn synth_qualified_call(
        &mut self,
        alias: &str,
        func_name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<MonoType, ()> {
        let qualified = format!("{}.{}", alias, func_name);
        // Look up full function signature for proper MetaVar instantiation of generics.
        if let Some(sig) = self.value_env.get_function(&qualified).cloned() {
            if sig.params.len() != args.len() {
                self.errors.push(TypeError::WrongArity {
                    expected: sig.params.len(),
                    actual: args.len(),
                    span,
                });
                return Err(());
            }
            let fn_ty = MonoType::Function {
                params: sig.params.clone(),
                ret: Box::new(sig.ret.clone().unwrap_or(MonoType::Void)),
            };
            let (inst_ty, _var_to_meta) = self.instantiate_vars(&sig.type_params, &fn_ty);
            let (inst_params, inst_ret) = match inst_ty {
                MonoType::Function { params, ret } => (params, *ret),
                _ => unreachable!(),
            };
            for (arg, expected_ty) in args.iter().zip(inst_params.iter()) {
                self.check_expr(arg, expected_ty)?;
            }
            return Ok(self.zonk(&inst_ret));
        }
        // Not a function or undefined
        match self.value_env.lookup(&qualified) {
            Some(ty) => {
                self.errors.push(TypeError::NotAFunction { ty, span });
                Err(())
            }
            None => {
                self.errors.push(TypeError::UndefinedVariable {
                    name: qualified,
                    span,
                });
                Err(())
            }
        }
    }

    /// Handle Cell.new / Cell.get / Cell.set / Cell.update polymorphically.
    fn synth_cell_call(
        &mut self,
        func_name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<MonoType, ()> {
        match func_name {
            "new" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 1,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                let inner = self.synth_expr(&args[0])?;
                Ok(MonoType::Named {
                    type_id: CELL_TYPE_ID,
                    args: vec![inner],
                })
            }
            "get" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 1,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                let cell_ty = self.synth_expr(&args[0])?;
                match cell_ty {
                    MonoType::Named {
                        type_id,
                        args: cell_args,
                    } if type_id == CELL_TYPE_ID => {
                        Ok(cell_args.into_iter().next().unwrap_or(MonoType::Void))
                    }
                    other => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Named {
                                type_id: CELL_TYPE_ID,
                                args: vec![],
                            },
                            actual: other,
                            span,
                            note: None,
                        });
                        Err(())
                    }
                }
            }
            "set" => {
                if args.len() != 2 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 2,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                let cell_ty = self.synth_expr(&args[0])?;
                match cell_ty {
                    MonoType::Named {
                        type_id,
                        args: cell_args,
                    } if type_id == CELL_TYPE_ID => {
                        let inner = cell_args.into_iter().next().unwrap_or(MonoType::Void);
                        self.check_expr(&args[1], &inner)?;
                        Ok(MonoType::Void)
                    }
                    other => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Named {
                                type_id: CELL_TYPE_ID,
                                args: vec![],
                            },
                            actual: other,
                            span,
                            note: None,
                        });
                        Err(())
                    }
                }
            }
            "update" => {
                if args.len() != 2 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 2,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                let cell_ty = self.synth_expr(&args[0])?;
                match cell_ty {
                    MonoType::Named {
                        type_id,
                        args: cell_args,
                    } if type_id == CELL_TYPE_ID => {
                        let inner = cell_args.into_iter().next().unwrap_or(MonoType::Void);
                        let expected_fn = MonoType::Function {
                            params: vec![inner.clone()],
                            ret: Box::new(inner),
                        };
                        self.check_expr(&args[1], &expected_fn)?;
                        Ok(MonoType::Void)
                    }
                    other => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Named {
                                type_id: CELL_TYPE_ID,
                                args: vec![],
                            },
                            actual: other,
                            span,
                            note: None,
                        });
                        Err(())
                    }
                }
            }
            _ => {
                self.errors.push(TypeError::UndefinedVariable {
                    name: format!("Cell.{}", func_name),
                    span,
                });
                Err(())
            }
        }
    }

    /// Handle Dict module-qualified calls.
    /// Dict.new() requires annotation context; other methods synthesize from first arg.
    fn synth_dict_module_call(
        &mut self,
        func_name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<MonoType, ()> {
        match func_name {
            "new" => {
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "Dict.new() without type annotation",
                    span,
                    note: "Dict.new() requires a type annotation, e.g. `m: Dict<String, Int> = Dict.new()`".to_string(),
                });
                Err(())
            }
            "len" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 1,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                let dict_ty = self.synth_expr(&args[0])?;
                if !matches!(dict_ty, MonoType::Dict(_, _)) {
                    self.errors.push(TypeError::TypeMismatch {
                        expected: MonoType::Dict(
                            Box::new(MonoType::Void),
                            Box::new(MonoType::Void),
                        ),
                        actual: dict_ty,
                        span,
                        note: Some("Dict.len expects Dict<K,V> as first argument".to_string()),
                    });
                    return Err(());
                }
                Ok(MonoType::Int)
            }
            "has" => {
                if args.len() != 2 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 2,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                let dict_ty = self.synth_expr(&args[0])?;
                match dict_ty {
                    MonoType::Dict(ref k_ty, _) => {
                        let k_ty = *k_ty.clone();
                        self.check_expr(&args[1], &k_ty)?;
                        Ok(MonoType::Bool)
                    }
                    other => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Dict(
                                Box::new(MonoType::Void),
                                Box::new(MonoType::Void),
                            ),
                            actual: other,
                            span,
                            note: Some("Dict.has expects Dict<K,V> as first argument".to_string()),
                        });
                        Err(())
                    }
                }
            }
            "keys" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 1,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                let dict_ty = self.synth_expr(&args[0])?;
                match dict_ty {
                    MonoType::Dict(k_ty, _) => Ok(MonoType::Vector(k_ty)),
                    other => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Dict(
                                Box::new(MonoType::Void),
                                Box::new(MonoType::Void),
                            ),
                            actual: other,
                            span,
                            note: Some("Dict.keys expects Dict<K,V> as first argument".to_string()),
                        });
                        Err(())
                    }
                }
            }
            "remove" => {
                if args.len() != 2 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 2,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                let dict_ty = self.synth_expr(&args[0])?;
                match dict_ty.clone() {
                    MonoType::Dict(ref k_ty, _) => {
                        let k_ty = *k_ty.clone();
                        self.check_expr(&args[1], &k_ty)?;
                        Ok(dict_ty)
                    }
                    other => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Dict(
                                Box::new(MonoType::Void),
                                Box::new(MonoType::Void),
                            ),
                            actual: other,
                            span,
                            note: Some(
                                "Dict.remove expects Dict<K,V> as first argument".to_string(),
                            ),
                        });
                        Err(())
                    }
                }
            }
            other => self.synth_qualified_call("Dict", other, args, span),
        }
    }

    /// Handle Iterator.next / Iterator.unfold polymorphically.
    fn synth_iterator_call(
        &mut self,
        func_name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<MonoType, ()> {
        match func_name {
            "next" => {
                // Iterator.next(it: Iterator<T>) Option<IterItem<T>>
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 1,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                let it_ty = self.synth_expr(&args[0])?;
                match it_ty {
                    MonoType::Named {
                        type_id,
                        args: ref it_args,
                    } if type_id == ITERATOR_TYPE_ID => {
                        let elem_ty = it_args.first().cloned().unwrap_or(MonoType::Void);
                        let item_ty = MonoType::Named {
                            type_id: ITER_ITEM_TYPE_ID,
                            args: vec![elem_ty],
                        };
                        Ok(MonoType::Named {
                            type_id: OPTION_TYPE_ID,
                            args: vec![item_ty],
                        })
                    }
                    other => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Named {
                                type_id: ITERATOR_TYPE_ID,
                                args: vec![],
                            },
                            actual: other,
                            span,
                            note: Some("Iterator.next expects an Iterator<T>".to_string()),
                        });
                        Err(())
                    }
                }
            }
            "unfold" => {
                // Iterator.unfold(seed: S, step: fn(S) UnfoldStep<T,S>) Iterator<T>
                // We infer T and S from the step function's signature.
                if args.len() != 2 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 2,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                // Synthesize seed to determine S
                let seed_ty = self.synth_expr(&args[0])?;
                // Synthesize the step closure
                let step_ty = self.synth_expr(&args[1])?;
                // Validate: step must be fn(S) UnfoldStep<T,S>
                match step_ty {
                    MonoType::Function {
                        ref params,
                        ref ret,
                    } => {
                        if params.len() != 1 {
                            self.errors.push(TypeError::WrongArity {
                                expected: 1,
                                actual: params.len(),
                                span,
                            });
                            return Err(());
                        }
                        // Check parameter matches seed type
                        let param_ty = &params[0];
                        if param_ty != &seed_ty {
                            self.errors.push(TypeError::TypeMismatch {
                                expected: seed_ty.clone(),
                                actual: param_ty.clone(),
                                span,
                                note: Some("Iterator.unfold: step function parameter type must match seed type".to_string()),
                            });
                            return Err(());
                        }
                        // Return type should be UnfoldStep<T,S>; extract T
                        match ret.as_ref() {
                            MonoType::Named {
                                type_id,
                                args: ret_args,
                            } if *type_id == UNFOLD_STEP_TYPE_ID => {
                                let elem_ty = ret_args.first().cloned().unwrap_or(MonoType::Void);
                                Ok(MonoType::Named {
                                    type_id: ITERATOR_TYPE_ID,
                                    args: vec![elem_ty],
                                })
                            }
                            other => {
                                self.errors.push(TypeError::TypeMismatch {
                                    expected: MonoType::Named { type_id: UNFOLD_STEP_TYPE_ID, args: vec![] },
                                    actual: other.clone(),
                                    span,
                                    note: Some("Iterator.unfold: step function must return UnfoldStep<T,S>".to_string()),
                                });
                                Err(())
                            }
                        }
                    }
                    other => {
                        self.errors
                            .push(TypeError::NotAFunction { ty: other, span });
                        Err(())
                    }
                }
            }
            other => {
                self.errors.push(TypeError::UndefinedVariable {
                    name: format!("Iterator.{}", other),
                    span,
                });
                Err(())
            }
        }
    }

    /// Handle Vector.method(vec, ...) module-qualified calls.
    fn synth_vector_call(
        &mut self,
        func_name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<MonoType, ()> {
        match func_name {
            "len" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 1,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                let vec_ty = self.synth_expr(&args[0])?;
                if !matches!(vec_ty, MonoType::Vector(_)) {
                    self.errors.push(TypeError::TypeMismatch {
                        expected: MonoType::Vector(Box::new(MonoType::Void)),
                        actual: vec_ty,
                        span,
                        note: Some("Vector.len expects Vector<T> as first argument".to_string()),
                    });
                    return Err(());
                }
                Ok(MonoType::Int)
            }
            "concat" => {
                if args.len() != 2 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 2,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                let vec_ty = self.synth_expr(&args[0])?;
                if !matches!(vec_ty, MonoType::Vector(_)) {
                    self.errors.push(TypeError::TypeMismatch {
                        expected: MonoType::Vector(Box::new(MonoType::Void)),
                        actual: vec_ty,
                        span,
                        note: Some("Vector.concat expects Vector<T> as first argument".to_string()),
                    });
                    return Err(());
                }
                self.check_expr(&args[1], &vec_ty)?;
                Ok(vec_ty)
            }
            "slice" => {
                if args.len() != 3 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 3,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                let vec_ty = self.synth_expr(&args[0])?;
                if !matches!(vec_ty, MonoType::Vector(_)) {
                    self.errors.push(TypeError::TypeMismatch {
                        expected: MonoType::Vector(Box::new(MonoType::Void)),
                        actual: vec_ty,
                        span,
                        note: Some("Vector.slice expects Vector<T> as first argument".to_string()),
                    });
                    return Err(());
                }
                self.check_expr(&args[1], &MonoType::Int)?;
                self.check_expr(&args[2], &MonoType::Int)?;
                Ok(vec_ty)
            }
            "make" => {
                // Vector.make(size: Int, fill: T) Vector<T>
                if args.len() != 2 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 2,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                self.check_expr(&args[0], &MonoType::Int)?;
                let elem_ty = self.synth_expr(&args[1])?;
                Ok(MonoType::Vector(Box::new(elem_ty)))
            }
            other => self.synth_qualified_call("Vector", other, args, span),
        }
    }

    /// Handle String.method(s, ...) module-qualified calls.
    fn synth_string_call(
        &mut self,
        func_name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<MonoType, ()> {
        match func_name {
            "len" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 1,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                self.check_expr(&args[0], &MonoType::String)?;
                Ok(MonoType::Int)
            }
            "concat" => {
                if args.len() != 2 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 2,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                self.check_expr(&args[0], &MonoType::String)?;
                self.check_expr(&args[1], &MonoType::String)?;
                Ok(MonoType::String)
            }
            "slice" => {
                if args.len() != 3 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 3,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                self.check_expr(&args[0], &MonoType::String)?;
                self.check_expr(&args[1], &MonoType::Int)?;
                self.check_expr(&args[2], &MonoType::Int)?;
                Ok(MonoType::String)
            }
            "get" => {
                if args.len() != 2 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 2,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                self.check_expr(&args[0], &MonoType::String)?;
                self.check_expr(&args[1], &MonoType::Int)?;
                Ok(MonoType::Named {
                    type_id: crate::types::ty::OPTION_TYPE_ID,
                    args: vec![MonoType::Byte],
                })
            }
            "to_string" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 1,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                self.check_expr(&args[0], &MonoType::String)?;
                Ok(MonoType::String)
            }
            other => self.synth_qualified_call("String", other, args, span),
        }
    }

    /// Handle Byte.method(...) module-qualified calls.
    fn synth_byte_call(
        &mut self,
        func_name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<MonoType, ()> {
        match func_name {
            "to_int" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 1,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                self.check_expr(&args[0], &MonoType::Byte)?;
                Ok(MonoType::Int)
            }
            "from_int" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 1,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                self.check_expr(&args[0], &MonoType::Int)?;
                Ok(MonoType::Named {
                    type_id: crate::types::ty::OPTION_TYPE_ID,
                    args: vec![MonoType::Byte],
                })
            }
            "to_string" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity {
                        expected: 1,
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                self.check_expr(&args[0], &MonoType::Byte)?;
                Ok(MonoType::String)
            }
            other => self.synth_qualified_call("Byte", other, args, span),
        }
    }

    fn check_interpolation_expr(&mut self, expr: &Expr) -> Result<(), ()> {
        let expr_ty = self.synth_expr(expr)?;
        let expr_ty = self.zonk(&expr_ty);
        self.validate_interpolation_to_string(expr, &expr_ty)
    }

    fn validate_interpolation_to_string(
        &mut self,
        expr: &Expr,
        expr_ty: &MonoType,
    ) -> Result<(), ()> {
        match expr_ty {
            MonoType::Int
            | MonoType::Float
            | MonoType::Bool
            | MonoType::String
            | MonoType::Byte => Ok(()),
            MonoType::Named { type_id, .. } => {
                if let Some(func_name) = self
                    .type_env
                    .get_method_function(*type_id, "to_string")
                    .cloned()
                {
                    if let Some(sig) = self.value_env.get_function(&func_name).cloned() {
                        let full_fn_ty = MonoType::Function {
                            params: sig.params.clone(),
                            ret: Box::new(sig.ret.clone().unwrap_or(MonoType::Void)),
                        };
                        let (inst_ty, _) = self.instantiate_vars(&sig.type_params, &full_fn_ty);
                        let (inst_params, inst_ret) = match inst_ty {
                            MonoType::Function { params, ret } => (params, *ret),
                            _ => unreachable!(),
                        };

                        if inst_params.len() != 1 {
                            self.errors.push(TypeError::UnsupportedFeature {
                                feature: "string interpolation",
                                span: expr.span,
                                note: format!(
                                    "Type {} has inherent method `to_string`, but interpolation requires `to_string() -> String`",
                                    expr_ty.format_with_names(&self.type_env)
                                ),
                            });
                            return Err(());
                        }

                        self.unify(expr_ty, &inst_params[0], expr.span)?;

                        let ret_ty = self.zonk(&inst_ret);
                        if ret_ty == MonoType::String {
                            return Ok(());
                        }

                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "string interpolation",
                            span: expr.span,
                            note: format!(
                                "Type {} has `to_string`, but it returns {} (expected String)",
                                expr_ty.format_with_names(&self.type_env),
                                ret_ty.format_with_names(&self.type_env)
                            ),
                        });
                        return Err(());
                    }
                }

                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "string interpolation",
                    span: expr.span,
                    note: format!(
                        "Cannot interpolate type {}: missing inherent `to_string() -> String`",
                        expr_ty.format_with_names(&self.type_env)
                    ),
                });
                Err(())
            }
            _ => {
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "string interpolation",
                    span: expr.span,
                    note: format!(
                        "Cannot interpolate type {}: missing inherent `to_string() -> String`",
                        expr_ty.format_with_names(&self.type_env)
                    ),
                });
                Err(())
            }
        }
    }

    /// Validate a first-class module method reference (e.g. `Vector.len`) in check mode.
    /// Called when a FieldAccess with a module alias base appears in check_expr.
    fn check_module_func_ref(
        &mut self,
        alias: &str,
        method: &str,
        expected: &MonoType,
        expr_id: ExprId,
        span: Span,
    ) -> Result<(), ()> {
        // expected must be a function type
        let (params, ret) = match expected {
            MonoType::Function { params, ret } => (params, ret.as_ref()),
            _ => {
                self.errors.push(TypeError::TypeMismatch {
                    expected: expected.clone(),
                    actual: MonoType::Function {
                        params: vec![],
                        ret: Box::new(MonoType::Void),
                    },
                    span,
                    note: Some(format!("'{}.{}' is a function", alias, method)),
                });
                return Err(());
            }
        };

        if params.is_empty() {
            self.errors.push(TypeError::UnsupportedFeature {
                feature: "module method reference with no params",
                span,
                note: format!("'{}.{}' must take at least one argument", alias, method),
            });
            return Err(());
        }

        // Prefer registered/qualified function signatures when available.
        // This covers stdlib-defined builtin receiver methods (e.g. Vector.map,
        // Dict.values, Int.to_float) without hard-coding every method shape.
        let qualified = format!("{}.{}", alias, method);
        if let Some(sig) = self.value_env.get_function(&qualified).cloned() {
            let full_fn_ty = MonoType::Function {
                params: sig.params.clone(),
                ret: Box::new(sig.ret.clone().unwrap_or(MonoType::Void)),
            };
            let (inst_ty, _) = self.instantiate_vars(&sig.type_params, &full_fn_ty);
            self.unify(expected, &inst_ty, span)?;
            self.type_map.set_expr_type(expr_id, self.zonk(expected));
            return Ok(());
        }

        // Validate the first param matches the base type and check the shape
        let valid = match alias {
            "Vector" => {
                if !matches!(params[0], MonoType::Vector(_)) {
                    false
                } else {
                    match (method, params.len()) {
                        ("len", 1) => matches!(ret, MonoType::Int),
                        ("concat", 2) => matches!(ret, MonoType::Vector(_)),
                        ("slice", 3) => matches!(ret, MonoType::Vector(_)),
                        _ => false,
                    }
                }
            }
            "String" => {
                if !matches!(params[0], MonoType::String) {
                    false
                } else {
                    match (method, params.len()) {
                        ("len", 1) => matches!(ret, MonoType::Int),
                        ("concat", 2) => matches!(ret, MonoType::String),
                        ("substring", 3) => matches!(ret, MonoType::String),
                        ("get", 2) => matches!(
                            ret,
                            MonoType::Named { type_id, args }
                                if *type_id == crate::types::ty::OPTION_TYPE_ID
                                    && args.len() == 1
                                    && args[0] == MonoType::String
                        ),
                        ("to_string", 1) => matches!(ret, MonoType::String),
                        _ => false,
                    }
                }
            }
            "Dict" => {
                if !matches!(params[0], MonoType::Dict(_, _)) {
                    false
                } else {
                    match (method, params.len()) {
                        ("len", 1) => matches!(ret, MonoType::Int),
                        ("has", 2) => matches!(ret, MonoType::Bool),
                        ("keys", 1) => matches!(ret, MonoType::Vector(_)),
                        ("remove", 2) => matches!(ret, MonoType::Dict(_, _)),
                        _ => false,
                    }
                }
            }
            "Int" => {
                if !matches!(params[0], MonoType::Int) {
                    false
                } else {
                    matches!(
                        (method, params.len(), ret),
                        ("to_string", 1, MonoType::String)
                    )
                }
            }
            "Float" => {
                if !matches!(params[0], MonoType::Float) {
                    false
                } else {
                    matches!(
                        (method, params.len(), ret),
                        ("to_string", 1, MonoType::String)
                    )
                }
            }
            "Bool" => {
                if !matches!(params[0], MonoType::Bool) {
                    false
                } else {
                    matches!(
                        (method, params.len(), ret),
                        ("to_string", 1, MonoType::String)
                    )
                }
            }
            _ => false,
        };

        if valid {
            self.type_map.set_expr_type(expr_id, expected.clone());
            Ok(())
        } else {
            self.errors.push(TypeError::TypeMismatch {
                expected: expected.clone(),
                actual: MonoType::Function {
                    params: vec![],
                    ret: Box::new(MonoType::Void),
                },
                span,
                note: Some(format!(
                    "'{}.{}' signature does not match annotation",
                    alias, method
                )),
            });
            Err(())
        }
    }

    fn try_synth_registered_method_call(
        &mut self,
        base: &Expr,
        base_ty: &MonoType,
        method: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<Option<MonoType>, ()> {
        let receiver_type_id = if let Some(type_id) = method_receiver_type_id(base_ty) {
            type_id
        } else {
            return Ok(None);
        };
        let func_name = if let Some(name) = self
            .type_env
            .get_method_function(receiver_type_id, method)
            .cloned()
        {
            name
        } else {
            return Ok(None);
        };
        let sig = if let Some(sig) = self.value_env.get_function(&func_name).cloned() {
            sig
        } else {
            return Ok(None);
        };
        let explicit_count = args.len();
        let expected_count = sig.params.len().saturating_sub(1);
        if explicit_count != expected_count {
            self.errors.push(TypeError::WrongArity {
                expected: expected_count,
                actual: explicit_count,
                span,
            });
            return Err(());
        }
        let full_fn_ty = MonoType::Function {
            params: sig.params.clone(),
            ret: Box::new(sig.ret.clone().unwrap_or(MonoType::Void)),
        };
        let (inst_ty, _var_to_meta) = self.instantiate_vars(&sig.type_params, &full_fn_ty);
        let (inst_params, inst_ret) = match inst_ty {
            MonoType::Function { params, ret } => (params, *ret),
            _ => unreachable!(),
        };
        if let Some(recv_ty) = inst_params.first() {
            self.unify(base_ty, recv_ty, base.span)?;
        }
        for (arg, expected_ty) in args.iter().zip(inst_params.iter().skip(1)) {
            self.check_expr(arg, expected_ty)?;
        }
        Ok(Some(self.zonk(&inst_ret)))
    }

    /// Handle method calls: `receiver.method(args)`.
    /// Dispatches to builtin methods (Vector, String, primitives) or user-defined
    /// inherent methods registered in TypeEnv.
    fn synth_method_call(
        &mut self,
        base: &Expr,
        base_ty: MonoType,
        method: &str,
        args: &[Expr],
        span: Span,
        _callee_id: ExprId,
    ) -> Result<MonoType, ()> {
        match base_ty.clone() {
            MonoType::Vector(ref elem_ty) => {
                let elem_ty = *elem_ty.clone();
                match method {
                    "len" => {
                        if !args.is_empty() {
                            self.errors.push(TypeError::WrongArity {
                                expected: 0,
                                actual: args.len(),
                                span,
                            });
                            return Err(());
                        }
                        Ok(MonoType::Int)
                    }
                    "push" => {
                        if args.len() != 1 {
                            self.errors.push(TypeError::WrongArity {
                                expected: 1,
                                actual: args.len(),
                                span,
                            });
                            return Err(());
                        }
                        self.check_expr(&args[0], &elem_ty)?;
                        Ok(base_ty)
                    }
                    "concat" => {
                        if args.len() != 1 {
                            self.errors.push(TypeError::WrongArity {
                                expected: 1,
                                actual: args.len(),
                                span,
                            });
                            return Err(());
                        }
                        self.check_expr(&args[0], &base_ty)?;
                        Ok(base_ty)
                    }
                    "slice" => {
                        if args.len() != 2 {
                            self.errors.push(TypeError::WrongArity {
                                expected: 2,
                                actual: args.len(),
                                span,
                            });
                            return Err(());
                        }
                        self.check_expr(&args[0], &MonoType::Int)?;
                        self.check_expr(&args[1], &MonoType::Int)?;
                        Ok(base_ty)
                    }
                    "get" => {
                        // v.get(i) Option<T> — safe index access
                        if args.len() != 1 {
                            self.errors.push(TypeError::WrongArity {
                                expected: 1,
                                actual: args.len(),
                                span,
                            });
                            return Err(());
                        }
                        self.check_expr(&args[0], &MonoType::Int)?;
                        Ok(MonoType::Named {
                            type_id: crate::types::ty::OPTION_TYPE_ID,
                            args: vec![elem_ty],
                        })
                    }
                    "set" => {
                        // v.set(i, val) Option<Vector<T>> — safe update
                        if args.len() != 2 {
                            self.errors.push(TypeError::WrongArity {
                                expected: 2,
                                actual: args.len(),
                                span,
                            });
                            return Err(());
                        }
                        self.check_expr(&args[0], &MonoType::Int)?;
                        self.check_expr(&args[1], &elem_ty)?;
                        Ok(MonoType::Named {
                            type_id: crate::types::ty::OPTION_TYPE_ID,
                            args: vec![base_ty],
                        })
                    }
                    _ => {
                        if let Some(ret_ty) = self
                            .try_synth_registered_method_call(base, &base_ty, method, args, span)?
                        {
                            return Ok(ret_ty);
                        }
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "unknown vector method",
                            span,
                            note: format!("Vector has no method '{}'", method),
                        });
                        Err(())
                    }
                }
            }
            MonoType::String => match method {
                "len" => {
                    if !args.is_empty() {
                        self.errors.push(TypeError::WrongArity {
                            expected: 0,
                            actual: args.len(),
                            span,
                        });
                        return Err(());
                    }
                    Ok(MonoType::Int)
                }
                "concat" => {
                    if args.len() != 1 {
                        self.errors.push(TypeError::WrongArity {
                            expected: 1,
                            actual: args.len(),
                            span,
                        });
                        return Err(());
                    }
                    self.check_expr(&args[0], &MonoType::String)?;
                    Ok(MonoType::String)
                }
                "to_string" => {
                    if !args.is_empty() {
                        self.errors.push(TypeError::WrongArity {
                            expected: 0,
                            actual: args.len(),
                            span,
                        });
                        return Err(());
                    }
                    Ok(MonoType::String)
                }
                "slice" => {
                    if args.len() != 2 {
                        self.errors.push(TypeError::WrongArity {
                            expected: 2,
                            actual: args.len(),
                            span,
                        });
                        return Err(());
                    }
                    self.check_expr(&args[0], &MonoType::Int)?;
                    self.check_expr(&args[1], &MonoType::Int)?;
                    Ok(MonoType::String)
                }
                "get" => {
                    if args.len() != 1 {
                        self.errors.push(TypeError::WrongArity {
                            expected: 1,
                            actual: args.len(),
                            span,
                        });
                        return Err(());
                    }
                    self.check_expr(&args[0], &MonoType::Int)?;
                    Ok(MonoType::Named {
                        type_id: crate::types::ty::OPTION_TYPE_ID,
                        args: vec![MonoType::Byte],
                    })
                }
                _ => {
                    if let Some(ret_ty) =
                        self.try_synth_registered_method_call(base, &base_ty, method, args, span)?
                    {
                        return Ok(ret_ty);
                    }
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "unknown string method",
                        span,
                        note: format!("String has no method '{}'", method),
                    });
                    Err(())
                }
            },
            MonoType::Dict(k_ty, v_ty) => match method {
                "keys" => {
                    if !args.is_empty() {
                        self.errors.push(TypeError::WrongArity {
                            expected: 0,
                            actual: args.len(),
                            span,
                        });
                        return Err(());
                    }
                    Ok(MonoType::Vector(k_ty))
                }
                "len" => {
                    if !args.is_empty() {
                        self.errors.push(TypeError::WrongArity {
                            expected: 0,
                            actual: args.len(),
                            span,
                        });
                        return Err(());
                    }
                    Ok(MonoType::Int)
                }
                "has" => {
                    if args.len() != 1 {
                        self.errors.push(TypeError::WrongArity {
                            expected: 1,
                            actual: args.len(),
                            span,
                        });
                        return Err(());
                    }
                    self.check_expr(&args[0], &k_ty)?;
                    Ok(MonoType::Bool)
                }
                "remove" => {
                    if args.len() != 1 {
                        self.errors.push(TypeError::WrongArity {
                            expected: 1,
                            actual: args.len(),
                            span,
                        });
                        return Err(());
                    }
                    self.check_expr(&args[0], &k_ty)?;
                    Ok(MonoType::Dict(k_ty, v_ty))
                }
                _ => {
                    if let Some(ret_ty) =
                        self.try_synth_registered_method_call(base, &base_ty, method, args, span)?
                    {
                        return Ok(ret_ty);
                    }
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "unknown dict method",
                        span,
                        note: format!("Dict has no method '{}'", method),
                    });
                    Err(())
                }
            },
            MonoType::Byte => match method {
                "to_int" => {
                    if !args.is_empty() {
                        self.errors.push(TypeError::WrongArity {
                            expected: 0,
                            actual: args.len(),
                            span,
                        });
                        return Err(());
                    }
                    Ok(MonoType::Int)
                }
                "to_string" => {
                    if !args.is_empty() {
                        self.errors.push(TypeError::WrongArity {
                            expected: 0,
                            actual: args.len(),
                            span,
                        });
                        return Err(());
                    }
                    Ok(MonoType::String)
                }
                _ => {
                    if let Some(ret_ty) =
                        self.try_synth_registered_method_call(base, &base_ty, method, args, span)?
                    {
                        return Ok(ret_ty);
                    }
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "method on Byte type",
                        span,
                        note: format!("Byte has no method '{}'", method),
                    });
                    Err(())
                }
            },
            MonoType::Int | MonoType::Float | MonoType::Bool => {
                if method == "to_string" {
                    if !args.is_empty() {
                        self.errors.push(TypeError::WrongArity {
                            expected: 0,
                            actual: args.len(),
                            span,
                        });
                        return Err(());
                    }
                    Ok(MonoType::String)
                } else {
                    if let Some(ret_ty) =
                        self.try_synth_registered_method_call(base, &base_ty, method, args, span)?
                    {
                        return Ok(ret_ty);
                    }
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "method on primitive type",
                        span,
                        note: format!("Type {:?} has no method '{}'", base_ty, method),
                    });
                    Err(())
                }
            }
            MonoType::Named {
                type_id,
                args: ref cell_args,
            } if type_id == CELL_TYPE_ID => {
                let inner = cell_args.first().cloned().unwrap_or(MonoType::Void);
                match method {
                    "get" => {
                        if !args.is_empty() {
                            self.errors.push(TypeError::WrongArity {
                                expected: 0,
                                actual: args.len(),
                                span,
                            });
                            return Err(());
                        }
                        Ok(inner)
                    }
                    "set" => {
                        if args.len() != 1 {
                            self.errors.push(TypeError::WrongArity {
                                expected: 1,
                                actual: args.len(),
                                span,
                            });
                            return Err(());
                        }
                        self.check_expr(&args[0], &inner)?;
                        Ok(MonoType::Void)
                    }
                    "update" => {
                        if args.len() != 1 {
                            self.errors.push(TypeError::WrongArity {
                                expected: 1,
                                actual: args.len(),
                                span,
                            });
                            return Err(());
                        }
                        let expected_fn = MonoType::Function {
                            params: vec![inner.clone()],
                            ret: Box::new(inner),
                        };
                        self.check_expr(&args[0], &expected_fn)?;
                        Ok(MonoType::Void)
                    }
                    _ => {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "unknown Cell method",
                            span,
                            note: format!(
                                "Cell has no method '{}'; available: get, set, update",
                                method
                            ),
                        });
                        Err(())
                    }
                }
            }
            MonoType::Named {
                type_id,
                args: named_args,
            } => {
                // Look up user-defined inherent method
                if let Some(func_name) = self.type_env.get_method_function(type_id, method).cloned()
                {
                    if let Some(sig) = self.value_env.get_function(&func_name).cloned() {
                        // all_args: receiver + explicit args
                        let explicit_count = args.len();
                        let expected_count = sig.params.len().saturating_sub(1);
                        if explicit_count != expected_count {
                            self.errors.push(TypeError::WrongArity {
                                expected: expected_count,
                                actual: explicit_count,
                                span,
                            });
                            return Err(());
                        }
                        // Instantiate the full method signature with MetaVars so that
                        // type-level params (e.g. T in Box<T>) and method-level params
                        // are all properly solved via unification.
                        let full_fn_ty = MonoType::Function {
                            params: sig.params.clone(),
                            ret: Box::new(sig.ret.clone().unwrap_or(MonoType::Void)),
                        };
                        let (inst_ty, _var_to_meta) =
                            self.instantiate_vars(&sig.type_params, &full_fn_ty);
                        let (inst_params, inst_ret) = match inst_ty {
                            MonoType::Function { params, ret } => (params, *ret),
                            _ => unreachable!(),
                        };
                        // Unify the already-synthesised receiver type against the
                        // instantiated first param — this solves the MetaVars for the
                        // type-level type params (e.g. T → String for Box<String>).
                        if let Some(recv_ty) = inst_params.first() {
                            self.unify(&base_ty, recv_ty, base.span)?;
                        }
                        // Check remaining explicit args
                        for (arg, expected_ty) in args.iter().zip(inst_params.iter().skip(1)) {
                            self.check_expr(arg, expected_ty)?;
                        }
                        return Ok(self.zonk(&inst_ret));
                    }
                }

                // No inherent method — check if it's a function-typed record field
                // (capability record call: `record.fn_field(args)`)
                // Apply type-arg substitution for generic capability records
                if let Some(field_idx) = self.type_env.get_field_index(type_id, method) {
                    if let Some(fields) = self.type_env.get_record_fields(type_id) {
                        let type_params = self
                            .type_env
                            .get_def(type_id)
                            .map(|d| d.type_params().to_vec())
                            .unwrap_or_default();
                        let subst = build_type_subst(&type_params, &named_args);
                        let field_ty = apply_subst(&fields[field_idx].ty, &subst);
                        if let MonoType::Function { params, ret } = field_ty {
                            if params.len() != args.len() {
                                self.errors.push(TypeError::WrongArity {
                                    expected: params.len(),
                                    actual: args.len(),
                                    span,
                                });
                                return Err(());
                            }
                            for (arg, expected_ty) in args.iter().zip(params.iter()) {
                                self.check_expr(arg, expected_ty)?;
                            }
                            return Ok(*ret);
                        }
                    }
                }

                // No method found — report as missing field
                let type_name = self
                    .type_env
                    .get_def(type_id)
                    .map(|d| d.name().to_string())
                    .unwrap_or_else(|| format!("Type#{}", type_id.0));
                self.errors.push(TypeError::NoSuchField {
                    record_type: type_name,
                    field: method.to_string(),
                    span,
                });
                Err(())
            }
            _ => {
                self.errors.push(TypeError::TypeMismatch {
                    expected: MonoType::Int, // dummy
                    actual: base_ty,
                    span: base.span,
                    note: None,
                });
                Err(())
            }
        }
    }

    //
    // Blocks
    //

    fn synth_block(&mut self, block: &Block) -> Result<MonoType, ()> {
        self.local_env.push_scope();

        let mut result_ty = MonoType::Void;

        for stmt in &block.stmts {
            match stmt {
                Stmt::Let {
                    pattern,
                    ty,
                    value,
                    span: _,
                    ..
                } => {
                    self.check_let_stmt(pattern, ty.as_ref(), value);
                }
                Stmt::Expr(e) => {
                    // Expression statement
                    // If it's the last statement, its type becomes the block's type
                    result_ty = self.synth_expr(e)?;
                }
                Stmt::Return { value, span } => {
                    if let Some(ret_ty) = self.current_function_ret.clone() {
                        if let Some(val) = value {
                            self.check_expr(val, &ret_ty)?;
                        } else {
                            self.unify(&MonoType::Void, &ret_ty, *span)?;
                        }
                    } else {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "return outside function",
                            span: *span,
                            note: "Return statements are only allowed inside functions".to_string(),
                        });
                    }
                    result_ty = MonoType::Never;
                }
                Stmt::For {
                    pattern,
                    index_pattern,
                    iter,
                    body,
                    ..
                } => {
                    self.check_for_stmt(pattern, index_pattern.as_ref(), iter, body);
                    result_ty = MonoType::Void;
                }
                Stmt::ForCond { cond, body, .. } => {
                    let _ = self.check_expr(cond, &MonoType::Bool);
                    let _ = self.synth_block(body);
                    result_ty = MonoType::Void;
                }
                Stmt::Break { value, .. } => {
                    if let Some(val) = value {
                        let _ = self.synth_expr(val);
                    }
                    result_ty = MonoType::Never;
                }
                Stmt::Continue { .. } => {
                    result_ty = MonoType::Never;
                }
                Stmt::Defer { expr, span } => {
                    let deferred_ty = self.synth_expr(expr)?;
                    if deferred_ty == MonoType::Never {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "never-typed expression in defer",
                            span: *span,
                            note: "A defer body cannot diverge (return, break, continue, or \
                                   error(...)). These control-flow effects would be ambiguous \
                                   when executed at scope exit."
                                .to_string(),
                        });
                    }
                    result_ty = MonoType::Void;
                }
            }
        }

        self.local_env.pop_scope();
        Ok(result_ty)
    }

    /// Bidirectional block check: processes all statements like `synth_block`
    /// but uses `check_expr(last_expr, expected_ty)` for the final expression
    /// statement so that expected types flow into anonymous record literals,
    /// if-expressions, etc.
    fn check_block(&mut self, block: &Block, expected_ty: &MonoType) -> Result<(), ()> {
        self.local_env.push_scope();

        // Index of the last Expr statement (if any)
        let last_expr_idx = block.stmts.iter().rposition(|s| matches!(s, Stmt::Expr(_)));

        // Track whether the block ends with a diverging statement (Return/Break/Continue).
        // Diverging blocks have type Never, which unifies with any expected type.
        let mut diverges = false;

        for (i, stmt) in block.stmts.iter().enumerate() {
            match stmt {
                Stmt::Let {
                    pattern,
                    ty,
                    value,
                    span: _,
                    ..
                } => {
                    self.check_let_stmt(pattern, ty.as_ref(), value);
                    diverges = false;
                }
                Stmt::Expr(e) => {
                    if last_expr_idx == Some(i) {
                        // Final expression — check against expected return type
                        self.check_expr(e, expected_ty)?;
                    } else {
                        let _ = self.synth_expr(e);
                    }
                    diverges = false;
                }
                Stmt::Return { value, span } => {
                    if let Some(ret_ty) = self.current_function_ret.clone() {
                        if let Some(val) = value {
                            let _ = self.check_expr(val, &ret_ty);
                        } else {
                            let _ = self.unify(&MonoType::Void, &ret_ty, *span);
                        }
                    }
                    diverges = true;
                }
                Stmt::For {
                    pattern,
                    index_pattern,
                    iter,
                    body,
                    ..
                } => {
                    self.check_for_stmt(pattern, index_pattern.as_ref(), iter, body);
                    diverges = false;
                }
                Stmt::ForCond { cond, body, .. } => {
                    let _ = self.check_expr(cond, &MonoType::Bool);
                    let _ = self.synth_block(body);
                    diverges = false;
                }
                Stmt::Break { value, .. } => {
                    if let Some(val) = value {
                        let _ = self.synth_expr(val);
                    }
                    diverges = true;
                }
                Stmt::Continue { .. } => {
                    diverges = true;
                }
                Stmt::Defer { expr, span } => {
                    let deferred_ty = self.synth_expr(expr)?;
                    if deferred_ty == MonoType::Never {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "never-typed expression in defer",
                            span: *span,
                            note: "A defer body cannot diverge (return, break, continue, or \
                                   error(...)). These control-flow effects would be ambiguous \
                                   when executed at scope exit."
                                .to_string(),
                        });
                    }
                    diverges = false;
                }
            }
        }

        // If there's no final Expr stmt, infer the block type from control flow.
        // Diverging blocks (ending with return/break/continue) have type Never,
        // which unifies with any expected type without emitting an error.
        if last_expr_idx.is_none() {
            let block_ty = if diverges {
                MonoType::Never
            } else {
                MonoType::Void
            };
            let _ = self.unify(&block_ty, expected_ty, block.span);
        }

        self.local_env.pop_scope();
        Ok(())
    }

    //
    // Let statements
    //

    fn check_let_stmt(
        &mut self,
        pattern: &Pattern,
        ty: Option<&crate::syntax::ast::Type>,
        value: &Expr,
    ) {
        // For now, only support simple identifier patterns
        match pattern {
            Pattern::Ident(name, _span) => {
                // Determine the expected type
                let value_ty = if let Some(ann_ty) = ty {
                    // Type annotation provided - check mode
                    let expected = match self.resolve_type(ann_ty) {
                        Ok(t) => t,
                        Err(()) => return, // Error already recorded
                    };
                    match self.check_expr(value, &expected) {
                        Ok(()) => expected,
                        Err(()) => return, // Error already recorded
                    }
                } else {
                    // No annotation - synthesis mode
                    let t = match self.synth_expr(value) {
                        Ok(t) => t,
                        Err(()) => return,
                    };
                    let t = self.zonk(&t);
                    if contains_meta(&t) {
                        self.errors.push(TypeError::AmbiguousType {
                            name: name.clone(),
                            span: value.span,
                            note: "type cannot be inferred; add a type annotation".to_string(),
                        });
                        return;
                    }
                    t
                };

                // Bind the variable
                self.local_env.bind(name.clone(), value_ty);
            }
            Pattern::Wildcard(_) => {
                // Just evaluate the value for side effects
                let _ = self.synth_expr(value);
            }
            Pattern::Variant { .. } | Pattern::Literal(..) => {
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "pattern matching in let bindings",
                    span: value.span,
                    note: "Only simple identifiers are supported in let bindings for now"
                        .to_string(),
                });
            }
        }
    }

    //
    // If expressions
    //

    fn synth_if(
        &mut self,
        cond: &Expr,
        then_branch: &Expr,
        else_branch: Option<&Expr>,
        _span: Span,
    ) -> Result<MonoType, ()> {
        // Condition must be Bool
        self.check_expr(cond, &MonoType::Bool)?;

        // Synthesize then branch type
        let then_ty = self.synth_expr(then_branch)?;

        // If there's an else branch, both branches must have the same type
        if let Some(else_expr) = else_branch {
            let else_ty = self.synth_expr(else_expr)?;
            self.unify(&then_ty, &else_ty, else_expr.span)?;
            // If one branch diverges (Never), use the other branch's type
            if then_ty == MonoType::Never {
                Ok(else_ty)
            } else {
                Ok(then_ty)
            }
        } else {
            // No else branch - result type is Void
            self.unify(&then_ty, &MonoType::Void, then_branch.span)?;
            Ok(MonoType::Void)
        }
    }

    //
    // Field access
    //

    fn synth_field_access(&mut self, base: &Expr, field: &str, span: Span) -> Result<MonoType, ()> {
        // Check for TypeName.Variant syntax: base is a type name, field is a variant
        if let ExprKind::Ident(type_name) = &base.kind {
            if let Some(type_id) = self.type_env.lookup_type(type_name) {
                if let Some(variant_idx) = self.type_env.get_variant_index(type_id, field) {
                    // Instantiate generic type params with fresh MetaVars so that
                    // unification (e.g. unifying Done with Yield's type) can solve them.
                    let type_params: Vec<String> = self
                        .type_env
                        .get_def(type_id)
                        .map(|d| d.type_params().to_vec())
                        .unwrap_or_default();
                    let inst_map: HashMap<String, MonoType> = type_params
                        .iter()
                        .map(|p| (p.clone(), self.fresh_meta()))
                        .collect();
                    let type_var_args: Vec<MonoType> = type_params
                        .iter()
                        .map(|p| MonoType::Var(p.clone()))
                        .collect();
                    let raw_named = MonoType::Named {
                        type_id,
                        args: type_var_args,
                    };
                    let named_ty = if inst_map.is_empty() {
                        raw_named
                    } else {
                        apply_subst(&raw_named, &inst_map)
                    };
                    // Record type of the type-name base as Named (so lowerer can identify it)
                    self.type_map.set_expr_type(base.id, named_ty.clone());
                    let variants = self
                        .type_env
                        .get_variants(type_id)
                        .expect("variant index exists, variants must exist");
                    let variant_fields_raw = variants[variant_idx].fields.clone();
                    let variant_fields: Vec<MonoType> = variant_fields_raw
                        .iter()
                        .map(|f| apply_subst(f, &inst_map))
                        .collect();
                    return if variant_fields.is_empty() {
                        // Zero-arg variant — directly a value of the named type
                        Ok(named_ty)
                    } else {
                        // Parameterized variant accessed as a value (not called here)
                        Ok(MonoType::Function {
                            params: variant_fields,
                            ret: Box::new(named_ty),
                        })
                    };
                }
            }
        }

        let base_ty = self.synth_expr(base)?;

        match base_ty {
            MonoType::Named {
                type_id,
                args: ref type_args,
            } => {
                // Check for field/method collision
                let has_field = self.type_env.has_field(type_id, field);
                let has_method = self.type_env.has_method(type_id, field);

                if has_field && has_method {
                    let type_name = self
                        .type_env
                        .get_def(type_id)
                        .map(|d| d.name().to_string())
                        .unwrap_or_else(|| format!("Type#{}", type_id.0));

                    self.errors.push(TypeError::FieldMethodCollision {
                        type_name,
                        name: field.to_string(),
                        span,
                    });
                    return Err(());
                }

                // Look up the record fields; apply type-arg substitution for generic types
                if let Some(record_fields) = self.type_env.get_record_fields(type_id) {
                    let type_params = self
                        .type_env
                        .get_def(type_id)
                        .map(|d| d.type_params().to_vec())
                        .unwrap_or_default();
                    let subst = build_type_subst(&type_params, &type_args);
                    // Find the field
                    for f in record_fields {
                        if f.name == field {
                            return Ok(apply_subst(&f.ty, &subst));
                        }
                    }

                    // Field not found - check if it's a method
                    if has_method {
                        // Method calls are handled earlier via synth_method_call;
                        // reaching here means field access syntax on a method name.
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "inherent method calls",
                            span,
                            note: format!(
                                "Method '{}' exists but method calls are not yet fully implemented",
                                field
                            ),
                        });
                        return Err(());
                    }

                    // Neither field nor method
                    let record_name = self
                        .type_env
                        .get_def(type_id)
                        .map(|d| d.name().to_string())
                        .unwrap_or_else(|| format!("Type#{}", type_id.0));

                    self.errors.push(TypeError::NoSuchField {
                        record_type: record_name,
                        field: field.to_string(),
                        span,
                    });
                    Err(())
                } else {
                    // Not a record type
                    self.errors.push(TypeError::TypeMismatch {
                        expected: MonoType::Int, // Dummy
                        actual: base_ty,
                        span: base.span,
                        note: None,
                    });
                    Err(())
                }
            }
            _ => {
                self.errors.push(TypeError::TypeMismatch {
                    expected: MonoType::Int, // Dummy
                    actual: base_ty,
                    span: base.span,
                    note: None,
                });
                Err(())
            }
        }
    }

    //
    // Vector indexing
    //

    fn synth_index(&mut self, base: &Expr, index: &Expr, _span: Span) -> Result<MonoType, ()> {
        let base_ty = self.synth_expr(base)?;

        match base_ty {
            MonoType::Vector(elem_ty) => {
                self.check_expr(index, &MonoType::Int)?;
                Ok(*elem_ty)
            }
            MonoType::String => {
                self.check_expr(index, &MonoType::Int)?;
                Ok(MonoType::Byte) // String indexing returns a byte at byte offset
            }
            MonoType::Dict(k_ty, v_ty) => {
                self.check_expr(index, &k_ty)?;
                // Dict indexing is safe: returns Option<V>
                Ok(MonoType::Named {
                    type_id: OPTION_TYPE_ID,
                    args: vec![*v_ty],
                })
            }
            _ => {
                self.errors.push(TypeError::TypeMismatch {
                    expected: MonoType::Vector(Box::new(MonoType::Int)), // Dummy
                    actual: base_ty,
                    span: base.span,
                    note: None,
                });
                Err(())
            }
        }
    }

    //
    // Vector literals
    //

    fn synth_array(&mut self, elements: &[Expr], span: Span) -> Result<MonoType, ()> {
        if elements.is_empty() {
            // Empty vector - we can't infer the type
            // For now, error - require type annotation
            self.errors.push(TypeError::UnsupportedFeature {
                feature: "empty vector literals",
                span,
                note: "Empty vectors require type annotations (not yet supported)".to_string(),
            });
            return Err(());
        }

        // Infer type from first element
        let first_ty = self.synth_expr(&elements[0])?;

        // Check all other elements match
        for elem in &elements[1..] {
            self.check_expr(elem, &first_ty)?;
        }

        Ok(MonoType::Vector(Box::new(first_ty)))
    }

    //
    // Record literals
    //

    fn synth_record_lit(
        &mut self,
        name: Option<&str>,
        fields: &[(String, Expr)],
        span: Span,
    ) -> Result<MonoType, ()> {
        if let Some(type_name) = name {
            // Named record literal: Point.{ x: 1, y: 2 }
            let type_id = match self.type_env.lookup_type(type_name) {
                Some(id) => id,
                None => {
                    self.errors.push(TypeError::UndefinedType {
                        name: type_name.to_string(),
                        span,
                    });
                    return Err(());
                }
            };

            let type_params = self
                .type_env
                .get_def(type_id)
                .map(|d| d.type_params().to_vec())
                .unwrap_or_default();

            if type_params.is_empty() {
                // Non-generic: check fields directly
                self.check_record_lit_fields(type_id, &[], fields, span)?;
                Ok(MonoType::named(type_id))
            } else {
                // Generic: instantiate type params with MetaVars, then synth each field
                // and unify against the instantiated declared type to solve the MetaVars.
                let def_fields: Vec<(String, MonoType)> = self
                    .type_env
                    .get_record_fields(type_id)
                    .map(|fs| fs.iter().map(|f| (f.name.clone(), f.ty.clone())).collect())
                    .unwrap_or_default();

                // Create a fresh MetaVar for each type param
                let inst_map: HashMap<String, MonoType> = type_params
                    .iter()
                    .map(|p| (p.clone(), self.fresh_meta()))
                    .collect();

                // Synth each field value; unify against instantiated declared type
                let mut field_synth: Vec<(&str, Result<MonoType, ()>)> = Vec::new();
                for (provided_name, provided_expr) in fields.iter() {
                    let result = self.synth_expr(provided_expr);
                    if let Ok(actual_ty) = &result {
                        if let Some((_, declared_ty)) =
                            def_fields.iter().find(|(n, _)| n == provided_name)
                        {
                            let inst_decl_ty = apply_subst(declared_ty, &inst_map);
                            let _ = self.unify(actual_ty, &inst_decl_ty, provided_expr.span);
                        }
                    }
                    field_synth.push((provided_name.as_str(), result));
                }

                // Build concrete type args by zonking the MetaVars
                let type_args: Vec<MonoType> = type_params
                    .iter()
                    .map(|p| self.zonk(inst_map.get(p).unwrap()))
                    .collect();
                let subst2 = build_type_subst(&type_params, &type_args);

                let record_name = self
                    .type_env
                    .get_def(type_id)
                    .map(|d| d.name().to_string())
                    .unwrap_or_else(|| format!("Type#{}", type_id.0));

                // Check for extra (unknown) fields
                let expected_names: Vec<&str> =
                    def_fields.iter().map(|(n, _)| n.as_str()).collect();
                for (provided_name, _) in fields.iter() {
                    if !expected_names.contains(&provided_name.as_str()) {
                        self.errors.push(TypeError::NoSuchField {
                            record_type: record_name.clone(),
                            field: provided_name.clone(),
                            span,
                        });
                        return Err(());
                    }
                }

                // Validate each expected field: present, correctly typed
                let mut ok = true;
                for (expected_name, declared_ty) in &def_fields {
                    let concrete_ty = apply_subst(declared_ty, &subst2);
                    match field_synth
                        .iter()
                        .find(|(n, _)| *n == expected_name.as_str())
                    {
                        Some((_, Ok(actual_ty))) => {
                            if self.unify(actual_ty, &concrete_ty, span).is_err() {
                                ok = false;
                            }
                        }
                        Some((_, Err(()))) => {
                            ok = false;
                        }
                        None => {
                            self.errors.push(TypeError::NoSuchField {
                                record_type: record_name.clone(),
                                field: expected_name.clone(),
                                span,
                            });
                            ok = false;
                        }
                    }
                }

                if ok {
                    Ok(MonoType::Named {
                        type_id,
                        args: type_args,
                    })
                } else {
                    Err(())
                }
            }
        } else {
            // Anonymous record literal: .{ x: 1, y: 2 }
            // This requires expected type from context - error in synthesis mode
            self.errors
                .push(TypeError::AnonymousRecordWithoutContext { span });
            Err(())
        }
    }

    fn check_anon_record_lit(
        &mut self,
        fields: &[(String, Expr)],
        expected: &MonoType,
        span: Span,
    ) -> Result<(), ()> {
        match expected {
            MonoType::Named { type_id, args } => {
                self.check_record_lit_fields(*type_id, args, fields, span)
            }
            _ => {
                self.errors.push(TypeError::TypeMismatch {
                    expected: expected.clone(),
                    actual: MonoType::Void, // Dummy
                    span,
                    note: None,
                });
                Err(())
            }
        }
    }

    fn check_record_lit_fields(
        &mut self,
        type_id: crate::types::ty::TypeId,
        type_args: &[MonoType],
        fields: &[(String, Expr)],
        span: Span,
    ) -> Result<(), ()> {
        let expected_fields = match self.type_env.get_record_fields(type_id) {
            Some(f) => f,
            None => {
                self.errors.push(TypeError::TypeMismatch {
                    expected: MonoType::named(type_id),
                    actual: MonoType::Void, // Dummy
                    span,
                    note: None,
                });
                return Err(());
            }
        };

        // Build substitution for generic types
        let subst = {
            let type_params = self
                .type_env
                .get_def(type_id)
                .map(|d| d.type_params().to_vec())
                .unwrap_or_default();
            build_type_subst(&type_params, type_args)
        };

        // Check all expected fields are present and have correct types
        // Apply substitution to declared field types for generic types
        let expected_fields_vec: Vec<_> = expected_fields
            .iter()
            .map(|f| (f.name.clone(), apply_subst(&f.ty, &subst)))
            .collect();

        let expected_names: Vec<String> = expected_fields_vec
            .iter()
            .map(|(name, _)| name.clone())
            .collect();

        for (field_name, field_ty) in &expected_fields_vec {
            let provided = fields.iter().find(|(name, _)| name == field_name);

            if let Some((_, value)) = provided {
                self.check_expr(value, field_ty)?;
            } else {
                // Missing field
                let record_name = self
                    .type_env
                    .get_def(type_id)
                    .map(|d| d.name().to_string())
                    .unwrap_or_else(|| format!("Type#{}", type_id.0));

                self.errors.push(TypeError::NoSuchField {
                    record_type: record_name,
                    field: field_name.clone(),
                    span,
                });
                return Err(());
            }
        }

        // Check for extra fields

        for (provided_name, _) in fields {
            if !expected_names.contains(provided_name) {
                let record_name = self
                    .type_env
                    .get_def(type_id)
                    .map(|d| d.name().to_string())
                    .unwrap_or_else(|| format!("Type#{}", type_id.0));

                self.errors.push(TypeError::NoSuchField {
                    record_type: record_name,
                    field: provided_name.clone(),
                    span,
                });
                return Err(());
            }
        }

        Ok(())
    }

    //
    // Variant literals
    //

    fn synth_variant_lit(
        &mut self,
        variant_name: &str,
        _fields: &[Expr],
        span: Span,
    ) -> Result<MonoType, ()> {
        // Variant literals require type context to disambiguate which sum type they belong to
        // Multiple sum types may have variants with the same name
        // Use checking mode with type annotation: `x: Option<Int> = .Some(42)`

        self.errors.push(TypeError::UnsupportedFeature {
            feature: "variant literals without type context",
            span,
            note: format!(
                "Cannot infer type for variant .{}(...) - provide a type annotation",
                variant_name
            ),
        });
        Err(())
    }

    fn check_variant_lit(
        &mut self,
        variant_name: &str,
        fields: &[Expr],
        expected: &MonoType,
        span: Span,
    ) -> Result<(), ()> {
        // Expected type must be a sum type
        match expected {
            MonoType::Named { type_id, args } => {
                // Get the variants for this sum type
                let variants = match self.type_env.get_variants(*type_id) {
                    Some(v) => v,
                    None => {
                        // Not a sum type
                        self.errors.push(TypeError::TypeMismatch {
                            expected: expected.clone(),
                            actual: MonoType::Void, // Placeholder
                            span,
                            note: None,
                        });
                        return Err(());
                    }
                };

                // Find the variant with the matching name
                let variant = variants.iter().find(|v| &v.name == variant_name);

                match variant {
                    Some(v) => {
                        // For Option<T> and Result<T,E>, the TypeDef holds Void placeholders.
                        // Use the actual type args from the MonoType instead.
                        let field_types: Vec<MonoType> = if *type_id == OPTION_TYPE_ID {
                            match variant_name {
                                "None" => vec![],
                                "Some" => vec![args.first().cloned().unwrap_or(MonoType::Void)],
                                _ => v.fields.clone(),
                            }
                        } else if *type_id == RESULT_TYPE_ID {
                            match variant_name {
                                "Ok" => vec![args.first().cloned().unwrap_or(MonoType::Void)],
                                "Err" => vec![args.get(1).cloned().unwrap_or(MonoType::Void)],
                                _ => v.fields.clone(),
                            }
                        } else {
                            // User-defined generic sum type: apply type-arg substitution
                            let type_params = self
                                .type_env
                                .get_def(*type_id)
                                .map(|d| d.type_params().to_vec())
                                .unwrap_or_default();
                            let subst = build_type_subst(&type_params, args);
                            v.fields.iter().map(|f| apply_subst(f, &subst)).collect()
                        };

                        // Check arity
                        if field_types.len() != fields.len() {
                            self.errors.push(TypeError::WrongArity {
                                expected: field_types.len(),
                                actual: fields.len(),
                                span,
                            });
                            return Err(());
                        }

                        // Check each field
                        for (field_expr, field_ty) in fields.iter().zip(field_types.iter()) {
                            self.check_expr(field_expr, field_ty)?;
                        }

                        Ok(())
                    }
                    None => {
                        // Variant not found in this sum type
                        let sum_type_name = self
                            .type_env
                            .get_def(*type_id)
                            .map(|d| d.name().to_string())
                            .unwrap_or_else(|| format!("Type#{}", type_id.0));

                        self.errors.push(TypeError::NoSuchVariant {
                            sum_type: sum_type_name,
                            variant: variant_name.to_string(),
                            span,
                        });
                        Err(())
                    }
                }
            }
            _ => {
                // Expected type is not a sum type
                self.errors.push(TypeError::TypeMismatch {
                    expected: expected.clone(),
                    actual: MonoType::Void, // Placeholder - we don't know the actual type
                    span,
                    note: None,
                });
                Err(())
            }
        }
    }

    //
    // Case expressions
    //

    fn synth_case(
        &mut self,
        scrutinee: &Expr,
        arms: &[crate::syntax::ast::CaseArm],
        span: Span,
    ) -> Result<MonoType, ()> {
        let scrut_ty = self.synth_expr(scrutinee)?;

        // Scrutinee must be a sum type or a matchable primitive (Int, Bool, String)
        let is_primitive_match =
            matches!(scrut_ty, MonoType::Int | MonoType::Bool | MonoType::String);
        if !is_primitive_match && !scrut_ty.is_sum(&self.type_env) {
            self.errors.push(TypeError::CaseScrutineeNotSumType {
                actual_type: scrut_ty.clone(),
                span: scrutinee.span,
            });
            return Err(());
        }

        if arms.is_empty() {
            self.errors.push(TypeError::NonExhaustiveMatch {
                missing: vec!["(all patterns)".to_string()],
                span,
            });
            return Err(());
        }

        // Check exhaustiveness
        PatternChecker::check_exhaustiveness(
            &self.type_env,
            &mut self.errors,
            &scrut_ty,
            arms,
            span,
        )?;

        // Type-check first arm to get result type
        let result_ty = self.synth_case_arm(&arms[0], &scrut_ty)?;

        // Check all other arms match
        for arm in &arms[1..] {
            let arm_ty = self.synth_case_arm(arm, &scrut_ty)?;
            self.unify(&arm_ty, &result_ty, arm.span)?;
        }

        Ok(result_ty)
    }

    fn synth_case_arm(
        &mut self,
        arm: &crate::syntax::ast::CaseArm,
        scrut_ty: &MonoType,
    ) -> Result<MonoType, ()> {
        // Push a new scope for pattern bindings
        self.local_env.push_scope();

        // Check the pattern and bind variables
        let mut pattern_checker =
            PatternChecker::new(&self.type_env, &mut self.local_env, &mut self.errors);
        pattern_checker.check_pattern(&arm.pattern, scrut_ty)?;

        // Type-check the arm body
        let body_ty = self.synth_expr(&arm.body)?;

        self.local_env.pop_scope();
        Ok(body_ty)
    }

    //
    // Unification
    //

    fn unify(&mut self, actual: &MonoType, expected: &MonoType, span: Span) -> Result<(), ()> {
        let actual = self.zonk(actual);
        let expected = self.zonk(expected);

        // Never (bottom type) unifies with anything
        if actual == MonoType::Never || expected == MonoType::Never {
            return Ok(());
        }
        if actual == expected {
            return Ok(());
        }

        match (&actual, &expected) {
            // MetaVar on left: solve it
            (MonoType::MetaVar(id), _) => {
                let id = *id;
                return self.solve_meta(id, expected, span);
            }
            // MetaVar on right: solve it
            (_, MonoType::MetaVar(id)) => {
                let id = *id;
                return self.solve_meta(id, actual, span);
            }
            // Structural: Vector
            (MonoType::Vector(a), MonoType::Vector(b)) => {
                let a = a.as_ref().clone();
                let b = b.as_ref().clone();
                return self.unify(&a, &b, span);
            }
            // Structural: Dict
            (MonoType::Dict(ak, av), MonoType::Dict(bk, bv)) => {
                let ak = ak.as_ref().clone();
                let av = av.as_ref().clone();
                let bk = bk.as_ref().clone();
                let bv = bv.as_ref().clone();
                self.unify(&ak, &bk, span)?;
                return self.unify(&av, &bv, span);
            }
            // Structural: Function
            (
                MonoType::Function {
                    params: p1,
                    ret: r1,
                },
                MonoType::Function {
                    params: p2,
                    ret: r2,
                },
            ) => {
                if p1.len() != p2.len() {
                    self.errors.push(TypeError::TypeMismatch {
                        expected: expected.clone(),
                        actual: actual.clone(),
                        span,
                        note: None,
                    });
                    return Err(());
                }
                let pairs: Vec<_> = p1
                    .iter()
                    .zip(p2.iter())
                    .map(|(a, b)| (a.clone(), b.clone()))
                    .collect();
                for (a, b) in pairs {
                    self.unify(&a, &b, span)?;
                }
                let r1 = r1.as_ref().clone();
                let r2 = r2.as_ref().clone();
                return self.unify(&r1, &r2, span);
            }
            // Structural: Named
            (
                MonoType::Named {
                    type_id: id1,
                    args: a1,
                },
                MonoType::Named {
                    type_id: id2,
                    args: a2,
                },
            ) => {
                if id1 != id2 || a1.len() != a2.len() {
                    self.errors.push(TypeError::TypeMismatch {
                        expected: expected.clone(),
                        actual: actual.clone(),
                        span,
                        note: None,
                    });
                    return Err(());
                }
                let pairs: Vec<_> = a1
                    .iter()
                    .zip(a2.iter())
                    .map(|(a, b)| (a.clone(), b.clone()))
                    .collect();
                for (a, b) in pairs {
                    self.unify(&a, &b, span)?;
                }
                return Ok(());
            }
            _ => {}
        }

        self.errors.push(TypeError::TypeMismatch {
            expected: expected.clone(),
            actual: actual.clone(),
            span,
            note: None,
        });
        Err(())
    }

    //
    // Assignment / rebinding
    //

    fn synth_assign(&mut self, left: &Expr, right: &Expr, span: Span) -> Result<MonoType, ()> {
        match &left.kind {
            ExprKind::Ident(name) => {
                if self.at_module_scope {
                    self.errors.push(TypeError::ModuleScopeRebinding {
                        name: name.clone(),
                        span: left.span,
                    });
                    return Err(());
                }
                let existing_ty = if let Some(ty) = self.local_env.lookup(name) {
                    ty.clone()
                } else if let Some(ty) = self.value_env.lookup(name) {
                    ty
                } else {
                    self.errors.push(TypeError::UndefinedVariable {
                        name: name.clone(),
                        span: left.span,
                    });
                    return Err(());
                };
                self.check_expr(right, &existing_ty)?;
                Ok(MonoType::Void)
            }
            ExprKind::FieldAccess { base, field } => {
                // r.field = expr — type-check both sides conservatively
                let base_ty = self.synth_expr(base)?;
                match base_ty {
                    MonoType::Named {
                        type_id,
                        args: type_args,
                    } => {
                        if let Some(fields) = self.type_env.get_record_fields(type_id) {
                            let type_params = self
                                .type_env
                                .get_def(type_id)
                                .map(|d| d.type_params().to_vec())
                                .unwrap_or_default();
                            let subst = build_type_subst(&type_params, &type_args);
                            let field_ty = fields
                                .iter()
                                .find(|f| f.name == *field)
                                .map(|f| apply_subst(&f.ty, &subst));
                            if let Some(fty) = field_ty {
                                self.check_expr(right, &fty)?;
                                Ok(MonoType::Void)
                            } else {
                                self.errors.push(TypeError::NoSuchField {
                                    record_type: type_id.0.to_string(),
                                    field: field.clone(),
                                    span,
                                });
                                Err(())
                            }
                        } else {
                            self.errors.push(TypeError::UnsupportedFeature {
                                feature: "field assignment on non-record type",
                                span,
                                note: "Field assignment requires a record type".to_string(),
                            });
                            Err(())
                        }
                    }
                    _ => {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "field assignment on non-record",
                            span,
                            note: "Field assignment requires a record type".to_string(),
                        });
                        Err(())
                    }
                }
            }
            ExprKind::Index { base, index } => {
                let base_ty = self.synth_expr(base)?;
                match base_ty {
                    MonoType::Vector(elem_ty) => {
                        // arr[i] = v — index must be Int, value must match element type
                        self.check_expr(index, &MonoType::Int)?;
                        self.check_expr(right, &elem_ty)?;
                        Ok(MonoType::Void)
                    }
                    MonoType::Dict(k_ty, v_ty) => {
                        // m[k] = v — index must match K, value must match V
                        self.check_expr(index, &k_ty)?;
                        self.check_expr(right, &v_ty)?;
                        Ok(MonoType::Void)
                    }
                    other => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Vector(Box::new(MonoType::Int)),
                            actual: other,
                            span: base.span,
                            note: None,
                        });
                        Err(())
                    }
                }
            }
            _ => {
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "complex assignment target",
                    span,
                    note: "Only identifiers, field accesses, and index expressions can be assigned"
                        .to_string(),
                });
                Err(())
            }
        }
    }

    //
    // For loop type checking
    //

    fn check_for_stmt(
        &mut self,
        pattern: &Pattern,
        index_pattern: Option<&Pattern>,
        iter: &Expr,
        body: &Block,
    ) {
        let iter_ty = match self.synth_expr(iter) {
            Ok(ty) => ty,
            Err(()) => return,
        };

        self.local_env.push_scope();

        match iter_ty {
            MonoType::Vector(elem) => {
                match pattern {
                    Pattern::Ident(name, _) => self.local_env.bind(name.clone(), *elem),
                    Pattern::Wildcard(_) => {}
                    _ => {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "complex pattern in for loop",
                            span: iter.span,
                            note: "Only simple identifiers are supported in for loop patterns"
                                .to_string(),
                        });
                    }
                }
                // index_pattern binds Int index
                if let Some(idx_pat) = index_pattern {
                    if let Pattern::Ident(name, _) = idx_pat {
                        self.local_env.bind(name.clone(), MonoType::Int);
                    }
                }
            }
            MonoType::String => {
                match pattern {
                    Pattern::Ident(name, _) => self.local_env.bind(name.clone(), MonoType::Byte),
                    Pattern::Wildcard(_) => {}
                    _ => {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "complex pattern in for loop",
                            span: iter.span,
                            note: "Only simple identifiers are supported in for loop patterns"
                                .to_string(),
                        });
                    }
                }
                // index_pattern binds Int index
                if let Some(idx_pat) = index_pattern {
                    if let Pattern::Ident(name, _) = idx_pat {
                        self.local_env.bind(name.clone(), MonoType::Int);
                    }
                }
            }
            MonoType::Named { type_id, .. } if type_id == RANGE_TYPE_ID => {
                match pattern {
                    Pattern::Ident(name, _) => self.local_env.bind(name.clone(), MonoType::Int),
                    Pattern::Wildcard(_) => {}
                    _ => {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "complex pattern in for loop",
                            span: iter.span,
                            note: "Only simple identifiers are supported in for loop patterns"
                                .to_string(),
                        });
                    }
                }
                // index_pattern also binds Int (a simple counter)
                if let Some(idx_pat) = index_pattern {
                    if let Pattern::Ident(name, _) = idx_pat {
                        self.local_env.bind(name.clone(), MonoType::Int);
                    }
                }
            }
            MonoType::Named { type_id, ref args } if type_id == ITERATOR_TYPE_ID => {
                let elem_ty = args.first().cloned().unwrap_or(MonoType::Void);
                match pattern {
                    Pattern::Ident(name, _) => self.local_env.bind(name.clone(), elem_ty),
                    Pattern::Wildcard(_) => {}
                    _ => {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "complex pattern in for loop over Iterator",
                            span: iter.span,
                            note: "Only simple identifiers are supported in for loop patterns over Iterator<T>".to_string(),
                        });
                    }
                }
                if index_pattern.is_some() {
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "indexed for loop over Iterator",
                        span: iter.span,
                        note: "Iterator<T> does not support the 'for x, i in' form".to_string(),
                    });
                }
            }
            MonoType::Dict(key_ty, val_ty) => {
                match pattern {
                    Pattern::Ident(name, _) => self.local_env.bind(name.clone(), *key_ty),
                    Pattern::Wildcard(_) => {}
                    _ => {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "complex pattern in for loop",
                            span: iter.span,
                            note: "Only simple identifiers are supported in for loop patterns"
                                .to_string(),
                        });
                    }
                }
                // index_pattern binds the value type (not an integer index)
                if let Some(val_pat) = index_pattern {
                    if let Pattern::Ident(name, _) = val_pat {
                        self.local_env.bind(name.clone(), *val_ty);
                    }
                }
            }
            other => {
                self.errors.push(TypeError::TypeMismatch {
                    expected: MonoType::Vector(Box::new(MonoType::Int)),
                    actual: other,
                    span: iter.span,
                    note: None,
                });
                return;
            }
        }

        let saved_scope = std::mem::replace(&mut self.at_module_scope, false);
        let _ = self.synth_block(body);
        self.at_module_scope = saved_scope;
        self.local_env.pop_scope();
    }

    //
    // Collect expression type checking
    //

    fn synth_collect(
        &mut self,
        pattern: &Pattern,
        index_pattern: Option<&Pattern>,
        iter: &Expr,
        body: &Expr,
        span: Span,
    ) -> Result<MonoType, ()> {
        let iter_ty = self.synth_expr(iter)?;

        self.local_env.push_scope();
        let mut had_error = false;

        match iter_ty {
            MonoType::Vector(elem) => {
                match pattern {
                    Pattern::Ident(name, _) => self.local_env.bind(name.clone(), *elem),
                    Pattern::Wildcard(_) => {}
                    _ => {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "complex pattern in collect",
                            span,
                            note: "Only simple identifiers are supported in collect patterns"
                                .to_string(),
                        });
                        had_error = true;
                    }
                }
                if let Some(idx_pat) = index_pattern {
                    if let Pattern::Ident(name, _) = idx_pat {
                        self.local_env.bind(name.clone(), MonoType::Int);
                    }
                }
            }
            MonoType::String => {
                match pattern {
                    Pattern::Ident(name, _) => self.local_env.bind(name.clone(), MonoType::Byte),
                    Pattern::Wildcard(_) => {}
                    _ => {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "complex pattern in collect",
                            span,
                            note: "Only simple identifiers are supported in collect patterns"
                                .to_string(),
                        });
                        had_error = true;
                    }
                }
                if let Some(idx_pat) = index_pattern {
                    if let Pattern::Ident(name, _) = idx_pat {
                        self.local_env.bind(name.clone(), MonoType::Int);
                    }
                }
            }
            MonoType::Named { type_id, .. } if type_id == RANGE_TYPE_ID => {
                match pattern {
                    Pattern::Ident(name, _) => self.local_env.bind(name.clone(), MonoType::Int),
                    Pattern::Wildcard(_) => {}
                    _ => {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "complex pattern in collect",
                            span,
                            note: "Only simple identifiers are supported in collect patterns"
                                .to_string(),
                        });
                        had_error = true;
                    }
                }
                if let Some(idx_pat) = index_pattern {
                    if let Pattern::Ident(name, _) = idx_pat {
                        self.local_env.bind(name.clone(), MonoType::Int);
                    }
                }
            }
            MonoType::Named { type_id, ref args } if type_id == ITERATOR_TYPE_ID => {
                let elem_ty = args.first().cloned().unwrap_or(MonoType::Void);
                match pattern {
                    Pattern::Ident(name, _) => self.local_env.bind(name.clone(), elem_ty),
                    Pattern::Wildcard(_) => {}
                    _ => {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "complex pattern in collect over Iterator",
                            span,
                            note: "Only simple identifiers are supported in collect patterns over Iterator<T>".to_string(),
                        });
                        had_error = true;
                    }
                }
                if index_pattern.is_some() {
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "indexed collect over Iterator",
                        span: iter.span,
                        note: "Iterator<T> does not support the 'collect x, i in' form".to_string(),
                    });
                    had_error = true;
                }
            }
            MonoType::Dict(key_ty, val_ty) => {
                match pattern {
                    Pattern::Ident(name, _) => self.local_env.bind(name.clone(), *key_ty),
                    Pattern::Wildcard(_) => {}
                    _ => {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "complex pattern in collect",
                            span,
                            note: "Only simple identifiers are supported in collect patterns"
                                .to_string(),
                        });
                        had_error = true;
                    }
                }
                if let Some(val_pat) = index_pattern {
                    if let Pattern::Ident(name, _) = val_pat {
                        self.local_env.bind(name.clone(), *val_ty);
                    }
                }
            }
            other => {
                self.errors.push(TypeError::TypeMismatch {
                    expected: MonoType::Vector(Box::new(MonoType::Int)),
                    actual: other,
                    span: iter.span,
                    note: None,
                });
                had_error = true;
            }
        }

        let body_ty = if had_error {
            MonoType::Void
        } else {
            match self.synth_expr(body) {
                Ok(ty) => ty,
                Err(()) => {
                    self.local_env.pop_scope();
                    return Err(());
                }
            }
        };
        self.local_env.pop_scope();

        if had_error {
            Err(())
        } else {
            Ok(MonoType::Vector(Box::new(body_ty)))
        }
    }

    fn synth_collect_while(
        &mut self,
        cond: &Expr,
        body: &Expr,
        _span: Span,
    ) -> Result<MonoType, ()> {
        self.check_expr(cond, &MonoType::Bool)?;
        let body_ty = self.synth_expr(body)?;
        Ok(MonoType::Vector(Box::new(body_ty)))
    }
}

// ---------------------------------------------------------------------------
// Generic substitution helpers
// ---------------------------------------------------------------------------

/// Build a substitution map from type parameter names to concrete type arguments.
pub fn build_type_subst(type_params: &[String], args: &[MonoType]) -> HashMap<String, MonoType> {
    type_params
        .iter()
        .zip(args.iter())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Apply a substitution map to a type, replacing all Var occurrences.
pub fn apply_subst(ty: &MonoType, subst: &HashMap<String, MonoType>) -> MonoType {
    match ty {
        MonoType::Var(name) => subst.get(name).cloned().unwrap_or_else(|| ty.clone()),
        MonoType::Vector(elem) => MonoType::Vector(Box::new(apply_subst(elem, subst))),
        MonoType::Dict(k, v) => MonoType::Dict(
            Box::new(apply_subst(k, subst)),
            Box::new(apply_subst(v, subst)),
        ),
        MonoType::Function { params, ret } => MonoType::Function {
            params: params.iter().map(|p| apply_subst(p, subst)).collect(),
            ret: Box::new(apply_subst(ret, subst)),
        },
        MonoType::Named { type_id, args } => MonoType::Named {
            type_id: *type_id,
            args: args.iter().map(|a| apply_subst(a, subst)).collect(),
        },
        other => other.clone(),
    }
}
