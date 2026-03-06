use std::collections::HashSet;

use crate::ir::anf::{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfOp, Atom};
use crate::ir::core::{FuncId, LocalId};
use crate::ir::lower::prelude;
use crate::opt::liveness::live_after;

// ── Known COW point-update operations ────────────────────────────────────────

struct PointRewriteInfo {
    in_place_id: FuncId,
    base_arg: usize,
}

fn point_rewrite_info(func_id: FuncId) -> Option<PointRewriteInfo> {
    if func_id == prelude::VECTOR_SET_UNSAFE {
        Some(PointRewriteInfo {
            in_place_id: prelude::VECTOR_SET_IN_PLACE,
            base_arg: 0,
        })
    } else {
        None
    }
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
    scan_tainted_expr(&func.body, &mut tainted);
    tainted
}

fn scan_tainted_expr(expr: &AnfExpr, tainted: &mut HashSet<LocalId>) {
    match expr {
        AnfExpr::Let { op, body, .. } => {
            scan_tainted_op(op, tainted);
            scan_tainted_expr(body, tainted);
        }
        _ => {}
    }
}

fn scan_tainted_op(op: &AnfOp, tainted: &mut HashSet<LocalId>) {
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
            if let Some(info) = point_rewrite_info(*func_id) {
                // COW op: only taint non-base args
                for (i, a) in args.iter().enumerate() {
                    if i != info.base_arg {
                        if let Atom::ALocal(x) = a {
                            tainted.insert(*x);
                        }
                    }
                }
            } else {
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
            scan_tainted_expr(then_branch, tainted);
            scan_tainted_expr(else_branch, tainted);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                scan_tainted_expr(&arm.body, tainted);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            scan_tainted_expr(body, tainted);
        }
        _ => {}
    }
}

// ── Consume-reassign pattern detection ───────────────────────────────────────

/// Checks if `body` starts with `Let { op: AAssign { local: base, value: result } }`.
fn is_consume_reassign(body: &AnfExpr, base: LocalId, result: LocalId) -> bool {
    if let AnfExpr::Let { op, .. } = body {
        if let AnfOp::AAssign { local, value: Atom::ALocal(v) } = op.as_ref() {
            return *local == base && *v == result;
        }
    }
    false
}

// ── Main rewrite pass ────────────────────────────────────────────────────────

/// Run uniqueness-based in-place update optimization on a single function.
///
/// Phase 1-2: Detects fresh (Unique) arrays where the only use is a known
/// COW point-update operation (VECTOR_SET_UNSAFE), and rewrites the call
/// to VECTOR_SET_IN_PLACE when the base is consumed (last use).
pub fn uniqueness_rewrite(func: &mut AnfFunctionDef) {
    let tainted = collect_tainted(func);
    let mut unique = HashSet::new();
    rewrite_expr(&mut func.body, &tainted, &mut unique);
}

fn rewrite_expr(
    expr: &mut AnfExpr,
    tainted: &HashSet<LocalId>,
    unique: &mut HashSet<LocalId>,
) {
    let AnfExpr::Let { local, op, body } = expr else {
        return;
    };
    let bind_local = *local;

    // Track fresh producers → Unique
    if is_fresh_producer(op) {
        unique.insert(bind_local);
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
    }

    // AAssign uniqueness transfer: assign(target = source) where source is unique
    if let AnfOp::AAssign {
        local: target,
        value: Atom::ALocal(source),
    } = op.as_ref()
    {
        if unique.contains(source) {
            unique.insert(*target);
        }
    }

    // Check for COW point-rewrite opportunity
    if let AnfOp::ACall {
        callee: Atom::AGlobalFunc(func_id),
        args,
    } = op.as_mut()
    {
        if let Some(info) = point_rewrite_info(*func_id) {
            if let Some(Atom::ALocal(base)) = args.get(info.base_arg) {
                let base = *base;
                if unique.contains(&base) && !tainted.contains(&base) {
                    let can_rewrite = is_consume_reassign(body, base, bind_local)
                        || !live_after(body).contains(&base);
                    if can_rewrite {
                        *func_id = info.in_place_id;
                        // Result inherits uniqueness (same object, mutated in place)
                        unique.insert(bind_local);
                    }
                }
            }
        }
    }

    // Recurse into op sub-expressions (branches, loops)
    rewrite_op(op, tainted, unique);
    // Continue with body
    rewrite_expr(body, tainted, unique);
}

fn rewrite_op(
    op: &mut AnfOp,
    tainted: &HashSet<LocalId>,
    unique: &mut HashSet<LocalId>,
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
            rewrite_expr(then_branch, tainted, &mut then_unique);
            rewrite_expr(else_branch, tainted, &mut else_unique);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                let mut arm_unique = unique.clone();
                rewrite_expr(&mut arm.body, tainted, &mut arm_unique);
            }
        }
        AnfOp::ALoop { body } => {
            // Conservative: don't propagate unique into loops
            let mut loop_unique = HashSet::new();
            rewrite_expr(body, tainted, &mut loop_unique);
        }
        _ => {}
    }
}
