use crate::module::artifacts::TypedModule;
use crate::syntax::ast::{
    BinOp, Block, Expr, ExprId, ExprKind, FunctionDecl, Item, Literal, Pattern, SourceFile,
    Stmt, StringPart, Type as AstType, UnOp,
};
use crate::syntax::span::Span;
use super::env::{LocalEnv, TypeEnv, ValueEnv};
use super::error::TypeError;
use super::patterns::PatternChecker;
use super::ty::{MonoType, OPTION_TYPE_ID, RESULT_TYPE_ID, CELL_TYPE_ID, RANGE_TYPE_ID, ITERATOR_TYPE_ID, ITER_ITEM_TYPE_ID, UNFOLD_STEP_TYPE_ID};
use super::type_map::TypeMap;
use std::collections::{HashSet, HashMap};

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
        };

        // Pass 1: Check all top-level lets and add to ValueEnv
        // This makes them available to all functions
        for item in &ast.items {
            if let Item::Stmt(stmt) = item {
                if let Stmt::Let { pattern, ty, value, span, .. } = stmt {
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
                            match checker.synth_expr(value) {
                                Ok(t) => t,
                                Err(()) => continue, // Error already recorded
                            }
                        };

                        // Add to ValueEnv so it's accessible from functions
                        checker.value_env.add_value(name.clone(), value_ty);
                    } else {
                        checker.errors.push(TypeError::UnsupportedFeature {
                            feature: "pattern matching in top-level let bindings",
                            span: *span,
                            note: "Only simple identifiers are supported for top-level lets".to_string(),
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
                    // Update the function signature with the inferred return type
                    let mut updated_sig = sig.clone();
                    updated_sig.ret = Some(body_ty);
                    self.value_env.update_function(updated_sig);
                }
                Err(()) => {
                    // Type checking failed, can't infer return type
                }
            }
        }

        // Clean up
        self.current_function_ret = None;
        self.local_env.pop_scope();
        self.type_var_scope = saved_type_vars;
        self.at_module_scope = saved_module_scope;
    }

    /// Type-check a top-level statement that is not a let binding.
    /// Allows for-loops, expression statements, break, continue, and return.
    fn check_top_level_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(e) => {
                let _ = self.synth_expr(e);
            }
            Stmt::For { pattern, index_pattern, iter, body, .. } => {
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
            Stmt::Let { .. } => {
                // Should not happen here; handled in Pass 1
            }
        }
    }

    fn synth_function_expr(&mut self, fe: &crate::syntax::ast::FunctionExpr, span: Span) -> Result<MonoType, ()> {
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
                        note: "All lambda parameters must have type annotations in Stage 5".to_string(),
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
                // Look up in local environment first, then value environment
                if let Some(ty) = self.local_env.lookup(name) {
                    Ok(ty.clone())
                } else if let Some(ty) = self.value_env.lookup(name) {
                    Ok(ty)
                } else {
                    self.errors.push(TypeError::UndefinedVariable {
                        name: name.clone(),
                        span: expr.span,
                    });
                    Err(())
                }
            }

            ExprKind::Binary { op, left, right } => {
                self.synth_binary(*op, left, right, expr.span)
            }

            ExprKind::Unary { op, expr: inner } => {
                self.synth_unary(*op, inner, expr.span)
            }

            ExprKind::Call { callee, args } => {
                self.synth_call(callee, args, expr.span)
            }

            ExprKind::Block(block) => self.synth_block(block),

            ExprKind::If { cond, then_branch, else_branch } => {
                self.synth_if(cond, then_branch, else_branch.as_deref(), expr.span)
            }

            ExprKind::FieldAccess { base, field } => {
                if let ExprKind::Ident(alias) = &base.kind {
                    if self.module_aliases.contains(alias.as_str()) {
                        let qualified = format!("{}.{}", alias, field);
                        // If it's a plain pub value (not a function), synthesize it directly
                        if let Some(ty) = self.value_env.lookup(&qualified) {
                            if !matches!(ty, MonoType::Function { .. }) {
                                self.type_map.set_expr_type(expr.id, ty.clone());
                                return Ok(ty);
                            }
                        }
                        // Otherwise it's a function ref: require a type annotation
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

            ExprKind::Index { base, index } => {
                self.synth_index(base, index, expr.span)
            }

            ExprKind::Array { elements } => {
                self.synth_array(elements, expr.span)
            }

            ExprKind::RecordLit { name, fields } => {
                self.synth_record_lit(name.as_deref(), fields, expr.span)
            }

            ExprKind::VariantLit { name, fields } => {
                self.synth_variant_lit(name, fields, expr.span)
            }

            ExprKind::Case { scrutinee, arms } => {
                self.synth_case(scrutinee, arms, expr.span)
            }

            ExprKind::StringInterpolation { parts } => {
                // Type-check each interpolated expression
                for part in parts {
                    if let StringPart::Interpolation(e) = part {
                        // Any type is ok, will be stringified
                        let _ = self.synth_expr(e);
                    }
                }
                Ok(MonoType::String)
            }

            ExprKind::Function(fe) => {
                self.synth_function_expr(fe, expr.span)
            }

            ExprKind::Collect { pattern, iter, body } => {
                self.synth_collect(pattern, iter, body, expr.span)
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
            ExprKind::Block(block) => {
                self.check_block(block, expected)
            }

            // If expressions: check both branches against expected type
            ExprKind::If { cond, then_branch, else_branch } => {
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
                let is_primitive_match = matches!(scrut_ty, MonoType::Int | MonoType::Bool | MonoType::String);
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
                    &self.type_env, &mut self.errors, &scrut_ty, arms, expr.span,
                )?;
                for arm in arms {
                    self.local_env.push_scope();
                    let mut pc = PatternChecker::new(
                        &self.type_env, &mut self.local_env, &mut self.errors,
                    );
                    pc.check_pattern(&arm.pattern, &scrut_ty)?;
                    drop(pc);
                    self.check_expr(&arm.body, expected)?;
                    self.local_env.pop_scope();
                }
                Ok(())
            }

            // Lambda: use expected Function type to supply unannotated param types
            ExprKind::Function(fe) => {
                if let MonoType::Function { params: expected_params, ret: expected_ret } = expected {
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
                        let ty = match &p.ty {
                            Some(ann) => self.resolve_type(ann)?,
                            None => exp_ty.clone(),
                        };
                        param_types.push(ty);
                    }
                    self.local_env.push_scope();
                    for (p, ty) in fe.params.iter().zip(&param_types) {
                        self.local_env.bind(p.name.clone(), ty.clone());
                    }
                    let saved = self.current_function_ret.take();
                    self.current_function_ret = Some(*expected_ret.clone());
                    let saved_scope = std::mem::replace(&mut self.at_module_scope, false);
                    let result = self.check_expr(&fe.body, expected_ret);
                    self.local_env.pop_scope();
                    self.current_function_ret = saved;
                    self.at_module_scope = saved_scope;
                    result
                } else {
                    let actual = self.synth_expr(expr)?;
                    self.unify(&actual, expected, expr.span)
                }
            }

            // Array literals: check each element against the expected element type
            ExprKind::Array { elements } => {
                if let MonoType::Array(elem_ty) = expected {
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
            ExprKind::Call { callee, args }
                if args.is_empty() =>
            {
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

            // First-class module method reference: Array.len, String.concat, etc.
            ExprKind::FieldAccess { base, field } => {
                if let ExprKind::Ident(alias) = &base.kind {
                    if self.module_aliases.contains(alias.as_str()) {
                        let alias = alias.clone();
                        let field = field.clone();
                        return self.check_module_func_ref(&alias, &field, expected, expr.id, expr.span);
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

    fn synth_binary(&mut self, op: BinOp, left: &Expr, right: &Expr, span: Span) -> Result<MonoType, ()> {
        match op {
            // Arithmetic: Int × Int → Int, Float × Float → Float
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                let left_ty = self.synth_expr(left)?;
                let right_ty = self.synth_expr(right)?;

                match (&left_ty, &right_ty) {
                    (MonoType::Int, MonoType::Int) => Ok(MonoType::Int),
                    (MonoType::Float, MonoType::Float) => Ok(MonoType::Float),
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
            BinOp::Assign => {
                self.synth_assign(left, right, span)
            }

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
        if let ExprKind::FieldAccess { base, field: method_name } = &callee.kind {
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
                    if let Some(variant_idx) = self.type_env.get_variant_index(type_id, method_name) {
                        // Build named_ty with Var args for generic types
                        let type_var_args: Vec<MonoType> = self.type_env.get_def(type_id)
                            .map(|d| d.type_params().iter().map(|p| MonoType::Var(p.clone())).collect())
                            .unwrap_or_default();
                        let named_ty = MonoType::Named { type_id, args: type_var_args };
                        self.type_map.set_expr_type(base.id, named_ty.clone());
                        let variants = self.type_env.get_variants(type_id)
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
                        // For generic types, use substitution to infer concrete type args
                        if variant_fields.iter().any(type_contains_var) {
                            let mut subst = HashMap::new();
                            let mut arg_tys = Vec::new();
                            for arg in args.iter() {
                                arg_tys.push(self.synth_expr(arg)?);
                            }
                            let mut ok = true;
                            for (idx, (field_ty, arg_ty)) in variant_fields.iter().zip(arg_tys.iter()).enumerate() {
                                if !collect_subst(field_ty, arg_ty, &mut subst) {
                                    self.errors.push(TypeError::TypeMismatch {
                                        expected: apply_subst(field_ty, &subst),
                                        actual: arg_ty.clone(),
                                        span,
                                        note: Some(format!("argument {} of variant constructor", idx + 1)),
                                    });
                                    ok = false;
                                }
                            }
                            if !ok { return Err(()); }
                            let concrete_named_ty = apply_subst(&named_ty, &subst);
                            let ctor_ty = MonoType::Function {
                                params: variant_fields,
                                ret: Box::new(named_ty),
                            };
                            self.type_map.set_expr_type(callee.id, apply_subst(&ctor_ty, &subst));
                            return Ok(concrete_named_ty);
                        }
                        // Non-generic: check each arg directly
                        for (arg, expected_ty) in args.iter().zip(variant_fields.iter()) {
                            self.check_expr(arg, expected_ty)?;
                        }
                        // Record callee type (constructor function)
                        let ctor_ty = if variant_fields.is_empty() {
                            named_ty.clone()
                        } else {
                            MonoType::Function { params: variant_fields, ret: Box::new(named_ty.clone()) }
                        };
                        self.type_map.set_expr_type(callee.id, ctor_ty);
                        return Ok(named_ty);
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

                // Generic call: params contain type variables — use substitution
                if params.iter().any(type_contains_var) {
                    let mut subst = HashMap::new();
                    let mut ok = true;
                    for arg in args.iter() {
                        let _ = self.synth_expr(arg)?;
                    }
                    let arg_tys: Vec<MonoType> = args.iter()
                        .filter_map(|a| self.type_map.get_expr_type(a.id).cloned())
                        .collect();
                    let callee_label = if let ExprKind::Ident(n) = &callee.kind {
                        format!(" of call to `{}`", n)
                    } else {
                        String::new()
                    };
                    for (idx, (param_ty, arg_ty)) in params.iter().zip(&arg_tys).enumerate() {
                        if !collect_subst(param_ty, arg_ty, &mut subst) {
                            self.errors.push(TypeError::TypeMismatch {
                                expected: apply_subst(param_ty, &subst),
                                actual: arg_ty.clone(),
                                span,
                                note: Some(format!("argument {}{}", idx + 1, callee_label)),
                            });
                            ok = false;
                        }
                    }
                    if !ok { return Err(()); }
                    return Ok(apply_subst(&ret, &subst));
                }

                // Check each argument; on failure, patch in call context
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

                Ok(*ret)
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
        // Special: Cell, Dict, Iterator, Array, and String modules provide polymorphic operations.
        if alias == "Cell" {
            return self.synth_cell_call(func_name, args, span);
        }
        if alias == "Dict" {
            return self.synth_dict_module_call(func_name, args, span);
        }
        if alias == "Iterator" {
            return self.synth_iterator_call(func_name, args, span);
        }
        if alias == "Array" {
            return self.synth_array_call(func_name, args, span);
        }
        if alias == "String" {
            return self.synth_string_call(func_name, args, span);
        }

        let qualified = format!("{}.{}", alias, func_name);
        match self.value_env.lookup(&qualified) {
            Some(MonoType::Function { params, ret }) => {
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
                Ok(*ret)
            }
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
    fn synth_cell_call(&mut self, func_name: &str, args: &[Expr], span: Span) -> Result<MonoType, ()> {
        match func_name {
            "new" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
                    return Err(());
                }
                let inner = self.synth_expr(&args[0])?;
                Ok(MonoType::Named { type_id: CELL_TYPE_ID, args: vec![inner] })
            }
            "get" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
                    return Err(());
                }
                let cell_ty = self.synth_expr(&args[0])?;
                match cell_ty {
                    MonoType::Named { type_id, args: cell_args } if type_id == CELL_TYPE_ID => {
                        Ok(cell_args.into_iter().next().unwrap_or(MonoType::Void))
                    }
                    other => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Named { type_id: CELL_TYPE_ID, args: vec![] },
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
                    self.errors.push(TypeError::WrongArity { expected: 2, actual: args.len(), span });
                    return Err(());
                }
                let cell_ty = self.synth_expr(&args[0])?;
                match cell_ty {
                    MonoType::Named { type_id, args: cell_args } if type_id == CELL_TYPE_ID => {
                        let inner = cell_args.into_iter().next().unwrap_or(MonoType::Void);
                        self.check_expr(&args[1], &inner)?;
                        Ok(MonoType::Void)
                    }
                    other => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Named { type_id: CELL_TYPE_ID, args: vec![] },
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
                    self.errors.push(TypeError::WrongArity { expected: 2, actual: args.len(), span });
                    return Err(());
                }
                let cell_ty = self.synth_expr(&args[0])?;
                match cell_ty {
                    MonoType::Named { type_id, args: cell_args } if type_id == CELL_TYPE_ID => {
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
                            expected: MonoType::Named { type_id: CELL_TYPE_ID, args: vec![] },
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
    fn synth_dict_module_call(&mut self, func_name: &str, args: &[Expr], span: Span) -> Result<MonoType, ()> {
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
                    self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
                    return Err(());
                }
                let dict_ty = self.synth_expr(&args[0])?;
                if !matches!(dict_ty, MonoType::Dict(_, _)) {
                    self.errors.push(TypeError::TypeMismatch {
                        expected: MonoType::Dict(Box::new(MonoType::Void), Box::new(MonoType::Void)),
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
                    self.errors.push(TypeError::WrongArity { expected: 2, actual: args.len(), span });
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
                            expected: MonoType::Dict(Box::new(MonoType::Void), Box::new(MonoType::Void)),
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
                    self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
                    return Err(());
                }
                let dict_ty = self.synth_expr(&args[0])?;
                match dict_ty {
                    MonoType::Dict(k_ty, _) => Ok(MonoType::Array(k_ty)),
                    other => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Dict(Box::new(MonoType::Void), Box::new(MonoType::Void)),
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
                    self.errors.push(TypeError::WrongArity { expected: 2, actual: args.len(), span });
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
                            expected: MonoType::Dict(Box::new(MonoType::Void), Box::new(MonoType::Void)),
                            actual: other,
                            span,
                            note: Some("Dict.remove expects Dict<K,V> as first argument".to_string()),
                        });
                        Err(())
                    }
                }
            }
            other => {
                self.errors.push(TypeError::UndefinedVariable {
                    name: format!("Dict.{}", other),
                    span,
                });
                Err(())
            }
        }
    }

    /// Handle Iterator.next / Iterator.unfold polymorphically.
    fn synth_iterator_call(&mut self, func_name: &str, args: &[Expr], span: Span) -> Result<MonoType, ()> {
        match func_name {
            "next" => {
                // Iterator.next(it: Iterator<T>) Option<IterItem<T>>
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
                    return Err(());
                }
                let it_ty = self.synth_expr(&args[0])?;
                match it_ty {
                    MonoType::Named { type_id, args: ref it_args } if type_id == ITERATOR_TYPE_ID => {
                        let elem_ty = it_args.first().cloned().unwrap_or(MonoType::Void);
                        let item_ty = MonoType::Named {
                            type_id: ITER_ITEM_TYPE_ID,
                            args: vec![elem_ty],
                        };
                        Ok(MonoType::Named { type_id: OPTION_TYPE_ID, args: vec![item_ty] })
                    }
                    other => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Named { type_id: ITERATOR_TYPE_ID, args: vec![] },
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
                    self.errors.push(TypeError::WrongArity { expected: 2, actual: args.len(), span });
                    return Err(());
                }
                // Synthesize seed to determine S
                let seed_ty = self.synth_expr(&args[0])?;
                // Synthesize the step closure
                let step_ty = self.synth_expr(&args[1])?;
                // Validate: step must be fn(S) UnfoldStep<T,S>
                match step_ty {
                    MonoType::Function { ref params, ref ret } => {
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
                            MonoType::Named { type_id, args: ret_args } if *type_id == UNFOLD_STEP_TYPE_ID => {
                                let elem_ty = ret_args.first().cloned().unwrap_or(MonoType::Void);
                                Ok(MonoType::Named { type_id: ITERATOR_TYPE_ID, args: vec![elem_ty] })
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
                        self.errors.push(TypeError::NotAFunction { ty: other, span });
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

    /// Handle Array.method(arr, ...) module-qualified calls.
    fn synth_array_call(&mut self, func_name: &str, args: &[Expr], span: Span) -> Result<MonoType, ()> {
        match func_name {
            "len" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
                    return Err(());
                }
                let arr_ty = self.synth_expr(&args[0])?;
                if !matches!(arr_ty, MonoType::Array(_)) {
                    self.errors.push(TypeError::TypeMismatch {
                        expected: MonoType::Array(Box::new(MonoType::Void)),
                        actual: arr_ty,
                        span,
                        note: Some("Array.len expects Array<T> as first argument".to_string()),
                    });
                    return Err(());
                }
                Ok(MonoType::Int)
            }
            "append" => {
                if args.len() != 2 {
                    self.errors.push(TypeError::WrongArity { expected: 2, actual: args.len(), span });
                    return Err(());
                }
                let arr_ty = self.synth_expr(&args[0])?;
                match arr_ty {
                    MonoType::Array(ref elem_ty) => {
                        let elem_ty = *elem_ty.clone();
                        self.check_expr(&args[1], &elem_ty)?;
                        Ok(arr_ty)
                    }
                    other => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Array(Box::new(MonoType::Void)),
                            actual: other,
                            span,
                            note: Some("Array.append expects Array<T> as first argument".to_string()),
                        });
                        Err(())
                    }
                }
            }
            "concat" => {
                if args.len() != 2 {
                    self.errors.push(TypeError::WrongArity { expected: 2, actual: args.len(), span });
                    return Err(());
                }
                let arr_ty = self.synth_expr(&args[0])?;
                if !matches!(arr_ty, MonoType::Array(_)) {
                    self.errors.push(TypeError::TypeMismatch {
                        expected: MonoType::Array(Box::new(MonoType::Void)),
                        actual: arr_ty,
                        span,
                        note: Some("Array.concat expects Array<T> as first argument".to_string()),
                    });
                    return Err(());
                }
                self.check_expr(&args[1], &arr_ty)?;
                Ok(arr_ty)
            }
            "slice" => {
                if args.len() != 3 {
                    self.errors.push(TypeError::WrongArity { expected: 3, actual: args.len(), span });
                    return Err(());
                }
                let arr_ty = self.synth_expr(&args[0])?;
                if !matches!(arr_ty, MonoType::Array(_)) {
                    self.errors.push(TypeError::TypeMismatch {
                        expected: MonoType::Array(Box::new(MonoType::Void)),
                        actual: arr_ty,
                        span,
                        note: Some("Array.slice expects Array<T> as first argument".to_string()),
                    });
                    return Err(());
                }
                self.check_expr(&args[1], &MonoType::Int)?;
                self.check_expr(&args[2], &MonoType::Int)?;
                Ok(arr_ty)
            }
            _ => {
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "unknown Array method",
                    span,
                    note: format!("Array has no method '{}'", func_name),
                });
                Err(())
            }
        }
    }

    /// Handle String.method(s, ...) module-qualified calls.
    fn synth_string_call(&mut self, func_name: &str, args: &[Expr], span: Span) -> Result<MonoType, ()> {
        match func_name {
            "len" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
                    return Err(());
                }
                self.check_expr(&args[0], &MonoType::String)?;
                Ok(MonoType::Int)
            }
            "concat" => {
                if args.len() != 2 {
                    self.errors.push(TypeError::WrongArity { expected: 2, actual: args.len(), span });
                    return Err(());
                }
                self.check_expr(&args[0], &MonoType::String)?;
                self.check_expr(&args[1], &MonoType::String)?;
                Ok(MonoType::String)
            }
            "substring" => {
                if args.len() != 3 {
                    self.errors.push(TypeError::WrongArity { expected: 3, actual: args.len(), span });
                    return Err(());
                }
                self.check_expr(&args[0], &MonoType::String)?;
                self.check_expr(&args[1], &MonoType::Int)?;
                self.check_expr(&args[2], &MonoType::Int)?;
                Ok(MonoType::String)
            }
            "to_string" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
                    return Err(());
                }
                self.check_expr(&args[0], &MonoType::String)?;
                Ok(MonoType::String)
            }
            _ => {
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "unknown String method",
                    span,
                    note: format!("String has no method '{}'", func_name),
                });
                Err(())
            }
        }
    }

    /// Validate a first-class module method reference (e.g. `Array.len`) in check mode.
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
                    actual: MonoType::Function { params: vec![], ret: Box::new(MonoType::Void) },
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

        // Validate the first param matches the base type and check the shape
        let valid = match alias {
            "Array" => {
                if !matches!(params[0], MonoType::Array(_)) {
                    false
                } else {
                    match (method, params.len()) {
                        ("len", 1) => matches!(ret, MonoType::Int),
                        ("append", 2) => matches!(ret, MonoType::Array(_)),
                        ("concat", 2) => matches!(ret, MonoType::Array(_)),
                        ("slice", 3) => matches!(ret, MonoType::Array(_)),
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
                        ("keys", 1) => matches!(ret, MonoType::Array(_)),
                        ("remove", 2) => matches!(ret, MonoType::Dict(_, _)),
                        _ => false,
                    }
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
                actual: MonoType::Function { params: vec![], ret: Box::new(MonoType::Void) },
                span,
                note: Some(format!("'{}.{}' signature does not match annotation", alias, method)),
            });
            Err(())
        }
    }

    /// Handle method calls: `receiver.method(args)`.
    /// Dispatches to builtin methods (Array, String, primitives) or user-defined
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
            MonoType::Array(ref elem_ty) => {
                let elem_ty = *elem_ty.clone();
                match method {
                    "len" => {
                        if !args.is_empty() {
                            self.errors.push(TypeError::WrongArity { expected: 0, actual: args.len(), span });
                            return Err(());
                        }
                        Ok(MonoType::Int)
                    }
                    "append" => {
                        if args.len() != 1 {
                            self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
                            return Err(());
                        }
                        self.check_expr(&args[0], &elem_ty)?;
                        Ok(base_ty)
                    }
                    "concat" => {
                        if args.len() != 1 {
                            self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
                            return Err(());
                        }
                        self.check_expr(&args[0], &base_ty)?;
                        Ok(base_ty)
                    }
                    "slice" => {
                        if args.len() != 2 {
                            self.errors.push(TypeError::WrongArity { expected: 2, actual: args.len(), span });
                            return Err(());
                        }
                        self.check_expr(&args[0], &MonoType::Int)?;
                        self.check_expr(&args[1], &MonoType::Int)?;
                        Ok(base_ty)
                    }
                    _ => {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "unknown array method",
                            span,
                            note: format!("Array has no method '{}'", method),
                        });
                        Err(())
                    }
                }
            }
            MonoType::String => match method {
                "len" => {
                    if !args.is_empty() {
                        self.errors.push(TypeError::WrongArity { expected: 0, actual: args.len(), span });
                        return Err(());
                    }
                    Ok(MonoType::Int)
                }
                "concat" => {
                    if args.len() != 1 {
                        self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
                        return Err(());
                    }
                    self.check_expr(&args[0], &MonoType::String)?;
                    Ok(MonoType::String)
                }
                "to_string" => {
                    if !args.is_empty() {
                        self.errors.push(TypeError::WrongArity { expected: 0, actual: args.len(), span });
                        return Err(());
                    }
                    Ok(MonoType::String)
                }
                "substring" => {
                    if args.len() != 2 {
                        self.errors.push(TypeError::WrongArity { expected: 2, actual: args.len(), span });
                        return Err(());
                    }
                    self.check_expr(&args[0], &MonoType::Int)?;
                    self.check_expr(&args[1], &MonoType::Int)?;
                    Ok(MonoType::String)
                }
                _ => {
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
                        self.errors.push(TypeError::WrongArity { expected: 0, actual: args.len(), span });
                        return Err(());
                    }
                    Ok(MonoType::Array(k_ty))
                }
                "len" => {
                    if !args.is_empty() {
                        self.errors.push(TypeError::WrongArity { expected: 0, actual: args.len(), span });
                        return Err(());
                    }
                    Ok(MonoType::Int)
                }
                "has" => {
                    if args.len() != 1 {
                        self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
                        return Err(());
                    }
                    self.check_expr(&args[0], &k_ty)?;
                    Ok(MonoType::Bool)
                }
                "remove" => {
                    if args.len() != 1 {
                        self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
                        return Err(());
                    }
                    self.check_expr(&args[0], &k_ty)?;
                    Ok(MonoType::Dict(k_ty, v_ty))
                }
                _ => {
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "unknown dict method",
                        span,
                        note: format!("Dict has no method '{}'", method),
                    });
                    Err(())
                }
            },
            MonoType::Int | MonoType::Float | MonoType::Bool => {
                if method == "to_string" {
                    if !args.is_empty() {
                        self.errors.push(TypeError::WrongArity { expected: 0, actual: args.len(), span });
                        return Err(());
                    }
                    Ok(MonoType::String)
                } else {
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "method on primitive type",
                        span,
                        note: format!("Type {:?} has no method '{}'", base_ty, method),
                    });
                    Err(())
                }
            }
            MonoType::Named { type_id, args: ref cell_args } if type_id == CELL_TYPE_ID => {
                let inner = cell_args.first().cloned().unwrap_or(MonoType::Void);
                match method {
                    "get" => {
                        if !args.is_empty() {
                            self.errors.push(TypeError::WrongArity { expected: 0, actual: args.len(), span });
                            return Err(());
                        }
                        Ok(inner)
                    }
                    "set" => {
                        if args.len() != 1 {
                            self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
                            return Err(());
                        }
                        self.check_expr(&args[0], &inner)?;
                        Ok(MonoType::Void)
                    }
                    "update" => {
                        if args.len() != 1 {
                            self.errors.push(TypeError::WrongArity { expected: 1, actual: args.len(), span });
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
                            note: format!("Cell has no method '{}'; available: get, set, update", method),
                        });
                        Err(())
                    }
                }
            }
            MonoType::Named { type_id, args: named_args } => {
                // Look up user-defined inherent method
                if let Some(func_name) = self.type_env.get_method_function(type_id, method).cloned() {
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
                        // Check receiver
                        if let Some(recv_ty) = sig.params.first() {
                            self.check_expr(base, recv_ty)?;
                        }
                        // Check remaining args
                        for (arg, expected_ty) in args.iter().zip(sig.params.iter().skip(1)) {
                            self.check_expr(arg, expected_ty)?;
                        }
                        return Ok(sig.ret.unwrap_or(MonoType::Void));
                    }
                }

                // No inherent method — check if it's a function-typed record field
                // (capability record call: `record.fn_field(args)`)
                // Apply type-arg substitution for generic capability records
                if let Some(field_idx) = self.type_env.get_field_index(type_id, method) {
                    if let Some(fields) = self.type_env.get_record_fields(type_id) {
                        let type_params = self.type_env.get_def(type_id)
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
                let type_name = self.type_env.get_def(type_id)
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
                Stmt::Let { pattern, ty, value, span: _, .. } => {
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
                    result_ty = MonoType::Void;
                }
                Stmt::For { pattern, index_pattern, iter, body, .. } => {
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

        for (i, stmt) in block.stmts.iter().enumerate() {
            match stmt {
                Stmt::Let { pattern, ty, value, span: _, .. } => {
                    self.check_let_stmt(pattern, ty.as_ref(), value);
                }
                Stmt::Expr(e) => {
                    if last_expr_idx == Some(i) {
                        // Final expression — check against expected return type
                        self.check_expr(e, expected_ty)?;
                    } else {
                        let _ = self.synth_expr(e);
                    }
                }
                Stmt::Return { value, span } => {
                    if let Some(ret_ty) = self.current_function_ret.clone() {
                        if let Some(val) = value {
                            let _ = self.check_expr(val, &ret_ty);
                        } else {
                            let _ = self.unify(&MonoType::Void, &ret_ty, *span);
                        }
                    }
                }
                Stmt::For { pattern, index_pattern, iter, body, .. } => {
                    self.check_for_stmt(pattern, index_pattern.as_ref(), iter, body);
                }
                Stmt::ForCond { cond, body, .. } => {
                    let _ = self.check_expr(cond, &MonoType::Bool);
                    let _ = self.synth_block(body);
                }
                Stmt::Break { value, .. } => {
                    if let Some(val) = value {
                        let _ = self.synth_expr(val);
                    }
                }
                Stmt::Continue { .. } => {}
            }
        }

        // If there's no final Expr stmt, the block type is Void
        if last_expr_idx.is_none() {
            let _ = self.unify(&MonoType::Void, expected_ty, block.span);
        }

        self.local_env.pop_scope();
        Ok(())
    }

    //
    // Let statements
    //

    fn check_let_stmt(&mut self, pattern: &Pattern, ty: Option<&crate::syntax::ast::Type>, value: &Expr) {
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
                    match self.synth_expr(value) {
                        Ok(t) => t,
                        Err(()) => return,
                    }
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
                    note: "Only simple identifiers are supported in let bindings for now".to_string(),
                });
            }
        }
    }

    //
    // If expressions
    //

    fn synth_if(&mut self, cond: &Expr, then_branch: &Expr, else_branch: Option<&Expr>, _span: Span) -> Result<MonoType, ()> {
        // Condition must be Bool
        self.check_expr(cond, &MonoType::Bool)?;

        // Synthesize then branch type
        let then_ty = self.synth_expr(then_branch)?;

        // If there's an else branch, both branches must have the same type
        if let Some(else_expr) = else_branch {
            let else_ty = self.synth_expr(else_expr)?;
            self.unify(&then_ty, &else_ty, else_expr.span)?;
            // If one branch diverges (Never), use the other branch's type
            if then_ty == MonoType::Never { Ok(else_ty) } else { Ok(then_ty) }
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
                    // For generic types, build named_ty with Var args so the generic
                    // call path in synth_call can infer concrete type args via collect_subst
                    let type_var_args: Vec<MonoType> = self.type_env.get_def(type_id)
                        .map(|d| d.type_params().iter().map(|p| MonoType::Var(p.clone())).collect())
                        .unwrap_or_default();
                    let named_ty = MonoType::Named { type_id, args: type_var_args };
                    // Record type of the type-name base as Named (so lowerer can identify it)
                    self.type_map.set_expr_type(base.id, named_ty.clone());
                    let variants = self.type_env.get_variants(type_id)
                        .expect("variant index exists, variants must exist");
                    let variant_fields = variants[variant_idx].fields.clone();
                    return if variant_fields.is_empty() {
                        // Zero-arg variant — directly a value of the named type
                        Ok(named_ty)
                    } else {
                        // Parameterized variant — a constructor function
                        Ok(MonoType::Function { params: variant_fields, ret: Box::new(named_ty) })
                    };
                }
            }
        }

        let base_ty = self.synth_expr(base)?;

        match base_ty {
            MonoType::Named { type_id, args: ref type_args } => {
                // Check for field/method collision
                let has_field = self.type_env.has_field(type_id, field);
                let has_method = self.type_env.has_method(type_id, field);

                if has_field && has_method {
                    let type_name = self.type_env.get_def(type_id)
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
                    let type_params = self.type_env.get_def(type_id)
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
                        // TODO: This is a method call, but we're treating it as field access
                        // For now, return an error. Full method support comes in Stage 3.
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "inherent method calls",
                            span,
                            note: format!("Method '{}' exists but method calls are not yet fully implemented", field),
                        });
                        return Err(());
                    }

                    // Neither field nor method
                    let record_name = self.type_env.get_def(type_id)
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
    // Array indexing
    //

    fn synth_index(&mut self, base: &Expr, index: &Expr, _span: Span) -> Result<MonoType, ()> {
        let base_ty = self.synth_expr(base)?;

        match base_ty {
            MonoType::Array(elem_ty) => {
                self.check_expr(index, &MonoType::Int)?;
                Ok(*elem_ty)
            }
            MonoType::String => {
                self.check_expr(index, &MonoType::Int)?;
                Ok(MonoType::String) // String indexing returns a single-char String
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
                    expected: MonoType::Array(Box::new(MonoType::Int)), // Dummy
                    actual: base_ty,
                    span: base.span,
                    note: None,
                });
                Err(())
            }
        }
    }

    //
    // Array literals
    //

    fn synth_array(&mut self, elements: &[Expr], span: Span) -> Result<MonoType, ()> {
        if elements.is_empty() {
            // Empty array - we can't infer the type
            // For now, error - require type annotation
            self.errors.push(TypeError::UnsupportedFeature {
                feature: "empty array literals",
                span,
                note: "Empty arrays require type annotations (not yet supported)".to_string(),
            });
            return Err(());
        }

        // Infer type from first element
        let first_ty = self.synth_expr(&elements[0])?;

        // Check all other elements match
        for elem in &elements[1..] {
            self.check_expr(elem, &first_ty)?;
        }

        Ok(MonoType::Array(Box::new(first_ty)))
    }

    //
    // Record literals
    //

    fn synth_record_lit(&mut self, name: Option<&str>, fields: &[(String, Expr)], span: Span) -> Result<MonoType, ()> {
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

            let type_params = self.type_env.get_def(type_id)
                .map(|d| d.type_params().to_vec())
                .unwrap_or_default();

            if type_params.is_empty() {
                // Non-generic: check fields directly
                self.check_record_lit_fields(type_id, &[], fields, span)?;
                Ok(MonoType::named(type_id))
            } else {
                // Generic: synth each field once to infer type args, then validate.
                // Single-pass to avoid double-evaluation: we collect (name, actual_ty) from
                // synth, then unify against the substituted declared types.
                let def_fields: Vec<(String, MonoType)> = self.type_env.get_record_fields(type_id)
                    .map(|fs| fs.iter().map(|f| (f.name.clone(), f.ty.clone())).collect())
                    .unwrap_or_default();

                // Synth pass: collect actual types and build substitution
                let mut subst = HashMap::new();
                let mut field_synth: Vec<(&str, Result<MonoType, ()>)> = Vec::new();
                for (provided_name, provided_expr) in fields.iter() {
                    let result = self.synth_expr(provided_expr);
                    if let Ok(actual_ty) = &result {
                        if let Some((_, declared_ty)) = def_fields.iter().find(|(n, _)| n == provided_name) {
                            collect_subst(declared_ty, actual_ty, &mut subst);
                        }
                    }
                    field_synth.push((provided_name.as_str(), result));
                }

                let type_args: Vec<MonoType> = type_params.iter()
                    .map(|p| subst.get(p).cloned().unwrap_or_else(|| MonoType::Var(p.clone())))
                    .collect();
                let subst2 = build_type_subst(&type_params, &type_args);

                let record_name = self.type_env.get_def(type_id)
                    .map(|d| d.name().to_string())
                    .unwrap_or_else(|| format!("Type#{}", type_id.0));

                // Check for extra (unknown) fields
                let expected_names: Vec<&str> = def_fields.iter().map(|(n, _)| n.as_str()).collect();
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
                    match field_synth.iter().find(|(n, _)| *n == expected_name.as_str()) {
                        Some((_, Ok(actual_ty))) => {
                            if self.unify(actual_ty, &concrete_ty, span).is_err() {
                                ok = false;
                            }
                        }
                        Some((_, Err(()))) => { ok = false; }
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

                if ok { Ok(MonoType::Named { type_id, args: type_args }) } else { Err(()) }
            }
        } else {
            // Anonymous record literal: .{ x: 1, y: 2 }
            // This requires expected type from context - error in synthesis mode
            self.errors.push(TypeError::AnonymousRecordWithoutContext { span });
            Err(())
        }
    }

    fn check_anon_record_lit(&mut self, fields: &[(String, Expr)], expected: &MonoType, span: Span) -> Result<(), ()> {
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

    fn check_record_lit_fields(&mut self, type_id: crate::types::ty::TypeId, type_args: &[MonoType], fields: &[(String, Expr)], span: Span) -> Result<(), ()> {
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
            let type_params = self.type_env.get_def(type_id)
                .map(|d| d.type_params().to_vec())
                .unwrap_or_default();
            build_type_subst(&type_params, type_args)
        };

        // Check all expected fields are present and have correct types
        // Apply substitution to declared field types for generic types
        let expected_fields_vec: Vec<_> = expected_fields.iter()
            .map(|f| (f.name.clone(), apply_subst(&f.ty, &subst)))
            .collect();

        let expected_names: Vec<String> = expected_fields_vec.iter()
            .map(|(name, _)| name.clone())
            .collect();

        for (field_name, field_ty) in &expected_fields_vec {
            let provided = fields.iter().find(|(name, _)| name == field_name);

            if let Some((_, value)) = provided {
                self.check_expr(value, field_ty)?;
            } else {
                // Missing field
                let record_name = self.type_env.get_def(type_id)
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
                let record_name = self.type_env.get_def(type_id)
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

    fn synth_variant_lit(&mut self, variant_name: &str, _fields: &[Expr], span: Span) -> Result<MonoType, ()> {
        // Variant literals require type context to disambiguate which sum type they belong to
        // Multiple sum types may have variants with the same name
        // Use checking mode with type annotation: `x: Option<Int> = .Some(42)`

        self.errors.push(TypeError::UnsupportedFeature {
            feature: "variant literals without type context",
            span,
            note: format!("Cannot infer type for variant .{}(...) - provide a type annotation", variant_name),
        });
        Err(())
    }

    fn check_variant_lit(&mut self, variant_name: &str, fields: &[Expr], expected: &MonoType, span: Span) -> Result<(), ()> {
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
                                "Ok"  => vec![args.first().cloned().unwrap_or(MonoType::Void)],
                                "Err" => vec![args.get(1).cloned().unwrap_or(MonoType::Void)],
                                _ => v.fields.clone(),
                            }
                        } else {
                            // User-defined generic sum type: apply type-arg substitution
                            let type_params = self.type_env.get_def(*type_id)
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
                        let sum_type_name = self.type_env
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

    fn synth_case(&mut self, scrutinee: &Expr, arms: &[crate::syntax::ast::CaseArm], span: Span) -> Result<MonoType, ()> {
        let scrut_ty = self.synth_expr(scrutinee)?;

        // Scrutinee must be a sum type or a matchable primitive (Int, Bool, String)
        let is_primitive_match = matches!(scrut_ty, MonoType::Int | MonoType::Bool | MonoType::String);
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

    fn synth_case_arm(&mut self, arm: &crate::syntax::ast::CaseArm, scrut_ty: &MonoType) -> Result<MonoType, ()> {
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
        // Never (bottom type) unifies with anything
        if *actual == MonoType::Never || *expected == MonoType::Never {
            return Ok(());
        }
        if actual == expected {
            return Ok(());
        }
        // Structural unification for Named types: Var(_) in either arg position
        // acts as a wildcard. This handles generic zero-arg variants like
        // `UnfoldStep.Done` which synthesize as Named(id, [Var("T"), Var("S")])
        // but need to match against a concrete Named(id, [Int, Int]).
        if let (
            MonoType::Named { type_id: aid, args: aa },
            MonoType::Named { type_id: eid, args: ea },
        ) = (actual, expected)
        {
            if aid == eid && aa.len() == ea.len() {
                let all_ok = aa.iter().zip(ea.iter()).all(|(a, e)| {
                    matches!(a, MonoType::Var(_)) || matches!(e, MonoType::Var(_)) || a == e
                });
                if all_ok {
                    return Ok(());
                }
            }
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
                    MonoType::Named { type_id, args: type_args } => {
                        if let Some(fields) = self.type_env.get_record_fields(type_id) {
                            let type_params = self.type_env.get_def(type_id)
                                .map(|d| d.type_params().to_vec())
                                .unwrap_or_default();
                            let subst = build_type_subst(&type_params, &type_args);
                            let field_ty = fields.iter().find(|f| f.name == *field)
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
                    MonoType::Array(elem_ty) => {
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
                            expected: MonoType::Array(Box::new(MonoType::Int)),
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
                    note: "Only identifiers, field accesses, and index expressions can be assigned".to_string(),
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
            MonoType::Array(elem) => {
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
                    expected: MonoType::Array(Box::new(MonoType::Int)),
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

    fn synth_collect(&mut self, pattern: &Pattern, iter: &Expr, body: &Expr, span: Span) -> Result<MonoType, ()> {
        let iter_ty = self.synth_expr(iter)?;

        let elem_ty = match iter_ty {
            MonoType::Array(elem) => *elem,
            MonoType::Named { type_id, .. } if type_id == RANGE_TYPE_ID => MonoType::Int,
            MonoType::Named { type_id, ref args } if type_id == ITERATOR_TYPE_ID => {
                args.first().cloned().unwrap_or(MonoType::Void)
            }
            other => {
                self.errors.push(TypeError::TypeMismatch {
                    expected: MonoType::Array(Box::new(MonoType::Int)),
                    actual: other,
                    span: iter.span,
                    note: None,
                });
                return Err(());
            }
        };

        self.local_env.push_scope();

        match pattern {
            Pattern::Ident(name, _) => self.local_env.bind(name.clone(), elem_ty),
            Pattern::Wildcard(_) => {}
            _ => {
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "complex pattern in collect",
                    span,
                    note: "Only simple identifiers are supported in collect patterns".to_string(),
                });
                self.local_env.pop_scope();
                return Err(());
            }
        }

        let body_ty = self.synth_expr(body)?;
        self.local_env.pop_scope();

        Ok(MonoType::Array(Box::new(body_ty)))
    }
}

// ---------------------------------------------------------------------------
// Generic substitution helpers
// ---------------------------------------------------------------------------

/// Unify `expected` (which may contain Var) against `actual` (concrete).
/// On success, adds substitutions to `subst`. Returns false on structural mismatch.
fn collect_subst(expected: &MonoType, actual: &MonoType, subst: &mut HashMap<String, MonoType>) -> bool {
    match (expected, actual) {
        (MonoType::Var(name), _) => {
            if let Some(existing) = subst.get(name) {
                existing == actual
            } else {
                subst.insert(name.clone(), actual.clone());
                true
            }
        }
        (MonoType::Array(a), MonoType::Array(b)) => collect_subst(a, b, subst),
        (MonoType::Dict(ak, av), MonoType::Dict(bk, bv)) => {
            collect_subst(ak, bk, subst) && collect_subst(av, bv, subst)
        }
        (MonoType::Function { params: p1, ret: r1 }, MonoType::Function { params: p2, ret: r2 }) => {
            p1.len() == p2.len()
                && p1.iter().zip(p2.iter()).all(|(a, b)| collect_subst(a, b, subst))
                && collect_subst(r1, r2, subst)
        }
        (MonoType::Named { type_id: id1, args: a1 }, MonoType::Named { type_id: id2, args: a2 }) => {
            id1 == id2
                && a1.len() == a2.len()
                && a1.iter().zip(a2.iter()).all(|(a, b)| collect_subst(a, b, subst))
        }
        (MonoType::Never, _) | (_, MonoType::Never) => true,
        (a, b) => a == b,
    }
}

/// Build a substitution map from type parameter names to concrete type arguments.
pub fn build_type_subst(type_params: &[String], args: &[MonoType]) -> HashMap<String, MonoType> {
    type_params.iter().zip(args.iter())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Apply a substitution map to a type, replacing all Var occurrences.
pub fn apply_subst(ty: &MonoType, subst: &HashMap<String, MonoType>) -> MonoType {
    match ty {
        MonoType::Var(name) => subst.get(name).cloned().unwrap_or_else(|| ty.clone()),
        MonoType::Array(elem) => MonoType::Array(Box::new(apply_subst(elem, subst))),
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

fn type_contains_var(ty: &MonoType) -> bool {
    match ty {
        MonoType::Var(_) => true,
        MonoType::Array(e) => type_contains_var(e),
        MonoType::Dict(k, v) => type_contains_var(k) || type_contains_var(v),
        MonoType::Function { params, ret } => {
            params.iter().any(type_contains_var) || type_contains_var(ret)
        }
        MonoType::Named { args, .. } => args.iter().any(type_contains_var),
        _ => false,
    }
}
