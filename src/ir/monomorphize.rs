//! Stage 9.5 — Monomorphization
//!
//! Specializes every generic user function per unique concrete instantiation,
//! rewrites all call sites to use the specialized version, and drops the
//! original generic definitions.
//!
//! Pipeline position:
//!   parse → resolve → typecheck → lower (Core IR) → **monomorphize** → lower (ANF) → …
//!
//! A function is "generic" if any of its `param_tys` or `return_ty` contains a
//! `MonoType::Var`.  After this pass no `Var` survives in any `FunctionDef`.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::ir::core::{CoreExpr, CoreExprKind, CoreModule, FuncId, FunctionDef, MatchArm};
use crate::types::ty::MonoType;

// ─── Public helpers (also used by tests) ─────────────────────────────────────

/// Returns `true` if `ty` contains any `MonoType::Var`.
pub fn contains_var(ty: &MonoType) -> bool {
    match ty {
        MonoType::Var(_) => true,
        MonoType::Vector(e) => contains_var(e),
        MonoType::Dict(k, v) => contains_var(k) || contains_var(v),
        MonoType::Function { params, ret } => params.iter().any(contains_var) || contains_var(ret),
        MonoType::Named { args, .. } => args.iter().any(contains_var),
        _ => false,
    }
}

/// Match a generic type pattern against a concrete type, extending `out` with
/// solved `Var` → concrete type bindings.
pub fn match_type_against(
    pattern: &MonoType,
    concrete: &MonoType,
    out: &mut HashMap<String, MonoType>,
) {
    match pattern {
        MonoType::Var(name) => {
            // First match wins; the type checker guarantees consistency.
            out.entry(name.clone()).or_insert_with(|| concrete.clone());
        }
        MonoType::Vector(elem_p) => {
            if let MonoType::Vector(elem_c) = concrete {
                match_type_against(elem_p, elem_c, out);
            }
        }
        MonoType::Dict(kp, vp) => {
            if let MonoType::Dict(kc, vc) = concrete {
                match_type_against(kp, kc, out);
                match_type_against(vp, vc, out);
            }
        }
        MonoType::Function {
            params: pp,
            ret: rp,
        } => {
            if let MonoType::Function {
                params: pc,
                ret: rc,
            } = concrete
            {
                for (pp_ty, pc_ty) in pp.iter().zip(pc.iter()) {
                    match_type_against(pp_ty, pc_ty, out);
                }
                match_type_against(rp, rc, out);
            }
        }
        MonoType::Named { args: ap, .. } => {
            if let MonoType::Named { args: ac, .. } = concrete {
                for (ap_ty, ac_ty) in ap.iter().zip(ac.iter()) {
                    match_type_against(ap_ty, ac_ty, out);
                }
            }
        }
        _ => {}
    }
}

/// Apply a `Var`-name → `MonoType` substitution throughout a `MonoType`.
pub fn apply_mono_subst(ty: &MonoType, subst: &HashMap<String, MonoType>) -> MonoType {
    match ty {
        MonoType::Var(name) => subst.get(name).cloned().unwrap_or_else(|| ty.clone()),
        MonoType::Vector(elem) => MonoType::Vector(Box::new(apply_mono_subst(elem, subst))),
        MonoType::Dict(k, v) => MonoType::Dict(
            Box::new(apply_mono_subst(k, subst)),
            Box::new(apply_mono_subst(v, subst)),
        ),
        MonoType::Function { params, ret } => MonoType::Function {
            params: params.iter().map(|p| apply_mono_subst(p, subst)).collect(),
            ret: Box::new(apply_mono_subst(ret, subst)),
        },
        MonoType::Named { type_id, args } => MonoType::Named {
            type_id: *type_id,
            args: args.iter().map(|a| apply_mono_subst(a, subst)).collect(),
        },
        other => other.clone(),
    }
}

// ─── Private helpers ──────────────────────────────────────────────────────────

fn is_generic(func: &FunctionDef) -> bool {
    func.param_tys.iter().any(contains_var) || contains_var(&func.return_ty)
}

/// Collect type-variable names in left-to-right, first-appearance order across
/// `param_tys` then `return_ty`.  Gives a deterministic ordering for the
/// canonical type-args vector used as the specialisation map key.
fn collect_type_params(param_tys: &[MonoType], return_ty: &MonoType) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut params = Vec::new();
    for ty in param_tys.iter().chain(std::iter::once(return_ty)) {
        collect_vars_in_order(ty, &mut seen, &mut params);
    }
    params
}

fn collect_vars_in_order(ty: &MonoType, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    match ty {
        MonoType::Var(name) => {
            if seen.insert(name.clone()) {
                out.push(name.clone());
            }
        }
        MonoType::Vector(e) => collect_vars_in_order(e, seen, out),
        MonoType::Dict(k, v) => {
            collect_vars_in_order(k, seen, out);
            collect_vars_in_order(v, seen, out);
        }
        MonoType::Function { params, ret } => {
            for p in params {
                collect_vars_in_order(p, seen, out);
            }
            collect_vars_in_order(ret, seen, out);
        }
        MonoType::Named { args, .. } => {
            for a in args {
                collect_vars_in_order(a, seen, out);
            }
        }
        _ => {}
    }
}

/// Canonical short string for a type, used when naming specialisations.
fn type_key(ty: &MonoType) -> String {
    match ty {
        MonoType::Int => "Int".to_string(),
        MonoType::Float => "Float".to_string(),
        MonoType::Bool => "Bool".to_string(),
        MonoType::String => "String".to_string(),
        MonoType::Void => "Void".to_string(),
        MonoType::Never => "Never".to_string(),
        MonoType::Var(name) => name.clone(),
        MonoType::MetaVar(id) => format!("M{}", id),
        MonoType::Vector(elem) => format!("Vec_{}", type_key(elem)),
        MonoType::Dict(k, v) => format!("Dict_{}_{}", type_key(k), type_key(v)),
        MonoType::Named { type_id, args } => {
            if args.is_empty() {
                format!("T{}", type_id.0)
            } else {
                let args_str = args.iter().map(type_key).collect::<Vec<_>>().join("_");
                format!("T{}_{}", type_id.0, args_str)
            }
        }
        MonoType::Function { params, ret } => {
            let params_str = params.iter().map(type_key).collect::<Vec<_>>().join("_");
            format!("Fn_{}_{}", params_str, type_key(ret))
        }
    }
}

// ─── Tree traversals ──────────────────────────────────────────────────────────

/// Walk `expr`, pushing `(orig_fid, subst)` onto `queue` for every direct or
/// first-class use of a generic function.
fn collect_instantiations<'a>(
    expr: &CoreExpr,
    generic_funcs: &HashMap<FuncId, &'a FunctionDef>,
    queue: &mut VecDeque<(FuncId, HashMap<String, MonoType>)>,
) {
    match &expr.kind {
        CoreExprKind::Call { callee, args } => {
            if let CoreExprKind::GlobalFunc(fid) = &callee.kind {
                if let Some(gf) = generic_funcs.get(fid) {
                    let mut subst = HashMap::new();
                    for (param_ty, arg) in gf.param_tys.iter().zip(args.iter()) {
                        match_type_against(param_ty, &arg.ty, &mut subst);
                    }
                    if !subst.is_empty() {
                        queue.push_back((*fid, subst));
                    }
                }
            }
            collect_instantiations(callee, generic_funcs, queue);
            for arg in args {
                collect_instantiations(arg, generic_funcs, queue);
            }
        }

        CoreExprKind::GlobalFunc(fid) => {
            // Non-call-position reference: concrete type is stored in expr.ty.
            if let Some(gf) = generic_funcs.get(fid) {
                let generic_fn_ty = MonoType::Function {
                    params: gf.param_tys.clone(),
                    ret: Box::new(gf.return_ty.clone()),
                };
                let mut subst = HashMap::new();
                match_type_against(&generic_fn_ty, &expr.ty, &mut subst);
                if !subst.is_empty() {
                    queue.push_back((*fid, subst));
                }
            }
        }

        // Recurse into sub-expressions.
        CoreExprKind::Let { value, body, .. } => {
            collect_instantiations(value, generic_funcs, queue);
            collect_instantiations(body, generic_funcs, queue);
        }
        CoreExprKind::Assign { value, .. } => {
            collect_instantiations(value, generic_funcs, queue);
        }
        CoreExprKind::BinOp { left, right, .. } => {
            collect_instantiations(left, generic_funcs, queue);
            collect_instantiations(right, generic_funcs, queue);
        }
        CoreExprKind::UnOp { expr, .. } => {
            collect_instantiations(expr, generic_funcs, queue);
        }
        CoreExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_instantiations(cond, generic_funcs, queue);
            collect_instantiations(then_branch, generic_funcs, queue);
            collect_instantiations(else_branch, generic_funcs, queue);
        }
        CoreExprKind::Match { scrutinee, arms } => {
            collect_instantiations(scrutinee, generic_funcs, queue);
            for arm in arms {
                collect_instantiations(&arm.body, generic_funcs, queue);
            }
        }
        CoreExprKind::Loop { body } => {
            collect_instantiations(body, generic_funcs, queue);
        }
        CoreExprKind::Break { value } => {
            if let Some(v) = value {
                collect_instantiations(v, generic_funcs, queue);
            }
        }
        CoreExprKind::Return { value } => {
            if let Some(v) = value {
                collect_instantiations(v, generic_funcs, queue);
            }
        }
        CoreExprKind::Record { fields, .. } => {
            for (_, val) in fields {
                collect_instantiations(val, generic_funcs, queue);
            }
        }
        CoreExprKind::RecordGet { target, .. } => {
            collect_instantiations(target, generic_funcs, queue);
        }
        CoreExprKind::RecordUpdate { base, value, .. } => {
            collect_instantiations(base, generic_funcs, queue);
            collect_instantiations(value, generic_funcs, queue);
        }
        CoreExprKind::Variant { args, .. } => {
            for arg in args {
                collect_instantiations(arg, generic_funcs, queue);
            }
        }
        CoreExprKind::ArrayLit { elements } => {
            for elem in elements {
                collect_instantiations(elem, generic_funcs, queue);
            }
        }
        CoreExprKind::Index { base, index } => {
            collect_instantiations(base, generic_funcs, queue);
            collect_instantiations(index, generic_funcs, queue);
        }
        CoreExprKind::Defer(inner) => {
            collect_instantiations(inner, generic_funcs, queue);
        }
        // Hoisted lambda bodies are top-level FunctionDefs in module.functions
        // and are scanned by the seed loop, so no traversal needed here.
        CoreExprKind::MakeClosure { .. }
        | CoreExprKind::LitInt(_)
        | CoreExprKind::LitFloat(_)
        | CoreExprKind::LitBool(_)
        | CoreExprKind::LitStr(_)
        | CoreExprKind::LitVoid
        | CoreExprKind::Local(_)
        | CoreExprKind::GlobalLocal(_)
        | CoreExprKind::Continue => {}
    }
}

/// Apply a type substitution to every `ty` annotation inside `expr`.
/// Does NOT rewrite `GlobalFunc` ids — that happens in the rewrite pass.
fn apply_subst_to_expr(expr: &CoreExpr, subst: &HashMap<String, MonoType>) -> CoreExpr {
    CoreExpr {
        ty: apply_mono_subst(&expr.ty, subst),
        kind: apply_subst_to_kind(&expr.kind, subst),
        span: expr.span,
    }
}

fn apply_subst_to_kind(kind: &CoreExprKind, subst: &HashMap<String, MonoType>) -> CoreExprKind {
    match kind {
        CoreExprKind::LitInt(_)
        | CoreExprKind::LitFloat(_)
        | CoreExprKind::LitBool(_)
        | CoreExprKind::LitStr(_)
        | CoreExprKind::LitVoid
        | CoreExprKind::Local(_)
        | CoreExprKind::GlobalLocal(_)
        | CoreExprKind::GlobalFunc(_) // FuncId rewriting is separate
        | CoreExprKind::Continue => kind.clone(),

        CoreExprKind::Let { local, value, body } => CoreExprKind::Let {
            local: *local,
            value: Box::new(apply_subst_to_expr(value, subst)),
            body: Box::new(apply_subst_to_expr(body, subst)),
        },
        CoreExprKind::Assign { local, value } => CoreExprKind::Assign {
            local: *local,
            value: Box::new(apply_subst_to_expr(value, subst)),
        },
        CoreExprKind::BinOp { op, left, right } => CoreExprKind::BinOp {
            op: *op,
            left: Box::new(apply_subst_to_expr(left, subst)),
            right: Box::new(apply_subst_to_expr(right, subst)),
        },
        CoreExprKind::UnOp { op, expr } => CoreExprKind::UnOp {
            op: *op,
            expr: Box::new(apply_subst_to_expr(expr, subst)),
        },
        CoreExprKind::Call { callee, args } => CoreExprKind::Call {
            callee: Box::new(apply_subst_to_expr(callee, subst)),
            args: args.iter().map(|a| apply_subst_to_expr(a, subst)).collect(),
        },
        CoreExprKind::MakeClosure { func_id, free_vars } => CoreExprKind::MakeClosure {
            func_id: *func_id,
            free_vars: free_vars.clone(),
        },
        CoreExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => CoreExprKind::If {
            cond: Box::new(apply_subst_to_expr(cond, subst)),
            then_branch: Box::new(apply_subst_to_expr(then_branch, subst)),
            else_branch: Box::new(apply_subst_to_expr(else_branch, subst)),
        },
        CoreExprKind::Match { scrutinee, arms } => CoreExprKind::Match {
            scrutinee: Box::new(apply_subst_to_expr(scrutinee, subst)),
            arms: arms
                .iter()
                .map(|arm| MatchArm {
                    pattern: arm.pattern.clone(),
                    body: apply_subst_to_expr(&arm.body, subst),
                })
                .collect(),
        },
        CoreExprKind::Loop { body } => CoreExprKind::Loop {
            body: Box::new(apply_subst_to_expr(body, subst)),
        },
        CoreExprKind::Break { value } => CoreExprKind::Break {
            value: value
                .as_ref()
                .map(|v| Box::new(apply_subst_to_expr(v, subst))),
        },
        CoreExprKind::Return { value } => CoreExprKind::Return {
            value: value
                .as_ref()
                .map(|v| Box::new(apply_subst_to_expr(v, subst))),
        },
        CoreExprKind::Defer(inner) => {
            CoreExprKind::Defer(Box::new(apply_subst_to_expr(inner, subst)))
        }
        CoreExprKind::Record { type_id, fields } => CoreExprKind::Record {
            type_id: *type_id,
            fields: fields
                .iter()
                .map(|(fid, val)| (*fid, apply_subst_to_expr(val, subst)))
                .collect(),
        },
        CoreExprKind::RecordGet { target, field } => CoreExprKind::RecordGet {
            target: Box::new(apply_subst_to_expr(target, subst)),
            field: *field,
        },
        CoreExprKind::RecordUpdate { base, field, value } => CoreExprKind::RecordUpdate {
            base: Box::new(apply_subst_to_expr(base, subst)),
            field: *field,
            value: Box::new(apply_subst_to_expr(value, subst)),
        },
        CoreExprKind::Variant {
            type_id,
            variant,
            args,
        } => CoreExprKind::Variant {
            type_id: *type_id,
            variant: *variant,
            args: args.iter().map(|a| apply_subst_to_expr(a, subst)).collect(),
        },
        CoreExprKind::ArrayLit { elements } => CoreExprKind::ArrayLit {
            elements: elements
                .iter()
                .map(|e| apply_subst_to_expr(e, subst))
                .collect(),
        },
        CoreExprKind::Index { base, index } => CoreExprKind::Index {
            base: Box::new(apply_subst_to_expr(base, subst)),
            index: Box::new(apply_subst_to_expr(index, subst)),
        },
    }
}

// Mapping: (orig_func_id, canonical type_args) → specialized FuncId
type SpecMap = HashMap<(FuncId, Vec<MonoType>), FuncId>;

fn rewrite_calls_in_func(
    mut func: FunctionDef,
    spec_map: &SpecMap,
    generic_funcs: &HashMap<FuncId, &FunctionDef>,
) -> FunctionDef {
    func.body = rewrite_calls_in_expr(&func.body, spec_map, generic_funcs);
    func
}

fn rewrite_calls_in_expr(
    expr: &CoreExpr,
    spec_map: &SpecMap,
    generic_funcs: &HashMap<FuncId, &FunctionDef>,
) -> CoreExpr {
    CoreExpr {
        ty: expr.ty.clone(),
        kind: rewrite_calls_in_kind(expr, spec_map, generic_funcs),
        span: expr.span,
    }
}

/// Rewrite `GlobalFunc(orig_fid)` to `GlobalFunc(spec_fid)` wherever it
/// refers to a generic function that has been specialised.  The `parent`
/// argument carries the containing `CoreExpr` so we can read its `.ty` for
/// the non-call-position case.
fn rewrite_calls_in_kind(
    parent: &CoreExpr,
    spec_map: &SpecMap,
    generic_funcs: &HashMap<FuncId, &FunctionDef>,
) -> CoreExprKind {
    match &parent.kind {
        CoreExprKind::Call { callee, args } => {
            let new_args: Vec<CoreExpr> = args
                .iter()
                .map(|a| rewrite_calls_in_expr(a, spec_map, generic_funcs))
                .collect();

            if let CoreExprKind::GlobalFunc(fid) = &callee.kind {
                if let Some(gf) = generic_funcs.get(fid) {
                    let type_params = collect_type_params(&gf.param_tys, &gf.return_ty);
                    let mut subst = HashMap::new();
                    for (param_ty, arg) in gf.param_tys.iter().zip(new_args.iter()) {
                        match_type_against(param_ty, &arg.ty, &mut subst);
                    }
                    let type_args: Vec<MonoType> = type_params
                        .iter()
                        .map(|p| subst.get(p).cloned().unwrap_or(MonoType::Void))
                        .collect();
                    debug_assert!(
                        type_params.iter().all(|p| subst.contains_key(p)),
                        "unsolved type params {:?} at call site for {:?}",
                        type_params
                            .iter()
                            .filter(|p| !subst.contains_key(*p))
                            .collect::<Vec<_>>(),
                        fid,
                    );
                    debug_assert!(
                        spec_map.contains_key(&(*fid, type_args.clone())),
                        "no specialization found for {:?} with type_args={:?}; call site will be left unpatched",
                        fid,
                        type_args,
                    );
                    if let Some(&new_fid) = spec_map.get(&(*fid, type_args)) {
                        return CoreExprKind::Call {
                            callee: Box::new(CoreExpr {
                                kind: CoreExprKind::GlobalFunc(new_fid),
                                ty: callee.ty.clone(),
                                span: callee.span,
                            }),
                            args: new_args,
                        };
                    }
                }
            }

            CoreExprKind::Call {
                callee: Box::new(rewrite_calls_in_expr(callee, spec_map, generic_funcs)),
                args: new_args,
            }
        }

        CoreExprKind::GlobalFunc(fid) => {
            // Non-call position: derive instantiation from parent.ty
            if let Some(gf) = generic_funcs.get(fid) {
                let generic_fn_ty = MonoType::Function {
                    params: gf.param_tys.clone(),
                    ret: Box::new(gf.return_ty.clone()),
                };
                let type_params = collect_type_params(&gf.param_tys, &gf.return_ty);
                let mut subst = HashMap::new();
                match_type_against(&generic_fn_ty, &parent.ty, &mut subst);
                let type_args: Vec<MonoType> = type_params
                    .iter()
                    .map(|p| subst.get(p).cloned().unwrap_or(MonoType::Void))
                    .collect();
                debug_assert!(
                    type_params.iter().all(|p| subst.contains_key(p)),
                    "unsolved type params {:?} for non-call-position ref to {:?}",
                    type_params
                        .iter()
                        .filter(|p| !subst.contains_key(*p))
                        .collect::<Vec<_>>(),
                    fid,
                );
                if let Some(&new_fid) = spec_map.get(&(*fid, type_args)) {
                    return CoreExprKind::GlobalFunc(new_fid);
                }
            }
            parent.kind.clone()
        }

        // Leaf / structural nodes — recurse then reconstruct.
        CoreExprKind::Let { local, value, body } => CoreExprKind::Let {
            local: *local,
            value: Box::new(rewrite_calls_in_expr(value, spec_map, generic_funcs)),
            body: Box::new(rewrite_calls_in_expr(body, spec_map, generic_funcs)),
        },
        CoreExprKind::Assign { local, value } => CoreExprKind::Assign {
            local: *local,
            value: Box::new(rewrite_calls_in_expr(value, spec_map, generic_funcs)),
        },
        CoreExprKind::BinOp { op, left, right } => CoreExprKind::BinOp {
            op: *op,
            left: Box::new(rewrite_calls_in_expr(left, spec_map, generic_funcs)),
            right: Box::new(rewrite_calls_in_expr(right, spec_map, generic_funcs)),
        },
        CoreExprKind::UnOp { op, expr } => CoreExprKind::UnOp {
            op: *op,
            expr: Box::new(rewrite_calls_in_expr(expr, spec_map, generic_funcs)),
        },
        CoreExprKind::MakeClosure { func_id, free_vars } => CoreExprKind::MakeClosure {
            func_id: *func_id,
            free_vars: free_vars.clone(),
        },
        CoreExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => CoreExprKind::If {
            cond: Box::new(rewrite_calls_in_expr(cond, spec_map, generic_funcs)),
            then_branch: Box::new(rewrite_calls_in_expr(then_branch, spec_map, generic_funcs)),
            else_branch: Box::new(rewrite_calls_in_expr(else_branch, spec_map, generic_funcs)),
        },
        CoreExprKind::Match { scrutinee, arms } => CoreExprKind::Match {
            scrutinee: Box::new(rewrite_calls_in_expr(scrutinee, spec_map, generic_funcs)),
            arms: arms
                .iter()
                .map(|arm| MatchArm {
                    pattern: arm.pattern.clone(),
                    body: rewrite_calls_in_expr(&arm.body, spec_map, generic_funcs),
                })
                .collect(),
        },
        CoreExprKind::Loop { body } => CoreExprKind::Loop {
            body: Box::new(rewrite_calls_in_expr(body, spec_map, generic_funcs)),
        },
        CoreExprKind::Break { value } => CoreExprKind::Break {
            value: value
                .as_ref()
                .map(|v| Box::new(rewrite_calls_in_expr(v, spec_map, generic_funcs))),
        },
        CoreExprKind::Return { value } => CoreExprKind::Return {
            value: value
                .as_ref()
                .map(|v| Box::new(rewrite_calls_in_expr(v, spec_map, generic_funcs))),
        },
        CoreExprKind::Defer(inner) => CoreExprKind::Defer(Box::new(rewrite_calls_in_expr(
            inner,
            spec_map,
            generic_funcs,
        ))),
        CoreExprKind::Record { type_id, fields } => CoreExprKind::Record {
            type_id: *type_id,
            fields: fields
                .iter()
                .map(|(fid, val)| (*fid, rewrite_calls_in_expr(val, spec_map, generic_funcs)))
                .collect(),
        },
        CoreExprKind::RecordGet { target, field } => CoreExprKind::RecordGet {
            target: Box::new(rewrite_calls_in_expr(target, spec_map, generic_funcs)),
            field: *field,
        },
        CoreExprKind::RecordUpdate { base, field, value } => CoreExprKind::RecordUpdate {
            base: Box::new(rewrite_calls_in_expr(base, spec_map, generic_funcs)),
            field: *field,
            value: Box::new(rewrite_calls_in_expr(value, spec_map, generic_funcs)),
        },
        CoreExprKind::Variant {
            type_id,
            variant,
            args,
        } => CoreExprKind::Variant {
            type_id: *type_id,
            variant: *variant,
            args: args
                .iter()
                .map(|a| rewrite_calls_in_expr(a, spec_map, generic_funcs))
                .collect(),
        },
        CoreExprKind::ArrayLit { elements } => CoreExprKind::ArrayLit {
            elements: elements
                .iter()
                .map(|e| rewrite_calls_in_expr(e, spec_map, generic_funcs))
                .collect(),
        },
        CoreExprKind::Index { base, index } => CoreExprKind::Index {
            base: Box::new(rewrite_calls_in_expr(base, spec_map, generic_funcs)),
            index: Box::new(rewrite_calls_in_expr(index, spec_map, generic_funcs)),
        },
        CoreExprKind::LitInt(_)
        | CoreExprKind::LitFloat(_)
        | CoreExprKind::LitBool(_)
        | CoreExprKind::LitStr(_)
        | CoreExprKind::LitVoid
        | CoreExprKind::Local(_)
        | CoreExprKind::GlobalLocal(_)
        | CoreExprKind::Continue => parent.kind.clone(),
    }
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Monomorphize the `CoreModule`.
///
/// Specialises every generic user function for each unique concrete
/// instantiation discovered by walking all call sites (transitively).
/// Rewrites all `GlobalFunc` references to use specialised FuncIds.
/// Drops original generic `FunctionDef`s — no `MonoType::Var` survives.
///
/// If the module contains no generic functions, returns it unchanged.
pub fn monomorphize(mut module: CoreModule) -> CoreModule {
    // Clone generic FunctionDefs so we can later consume module.functions freely.
    let generic_owned: Vec<FunctionDef> = module
        .functions
        .iter()
        .filter(|f| is_generic(f))
        .cloned()
        .collect();

    if generic_owned.is_empty() {
        return module;
    }

    // Build lookup map from cloned copies.
    let generic_funcs: HashMap<FuncId, &FunctionDef> =
        generic_owned.iter().map(|f| (f.func_id, f)).collect();

    // Next unused FuncId (above all existing ones).
    let mut next_func_id: u32 = module
        .functions
        .iter()
        .map(|f| f.func_id.0 + 1)
        .max()
        .unwrap_or(0);

    let mut spec_map: SpecMap = HashMap::new();
    let mut queue: VecDeque<(FuncId, HashMap<String, MonoType>)> = VecDeque::new();
    let mut processed: HashSet<(FuncId, Vec<MonoType>)> = HashSet::new();

    // Seed: collect instantiations from all non-generic functions.
    for func in &module.functions {
        if !is_generic(func) {
            collect_instantiations(&func.body, &generic_funcs, &mut queue);
        }
    }

    // Process queue; each specialisation may reveal transitive instantiations.
    let mut new_funcs: Vec<FunctionDef> = Vec::new();
    while let Some((orig_fid, subst)) = queue.pop_front() {
        let gf = match generic_funcs.get(&orig_fid) {
            Some(f) => *f,
            None => continue,
        };

        let type_params = collect_type_params(&gf.param_tys, &gf.return_ty);
        let type_args: Vec<MonoType> = type_params
            .iter()
            .map(|p| subst.get(p).cloned().unwrap_or(MonoType::Void))
            .collect();
        debug_assert!(
            type_params.iter().all(|p| subst.contains_key(p)),
            "unsolved type params {:?} for {:?}",
            type_params
                .iter()
                .filter(|p| !subst.contains_key(*p))
                .collect::<Vec<_>>(),
            orig_fid,
        );

        let key = (orig_fid, type_args.clone());
        if processed.contains(&key) {
            continue;
        }
        processed.insert(key.clone());

        // Assign fresh FuncId and record the mapping.
        let new_fid = FuncId(next_func_id);
        next_func_id += 1;
        spec_map.insert(key, new_fid);

        // Clone and substitute all type annotations.
        let suffix = type_args.iter().map(type_key).collect::<Vec<_>>().join("_");
        let specialised = FunctionDef {
            func_id: new_fid,
            name: format!("{}__{}", gf.name, suffix),
            params: gf.params.clone(),
            param_tys: gf
                .param_tys
                .iter()
                .map(|ty| apply_mono_subst(ty, &subst))
                .collect(),
            body: apply_subst_to_expr(&gf.body, &subst),
            return_ty: apply_mono_subst(&gf.return_ty, &subst),
        };

        // Collect transitive instantiations from the now-concrete body.
        collect_instantiations(&specialised.body, &generic_funcs, &mut queue);
        new_funcs.push(specialised);
    }

    // Rewrite all call sites; drop original generic defs.
    let rewritten: Vec<FunctionDef> = module
        .functions
        .into_iter()
        .filter(|f| !is_generic(f))
        .map(|f| rewrite_calls_in_func(f, &spec_map, &generic_funcs))
        .collect();

    let rewritten_new: Vec<FunctionDef> = new_funcs
        .into_iter()
        .map(|f| rewrite_calls_in_func(f, &spec_map, &generic_funcs))
        .collect();

    module.functions = rewritten;
    module.functions.extend(rewritten_new);
    module
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── contains_var ─────────────────────────────────────────────────────────

    #[test]
    fn contains_var_primitive() {
        assert!(!contains_var(&MonoType::Int));
        assert!(!contains_var(&MonoType::Bool));
        assert!(!contains_var(&MonoType::String));
        assert!(!contains_var(&MonoType::Void));
    }

    #[test]
    fn contains_var_direct() {
        assert!(contains_var(&MonoType::Var("T".into())));
    }

    #[test]
    fn contains_var_nested() {
        assert!(contains_var(&MonoType::Vector(Box::new(MonoType::Var(
            "T".into()
        )))));
        assert!(contains_var(&MonoType::Function {
            params: vec![MonoType::Var("A".into())],
            ret: Box::new(MonoType::Int),
        }));
    }

    #[test]
    fn contains_var_no_var_in_function() {
        assert!(!contains_var(&MonoType::Function {
            params: vec![MonoType::Int],
            ret: Box::new(MonoType::String),
        }));
    }

    // ── match_type_against ───────────────────────────────────────────────────

    #[test]
    fn match_var_against_int() {
        let mut out = HashMap::new();
        match_type_against(&MonoType::Var("T".into()), &MonoType::Int, &mut out);
        assert_eq!(out.get("T"), Some(&MonoType::Int));
    }

    #[test]
    fn match_vector_of_var_against_vector_of_int() {
        let mut out = HashMap::new();
        match_type_against(
            &MonoType::Vector(Box::new(MonoType::Var("T".into()))),
            &MonoType::Vector(Box::new(MonoType::Int)),
            &mut out,
        );
        assert_eq!(out.get("T"), Some(&MonoType::Int));
    }

    #[test]
    fn match_function_type_derives_both_type_params() {
        let mut out = HashMap::new();
        // fn(A) B matched against fn(Int) String
        match_type_against(
            &MonoType::Function {
                params: vec![MonoType::Var("A".into())],
                ret: Box::new(MonoType::Var("B".into())),
            },
            &MonoType::Function {
                params: vec![MonoType::Int],
                ret: Box::new(MonoType::String),
            },
            &mut out,
        );
        assert_eq!(out.get("A"), Some(&MonoType::Int));
        assert_eq!(out.get("B"), Some(&MonoType::String));
    }

    #[test]
    fn match_primitive_against_primitive_noop() {
        let mut out = HashMap::new();
        match_type_against(&MonoType::Int, &MonoType::Int, &mut out);
        assert!(out.is_empty());
    }

    // ── apply_mono_subst ─────────────────────────────────────────────────────

    #[test]
    fn apply_subst_replaces_var() {
        let subst: HashMap<String, MonoType> =
            [("T".to_string(), MonoType::Int)].into_iter().collect();
        assert_eq!(
            apply_mono_subst(&MonoType::Var("T".into()), &subst),
            MonoType::Int
        );
    }

    #[test]
    fn apply_subst_leaves_unknown_var() {
        let subst: HashMap<String, MonoType> = HashMap::new();
        assert_eq!(
            apply_mono_subst(&MonoType::Var("T".into()), &subst),
            MonoType::Var("T".into())
        );
    }

    #[test]
    fn apply_subst_nested_vector() {
        let subst: HashMap<String, MonoType> =
            [("T".to_string(), MonoType::Bool)].into_iter().collect();
        assert_eq!(
            apply_mono_subst(
                &MonoType::Vector(Box::new(MonoType::Var("T".into()))),
                &subst
            ),
            MonoType::Vector(Box::new(MonoType::Bool))
        );
    }

    #[test]
    fn apply_subst_function_type() {
        let subst: HashMap<String, MonoType> = [
            ("A".to_string(), MonoType::Int),
            ("B".to_string(), MonoType::String),
        ]
        .into_iter()
        .collect();
        let result = apply_mono_subst(
            &MonoType::Function {
                params: vec![MonoType::Var("A".into())],
                ret: Box::new(MonoType::Var("B".into())),
            },
            &subst,
        );
        assert_eq!(
            result,
            MonoType::Function {
                params: vec![MonoType::Int],
                ret: Box::new(MonoType::String),
            }
        );
    }

    // ── collect_type_params ──────────────────────────────────────────────────

    #[test]
    fn collect_type_params_preserves_order() {
        // fn(A, B) B  => ["A", "B"] (A appears first in params)
        let param_tys = vec![MonoType::Var("A".into()), MonoType::Var("B".into())];
        let return_ty = MonoType::Var("B".into());
        let params = collect_type_params(&param_tys, &return_ty);
        assert_eq!(params, vec!["A", "B"]);
    }

    #[test]
    fn collect_type_params_return_only_var() {
        // fn(Int) T  => ["T"]
        let param_tys = vec![MonoType::Int];
        let return_ty = MonoType::Var("T".into());
        let params = collect_type_params(&param_tys, &return_ty);
        assert_eq!(params, vec!["T"]);
    }

    // ── type_key ─────────────────────────────────────────────────────────────

    #[test]
    fn type_key_primitives() {
        assert_eq!(type_key(&MonoType::Int), "Int");
        assert_eq!(type_key(&MonoType::Bool), "Bool");
        assert_eq!(type_key(&MonoType::String), "String");
        assert_eq!(type_key(&MonoType::Void), "Void");
    }

    #[test]
    fn type_key_vector() {
        assert_eq!(
            type_key(&MonoType::Vector(Box::new(MonoType::Int))),
            "Vec_Int"
        );
    }

    // ── monomorphize() integration ────────────────────────────────────────────

    use crate::ir::core::{CoreModule, LocalId};
    use crate::syntax::span::{FileId, Span};
    use crate::types::env::TypeEnv;

    fn dummy_span() -> Span {
        Span {
            file_id: FileId(0),
            start: 0,
            end: 0,
        }
    }

    fn expr(kind: CoreExprKind, ty: MonoType) -> CoreExpr {
        CoreExpr {
            kind,
            ty,
            span: dummy_span(),
        }
    }

    fn make_func(
        id: u32,
        name: &str,
        params: Vec<u32>,
        param_tys: Vec<MonoType>,
        body: CoreExpr,
        return_ty: MonoType,
    ) -> FunctionDef {
        FunctionDef {
            func_id: FuncId(id),
            name: name.to_string(),
            params: params.into_iter().map(LocalId).collect(),
            param_tys,
            body,
            return_ty,
        }
    }

    fn empty_module(functions: Vec<FunctionDef>) -> CoreModule {
        CoreModule {
            functions,
            type_env: TypeEnv::default(),
            init_func_id: None,
            all_init_func_ids: vec![],
        }
    }

    /// Generic `id<T>(x: T) T { x }` called with `Int` from `__init__`.
    /// After mono: one specialization; no generic def remains.
    #[test]
    fn monomorphize_basic_call() {
        let id_fid = FuncId(41);

        // id(x: T) T { Local(0) }
        let id_func = make_func(
            41,
            "id",
            vec![0],
            vec![MonoType::Var("T".into())],
            expr(CoreExprKind::Local(LocalId(0)), MonoType::Var("T".into())),
            MonoType::Var("T".into()),
        );

        // __init__() Void { let _ = id(42) in Void }
        let call = expr(
            CoreExprKind::Call {
                callee: Box::new(expr(
                    CoreExprKind::GlobalFunc(id_fid),
                    MonoType::Function {
                        params: vec![MonoType::Int],
                        ret: Box::new(MonoType::Int),
                    },
                )),
                args: vec![expr(CoreExprKind::LitInt(42), MonoType::Int)],
            },
            MonoType::Int,
        );
        let init_func = make_func(
            42,
            "__init__",
            vec![],
            vec![],
            expr(
                CoreExprKind::Let {
                    local: LocalId(1),
                    value: Box::new(call),
                    body: Box::new(expr(CoreExprKind::LitVoid, MonoType::Void)),
                },
                MonoType::Void,
            ),
            MonoType::Void,
        );

        let module = empty_module(vec![id_func, init_func]);
        let result = monomorphize(module);

        // Original generic `id` dropped; one specialization added; __init__ kept.
        assert_eq!(result.functions.len(), 2);
        assert!(
            result.functions.iter().all(|f| !is_generic(f)),
            "all functions should be concrete after monomorphization"
        );
        // The specialization name encodes the type.
        assert!(
            result.functions.iter().any(|f| f.name.contains("id__")),
            "expected a specialization of id"
        );
    }

    /// `outer<T>` calls `inner<T>` — both should be specialized transitively.
    #[test]
    fn monomorphize_transitive_generic_calls() {
        let inner_fid = FuncId(41);
        let outer_fid = FuncId(42);

        // inner(x: T) T { Local(0) }
        let inner_func = make_func(
            41,
            "inner",
            vec![0],
            vec![MonoType::Var("T".into())],
            expr(CoreExprKind::Local(LocalId(0)), MonoType::Var("T".into())),
            MonoType::Var("T".into()),
        );

        // outer(x: T) T { inner(x) }
        let outer_body = expr(
            CoreExprKind::Call {
                callee: Box::new(expr(
                    CoreExprKind::GlobalFunc(inner_fid),
                    MonoType::Function {
                        params: vec![MonoType::Var("T".into())],
                        ret: Box::new(MonoType::Var("T".into())),
                    },
                )),
                args: vec![expr(
                    CoreExprKind::Local(LocalId(0)),
                    MonoType::Var("T".into()),
                )],
            },
            MonoType::Var("T".into()),
        );
        let outer_func = make_func(
            42,
            "outer",
            vec![0],
            vec![MonoType::Var("T".into())],
            outer_body,
            MonoType::Var("T".into()),
        );

        // __init__() Void { outer(42) }
        let init_func = make_func(
            43,
            "__init__",
            vec![],
            vec![],
            expr(
                CoreExprKind::Call {
                    callee: Box::new(expr(
                        CoreExprKind::GlobalFunc(outer_fid),
                        MonoType::Function {
                            params: vec![MonoType::Int],
                            ret: Box::new(MonoType::Int),
                        },
                    )),
                    args: vec![expr(CoreExprKind::LitInt(1), MonoType::Int)],
                },
                MonoType::Int,
            ),
            MonoType::Void,
        );

        let module = empty_module(vec![inner_func, outer_func, init_func]);
        let result = monomorphize(module);

        // __init__ + outer__Int + inner__Int
        assert_eq!(result.functions.len(), 3);
        assert!(result.functions.iter().all(|f| !is_generic(f)));
        assert!(result.functions.iter().any(|f| f.name.contains("outer__")));
        assert!(result.functions.iter().any(|f| f.name.contains("inner__")));
    }

    /// A generic function stored as a first-class value (non-call-position).
    /// `let f: fn(Int) Int = id` — the GlobalFunc node carries the concrete type.
    #[test]
    fn monomorphize_non_call_position_reference() {
        let id_fid = FuncId(41);

        let id_func = make_func(
            41,
            "id",
            vec![0],
            vec![MonoType::Var("T".into())],
            expr(CoreExprKind::Local(LocalId(0)), MonoType::Var("T".into())),
            MonoType::Var("T".into()),
        );

        // __init__: let f: fn(Int)Int = GlobalFunc(id) in Void
        let init_func = make_func(
            42,
            "__init__",
            vec![],
            vec![],
            expr(
                CoreExprKind::Let {
                    local: LocalId(0),
                    value: Box::new(expr(
                        CoreExprKind::GlobalFunc(id_fid),
                        MonoType::Function {
                            params: vec![MonoType::Int],
                            ret: Box::new(MonoType::Int),
                        },
                    )),
                    body: Box::new(expr(CoreExprKind::LitVoid, MonoType::Void)),
                },
                MonoType::Void,
            ),
            MonoType::Void,
        );

        let module = empty_module(vec![id_func, init_func]);
        let result = monomorphize(module);

        assert_eq!(result.functions.len(), 2);
        assert!(result.functions.iter().all(|f| !is_generic(f)));
        // The GlobalFunc in __init__ body should now point to the specialization.
        let init = result
            .functions
            .iter()
            .find(|f| f.name == "__init__")
            .unwrap();
        if let CoreExprKind::Let { value, .. } = &init.body.kind {
            assert!(
                matches!(value.kind, CoreExprKind::GlobalFunc(_)),
                "GlobalFunc should be rewritten"
            );
            // The rewritten fid must not be the original generic id_fid.
            if let CoreExprKind::GlobalFunc(rewritten_fid) = value.kind {
                assert_ne!(
                    rewritten_fid, id_fid,
                    "should point to specialization, not original"
                );
            }
        } else {
            panic!("expected Let in __init__ body");
        }
    }
}
