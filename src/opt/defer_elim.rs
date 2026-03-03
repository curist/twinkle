/// ANF defer elimination pass.
///
/// Removes all `ADefer` nodes by rewriting exit points (`Return`, `Break`,
/// `Continue`, terminal `Atom`) to execute deferred expressions in LIFO order
/// before transferring control.
///
/// The pass threads two defer lists:
///
/// * `fn_defers` — expressions active from the current point to the enclosing
///   function boundary.  Run on `Return`.
/// * `loop_defers` — expressions active within the current loop iteration.
///   Run on `Break` and `Continue` (loop-level exits only).
///
/// When a `Return` fires inside a loop, **both** lists run (loop_defers then
/// fn_defers, both LIFO), so the inner-most defer always runs first.
///
/// Entering a loop resets `loop_defers = []` and folds the previous
/// `loop_defers` into `fn_defers` (so a Return inside an inner loop still
/// unwinds the outer loop's defers).
///
/// After this pass no `ADefer` nodes remain; the WAT backend sees none.
use crate::ir::anf::{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfOp};
use crate::ir::core::LocalId;

pub fn eliminate_defers(func: AnfFunctionDef) -> AnfFunctionDef {
    let body = elim(func.body, &[], &[], false);
    AnfFunctionDef { body, ..func }
}

/// Recursively rewrite `expr`, threading the active defer lists.
///
/// `fn_defers` — declared order (will be reversed on insertion).
/// `loop_defers` — declared order (will be reversed on insertion).
/// `in_sub_expr` — true when processing a branch/arm VALUE (AIf/AMatch sub-expression).
///   When true, terminal `Atom` nodes are NOT treated as scope exits and do NOT fire
///   defers — they are just value-producing positions, not function/loop exit points.
///   `Return`/`Break`/`Continue` always fire defers regardless of this flag.
fn elim(
    expr: AnfExpr,
    fn_defers: &[AnfExpr],
    loop_defers: &[AnfExpr],
    in_sub_expr: bool,
) -> AnfExpr {
    match expr {
        // ── ADefer: register and continue into body ───────────────────────────
        AnfExpr::Let { local: _, op, body } if matches!(*op, AnfOp::ADefer(_)) => {
            let AnfOp::ADefer(d) = *op else {
                unreachable!()
            };
            // Add to both lists (LIFO: append; later prepended in reverse).
            let mut new_fn = fn_defers.to_vec();
            new_fn.push(*d.clone());
            let mut new_loop = loop_defers.to_vec();
            new_loop.push(*d);
            // The Let wrapper and the binding local are dropped — the ADefer op
            // produced void and had no observable result beyond registration.
            elim(*body, &new_fn, &new_loop, in_sub_expr)
        }

        // ── ALoop: reset loop_defers; fold old loop_defers into fn_defers ────
        AnfExpr::Let { local, op, body } if matches!(*op, AnfOp::ALoop { .. }) => {
            let AnfOp::ALoop { body: loop_body } = *op else {
                unreachable!()
            };
            // Inside the loop, fn_defers grows by the outer loop_defers;
            // loop_defers resets to empty.
            let mut inner_fn = fn_defers.to_vec();
            inner_fn.extend_from_slice(loop_defers);
            // Loop body is a tail position for the iteration (not a sub-expr).
            let new_loop_body = elim(*loop_body, &inner_fn, &[], false);
            let new_op = AnfOp::ALoop {
                body: Box::new(new_loop_body),
            };
            // Continuation (rest after the loop) keeps original defer lists.
            let new_body = elim(*body, fn_defers, loop_defers, in_sub_expr);
            AnfExpr::Let {
                local,
                op: Box::new(new_op),
                body: Box::new(new_body),
            }
        }

        // ── General Let: recurse into op sub-expressions and body ─────────────
        AnfExpr::Let { local, op, body } => {
            let new_op = elim_op(*op, fn_defers, loop_defers);
            let new_body = elim(*body, fn_defers, loop_defers, in_sub_expr);
            AnfExpr::Let {
                local,
                op: Box::new(new_op),
                body: Box::new(new_body),
            }
        }

        // ── Return: prepend fn_defers ++ loop_defers (LIFO) before returning ──
        AnfExpr::Return(v) => {
            // Defers run innermost-first: loop_defers (most recent) then fn_defers.
            let all: Vec<AnfExpr> = loop_defers
                .iter()
                .chain(fn_defers.iter())
                .cloned()
                .collect();
            prepend_defers(all, AnfExpr::Return(v))
        }

        // ── Break: prepend loop_defers (LIFO) before breaking ─────────────────
        AnfExpr::Break(v) => {
            prepend_defers(loop_defers.iter().cloned().collect(), AnfExpr::Break(v))
        }

        // ── Continue: prepend loop_defers (LIFO) before continuing ────────────
        AnfExpr::Continue => {
            prepend_defers(loop_defers.iter().cloned().collect(), AnfExpr::Continue)
        }

        // ── Terminal Atom ──────────────────────────────────────────────────────
        // In sub-expression position (AIf/AMatch branch value): just a value,
        // not a scope exit — do NOT fire defers.
        // In tail position (function or loop iteration body): fire all active defers.
        AnfExpr::Atom(a) => {
            if in_sub_expr {
                AnfExpr::Atom(a)
            } else {
                let all: Vec<AnfExpr> = loop_defers
                    .iter()
                    .chain(fn_defers.iter())
                    .cloned()
                    .collect();
                prepend_defers(all, AnfExpr::Atom(a))
            }
        }
    }
}

/// Recurse into ops that contain sub-expressions.
fn elim_op(op: AnfOp, fn_defers: &[AnfExpr], loop_defers: &[AnfExpr]) -> AnfOp {
    match op {
        // Branches produce VALUES, not scope exits — pass in_sub_expr=true so
        // terminal Atom nodes inside branches don't fire defers prematurely.
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => AnfOp::AIf {
            cond,
            then_branch: Box::new(elim(*then_branch, fn_defers, loop_defers, true)),
            else_branch: Box::new(elim(*else_branch, fn_defers, loop_defers, true)),
        },
        AnfOp::AMatch { scrutinee, arms } => AnfOp::AMatch {
            scrutinee,
            arms: arms
                .into_iter()
                .map(|AnfMatchArm { pattern, body }| AnfMatchArm {
                    pattern,
                    body: elim(body, fn_defers, loop_defers, true),
                })
                .collect(),
        },
        // ALoop inside an op (not a Let): same reset logic as the Let arm.
        // Loop body is a tail position (not a sub-expr value).
        AnfOp::ALoop { body } => {
            let mut inner_fn = fn_defers.to_vec();
            inner_fn.extend_from_slice(loop_defers);
            AnfOp::ALoop {
                body: Box::new(elim(*body, &inner_fn, &[], false)),
            }
        }
        // ADefer in op position means it was never inside a Let — invalid ANF.
        // lower_anf always wraps ADefer as the op of a Let node.
        AnfOp::ADefer(_) => {
            unreachable!("ADefer in op position — invalid ANF; should always appear as Let op")
        }
        other => other,
    }
}

/// Insert deferred expressions (LIFO — `defers` is in declaration order, so we
/// reverse before building the chain) as sequenced let-bindings before `tail`.
///
/// Each deferred `AnfExpr` is spliced in by walking its let-chain to its terminal
/// `Atom` and replacing it with `tail`. The deferred void result is discarded.
fn prepend_defers(defers: Vec<AnfExpr>, tail: AnfExpr) -> AnfExpr {
    // LIFO: reverse so the last-declared defer runs first.
    defers
        .into_iter()
        .rev()
        .fold(tail, |acc, d| splice_defer_before(d, acc))
}

/// Walk the let-chain of `deferred` and replace its terminal `Atom` with
/// `continuation`, discarding the void result. Non-atom terminals (Return/
/// Break/Continue) in the deferred body should not exist after the type checker
/// rejected Never-typed defers; treat them as-is as a safety fallback.
fn splice_defer_before(deferred: AnfExpr, continuation: AnfExpr) -> AnfExpr {
    match deferred {
        // Terminal atom — deferred expression produced void; discard and continue.
        AnfExpr::Atom(_) => continuation,
        // Let chain — walk to the leaf.
        AnfExpr::Let { local, op, body } => AnfExpr::Let {
            local,
            op,
            body: Box::new(splice_defer_before(*body, continuation)),
        },
        // Diverging terminal in a deferred expression — should not happen after
        // type checking, but keep the terminal as a safe fallback.
        terminal => terminal,
    }
}

/// A fresh local counter for the defer elimination pass.
///
/// The pass needs unique LocalIds for the Let nodes it introduces when splicing
/// deferred let-chains. Since `AnfFunctionDef` doesn't carry a counter, we
/// compute the maximum existing LocalId and use IDs above it.
#[allow(dead_code)]
fn max_local_in_expr(expr: &AnfExpr, max: &mut u32) {
    match expr {
        AnfExpr::Let { local, op, body } => {
            if local.0 > *max {
                *max = local.0;
            }
            max_local_in_op(op, max);
            max_local_in_expr(body, max);
        }
        _ => {}
    }
}

#[allow(dead_code)]
fn max_local_in_op(op: &AnfOp, max: &mut u32) {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            max_local_in_expr(then_branch, max);
            max_local_in_expr(else_branch, max);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                max_local_in_expr(&arm.body, max);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            max_local_in_expr(body, max);
        }
        _ => {}
    }
}

/// Allocate a fresh `LocalId` above the maximum seen in `func`.
#[allow(dead_code)]
fn fresh_above(func: &AnfFunctionDef) -> (LocalId, u32) {
    let mut max = 0u32;
    for p in &func.params {
        if p.0 > max {
            max = p.0;
        }
    }
    max_local_in_expr(&func.body, &mut max);
    let next = max + 1;
    (LocalId(next), next + 1)
}
