use std::collections::HashSet;

use crate::ir::anf::{Atom, AnfExpr, AnfFunctionDef, AnfMatchArm, AnfOp};
use crate::ir::core::LocalId;

/// Compute the set of locals that are *live* (may be read) at the entry of
/// `body`. This is a backward analysis:
///
/// - `Atom(ALocal(t))` → `{t}` is live here.
/// - `Let(t, op, body)` → `(live(body) \ {t}) ∪ locals_read_in(op)`.
///   `t` is killed by its own binding; everything op reads must be live before it.
pub fn live_after(body: &AnfExpr) -> HashSet<LocalId> {
    let mut set = HashSet::new();
    compute_live(body, &mut set);
    set
}

fn add_atom(a: &Atom, set: &mut HashSet<LocalId>) {
    if let Atom::ALocal(id) = a {
        set.insert(*id);
    }
}

fn compute_live(expr: &AnfExpr, live: &mut HashSet<LocalId>) {
    match expr {
        AnfExpr::Let { local, op, body } => {
            // Start from what's live after the body.
            compute_live(body, live);
            // Kill the binding.
            live.remove(local);
            // Add everything the op reads.
            live_in_op(op, live);
        }
        AnfExpr::Return(Some(a)) => {
            add_atom(a, live);
        }
        AnfExpr::Return(None) => {}
        AnfExpr::Break(Some(a)) => {
            add_atom(a, live);
        }
        AnfExpr::Break(None) => {}
        AnfExpr::Continue => {}
        AnfExpr::Atom(a) => {
            add_atom(a, live);
        }
    }
}

fn live_in_op(op: &AnfOp, live: &mut HashSet<LocalId>) {
    match op {
        AnfOp::ACall { callee, args } => {
            add_atom(callee, live);
            for a in args {
                add_atom(a, live);
            }
        }
        AnfOp::AIf { cond, then_branch, else_branch } => {
            add_atom(cond, live);
            // Both branches may execute; union their live sets conservatively.
            let mut then_live = HashSet::new();
            compute_live(then_branch, &mut then_live);
            let mut else_live = HashSet::new();
            compute_live(else_branch, &mut else_live);
            live.extend(then_live);
            live.extend(else_live);
        }
        AnfOp::AMatch { scrutinee, arms } => {
            add_atom(scrutinee, live);
            for AnfMatchArm { body, .. } in arms {
                let mut arm_live = HashSet::new();
                compute_live(body, &mut arm_live);
                live.extend(arm_live);
            }
        }
        AnfOp::ALoop { body } => {
            // Conservative: treat all locals read anywhere in the loop as live.
            let mut loop_live = HashSet::new();
            compute_live(body, &mut loop_live);
            live.extend(loop_live);
        }
        AnfOp::ABinOp { left, right, .. } => {
            add_atom(left, live);
            add_atom(right, live);
        }
        AnfOp::AUnOp { expr, .. } => {
            add_atom(expr, live);
        }
        AnfOp::AMakeClosure { free_vars, .. } => {
            for v in free_vars {
                live.insert(*v);
            }
        }
        AnfOp::ARecord { fields, .. } => {
            for (_, a) in fields {
                add_atom(a, live);
            }
        }
        AnfOp::ARecordGet { target, .. } => {
            add_atom(target, live);
        }
        AnfOp::ARecordUpdate { base, value, .. } => {
            add_atom(base, live);
            add_atom(value, live);
        }
        AnfOp::AVariant { args, .. } => {
            for a in args {
                add_atom(a, live);
            }
        }
        AnfOp::AArrayLit(elems) => {
            for a in elems {
                add_atom(a, live);
            }
        }
        AnfOp::AIndex { base, index } => {
            add_atom(base, live);
            add_atom(index, live);
        }
        AnfOp::AInit { value } => {
            add_atom(value, live);
        }
        AnfOp::AAssign { value, .. } => {
            add_atom(value, live);
        }
    }
}

/// Walk the function body and set `can_reuse_in_place = true` on any
/// `ARecordUpdate { base: ALocal(r), .. }` where `r` is provably dead in the
/// continuation (not live after the update point).
///
/// Proof of safety:
/// - `r` is dead in the body after the update, so no subsequent code can
///   observe the pre-update value through `r`.
/// - Evaluation order is unchanged (the update is still evaluated).
/// - Trap behavior is unchanged.
///
/// The WAT backend reads this flag to decide whether to emit `struct.set`
/// (in-place) instead of allocating a new struct.
pub fn annotate_in_place(func: &mut AnfFunctionDef) {
    annotate_expr(&mut func.body);
}

fn annotate_expr(expr: &mut AnfExpr) {
    match expr {
        AnfExpr::Let { local: _, op, body } => {
            // Check if op is ARecordUpdate with an ALocal base.
            if let AnfOp::ARecordUpdate { base: Atom::ALocal(r), can_reuse_in_place, .. } = op.as_mut() {
                let live = live_after(body);
                if !live.contains(r) {
                    *can_reuse_in_place = true;
                }
            }
            // Recurse into sub-expressions of op, then into body.
            annotate_op(op);
            annotate_expr(body);
        }
        _ => {}
    }
}

fn annotate_op(op: &mut AnfOp) {
    match op {
        AnfOp::AIf { then_branch, else_branch, .. } => {
            annotate_expr(then_branch);
            annotate_expr(else_branch);
        }
        AnfOp::AMatch { arms, .. } => {
            for AnfMatchArm { body, .. } in arms {
                annotate_expr(body);
            }
        }
        AnfOp::ALoop { body } => {
            annotate_expr(body);
        }
        _ => {}
    }
}
