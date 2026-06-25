use super::env::{LocalEnv, TypeEnv, ValueEnv};
use super::error::TypeError;
use super::patterns::PatternChecker;
use super::ty::{
    CELL_TYPE_ID, CHANNEL_TYPE_ID, ITER_ITEM_TYPE_ID, ITERATOR_TYPE_ID, MonoType, OPTION_TYPE_ID,
    RANGE_TYPE_ID, RESULT_TYPE_ID, TASK_TYPE_ID, TypeDef, TypeId, UNFOLD_STEP_TYPE_ID,
    builtin_method_alias, contains_meta, method_receiver_type_id, zonk_ty,
};
use super::type_map::TypeMap;
use crate::module::artifacts::TypedModule;
use crate::syntax::ast::{
    BinOp, Block, CondArm, Expr, ExprId, ExprKind, FunctionDecl, Item, Literal, Pattern,
    SourceFile, Stmt, StringPart, Type as AstType, UnOp,
};
use crate::syntax::span::Span;
use std::collections::{HashMap, HashSet, VecDeque};

/// Parameterized container contracts whose single type argument is the element
/// type, exposing a `Self -> Elem` functional dependency.
const CONTAINER_ELEM_CONTRACTS: [&str; 3] = ["IndexRead", "IndexWrite", "IntoIterator"];

/// Split a comma-separated type-argument list at the top level, respecting `<>`
/// and `()` nesting so nested generics stay intact:
/// `"String, Dict<Int, Bool>"` -> `["String", "Dict<Int, Bool>"]`.
fn split_top_level_type_args(s: &str) -> Vec<&str> {
    let mut args = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' => depth += 1,
            '>' | ')' => depth -= 1,
            ',' if depth == 0 => {
                args.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = s[start..].trim();
    if !last.is_empty() {
        args.push(last);
    }
    args
}

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
    // Whether we're inside any function body (true even when ret type is inferred)
    in_function: bool,

    // Module aliases (for cross-module call resolution)
    module_aliases: HashSet<String>,

    // Type variable scope — names in scope resolve to MonoType::Var
    type_var_scope: Vec<String>,
    current_type_param_bounds: HashMap<String, Vec<String>>,

    // Names declared `pub` at module scope — these cannot be rebound
    pub_bindings: HashSet<String>,

    // Unification engine: MetaVar counter and solved assignments
    next_meta: u32,
    meta_subst: HashMap<u32, MonoType>,

    /// When check_expr dispatches a Call to synth_call, this carries the
    /// expected return type so that generic instantiation can pre-unify the
    /// return type *before* checking arguments, solving MetaVars earlier.
    call_expected_ret: Option<MonoType>,

    // Internal host intrinsics are only callable from stdlib/prelude modules.
    allow_internal_host_builtins: bool,

    // Deferred ambiguity checks for collection literals (Dict.new(), []) whose
    // type args are MetaVars at binding time but may be resolved by downstream
    // usage before scope exit.  Each entry: (variable_name, span).
    // Managed with a "generation" index: callers save the current length,
    // then drain from that index at scope exit.
    pending_meta_let_bindings: Vec<(String, Span)>,
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
            in_function: false,
            module_aliases,
            type_var_scope: Vec::new(),
            current_type_param_bounds: HashMap::new(),
            pub_bindings: HashSet::new(),
            next_meta: 0,
            meta_subst: HashMap::new(),
            call_expected_ret: None,
            allow_internal_host_builtins,
            pending_meta_let_bindings: Vec::new(),
        };

        // Pass 0: give every unannotated function a fresh MetaVar return type.
        // All call sites (including top-level statements, forward references,
        // and recursion) then instantiate the same return type instead of
        // falling back to Void before the body has been checked.
        let unannotated_source_fns: HashMap<String, Span> = ast
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Function(decl) if decl.return_type.is_none() => {
                    Some((decl.name.clone(), decl.body.span))
                }
                _ => None,
            })
            .collect();
        let pass0_seed: Vec<_> = checker
            .value_env
            .all_functions()
            .map(|(_, sig)| sig.clone())
            .collect();
        let pass0_functions: Vec<_> = pass0_seed
            .into_iter()
            .map(|mut sig| {
                if sig.ret.is_none() && unannotated_source_fns.contains_key(sig.name.as_str()) {
                    sig.ret = Some(checker.fresh_meta());
                }
                sig
            })
            .collect();
        for sig in pass0_functions {
            checker.value_env.update_function(sig);
        }

        // Build dependency graph and topologically sort top-level items.
        // This ensures each binding is checked after everything it depends on,
        // eliminating the need for multi-pass re-checking.
        let order = topo_sort_top_level(ast, &checker.value_env);

        for idx in order {
            match &ast.items[idx] {
                Item::TypeDecl(_)
                | Item::Import(_)
                | Item::ExternFunction(_)
                | Item::ExternType(_) => {
                    // Already handled by resolver
                }
                Item::Function(decl) => {
                    checker.check_function(decl);
                }
                Item::Stmt(stmt) => {
                    if let Stmt::Let {
                        pattern,
                        ty,
                        value,
                        is_pub,
                        span,
                        ..
                    } = stmt
                    {
                        // Only simple identifier patterns for top-level lets
                        if let Pattern::Ident(name, _) = pattern {
                            if *is_pub {
                                checker.pub_bindings.insert(name.clone());
                            }
                            // Determine the expected type.
                            // Even when checking fails, keep a binding to avoid
                            // noisy follow-up "undefined variable" diagnostics.
                            let value_ty = if let Some(ann_ty) = ty {
                                // Type annotation provided - check mode
                                let expected = match checker.resolve_type(ann_ty) {
                                    Ok(t) => t,
                                    Err(()) => {
                                        let recovery_ty = checker.fresh_meta();
                                        checker.value_env.add_value(name.clone(), recovery_ty);
                                        continue;
                                    }
                                };
                                if checker.check_expr(value, &expected).is_err() {
                                    checker.value_env.add_value(name.clone(), expected);
                                    continue;
                                }
                                expected
                            } else {
                                // No annotation - synthesis mode
                                let t = match checker.synth_expr(value) {
                                    Ok(t) => t,
                                    Err(()) => {
                                        let recovery_ty = checker.fresh_meta();
                                        checker.value_env.add_value(name.clone(), recovery_ty);
                                        continue;
                                    }
                                };
                                let t = checker.zonk(&t);
                                if contains_meta(&t) {
                                    if matches!(&t, MonoType::Dict(_, _) | MonoType::Vector(_)) {
                                        // Defer: type args may be resolved by subsequent items
                                        checker
                                            .pending_meta_let_bindings
                                            .push((name.clone(), value.span));
                                        checker.value_env.add_value(name.clone(), t);
                                        continue;
                                    }
                                    checker.errors.push(TypeError::AmbiguousType {
                                        name: name.clone(),
                                        span: value.span,
                                        note: "type cannot be inferred; add a type annotation"
                                            .to_string(),
                                    });
                                    let recovery_ty = checker.fresh_meta();
                                    checker.value_env.add_value(name.clone(), recovery_ty);
                                    continue;
                                }
                                t
                            };

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
        }

        // Drain top-level deferred collection meta-bindings.
        // Any Dict/Vector whose element types are still unsolved MetaVars
        // after all items have been checked is genuinely ambiguous.
        {
            let entries: Vec<_> = checker.pending_meta_let_bindings.drain(..).collect();
            for (name, span) in entries {
                let ty = checker.value_env.lookup(&name);
                if let Some(ty) = ty {
                    let ty = checker.zonk(&ty);
                    if contains_meta(&ty) {
                        checker.errors.push(TypeError::AmbiguousType {
                            name: name.clone(),
                            span,
                            note: "type cannot be inferred; add a type annotation".to_string(),
                        });
                        let recovery = checker.fresh_meta();
                        checker.value_env.add_value(name, recovery);
                    }
                }
            }
        }

        // Final zonk: resolve any MetaVars from top-level stmt checking
        let meta_subst = std::mem::take(&mut checker.meta_subst);
        checker.type_map.zonk(&meta_subst);
        checker.value_env.zonk_values(&meta_subst);
        checker.value_env.zonk_functions(&meta_subst);

        let unresolved_returns: Vec<_> = checker
            .value_env
            .all_functions()
            .filter_map(|(name, sig)| {
                let span = *unannotated_source_fns.get(name)?;
                sig.ret
                    .as_ref()
                    .is_some_and(contains_meta)
                    .then(|| (name.to_string(), span))
            })
            .collect();
        for (name, span) in unresolved_returns {
            checker.errors.push(TypeError::AmbiguousType {
                name: format!("return type of `{name}`"),
                span,
                note: "return type cannot be inferred; add a type annotation or call the generic value".to_string(),
            });
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

    fn resolves_as_value_binding(&self, name: &str) -> bool {
        self.local_env.lookup(name).is_some() || self.value_env.has_value_binding(name)
    }

    fn can_use_module_alias(&self, name: &str) -> bool {
        (self.module_aliases.contains(name) || self.value_env.is_extern_namespace(name))
            && !self.resolves_as_value_binding(name)
    }

    //
    // Top-level checking
    //

    fn check_function(&mut self, decl: &FunctionDecl) {
        // Push type variable scope for generic functions
        let saved_type_vars = std::mem::replace(
            &mut self.type_var_scope,
            decl.type_params.iter().map(|p| p.name.clone()).collect(),
        );
        let saved_type_param_bounds = std::mem::replace(
            &mut self.current_type_param_bounds,
            decl.type_params
                .iter()
                .filter(|p| !p.bounds.is_empty())
                .map(|p| (p.name.clone(), p.bounds.clone()))
                .collect(),
        );

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
        let saved_in_function = self.in_function;
        self.current_function_ret = sig.ret.clone();
        self.in_function = true;

        // Type-check the function body. An explicit annotation checks the body
        // against it; an unannotated function (whose signature return is a
        // Pass-0 MetaVar) synthesizes the body's real type — including Never for
        // diverging bodies — and binds the MetaVar to it so every call site sees
        // the same inferred return.
        if decl.return_type.is_some() {
            if let Some(expected_ret) = &sig.ret {
                // Explicit return type — use bidirectional check so that the
                // expected type flows into the last expression (e.g. anonymous
                // record literals in return position).
                let _ = self.check_block(&decl.body, expected_ret);
            } else {
                let _ = self.check_block(&decl.body, &MonoType::Void);
            }
        } else {
            match self.synth_block(&decl.body) {
                Ok(body_ty) => {
                    let body_ty = self.zonk(&body_ty);
                    if let Some(MonoType::MetaVar(id)) = sig.ret.as_ref() {
                        self.meta_subst.insert(*id, body_ty.clone());
                    }
                    // Update the function signature with the inferred return type
                    // so later source-order lookups can reuse it. If the type still
                    // contains MetaVars, final module checking reports ambiguity only
                    // after other recursive functions have had a chance to solve them.
                    let mut updated_sig = sig.clone();
                    updated_sig.ret = Some(body_ty);
                    self.value_env.update_function(updated_sig);
                }
                Err(()) => {
                    // Type checking failed, can't infer return type
                }
            }
        }

        // Zonk TypeMap entries and propagate resolved metas to value_env so
        // that cross-item metas survive when meta_subst is cleared.
        let meta_subst = std::mem::take(&mut self.meta_subst);
        self.type_map.zonk(&meta_subst);
        self.value_env.zonk_values(&meta_subst);
        self.value_env.zonk_functions(&meta_subst);

        // Clean up
        self.current_function_ret = None;
        self.in_function = saved_in_function;
        self.local_env.pop_scope();
        self.type_var_scope = saved_type_vars;
        self.current_type_param_bounds = saved_type_param_bounds;
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
        if let MonoType::MetaVar(other) = &zonked
            && *other == id
        {
            return Ok(()); // trivial self-unification
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

    /// Drain deferred meta-binding ambiguity checks added since `from`.
    ///
    /// Called at scope exit (just before `pop_scope`) so that all statements
    /// in the scope have had a chance to resolve the MetaVars via unification.
    /// Any binding whose type still contains a MetaVar after zonking is reported
    /// as ambiguous.
    ///
    /// Limitation: if the same name is re-bound in the same scope after the
    /// deferred binding (e.g. `xs := []; xs := [1]`), the second binding
    /// overwrites the name in local_env and the lookup here finds the concrete
    /// type, silently suppressing the ambiguity error for the original binding.
    /// This is a known edge case; duplicate lets in the same scope are already
    /// poor style and the net result (the name has a good type) is not harmful.
    fn drain_pending_meta_bindings(&mut self, from: usize) {
        let entries: Vec<_> = self.pending_meta_let_bindings.drain(from..).collect();
        for (name, span) in entries {
            let ty = self
                .local_env
                .lookup(&name)
                .cloned()
                .or_else(|| self.value_env.lookup(&name));
            if let Some(ty) = ty {
                let ty = self.zonk(&ty);
                if contains_meta(&ty) {
                    self.errors.push(TypeError::AmbiguousType {
                        name,
                        span,
                        note: "type cannot be inferred; add a type annotation".to_string(),
                    });
                }
            }
        }
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
                let _ = self.synth_block(body);
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
        let saved_in_function = self.in_function;
        self.current_function_ret = expected_ret.clone();
        self.in_function = true;

        let body_ty = match &expected_ret {
            Some(exp) => {
                self.check_expr(&fe.body, exp)?;
                exp.clone()
            }
            None => self.synth_expr(&fe.body)?,
        };

        self.local_env.pop_scope();
        self.current_function_ret = saved;
        self.in_function = saved_in_function;

        Ok(MonoType::Function {
            params: param_types,
            ret: Box::new(body_ty),
        })
    }

    /// Resolve an AST type annotation to a MonoType, including type variables
    /// from the current generic function scope even when nested inside generic
    /// containers such as `Dict<String, A>`.
    fn resolve_type(&mut self, ty: &AstType) -> Result<MonoType, ()> {
        if self.type_var_scope.is_empty() {
            return self.type_env.resolve_type(ty, &mut self.errors);
        }
        self.resolve_type_with_current_vars(ty)
    }

    fn resolve_type_with_current_vars(&mut self, ty: &AstType) -> Result<MonoType, ()> {
        if let AstType::Named { name, args, .. } = ty
            && args.is_empty()
            && self.type_var_scope.contains(name)
        {
            return Ok(MonoType::Var(name.clone()));
        }

        match ty {
            AstType::Named { name, args, span } if !args.is_empty() => {
                let resolved_args: Vec<MonoType> = args
                    .iter()
                    .map(|a| self.resolve_type_with_current_vars(a))
                    .collect::<Result<_, _>>()?;

                match name.as_str() {
                    "Vector" if resolved_args.len() == 1 => Ok(MonoType::Vector(Box::new(
                        resolved_args.into_iter().next().unwrap(),
                    ))),
                    "Dict" if resolved_args.len() == 2 => {
                        let mut it = resolved_args.into_iter();
                        let key = it.next().unwrap();
                        match &key {
                            MonoType::Int | MonoType::String | MonoType::Byte => {}
                            _ => {
                                self.errors.push(TypeError::InvalidDictKey {
                                    key_type: key.clone(),
                                    span: *span,
                                });
                                return Err(());
                            }
                        }
                        Ok(MonoType::Dict(Box::new(key), Box::new(it.next().unwrap())))
                    }
                    "Option" if resolved_args.len() == 1 => Ok(MonoType::Named {
                        type_id: OPTION_TYPE_ID,
                        args: resolved_args,
                    }),
                    "Result" if resolved_args.len() == 2 => Ok(MonoType::Named {
                        type_id: RESULT_TYPE_ID,
                        args: resolved_args,
                    }),
                    "Cell" if resolved_args.len() == 1 => Ok(MonoType::Named {
                        type_id: CELL_TYPE_ID,
                        args: resolved_args,
                    }),
                    "Iterator" if resolved_args.len() == 1 => Ok(MonoType::Named {
                        type_id: ITERATOR_TYPE_ID,
                        args: resolved_args,
                    }),
                    "Task" if resolved_args.len() == 1 => Ok(MonoType::Named {
                        type_id: TASK_TYPE_ID,
                        args: resolved_args,
                    }),
                    "Channel" if resolved_args.len() == 1 => Ok(MonoType::Named {
                        type_id: CHANNEL_TYPE_ID,
                        args: resolved_args,
                    }),
                    "IterItem" if resolved_args.len() == 1 => Ok(MonoType::Named {
                        type_id: ITER_ITEM_TYPE_ID,
                        args: resolved_args,
                    }),
                    "UnfoldStep" if resolved_args.len() == 2 => Ok(MonoType::Named {
                        type_id: UNFOLD_STEP_TYPE_ID,
                        args: resolved_args,
                    }),
                    _ => match self.type_env.lookup_type(name) {
                        Some(type_id) => {
                            if let Some(TypeDef::Alias { .. }) = self.type_env.get_def(type_id) {
                                self.errors.push(TypeError::GenericNotSupported {
                                    name: name.clone(),
                                    span: *span,
                                    note: "Type aliases cannot take type arguments".to_string(),
                                });
                                return Err(());
                            }
                            let expected_arity = self
                                .type_env
                                .get_def(type_id)
                                .map(|d| d.type_params().len())
                                .unwrap_or(0);
                            if resolved_args.len() != expected_arity {
                                self.errors.push(TypeError::UndefinedType {
                                    name: format!(
                                        "{} (expected {} type arg(s), found {})",
                                        name,
                                        expected_arity,
                                        resolved_args.len()
                                    ),
                                    span: *span,
                                });
                                Err(())
                            } else {
                                Ok(MonoType::Named {
                                    type_id,
                                    args: resolved_args,
                                })
                            }
                        }
                        None => {
                            self.errors.push(TypeError::UndefinedType {
                                name: name.clone(),
                                span: *span,
                            });
                            Err(())
                        }
                    },
                }
            }
            AstType::Function { params, ret, .. } => {
                let param_tys: Vec<MonoType> = params
                    .iter()
                    .map(|p| self.resolve_type_with_current_vars(p))
                    .collect::<Result<_, _>>()?;
                let ret_ty = self.resolve_type_with_current_vars(ret)?;
                Ok(MonoType::Function {
                    params: param_tys,
                    ret: Box::new(ret_ty),
                })
            }
            _ => self.type_env.resolve_type(ty, &mut self.errors),
        }
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

            ExprKind::Cond { arms } => self.synth_cond(arms, expr.span),

            ExprKind::FieldAccess { base, field } => {
                if let ExprKind::Ident(alias) = &base.kind
                    && self.can_use_module_alias(alias)
                    && !self.is_qualified_variant(base, field)
                {
                    let qualified = format!("{}.{}", alias, field);
                    if let Some(ty) = self.value_env.lookup(&qualified) {
                        // Plain pub value or monomorphic function: synthesize directly
                        if !matches!(ty, MonoType::Function { .. }) {
                            self.type_map.set_expr_type(expr.id, ty.clone());
                            return Ok(ty);
                        }
                        // Monomorphic function: can infer without annotation
                        if let Some(sig) = self.value_env.get_function(&qualified)
                            && sig.type_params.is_empty()
                        {
                            let fn_ty = ty.clone();
                            self.type_map.set_expr_type(expr.id, fn_ty.clone());
                            return Ok(fn_ty);
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
                // Derive expected inner type from the enclosing function's return type
                // so that generic MetaVars in the inner call get solved earlier.
                // e.g. function returns Result<X, ParseError> → inner expected is
                // Result<fresh_meta, ParseError>, solving E before checking args.
                let inner_ty = if let Some(MonoType::Named {
                    type_id,
                    args: ret_args,
                }) = &self.current_function_ret.clone()
                {
                    if *type_id == RESULT_TYPE_ID {
                        let ok_meta = self.fresh_meta();
                        let err_ty = ret_args.get(1).cloned().unwrap_or(MonoType::Void);
                        let expected_inner = MonoType::Named {
                            type_id: RESULT_TYPE_ID,
                            args: vec![ok_meta, err_ty],
                        };
                        self.check_expr(inner, &expected_inner)?;
                        self.zonk(&expected_inner)
                    } else {
                        self.synth_expr(inner)?
                    }
                } else {
                    self.synth_expr(inner)?
                };
                match &inner_ty {
                    MonoType::Named { type_id, args } if *type_id == RESULT_TYPE_ID => {
                        // try Result<T,E> → extracts T; propagates Err(E) via early return
                        Ok(args.first().cloned().unwrap_or(MonoType::Void))
                    }
                    MonoType::Named { type_id, args } if *type_id == OPTION_TYPE_ID => {
                        // try Option<T> → extracts T; propagates None via early return
                        // Only valid in functions returning Option<U>
                        let in_option_ctx = matches!(
                            &self.current_function_ret,
                            Some(MonoType::Named { type_id, .. }) if *type_id == OPTION_TYPE_ID
                        );
                        if in_option_ctx {
                            Ok(args.first().cloned().unwrap_or(MonoType::Void))
                        } else {
                            self.errors.push(TypeError::UnsupportedFeature {
                                feature: "try on Option",
                                span: expr.span,
                                note: "`try` on Option is only allowed in functions \
                                       returning Option; use `.ok_or(err)` to convert \
                                       to Result first"
                                    .to_string(),
                            });
                            Err(())
                        }
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
        // Zonk expected type so solved MetaVars are visible to all branches
        // (e.g. check_variant_lit needs a concrete Named type, not a MetaVar).
        let expected_z = self.zonk(expected);
        let expected = &expected_z;

        // Contextual literal narrowing: integer literals can satisfy Byte
        // expectations when they are in range.
        if let ExprKind::Literal(Literal::Int(n)) = &expr.kind
            && *expected == MonoType::Byte
        {
            if (0..=255).contains(n) {
                self.type_map.set_expr_type(expr.id, MonoType::Byte);
                return Ok(());
            }

            self.errors.push(TypeError::TypeMismatch {
                expected: MonoType::Byte,
                actual: MonoType::Int,
                span: expr.span,
                note: Some(format!(
                    "integer literal {} is out of range for Byte (0..255)",
                    n
                )),
            });
            return Err(());
        }

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

            // Cond: check each arm against expected type
            ExprKind::Cond { arms } => {
                let has_default = arms.last().is_some_and(|a| a.condition.is_none());
                for arm in arms {
                    if let Some(cond) = &arm.condition {
                        self.check_expr(cond, &MonoType::Bool)?;
                    }
                    self.check_expr(&arm.body, expected)?;
                }
                if !has_default {
                    let _ = self.unify(&MonoType::Void, expected, expr.span);
                }
                Ok(())
            }

            // Case: check each arm body against expected type
            ExprKind::Case { scrutinee, arms } => {
                let scrut_ty = self.synth_expr(scrutinee)?;
                let is_primitive_match = matches!(
                    scrut_ty,
                    MonoType::Int | MonoType::Bool | MonoType::String | MonoType::Byte
                );
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
                            None => param_types.push(self.zonk(exp_ty)),
                        }
                    }
                    let ret_ty = match &fe.return_type {
                        Some(ann) => {
                            let ann_ret = self.resolve_type(ann)?;
                            self.unify(&ann_ret, expected_ret.as_ref(), ann.span())?;
                            self.zonk(expected_ret.as_ref())
                        }
                        None => self.zonk(expected_ret.as_ref()),
                    };
                    self.local_env.push_scope();
                    for (p, ty) in fe.params.iter().zip(&param_types) {
                        self.local_env.bind(p.name.clone(), ty.clone());
                    }
                    let saved = self.current_function_ret.take();
                    let saved_in_function = self.in_function;
                    self.current_function_ret = Some(ret_ty.clone());
                    self.in_function = true;
                    let result = self.check_expr(&fe.body, &ret_ty);
                    self.local_env.pop_scope();
                    self.current_function_ret = saved;
                    self.in_function = saved_in_function;
                    result
                } else {
                    let actual = self.synth_expr(expr)?;
                    self.unify(&actual, expected, expr.span)
                }
            }

            // Collect: propagate expected element type into body
            ExprKind::Collect {
                pattern,
                index_pattern,
                iter,
                body,
            } => {
                if let MonoType::Vector(elem_ty) = expected {
                    let actual = self.check_collect(
                        pattern,
                        index_pattern.as_ref(),
                        iter,
                        body,
                        expr.span,
                        elem_ty,
                    )?;
                    self.unify(&actual, expected, expr.span)
                } else {
                    let actual =
                        self.synth_collect(pattern, index_pattern.as_ref(), iter, body, expr.span)?;
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
                if let ExprKind::FieldAccess { base, field } = &callee.kind
                    && let ExprKind::Ident(alias) = &base.kind
                    && alias == "Dict"
                    && field == "new"
                    && let MonoType::Dict(_, _) = expected
                {
                    self.type_map.set_expr_type(expr.id, expected.clone());
                    self.type_map.set_expr_type(callee.id, expected.clone());
                    self.type_map.set_expr_type(base.id, expected.clone());
                    return Ok(());
                }
                let actual = self.synth_expr(expr)?;
                self.unify(&actual, expected, expr.span)
            }

            // First-class module method reference: Vector.len, String.concat, etc.
            ExprKind::FieldAccess { base, field } => {
                if let ExprKind::Ident(alias) = &base.kind
                    && self.can_use_module_alias(alias)
                    && !self.is_qualified_variant(base, field)
                {
                    let alias = alias.clone();
                    let field = field.clone();
                    return self
                        .check_module_func_ref(&alias, &field, expected, expr.id, expr.span);
                }
                let actual = self.synth_expr(expr)?;
                self.unify(&actual, expected, expr.span)
            }

            // Calls: propagate expected return type into generic instantiation
            // so MetaVars are solved before checking arguments.
            ExprKind::Call { callee, args } => {
                self.call_expected_ret = Some(expected.clone());
                let actual = self.synth_call(callee, args, expr.span)?;
                self.call_expected_ret = None;
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
            // Arithmetic:
            // - Int × Int → Int
            // - Byte × Byte → Int
            // - Int × Byte / Byte × Int → Int
            // - Float × Float → Float
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                let left_raw = self.synth_expr(left)?;
                let right_raw = self.synth_expr(right)?;
                let left_ty = self.zonk(&left_raw);
                let right_ty = self.zonk(&right_raw);

                match (&left_ty, &right_ty) {
                    (MonoType::Int, MonoType::Int) => Ok(MonoType::Int),
                    (MonoType::Byte, MonoType::Byte) => Ok(MonoType::Int),
                    (MonoType::Int, MonoType::Byte) | (MonoType::Byte, MonoType::Int) => {
                        Ok(MonoType::Int)
                    }
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
                    // Byte arithmetic promotes to Int; unresolved metas are
                    // constrained to Int for deterministic lowering/codegen.
                    (MonoType::MetaVar(id), MonoType::Byte) => {
                        self.solve_meta(*id, MonoType::Int, left.span)?;
                        Ok(MonoType::Int)
                    }
                    (MonoType::Byte, MonoType::MetaVar(id)) => {
                        self.solve_meta(*id, MonoType::Int, right.span)?;
                        Ok(MonoType::Int)
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

            // Bitwise: Int/Byte only → Int
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                let left_raw = self.synth_expr(left)?;
                let right_raw = self.synth_expr(right)?;
                let left_ty = self.zonk(&left_raw);
                let right_ty = self.zonk(&right_raw);

                match (&left_ty, &right_ty) {
                    (MonoType::Int, MonoType::Int)
                    | (MonoType::Byte, MonoType::Byte)
                    | (MonoType::Int, MonoType::Byte)
                    | (MonoType::Byte, MonoType::Int) => Ok(MonoType::Int),
                    (MonoType::MetaVar(id), MonoType::Int)
                    | (MonoType::MetaVar(id), MonoType::Byte) => {
                        self.solve_meta(*id, MonoType::Int, left.span)?;
                        Ok(MonoType::Int)
                    }
                    (MonoType::Int, MonoType::MetaVar(id))
                    | (MonoType::Byte, MonoType::MetaVar(id)) => {
                        self.solve_meta(*id, MonoType::Int, right.span)?;
                        Ok(MonoType::Int)
                    }
                    _ => {
                        let left_bad = !matches!(left_ty, MonoType::Int | MonoType::Byte);
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Int,
                            actual: if left_bad { left_ty } else { right_ty },
                            span: if left_bad { left.span } else { right.span },
                            note: Some("bitwise operators require Int or Byte operands".into()),
                        });
                        Err(())
                    }
                }
            }

            // Equality with directional type propagation: try to use
            // one operand's known type as context for the other, so that
            // shorthand variants like `kind == .Use` type-check.
            BinOp::Eq | BinOp::Ne => {
                // Attempt 1: synth left, check right against left's type
                if let Some(result) = self.try_eq_directional(left, right) {
                    return result;
                }
                // Attempt 2: synth right, check left against right's type
                if let Some(result) = self.try_eq_directional(right, left) {
                    return result;
                }
                // Fallback: original synth+synth+unify for diagnostics
                let left_ty = self.synth_expr(left)?;
                let right_ty = self.synth_expr(right)?;
                let left_z = self.zonk(&left_ty);
                let right_z = self.zonk(&right_ty);
                // Byte/Int mix allowed (same numeric coercion as arithmetic)
                match (&left_z, &right_z) {
                    (MonoType::Int, MonoType::Byte)
                    | (MonoType::Byte, MonoType::Int)
                    | (MonoType::Byte, MonoType::Byte) => {}
                    _ => {
                        self.unify(&left_ty, &right_ty, right.span)?;
                    }
                }
                Ok(MonoType::Bool)
            }

            // Ordered comparison: T × T → Bool (requires Ord for non-primitives)
            // Byte/Int mix allowed (same numeric coercion as arithmetic)
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let op_name = match op {
                    BinOp::Lt => "<",
                    BinOp::Le => "<=",
                    BinOp::Gt => ">",
                    BinOp::Ge => ">=",
                    _ => unreachable!(),
                };
                let left_raw = self.synth_expr(left)?;
                let right_raw = self.synth_expr(right)?;
                let left_ty = self.zonk(&left_raw);
                let right_ty = self.zonk(&right_raw);

                match (&left_ty, &right_ty) {
                    (MonoType::Int, MonoType::Byte)
                    | (MonoType::Byte, MonoType::Int)
                    | (MonoType::Byte, MonoType::Byte) => {
                        // Byte widens to Int for comparison
                    }
                    _ => {
                        self.unify(&left_ty, &right_ty, right.span)?;
                        // Check Ord contract for non-primitive types
                        let unified = self.zonk(&left_ty);
                        match &unified {
                            MonoType::Int | MonoType::Float | MonoType::Byte | MonoType::String => {
                            }
                            MonoType::MetaVar(_) => {}
                            MonoType::Var(name) => {
                                // Type variables require an Ord bound
                                if !self
                                    .current_type_param_bounds
                                    .get(name)
                                    .map(|b| b.contains(&"Ord".to_string()))
                                    .unwrap_or(false)
                                {
                                    self.errors.push(TypeError::UnsupportedFeature {
                                        feature: "contract bound",
                                        span,
                                        note: format!(
                                            "type variable {} does not satisfy Ord required by {}: missing Ord bound",
                                            name, op_name
                                        ),
                                    });
                                }
                            }
                            _ => {
                                if let Err(reason) = self.validate_ord_type(&unified) {
                                    self.errors.push(TypeError::UnsupportedFeature {
                                        feature: "contract bound",
                                        span,
                                        note: format!(
                                            "type {} does not satisfy Ord required by {}: {}",
                                            unified.format_with_names(&self.type_env),
                                            op_name,
                                            reason
                                        ),
                                    });
                                }
                            }
                        }
                    }
                }
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

            // Range literal: m..n → Range (both sides must be Int)
            BinOp::Range => {
                self.check_expr(left, &MonoType::Int)?;
                self.check_expr(right, &MonoType::Int)?;
                Ok(MonoType::named(RANGE_TYPE_ID))
            }
        }
    }

    /// Speculatively try: synth `synth_side`, then check `check_side` against
    /// that type. Returns `Some(Ok(Bool))` on success, `None` if the attempt
    /// failed (state is rolled back). This avoids emitting misleading diagnostics
    /// from failed directional attempts.
    fn try_eq_directional(
        &mut self,
        synth_side: &Expr,
        check_side: &Expr,
    ) -> Option<Result<MonoType, ()>> {
        // Save state for rollback
        let saved_errors_len = self.errors.len();
        let saved_next_meta = self.next_meta;
        let saved_meta_subst = self.meta_subst.clone();
        let saved_type_map = self.type_map.clone();
        let saved_call_expected_ret = self.call_expected_ret.clone();

        let result = (|| {
            let ty = self.synth_expr(synth_side)?;
            let ty = self.zonk(&ty);
            // Only proceed if we got a concrete type (not an unsolved MetaVar)
            if matches!(ty, MonoType::MetaVar(_)) {
                return Err(());
            }
            self.check_expr(check_side, &ty)?;
            Ok(MonoType::Bool)
        })();

        match result {
            Ok(ty) => Some(Ok(ty)),
            Err(()) => {
                // Roll back all mutable state from the failed attempt
                self.errors.truncate(saved_errors_len);
                self.next_meta = saved_next_meta;
                self.meta_subst = saved_meta_subst;
                self.type_map = saved_type_map;
                self.call_expected_ret = saved_call_expected_ret;
                None
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
            UnOp::BitNot => {
                let ty = self.synth_expr(expr)?;
                let ty = self.zonk(&ty);
                match &ty {
                    MonoType::Int => Ok(MonoType::Int),
                    MonoType::Byte => Ok(MonoType::Int),
                    MonoType::MetaVar(id) => {
                        self.solve_meta(*id, MonoType::Int, expr.span)?;
                        Ok(MonoType::Int)
                    }
                    _ => {
                        self.errors.push(TypeError::TypeMismatch {
                            expected: MonoType::Int,
                            actual: ty,
                            span: expr.span,
                            note: Some("bitwise not requires Int or Byte operand".into()),
                        });
                        Err(())
                    }
                }
            }
        }
    }

    //
    // Function calls
    //

    fn synth_call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> Result<MonoType, ()> {
        // Capture and clear expected return type so it doesn't leak into
        // nested synth_expr calls (e.g. synthesising the callee/base).
        // The value is used once for pre-unification at the right level.
        let call_expected = self.call_expected_ret.take();

        // Special case: field-access calls — handles both module-qualified
        // calls (module.func(args)) and method calls (receiver.method(args)).
        if let ExprKind::FieldAccess {
            base,
            field: method_name,
        } = &callee.kind
        {
            // Check for module-qualified call FIRST (before synthesising base type),
            // unless the name is actually a qualified variant constructor — names
            // like Option/Result are both module aliases and types, and the
            // variant form (Option.Some(x)) must win over the module form.
            if let ExprKind::Ident(alias) = &base.kind
                && self.can_use_module_alias(alias)
                && !self.is_qualified_variant(base, method_name)
            {
                let alias = alias.clone();
                let method_name = method_name.clone();
                let callee_id = callee.id;
                self.call_expected_ret = call_expected;
                return self.synth_module_call(&alias, &method_name, args, span, callee_id);
            }

            // TypeName.Variant(args) or module.TypeName.Variant(args)
            if let Some(type_id) = self.try_resolve_type_from_expr(base)
                && self
                    .type_env
                    .get_variant_index(type_id, method_name)
                    .is_some()
            {
                let (field_types, result_ty) =
                    self.qualified_variant_signature(type_id, method_name);
                self.type_map.set_expr_type(base.id, result_ty.clone());
                // Check arity
                if field_types.len() != args.len() {
                    self.errors.push(TypeError::WrongArity {
                        expected: field_types.len(),
                        actual: args.len(),
                        span,
                    });
                    return Err(());
                }
                // Check each arg against the field type
                for (arg, expected_ty) in args.iter().zip(field_types.iter()) {
                    if let Err(()) = self.check_expr(arg, expected_ty) {
                        return Err(());
                    }
                }
                // Record callee type (constructor function) and return
                let ctor_ty = if field_types.is_empty() {
                    result_ty.clone()
                } else {
                    MonoType::Function {
                        params: field_types,
                        ret: Box::new(result_ty.clone()),
                    }
                };
                self.type_map.set_expr_type(callee.id, self.zonk(&ctor_ty));
                return Ok(self.zonk(&result_ty));
            }

            // Method call on a value: synthesise base type, then dispatch.
            // Base synthesis is done first (no call_expected_ret visible to nested
            // calls), then restore expected for the actual method dispatch.
            let base_ty = self.synth_expr(base)?;
            self.call_expected_ret = call_expected;
            let method_name = method_name.clone();
            let callee_id = callee.id;
            return self.synth_method_call(base, base_ty, &method_name, args, span, callee_id);
        }

        // Normal function call: if callee is a plain Ident with a FunctionSignature
        // (and not shadowed by a local), use the signature-aware path.
        if let ExprKind::Ident(name) = &callee.kind
            && self.local_env.lookup(name).is_none()
            && let Some(sig) = self.value_env.get_function(name).cloned()
        {
            let call_label = format!("call to `{}`", name);
            let (ret, callee_ty) =
                self.synth_sig_call(&sig, args, call_expected, Some(&call_label), span)?;
            self.type_map.set_expr_type(callee.id, callee_ty);
            return Ok(ret);
        }

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

                // Best-effort pre-unify: solve generic MetaVars from expected
                // return type before checking arguments. Errors are deliberately
                // ignored — the outer unify in check_expr will re-report any
                // real mismatch.
                if let Some(expected_ret) = call_expected {
                    let _ = self.unify(&ret, &expected_ret, span);
                }

                // Check each argument; MetaVar params are solved by unify inside check_expr.
                // On failure, patch in call context for better error messages.
                for (idx, (arg, expected_ty)) in args.iter().zip(params.iter()).enumerate() {
                    if let Err(()) = self.check_expr(arg, expected_ty) {
                        if let Some(TypeError::TypeMismatch { note, .. }) = self.errors.last_mut()
                            && note.is_none()
                        {
                            let label = if let ExprKind::Ident(n) = &callee.kind {
                                format!("argument {} of call to `{}`", idx + 1, n)
                            } else {
                                format!("argument {} of call", idx + 1)
                            };
                            *note = Some(label);
                        }
                        return Err(());
                    }
                }

                Ok(self.zonk(&ret))
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
        // Dict.new() — emit fresh MetaVars for K and V so downstream usage can
        // resolve the type via unification.
        if alias == "Dict" && func_name == "new" {
            let k = self.fresh_meta();
            let v = self.fresh_meta();
            return Ok(MonoType::Dict(Box::new(k), Box::new(v)));
        }
        self.synth_qualified_call(alias, func_name, args, span)
    }

    /// Shared logic for calling a function with a known FunctionSignature:
    /// arity check → instantiate type params → pre-unify return → check args → validate bounds.
    /// `call_label` is used for error context (e.g. "call to `min`"); None omits annotation.
    /// Returns `(return_type, instantiated_callee_type)`.
    fn synth_sig_call(
        &mut self,
        sig: &crate::types::ty::FunctionSignature,
        args: &[Expr],
        call_expected: Option<MonoType>,
        call_label: Option<&str>,
        span: Span,
    ) -> Result<(MonoType, MonoType), ()> {
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
        let (inst_ty, var_to_meta) = self.instantiate_vars(&sig.type_params, &fn_ty);
        let (inst_params, inst_ret) = match &inst_ty {
            MonoType::Function { params, ret } => (params.clone(), *ret.clone()),
            _ => unreachable!(),
        };
        if let Some(expected_ret) = call_expected {
            let _ = self.unify(&inst_ret, &expected_ret, span);
        }
        for (idx, (arg, expected_ty)) in args.iter().zip(inst_params.iter()).enumerate() {
            if let Err(()) = self.check_expr(arg, expected_ty) {
                if let Some(label) = call_label
                    && let Some(TypeError::TypeMismatch { note, .. }) = self.errors.last_mut()
                    && note.is_none()
                {
                    *note = Some(format!("argument {} of {}", idx + 1, label));
                }
                return Err(());
            }
        }
        self.check_instantiated_contract_bounds(sig, &var_to_meta, span)?;
        let ret = self.zonk(&inst_ret);
        let callee_ty = self.zonk(&inst_ty);
        Ok((ret, callee_ty))
    }

    fn synth_qualified_call(
        &mut self,
        alias: &str,
        func_name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<MonoType, ()> {
        // Capture expected return type up front so it cannot leak on early-exit
        // paths (e.g. arity mismatch).
        let call_expected = self.call_expected_ret.take();

        let qualified = format!("{}.{}", alias, func_name);
        if let Some(sig) = self.value_env.get_function(&qualified).cloned() {
            let (ret, _callee_ty) = self.synth_sig_call(&sig, args, call_expected, None, span)?;
            return Ok(ret);
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

    fn check_interpolation_expr(&mut self, expr: &Expr) -> Result<(), ()> {
        let expr_ty = self.synth_expr(expr)?;
        let expr_ty = self.zonk(&expr_ty);
        self.validate_interpolation_to_string(expr, &expr_ty)
    }

    fn has_stringify_bound(&self, ty: &MonoType) -> bool {
        matches!(ty, MonoType::Var(name) if self.current_type_param_bounds.get(name).map(|b| b.contains(&"Stringify".to_string())) == Some(true))
    }

    fn mono_from_bound_arg(&self, name: &str) -> MonoType {
        let name = name.trim();
        // Generic application written in the bound string, e.g. `Option<Int>` or
        // `Dict<String, Vector<Int>>`. Resolve the head and recurse on each
        // top-level argument so nested generics survive.
        if let Some(open) = name.find('<')
            && let Some(without_close) = name.strip_suffix('>')
        {
            let head = name[..open].trim();
            let args: Vec<MonoType> = split_top_level_type_args(&without_close[open + 1..])
                .into_iter()
                .map(|arg| self.mono_from_bound_arg(arg))
                .collect();
            return match head {
                "Vector" if args.len() == 1 => {
                    MonoType::Vector(Box::new(args.into_iter().next().unwrap()))
                }
                "Dict" if args.len() == 2 => {
                    let mut it = args.into_iter();
                    MonoType::Dict(Box::new(it.next().unwrap()), Box::new(it.next().unwrap()))
                }
                _ => self
                    .type_env
                    .lookup_type(head)
                    .map(|type_id| MonoType::Named { type_id, args })
                    .unwrap_or(MonoType::Void),
            };
        }
        match name {
            "Int" => MonoType::Int,
            "Float" => MonoType::Float,
            "Bool" => MonoType::Bool,
            "Byte" => MonoType::Byte,
            "String" => MonoType::String,
            "Void" => MonoType::Void,
            "Never" => MonoType::Never,
            other if self.type_var_scope.iter().any(|param| param == other) => {
                MonoType::Var(other.to_string())
            }
            other => self
                .type_env
                .lookup_type(other)
                .and_then(|type_id| {
                    let arity = self
                        .type_env
                        .get_def(type_id)
                        .map(|def| def.type_params().len())
                        .unwrap_or(0);
                    (arity == 0).then_some(MonoType::Named {
                        type_id,
                        args: vec![],
                    })
                })
                .unwrap_or(MonoType::Void),
        }
    }

    /// Strip a parameterized contract bound (`Contract<Inner>`) and return the
    /// trimmed `Inner`. The single place that knows the `Contract<...>` bound-string
    /// shape; shared by the per-contract element lookups and the bound-element
    /// recovery for the `Self -> Elem` functional dependency.
    fn contract_bound_arg<'a>(bound: &'a str, contract: &str) -> Option<&'a str> {
        bound
            .strip_prefix(contract)?
            .strip_prefix('<')?
            .strip_suffix('>')
            .map(str::trim)
    }

    /// Element type written in a `Var`'s `Contract<Elem>` bound, if any.
    fn contract_bound_elem(&self, ty: &MonoType, contract: &str) -> Option<MonoType> {
        let MonoType::Var(name) = ty else {
            return None;
        };
        let bounds = self.current_type_param_bounds.get(name)?;
        bounds
            .iter()
            .find_map(|bound| Self::contract_bound_arg(bound, contract))
            .map(|inner| self.mono_from_bound_arg(inner))
    }

    fn into_iterator_bound_elem(&self, ty: &MonoType) -> Option<MonoType> {
        self.contract_bound_elem(ty, "IntoIterator")
    }

    fn index_read_bound_elem(&self, ty: &MonoType) -> Option<MonoType> {
        self.contract_bound_elem(ty, "IndexRead")
    }

    fn validate_stringify_type(
        &mut self,
        ty: &MonoType,
        active: &mut HashSet<String>,
    ) -> Result<(), String> {
        let ty = self.zonk(ty);
        if self.has_stringify_bound(&ty) {
            return Ok(());
        }
        match &ty {
            MonoType::Int
            | MonoType::Float
            | MonoType::Bool
            | MonoType::String
            | MonoType::Byte => {
                return Ok(());
            }
            _ => {}
        }

        let key = format!("{}::Stringify", ty.format_with_names(&self.type_env));
        if !active.insert(key.clone()) {
            return Err(format!(
                "cyclic Stringify proof for {}",
                ty.format_with_names(&self.type_env)
            ));
        }

        let Some(type_id) = method_receiver_type_id(&ty) else {
            active.remove(&key);
            return Err("missing inherent `to_string() -> String`".to_string());
        };
        let Some(method_info) = self.type_env.get_method(type_id, "to_string").cloned() else {
            active.remove(&key);
            return Err("missing inherent `to_string() -> String`".to_string());
        };
        let Some(sig) = method_info
            .signature
            .or_else(|| self.value_env.get_function(&method_info.func_name).cloned())
        else {
            active.remove(&key);
            return Err("missing inherent `to_string() -> String`".to_string());
        };

        let full_fn_ty = MonoType::Function {
            params: sig.params.clone(),
            ret: Box::new(sig.ret.clone().unwrap_or(MonoType::Void)),
        };
        let (inst_ty, var_to_meta) = self.instantiate_vars(&sig.type_params, &full_fn_ty);
        let (inst_params, inst_ret) = match inst_ty {
            MonoType::Function { params, ret } => (params, *ret),
            _ => unreachable!(),
        };
        if inst_params.len() != 1 {
            active.remove(&key);
            return Err("has `to_string`, but it does not have receiver-only shape".to_string());
        }
        self.unify(
            &ty,
            &inst_params[0],
            Span::new(crate::syntax::span::FileId(0), 0, 0),
        )
        .map_err(|_| {
            format!(
                "missing inherent `to_string() -> String` for {}",
                ty.format_with_names(&self.type_env)
            )
        })?;
        let ret_ty = self.zonk(&inst_ret);
        if ret_ty != MonoType::String {
            active.remove(&key);
            return Err(format!(
                "has `to_string`, but it returns {} (expected String)",
                ret_ty.format_with_names(&self.type_env)
            ));
        }
        for (name, meta_ty) in var_to_meta {
            if sig
                .type_param_bounds
                .get(&name)
                .map(|b| b.contains(&"Stringify".to_string()))
                == Some(true)
            {
                self.validate_stringify_type(&self.zonk(&meta_ty), active)?;
            }
        }
        active.remove(&key);
        Ok(())
    }

    fn has_ord_bound(&self, ty: &MonoType) -> bool {
        matches!(ty, MonoType::Var(name) if self.current_type_param_bounds.get(name).map(|b| b.contains(&"Ord".to_string())) == Some(true))
    }

    fn validate_ord_type(&mut self, ty: &MonoType) -> Result<(), String> {
        let ty = self.zonk(ty);
        if self.has_ord_bound(&ty) {
            return Ok(());
        }
        match &ty {
            MonoType::Int | MonoType::Float | MonoType::String | MonoType::Byte => {
                return Ok(());
            }
            // Containers: recurse into element types
            MonoType::Vector(elem) => {
                return self.validate_ord_type(&elem.as_ref().clone());
            }
            _ => {}
        }
        // Check for explicit compare method with correct shape
        let Some(type_id) = method_receiver_type_id(&ty) else {
            return Err("missing inherent `compare(self, other: Self) -> Order`".to_string());
        };
        let Some(method_info) = self.type_env.get_method(type_id, "compare").cloned() else {
            return Err(format!(
                "type {} does not satisfy Ord: missing `compare` method",
                ty.format_with_names(&self.type_env)
            ));
        };
        // Validate signature shape: compare(self, other: Self) -> Order
        let Some(sig) = method_info
            .signature
            .or_else(|| self.value_env.get_function(&method_info.func_name).cloned())
        else {
            return Err("missing inherent `compare(self, other: Self) -> Order`".to_string());
        };
        let full_fn_ty = MonoType::Function {
            params: sig.params.clone(),
            ret: Box::new(sig.ret.clone().unwrap_or(MonoType::Void)),
        };
        let (inst_ty, var_to_meta) = self.instantiate_vars(&sig.type_params, &full_fn_ty);
        let (inst_params, inst_ret) = match inst_ty {
            MonoType::Function { params, ret } => (params, *ret),
            _ => unreachable!(),
        };
        if inst_params.len() != 2 {
            return Err(format!(
                "`compare` has {} params, expected 2 (self, other: Self)",
                inst_params.len()
            ));
        }
        // Both params must unify with the target type (snapshot errors to avoid noise)
        let errors_before = self.errors.len();
        let span = Span::new(crate::syntax::span::FileId(0), 0, 0);
        let ok0 = self.unify(&ty, &inst_params[0], span).is_ok();
        let ok1 = self.unify(&ty, &inst_params[1], span).is_ok();
        // Roll back any diagnostics from the silent proof
        self.errors.truncate(errors_before);
        if !ok0 {
            return Err("compare first param does not match receiver type".to_string());
        }
        if !ok1 {
            return Err("compare second param does not match receiver type".to_string());
        }
        // Return type must be Order
        let ret_ty = self.zonk(&inst_ret);
        let order_ty = MonoType::named(crate::types::ty::ORDER_TYPE_ID);
        if ret_ty != order_ty {
            return Err(format!(
                "`compare` returns {} (expected Order)",
                ret_ty.format_with_names(&self.type_env)
            ));
        }
        // Validate bounds on the compare method's type params
        for (name, meta_ty) in &var_to_meta {
            if sig
                .type_param_bounds
                .get(name)
                .map(|b| b.contains(&"Ord".to_string()))
                == Some(true)
            {
                let concrete = self.zonk(meta_ty);
                self.validate_ord_type(&concrete)?;
            }
        }
        Ok(())
    }

    /// Recover the element type `E` for a concrete container that satisfies one of
    /// the parameterized container contracts (`IndexRead<E>` / `IndexWrite<E>` /
    /// `IntoIterator<E>`). Builtins resolve directly (`Vector<T>` → `T`, `String` →
    /// `Byte`); a satisfier record like `View<C>` resolves through its inherent
    /// `at` method, whose return type is the element. Returns `None` when the
    /// container type is not yet concrete (e.g. an unsolved metavar).
    fn container_contract_elem(&mut self, concrete: &MonoType) -> Option<MonoType> {
        match concrete {
            MonoType::Vector(elem) => Some((**elem).clone()),
            MonoType::String => Some(MonoType::Byte),
            MonoType::Named { .. } => {
                let type_id = method_receiver_type_id(concrete)?;
                let method_info = self.type_env.get_method(type_id, "at").cloned()?;
                let sig = method_info
                    .signature
                    .or_else(|| self.value_env.get_function(&method_info.func_name).cloned())?;
                let full_fn_ty = MonoType::Function {
                    params: sig.params.clone(),
                    ret: Box::new(sig.ret.clone().unwrap_or(MonoType::Void)),
                };
                let (inst_ty, _) = self.instantiate_vars(&sig.type_params, &full_fn_ty);
                let (inst_params, inst_ret) = match inst_ty {
                    MonoType::Function { params, ret } => (params, *ret),
                    _ => return None,
                };
                let recv = inst_params.first()?;
                let span = Span::new(crate::syntax::span::FileId(0), 0, 0);
                let errors_before = self.errors.len();
                let ok = self.unify(concrete, recv, span).is_ok();
                self.errors.truncate(errors_before);
                if !ok {
                    return None;
                }
                let elem = self.zonk(&inst_ret);
                if matches!(elem, MonoType::MetaVar(_)) {
                    None
                } else {
                    Some(elem)
                }
            }
            _ => None,
        }
    }

    /// For a parameterized container bound `C: IndexRead<E>` (or `IndexWrite` /
    /// `IntoIterator`), recover `E` once `C` is concrete and unify it with the
    /// element metavar. `E` appears only in the bound — never in a value parameter
    /// — so without this its metavar stays unsolved (e.g. `view.at` returning an
    /// unresolved type). This is the contract's `Self -> Elem` functional
    /// dependency, mirrored from the boot monomorphizer.
    fn recover_container_bound_elems(
        &mut self,
        type_param_bounds: &HashMap<String, Vec<String>>,
        var_to_meta: &[(String, MonoType)],
    ) {
        for (name, meta_ty) in var_to_meta {
            let Some(bounds) = type_param_bounds.get(name) else {
                continue;
            };
            for bound in bounds {
                let Some(inner) = CONTAINER_ELEM_CONTRACTS
                    .iter()
                    .find_map(|contract| Self::contract_bound_arg(bound, contract))
                else {
                    continue;
                };
                // The element argument is itself a type param of this signature
                // (e.g. `E`); locate its metavar so we can solve it.
                let Some((_, elem_meta)) = var_to_meta.iter().find(|(n, _)| n == inner) else {
                    continue;
                };
                if !matches!(self.zonk(elem_meta), MonoType::MetaVar(_)) {
                    continue;
                }
                let concrete = self.zonk(meta_ty);
                if let Some(elem) = self.container_contract_elem(&concrete) {
                    let span = Span::new(crate::syntax::span::FileId(0), 0, 0);
                    let _ = self.unify(elem_meta, &elem, span);
                }
            }
        }
    }

    fn check_instantiated_contract_bounds(
        &mut self,
        sig: &crate::types::ty::FunctionSignature,
        var_to_meta: &[(String, MonoType)],
        span: Span,
    ) -> Result<(), ()> {
        self.recover_container_bound_elems(&sig.type_param_bounds, var_to_meta);
        for (name, meta_ty) in var_to_meta {
            let bounds = sig.type_param_bounds.get(name);
            if bounds.map(|b| b.contains(&"Stringify".to_string())) == Some(true) {
                let concrete = self.zonk(meta_ty);
                if let Err(reason) = self.validate_stringify_type(&concrete, &mut HashSet::new()) {
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "contract bound",
                        span,
                        note: format!(
                            "type argument {} does not satisfy Stringify: {}",
                            concrete.format_with_names(&self.type_env),
                            reason
                        ),
                    });
                    return Err(());
                }
            }
            if bounds.map(|b| b.contains(&"Ord".to_string())) == Some(true) {
                let concrete = self.zonk(meta_ty);
                if let Err(reason) = self.validate_ord_type(&concrete) {
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "contract bound",
                        span,
                        note: format!(
                            "type argument {} does not satisfy Ord: {}",
                            concrete.format_with_names(&self.type_env),
                            reason
                        ),
                    });
                    return Err(());
                }
            }
        }
        Ok(())
    }

    fn validate_interpolation_to_string(
        &mut self,
        expr: &Expr,
        expr_ty: &MonoType,
    ) -> Result<(), ()> {
        match self.validate_stringify_type(expr_ty, &mut HashSet::new()) {
            Ok(()) => Ok(()),
            Err(reason) => {
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "string interpolation",
                    span: expr.span,
                    note: format!(
                        "Cannot interpolate type {}: {}",
                        expr_ty.format_with_names(&self.type_env),
                        reason
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

        match self.value_env.lookup(&qualified) {
            Some(actual) => {
                self.unify(expected, &actual, span)?;
                self.type_map.set_expr_type(expr_id, self.zonk(expected));
                Ok(())
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

    fn try_synth_registered_method_call(
        &mut self,
        base: &Expr,
        base_ty: &MonoType,
        method: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<Option<MonoType>, ()> {
        // Capture expected return type up front so it cannot leak on early-exit
        // paths (e.g. method not found, arity mismatch).
        let call_expected = self.call_expected_ret.take();

        let receiver_type_id = if let Some(type_id) = method_receiver_type_id(base_ty) {
            type_id
        } else {
            // Not a method receiver — put expected back for fallback dispatch
            self.call_expected_ret = call_expected;
            return Ok(None);
        };
        let method_info =
            if let Some(info) = self.type_env.get_method(receiver_type_id, method).cloned() {
                info
            } else {
                self.call_expected_ret = call_expected;
                return Ok(None);
            };
        // Use stored signature directly (Option B) or fall back to ValueEnv
        // for builtin methods registered without a signature.
        let sig = if let Some(sig) = method_info.signature {
            sig
        } else if let Some(sig) = self.value_env.get_function(&method_info.func_name).cloned() {
            sig
        } else {
            self.call_expected_ret = call_expected;
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
        let (inst_ty, var_to_meta) = self.instantiate_vars(&sig.type_params, &full_fn_ty);
        let (inst_params, inst_ret) = match inst_ty {
            MonoType::Function { params, ret } => (params, *ret),
            _ => unreachable!(),
        };
        // Best-effort pre-unify: solve generic MetaVars from expected return
        // type before checking arguments. Errors are deliberately ignored —
        // the outer unify in check_expr will re-report any real mismatch.
        if let Some(expected_ret) = call_expected {
            let _ = self.unify(&inst_ret, &expected_ret, span);
        }
        if let Some(recv_ty) = inst_params.first() {
            self.unify(base_ty, recv_ty, base.span)?;
        }
        for (arg, expected_ty) in args.iter().zip(inst_params.iter().skip(1)) {
            self.check_expr(arg, expected_ty)?;
        }
        self.check_instantiated_contract_bounds(&sig, &var_to_meta, span)?;
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
        if let Some(ret_ty) =
            self.try_synth_registered_method_call(base, &base_ty, method, args, span)?
        {
            return Ok(ret_ty);
        }
        if method == "iter"
            && args.is_empty()
            && let Some(elem_ty) = self.into_iterator_bound_elem(&base_ty)
        {
            return Ok(MonoType::Named {
                type_id: crate::types::ty::ITERATOR_TYPE_ID,
                args: vec![elem_ty],
            });
        }
        if method == "len" && args.is_empty() && self.index_read_bound_elem(&base_ty).is_some() {
            return Ok(MonoType::Int);
        }
        if method == "at"
            && args.len() == 1
            && let Some(elem_ty) = self.index_read_bound_elem(&base_ty)
        {
            self.check_expr(&args[0], &MonoType::Int)?;
            return Ok(elem_ty);
        }
        if method == "to_string" && args.is_empty() && self.has_stringify_bound(&base_ty) {
            return Ok(MonoType::String);
        }
        if method == "compare" && args.len() == 1 && self.has_ord_bound(&base_ty) {
            self.check_expr(&args[0], &base_ty)?;
            return Ok(MonoType::named(crate::types::ty::ORDER_TYPE_ID));
        }

        if let MonoType::Named {
            type_id,
            args: named_args,
        } = base_ty.clone()
        {
            // If there is no inherent method, this may still be a capability
            // record function field call: `record.fn_field(args)`.
            if let Some(field_idx) = self.type_env.get_field_index(type_id, method)
                && let Some(fields) = self.type_env.get_record_fields(type_id)
            {
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
            return Err(());
        }

        match method_receiver_type_id(&base_ty).and_then(builtin_method_alias) {
            Some("Vector") => self.errors.push(TypeError::UnsupportedFeature {
                feature: "unknown vector method",
                span,
                note: format!("Vector has no method '{}'", method),
            }),
            Some("String") => self.errors.push(TypeError::UnsupportedFeature {
                feature: "unknown string method",
                span,
                note: format!("String has no method '{}'", method),
            }),
            Some("Dict") => self.errors.push(TypeError::UnsupportedFeature {
                feature: "unknown dict method",
                span,
                note: format!("Dict has no method '{}'", method),
            }),
            Some("Byte") => self.errors.push(TypeError::UnsupportedFeature {
                feature: "method on Byte type",
                span,
                note: format!("Byte has no method '{}'", method),
            }),
            Some("Int" | "Float" | "Bool") => self.errors.push(TypeError::UnsupportedFeature {
                feature: "method on primitive type",
                span,
                note: format!("Type {:?} has no method '{}'", base_ty, method),
            }),
            _ => self.errors.push(TypeError::UnsupportedFeature {
                feature: "unknown method call",
                span,
                note: format!(
                    "Type {} has no method '{}'",
                    base_ty.format_with_names(&self.type_env),
                    method
                ),
            }),
        }
        Err(())
    }

    //
    // Blocks
    //

    fn synth_block(&mut self, block: &Block) -> Result<MonoType, ()> {
        self.local_env.push_scope();
        let pending_base = self.pending_meta_let_bindings.len();

        let mut result_ty = MonoType::Void;
        let mut had_error = false;

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
                Stmt::Expr(e) => match self.synth_expr(e) {
                    Ok(ty) => result_ty = ty,
                    Err(()) => had_error = true,
                },
                Stmt::Return { value, span } => {
                    if let Some(ret_ty) = self.current_function_ret.clone() {
                        if let Some(val) = value {
                            if self.check_expr(val, &ret_ty).is_err() {
                                had_error = true;
                            }
                        } else if self.unify(&MonoType::Void, &ret_ty, *span).is_err() {
                            had_error = true;
                        }
                    } else if self.in_function {
                        // Inside a function with inferred return type.
                        // Bare return is valid (implies void); return-with-value
                        // is synthesized for type-mapping.
                        if let Some(val) = value {
                            let _ = self.synth_expr(val);
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
                    match self.synth_expr(expr) {
                        Ok(deferred_ty) => {
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
                        Err(()) => had_error = true,
                    }
                    result_ty = MonoType::Void;
                }
            }
        }

        self.drain_pending_meta_bindings(pending_base);
        self.local_env.pop_scope();
        if had_error { Err(()) } else { Ok(result_ty) }
    }

    /// Bidirectional block check: processes all statements like `synth_block`
    /// but uses `check_expr(last_expr, expected_ty)` for the final expression
    /// statement so that expected types flow into anonymous record literals,
    /// if-expressions, etc.
    fn check_block(&mut self, block: &Block, expected_ty: &MonoType) -> Result<(), ()> {
        self.local_env.push_scope();
        let pending_base = self.pending_meta_let_bindings.len();

        // Index of the last Expr statement (if any)
        let last_expr_idx = block.stmts.iter().rposition(|s| matches!(s, Stmt::Expr(_)));

        // Track whether the block ends with a diverging statement (Return/Break/Continue).
        // Diverging blocks have type Never, which unifies with any expected type.
        let mut diverges = false;
        let mut had_error = false;

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
                        if self.check_expr(e, expected_ty).is_err() {
                            had_error = true;
                        }
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
                    match self.synth_expr(expr) {
                        Ok(deferred_ty) => {
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
                        Err(()) => had_error = true,
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

        self.drain_pending_meta_bindings(pending_base);
        self.local_env.pop_scope();
        if had_error { Err(()) } else { Ok(()) }
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
                if let Some(ann_ty) = ty {
                    // Type annotation provided - check mode.
                    // Even if checking fails, keep a binding to avoid noisy
                    // follow-up "undefined variable" diagnostics.
                    let expected = match self.resolve_type(ann_ty) {
                        Ok(t) => t,
                        Err(()) => {
                            let recovery_ty = self.fresh_meta();
                            self.local_env.bind(name.clone(), recovery_ty);
                            return;
                        }
                    };
                    if self.check_expr(value, &expected).is_err() {
                        self.local_env.bind(name.clone(), expected);
                        return;
                    }
                    self.local_env.bind(name.clone(), expected);
                } else {
                    // No annotation - synthesis mode.
                    let t = match self.synth_expr(value) {
                        Ok(t) => t,
                        Err(()) => {
                            let recovery_ty = self.fresh_meta();
                            self.local_env.bind(name.clone(), recovery_ty);
                            return;
                        }
                    };
                    let t = self.zonk(&t);
                    if contains_meta(&t) {
                        if matches!(&t, MonoType::Dict(_, _) | MonoType::Vector(_)) {
                            // Defer: downstream usage in this scope may resolve the MetaVars.
                            self.pending_meta_let_bindings
                                .push((name.clone(), value.span));
                            self.local_env.bind(name.clone(), t);
                            return;
                        }
                        self.errors.push(TypeError::AmbiguousType {
                            name: name.clone(),
                            span: value.span,
                            note: "type cannot be inferred; add a type annotation".to_string(),
                        });
                        let recovery_ty = self.fresh_meta();
                        self.local_env.bind(name.clone(), recovery_ty);
                        return;
                    }
                    self.local_env.bind(name.clone(), t);
                }
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
            // If then_ty is concrete, use it as context for the else branch so that
            // variant shorthands like `.Void` can be resolved.
            let then_zonked = self.zonk(&then_ty);
            let else_ty = if !contains_meta(&then_zonked) && then_zonked != MonoType::Never {
                self.check_expr(else_expr, &then_zonked)?;
                then_zonked.clone()
            } else {
                let else_ty = self.synth_expr(else_expr)?;
                self.unify(&then_ty, &else_ty, else_expr.span)?;
                else_ty
            };
            // If one branch diverges (Never), use the other branch's type
            if then_zonked == MonoType::Never {
                Ok(else_ty)
            } else {
                Ok(then_zonked)
            }
        } else {
            // No else branch - result type is Void
            self.unify(&then_ty, &MonoType::Void, then_branch.span)?;
            Ok(MonoType::Void)
        }
    }

    fn synth_cond(&mut self, arms: &[CondArm], _span: Span) -> Result<MonoType, ()> {
        let has_default = arms.last().is_some_and(|a| a.condition.is_none());
        let mut result_ty: Option<MonoType> = None;

        for arm in arms {
            if let Some(cond) = &arm.condition {
                self.check_expr(cond, &MonoType::Bool)?;
            }
            let arm_ty = self.synth_expr(&arm.body)?;
            let arm_zonked = self.zonk(&arm_ty);
            if arm_zonked == MonoType::Never {
                continue;
            }
            match &result_ty {
                Some(rt) => {
                    self.unify(&arm_zonked, rt, arm.span)?;
                }
                None => {
                    result_ty = Some(arm_zonked);
                }
            }
        }

        if !has_default {
            Ok(MonoType::Void)
        } else {
            Ok(result_ty.unwrap_or(MonoType::Never))
        }
    }

    //
    // Field access
    //

    /// Synthesize a first-class method value reference: `receiver.method` → `fn(args...) ret`.
    /// The receiver is already bound, so the returned function type drops the first param.
    fn synth_method_value_ref(
        &mut self,
        base_ty: &MonoType,
        type_id: TypeId,
        method: &str,
        span: Span,
    ) -> Result<MonoType, ()> {
        if method == "to_string" && self.has_stringify_bound(base_ty) {
            return Ok(MonoType::Function {
                params: vec![],
                ret: Box::new(MonoType::String),
            });
        }

        let method_info = if let Some(info) = self.type_env.get_method(type_id, method).cloned() {
            info
        } else {
            let type_name = self
                .type_env
                .get_def(type_id)
                .map(|d| d.name().to_string())
                .or_else(|| builtin_method_alias(type_id).map(|name| name.to_string()))
                .unwrap_or_else(|| format!("Type#{}", type_id.0));
            self.errors.push(TypeError::NoSuchField {
                record_type: type_name,
                field: method.to_string(),
                span,
            });
            return Err(());
        };
        let sig = if let Some(sig) = method_info.signature {
            sig
        } else if let Some(sig) = self.value_env.get_function(&method_info.func_name).cloned() {
            sig
        } else {
            self.errors.push(TypeError::UndefinedVariable {
                name: method_info.func_name,
                span,
            });
            return Err(());
        };
        let full_fn_ty = MonoType::Function {
            params: sig.params.clone(),
            ret: Box::new(sig.ret.clone().unwrap_or(MonoType::Void)),
        };
        let (inst_ty, _var_to_meta) = self.instantiate_vars(&sig.type_params, &full_fn_ty);
        let (inst_params, inst_ret) = match inst_ty {
            MonoType::Function { params, ret } => (params, *ret),
            _ => unreachable!(),
        };
        // Unify receiver param with base type
        if let Some(recv_ty) = inst_params.first() {
            self.unify(base_ty, recv_ty, span)?;
        }
        // Return function type with remaining params (receiver stripped)
        let remaining_params: Vec<MonoType> = inst_params.into_iter().skip(1).collect();
        let ret = self.zonk(&inst_ret);
        let remaining_params: Vec<MonoType> =
            remaining_params.iter().map(|p| self.zonk(p)).collect();
        Ok(MonoType::Function {
            params: remaining_params,
            ret: Box::new(ret),
        })
    }

    /// Extract a dotted name from an Ident/FieldAccess chain and look it up as a type.
    /// Handles both `TypeName` (bare Ident) and `module.TypeName` (FieldAccess chain).
    /// Returns None if the expression is not an identifier chain or does not resolve to a type.
    fn try_resolve_type_from_expr(&self, expr: &Expr) -> Option<TypeId> {
        let name = expr_as_dotted_name(expr)?;
        self.type_env.lookup_type(&name)
    }

    /// True when `base.name` denotes a qualified variant constructor
    /// (e.g. `Option.Some`, `UserEnum.Variant`). Used to let a name that is both
    /// a module alias and a type (Option, Result) construct a variant rather than
    /// being intercepted as a module-qualified reference.
    fn is_qualified_variant(&self, base: &Expr, name: &str) -> bool {
        self.try_resolve_type_from_expr(base)
            .map(|tid| self.type_env.get_variant_index(tid, name).is_some())
            .unwrap_or(false)
    }

    /// Field types and result type of a qualified variant constructor
    /// `Type.Variant`, with fresh metavars for the type parameters. Builtin
    /// Option/Result store placeholder field types in their TypeDef, so they are
    /// special-cased to use their type arguments (mirroring `check_variant_lit`).
    /// The caller must have verified the variant exists.
    fn qualified_variant_signature(
        &mut self,
        type_id: TypeId,
        variant_name: &str,
    ) -> (Vec<MonoType>, MonoType) {
        if type_id == OPTION_TYPE_ID {
            let m = self.fresh_meta();
            let fields = if variant_name == "Some" {
                vec![m.clone()]
            } else {
                vec![]
            };
            return (
                fields,
                MonoType::Named {
                    type_id,
                    args: vec![m],
                },
            );
        }
        if type_id == RESULT_TYPE_ID {
            let ok = self.fresh_meta();
            let err = self.fresh_meta();
            let fields = if variant_name == "Ok" {
                vec![ok.clone()]
            } else {
                vec![err.clone()]
            };
            return (
                fields,
                MonoType::Named {
                    type_id,
                    args: vec![ok, err],
                },
            );
        }
        // User-defined sum type: instantiate type params with fresh metas.
        let type_params: Vec<String> = self
            .type_env
            .get_def(type_id)
            .map(|d| d.type_params().to_vec())
            .unwrap_or_default();
        let variant_idx = self
            .type_env
            .get_variant_index(type_id, variant_name)
            .expect("caller verified variant exists");
        let raw_fields = self.type_env.get_variants(type_id).expect("variant exists")[variant_idx]
            .fields
            .clone();
        let inst_map: HashMap<String, MonoType> = type_params
            .iter()
            .map(|p| (p.clone(), self.fresh_meta()))
            .collect();
        let type_var_args: Vec<MonoType> = type_params
            .iter()
            .map(|p| MonoType::Var(p.clone()))
            .collect();
        let named = apply_subst(
            &MonoType::Named {
                type_id,
                args: type_var_args,
            },
            &inst_map,
        );
        let fields: Vec<MonoType> = raw_fields
            .iter()
            .map(|f| apply_subst(f, &inst_map))
            .collect();
        (fields, named)
    }

    fn synth_field_access(&mut self, base: &Expr, field: &str, span: Span) -> Result<MonoType, ()> {
        // Check for TypeName.Variant or module.TypeName.Variant syntax
        if let Some(type_id) = self.try_resolve_type_from_expr(base)
            && self.type_env.get_variant_index(type_id, field).is_some()
        {
            let (variant_fields, named_ty) = self.qualified_variant_signature(type_id, field);
            // Record type of the type-name base as Named (so lowerer can identify it)
            self.type_map.set_expr_type(base.id, named_ty.clone());
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
                    let subst = build_type_subst(&type_params, type_args);
                    // Find the field
                    for f in record_fields {
                        if f.name == field {
                            return Ok(apply_subst(&f.ty, &subst));
                        }
                    }

                    // Field not found - check if it's a method value reference
                    if has_method {
                        return self.synth_method_value_ref(&base_ty, type_id, field, span);
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
                } else if has_method {
                    // Non-record Named type (e.g. enum) with a method
                    self.synth_method_value_ref(&base_ty, type_id, field, span)
                } else {
                    // Not a record type and no method
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
                // Check for registered method value reference (e.g. xs.map, n.to_string).
                if let Some(type_id) = method_receiver_type_id(&base_ty)
                    && self.type_env.has_method(type_id, field)
                {
                    return self.synth_method_value_ref(&base_ty, type_id, field, span);
                }
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

    fn synth_array(&mut self, elements: &[Expr], _span: Span) -> Result<MonoType, ()> {
        if elements.is_empty() {
            // Emit a fresh MetaVar for the element type; downstream usage resolves it.
            let elem = self.fresh_meta();
            return Ok(MonoType::Vector(Box::new(elem)));
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
            // Also supports alias constructors: type P = Point; P.{ x: 1, y: 2 }
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

            // Follow alias chains to find the canonical record type
            let (type_id, alias_args) =
                self.canonicalize_record_constructor(type_id, type_name, span)?;

            let type_params = self
                .type_env
                .get_def(type_id)
                .map(|d| d.type_params().to_vec())
                .unwrap_or_default();

            if type_params.is_empty() || alias_args.len() == type_params.len() {
                // Non-generic, or alias already provides all type args (e.g. IntBox = Box<Int>)
                self.check_record_lit_fields(type_id, &alias_args, fields, span)?;
                if alias_args.is_empty() {
                    Ok(MonoType::named(type_id))
                } else {
                    Ok(MonoType::Named {
                        type_id,
                        args: alias_args,
                    })
                }
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
                    if let Ok(actual_ty) = &result
                        && let Some((_, declared_ty)) =
                            def_fields.iter().find(|(n, _)| n == provided_name)
                    {
                        let inst_decl_ty = apply_subst(declared_ty, &inst_map);
                        let _ = self.unify(actual_ty, &inst_decl_ty, provided_expr.span);
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

    /// Follow alias chains from a constructor name to find the canonical record TypeId.
    /// Returns `(record_type_id, concrete_type_args)` or errors if the target is not a record.
    fn canonicalize_record_constructor(
        &mut self,
        mut type_id: TypeId,
        type_name: &str,
        span: Span,
    ) -> Result<(TypeId, Vec<MonoType>), ()> {
        let mut args: Vec<MonoType> = Vec::new();
        loop {
            match self.type_env.get_def(type_id) {
                Some(TypeDef::Record { .. }) => return Ok((type_id, args)),
                Some(TypeDef::Alias { target, .. }) => match target {
                    MonoType::Named {
                        type_id: target_id,
                        args: target_args,
                    } => {
                        args = target_args.clone();
                        type_id = *target_id;
                    }
                    other => {
                        self.errors.push(TypeError::NotARecordConstructor {
                            name: type_name.to_string(),
                            resolved: format!("{}", other),
                            span,
                        });
                        return Err(());
                    }
                },
                Some(TypeDef::Sum { name: sum_name, .. }) => {
                    self.errors.push(TypeError::NotARecordConstructor {
                        name: type_name.to_string(),
                        resolved: sum_name.clone(),
                        span,
                    });
                    return Err(());
                }
                _ => {
                    self.errors.push(TypeError::NotARecordConstructor {
                        name: type_name.to_string(),
                        resolved: type_name.to_string(),
                        span,
                    });
                    return Err(());
                }
            }
        }
    }

    fn check_anon_record_lit(
        &mut self,
        fields: &[(String, Expr)],
        expected: &MonoType,
        span: Span,
    ) -> Result<(), ()> {
        let expected = self.zonk(expected);
        match &expected {
            MonoType::Named { type_id, args } => {
                self.check_record_lit_fields(*type_id, args, fields, span)
            }
            _ => {
                self.errors.push(TypeError::TypeMismatch {
                    expected,
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
                let variant = variants.iter().find(|v| v.name == variant_name);

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

        // Scrutinee must be a sum type or a matchable primitive (Int, Bool, String, Byte)
        let is_primitive_match = matches!(
            scrut_ty,
            MonoType::Int | MonoType::Bool | MonoType::String | MonoType::Byte
        );
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

        let mut result_ty: Option<MonoType> = None;
        let mut deferred_arms = Vec::new();

        // First synthesize arms that do not require contextual expected types.
        // This keeps case-arm inference order-independent: `.Void` can appear
        // before the arm that reveals the enclosing sum type.
        for arm in arms {
            if Self::case_arm_needs_expected_type(&arm.body) {
                deferred_arms.push(arm);
                continue;
            }

            let arm_ty = self.synth_case_arm(arm, &scrut_ty)?;
            let arm_ty = self.zonk(&arm_ty);
            if arm_ty == MonoType::Never {
                continue;
            }

            match &result_ty {
                Some(current) => self.unify(&arm_ty, current, arm.span)?,
                None => result_ty = Some(arm_ty.clone()),
            }
        }

        if let Some(join_ty) = result_ty.clone().map(|ty| self.zonk(&ty))
            && !contains_meta(&join_ty)
        {
            for arm in deferred_arms {
                self.check_case_arm(arm, &scrut_ty, &join_ty)?;
            }
            return Ok(join_ty);
        }

        if deferred_arms.is_empty() {
            return Ok(result_ty
                .map(|ty| self.zonk(&ty))
                .unwrap_or(MonoType::Never));
        }

        // No concrete join type was available. Re-run deferred arms in synth
        // mode to surface the existing "needs context" diagnostics.
        let mut had_error = false;
        for arm in deferred_arms {
            if self.synth_case_arm(arm, &scrut_ty).is_err() {
                had_error = true;
            }
        }

        if had_error {
            Err(())
        } else {
            Ok(result_ty
                .map(|ty| self.zonk(&ty))
                .unwrap_or(MonoType::Never))
        }
    }

    fn case_arm_needs_expected_type(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::VariantLit { .. } => true,
            ExprKind::RecordLit { name: None, .. } => true,
            ExprKind::Block(block) => Self::block_needs_expected_type(block),
            _ => false,
        }
    }

    fn block_needs_expected_type(block: &Block) -> bool {
        block
            .stmts
            .iter()
            .rposition(|stmt| matches!(stmt, Stmt::Expr(_)))
            .and_then(|idx| match &block.stmts[idx] {
                Stmt::Expr(expr) => Some(Self::case_arm_needs_expected_type(expr)),
                _ => None,
            })
            .unwrap_or(false)
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
        let pat_result = pattern_checker.check_pattern(&arm.pattern, scrut_ty);

        if pat_result.is_err() {
            self.local_env.pop_scope();
            return Err(());
        }

        // Type-check the arm body
        let body_result = self.synth_expr(&arm.body);

        self.local_env.pop_scope();
        body_result
    }

    fn check_case_arm(
        &mut self,
        arm: &crate::syntax::ast::CaseArm,
        scrut_ty: &MonoType,
        expected: &MonoType,
    ) -> Result<(), ()> {
        self.local_env.push_scope();

        let mut pattern_checker =
            PatternChecker::new(&self.type_env, &mut self.local_env, &mut self.errors);
        let pat_result = pattern_checker.check_pattern(&arm.pattern, scrut_ty);

        if pat_result.is_err() {
            self.local_env.pop_scope();
            return Err(());
        }

        let body_result = self.check_expr(&arm.body, expected);

        self.local_env.pop_scope();
        body_result
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
                if !self.in_function && self.pub_bindings.contains(name) {
                    self.errors.push(TypeError::PubBindingRebinding {
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
                if let Some(idx_pat) = index_pattern
                    && let Pattern::Ident(name, _) = idx_pat
                {
                    self.local_env.bind(name.clone(), MonoType::Int);
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
                if let Some(idx_pat) = index_pattern
                    && let Pattern::Ident(name, _) = idx_pat
                {
                    self.local_env.bind(name.clone(), MonoType::Int);
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
                if let Some(idx_pat) = index_pattern
                    && let Pattern::Ident(name, _) = idx_pat
                {
                    self.local_env.bind(name.clone(), MonoType::Int);
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
                if let Some(idx_pat) = index_pattern
                    && let Pattern::Ident(name, _) = idx_pat
                {
                    self.local_env.bind(name.clone(), MonoType::Int);
                }
            }
            other if self.into_iterator_bound_elem(&other).is_some() => {
                let elem_ty = self
                    .into_iterator_bound_elem(&other)
                    .unwrap_or(MonoType::Void);
                match pattern {
                    Pattern::Ident(name, _) => self.local_env.bind(name.clone(), elem_ty),
                    Pattern::Wildcard(_) => {}
                    _ => {
                        self.errors.push(TypeError::UnsupportedFeature {
                            feature: "complex pattern in for loop over IntoIterator",
                            span: iter.span,
                            note: "Only simple identifiers are supported in for loop patterns over IntoIterator".to_string(),
                        });
                    }
                }
                if let Some(idx_pat) = index_pattern
                    && let Pattern::Ident(name, _) = idx_pat
                {
                    self.local_env.bind(name.clone(), MonoType::Int);
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
                if let Some(val_pat) = index_pattern
                    && let Pattern::Ident(name, _) = val_pat
                {
                    self.local_env.bind(name.clone(), *val_ty);
                }
            }
            other => {
                self.errors.push(TypeError::TypeMismatch {
                    expected: MonoType::Vector(Box::new(MonoType::Int)),
                    actual: other,
                    span: iter.span,
                    note: None,
                });
                self.local_env.pop_scope();
                return;
            }
        }

        let _ = self.synth_block(body);
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
        self.collect_impl(pattern, index_pattern, iter, body, span, None)
    }

    fn check_collect(
        &mut self,
        pattern: &Pattern,
        index_pattern: Option<&Pattern>,
        iter: &Expr,
        body: &Expr,
        span: Span,
        expected_elem: &MonoType,
    ) -> Result<MonoType, ()> {
        self.collect_impl(
            pattern,
            index_pattern,
            iter,
            body,
            span,
            Some(expected_elem),
        )
    }

    fn collect_impl(
        &mut self,
        pattern: &Pattern,
        index_pattern: Option<&Pattern>,
        iter: &Expr,
        body: &Expr,
        span: Span,
        expected_elem: Option<&MonoType>,
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
                if let Some(idx_pat) = index_pattern
                    && let Pattern::Ident(name, _) = idx_pat
                {
                    self.local_env.bind(name.clone(), MonoType::Int);
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
                if let Some(idx_pat) = index_pattern
                    && let Pattern::Ident(name, _) = idx_pat
                {
                    self.local_env.bind(name.clone(), MonoType::Int);
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
                if let Some(idx_pat) = index_pattern
                    && let Pattern::Ident(name, _) = idx_pat
                {
                    self.local_env.bind(name.clone(), MonoType::Int);
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
                if let Some(idx_pat) = index_pattern
                    && let Pattern::Ident(name, _) = idx_pat
                {
                    self.local_env.bind(name.clone(), MonoType::Int);
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
                if let Some(val_pat) = index_pattern
                    && let Pattern::Ident(name, _) = val_pat
                {
                    self.local_env.bind(name.clone(), *val_ty);
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
        } else if let Some(expected) = expected_elem {
            match self.check_expr(body, expected) {
                Ok(()) => expected.clone(),
                Err(()) => {
                    self.local_env.pop_scope();
                    return Err(());
                }
            }
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

/// Extract a dotted name from an Ident/FieldAccess expression chain.
/// `Ident("Type")` → `Some("Type")`
/// `FieldAccess { base: Ident("module"), field: "Type" }` → `Some("module.Type")`
fn expr_as_dotted_name(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Ident(name) => Some(name.clone()),
        ExprKind::FieldAccess { base, field } => {
            expr_as_dotted_name(base).map(|prefix| format!("{}.{}", prefix, field))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Dependency-ordered top-level checking
// ---------------------------------------------------------------------------

/// Collect all free identifier references from an expression.
/// `locals` tracks names bound in enclosing scopes (excluded from results).
fn collect_expr_refs(expr: &Expr, locals: &HashSet<String>, out: &mut HashSet<String>) {
    match &expr.kind {
        ExprKind::Literal(_) => {}
        ExprKind::Ident(name) => {
            if !locals.contains(name.as_str()) {
                out.insert(name.clone());
            }
        }
        ExprKind::Binary { left, right, .. } => {
            collect_expr_refs(left, locals, out);
            collect_expr_refs(right, locals, out);
        }
        ExprKind::Unary { expr: e, .. } => {
            collect_expr_refs(e, locals, out);
        }
        ExprKind::Call { callee, args } => {
            collect_expr_refs(callee, locals, out);
            for arg in args {
                collect_expr_refs(arg, locals, out);
            }
        }
        ExprKind::FieldAccess { base, .. } => {
            collect_expr_refs(base, locals, out);
        }
        ExprKind::Index { base, index } => {
            collect_expr_refs(base, locals, out);
            collect_expr_refs(index, locals, out);
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr_refs(cond, locals, out);
            collect_expr_refs(then_branch, locals, out);
            if let Some(e) = else_branch {
                collect_expr_refs(e, locals, out);
            }
        }
        ExprKind::Case { scrutinee, arms } => {
            collect_expr_refs(scrutinee, locals, out);
            for arm in arms {
                let mut arm_locals = locals.clone();
                collect_pattern_names(&arm.pattern, &mut arm_locals);
                collect_expr_refs(&arm.body, &arm_locals, out);
            }
        }
        ExprKind::Cond { arms } => {
            for arm in arms {
                if let Some(cond) = &arm.condition {
                    collect_expr_refs(cond, locals, out);
                }
                collect_expr_refs(&arm.body, locals, out);
            }
        }
        ExprKind::Block(block) => {
            collect_block_refs(block, locals, out);
        }
        ExprKind::Array { elements } => {
            for e in elements {
                collect_expr_refs(e, locals, out);
            }
        }
        ExprKind::RecordLit { fields, .. } => {
            for (_, value) in fields {
                collect_expr_refs(value, locals, out);
            }
        }
        ExprKind::VariantLit { fields, .. } => {
            for f in fields {
                collect_expr_refs(f, locals, out);
            }
        }
        ExprKind::Function(fn_expr) => {
            let mut fn_locals = locals.clone();
            for param in &fn_expr.params {
                fn_locals.insert(param.name.clone());
            }
            collect_expr_refs(&fn_expr.body, &fn_locals, out);
        }
        ExprKind::Collect {
            pattern,
            index_pattern,
            iter,
            body,
        } => {
            collect_expr_refs(iter, locals, out);
            let mut collect_locals = locals.clone();
            collect_pattern_names(pattern, &mut collect_locals);
            if let Some(ip) = index_pattern {
                collect_pattern_names(ip, &mut collect_locals);
            }
            collect_expr_refs(body, &collect_locals, out);
        }
        ExprKind::CollectWhile { cond, body } => {
            collect_expr_refs(cond, locals, out);
            collect_expr_refs(body, locals, out);
        }
        ExprKind::Try { expr: e } => {
            collect_expr_refs(e, locals, out);
        }
        ExprKind::StringInterpolation { parts } => {
            for part in parts {
                if let StringPart::Interpolation(e) = part {
                    collect_expr_refs(e, locals, out);
                }
            }
        }
    }
}

fn collect_block_refs(block: &Block, locals: &HashSet<String>, out: &mut HashSet<String>) {
    let mut block_locals = locals.clone();
    for stmt in &block.stmts {
        collect_stmt_refs(stmt, &block_locals, out);
        // Let bindings introduce locals that shadow top-level names
        if let Stmt::Let { pattern, .. } = stmt {
            collect_pattern_names(pattern, &mut block_locals);
        }
    }
}

fn collect_stmt_refs(stmt: &Stmt, locals: &HashSet<String>, out: &mut HashSet<String>) {
    match stmt {
        Stmt::Let { value, .. } => {
            collect_expr_refs(value, locals, out);
        }
        Stmt::Expr(e) => {
            collect_expr_refs(e, locals, out);
        }
        Stmt::For {
            pattern,
            index_pattern,
            iter,
            body,
            ..
        } => {
            collect_expr_refs(iter, locals, out);
            let mut for_locals = locals.clone();
            collect_pattern_names(pattern, &mut for_locals);
            if let Some(ip) = index_pattern {
                collect_pattern_names(ip, &mut for_locals);
            }
            collect_block_refs(body, &for_locals, out);
        }
        Stmt::ForCond { cond, body, .. } => {
            collect_expr_refs(cond, locals, out);
            collect_block_refs(body, locals, out);
        }
        Stmt::Break { value, .. } => {
            if let Some(v) = value {
                collect_expr_refs(v, locals, out);
            }
        }
        Stmt::Continue { .. } => {}
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                collect_expr_refs(v, locals, out);
            }
        }
        Stmt::Defer { expr, .. } => {
            collect_expr_refs(expr, locals, out);
        }
    }
}

fn collect_pattern_names(pattern: &Pattern, names: &mut HashSet<String>) {
    match pattern {
        Pattern::Ident(name, _) => {
            names.insert(name.clone());
        }
        Pattern::Variant { fields, .. } => {
            for f in fields {
                collect_pattern_names(f, names);
            }
        }
        Pattern::Wildcard(_) | Pattern::Literal(_, _) => {}
    }
}

/// Topologically sort top-level items by their value-level dependencies.
///
/// Returns indices into `ast.items` in an order where each item is checked
/// after everything it depends on.  Items with no dependency relationship
/// preserve their original source order (stable Kahn's with FIFO queue).
///
/// Only items whose types need inference create real dependencies:
/// - Unannotated functions (return type inferred from body)
/// - Top-level let bindings (type determined by checking)
///
/// Fully-annotated functions have no incoming edges — their signatures are
/// already registered by the resolver, so referencing them doesn't require
/// waiting for their bodies to be checked.
fn topo_sort_top_level(ast: &SourceFile, _value_env: &ValueEnv) -> Vec<usize> {
    // Collect the set of top-level names and which items define them.
    let mut name_to_idx: HashMap<&str, usize> = HashMap::new();
    let mut needs_inference: HashSet<&str> = HashSet::new();

    for (idx, item) in ast.items.iter().enumerate() {
        match item {
            Item::Function(decl) => {
                name_to_idx.insert(&decl.name, idx);
                if decl.return_type.is_none() {
                    needs_inference.insert(&decl.name);
                }
            }
            Item::Stmt(Stmt::Let {
                pattern: Pattern::Ident(name, _),
                ..
            }) => {
                name_to_idx.insert(name.as_str(), idx);
                // All lets need checking before their types are known
                needs_inference.insert(name.as_str());
            }
            _ => {}
        }
    }

    // Build dependency edges: item_index → set of item_indices it depends on.
    let n = ast.items.len();
    let mut deps: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    let locals = HashSet::new(); // no enclosing scope at top level

    for (idx, item) in ast.items.iter().enumerate() {
        let mut refs = HashSet::new();
        match item {
            Item::Function(decl) => {
                let mut fn_locals = locals.clone();
                for param in &decl.params {
                    fn_locals.insert(param.name.clone());
                }
                collect_block_refs(&decl.body, &fn_locals, &mut refs);
            }
            Item::Stmt(Stmt::Let { value, .. }) => {
                collect_expr_refs(value, &locals, &mut refs);
            }
            Item::Stmt(stmt) => {
                collect_stmt_refs(stmt, &locals, &mut refs);
            }
            _ => continue,
        }

        // Convert name references to item indices, filtering to only
        // real dependencies (items that need inference before use).
        for name in &refs {
            if let Some(&dep_idx) = name_to_idx.get(name.as_str())
                && dep_idx != idx
                && needs_inference.contains(name.as_str())
            {
                deps[idx].insert(dep_idx);
            }
        }
    }

    // Kahn's algorithm with FIFO queue for source-order stability.
    let mut in_degree: Vec<usize> = vec![0; n];
    let mut reverse: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut checkable: Vec<bool> = vec![false; n];

    for (idx, item) in ast.items.iter().enumerate() {
        match item {
            Item::Function(_) | Item::Stmt(_) => {
                checkable[idx] = true;
                in_degree[idx] = deps[idx].len();
                for &dep in &deps[idx] {
                    reverse[dep].push(idx);
                }
            }
            _ => {}
        }
    }

    let mut queue: VecDeque<usize> = VecDeque::new();
    for idx in 0..n {
        if checkable[idx] && in_degree[idx] == 0 {
            queue.push_back(idx);
        }
    }

    let mut result: Vec<usize> = Vec::with_capacity(n);
    while let Some(idx) = queue.pop_front() {
        result.push(idx);
        for &dependent in &reverse[idx] {
            in_degree[dependent] -= 1;
            if in_degree[dependent] == 0 {
                queue.push_back(dependent);
            }
        }
    }

    // Any remaining items are in dependency cycles. Append them in source
    // order; unannotated functions already have Pass-0 MetaVar returns, so
    // recursive call sites share the same inferred type where constraints solve it.
    for idx in 0..n {
        if checkable[idx] && in_degree[idx] > 0 {
            result.push(idx);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::parse_source;

    fn typecheck(source: &str) -> Result<TypedModule, Vec<TypeError>> {
        let (ast, _) = parse_source(source, "test.tw").expect("parse should succeed");
        let resolved = crate::types::Resolver::resolve(&ast, TypeEnv::new(), ValueEnv::new())
            .expect("resolve should succeed");
        let aliases = crate::intrinsics::registry::builtin_module_aliases()
            .iter()
            .map(|s| s.to_string())
            .collect::<HashSet<_>>();
        TypeChecker::check_module(&ast, resolved.type_env, resolved.value_env, aliases)
    }

    #[test]
    fn test_range_expr_produces_range_type() {
        // `0..10` should typecheck and produce a Range value
        let result = typecheck("x := 0..10");
        assert!(
            result.is_ok(),
            "expected 0..10 to typecheck, got: {result:?}"
        );
        let module = result.unwrap();
        let ty = module.value_env.lookup("x").expect("expected binding 'x'");
        assert!(
            matches!(ty, MonoType::Named { type_id, .. } if type_id == RANGE_TYPE_ID),
            "expected x to have Range type, got: {ty:?}"
        );
    }

    #[test]
    fn test_range_expr_rejects_non_int_operand() {
        // `"a"..10` should fail: String is not Int
        let result = typecheck(r#"x := "a"..10"#);
        assert!(
            result.is_err(),
            "expected type error for String operand in range"
        );
        let errors = result.unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, TypeError::TypeMismatch { .. })),
            "expected TypeMismatch error, got: {errors:?}"
        );
    }

    #[test]
    fn test_pub_top_level_rebind_is_rejected() {
        let result = typecheck("pub x := 1\nx = 2");
        assert!(result.is_err(), "expected public rebinding to fail");
        let errors = result.unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, TypeError::PubBindingRebinding { name, .. } if name == "x")),
            "expected PubBindingRebinding error, got: {errors:?}"
        );
    }

    #[test]
    fn test_unannotated_function_return_type_is_shared_with_call_sites() {
        let result = typecheck(
            "fn inferred_value() {\n  19 + 23\n}\n\
             fn calls_forward() Int {\n  forward_helper() + 1\n}\n\
             fn forward_helper() {\n  41\n}\n\
             fn countdown_sum(n: Int) {\n  if n <= 0 { 0 } else { n + countdown_sum(n - 1) }\n}\n\
             x := calls_forward() + countdown_sum(5)\n\
             inferred_value()\n",
        );
        assert!(
            result.is_ok(),
            "expected inferred return types to flow to all call sites, got: {result:?}"
        );
    }

    #[test]
    fn test_function_can_shadow_pub_top_level_binding() {
        let result = typecheck("pub x := 1\ny := fn() Int {\n  x := 2\n  x = x + 1\n  x\n}");
        assert!(
            result.is_ok(),
            "expected function-local shadow/rebind to typecheck, got: {result:?}"
        );
    }
}
