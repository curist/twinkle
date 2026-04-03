use std::collections::HashSet;

use crate::ir::anf::{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfOp, Atom};
use crate::ir::core::{CorePattern, FuncId, LocalId};
use crate::ir::lower::prelude;
use crate::opt::liveness::live_after;

// ── Known COW operations ─────────────────────────────────────────────────────

struct CowOpInfo {
    // If present, the call may be rewritten to this in-place intrinsic when
    // the base local is unique + consumed.
    in_place_rewrite: Option<FuncId>,
    base_arg: usize,
}

fn cow_op_info(func_id: FuncId) -> Option<CowOpInfo> {
    if func_id == prelude::VECTOR_SET_UNSAFE {
        Some(CowOpInfo {
            in_place_rewrite: Some(prelude::VECTOR_SET_IN_PLACE),
            base_arg: 0,
        })
    } else if func_id == prelude::DICT_SET {
        Some(CowOpInfo {
            in_place_rewrite: Some(prelude::DICT_SET_IN_PLACE),
            base_arg: 0,
        })
    } else if func_id == prelude::DICT_REMOVE {
        Some(CowOpInfo {
            in_place_rewrite: Some(prelude::DICT_REMOVE_IN_PLACE),
            base_arg: 0,
        })
    } else if func_id == prelude::VECTOR_APPEND {
        // Growth update known to preserve uniqueness; loop-region rewrite handles
        // push with builder wrapping.
        Some(CowOpInfo {
            in_place_rewrite: None,
            base_arg: 0,
        })
    } else {
        None
    }
}

fn is_no_retain_read_only(func_id: FuncId) -> bool {
    func_id == prelude::VECTOR_LEN
        || func_id == prelude::DICT_LEN
        || func_id == prelude::DICT_HAS
        || func_id == prelude::DICT_GET
        || func_id == prelude::DICT_GET_UNSAFE
        || func_id == prelude::DICT_KEYS
}

/// Info for a COW op that can be rewritten to an in-place variant inside a loop
/// by simply swapping the callee (no builder lifecycle needed).
struct InPlaceSwapInfo {
    in_place_id: FuncId,
    base_arg: usize,
}

/// Check if a func_id is a COW op that can be rewritten to in-place by a simple
/// callee swap (i.e., has an in_place_rewrite and doesn't need builder wrapping).
fn in_place_swap_info(func_id: FuncId) -> Option<InPlaceSwapInfo> {
    let info = cow_op_info(func_id)?;
    Some(InPlaceSwapInfo {
        in_place_id: info.in_place_rewrite?,
        base_arg: info.base_arg,
    })
}

fn alloc_local(next_local: &mut u32) -> LocalId {
    let local = LocalId(*next_local);
    *next_local += 1;
    local
}

// ── Fresh producer detection ─────────────────────────────────────────────────

fn is_fresh_producer(op: &AnfOp) -> bool {
    match op {
        AnfOp::AArrayLit(_) | AnfOp::ARecord { .. } | AnfOp::AVariant { .. } => true,
        AnfOp::ACall {
            callee: Atom::AGlobalFunc(id),
            ..
        } => {
            *id == prelude::VECTOR_MAKE
                || *id == prelude::VECTOR_BUILDER_FREEZE
                || *id == prelude::DICT_NEW
        }
        _ => false,
    }
}

// ── Pre-scan: collect tainted (aliased / escaped) locals ─────────────────────

fn collect_tainted(func: &AnfFunctionDef) -> HashSet<LocalId> {
    let mut tainted = HashSet::new();
    // Function params come from outside — never unique.
    for p in &func.params {
        tainted.insert(*p);
    }
    scan_tainted_expr(&func.body, &mut tainted, &HashSet::new());
    tainted
}

fn scan_tainted_expr(expr: &AnfExpr, tainted: &mut HashSet<LocalId>, live_out: &HashSet<LocalId>) {
    match expr {
        AnfExpr::Let { local, op, body } => {
            let bind_local = *local;
            let live_after_body = live_after(body);
            let init_alias_source = if let AnfOp::AInit {
                value: Atom::ALocal(source),
            } = op.as_ref()
            {
                Some(*source)
            } else {
                None
            };

            // Alias copy: let y = x
            // Taint when source is still live after the copy (straight-line aliasing).
            if let Some(source) = init_alias_source {
                if live_after_body.contains(&source) {
                    tainted.insert(source);
                }
            }
            // Alias copy through reassignment: y = x
            if let AnfOp::AAssign {
                value: Atom::ALocal(source),
                ..
            } = op.as_ref()
            {
                // Reassignment can cross sub-expression boundaries (e.g. branch writes
                // to an outer local). Include outer live-out for conservative safety.
                if live_after_body.contains(source) || live_out.contains(source) {
                    tainted.insert(*source);
                }
            }
            scan_tainted_op(op, tainted, &live_after_body);
            scan_tainted_expr(body, tainted, live_out);

            // Branch-boundary alias escape:
            // If an init alias `y := x` occurs in a nested scope and `y` escapes
            // (e.g. captured by a closure assigned outward), then `x` is aliased
            // across the boundary when `x` is live-out of the parent continuation.
            if let Some(source) = init_alias_source {
                if live_out.contains(&source) && tainted.contains(&bind_local) {
                    tainted.insert(source);
                }
            }
        }
        _ => {}
    }
}

fn scan_tainted_op(op: &AnfOp, tainted: &mut HashSet<LocalId>, live_out: &HashSet<LocalId>) {
    match op {
        // Escaped: captured by closure
        AnfOp::AMakeClosure { free_vars, .. } => {
            for v in free_vars {
                tainted.insert(*v);
            }
        }
        // Stored in container — reference escapes into the container
        AnfOp::AArrayLit(elems) => {
            for a in elems {
                if let Atom::ALocal(x) = a {
                    tainted.insert(*x);
                }
            }
        }
        AnfOp::ARecord { fields, .. } => {
            for (_, a) in fields {
                if let Atom::ALocal(x) = a {
                    tainted.insert(*x);
                }
            }
        }
        AnfOp::AVariant { args, .. } => {
            for a in args {
                if let Atom::ALocal(x) = a {
                    tainted.insert(*x);
                }
            }
        }
        // Passed to non-COW function — might be retained
        AnfOp::ACall {
            callee: Atom::AGlobalFunc(func_id),
            args,
        } => {
            if let Some(info) = cow_op_info(*func_id) {
                // COW op: only taint non-base args
                for (i, a) in args.iter().enumerate() {
                    if i != info.base_arg {
                        if let Atom::ALocal(x) = a {
                            tainted.insert(*x);
                        }
                    }
                }
            } else if !is_no_retain_read_only(*func_id) {
                // Non-COW function: taint all local args (conservative)
                for a in args {
                    if let Atom::ALocal(x) = a {
                        tainted.insert(*x);
                    }
                }
            }
        }
        // Indirect call (closure call): taint all local args
        AnfOp::ACall { args, .. } => {
            for a in args {
                if let Atom::ALocal(x) = a {
                    tainted.insert(*x);
                }
            }
        }
        // Recurse into sub-expressions
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            scan_tainted_expr(then_branch, tainted, live_out);
            scan_tainted_expr(else_branch, tainted, live_out);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                scan_tainted_expr(&arm.body, tainted, live_out);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            scan_tainted_expr(body, tainted, live_out);
        }
        _ => {}
    }
}

// ── Consume-reassign pattern detection ───────────────────────────────────────

/// Checks if `body` starts with `Let { op: AAssign { local: base, value: result } }`.
fn is_consume_reassign(body: &AnfExpr, base: LocalId, result: LocalId) -> bool {
    if let AnfExpr::Let { op, .. } = body {
        if let AnfOp::AAssign {
            local,
            value: Atom::ALocal(v),
        } = op.as_ref()
        {
            return *local == base && *v == result;
        }
    }
    false
}

fn atom_is_local(atom: &Atom, local: LocalId) -> bool {
    matches!(atom, Atom::ALocal(id) if *id == local)
}

fn op_uses_local_non_recursive(op: &AnfOp, local: LocalId) -> bool {
    match op {
        AnfOp::ACall { callee, args } => {
            atom_is_local(callee, local) || args.iter().any(|a| atom_is_local(a, local))
        }
        AnfOp::ABinOp { left, right, .. } => {
            atom_is_local(left, local) || atom_is_local(right, local)
        }
        AnfOp::AUnOp { expr, .. } => atom_is_local(expr, local),
        AnfOp::AMakeClosure { free_vars, .. } => free_vars.contains(&local),
        AnfOp::ARecord { fields, .. } => fields.iter().any(|(_, a)| atom_is_local(a, local)),
        AnfOp::ARecordGet { target, .. } => atom_is_local(target, local),
        AnfOp::ARecordUpdate { base, value, .. } => {
            atom_is_local(base, local) || atom_is_local(value, local)
        }
        AnfOp::AVariant { args, .. } | AnfOp::AArrayLit(args) => {
            args.iter().any(|a| atom_is_local(a, local))
        }
        AnfOp::AIndex { base, index, .. } => {
            atom_is_local(base, local) || atom_is_local(index, local)
        }
        AnfOp::AInit { value } => atom_is_local(value, local),
        AnfOp::AAssign {
            local: target,
            value,
        } => *target == local || atom_is_local(value, local),
        AnfOp::AIf { cond, .. } => atom_is_local(cond, local),
        AnfOp::AMatch { scrutinee, .. } => atom_is_local(scrutinee, local),
        AnfOp::ALoop { .. } | AnfOp::ADefer(_) => false,
    }
}

fn analyze_loop_op_subexpr(op: &AnfOp, base: LocalId) -> Option<usize> {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => Some(analyze_loop_expr(then_branch, base)? + analyze_loop_expr(else_branch, base)?),
        AnfOp::AMatch { arms, .. } => {
            let mut sites = 0usize;
            for arm in arms {
                sites += analyze_loop_expr(&arm.body, base)?;
            }
            Some(sites)
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => analyze_loop_expr(body, base),
        _ => Some(0),
    }
}

/// Check if a call op is a COW consume-reassign on `base`:
///   let result = COW_OP(base, ...) ; assign(base = result)
/// where none of the non-base args reference `base`.
fn is_cow_consume_reassign(
    func_id: FuncId,
    args: &[Atom],
    body: &AnfExpr,
    base: LocalId,
    result: LocalId,
) -> bool {
    let Some(info) = cow_op_info(func_id) else {
        return false;
    };
    if info.base_arg >= args.len() {
        return false;
    }
    if !atom_is_local(&args[info.base_arg], base) {
        return false;
    }
    // No other arg should reference base
    for (i, arg) in args.iter().enumerate() {
        if i != info.base_arg && atom_is_local(arg, base) {
            return false;
        }
    }
    is_consume_reassign(body, base, result)
}

/// Analyze whether `base` is only used via consuming COW op + assign(base=result)
/// patterns inside the loop body (vector append, dict set, dict remove, etc.).
///
/// Returns the number of valid sites if allowed, otherwise `None`.
fn analyze_loop_expr(expr: &AnfExpr, base: LocalId) -> Option<usize> {
    match expr {
        AnfExpr::Let { local, op, body } => {
            if let AnfOp::ACall {
                callee: Atom::AGlobalFunc(func_id),
                args,
            } = op.as_ref()
            {
                if is_cow_consume_reassign(*func_id, args, body, base, *local) {
                    let AnfExpr::Let {
                        body: rest_after_assign,
                        ..
                    } = body.as_ref()
                    else {
                        return None;
                    };
                    return Some(1 + analyze_loop_expr(rest_after_assign, base)?);
                }
            }

            if op_uses_local_non_recursive(op, base) {
                return None;
            }
            Some(analyze_loop_op_subexpr(op, base)? + analyze_loop_expr(body, base)?)
        }
        AnfExpr::Atom(atom) | AnfExpr::Return(Some(atom)) | AnfExpr::Break(Some(atom)) => {
            if atom_is_local(atom, base) {
                None
            } else {
                Some(0)
            }
        }
        AnfExpr::Return(None) | AnfExpr::Break(None) | AnfExpr::Continue => Some(0),
    }
}

// ── Dict in-place loop rewrite (simple callee swap, no builder) ─────────────

/// Analyze whether the loop body uses `base` only via in-place-swappable COW ops
/// (dict set/remove). Returns the count of such sites, or None if base is used
/// in any other way (including vector append, which needs builder wrapping).
fn analyze_loop_dict_sites(expr: &AnfExpr, base: LocalId) -> Option<usize> {
    match expr {
        AnfExpr::Let { local, op, body } => {
            if let AnfOp::ACall {
                callee: Atom::AGlobalFunc(func_id),
                args,
            } = op.as_ref()
            {
                if in_place_swap_info(*func_id).is_some()
                    && is_cow_consume_reassign(*func_id, args, body, base, *local)
                {
                    let AnfExpr::Let {
                        body: rest_after_assign,
                        ..
                    } = body.as_ref()
                    else {
                        return None;
                    };
                    return Some(1 + analyze_loop_dict_sites(rest_after_assign, base)?);
                }
            }

            // Allow read-only ops that reference base (e.g., dict.has, dict.get, dict.len)
            if op_uses_local_non_recursive(op, base) {
                let is_read_only = matches!(
                    op.as_ref(),
                    AnfOp::ACall {
                        callee: Atom::AGlobalFunc(fid),
                        ..
                    } if is_no_retain_read_only(*fid)
                );
                if !is_read_only {
                    return None;
                }
            }
            Some(analyze_loop_dict_sites_in_op(op, base)? + analyze_loop_dict_sites(body, base)?)
        }
        AnfExpr::Atom(atom) | AnfExpr::Return(Some(atom)) | AnfExpr::Break(Some(atom)) => {
            if atom_is_local(atom, base) {
                None
            } else {
                Some(0)
            }
        }
        AnfExpr::Return(None) | AnfExpr::Break(None) | AnfExpr::Continue => Some(0),
    }
}

fn analyze_loop_dict_sites_in_op(op: &AnfOp, base: LocalId) -> Option<usize> {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => Some(
            analyze_loop_dict_sites(then_branch, base)?
                + analyze_loop_dict_sites(else_branch, base)?,
        ),
        AnfOp::AMatch { arms, .. } => {
            let mut sites = 0usize;
            for arm in arms {
                sites += analyze_loop_dict_sites(&arm.body, base)?;
            }
            Some(sites)
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => analyze_loop_dict_sites(body, base),
        _ => Some(0),
    }
}

/// Rewrite in-place-swappable COW ops in a loop body by swapping the callee ID.
fn rewrite_loop_dict_expr(expr: &mut AnfExpr, base: LocalId, sites: &mut usize) {
    let AnfExpr::Let { local, op, body } = expr else {
        return;
    };

    if let AnfOp::ACall {
        callee: Atom::AGlobalFunc(func_id),
        args,
    } = op.as_mut()
    {
        if let Some(swap) = in_place_swap_info(*func_id) {
            if atom_is_local(&args[swap.base_arg], base) && is_consume_reassign(body, base, *local)
            {
                *func_id = swap.in_place_id;
                *sites += 1;
                if let AnfExpr::Let {
                    body: rest_after_assign,
                    ..
                } = body.as_mut()
                {
                    rewrite_loop_dict_expr(rest_after_assign, base, sites);
                    return;
                }
            }
        }
    }

    rewrite_loop_dict_op_subexpr(op, base, sites);
    rewrite_loop_dict_expr(body, base, sites);
}

fn rewrite_loop_dict_op_subexpr(op: &mut AnfOp, base: LocalId, sites: &mut usize) {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_loop_dict_expr(then_branch, base, sites);
            rewrite_loop_dict_expr(else_branch, base, sites);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                rewrite_loop_dict_expr(&mut arm.body, base, sites);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            rewrite_loop_dict_expr(body, base, sites);
        }
        _ => {}
    }
}

// ── Vector builder loop rewrite ─────────────────────────────────────────────

fn rewrite_loop_op_subexpr(op: &mut AnfOp, base: LocalId, builder: LocalId, sites: &mut usize) {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_loop_expr(then_branch, base, builder, sites);
            rewrite_loop_expr(else_branch, base, builder, sites);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                rewrite_loop_expr(&mut arm.body, base, builder, sites);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            rewrite_loop_expr(body, base, builder, sites);
        }
        _ => {}
    }
}

fn rewrite_loop_expr(expr: &mut AnfExpr, base: LocalId, builder: LocalId, sites: &mut usize) {
    let AnfExpr::Let { local, op, body } = expr else {
        return;
    };

    if let AnfOp::ACall {
        callee: Atom::AGlobalFunc(func_id),
        args,
    } = op.as_mut()
    {
        if *func_id == prelude::VECTOR_APPEND
            && args.len() == 2
            && atom_is_local(&args[0], base)
            && !atom_is_local(&args[1], base)
            && is_consume_reassign(body, base, *local)
        {
            *func_id = prelude::VECTOR_BUILDER_PUSH;
            args[0] = Atom::ALocal(builder);
            if let AnfExpr::Let {
                op: assign_op,
                body: rest_after_assign,
                ..
            } = body.as_mut()
            {
                *assign_op = Box::new(AnfOp::AInit {
                    value: Atom::ALitVoid,
                });
                *sites += 1;
                rewrite_loop_expr(rest_after_assign, base, builder, sites);
                return;
            }
        }
    }

    rewrite_loop_op_subexpr(op, base, builder, sites);
    rewrite_loop_expr(body, base, builder, sites);
}

fn next_local_id(func: &AnfFunctionDef) -> u32 {
    let mut max = 0u32;
    for p in &func.params {
        note_max(*p, &mut max);
    }
    max_local_in_expr(&func.body, &mut max);
    max + 1
}

fn note_max(local: LocalId, max: &mut u32) {
    if local.0 > *max {
        *max = local.0;
    }
}

fn max_local_in_expr(expr: &AnfExpr, max: &mut u32) {
    match expr {
        AnfExpr::Let { local, op, body } => {
            note_max(*local, max);
            max_local_in_op(op, max);
            max_local_in_expr(body, max);
        }
        AnfExpr::Atom(atom) | AnfExpr::Return(Some(atom)) | AnfExpr::Break(Some(atom)) => {
            max_local_in_atom(atom, max);
        }
        AnfExpr::Return(None) | AnfExpr::Break(None) | AnfExpr::Continue => {}
    }
}

fn max_local_in_op(op: &AnfOp, max: &mut u32) {
    match op {
        AnfOp::ACall { callee, args } => {
            max_local_in_atom(callee, max);
            for arg in args {
                max_local_in_atom(arg, max);
            }
        }
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => {
            max_local_in_atom(cond, max);
            max_local_in_expr(then_branch, max);
            max_local_in_expr(else_branch, max);
        }
        AnfOp::AMatch { scrutinee, arms } => {
            max_local_in_atom(scrutinee, max);
            for AnfMatchArm { pattern, body } in arms {
                max_local_in_pattern(pattern, max);
                max_local_in_expr(body, max);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => max_local_in_expr(body, max),
        AnfOp::ABinOp { left, right, .. } => {
            max_local_in_atom(left, max);
            max_local_in_atom(right, max);
        }
        AnfOp::AUnOp { expr, .. } => max_local_in_atom(expr, max),
        AnfOp::AMakeClosure { free_vars, .. } => {
            for local in free_vars {
                note_max(*local, max);
            }
        }
        AnfOp::ARecord { fields, .. } => {
            for (_, atom) in fields {
                max_local_in_atom(atom, max);
            }
        }
        AnfOp::ARecordGet { target, .. } => max_local_in_atom(target, max),
        AnfOp::ARecordUpdate { base, value, .. } => {
            max_local_in_atom(base, max);
            max_local_in_atom(value, max);
        }
        AnfOp::AVariant { args, .. } | AnfOp::AArrayLit(args) => {
            for atom in args {
                max_local_in_atom(atom, max);
            }
        }
        AnfOp::AIndex { base, index, .. } => {
            max_local_in_atom(base, max);
            max_local_in_atom(index, max);
        }
        AnfOp::AInit { value } => max_local_in_atom(value, max),
        AnfOp::AAssign { local, value } => {
            note_max(*local, max);
            max_local_in_atom(value, max);
        }
    }
}

fn max_local_in_pattern(pattern: &CorePattern, max: &mut u32) {
    match pattern {
        CorePattern::Var(local) => note_max(*local, max),
        CorePattern::Variant { fields, .. } => {
            for field in fields {
                max_local_in_pattern(field, max);
            }
        }
        CorePattern::Wildcard
        | CorePattern::LitInt(_)
        | CorePattern::LitBool(_)
        | CorePattern::LitStr(_) => {}
    }
}

fn max_local_in_atom(atom: &Atom, max: &mut u32) {
    if let Atom::ALocal(local) = atom {
        note_max(*local, max);
    }
}

// ── Main rewrite pass ────────────────────────────────────────────────────────

/// Run uniqueness-based in-place update optimization on a single function.
///
/// Phase 1-2 + incremental extensions:
/// - Rewrite `VECTOR_SET_UNSAFE` -> `VECTOR_SET_IN_PLACE` when base is unique + consumed
/// - Rewrite `DICT_SET`/`DICT_REMOVE` to uniqueness-safe in-place helpers
/// - Annotate `ARecordUpdate` with `can_reuse_in_place=true` when base is unique + consumed
/// - Preserve uniqueness across known COW updates (`VECTOR_APPEND`)
pub fn uniqueness_rewrite(func: &mut AnfFunctionDef) {
    let tainted = collect_tainted(func);
    let mut unique = HashSet::new();
    let mut known_empty = HashSet::new();
    let mut next_local = next_local_id(func);
    rewrite_expr(
        &mut func.body,
        &tainted,
        &mut unique,
        &mut known_empty,
        &mut next_local,
    );
}

fn rewrite_expr(
    expr: &mut AnfExpr,
    tainted: &HashSet<LocalId>,
    unique: &mut HashSet<LocalId>,
    known_empty: &mut HashSet<LocalId>,
    next_local: &mut u32,
) {
    let AnfExpr::Let { local, op, body } = expr else {
        return;
    };
    let bind_local = *local;

    // Region rewrite (Phase 3, conservative):
    // Loop accumulator `xs = xs.append(v)` -> builder_new/push/freeze wrapping
    // when `xs` is unique, non-escaped, and known-empty at loop entry.
    if let AnfOp::ALoop { body: loop_body } = op.as_ref() {
        let mut candidates = unique
            .iter()
            .copied()
            .filter(|id| !tainted.contains(id))
            .collect::<Vec<_>>();
        candidates.sort_by_key(|id| id.0);

        for base in candidates {
            let Some(expected_sites) = analyze_loop_expr(loop_body, base) else {
                continue;
            };
            if expected_sites == 0 {
                continue;
            }

            let builder_local = alloc_local(next_local);
            let freeze_local = alloc_local(next_local);
            let assign_local = alloc_local(next_local);
            let use_builder_new = known_empty.contains(&base);

            let mut rewritten_loop_body = (*loop_body.as_ref()).clone();
            let mut rewritten_sites = 0usize;
            rewrite_loop_expr(
                &mut rewritten_loop_body,
                base,
                builder_local,
                &mut rewritten_sites,
            );
            if rewritten_sites == 0 || rewritten_sites != expected_sites {
                continue;
            }

            let old_cont = (*body.as_ref()).clone();
            *expr = AnfExpr::Let {
                local: builder_local,
                op: Box::new(AnfOp::ACall {
                    callee: Atom::AGlobalFunc(if use_builder_new {
                        prelude::VECTOR_BUILDER_NEW
                    } else {
                        prelude::VECTOR_BUILDER_FROM
                    }),
                    args: if use_builder_new {
                        vec![]
                    } else {
                        vec![Atom::ALocal(base)]
                    },
                }),
                body: Box::new(AnfExpr::Let {
                    local: bind_local,
                    op: Box::new(AnfOp::ALoop {
                        body: Box::new(rewritten_loop_body),
                    }),
                    body: Box::new(AnfExpr::Let {
                        local: freeze_local,
                        op: Box::new(AnfOp::ACall {
                            callee: Atom::AGlobalFunc(prelude::VECTOR_BUILDER_FREEZE),
                            args: vec![Atom::ALocal(builder_local)],
                        }),
                        body: Box::new(AnfExpr::Let {
                            local: assign_local,
                            op: Box::new(AnfOp::AAssign {
                                local: base,
                                value: Atom::ALocal(freeze_local),
                            }),
                            body: Box::new(old_cont),
                        }),
                    }),
                }),
            };
            rewrite_expr(expr, tainted, unique, known_empty, next_local);
            return;
        }

        // Dict in-place loop rewrite: for COW ops with a direct in-place swap
        // (no builder lifecycle needed), just swap the callee inside the loop.
        for base in unique
            .iter()
            .copied()
            .filter(|id| !tainted.contains(id))
            .collect::<Vec<_>>()
        {
            let Some(expected_sites) = analyze_loop_dict_sites(loop_body, base) else {
                continue;
            };
            if expected_sites == 0 {
                continue;
            }

            let mut rewritten_loop_body = (*loop_body.as_ref()).clone();
            let mut rewritten_sites = 0usize;
            rewrite_loop_dict_expr(&mut rewritten_loop_body, base, &mut rewritten_sites);
            if rewritten_sites == 0 || rewritten_sites != expected_sites {
                continue;
            }

            *op = Box::new(AnfOp::ALoop {
                body: Box::new(rewritten_loop_body),
            });
            rewrite_expr(expr, tainted, unique, known_empty, next_local);
            return;
        }
    }

    // Track fresh producers → Unique
    if is_fresh_producer(op) {
        unique.insert(bind_local);
    }
    if let AnfOp::AArrayLit(elems) = op.as_ref() {
        if elems.is_empty() {
            known_empty.insert(bind_local);
        }
    }

    // AInit uniqueness transfer: let x = init(y) where y is unique → x is unique
    // (y is moved to x; y should no longer be considered unique)
    if let AnfOp::AInit {
        value: Atom::ALocal(source),
    } = op.as_ref()
    {
        if unique.contains(source) && !tainted.contains(source) {
            // Transfer: source dies (moved), target becomes unique
            unique.remove(source);
            unique.insert(bind_local);
        }
        if known_empty.contains(source) && !tainted.contains(source) {
            known_empty.remove(source);
            known_empty.insert(bind_local);
        }
    }

    // AAssign always redefines target; old target uniqueness is killed.
    if let AnfOp::AAssign {
        local: target,
        value,
    } = op.as_ref()
    {
        unique.remove(target);
        known_empty.remove(target);
        if let Atom::ALocal(source) = value {
            if unique.contains(source) && !tainted.contains(source) {
                // Treat as move when source is not tainted (no surviving alias).
                unique.remove(source);
                unique.insert(*target);
            }
            if known_empty.contains(source) && !tainted.contains(source) {
                known_empty.remove(source);
                known_empty.insert(*target);
            }
        }
    }

    // Record update reuse annotation:
    // Treat as a point update on the base record when base is unique + consumed.
    if let AnfOp::ARecordUpdate {
        base: Atom::ALocal(base),
        can_reuse_in_place,
        ..
    } = op.as_mut()
    {
        // ARecordUpdate always produces a record value local to this binding.
        unique.insert(bind_local);
        let can_rewrite =
            is_consume_reassign(body, *base, bind_local) || !live_after(body).contains(base);
        if unique.contains(base) && !tainted.contains(base) && can_rewrite {
            *can_reuse_in_place = true;
        }
    }

    // Check for COW rewrite / uniqueness-propagation opportunity
    if let AnfOp::ACall {
        callee: Atom::AGlobalFunc(func_id),
        args,
    } = op.as_mut()
    {
        if let Some(info) = cow_op_info(*func_id) {
            if let Some(Atom::ALocal(base)) = args.get(info.base_arg) {
                let base = *base;
                if unique.contains(&base) && !tainted.contains(&base) {
                    let can_rewrite = is_consume_reassign(body, base, bind_local)
                        || !live_after(body).contains(&base);
                    if can_rewrite {
                        if let Some(in_place_id) = info.in_place_rewrite {
                            *func_id = in_place_id;
                        }
                        // Result inherits uniqueness from the consuming update.
                        unique.insert(bind_local);
                        // Any consuming update may change container cardinality/content.
                        known_empty.remove(&bind_local);
                    }
                }
            }
        }
    }

    // Recurse into op sub-expressions (branches, loops)
    rewrite_op(op, tainted, unique, known_empty, next_local);
    // Continue with body
    rewrite_expr(body, tainted, unique, known_empty, next_local);
}

fn rewrite_op(
    op: &mut AnfOp,
    tainted: &HashSet<LocalId>,
    unique: &mut HashSet<LocalId>,
    known_empty: &mut HashSet<LocalId>,
    next_local: &mut u32,
) {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            // Conservative: separate unique sets for each branch, don't propagate out
            let mut then_unique = unique.clone();
            let mut else_unique = unique.clone();
            let mut then_empty = known_empty.clone();
            let mut else_empty = known_empty.clone();
            rewrite_expr(
                then_branch,
                tainted,
                &mut then_unique,
                &mut then_empty,
                next_local,
            );
            rewrite_expr(
                else_branch,
                tainted,
                &mut else_unique,
                &mut else_empty,
                next_local,
            );
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                let mut arm_unique = unique.clone();
                let mut arm_empty = known_empty.clone();
                rewrite_expr(
                    &mut arm.body,
                    tainted,
                    &mut arm_unique,
                    &mut arm_empty,
                    next_local,
                );
            }
        }
        AnfOp::ALoop { body } => {
            // Conservative: don't propagate unique into loops
            let mut loop_unique = HashSet::new();
            let mut loop_empty = HashSet::new();
            rewrite_expr(body, tainted, &mut loop_unique, &mut loop_empty, next_local);
        }
        _ => {}
    }
}
