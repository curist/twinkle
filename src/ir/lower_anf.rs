/// Core IR → ANF IR lowering pass.
///
/// This pass transforms Core IR (block-structured, expression-oriented) into ANF IR
/// (flat let-chains where every intermediate computation is explicitly named).
///
/// Instead of CPS (which fights Rust's borrow checker when nesting closures),
/// we use an **explicit let-accumulator** style:
///   - A `LetAccum` (vec of `(LocalId, AnfOp)` pairs) collects bindings imperatively.
///   - `atomize(expr, accum)` lowers an expression to an `Atom`, accumulating any
///     intermediate let-bindings in `accum`.
///   - `build_lets(accum, tail_expr)` converts the accumulator into nested AnfExpr::Let.
///
/// This avoids the nested-closure borrow problem completely and is equivalent to CPS.
use crate::ir::anf::{
    AnfExpr, AnfFunctionDef, AnfMatchArm, AnfModule, AnfOp, Atom, IndexKind, OpKind,
};
use crate::ir::core::{
    CoreExpr, CoreExprKind, CoreModule, CorePattern, FunctionDef, LocalId, MatchArm,
};
use crate::types::ty::{MonoType, TypeId};

/// Derive `OpKind` from the operand's `MonoType`.
fn op_kind_from(ty: &MonoType) -> OpKind {
    match ty {
        MonoType::Int => OpKind::Int,
        // Byte arithmetic/comparison is lowered through integer operators.
        // Typechecking ensures arithmetic results are Int.
        MonoType::Byte => OpKind::Int,
        MonoType::Float => OpKind::Float,
        MonoType::Bool => OpKind::Bool,
        MonoType::String => OpKind::String,
        other => panic!(
            "op_kind_from: expected Int/Byte/Float/Bool/String, got {:?}",
            other
        ),
    }
}

/// Derive `IndexKind` from the base expression's `MonoType`.
fn index_kind_from(ty: &MonoType) -> IndexKind {
    match ty {
        MonoType::Vector(_) => IndexKind::Array,
        MonoType::Dict(_, _) => IndexKind::Dict,
        MonoType::String => IndexKind::String,
        other => panic!(
            "index_kind_from: expected Vector/Dict/String, got {:?}",
            other
        ),
    }
}

/// Extract `TypeId` from a `MonoType::Named` type.
fn type_id_from(ty: &MonoType) -> TypeId {
    match ty {
        MonoType::Named { type_id, .. } => *type_id,
        other => panic!("type_id_from: expected Named type, got {:?}", other),
    }
}

/// A let-accumulator: a sequence of (LocalId, AnfOp) pairs to be wrapped around
/// the "tail" expression.  Accumulated left-to-right → outermost let first.
type LetAccum = Vec<(LocalId, AnfOp)>;

/// Lower a `CoreModule` into an `AnfModule`.
pub fn lower_module(module: &CoreModule) -> AnfModule {
    let functions = module
        .functions
        .iter()
        .map(|f| {
            let mut next_temp = max_local_id_in_func(f) + 1;
            lower_func(f, &mut next_temp)
        })
        .collect();

    AnfModule {
        functions,
        init_func_id: module.init_func_id,
        all_init_func_ids: module.all_init_func_ids.clone(),
    }
}

/// Lower a single `FunctionDef` into an `AnfFunctionDef`.
pub fn lower_func(func: &FunctionDef, next_temp: &mut u32) -> AnfFunctionDef {
    let body = lower_expr_top(&func.body, next_temp);

    AnfFunctionDef {
        func_id: func.func_id,
        name: func.name.clone(),
        params: func.params.clone(),
        param_tys: func.param_tys.clone(),
        body,
        return_ty: func.return_ty.clone(),
    }
}

/// Allocate a fresh temporary `LocalId`.
#[inline]
fn fresh(next_temp: &mut u32) -> LocalId {
    let id = *next_temp;
    *next_temp += 1;
    LocalId(id)
}

/// Wrap accumulated let-bindings around a tail expression, outermost first.
fn build_lets(accum: LetAccum, tail: AnfExpr) -> AnfExpr {
    accum
        .into_iter()
        .rev()
        .fold(tail, |body, (local, op)| AnfExpr::Let {
            local,
            op: Box::new(op),
            body: Box::new(body),
        })
}

/// Lower a `CoreExpr` to a full `AnfExpr`.
///
/// This is the top-level entry: it processes the expression, collecting intermediate
/// let-bindings in an accumulator, then builds the final expression.
fn lower_expr_top(expr: &CoreExpr, next_temp: &mut u32) -> AnfExpr {
    let mut accum: LetAccum = Vec::new();
    let result = lower_expr(expr, next_temp, &mut accum);
    build_lets(accum, result)
}

/// Lower a `CoreExpr` to an `AnfExpr`, accumulating let-bindings in `accum`.
///
/// Returns:
/// - For atoms: `AnfExpr::Atom(atom)` (no bindings added to accum).
/// - For non-atomic ops: `AnfExpr::Atom(ALocal(tmp))` with `(tmp, op)` pushed to accum.
/// - For terminals (Break, Continue, Return): the terminal expression directly.
/// - For structural forms (If, Match, Loop): a Let binding the result to a fresh temp.
/// - For Let: processes value into accum at the original local, then processes body.
/// - For Assign: processes value, adds AAssign to accum, returns Atom(ALitVoid).
fn lower_expr(expr: &CoreExpr, next_temp: &mut u32, accum: &mut LetAccum) -> AnfExpr {
    match &expr.kind {
        // ── Atoms: return directly ─────────────────────────────────────────────
        CoreExprKind::LitInt(n) => AnfExpr::Atom(Atom::ALitInt(*n)),
        CoreExprKind::LitFloat(v) => AnfExpr::Atom(Atom::ALitFloat(*v)),
        CoreExprKind::LitBool(b) => AnfExpr::Atom(Atom::ALitBool(*b)),
        CoreExprKind::LitStr(s) => AnfExpr::Atom(Atom::ALitStr(s.clone())),
        CoreExprKind::LitVoid => AnfExpr::Atom(Atom::ALitVoid),
        CoreExprKind::Local(id) => AnfExpr::Atom(Atom::ALocal(*id)),
        CoreExprKind::GlobalLocal(id) => AnfExpr::Atom(Atom::ALocal(*id)),
        CoreExprKind::GlobalFunc(id) => AnfExpr::Atom(Atom::AGlobalFunc(*id)),

        // ── Non-atomic: push (tmp, op) to accum, return Atom(ALocal(tmp)) ──────
        CoreExprKind::MakeClosure { func_id, free_vars } => {
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::AMakeClosure {
                    func_id: *func_id,
                    free_vars: free_vars.clone(),
                },
            ));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::BinOp { op, left, right } => {
            let operand_ty = op_kind_from(&left.ty);
            let left_atom = atomize(left, next_temp, accum);
            let right_atom = atomize(right, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::ABinOp {
                    op: *op,
                    left: left_atom,
                    right: right_atom,
                    operand_ty,
                },
            ));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::UnOp { op, expr: inner } => {
            let operand_ty = op_kind_from(&inner.ty);
            let inner_atom = atomize(inner, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::AUnOp {
                    op: *op,
                    expr: inner_atom,
                    operand_ty,
                },
            ));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::Call { callee, args } => {
            let callee_atom = atomize(callee, next_temp, accum);
            let arg_atoms: Vec<Atom> = args.iter().map(|a| atomize(a, next_temp, accum)).collect();
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::ACall {
                    callee: callee_atom,
                    args: arg_atoms,
                },
            ));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::Record { type_id, fields } => {
            let type_id = *type_id;
            let anf_fields: Vec<_> = fields
                .iter()
                .map(|(fid, e)| (*fid, atomize(e, next_temp, accum)))
                .collect();
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::ARecord {
                    type_id,
                    fields: anf_fields,
                },
            ));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::RecordGet { target, field } => {
            let tid = type_id_from(&target.ty);
            let target_atom = atomize(target, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::ARecordGet {
                    target: target_atom,
                    field: *field,
                    type_id: tid,
                },
            ));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::RecordUpdate { base, field, value } => {
            let tid = type_id_from(&base.ty);
            let base_atom = atomize(base, next_temp, accum);
            let value_atom = atomize(value, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::ARecordUpdate {
                    base: base_atom,
                    field: *field,
                    value: value_atom,
                    can_reuse_in_place: false,
                    type_id: tid,
                },
            ));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::Variant {
            type_id,
            variant,
            args,
        } => {
            let type_id = *type_id;
            let variant = *variant;
            let arg_atoms: Vec<Atom> = args.iter().map(|a| atomize(a, next_temp, accum)).collect();
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::AVariant {
                    type_id,
                    variant,
                    args: arg_atoms,
                },
            ));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::ArrayLit { elements } => {
            let atoms: Vec<Atom> = elements
                .iter()
                .map(|e| atomize(e, next_temp, accum))
                .collect();
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::AArrayLit(atoms)));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::Index { base, index } => {
            let bty = index_kind_from(&base.ty);
            let base_atom = atomize(base, next_temp, accum);
            let index_atom = atomize(index, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::AIndex {
                    base: base_atom,
                    index: index_atom,
                    base_ty: bty,
                    result_ty: expr.ty.clone(),
                },
            ));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        // ── Defer: lower inner independently, push ADefer, return ALitVoid ─────
        CoreExprKind::Defer(inner) => {
            let inner_anf = lower_expr_top(inner, next_temp);
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::ADefer(Box::new(inner_anf))));
            AnfExpr::Atom(Atom::ALitVoid)
        }

        // ── Assign: atomize value, push AAssign, return Atom(ALitVoid) ─────────
        CoreExprKind::Assign { local, value } => {
            let val_atom = atomize(value, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::AAssign {
                    local: *local,
                    value: val_atom,
                },
            ));
            AnfExpr::Atom(Atom::ALitVoid)
        }

        // ── Let: introduce a new binding for orig_local, then lower body ────────
        // Uses AInit (not AAssign) — this is a fresh local introduction, not mutation.
        CoreExprKind::Let {
            local: orig_local,
            value,
            body,
        } => {
            let orig_local = *orig_local;
            // Lower value in an isolated accumulator so terminal value expressions
            // don't leak partial bindings into the caller's accumulator.
            let mut value_accum: LetAccum = Vec::new();
            let value_result = lower_expr(value, next_temp, &mut value_accum);
            match value_result {
                AnfExpr::Atom(atom) => {
                    accum.extend(value_accum);
                    // Value reduced to an atom — initialize orig_local with it.
                    // AInit marks this as a new binding (distinct from AAssign mutation).
                    accum.push((orig_local, AnfOp::AInit { value: atom }));
                }
                // Terminals: body is unreachable. Return a self-contained subtree
                // for value side effects + terminal, without mutating outer accum.
                terminal => {
                    return build_lets(value_accum, terminal);
                }
            }
            // Lower body (continues with the same accum).
            lower_expr(body, next_temp, accum)
        }

        // ── If: atomize cond, lower branches independently ────────────────────
        CoreExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let cond_atom = atomize(cond, next_temp, accum);
            let then_anf = lower_expr_top(then_branch, next_temp);
            let else_anf = lower_expr_top(else_branch, next_temp);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::AIf {
                    cond: cond_atom,
                    then_branch: Box::new(then_anf),
                    else_branch: Box::new(else_anf),
                },
            ));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        // ── Match: atomize scrutinee, lower each arm independently ─────────────
        CoreExprKind::Match { scrutinee, arms } => {
            let scrut_atom = atomize(scrutinee, next_temp, accum);
            let anf_arms: Vec<AnfMatchArm> = arms
                .iter()
                .map(|MatchArm { pattern, body }| AnfMatchArm {
                    pattern: pattern.clone(),
                    body: lower_expr_top(body, next_temp),
                })
                .collect();
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::AMatch {
                    scrutinee: scrut_atom,
                    arms: anf_arms,
                },
            ));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        // ── Loop: lower body independently ────────────────────────────────────
        CoreExprKind::Loop { body } => {
            let body_anf = lower_expr_top(body, next_temp);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::ALoop {
                    body: Box::new(body_anf),
                },
            ));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        // ── Terminals ─────────────────────────────────────────────────────────
        // Flush accum before emitting terminal (accum will be wrapped around it by caller).
        CoreExprKind::Break { value: None } => AnfExpr::Break(None),
        CoreExprKind::Break { value: Some(val) } => {
            let atom = atomize(val, next_temp, accum);
            AnfExpr::Break(Some(atom))
        }
        CoreExprKind::Continue => AnfExpr::Continue,
        CoreExprKind::Return { value: None } => AnfExpr::Return(None),
        CoreExprKind::Return { value: Some(val) } => {
            let atom = atomize(val, next_temp, accum);
            AnfExpr::Return(Some(atom))
        }
    }
}

/// Atomize a `CoreExpr`: lower it and ensure the result is an `Atom`.
///
/// For already-atomic expressions (literals, locals), returns the atom directly.
/// For non-atomic expressions, pushes a `(tmp, op)` pair to `accum` and returns
/// `ALocal(tmp)` as the atom.
///
/// For structural forms (If, Match, Loop), this lowers them independently and
/// binds the result to a fresh temp.
fn atomize(expr: &CoreExpr, next_temp: &mut u32, accum: &mut LetAccum) -> Atom {
    match &expr.kind {
        // Already atoms
        CoreExprKind::LitInt(n) => Atom::ALitInt(*n),
        CoreExprKind::LitFloat(v) => Atom::ALitFloat(*v),
        CoreExprKind::LitBool(b) => Atom::ALitBool(*b),
        CoreExprKind::LitStr(s) => Atom::ALitStr(s.clone()),
        CoreExprKind::LitVoid => Atom::ALitVoid,
        CoreExprKind::Local(id) => Atom::ALocal(*id),
        CoreExprKind::GlobalLocal(id) => Atom::ALocal(*id),
        CoreExprKind::GlobalFunc(id) => Atom::AGlobalFunc(*id),

        // For structural forms (If/Match/Loop/Let), we lower independently and
        // bind the result to a temp.
        CoreExprKind::If { .. }
        | CoreExprKind::Match { .. }
        | CoreExprKind::Loop { .. }
        | CoreExprKind::Let { .. } => {
            let anf = lower_expr_top(expr, next_temp);
            // Wrap into an If/Match/Loop op bound to a temp.
            // But lower_expr_top returns a full AnfExpr, not an op.
            // We need to splat it as an AIf/AMatch/ALoop if possible, or
            // use a special "inline block" mechanism.
            //
            // The cleanest approach: inline the Let-chain from anf into accum.
            // We do this by extracting the Let bindings from the AnfExpr.
            let tmp = fresh(next_temp);
            let wrapped = splice_atom_bind(anf, tmp);
            flatten_into_accum(wrapped, accum, tmp);
            Atom::ALocal(tmp)
        }

        // Break/Continue/Return in operand (atomize) position indicates malformed Core IR.
        // A terminal expression cannot produce a value that is used as an operand.
        // The type checker prevents this via Never-typed expressions, but guard anyway.
        CoreExprKind::Break { .. } | CoreExprKind::Continue | CoreExprKind::Return { .. } => {
            panic!(
                "ANF lowering: terminal expression ({:?}) in operand position — Core IR is malformed",
                &expr.kind
            );
        }

        // Non-atomic: push the op to accum, return temp atom.
        // These mirror lower_expr's non-atomic cases.
        CoreExprKind::MakeClosure { func_id, free_vars } => {
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::AMakeClosure {
                    func_id: *func_id,
                    free_vars: free_vars.clone(),
                },
            ));
            Atom::ALocal(tmp)
        }
        CoreExprKind::BinOp { op, left, right } => {
            let operand_ty = op_kind_from(&left.ty);
            let left_atom = atomize(left, next_temp, accum);
            let right_atom = atomize(right, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::ABinOp {
                    op: *op,
                    left: left_atom,
                    right: right_atom,
                    operand_ty,
                },
            ));
            Atom::ALocal(tmp)
        }
        CoreExprKind::UnOp { op, expr: inner } => {
            let operand_ty = op_kind_from(&inner.ty);
            let inner_atom = atomize(inner, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::AUnOp {
                    op: *op,
                    expr: inner_atom,
                    operand_ty,
                },
            ));
            Atom::ALocal(tmp)
        }
        CoreExprKind::Call { callee, args } => {
            let callee_atom = atomize(callee, next_temp, accum);
            let arg_atoms: Vec<Atom> = args.iter().map(|a| atomize(a, next_temp, accum)).collect();
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::ACall {
                    callee: callee_atom,
                    args: arg_atoms,
                },
            ));
            Atom::ALocal(tmp)
        }
        CoreExprKind::Record { type_id, fields } => {
            let type_id = *type_id;
            let anf_fields: Vec<_> = fields
                .iter()
                .map(|(fid, e)| (*fid, atomize(e, next_temp, accum)))
                .collect();
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::ARecord {
                    type_id,
                    fields: anf_fields,
                },
            ));
            Atom::ALocal(tmp)
        }
        CoreExprKind::RecordGet { target, field } => {
            let tid = type_id_from(&target.ty);
            let target_atom = atomize(target, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::ARecordGet {
                    target: target_atom,
                    field: *field,
                    type_id: tid,
                },
            ));
            Atom::ALocal(tmp)
        }
        CoreExprKind::RecordUpdate { base, field, value } => {
            let tid = type_id_from(&base.ty);
            let base_atom = atomize(base, next_temp, accum);
            let value_atom = atomize(value, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::ARecordUpdate {
                    base: base_atom,
                    field: *field,
                    value: value_atom,
                    can_reuse_in_place: false,
                    type_id: tid,
                },
            ));
            Atom::ALocal(tmp)
        }
        CoreExprKind::Variant {
            type_id,
            variant,
            args,
        } => {
            let type_id = *type_id;
            let variant = *variant;
            let arg_atoms: Vec<Atom> = args.iter().map(|a| atomize(a, next_temp, accum)).collect();
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::AVariant {
                    type_id,
                    variant,
                    args: arg_atoms,
                },
            ));
            Atom::ALocal(tmp)
        }
        CoreExprKind::ArrayLit { elements } => {
            let atoms: Vec<Atom> = elements
                .iter()
                .map(|e| atomize(e, next_temp, accum))
                .collect();
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::AArrayLit(atoms)));
            Atom::ALocal(tmp)
        }
        CoreExprKind::Index { base, index } => {
            let bty = index_kind_from(&base.ty);
            let base_atom = atomize(base, next_temp, accum);
            let index_atom = atomize(index, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::AIndex {
                    base: base_atom,
                    index: index_atom,
                    base_ty: bty,
                    result_ty: expr.ty.clone(),
                },
            ));
            Atom::ALocal(tmp)
        }
        CoreExprKind::Assign { local, value } => {
            let val_atom = atomize(value, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::AAssign {
                    local: *local,
                    value: val_atom,
                },
            ));
            Atom::ALitVoid
        }
        CoreExprKind::Defer(inner) => {
            let inner_anf = lower_expr_top(inner, next_temp);
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::ADefer(Box::new(inner_anf))));
            Atom::ALitVoid
        }
    }
}

/// Walk an `AnfExpr` produced by `lower_expr_top` and extract all its Let bindings
/// into `accum`, binding the final result atom to `result_local`.
///
/// This is used when we need to "inline" a structural expression's result into
/// the current let-accumulator context.
fn flatten_into_accum(anf: AnfExpr, accum: &mut LetAccum, result_local: LocalId) {
    match anf {
        AnfExpr::Let { local, op, body } => {
            accum.push((local, *op));
            flatten_into_accum(*body, accum, result_local);
        }
        AnfExpr::Atom(atom) => {
            accum.push((result_local, AnfOp::AInit { value: atom }));
        }
        // A terminal (Return/Break/Continue) in flattening position means the structural
        // sub-expression always diverges. This indicates the Core IR has a terminal as the
        // value of a Let binding or in an otherwise unexpected position. The lowerer does
        // not generate such trees; if this panics, investigate the Core IR producer.
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue => {
            panic!(
                "ANF lowering: terminal expression in flatten_into_accum — \
                 structural sub-expression diverges (result_local = L{}). \
                 Core IR should not have terminals in value position.",
                result_local.0
            );
        }
    }
}

/// Wrap an `AnfExpr` tree such that its final `Atom(a)` leaf is replaced with
/// a binding of `a` to `result_local`.
fn splice_atom_bind(anf: AnfExpr, result_local: LocalId) -> AnfExpr {
    match anf {
        AnfExpr::Let { local, op, body } => AnfExpr::Let {
            local,
            op,
            body: Box::new(splice_atom_bind(*body, result_local)),
        },
        AnfExpr::Atom(atom) => AnfExpr::Let {
            local: result_local,
            op: Box::new(AnfOp::AInit { value: atom }),
            body: Box::new(AnfExpr::Atom(Atom::ALocal(result_local))),
        },
        terminal => terminal,
    }
}

/// Find the maximum LocalId used in a function (params + body).
fn max_local_id_in_func(func: &FunctionDef) -> u32 {
    let mut max = 0u32;
    for param in &func.params {
        if param.0 > max {
            max = param.0;
        }
    }
    max_local_id_in_expr(&func.body, &mut max);
    max
}

/// Walk a `CoreExpr` and update `max` with the highest `LocalId` seen.
fn max_local_id_in_expr(expr: &CoreExpr, max: &mut u32) {
    match &expr.kind {
        CoreExprKind::Local(id) | CoreExprKind::GlobalLocal(id) => {
            if id.0 > *max {
                *max = id.0;
            }
        }
        CoreExprKind::Let { local, value, body } => {
            if local.0 > *max {
                *max = local.0;
            }
            max_local_id_in_expr(value, max);
            max_local_id_in_expr(body, max);
        }
        CoreExprKind::Assign { local, value } => {
            if local.0 > *max {
                *max = local.0;
            }
            max_local_id_in_expr(value, max);
        }
        CoreExprKind::MakeClosure { free_vars, .. } => {
            for v in free_vars {
                if v.0 > *max {
                    *max = v.0;
                }
            }
        }
        CoreExprKind::BinOp { left, right, .. } => {
            max_local_id_in_expr(left, max);
            max_local_id_in_expr(right, max);
        }
        CoreExprKind::UnOp { expr, .. } => max_local_id_in_expr(expr, max),
        CoreExprKind::Call { callee, args } => {
            max_local_id_in_expr(callee, max);
            for arg in args {
                max_local_id_in_expr(arg, max);
            }
        }
        CoreExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            max_local_id_in_expr(cond, max);
            max_local_id_in_expr(then_branch, max);
            max_local_id_in_expr(else_branch, max);
        }
        CoreExprKind::Match { scrutinee, arms } => {
            max_local_id_in_expr(scrutinee, max);
            for arm in arms {
                max_local_id_in_expr(&arm.body, max);
                max_local_id_in_pattern(&arm.pattern, max);
            }
        }
        CoreExprKind::Loop { body } => max_local_id_in_expr(body, max),
        CoreExprKind::Break { value } => {
            if let Some(v) = value {
                max_local_id_in_expr(v, max);
            }
        }
        CoreExprKind::Return { value } => {
            if let Some(v) = value {
                max_local_id_in_expr(v, max);
            }
        }
        CoreExprKind::Record { fields, .. } => {
            for (_, v) in fields {
                max_local_id_in_expr(v, max);
            }
        }
        CoreExprKind::RecordGet { target, .. } => max_local_id_in_expr(target, max),
        CoreExprKind::RecordUpdate { base, value, .. } => {
            max_local_id_in_expr(base, max);
            max_local_id_in_expr(value, max);
        }
        CoreExprKind::Variant { args, .. } => {
            for arg in args {
                max_local_id_in_expr(arg, max);
            }
        }
        CoreExprKind::ArrayLit { elements } => {
            for e in elements {
                max_local_id_in_expr(e, max);
            }
        }
        CoreExprKind::Index { base, index } => {
            max_local_id_in_expr(base, max);
            max_local_id_in_expr(index, max);
        }
        CoreExprKind::Defer(inner) => max_local_id_in_expr(inner, max),
        CoreExprKind::LitInt(_)
        | CoreExprKind::LitFloat(_)
        | CoreExprKind::LitBool(_)
        | CoreExprKind::LitStr(_)
        | CoreExprKind::LitVoid
        | CoreExprKind::GlobalFunc(_)
        | CoreExprKind::Continue => {}
    }
}

fn max_local_id_in_pattern(pattern: &CorePattern, max: &mut u32) {
    match pattern {
        CorePattern::Var(id) => {
            if id.0 > *max {
                *max = id.0;
            }
        }
        CorePattern::Variant { fields, .. } => {
            for f in fields {
                max_local_id_in_pattern(f, max);
            }
        }
        CorePattern::Wildcard
        | CorePattern::LitInt(_)
        | CorePattern::LitBool(_)
        | CorePattern::LitStr(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::lower_expr_top;
    use crate::ir::anf::{AnfExpr, AnfOp, Atom, OpKind};
    use crate::ir::core::{CoreExpr, CoreExprKind, LocalId};
    use crate::syntax::ast::BinOp;
    use crate::syntax::span::{FileId, Span};
    use crate::types::ty::MonoType;

    fn sp() -> Span {
        Span::new(FileId(0), 0, 0)
    }

    fn lit_int(n: i64) -> CoreExpr {
        CoreExpr {
            kind: CoreExprKind::LitInt(n),
            ty: MonoType::Int,
            span: sp(),
        }
    }

    fn local(id: u32, ty: MonoType) -> CoreExpr {
        CoreExpr {
            kind: CoreExprKind::Local(LocalId(id)),
            ty,
            span: sp(),
        }
    }

    #[test]
    fn let_value_terminal_keeps_value_bindings_inside_returned_subtree() {
        let inner_value = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Add,
                left: Box::new(lit_int(1)),
                right: Box::new(lit_int(2)),
            },
            ty: MonoType::Int,
            span: sp(),
        };
        let inner_let = CoreExpr {
            kind: CoreExprKind::Let {
                local: LocalId(2),
                value: Box::new(inner_value),
                body: Box::new(CoreExpr {
                    kind: CoreExprKind::Return {
                        value: Some(Box::new(local(2, MonoType::Int))),
                    },
                    ty: MonoType::Never,
                    span: sp(),
                }),
            },
            ty: MonoType::Never,
            span: sp(),
        };
        let outer = CoreExpr {
            kind: CoreExprKind::Let {
                local: LocalId(1),
                value: Box::new(inner_let),
                body: Box::new(lit_int(99)),
            },
            ty: MonoType::Never,
            span: sp(),
        };

        let mut next_temp = 3;
        let anf = lower_expr_top(&outer, &mut next_temp);

        let (outer_op, outer_body) = match anf {
            AnfExpr::Let {
                local: LocalId(3),
                op,
                body,
            } => (op, body),
            other => panic!("unexpected ANF shape for outer let: {other:?}"),
        };
        assert!(matches!(
            *outer_op,
            AnfOp::ABinOp {
                op: BinOp::Add,
                operand_ty: OpKind::Int,
                ..
            }
        ));

        let (inner_op, inner_body) = match *outer_body {
            AnfExpr::Let {
                local: LocalId(2),
                op,
                body,
            } => (op, body),
            other => panic!("unexpected ANF shape for inner let: {other:?}"),
        };
        assert!(matches!(
            *inner_op,
            AnfOp::AInit {
                value: Atom::ALocal(LocalId(3))
            }
        ));
        assert!(matches!(
            *inner_body,
            AnfExpr::Return(Some(Atom::ALocal(LocalId(2))))
        ));
    }

    #[test]
    fn let_value_non_terminal_still_commits_value_bindings_to_outer_accum() {
        let value = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Add,
                left: Box::new(lit_int(1)),
                right: Box::new(lit_int(2)),
            },
            ty: MonoType::Int,
            span: sp(),
        };
        let expr = CoreExpr {
            kind: CoreExprKind::Let {
                local: LocalId(1),
                value: Box::new(value),
                body: Box::new(local(1, MonoType::Int)),
            },
            ty: MonoType::Int,
            span: sp(),
        };

        let mut next_temp = 2;
        let anf = lower_expr_top(&expr, &mut next_temp);

        let (outer_op, outer_body) = match anf {
            AnfExpr::Let {
                local: LocalId(2),
                op,
                body,
            } => (op, body),
            other => panic!("unexpected ANF shape for outer let: {other:?}"),
        };
        assert!(matches!(
            *outer_op,
            AnfOp::ABinOp {
                op: BinOp::Add,
                operand_ty: OpKind::Int,
                ..
            }
        ));

        let (inner_op, inner_body) = match *outer_body {
            AnfExpr::Let {
                local: LocalId(1),
                op,
                body,
            } => (op, body),
            other => panic!("unexpected ANF shape for inner let: {other:?}"),
        };
        assert!(matches!(
            *inner_op,
            AnfOp::AInit {
                value: Atom::ALocal(LocalId(2))
            }
        ));
        assert!(matches!(
            *inner_body,
            AnfExpr::Atom(Atom::ALocal(LocalId(1)))
        ));
    }
}
