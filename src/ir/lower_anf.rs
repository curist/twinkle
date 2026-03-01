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
use crate::ir::anf::{Atom, AnfExpr, AnfFunctionDef, AnfMatchArm, AnfModule, AnfOp};
use crate::ir::core::{
    CoreExpr, CoreExprKind, CoreModule, CorePattern, FunctionDef, LocalId, MatchArm,
};

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
    accum.into_iter().rev().fold(tail, |body, (local, op)| AnfExpr::Let {
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
                AnfOp::AMakeClosure { func_id: *func_id, free_vars: free_vars.clone() },
            ));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::BinOp { op, left, right } => {
            let left_atom = atomize(left, next_temp, accum);
            let right_atom = atomize(right, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::ABinOp { op: *op, left: left_atom, right: right_atom }));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::UnOp { op, expr: inner } => {
            let inner_atom = atomize(inner, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::AUnOp { op: *op, expr: inner_atom }));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::Call { callee, args } => {
            let callee_atom = atomize(callee, next_temp, accum);
            let arg_atoms: Vec<Atom> = args.iter().map(|a| atomize(a, next_temp, accum)).collect();
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::ACall { callee: callee_atom, args: arg_atoms }));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::Record { type_id, fields } => {
            let type_id = *type_id;
            let anf_fields: Vec<_> = fields
                .iter()
                .map(|(fid, e)| (*fid, atomize(e, next_temp, accum)))
                .collect();
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::ARecord { type_id, fields: anf_fields }));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::RecordGet { target, field } => {
            let target_atom = atomize(target, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::ARecordGet { target: target_atom, field: *field }));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::RecordUpdate { base, field, value } => {
            let base_atom = atomize(base, next_temp, accum);
            let value_atom = atomize(value, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::ARecordUpdate { base: base_atom, field: *field, value: value_atom },
            ));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::Variant { type_id, variant, args } => {
            let type_id = *type_id;
            let variant = *variant;
            let arg_atoms: Vec<Atom> = args.iter().map(|a| atomize(a, next_temp, accum)).collect();
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::AVariant { type_id, variant, args: arg_atoms }));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::ArrayLit { elements } => {
            let atoms: Vec<Atom> = elements.iter().map(|e| atomize(e, next_temp, accum)).collect();
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::AArrayLit(atoms)));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        CoreExprKind::Index { base, index } => {
            let base_atom = atomize(base, next_temp, accum);
            let index_atom = atomize(index, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::AIndex { base: base_atom, index: index_atom }));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        // ── Assign: atomize value, push AAssign, return Atom(ALitVoid) ─────────
        CoreExprKind::Assign { local, value } => {
            let val_atom = atomize(value, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::AAssign { local: *local, value: val_atom }));
            AnfExpr::Atom(Atom::ALitVoid)
        }

        // ── Let: bind value to orig_local, then lower body ────────────────────
        // We flush the current accum, produce a Let for orig_local, then lower body.
        CoreExprKind::Let { local: orig_local, value, body } => {
            let orig_local = *orig_local;
            // Lower value: collect its ops into accum, get the result atom/expr.
            let value_result = lower_expr(value, next_temp, accum);
            // Bind the result of value to orig_local.
            match value_result {
                AnfExpr::Atom(atom) => {
                    // Simple case: value reduced to an atom. Bind orig_local to it.
                    accum.push((orig_local, AnfOp::AAssign { local: orig_local, value: atom }));
                }
                AnfExpr::Let { .. } => {
                    // This shouldn't happen: lower_expr for non-structural forms always
                    // returns Atom (with ops in accum). For structural forms (If/Match/Loop),
                    // they return a Let-chain. We need to emit those into accum too.
                    // Handle by unwinding the Let chain.
                    // Actually: for structural forms, we need to:
                    // 1. Flush accum (build the let-chain so far as prefix).
                    // 2. Lower the structural value independently.
                    // 3. Bind its result atom to orig_local.
                    // 4. Continue with body.
                    //
                    // Since we're in the imperative style, the cleaner approach is:
                    // for structural values, we snapshot the accum, lower the value
                    // independently, and splice the result binding into orig_local.
                    // But lower_expr returned an AnfExpr here — we need to re-lower.
                    //
                    // This indicates a design issue: lower_expr should return Atom always
                    // for non-structural, and full AnfExpr for structural.
                    // We handle this by checking value.kind before calling lower_expr.
                    unreachable!("lower_expr returned Let for a non-structural value — this is a bug");
                }
                // Terminals: the body is unreachable.
                terminal => {
                    return terminal;
                }
            }
            // Lower body (continues with the same accum).
            lower_expr(body, next_temp, accum)
        }

        // ── If: atomize cond, lower branches independently ────────────────────
        CoreExprKind::If { cond, then_branch, else_branch } => {
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
            accum.push((tmp, AnfOp::AMatch { scrutinee: scrut_atom, arms: anf_arms }));
            AnfExpr::Atom(Atom::ALocal(tmp))
        }

        // ── Loop: lower body independently ────────────────────────────────────
        CoreExprKind::Loop { body } => {
            let body_anf = lower_expr_top(body, next_temp);
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::ALoop { body: Box::new(body_anf) }));
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

        // Break/Continue/Return: these are terminals — atomizing them doesn't make sense.
        // If they appear in atomize position, it's because of a structural expression
        // that terminates early. We handle them as void atoms but push the terminal
        // as a special marker.
        CoreExprKind::Break { value: None } => {
            accum.push((LocalId(u32::MAX), AnfOp::AAssign { local: LocalId(u32::MAX), value: Atom::ALitVoid }));
            Atom::ALitVoid
        }
        CoreExprKind::Break { value: Some(val) } => {
            let atom = atomize(val, next_temp, accum);
            accum.push((LocalId(u32::MAX), AnfOp::AAssign { local: LocalId(u32::MAX), value: atom }));
            Atom::ALitVoid
        }
        CoreExprKind::Continue => {
            Atom::ALitVoid
        }
        CoreExprKind::Return { value: None } => {
            Atom::ALitVoid
        }
        CoreExprKind::Return { value: Some(val) } => {
            atomize(val, next_temp, accum)
        }

        // Non-atomic: push the op to accum, return temp atom.
        // These mirror lower_expr's non-atomic cases.
        CoreExprKind::MakeClosure { func_id, free_vars } => {
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::AMakeClosure { func_id: *func_id, free_vars: free_vars.clone() },
            ));
            Atom::ALocal(tmp)
        }
        CoreExprKind::BinOp { op, left, right } => {
            let left_atom = atomize(left, next_temp, accum);
            let right_atom = atomize(right, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::ABinOp { op: *op, left: left_atom, right: right_atom }));
            Atom::ALocal(tmp)
        }
        CoreExprKind::UnOp { op, expr: inner } => {
            let inner_atom = atomize(inner, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::AUnOp { op: *op, expr: inner_atom }));
            Atom::ALocal(tmp)
        }
        CoreExprKind::Call { callee, args } => {
            let callee_atom = atomize(callee, next_temp, accum);
            let arg_atoms: Vec<Atom> = args.iter().map(|a| atomize(a, next_temp, accum)).collect();
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::ACall { callee: callee_atom, args: arg_atoms }));
            Atom::ALocal(tmp)
        }
        CoreExprKind::Record { type_id, fields } => {
            let type_id = *type_id;
            let anf_fields: Vec<_> = fields
                .iter()
                .map(|(fid, e)| (*fid, atomize(e, next_temp, accum)))
                .collect();
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::ARecord { type_id, fields: anf_fields }));
            Atom::ALocal(tmp)
        }
        CoreExprKind::RecordGet { target, field } => {
            let target_atom = atomize(target, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::ARecordGet { target: target_atom, field: *field }));
            Atom::ALocal(tmp)
        }
        CoreExprKind::RecordUpdate { base, field, value } => {
            let base_atom = atomize(base, next_temp, accum);
            let value_atom = atomize(value, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((
                tmp,
                AnfOp::ARecordUpdate { base: base_atom, field: *field, value: value_atom },
            ));
            Atom::ALocal(tmp)
        }
        CoreExprKind::Variant { type_id, variant, args } => {
            let type_id = *type_id;
            let variant = *variant;
            let arg_atoms: Vec<Atom> = args.iter().map(|a| atomize(a, next_temp, accum)).collect();
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::AVariant { type_id, variant, args: arg_atoms }));
            Atom::ALocal(tmp)
        }
        CoreExprKind::ArrayLit { elements } => {
            let atoms: Vec<Atom> = elements.iter().map(|e| atomize(e, next_temp, accum)).collect();
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::AArrayLit(atoms)));
            Atom::ALocal(tmp)
        }
        CoreExprKind::Index { base, index } => {
            let base_atom = atomize(base, next_temp, accum);
            let index_atom = atomize(index, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::AIndex { base: base_atom, index: index_atom }));
            Atom::ALocal(tmp)
        }
        CoreExprKind::Assign { local, value } => {
            let val_atom = atomize(value, next_temp, accum);
            let tmp = fresh(next_temp);
            accum.push((tmp, AnfOp::AAssign { local: *local, value: val_atom }));
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
            accum.push((result_local, AnfOp::AAssign { local: result_local, value: atom }));
        }
        // Terminals: the binding is unreachable.
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue => {
            // Push a terminal marker — the result_local will never be used.
            // We can't push terminals to accum (which only holds (LocalId, AnfOp) pairs).
            // Instead, we need a way to signal that the code after this is unreachable.
            // For now, we add a dummy void assign (will be cleaned up in a later pass).
            accum.push((
                result_local,
                AnfOp::AAssign { local: result_local, value: Atom::ALitVoid },
            ));
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
            op: Box::new(AnfOp::AAssign { local: result_local, value: atom }),
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
        CoreExprKind::If { cond, then_branch, else_branch } => {
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
