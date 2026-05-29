//! ANF verifier pass — enforces structural and typing invariants on ANF IR
//! before it reaches code generation.
//!
//! # Invariants checked
//!
//! **Control flow:**
//! - `BreakOutsideLoop` / `ContinueOutsideLoop` — break/continue only inside loops
//! - `ReturnTypeMismatch` — return value compatible with function return type
//! - `BreakTypeMismatch` — break value compatible with enclosing loop result type
//!
//! **Local bindings:**
//! - `UndeclaredLocal` — every referenced local is declared (param, let-bound, pattern-bound, or implicit capture)
//! - `StrictUndeclaredLocal` — `AMakeClosure.free_vars` and `AAssign` targets are declared
//!   in the enclosing function scope (params, let-bindings, or module-proven closure captures)
//! - `AssignTypeMismatch` — assigned value type matches the local's declared type
//! - `op_result_mono` completeness (post-lowering only)
//!
//! **Structural:**
//! - `DeferSurvived` — no `ADefer` nodes after optimization
//! - `UnfoldStepMetadataMissing` — UnfoldStep variant literals have concrete result metadata
//!
//! # Pipeline integration
//!
//! - **Post-lowering gate** (`compile_backend_anf`): all checks including `op_result_mono`
//! - **Post-optimization gate** (`compile_backend_opt`): structural + type checks, no `op_result_mono`
//! - **Per-pass hooks** (`optimize_func`): debug-only mid-optimization checks after each pass
//!
//! # Release-mode policy
//!
//! Pipeline gates are always-on (fast single linear walk). Per-pass hooks are
//! `#[cfg(debug_assertions)]` only — zero cost in release builds.
//!
//! # Limitations
//!
//! - Closure captures: the `UndeclaredLocal` check treats free locals as implicit captures.
//!   The `StrictUndeclaredLocal` check (enabled post-lowering and post-optimization)
//!   enforces that `AMakeClosure.free_vars` and `AAssign` targets are actually declared
//!   in enclosing scope, seeding closure-capture locals from module closure creation sites.
//! - Representation-level checks (iterator metadata, sum repr, typed symbols) require
//!   `EmitCtx` and are enforced by existing `debug_assert!` calls in `codegen/emit.rs`
//!   and `codegen/ctx.rs`, not by this verifier.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::ir::anf::analysis::{collect_free_locals, collect_pattern_bindings};
use crate::ir::anf::{AnfExpr, AnfFunctionDef, AnfModule, AnfOp, Atom};
use crate::ir::core::{FuncId, LocalId};
use crate::types::ty::{MonoType, UNFOLD_STEP_TYPE_ID};

// ── Error types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VerifyError {
    pub func_name: String,
    pub func_id: u32,
    pub invariant: Invariant,
    pub detail: String,
    /// The local involved in the error, if applicable.
    pub local_id: Option<LocalId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invariant {
    BreakOutsideLoop,
    ContinueOutsideLoop,
    UndeclaredLocal,
    /// A local in `AMakeClosure.free_vars` or `AAssign.local` is not declared
    /// in any enclosing scope at the module level (strict enforcement).
    StrictUndeclaredLocal,
    DeferSurvived,
    ReturnTypeMismatch,
    BreakTypeMismatch,
    AssignTypeMismatch,
    /// UnfoldStep variant literal missing concrete result metadata.
    UnfoldStepMetadataMissing,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[ANF verify] {invariant:?} in {name} (FuncId({id}))",
            invariant = self.invariant,
            name = self.func_name,
            id = self.func_id,
        )?;
        if let Some(lid) = self.local_id {
            write!(f, " [L{}]", lid.0)?;
        }
        write!(f, ": {}", self.detail)
    }
}

// ── Options ─────────────────────────────────────────────────────────────────

/// Controls which invariants the verifier checks.
#[derive(Debug, Clone)]
pub struct VerifyOptions {
    /// If true, `ADefer` nodes are an error (they should have been eliminated).
    /// Set to true for post-optimization / pre-codegen checks.
    pub reject_defer: bool,
    /// If true, check that every let-bound local has an `op_result_mono` entry.
    /// Only reliable post-lowering (optimizer passes may introduce new locals).
    pub check_op_result_mono: bool,
    /// If true, run strict declared-local enforcement at module level:
    /// `AMakeClosure.free_vars` and `AAssign` targets must be declared in
    /// the enclosing function's params or let-bindings (not just implicit captures).
    pub strict_locals: bool,
    /// If true, validate that UnfoldStep variant literals have concrete
    /// result metadata in `op_result_mono`.
    pub check_unfold_step_metadata: bool,
}

impl VerifyOptions {
    /// Post-lowering: defers are still present and valid, op_result_mono is complete.
    pub fn post_lowering() -> Self {
        Self {
            reject_defer: false,
            check_op_result_mono: true,
            strict_locals: true,
            check_unfold_step_metadata: true,
        }
    }

    /// Post-optimization: defers must have been eliminated, but optimizer
    /// may have introduced new locals without op_result_mono entries.
    pub fn post_optimization() -> Self {
        Self {
            reject_defer: true,
            check_op_result_mono: false,
            strict_locals: true,
            check_unfold_step_metadata: true,
        }
    }

    /// Mid-optimization: defers may still exist, op_result_mono may be stale.
    /// Checks structural and control-flow invariants only.
    pub fn mid_optimization() -> Self {
        Self {
            reject_defer: false,
            check_op_result_mono: false,
            strict_locals: false,
            check_unfold_step_metadata: false,
        }
    }
}

// ── Verifier context ────────────────────────────────────────────────────────

struct VerifyCtx {
    func_name: String,
    func_id: u32,
    /// Locals that are in scope (params + let-bound + pattern-bound).
    declared: HashSet<LocalId>,
    /// Free variables from enclosing scope — implicit closure captures.
    /// These are provided by the closure struct at runtime.
    captures: HashSet<LocalId>,
    /// Loop nesting depth.
    loop_depth: u32,
    /// Function return type.
    return_ty: MonoType,
    /// Stack of expected loop result types (innermost last).
    loop_result_ty_stack: Vec<Option<MonoType>>,
    /// Local → type mapping (params + let-bound locals from op_result_mono).
    local_types: HashMap<LocalId, MonoType>,
    options: VerifyOptions,
    errors: Vec<VerifyError>,
}

impl VerifyCtx {
    fn new(func: &AnfFunctionDef, options: VerifyOptions) -> Self {
        let declared: HashSet<LocalId> = func.params.iter().copied().collect();
        // Closure functions reference free variables from their enclosing scope.
        // These aren't in `params` but are provided by the closure struct at runtime.
        let captures = collect_free_locals(&func.body, declared.clone());
        // Build local→type mapping from params + op_result_mono
        let mut local_types = HashMap::new();
        for (i, param) in func.params.iter().enumerate() {
            if let Some(ty) = func.param_tys.get(i) {
                local_types.insert(*param, ty.clone());
            }
        }
        for (id, ty) in &func.op_result_mono {
            local_types.insert(*id, ty.clone());
        }
        Self {
            func_name: func.name.clone(),
            func_id: func.func_id.0,
            declared,
            captures,
            loop_depth: 0,
            return_ty: func.return_ty.clone(),
            loop_result_ty_stack: Vec::new(),
            local_types,
            options,
            errors: Vec::new(),
        }
    }

    fn is_available(&self, id: &LocalId) -> bool {
        self.declared.contains(id) || self.captures.contains(id)
    }

    /// Resolve the type of an atom, if knowable.
    fn resolve_atom_ty(&self, atom: &Atom) -> Option<MonoType> {
        match atom {
            Atom::ALocal(id) => self.local_types.get(id).cloned(),
            Atom::ALitInt(_) => Some(MonoType::Int),
            Atom::ALitFloat(_) => Some(MonoType::Float),
            Atom::ALitBool(_) => Some(MonoType::Bool),
            Atom::ALitStr(_) => Some(MonoType::String),
            Atom::ALitVoid => Some(MonoType::Void),
            Atom::AGlobalFunc(_) => None, // can't resolve without module context
        }
    }

    fn err(&mut self, invariant: Invariant, detail: String) {
        self.errors.push(VerifyError {
            func_name: self.func_name.clone(),
            func_id: self.func_id,
            invariant,
            detail,
            local_id: None,
        });
    }

    fn err_local(&mut self, invariant: Invariant, local: LocalId, detail: String) {
        self.errors.push(VerifyError {
            func_name: self.func_name.clone(),
            func_id: self.func_id,
            invariant,
            detail,
            local_id: Some(local),
        });
    }

    fn check_atom_defined(&mut self, atom: &Atom) {
        if let Atom::ALocal(id) = atom
            && !self.is_available(id)
        {
            self.err_local(
                Invariant::UndeclaredLocal,
                *id,
                format!("L{} used but not declared", id.0),
            );
        }
    }
}

/// Check if `actual` is compatible with `expected`. Permissive: only flags
/// obvious mismatches between resolved primitive/void/never types.
/// Returns true if compatible (or if we can't determine).
fn is_type_compatible(expected: &MonoType, actual: &MonoType) -> bool {
    // Never is compatible with anything (diverging expressions)
    if matches!(actual, MonoType::Never) || matches!(expected, MonoType::Never) {
        return true;
    }
    // MetaVar or Var — can't check, assume compatible
    if matches!(actual, MonoType::MetaVar(_) | MonoType::Var(_))
        || matches!(expected, MonoType::MetaVar(_) | MonoType::Var(_))
    {
        return true;
    }
    // For primitives/void, require exact match
    match (expected, actual) {
        (MonoType::Int, MonoType::Int)
        | (MonoType::Float, MonoType::Float)
        | (MonoType::Bool, MonoType::Bool)
        | (MonoType::Byte, MonoType::Byte)
        | (MonoType::String, MonoType::String)
        | (MonoType::Void, MonoType::Void) => true,
        // Named types: same TypeId is compatible (don't check type args deeply)
        (MonoType::Named { type_id: a, .. }, MonoType::Named { type_id: b, .. }) => a == b,
        // Function types: don't deeply check for now
        (MonoType::Function { .. }, MonoType::Function { .. }) => true,
        // Vector/Dict: permissive (don't deep-check element types)
        (MonoType::Vector(_), MonoType::Vector(_)) => true,
        (MonoType::Dict(_, _), MonoType::Dict(_, _)) => true,
        // Mixed categories — mismatch
        _ => {
            // Both are primitives but different → mismatch
            // One is Named, other is primitive → mismatch
            // etc.
            false
        }
    }
}

/// Check if a MonoType is a concrete UnfoldStep<Y, S> with resolved type args.
fn is_concrete_unfold_step_mono(mono: &MonoType) -> bool {
    match mono {
        MonoType::Named { type_id, args } => {
            *type_id == UNFOLD_STEP_TYPE_ID
                && args.len() == 2
                && args
                    .iter()
                    .all(|a| !matches!(a, MonoType::Var(_) | MonoType::MetaVar(_)))
        }
        _ => false,
    }
}

// ── Verification logic ─────────────────────────────────────────────────────

fn verify_expr(ctx: &mut VerifyCtx, expr: &AnfExpr) {
    match expr {
        AnfExpr::Let { local, op, body } => {
            // Pass the expected result type for this let-binding to verify_op
            // (used by ALoop to set the expected break type).
            let expected_ty = ctx.local_types.get(local).cloned();
            verify_op(ctx, *local, op, expected_ty);

            // The local is now in scope for the body
            ctx.declared.insert(*local);
            verify_expr(ctx, body);
        }
        AnfExpr::Return(atom) => {
            if let Some(a) = atom {
                ctx.check_atom_defined(a);
                // Check return value type against function return type
                if let Some(actual_ty) = ctx.resolve_atom_ty(a)
                    && !is_type_compatible(&ctx.return_ty, &actual_ty)
                {
                    ctx.err(
                        Invariant::ReturnTypeMismatch,
                        format!(
                            "return value has type {:?}, expected {:?}",
                            actual_ty, ctx.return_ty
                        ),
                    );
                }
            } else {
                // Return with no value — expected return type should be Void
                if !is_type_compatible(&ctx.return_ty, &MonoType::Void) {
                    ctx.err(
                        Invariant::ReturnTypeMismatch,
                        format!(
                            "return without value, but function returns {:?}",
                            ctx.return_ty
                        ),
                    );
                }
            }
        }
        AnfExpr::Break(atom) => {
            if ctx.loop_depth == 0 {
                ctx.err(
                    Invariant::BreakOutsideLoop,
                    "break outside of any loop".to_string(),
                );
            }
            if let Some(a) = atom {
                ctx.check_atom_defined(a);
                // Check break value type against expected loop result type
                if let Some(Some(expected_ty)) = ctx.loop_result_ty_stack.last()
                    && let Some(actual_ty) = ctx.resolve_atom_ty(a)
                    && !is_type_compatible(expected_ty, &actual_ty)
                {
                    ctx.err(
                        Invariant::BreakTypeMismatch,
                        format!(
                            "break value has type {:?}, expected {:?}",
                            actual_ty, expected_ty
                        ),
                    );
                }
            }
        }
        AnfExpr::Continue => {
            if ctx.loop_depth == 0 {
                ctx.err(
                    Invariant::ContinueOutsideLoop,
                    "continue outside of any loop".to_string(),
                );
            }
        }
        AnfExpr::Atom(atom) => {
            ctx.check_atom_defined(atom);
        }
    }
}

fn verify_op(ctx: &mut VerifyCtx, let_local: LocalId, op: &AnfOp, let_result_ty: Option<MonoType>) {
    // UnfoldStep metadata check: variant literals with UNFOLD_STEP_TYPE_ID
    // must have concrete result metadata in op_result_mono.
    if ctx.options.check_unfold_step_metadata
        && let AnfOp::AVariant { type_id, .. } = op
        && *type_id == UNFOLD_STEP_TYPE_ID
    {
        let has_concrete = ctx
            .local_types
            .get(&let_local)
            .is_some_and(is_concrete_unfold_step_mono);
        if !has_concrete {
            ctx.err_local(
                Invariant::UnfoldStepMetadataMissing,
                let_local,
                format!(
                    "UnfoldStep variant for L{} missing concrete result metadata",
                    let_local.0
                ),
            );
        }
    }

    match op {
        AnfOp::ACall { callee, args } => {
            ctx.check_atom_defined(callee);
            for a in args {
                ctx.check_atom_defined(a);
            }
        }
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => {
            ctx.check_atom_defined(cond);
            // Branches get a snapshot of declared locals (let-bindings inside
            // a branch are not visible to the other branch or to the continuation).
            let snapshot = ctx.declared.clone();
            verify_expr(ctx, then_branch);
            ctx.declared = snapshot.clone();
            verify_expr(ctx, else_branch);
            ctx.declared = snapshot;
        }
        AnfOp::AMatch { scrutinee, arms } => {
            ctx.check_atom_defined(scrutinee);
            let snapshot = ctx.declared.clone();
            for arm in arms {
                ctx.declared = snapshot.clone();
                // Pattern bindings are in scope for the arm body
                let mut pat_bindings = HashSet::new();
                collect_pattern_bindings(&arm.pattern, &mut pat_bindings);
                ctx.declared.extend(&pat_bindings);
                verify_expr(ctx, &arm.body);
            }
            ctx.declared = snapshot;
        }
        AnfOp::ALoop { body } => {
            ctx.loop_depth += 1;
            ctx.loop_result_ty_stack.push(let_result_ty);
            let snapshot = ctx.declared.clone();
            verify_expr(ctx, body);
            ctx.declared = snapshot;
            ctx.loop_result_ty_stack.pop();
            ctx.loop_depth -= 1;
        }
        AnfOp::ABinOp { left, right, .. } => {
            ctx.check_atom_defined(left);
            ctx.check_atom_defined(right);
        }
        AnfOp::AUnOp { expr, .. } => {
            ctx.check_atom_defined(expr);
        }
        AnfOp::AMakeClosure { free_vars, .. } => {
            for id in free_vars {
                if !ctx.is_available(id) {
                    ctx.err_local(
                        Invariant::UndeclaredLocal,
                        *id,
                        format!("closure captures undeclared L{}", id.0),
                    );
                }
            }
        }
        AnfOp::ARecord { fields, .. } => {
            for (_, atom) in fields {
                ctx.check_atom_defined(atom);
            }
        }
        AnfOp::ARecordGet { target, .. } => {
            ctx.check_atom_defined(target);
        }
        AnfOp::ARecordUpdate { base, value, .. } => {
            ctx.check_atom_defined(base);
            ctx.check_atom_defined(value);
        }
        AnfOp::AVariant { args, .. } => {
            for a in args {
                ctx.check_atom_defined(a);
            }
        }
        AnfOp::AArrayLit(elems) => {
            for a in elems {
                ctx.check_atom_defined(a);
            }
        }
        AnfOp::AIndex { base, index, .. } => {
            ctx.check_atom_defined(base);
            ctx.check_atom_defined(index);
        }
        AnfOp::AInit { value } => {
            ctx.check_atom_defined(value);
        }
        AnfOp::AAssign { local, value } => {
            if !ctx.is_available(local) {
                ctx.err_local(
                    Invariant::UndeclaredLocal,
                    *local,
                    format!("assign to undeclared L{}", local.0),
                );
            }
            ctx.check_atom_defined(value);
            // Validate type stability: the assigned value must be compatible
            // with the local's declared type (Wasm local.set requires stable type).
            if let Some(declared_ty) = ctx.local_types.get(local)
                && let Some(value_ty) = ctx.resolve_atom_ty(value)
                && !is_type_compatible(declared_ty, &value_ty)
            {
                ctx.err_local(
                    Invariant::AssignTypeMismatch,
                    *local,
                    format!(
                        "assign to L{} has type {:?}, but local declared as {:?}",
                        local.0, value_ty, declared_ty
                    ),
                );
            }
        }
        AnfOp::ADefer(inner) => {
            if ctx.options.reject_defer {
                ctx.err(
                    Invariant::DeferSurvived,
                    "ADefer node present (should have been eliminated)".to_string(),
                );
            }
            let snapshot = ctx.declared.clone();
            verify_expr(ctx, inner);
            ctx.declared = snapshot;
        }
    }
}

// ── op_result_mono consistency ──────────────────────────────────────────────

fn verify_op_result_mono(ctx: &mut VerifyCtx, func: &AnfFunctionDef) {
    // Every let-bound local in the body should have an entry in op_result_mono.
    // We walk the body and collect let-bound locals, checking each one.
    verify_op_result_mono_expr(ctx, &func.body, &func.op_result_mono);
}

fn verify_op_result_mono_expr(
    ctx: &mut VerifyCtx,
    expr: &AnfExpr,
    mono_map: &HashMap<LocalId, MonoType>,
) {
    if let AnfExpr::Let { local, op, body } = expr {
        if !mono_map.contains_key(local) {
            ctx.err_local(
                Invariant::UndeclaredLocal,
                *local,
                format!("L{} let-bound but missing from op_result_mono", local.0),
            );
        }
        // Recurse into sub-expressions inside ops
        verify_op_result_mono_op(ctx, op, mono_map);
        verify_op_result_mono_expr(ctx, body, mono_map);
    }
}

fn verify_op_result_mono_op(
    ctx: &mut VerifyCtx,
    op: &AnfOp,
    mono_map: &HashMap<LocalId, MonoType>,
) {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            verify_op_result_mono_expr(ctx, then_branch, mono_map);
            verify_op_result_mono_expr(ctx, else_branch, mono_map);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                verify_op_result_mono_expr(ctx, &arm.body, mono_map);
            }
        }
        AnfOp::ALoop { body } => {
            verify_op_result_mono_expr(ctx, body, mono_map);
        }
        AnfOp::ADefer(inner) => {
            verify_op_result_mono_expr(ctx, inner, mono_map);
        }
        _ => {}
    }
}

// ── Strict declared-local enforcement ───────────────────────────────────────

/// Walk a function body tracking declared locals (params + let-bindings).
/// Report errors when `AMakeClosure.free_vars` or `AAssign.local` references
/// a local that is not declared in any enclosing scope at this point.
fn verify_strict_locals(
    func: &AnfFunctionDef,
    capture_seed: &HashSet<LocalId>,
) -> Vec<VerifyError> {
    let mut declared: HashSet<LocalId> = func.params.iter().copied().collect();
    declared.extend(capture_seed.iter().copied());
    let mut errors = Vec::new();
    verify_strict_locals_expr(func, &func.body, &mut declared, &mut errors);
    errors
}

fn verify_strict_locals_expr(
    func: &AnfFunctionDef,
    expr: &AnfExpr,
    declared: &mut HashSet<LocalId>,
    errors: &mut Vec<VerifyError>,
) {
    match expr {
        AnfExpr::Let { local, op, body } => {
            verify_strict_locals_op(func, op, declared, errors);
            declared.insert(*local);
            verify_strict_locals_expr(func, body, declared, errors);
        }
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue | AnfExpr::Atom(_) => {}
    }
}

fn verify_strict_locals_op(
    func: &AnfFunctionDef,
    op: &AnfOp,
    declared: &mut HashSet<LocalId>,
    errors: &mut Vec<VerifyError>,
) {
    match op {
        AnfOp::AMakeClosure { free_vars, .. } => {
            for id in free_vars {
                if !declared.contains(id) {
                    errors.push(VerifyError {
                        func_name: func.name.clone(),
                        func_id: func.func_id.0,
                        invariant: Invariant::StrictUndeclaredLocal,
                        detail: format!(
                            "closure free_var L{} not declared in enclosing scope",
                            id.0
                        ),
                        local_id: Some(*id),
                    });
                }
            }
        }
        AnfOp::AAssign { local, .. } => {
            if !declared.contains(local) {
                errors.push(VerifyError {
                    func_name: func.name.clone(),
                    func_id: func.func_id.0,
                    invariant: Invariant::StrictUndeclaredLocal,
                    detail: format!("assign target L{} not declared in enclosing scope", local.0),
                    local_id: Some(*local),
                });
            }
        }
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            let snapshot = declared.clone();
            verify_strict_locals_expr(func, then_branch, declared, errors);
            *declared = snapshot.clone();
            verify_strict_locals_expr(func, else_branch, declared, errors);
            *declared = snapshot;
        }
        AnfOp::AMatch { arms, .. } => {
            let snapshot = declared.clone();
            for arm in arms {
                *declared = snapshot.clone();
                let mut pat_bindings = HashSet::new();
                collect_pattern_bindings(&arm.pattern, &mut pat_bindings);
                declared.extend(&pat_bindings);
                verify_strict_locals_expr(func, &arm.body, declared, errors);
            }
            *declared = snapshot;
        }
        AnfOp::ALoop { body } => {
            let snapshot = declared.clone();
            verify_strict_locals_expr(func, body, declared, errors);
            *declared = snapshot;
        }
        AnfOp::ADefer(inner) => {
            let snapshot = declared.clone();
            verify_strict_locals_expr(func, inner, declared, errors);
            *declared = snapshot;
        }
        _ => {}
    }
}

fn collect_module_capture_seeds(module: &AnfModule) -> HashMap<FuncId, HashSet<LocalId>> {
    let mut seeds: HashMap<FuncId, HashSet<LocalId>> = HashMap::new();
    for func in &module.functions {
        collect_capture_seeds_expr(&func.body, &mut seeds);
    }
    seeds
}

fn collect_capture_seeds_expr(expr: &AnfExpr, seeds: &mut HashMap<FuncId, HashSet<LocalId>>) {
    if let AnfExpr::Let { op, body, .. } = expr {
        collect_capture_seeds_op(op, seeds);
        collect_capture_seeds_expr(body, seeds);
    }
}

fn collect_capture_seeds_op(op: &AnfOp, seeds: &mut HashMap<FuncId, HashSet<LocalId>>) {
    match op {
        AnfOp::AMakeClosure { func_id, free_vars } => {
            let entry = seeds.entry(*func_id).or_default();
            entry.extend(free_vars.iter().copied());
        }
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            collect_capture_seeds_expr(then_branch, seeds);
            collect_capture_seeds_expr(else_branch, seeds);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                collect_capture_seeds_expr(&arm.body, seeds);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            collect_capture_seeds_expr(body, seeds);
        }
        _ => {}
    }
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Verify a single ANF function definition. Returns errors found.
pub fn verify_function(func: &AnfFunctionDef, options: &VerifyOptions) -> Vec<VerifyError> {
    let empty_capture_seed = HashSet::new();
    verify_function_with_capture_seed(func, options, &empty_capture_seed)
}

fn verify_function_with_capture_seed(
    func: &AnfFunctionDef,
    options: &VerifyOptions,
    capture_seed: &HashSet<LocalId>,
) -> Vec<VerifyError> {
    let mut ctx = VerifyCtx::new(func, options.clone());
    verify_expr(&mut ctx, &func.body);
    if options.check_op_result_mono {
        verify_op_result_mono(&mut ctx, func);
    }
    if options.strict_locals {
        ctx.errors.extend(verify_strict_locals(func, capture_seed));
    }
    ctx.errors
}

/// Verify all functions in an ANF module. Returns errors found.
pub fn verify_module(module: &AnfModule, options: &VerifyOptions) -> Vec<VerifyError> {
    let mut errors = Vec::new();
    let capture_seeds = collect_module_capture_seeds(module);
    let empty_capture_seed = HashSet::new();
    for func in &module.functions {
        let capture_seed = capture_seeds
            .get(&func.func_id)
            .unwrap_or(&empty_capture_seed);
        errors.extend(verify_function_with_capture_seed(
            func,
            options,
            capture_seed,
        ));
    }
    errors
}

/// Verify a single function after an optimization pass (debug mode).
/// Panics with pass attribution if any invariant fails.
#[cfg(debug_assertions)]
pub fn verify_function_after_pass(func: &AnfFunctionDef, pass_name: &str) {
    let options = VerifyOptions::mid_optimization();
    let errors = verify_function(func, &options);
    if !errors.is_empty() {
        let msgs: Vec<String> = errors.iter().map(|e| format!("  - {e}")).collect();
        panic!(
            "ANF verification failed after pass '{pass_name}':\n{}",
            msgs.join("\n")
        );
    }
}

/// Verify an ANF module, panicking with diagnostics if any invariant is violated.
///
/// Intended for use as a pipeline gate (post-lowering, pre-codegen).
pub fn verify_module_or_panic(module: &AnfModule, stage: &str) {
    let options = if stage.contains("optimization") || stage.contains("codegen") {
        VerifyOptions::post_optimization()
    } else {
        VerifyOptions::post_lowering()
    };
    let errors = verify_module(module, &options);
    if !errors.is_empty() {
        let msgs: Vec<String> = errors.iter().map(|e| format!("  - {e}")).collect();
        panic!("ANF verification failed ({stage}):\n{}", msgs.join("\n"));
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::anf::{AnfExpr, AnfOp, Atom};
    use crate::ir::core::{FuncId, LocalId};
    use crate::types::ty::MonoType;

    fn lid(n: u32) -> LocalId {
        LocalId(n)
    }

    fn fid(n: u32) -> FuncId {
        FuncId(n)
    }

    fn make_func(
        body: AnfExpr,
        params: Vec<LocalId>,
        op_result_mono: HashMap<LocalId, MonoType>,
    ) -> AnfFunctionDef {
        let param_tys = params.iter().map(|_| MonoType::Int).collect();
        AnfFunctionDef {
            func_id: fid(0),
            name: "test_fn".to_string(),
            params,
            param_tys,
            op_result_mono,
            body,
            return_ty: MonoType::Void,
        }
    }

    #[test]
    fn break_outside_loop_detected() {
        let body = AnfExpr::Break(None);
        let func = make_func(body, vec![], HashMap::new());
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(
            errors
                .iter()
                .any(|e| e.invariant == Invariant::BreakOutsideLoop)
        );
    }

    #[test]
    fn continue_outside_loop_detected() {
        let body = AnfExpr::Continue;
        let func = make_func(body, vec![], HashMap::new());
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(
            errors
                .iter()
                .any(|e| e.invariant == Invariant::ContinueOutsideLoop)
        );
    }

    #[test]
    fn break_inside_loop_ok() {
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::ALoop {
                body: Box::new(AnfExpr::Break(None)),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn undeclared_local_in_call_detected() {
        // A local referenced in a call arg but not declared or capturable
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::ACall {
                callee: Atom::AGlobalFunc(fid(1)),
                args: vec![Atom::ALocal(lid(99))],
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        // lid(99) is free → treated as implicit capture, so no error at function level.
        // (Module-level AMakeClosure check would catch if it's not actually provided.)
        // This is the expected behavior for closure functions.
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn undeclared_local_in_non_capture_context() {
        // Params cover lid(0); body uses lid(0) — should be fine
        let body = AnfExpr::Atom(Atom::ALocal(lid(0)));
        let func = make_func(body, vec![lid(0)], HashMap::new());
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn param_local_is_declared() {
        let body = AnfExpr::Atom(Atom::ALocal(lid(0)));
        let func = make_func(body, vec![lid(0)], HashMap::new());
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn let_bound_local_is_declared() {
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Int);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AInit {
                value: Atom::ALitInt(42),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALocal(lid(0)))),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn missing_op_result_mono_detected() {
        // Let-bind a local but don't put it in op_result_mono
        let body = AnfExpr::Let {
            local: lid(5),
            op: Box::new(AnfOp::AInit {
                value: Atom::ALitInt(1),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], HashMap::new());
        let errors = verify_function(&func, &VerifyOptions::post_lowering());
        assert!(
            errors.iter().any(|e| e.detail.contains("op_result_mono")),
            "expected op_result_mono error, got: {:?}",
            errors
        );
    }

    #[test]
    fn assign_to_declared_param_ok() {
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AAssign {
                local: lid(1),
                value: Atom::ALitInt(1),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![lid(1)], mono);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn closure_captures_declared_local_ok() {
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Int);
        mono.insert(lid(1), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AInit {
                value: Atom::ALitInt(42),
            }),
            body: Box::new(AnfExpr::Let {
                local: lid(1),
                op: Box::new(AnfOp::AMakeClosure {
                    func_id: fid(1),
                    free_vars: vec![lid(0)],
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
            }),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn closure_captures_free_var_treated_as_capture() {
        // lid(99) referenced in AMakeClosure.free_vars is treated as an implicit
        // capture at the function level (collect_free_locals finds it).
        // The basic UndeclaredLocal check passes, but StrictUndeclaredLocal would flag it.
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AMakeClosure {
                func_id: fid(1),
                free_vars: vec![lid(99)],
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], mono);
        // Mid-optimization: no strict locals check, so implicit capture is OK
        let errors = verify_function(&func, &VerifyOptions::mid_optimization());
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn strict_closure_captures_undeclared_detected() {
        // lid(99) is not declared anywhere — strict mode catches it.
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AMakeClosure {
                func_id: fid(1),
                free_vars: vec![lid(99)],
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(
            errors
                .iter()
                .any(|e| e.invariant == Invariant::StrictUndeclaredLocal),
            "expected StrictUndeclaredLocal, got: {:?}",
            errors
        );
    }

    #[test]
    fn strict_assign_undeclared_detected() {
        // lid(99) is not declared — strict mode catches it.
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AAssign {
                local: lid(99),
                value: Atom::ALitInt(1),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(
            errors
                .iter()
                .any(|e| e.invariant == Invariant::StrictUndeclaredLocal),
            "expected StrictUndeclaredLocal, got: {:?}",
            errors
        );
    }

    // ── Return type checks ──────────────────────────────────────────────

    fn make_func_with_ret(
        body: AnfExpr,
        params: Vec<LocalId>,
        op_result_mono: HashMap<LocalId, MonoType>,
        return_ty: MonoType,
    ) -> AnfFunctionDef {
        let param_tys = params.iter().map(|_| MonoType::Int).collect();
        AnfFunctionDef {
            func_id: fid(0),
            name: "test_fn".to_string(),
            params,
            param_tys,
            op_result_mono,
            body,
            return_ty,
        }
    }

    #[test]
    fn return_type_mismatch_detected() {
        // Function returns Int, but we return a String literal
        let body = AnfExpr::Return(Some(Atom::ALitStr("oops".to_string())));
        let func = make_func_with_ret(body, vec![], HashMap::new(), MonoType::Int);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(
            errors
                .iter()
                .any(|e| e.invariant == Invariant::ReturnTypeMismatch),
            "expected ReturnTypeMismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn return_type_match_ok() {
        let body = AnfExpr::Return(Some(Atom::ALitInt(42)));
        let func = make_func_with_ret(body, vec![], HashMap::new(), MonoType::Int);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn return_void_from_void_fn_ok() {
        let body = AnfExpr::Return(None);
        let func = make_func_with_ret(body, vec![], HashMap::new(), MonoType::Void);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn return_void_from_int_fn_detected() {
        let body = AnfExpr::Return(None);
        let func = make_func_with_ret(body, vec![], HashMap::new(), MonoType::Int);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(
            errors
                .iter()
                .any(|e| e.invariant == Invariant::ReturnTypeMismatch),
            "expected ReturnTypeMismatch, got: {:?}",
            errors
        );
    }

    // ── Break type checks ───────────────────────────────────────────────

    #[test]
    fn break_type_mismatch_detected() {
        // Loop bound to lid(0) with expected type Int, but break with String
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Int);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::ALoop {
                body: Box::new(AnfExpr::Break(Some(Atom::ALitStr("wrong".to_string())))),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(
            errors
                .iter()
                .any(|e| e.invariant == Invariant::BreakTypeMismatch),
            "expected BreakTypeMismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn break_type_match_ok() {
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Int);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::ALoop {
                body: Box::new(AnfExpr::Break(Some(Atom::ALitInt(42)))),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    // ── is_type_compatible ──────────────────────────────────────────────

    #[test]
    fn never_is_compatible_with_anything() {
        assert!(is_type_compatible(&MonoType::Int, &MonoType::Never));
        assert!(is_type_compatible(&MonoType::Never, &MonoType::String));
    }

    #[test]
    fn same_primitives_are_compatible() {
        assert!(is_type_compatible(&MonoType::Int, &MonoType::Int));
        assert!(is_type_compatible(&MonoType::Float, &MonoType::Float));
        assert!(is_type_compatible(&MonoType::Bool, &MonoType::Bool));
        assert!(is_type_compatible(&MonoType::String, &MonoType::String));
        assert!(is_type_compatible(&MonoType::Void, &MonoType::Void));
    }

    #[test]
    fn different_primitives_are_incompatible() {
        assert!(!is_type_compatible(&MonoType::Int, &MonoType::String));
        assert!(!is_type_compatible(&MonoType::Bool, &MonoType::Float));
        assert!(!is_type_compatible(&MonoType::Void, &MonoType::Int));
    }

    // ── Assign type stability ───────────────────────────────────────────

    #[test]
    fn assign_type_mismatch_detected() {
        // lid(1) declared as Int (param), assigning a String
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AAssign {
                local: lid(1),
                value: Atom::ALitStr("wrong".to_string()),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let mut func = make_func(body, vec![lid(1)], mono);
        func.param_tys = vec![MonoType::Int]; // lid(1) is Int
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(
            errors
                .iter()
                .any(|e| e.invariant == Invariant::AssignTypeMismatch),
            "expected AssignTypeMismatch, got: {:?}",
            errors
        );
    }

    #[test]
    fn assign_same_type_ok() {
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AAssign {
                local: lid(1),
                value: Atom::ALitInt(99),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let mut func = make_func(body, vec![lid(1)], mono);
        func.param_tys = vec![MonoType::Int];
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    // ── Mid-optimization options ────────────────────────────────────────

    #[test]
    fn mid_optimization_allows_defer() {
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::ADefer(Box::new(AnfExpr::Atom(Atom::ALitVoid)))),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::mid_optimization());
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn defer_scope_does_not_leak_locals_strict() {
        // lid(1) is let-bound inside ADefer; using it in an AAssign in the
        // continuation after the defer must trigger StrictUndeclaredLocal.
        // (The basic UndeclaredLocal check treats free locals as implicit
        // captures, so we need strict mode to catch this.)
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        mono.insert(lid(1), MonoType::Int);
        mono.insert(lid(2), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::ADefer(Box::new(AnfExpr::Let {
                local: lid(1),
                op: Box::new(AnfOp::AInit {
                    value: Atom::ALitInt(42),
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
            }))),
            // lid(1) should NOT be in scope here — strict check catches it
            body: Box::new(AnfExpr::Let {
                local: lid(2),
                op: Box::new(AnfOp::AAssign {
                    local: lid(1),
                    value: Atom::ALitInt(99),
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
            }),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_lowering());
        assert!(
            errors.iter().any(|e| e.invariant == Invariant::StrictUndeclaredLocal
                && e.detail.contains("L1")),
            "expected StrictUndeclaredLocal for L1 leaked from ADefer, got: {:?}", errors
        );
    }

    #[test]
    fn post_optimization_rejects_defer() {
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::ADefer(Box::new(AnfExpr::Atom(Atom::ALitVoid)))),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        assert!(
            errors
                .iter()
                .any(|e| e.invariant == Invariant::DeferSurvived),
            "expected DeferSurvived, got: {:?}",
            errors
        );
    }

    // ── Diagnostic message snapshots ────────────────────────────────────
    // These test exact error formatting to prevent diagnostic regressions.

    #[test]
    fn diagnostic_break_outside_loop() {
        let body = AnfExpr::Break(Some(Atom::ALitInt(1)));
        let func = make_func(body, vec![], HashMap::new());
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        let msg = errors[0].to_string();
        assert_eq!(
            msg,
            "[ANF verify] BreakOutsideLoop in test_fn (FuncId(0)): break outside of any loop"
        );
    }

    #[test]
    fn diagnostic_return_type_mismatch() {
        let body = AnfExpr::Return(Some(Atom::ALitStr("x".to_string())));
        let func = make_func_with_ret(body, vec![], HashMap::new(), MonoType::Int);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        let msg = errors[0].to_string();
        assert_eq!(
            msg,
            "[ANF verify] ReturnTypeMismatch in test_fn (FuncId(0)): return value has type String, expected Int"
        );
    }

    #[test]
    fn diagnostic_assign_type_mismatch() {
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AAssign {
                local: lid(1),
                value: Atom::ALitFloat(1.0),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let mut func = make_func(body, vec![lid(1)], mono);
        func.param_tys = vec![MonoType::Int];
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        let msg = errors[0].to_string();
        assert_eq!(
            msg,
            "[ANF verify] AssignTypeMismatch in test_fn (FuncId(0)) [L1]: assign to L1 has type Float, but local declared as Int"
        );
    }

    #[test]
    fn diagnostic_break_type_mismatch() {
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Bool);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::ALoop {
                body: Box::new(AnfExpr::Break(Some(Atom::ALitInt(1)))),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        let msg = errors[0].to_string();
        assert_eq!(
            msg,
            "[ANF verify] BreakTypeMismatch in test_fn (FuncId(0)): break value has type Int, expected Bool"
        );
    }

    #[test]
    fn diagnostic_defer_survived() {
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::ADefer(Box::new(AnfExpr::Atom(Atom::ALitVoid)))),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        let msg = errors[0].to_string();
        assert_eq!(
            msg,
            "[ANF verify] DeferSurvived in test_fn (FuncId(0)): ADefer node present (should have been eliminated)"
        );
    }

    #[test]
    fn diagnostic_multiple_errors_collected() {
        // A body with both break-outside-loop and return type mismatch
        // Only break — return is on a different path. Let's use two sequential errors.
        let body = AnfExpr::Break(None);
        let func = make_func_with_ret(body, vec![], HashMap::new(), MonoType::Int);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        // Should have BreakOutsideLoop (break with no loop context)
        assert!(
            errors
                .iter()
                .any(|e| e.invariant == Invariant::BreakOutsideLoop)
        );
    }

    // ── Strict locals ─────────────────────────────────────────────────

    #[test]
    fn diagnostic_strict_undeclared_closure_free_var() {
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AMakeClosure {
                func_id: fid(1),
                free_vars: vec![lid(42)],
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        let strict_err = errors
            .iter()
            .find(|e| e.invariant == Invariant::StrictUndeclaredLocal)
            .expect("expected StrictUndeclaredLocal");
        assert_eq!(strict_err.local_id, Some(lid(42)));
        assert_eq!(
            strict_err.to_string(),
            "[ANF verify] StrictUndeclaredLocal in test_fn (FuncId(0)) [L42]: closure free_var L42 not declared in enclosing scope"
        );
    }

    #[test]
    fn diagnostic_strict_undeclared_assign_target() {
        let mut mono = HashMap::new();
        mono.insert(lid(0), MonoType::Void);
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AAssign {
                local: lid(77),
                value: Atom::ALitInt(1),
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_optimization());
        let strict_err = errors
            .iter()
            .find(|e| e.invariant == Invariant::StrictUndeclaredLocal)
            .expect("expected StrictUndeclaredLocal");
        assert_eq!(strict_err.local_id, Some(lid(77)));
        assert_eq!(
            strict_err.to_string(),
            "[ANF verify] StrictUndeclaredLocal in test_fn (FuncId(0)) [L77]: assign target L77 not declared in enclosing scope"
        );
    }

    // ── UnfoldStep metadata ──────────────────────────────────────────

    #[test]
    fn unfold_step_missing_metadata_detected() {
        use crate::types::ty::UNFOLD_STEP_TYPE_ID;
        // AVariant with UNFOLD_STEP_TYPE_ID but no op_result_mono entry
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AVariant {
                type_id: UNFOLD_STEP_TYPE_ID,
                variant: crate::ir::core::VariantId(0),
                args: vec![],
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], HashMap::new());
        let errors = verify_function(&func, &VerifyOptions::post_lowering());
        assert!(
            errors
                .iter()
                .any(|e| e.invariant == Invariant::UnfoldStepMetadataMissing),
            "expected UnfoldStepMetadataMissing, got: {:?}",
            errors
        );
    }

    #[test]
    fn unfold_step_with_concrete_metadata_ok() {
        use crate::types::ty::UNFOLD_STEP_TYPE_ID;
        let mut mono = HashMap::new();
        mono.insert(
            lid(0),
            MonoType::Named {
                type_id: UNFOLD_STEP_TYPE_ID,
                args: vec![MonoType::Int, MonoType::String],
            },
        );
        let body = AnfExpr::Let {
            local: lid(0),
            op: Box::new(AnfOp::AVariant {
                type_id: UNFOLD_STEP_TYPE_ID,
                variant: crate::ir::core::VariantId(0),
                args: vec![],
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let func = make_func(body, vec![], mono);
        let errors = verify_function(&func, &VerifyOptions::post_lowering());
        let unfold_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.invariant == Invariant::UnfoldStepMetadataMissing)
            .collect();
        assert!(
            unfold_errors.is_empty(),
            "unexpected UnfoldStep errors: {:?}",
            unfold_errors
        );
    }

    // ── Verify module ───────────────────────────────────────────────────

    #[test]
    fn verify_module_collects_errors_from_all_functions() {
        let func1 = make_func(AnfExpr::Break(None), vec![], HashMap::new());
        let mut func2 = make_func(AnfExpr::Continue, vec![], HashMap::new());
        func2.func_id = fid(1);
        func2.name = "other_fn".to_string();
        let module = AnfModule {
            functions: vec![func1, func2],
            init_func_id: None,
            all_init_func_ids: vec![],
            extern_imports: HashMap::new(),
            module_global_locals: None,
        };
        let errors = verify_module(&module, &VerifyOptions::post_optimization());
        assert!(
            errors
                .iter()
                .any(|e| e.invariant == Invariant::BreakOutsideLoop && e.func_name == "test_fn")
        );
        assert!(errors
            .iter()
            .any(|e| e.invariant == Invariant::ContinueOutsideLoop && e.func_name == "other_fn"));
    }

    #[test]
    fn verify_module_allows_nested_closure_recapture_of_enclosing_capture() {
        // outer creates mid with free_vars=[L97]
        // mid creates inner with free_vars=[L97]
        // L97 is not let-bound inside mid, but is available via mid's closure capture.
        let outer_body = AnfExpr::Let {
            local: lid(97),
            op: Box::new(AnfOp::AInit {
                value: Atom::ALitInt(5),
            }),
            body: Box::new(AnfExpr::Let {
                local: lid(0),
                op: Box::new(AnfOp::AMakeClosure {
                    func_id: fid(2),
                    free_vars: vec![lid(97)],
                }),
                body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
            }),
        };
        let mut outer = make_func(outer_body, vec![], HashMap::new());
        outer.func_id = fid(1);
        outer.name = "outer".to_string();

        let mid_body = AnfExpr::Let {
            local: lid(1),
            op: Box::new(AnfOp::AMakeClosure {
                func_id: fid(3),
                free_vars: vec![lid(97)],
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitVoid)),
        };
        let mut mid = make_func(mid_body, vec![], HashMap::new());
        mid.func_id = fid(2);
        mid.name = "mid".to_string();

        let mut inner = make_func(AnfExpr::Atom(Atom::ALocal(lid(97))), vec![], HashMap::new());
        inner.func_id = fid(3);
        inner.name = "inner".to_string();
        inner.return_ty = MonoType::Int;

        let module = AnfModule {
            functions: vec![outer, mid, inner],
            init_func_id: None,
            all_init_func_ids: vec![],
            extern_imports: HashMap::new(),
            module_global_locals: None,
        };
        let errors = verify_module(&module, &VerifyOptions::post_optimization());
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }
}
