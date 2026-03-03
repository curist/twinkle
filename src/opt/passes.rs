use std::collections::{HashMap, HashSet};

use crate::ir::anf::{AnfExpr, AnfMatchArm, AnfOp, Atom};
use crate::ir::core::LocalId;
use crate::opt::use_count::{collect_assigned_locals, count_uses_excluding_free_vars, is_pure};
use crate::syntax::ast::{BinOp, UnOp};

// ── Substitution helper ───────────────────────────────────────────────────────

/// Substitute all `ALocal(target)` occurrences in `body` with `replacement`.
///
/// Only called with non-local atoms (literals or `AGlobalFunc`), so there is
/// no risk of capturing or skipping a mutation of `replacement` between the
/// definition and the use.
///
/// ANF invariant: LocalIds are unique within a function — no two `Let` nodes
/// bind the same `LocalId`. If that invariant holds, the shadow-stop guard
/// below (`local == target`) can never trigger in valid input; it is included
/// as a safety net against any future violation of that invariant.
pub fn subst_atom(body: AnfExpr, target: LocalId, replacement: &Atom) -> AnfExpr {
    match body {
        AnfExpr::Let { local, op, body } => {
            let new_op = Box::new(subst_op(*op, target, replacement));
            // If this Let rebinds `target`, do not substitute further into
            // its body (shadow-stop guard for ANF unique-LocalId safety).
            let new_body = if local == target {
                body
            } else {
                Box::new(subst_atom(*body, target, replacement))
            };
            AnfExpr::Let {
                local,
                op: new_op,
                body: new_body,
            }
        }
        AnfExpr::Return(Some(a)) => AnfExpr::Return(Some(subst_atom_val(a, target, replacement))),
        AnfExpr::Return(None) => AnfExpr::Return(None),
        AnfExpr::Break(Some(a)) => AnfExpr::Break(Some(subst_atom_val(a, target, replacement))),
        AnfExpr::Break(None) => AnfExpr::Break(None),
        AnfExpr::Continue => AnfExpr::Continue,
        AnfExpr::Atom(a) => AnfExpr::Atom(subst_atom_val(a, target, replacement)),
    }
}

fn subst_atom_val(a: Atom, target: LocalId, replacement: &Atom) -> Atom {
    match &a {
        Atom::ALocal(id) if *id == target => replacement.clone(),
        _ => a,
    }
}

fn subst_op(op: AnfOp, target: LocalId, replacement: &Atom) -> AnfOp {
    let sa = |a: Atom| subst_atom_val(a, target, replacement);
    match op {
        AnfOp::ACall { callee, args } => AnfOp::ACall {
            callee: sa(callee),
            args: args.into_iter().map(sa).collect(),
        },
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => AnfOp::AIf {
            cond: sa(cond),
            then_branch: Box::new(subst_atom(*then_branch, target, replacement)),
            else_branch: Box::new(subst_atom(*else_branch, target, replacement)),
        },
        AnfOp::AMatch { scrutinee, arms } => AnfOp::AMatch {
            scrutinee: sa(scrutinee),
            arms: arms
                .into_iter()
                .map(|AnfMatchArm { pattern, body }| AnfMatchArm {
                    pattern,
                    body: subst_atom(body, target, replacement),
                })
                .collect(),
        },
        AnfOp::ALoop { body } => AnfOp::ALoop {
            body: Box::new(subst_atom(*body, target, replacement)),
        },
        AnfOp::ABinOp {
            op,
            left,
            right,
            operand_ty,
        } => AnfOp::ABinOp {
            op,
            left: sa(left),
            right: sa(right),
            operand_ty,
        },
        AnfOp::AUnOp {
            op,
            expr,
            operand_ty,
        } => AnfOp::AUnOp {
            op,
            expr: sa(expr),
            operand_ty,
        },
        AnfOp::AMakeClosure { func_id, free_vars } => {
            // free_vars are Vec<LocalId>, not Vec<Atom>. We cannot substitute
            // a literal atom into a free_var slot (Wasm closure capture requires
            // a local, not an immediate). We leave free_vars unchanged.
            //
            // copy_propagate guards against this: it calls `uses_excluding_free_vars`
            // and only propagates when the free-var-excluded count <= 1, ensuring
            // that a local whose sole use is as a free_var is never propagated
            // (which would drop the binding Let and orphan the closure).
            AnfOp::AMakeClosure { func_id, free_vars }
        }
        AnfOp::ARecord { type_id, fields } => AnfOp::ARecord {
            type_id,
            fields: fields.into_iter().map(|(fid, a)| (fid, sa(a))).collect(),
        },
        AnfOp::ARecordGet {
            target: t,
            field,
            type_id,
        } => AnfOp::ARecordGet {
            target: sa(t),
            field,
            type_id,
        },
        AnfOp::ARecordUpdate {
            base,
            field,
            value,
            can_reuse_in_place,
            type_id,
        } => AnfOp::ARecordUpdate {
            base: sa(base),
            field,
            value: sa(value),
            can_reuse_in_place,
            type_id,
        },
        AnfOp::AVariant {
            type_id,
            variant,
            args,
        } => AnfOp::AVariant {
            type_id,
            variant,
            args: args.into_iter().map(sa).collect(),
        },
        AnfOp::AArrayLit(elems) => AnfOp::AArrayLit(elems.into_iter().map(sa).collect()),
        AnfOp::AIndex {
            base,
            index,
            base_ty,
            result_ty,
        } => AnfOp::AIndex {
            base: sa(base),
            index: sa(index),
            base_ty,
            result_ty,
        },
        AnfOp::AInit { value } => AnfOp::AInit { value: sa(value) },
        AnfOp::AAssign { local, value } => AnfOp::AAssign {
            local,
            value: sa(value),
        },
        AnfOp::ADefer(inner) => AnfOp::ADefer(Box::new(subst_atom(*inner, target, replacement))),
    }
}

// ── Dead let elimination ──────────────────────────────────────────────────────

/// Eliminate `Let(t, pure_op, body)` bindings where `t` is never used.
///
/// Returns `(new_expr, changed)`. Call repeatedly until `changed` is false.
pub fn dead_let_elim(
    body: AnfExpr,
    uses: &HashMap<LocalId, usize>,
    assigned: &HashSet<LocalId>,
) -> (AnfExpr, bool) {
    match body {
        AnfExpr::Let { local, op, body } => {
            // Check if this binding is dead and the op is pure.
            let use_count = uses.get(&local).copied().unwrap_or(0);
            if use_count == 0 && !assigned.contains(&local) && is_pure(&op) {
                // Drop the binding; recurse into body.
                let (new_body, _) = dead_let_elim(*body, uses, assigned);
                return (new_body, true);
            }
            // Recurse into op's sub-expressions and body.
            let (new_op, op_changed) = dead_let_elim_op(*op, uses, assigned);
            let (new_body, body_changed) = dead_let_elim(*body, uses, assigned);
            (
                AnfExpr::Let {
                    local,
                    op: Box::new(new_op),
                    body: Box::new(new_body),
                },
                op_changed || body_changed,
            )
        }
        other => (other, false),
    }
}

fn dead_let_elim_op(
    op: AnfOp,
    uses: &HashMap<LocalId, usize>,
    assigned: &HashSet<LocalId>,
) -> (AnfOp, bool) {
    match op {
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => {
            let (new_then, c1) = dead_let_elim(*then_branch, uses, assigned);
            let (new_else, c2) = dead_let_elim(*else_branch, uses, assigned);
            (
                AnfOp::AIf {
                    cond,
                    then_branch: Box::new(new_then),
                    else_branch: Box::new(new_else),
                },
                c1 || c2,
            )
        }
        AnfOp::AMatch { scrutinee, arms } => {
            let mut changed = false;
            let arms = arms
                .into_iter()
                .map(|AnfMatchArm { pattern, body }| {
                    let (new_body, c) = dead_let_elim(body, uses, assigned);
                    changed |= c;
                    AnfMatchArm {
                        pattern,
                        body: new_body,
                    }
                })
                .collect();
            (AnfOp::AMatch { scrutinee, arms }, changed)
        }
        AnfOp::ALoop { body } => {
            let (new_body, changed) = dead_let_elim(*body, uses, assigned);
            (
                AnfOp::ALoop {
                    body: Box::new(new_body),
                },
                changed,
            )
        }
        AnfOp::ADefer(inner) => {
            let (new_inner, changed) = dead_let_elim(*inner, uses, assigned);
            (AnfOp::ADefer(Box::new(new_inner)), changed)
        }
        other => (other, false),
    }
}

// ── Literal copy propagation ──────────────────────────────────────────────────

/// Inline `Let(t, AInit(lit), body)` where `lit` is a non-local atom and
/// `t` is used at most once *excluding closure free_var positions*.
///
/// The free_var exclusion is critical: `AMakeClosure.free_vars` holds
/// `LocalId`s (not `Atom`s) and cannot receive literal substitution.
/// If a local's only use is as a free_var, propagating it would drop
/// the defining `Let` while leaving the free_var referencing an unbound
/// local — producing invalid ANF. By using `count_uses_excluding_free_vars`,
/// such locals are conservatively kept alive.
///
/// Returns `(new_expr, changed)`.
pub fn copy_propagate(body: AnfExpr) -> (AnfExpr, bool) {
    // Recompute use counts excluding free_var positions for safety.
    let uses = count_uses_excluding_free_vars(&body);
    let assigned = collect_assigned_locals(&body);
    copy_propagate_inner(body, &uses, &assigned)
}

fn copy_propagate_inner(
    body: AnfExpr,
    uses: &HashMap<LocalId, usize>,
    assigned: &HashSet<LocalId>,
) -> (AnfExpr, bool) {
    match body {
        AnfExpr::Let { local, op, body } => {
            if let AnfOp::AInit { value: ref lit } = *op {
                if is_non_local_atom(lit) {
                    let use_count = uses.get(&local).copied().unwrap_or(0);
                    if use_count <= 1 && !assigned.contains(&local) {
                        let new_body = subst_atom(*body, local, lit);
                        return (new_body, true);
                    }
                }
            }
            // Recurse into sub-expressions.
            let (new_op, op_changed) = copy_propagate_op(*op, uses, assigned);
            let (new_body, body_changed) = copy_propagate_inner(*body, uses, assigned);
            (
                AnfExpr::Let {
                    local,
                    op: Box::new(new_op),
                    body: Box::new(new_body),
                },
                op_changed || body_changed,
            )
        }
        other => (other, false),
    }
}

/// A "non-local atom" is any literal or global function reference — values
/// that cannot be mutated between definition and use.
fn is_non_local_atom(a: &Atom) -> bool {
    !matches!(a, Atom::ALocal(_))
}

fn copy_propagate_op(
    op: AnfOp,
    uses: &HashMap<LocalId, usize>,
    assigned: &HashSet<LocalId>,
) -> (AnfOp, bool) {
    match op {
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => {
            let (new_then, c1) = copy_propagate_inner(*then_branch, uses, assigned);
            let (new_else, c2) = copy_propagate_inner(*else_branch, uses, assigned);
            (
                AnfOp::AIf {
                    cond,
                    then_branch: Box::new(new_then),
                    else_branch: Box::new(new_else),
                },
                c1 || c2,
            )
        }
        AnfOp::AMatch { scrutinee, arms } => {
            let mut changed = false;
            let arms = arms
                .into_iter()
                .map(|AnfMatchArm { pattern, body }| {
                    let (new_body, c) = copy_propagate_inner(body, uses, assigned);
                    changed |= c;
                    AnfMatchArm {
                        pattern,
                        body: new_body,
                    }
                })
                .collect();
            (AnfOp::AMatch { scrutinee, arms }, changed)
        }
        AnfOp::ALoop { body } => {
            let (new_body, changed) = copy_propagate_inner(*body, uses, assigned);
            (
                AnfOp::ALoop {
                    body: Box::new(new_body),
                },
                changed,
            )
        }
        AnfOp::ADefer(inner) => {
            let (new_inner, changed) = copy_propagate_inner(*inner, uses, assigned);
            (AnfOp::ADefer(Box::new(new_inner)), changed)
        }
        other => (other, false),
    }
}

// ── Constant folding ──────────────────────────────────────────────────────────

/// Fold `Let(t, ABinOp/AUnOp with literal operands, body)` into
/// `Let(t, AInit(result), body)`. Copy propagation will then eliminate `t`.
///
/// Division/modulo by zero literals are left as-is (runtime trap is intended).
///
/// Returns `(new_expr, changed)`.
pub fn constant_fold(body: AnfExpr) -> (AnfExpr, bool) {
    match body {
        AnfExpr::Let { local, op, body } => {
            if let Some(folded) = try_fold_op(&op) {
                let new_op = AnfOp::AInit { value: folded };
                let (new_body, _) = constant_fold(*body);
                return (
                    AnfExpr::Let {
                        local,
                        op: Box::new(new_op),
                        body: Box::new(new_body),
                    },
                    true,
                );
            }
            let (new_op, op_changed) = constant_fold_op(*op);
            let (new_body, body_changed) = constant_fold(*body);
            (
                AnfExpr::Let {
                    local,
                    op: Box::new(new_op),
                    body: Box::new(new_body),
                },
                op_changed || body_changed,
            )
        }
        other => (other, false),
    }
}

fn try_fold_op(op: &AnfOp) -> Option<Atom> {
    match op {
        AnfOp::ABinOp {
            op, left, right, ..
        } => fold_binop(*op, left, right),
        AnfOp::AUnOp { op, expr, .. } => fold_unop(*op, expr),
        _ => None,
    }
}

fn fold_binop(op: BinOp, left: &Atom, right: &Atom) -> Option<Atom> {
    match (left, right) {
        (Atom::ALitInt(a), Atom::ALitInt(b)) => {
            let result = match op {
                BinOp::Add => a.wrapping_add(*b),
                BinOp::Sub => a.wrapping_sub(*b),
                BinOp::Mul => a.wrapping_mul(*b),
                // Leave div/mod by zero for runtime trap.
                BinOp::Div if *b == 0 => return None,
                BinOp::Div => *a / *b,
                BinOp::Mod if *b == 0 => return None,
                BinOp::Mod => *a % *b,
                BinOp::Eq => return Some(Atom::ALitBool(*a == *b)),
                BinOp::Ne => return Some(Atom::ALitBool(*a != *b)),
                BinOp::Lt => return Some(Atom::ALitBool(*a < *b)),
                BinOp::Le => return Some(Atom::ALitBool(*a <= *b)),
                BinOp::Gt => return Some(Atom::ALitBool(*a > *b)),
                BinOp::Ge => return Some(Atom::ALitBool(*a >= *b)),
                _ => return None,
            };
            Some(Atom::ALitInt(result))
        }
        (Atom::ALitFloat(a), Atom::ALitFloat(b)) => {
            let result = match op {
                BinOp::Add => *a + *b,
                BinOp::Sub => *a - *b,
                BinOp::Mul => *a * *b,
                BinOp::Div => *a / *b,
                BinOp::Eq => return Some(Atom::ALitBool(*a == *b)),
                BinOp::Ne => return Some(Atom::ALitBool(*a != *b)),
                BinOp::Lt => return Some(Atom::ALitBool(*a < *b)),
                BinOp::Le => return Some(Atom::ALitBool(*a <= *b)),
                BinOp::Gt => return Some(Atom::ALitBool(*a > *b)),
                BinOp::Ge => return Some(Atom::ALitBool(*a >= *b)),
                _ => return None,
            };
            Some(Atom::ALitFloat(result))
        }
        (Atom::ALitBool(a), Atom::ALitBool(b)) => match op {
            BinOp::And => Some(Atom::ALitBool(*a && *b)),
            BinOp::Or => Some(Atom::ALitBool(*a || *b)),
            BinOp::Eq => Some(Atom::ALitBool(*a == *b)),
            BinOp::Ne => Some(Atom::ALitBool(*a != *b)),
            _ => None,
        },
        _ => None,
    }
}

fn fold_unop(op: UnOp, expr: &Atom) -> Option<Atom> {
    match op {
        UnOp::Neg => match expr {
            Atom::ALitInt(n) => Some(Atom::ALitInt(-*n)),
            Atom::ALitFloat(f) => Some(Atom::ALitFloat(-*f)),
            _ => None,
        },
        UnOp::Not => match expr {
            Atom::ALitBool(b) => Some(Atom::ALitBool(!*b)),
            _ => None,
        },
    }
}

fn constant_fold_op(op: AnfOp) -> (AnfOp, bool) {
    match op {
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => {
            let (new_then, c1) = constant_fold(*then_branch);
            let (new_else, c2) = constant_fold(*else_branch);
            (
                AnfOp::AIf {
                    cond,
                    then_branch: Box::new(new_then),
                    else_branch: Box::new(new_else),
                },
                c1 || c2,
            )
        }
        AnfOp::AMatch { scrutinee, arms } => {
            let mut changed = false;
            let arms = arms
                .into_iter()
                .map(|AnfMatchArm { pattern, body }| {
                    let (new_body, c) = constant_fold(body);
                    changed |= c;
                    AnfMatchArm {
                        pattern,
                        body: new_body,
                    }
                })
                .collect();
            (AnfOp::AMatch { scrutinee, arms }, changed)
        }
        AnfOp::ALoop { body } => {
            let (new_body, changed) = constant_fold(*body);
            (
                AnfOp::ALoop {
                    body: Box::new(new_body),
                },
                changed,
            )
        }
        AnfOp::ADefer(inner) => {
            let (new_inner, changed) = constant_fold(*inner);
            (AnfOp::ADefer(Box::new(new_inner)), changed)
        }
        other => (other, false),
    }
}

// ── Branch simplification ─────────────────────────────────────────────────────

/// Eliminate `Let(t, AIf { cond: ALitBool(b), ... }, body)` by selecting the
/// known branch. The selected branch's terminal atom becomes an `AInit` binding
/// for `t` (which copy propagation then eliminates in the next round).
///
/// Returns `(new_expr, changed)`.
pub fn branch_simplify(body: AnfExpr) -> (AnfExpr, bool) {
    match body {
        AnfExpr::Let { local, op, body } => {
            if let AnfOp::AIf {
                cond: Atom::ALitBool(b),
                then_branch,
                else_branch,
            } = *op
            {
                let selected = if b { *then_branch } else { *else_branch };
                let spliced = splice_branch(selected, local, *body);
                return (spliced, true);
            }
            let (new_op, op_changed) = branch_simplify_op(*op);
            let (new_body, body_changed) = branch_simplify(*body);
            (
                AnfExpr::Let {
                    local,
                    op: Box::new(new_op),
                    body: Box::new(new_body),
                },
                op_changed || body_changed,
            )
        }
        other => (other, false),
    }
}

/// Splice a selected branch into the continuation.
///
/// If the branch ends in `Atom(a)`, rewrite the terminal to
/// `Let(result_local, AInit(a), continuation)` so copy propagation picks it up.
/// If the branch ends in a terminal signal (Return/Break/Continue), the
/// continuation is unreachable and we just return the branch as-is.
fn splice_branch(branch: AnfExpr, result_local: LocalId, continuation: AnfExpr) -> AnfExpr {
    match branch {
        AnfExpr::Atom(a) => AnfExpr::Let {
            local: result_local,
            op: Box::new(AnfOp::AInit { value: a }),
            body: Box::new(continuation),
        },
        // Terminals: the continuation is unreachable; just return the branch.
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue => branch,
        // For Let chains in the branch, walk to the leaf.
        AnfExpr::Let { local, op, body } => AnfExpr::Let {
            local,
            op,
            body: Box::new(splice_branch(*body, result_local, continuation)),
        },
    }
}

fn branch_simplify_op(op: AnfOp) -> (AnfOp, bool) {
    match op {
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => {
            let (new_then, c1) = branch_simplify(*then_branch);
            let (new_else, c2) = branch_simplify(*else_branch);
            (
                AnfOp::AIf {
                    cond,
                    then_branch: Box::new(new_then),
                    else_branch: Box::new(new_else),
                },
                c1 || c2,
            )
        }
        AnfOp::AMatch { scrutinee, arms } => {
            let mut changed = false;
            let arms = arms
                .into_iter()
                .map(|AnfMatchArm { pattern, body }| {
                    let (new_body, c) = branch_simplify(body);
                    changed |= c;
                    AnfMatchArm {
                        pattern,
                        body: new_body,
                    }
                })
                .collect();
            (AnfOp::AMatch { scrutinee, arms }, changed)
        }
        AnfOp::ALoop { body } => {
            let (new_body, changed) = branch_simplify(*body);
            (
                AnfOp::ALoop {
                    body: Box::new(new_body),
                },
                changed,
            )
        }
        AnfOp::ADefer(inner) => {
            let (new_inner, changed) = branch_simplify(*inner);
            (AnfOp::ADefer(Box::new(new_inner)), changed)
        }
        other => (other, false),
    }
}

#[cfg(test)]
mod tests {
    use super::dead_let_elim;
    use crate::ir::anf::{AnfExpr, AnfOp, Atom, OpKind};
    use crate::ir::core::LocalId;
    use crate::opt::use_count::{collect_assigned_locals, count_uses};
    use crate::syntax::ast::BinOp;

    #[test]
    fn dead_let_elim_keeps_integer_div_even_when_unused() {
        let expr = AnfExpr::Let {
            local: LocalId(1),
            op: Box::new(AnfOp::ABinOp {
                op: BinOp::Div,
                left: Atom::ALitInt(1),
                right: Atom::ALitInt(0),
                operand_ty: OpKind::Int,
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitInt(42))),
        };
        let uses = count_uses(&expr);
        let assigned = collect_assigned_locals(&expr);

        let (optimized, changed) = dead_let_elim(expr, &uses, &assigned);

        assert!(!changed);
        assert!(matches!(optimized, AnfExpr::Let { .. }));
    }

    #[test]
    fn dead_let_elim_drops_unused_non_trapping_add() {
        let expr = AnfExpr::Let {
            local: LocalId(1),
            op: Box::new(AnfOp::ABinOp {
                op: BinOp::Add,
                left: Atom::ALitInt(1),
                right: Atom::ALitInt(2),
                operand_ty: OpKind::Int,
            }),
            body: Box::new(AnfExpr::Atom(Atom::ALitInt(42))),
        };
        let uses = count_uses(&expr);
        let assigned = collect_assigned_locals(&expr);

        let (optimized, changed) = dead_let_elim(expr, &uses, &assigned);

        assert!(changed);
        assert!(matches!(optimized, AnfExpr::Atom(Atom::ALitInt(42))));
    }
}
