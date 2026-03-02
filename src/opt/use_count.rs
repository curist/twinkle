use std::collections::HashMap;

use crate::ir::anf::{Atom, AnfExpr, AnfMatchArm, AnfOp};
use crate::ir::core::LocalId;

/// Count how many times each `LocalId` is *used* (read) in the expression.
///
/// A "use" is any `ALocal(id)` appearing in an operand/atom position.
/// The following are NOT counted as uses:
/// - `Let.local` — the binding site (a definition, not a read)
/// - `AAssign.local` — a write target (not a read)
pub fn count_uses(body: &AnfExpr) -> HashMap<LocalId, usize> {
    let mut map = HashMap::new();
    count_expr(body, &mut map);
    map
}

fn count_atom(atom: &Atom, map: &mut HashMap<LocalId, usize>) {
    if let Atom::ALocal(id) = atom {
        *map.entry(*id).or_insert(0) += 1;
    }
}

fn count_op(op: &AnfOp, map: &mut HashMap<LocalId, usize>) {
    match op {
        AnfOp::ACall { callee, args } => {
            count_atom(callee, map);
            for a in args {
                count_atom(a, map);
            }
        }
        AnfOp::AIf { cond, then_branch, else_branch } => {
            count_atom(cond, map);
            count_expr(then_branch, map);
            count_expr(else_branch, map);
        }
        AnfOp::AMatch { scrutinee, arms } => {
            count_atom(scrutinee, map);
            for AnfMatchArm { body, .. } in arms {
                count_expr(body, map);
            }
        }
        AnfOp::ALoop { body } => {
            count_expr(body, map);
        }
        AnfOp::ABinOp { left, right, .. } => {
            count_atom(left, map);
            count_atom(right, map);
        }
        AnfOp::AUnOp { expr, .. } => {
            count_atom(expr, map);
        }
        AnfOp::AMakeClosure { free_vars, .. } => {
            for v in free_vars {
                *map.entry(*v).or_insert(0) += 1;
            }
        }
        AnfOp::ARecord { fields, .. } => {
            for (_, a) in fields {
                count_atom(a, map);
            }
        }
        AnfOp::ARecordGet { target, .. } => {
            count_atom(target, map);
        }
        AnfOp::ARecordUpdate { base, value, .. } => {
            count_atom(base, map);
            count_atom(value, map);
        }
        AnfOp::AVariant { args, .. } => {
            for a in args {
                count_atom(a, map);
            }
        }
        AnfOp::AArrayLit(elems) => {
            for a in elems {
                count_atom(a, map);
            }
        }
        AnfOp::AIndex { base, index } => {
            count_atom(base, map);
            count_atom(index, map);
        }
        AnfOp::AInit { value } => {
            count_atom(value, map);
        }
        // AAssign.local is a write target — NOT a use. Only count the value.
        AnfOp::AAssign { value, .. } => {
            count_atom(value, map);
        }
        AnfOp::ADefer(inner) => {
            count_expr(inner, map);
        }
    }
}

fn count_expr(expr: &AnfExpr, map: &mut HashMap<LocalId, usize>) {
    match expr {
        // Let.local is the binder — NOT a use. Count uses inside op and body.
        AnfExpr::Let { op, body, .. } => {
            count_op(op, map);
            count_expr(body, map);
        }
        AnfExpr::Return(Some(a)) => count_atom(a, map),
        AnfExpr::Return(None) => {}
        AnfExpr::Break(Some(a)) => count_atom(a, map),
        AnfExpr::Break(None) => {}
        AnfExpr::Continue => {}
        AnfExpr::Atom(a) => count_atom(a, map),
    }
}

/// Returns true if `op` has no observable side effects (cannot trap, cannot
/// perform I/O, cannot mutate state visible outside the expression).
///
/// Pure ops with zero uses can be safely eliminated by dead-let elimination.
pub fn is_pure(op: &AnfOp) -> bool {
    matches!(
        op,
        AnfOp::AInit { .. }
            | AnfOp::ABinOp { .. }
            | AnfOp::AUnOp { .. }
            | AnfOp::ARecord { .. }
            | AnfOp::ARecordGet { .. }
            | AnfOp::ARecordUpdate { .. }
            | AnfOp::AVariant { .. }
            | AnfOp::AArrayLit(_)
            | AnfOp::AMakeClosure { .. }
    )
    // ACall: may I/O or trap — impure.
    // AAssign: mutates state — impure.
    // AIndex: may trap on out-of-bounds — impure.
    // AIf / AMatch / ALoop: contain arbitrary sub-expressions — conservative.
}

/// Count uses of each `LocalId` in *atom* positions only — identical to
/// `count_uses` but excluding appearances in `AMakeClosure.free_vars`.
///
/// Used by `copy_propagate` to determine whether it is safe to substitute
/// a non-local atom for a local: if a local's only use is as a closure
/// free_var, substitution would orphan the free_var reference (because
/// `AMakeClosure.free_vars` holds `LocalId`s, not `Atom`s, and cannot
/// receive a literal). By excluding free_var appearances, `copy_propagate`
/// conservatively keeps the binding alive in that case.
pub fn count_uses_excluding_free_vars(body: &AnfExpr) -> HashMap<LocalId, usize> {
    let mut map = HashMap::new();
    count_expr_ex(body, &mut map);
    map
}

fn count_op_ex(op: &AnfOp, map: &mut HashMap<LocalId, usize>) {
    match op {
        // AMakeClosure: do NOT count free_vars — they are LocalIds in a
        // closure-capture position and cannot receive literal substitution.
        AnfOp::AMakeClosure { .. } => {}
        // All other ops: delegate to the standard count_op.
        other => count_op(other, map),
    }
}

fn count_expr_ex(expr: &AnfExpr, map: &mut HashMap<LocalId, usize>) {
    match expr {
        AnfExpr::Let { op, body, .. } => {
            count_op_ex(op, map);
            count_expr_ex(body, map);
        }
        AnfExpr::Return(Some(a)) => count_atom(a, map),
        AnfExpr::Return(None) => {}
        AnfExpr::Break(Some(a)) => count_atom(a, map),
        AnfExpr::Break(None) => {}
        AnfExpr::Continue => {}
        AnfExpr::Atom(a) => count_atom(a, map),
    }
}

/// Collect all `LocalId`s referenced as *operands* in `op`'s atom fields.
/// Used by the liveness analysis.
pub fn locals_in_op(op: &AnfOp) -> Vec<LocalId> {
    let mut out = Vec::new();
    let mut map = HashMap::new();
    count_op(op, &mut map);
    for (id, _) in map {
        out.push(id);
    }
    out
}
