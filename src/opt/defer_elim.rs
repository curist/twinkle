/// ANF defer elimination pass.
///
/// Removes all `ADefer` nodes by rewriting exit points (`Return`, `Break`,
/// `Continue`, terminal `Atom`) to execute deferred expressions in LIFO order
/// before transferring control.
///
/// The pass threads two defer lists:
///
/// * `fn_defers` — expressions declared outside the current loop.  Run on
///   `Return` and on normal function exit (terminal `Atom` at function tail).
/// * `loop_defers` — expressions declared inside the current loop iteration.
///   Run on `Break`, `Continue`, and end-of-iteration (terminal `Atom` at loop
///   body tail).
///
/// When a `Return` fires inside a loop, **both** lists run with full LIFO
/// semantics (inner loop defers before outer function defers).
///
/// Entering a loop resets `loop_defers = []` and folds the previous
/// `loop_defers` into `fn_defers` (so a Return inside an inner loop still
/// unwinds the outer loop's defers).
///
/// After this pass no `ADefer` nodes remain; the WAT backend sees none.
use std::collections::{HashMap, HashSet};

use crate::ir::anf::{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfOp, Atom};
use crate::ir::core::{CorePattern, LocalId};

pub fn eliminate_defers(func: AnfFunctionDef) -> AnfFunctionDef {
    let mut next_local = next_local_id(&func);
    let body = elim(func.body, &[], &[], false, false, &mut next_local);
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
/// `in_loop` — true when inside a loop body. Controls whether terminal `Atom`
///   fires only loop_defers (loop iteration end) or all defers (function exit).
fn elim(
    expr: AnfExpr,
    fn_defers: &[AnfExpr],
    loop_defers: &[AnfExpr],
    in_sub_expr: bool,
    in_loop: bool,
    next_local: &mut u32,
) -> AnfExpr {
    match expr {
        // ── ADefer: register and continue into body ───────────────────────────
        AnfExpr::Let { local: _, op, body } if matches!(*op, AnfOp::ADefer(_)) => {
            let AnfOp::ADefer(d) = *op else {
                unreachable!()
            };

            // Defer capture-by-value: snapshot each free local referenced by
            // the deferred expression into a fresh local at registration time,
            // then rewrite the deferred body to read those snapshot locals.
            let captures = collect_defer_captures(&d);
            let mut remap = HashMap::new();
            let mut snapshots = Vec::with_capacity(captures.len());
            for src in captures {
                let snap = LocalId(*next_local);
                *next_local += 1;
                remap.insert(src, snap);
                snapshots.push((src, snap));
            }
            let deferred = if remap.is_empty() {
                *d
            } else {
                remap_expr(*d, &remap)
            };

            // Add to loop_defers only — fn_defers is set by the loop handler
            // when entering a loop (it folds loop_defers into fn_defers).
            let mut new_loop = loop_defers.to_vec();
            new_loop.push(deferred);

            // The Let wrapper and the binding local are dropped — the ADefer op
            // produced void and had no observable result beyond registration.
            let rewritten_tail = elim(
                *body,
                fn_defers,
                &new_loop,
                in_sub_expr,
                in_loop,
                next_local,
            );
            prepend_snapshot_inits(snapshots, rewritten_tail)
        }

        // ── ALoop: reset loop_defers; fold old loop_defers into fn_defers ────
        AnfExpr::Let { local, op, body } if matches!(*op, AnfOp::ALoop { .. }) => {
            let AnfOp::ALoop { body: loop_body } = *op else {
                unreachable!()
            };
            // Inside the loop, fn_defers grows by the current loop_defers;
            // loop_defers resets to empty.
            let mut inner_fn = fn_defers.to_vec();
            inner_fn.extend_from_slice(loop_defers);
            // Loop body is a tail position for the iteration (not a sub-expr).
            let new_loop_body = elim(*loop_body, &inner_fn, &[], false, true, next_local);
            let new_op = AnfOp::ALoop {
                body: Box::new(new_loop_body),
            };
            // Continuation (rest after the loop) keeps original defer lists.
            let new_body = elim(
                *body,
                fn_defers,
                loop_defers,
                in_sub_expr,
                in_loop,
                next_local,
            );
            AnfExpr::Let {
                local,
                op: Box::new(new_op),
                body: Box::new(new_body),
            }
        }

        // ── General Let: recurse into op sub-expressions and body ─────────────
        AnfExpr::Let { local, op, body } => {
            let new_op = elim_op(*op, fn_defers, loop_defers, in_loop, next_local);
            let new_body = elim(
                *body,
                fn_defers,
                loop_defers,
                in_sub_expr,
                in_loop,
                next_local,
            );
            AnfExpr::Let {
                local,
                op: Box::new(new_op),
                body: Box::new(new_body),
            }
        }

        // ── Return: prepend all defers (LIFO) before returning ──────────────
        AnfExpr::Return(v) => {
            // prepend_defers executes the last element first, so place outer
            // defers first and inner loop defers last to preserve true LIFO.
            let all: Vec<AnfExpr> = fn_defers
                .iter()
                .chain(loop_defers.iter())
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
        // In loop body tail: fire loop_defers only (iteration end, like continue).
        // In function body tail: fire all defers (function exit).
        AnfExpr::Atom(a) => {
            if in_sub_expr {
                AnfExpr::Atom(a)
            } else if in_loop {
                // End of loop iteration — only loop-body-local defers
                prepend_defers(loop_defers.iter().cloned().collect(), AnfExpr::Atom(a))
            } else {
                // Function exit — all defers
                let all: Vec<AnfExpr> = fn_defers
                    .iter()
                    .chain(loop_defers.iter())
                    .cloned()
                    .collect();
                prepend_defers(all, AnfExpr::Atom(a))
            }
        }
    }
}

/// Recurse into ops that contain sub-expressions.
fn elim_op(
    op: AnfOp,
    fn_defers: &[AnfExpr],
    loop_defers: &[AnfExpr],
    in_loop: bool,
    next_local: &mut u32,
) -> AnfOp {
    match op {
        // Branches produce VALUES, not scope exits — pass in_sub_expr=true so
        // terminal Atom nodes inside branches don't fire defers prematurely.
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => AnfOp::AIf {
            cond,
            then_branch: Box::new(elim(
                *then_branch,
                fn_defers,
                loop_defers,
                true,
                in_loop,
                next_local,
            )),
            else_branch: Box::new(elim(
                *else_branch,
                fn_defers,
                loop_defers,
                true,
                in_loop,
                next_local,
            )),
        },
        AnfOp::AMatch { scrutinee, arms } => AnfOp::AMatch {
            scrutinee,
            arms: arms
                .into_iter()
                .map(|AnfMatchArm { pattern, body }| AnfMatchArm {
                    pattern,
                    body: elim(body, fn_defers, loop_defers, true, in_loop, next_local),
                })
                .collect(),
        },
        // ALoop inside an op (not a Let): same reset logic as the Let arm.
        // Loop body is a tail position (not a sub-expr value).
        AnfOp::ALoop { body } => {
            let mut inner_fn = fn_defers.to_vec();
            inner_fn.extend_from_slice(loop_defers);
            AnfOp::ALoop {
                body: Box::new(elim(*body, &inner_fn, &[], false, true, next_local)),
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
    // LIFO: fold in declaration order — splice_defer_before puts each defer
    // before the accumulator, so the last-declared defer ends up outermost
    // (first to execute).
    defers
        .into_iter()
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

fn prepend_snapshot_inits(snapshots: Vec<(LocalId, LocalId)>, tail: AnfExpr) -> AnfExpr {
    snapshots
        .into_iter()
        .rev()
        .fold(tail, |acc, (src, snap)| AnfExpr::Let {
            local: snap,
            op: Box::new(AnfOp::AInit {
                value: Atom::ALocal(src),
            }),
            body: Box::new(acc),
        })
}

fn collect_defer_captures(expr: &AnfExpr) -> Vec<LocalId> {
    let mut declared = HashSet::new();
    let mut free = HashSet::new();
    collect_free_locals_expr(expr, &mut declared, &mut free);
    let mut out: Vec<_> = free.into_iter().collect();
    out.sort_by_key(|id| id.0);
    out
}

fn collect_free_locals_expr(
    expr: &AnfExpr,
    declared: &mut HashSet<LocalId>,
    free: &mut HashSet<LocalId>,
) {
    match expr {
        AnfExpr::Let { local, op, body } => {
            collect_free_locals_op(op, declared, free);
            declared.insert(*local);
            collect_free_locals_expr(body, declared, free);
        }
        AnfExpr::Atom(atom) | AnfExpr::Return(Some(atom)) | AnfExpr::Break(Some(atom)) => {
            collect_free_locals_atom(atom, declared, free);
        }
        AnfExpr::Return(None) | AnfExpr::Break(None) | AnfExpr::Continue => {}
    }
}

fn collect_free_locals_op(
    op: &AnfOp,
    declared: &mut HashSet<LocalId>,
    free: &mut HashSet<LocalId>,
) {
    match op {
        AnfOp::ACall { callee, args } => {
            collect_free_locals_atom(callee, declared, free);
            for arg in args {
                collect_free_locals_atom(arg, declared, free);
            }
        }
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_free_locals_atom(cond, declared, free);
            let mut then_declared = declared.clone();
            let mut else_declared = declared.clone();
            collect_free_locals_expr(then_branch, &mut then_declared, free);
            collect_free_locals_expr(else_branch, &mut else_declared, free);
        }
        AnfOp::AMatch { scrutinee, arms } => {
            collect_free_locals_atom(scrutinee, declared, free);
            for AnfMatchArm { pattern, body } in arms {
                let mut arm_declared = declared.clone();
                collect_pattern_bindings(pattern, &mut arm_declared);
                collect_free_locals_expr(body, &mut arm_declared, free);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            let mut body_declared = declared.clone();
            collect_free_locals_expr(body, &mut body_declared, free);
        }
        AnfOp::ABinOp { left, right, .. } => {
            collect_free_locals_atom(left, declared, free);
            collect_free_locals_atom(right, declared, free);
        }
        AnfOp::AUnOp { expr, .. } => collect_free_locals_atom(expr, declared, free),
        AnfOp::AMakeClosure { free_vars, .. } => {
            for local_id in free_vars {
                if !declared.contains(local_id) {
                    free.insert(*local_id);
                }
            }
        }
        AnfOp::ARecord { fields, .. } => {
            for (_, atom) in fields {
                collect_free_locals_atom(atom, declared, free);
            }
        }
        AnfOp::ARecordGet { target, .. } => collect_free_locals_atom(target, declared, free),
        AnfOp::ARecordUpdate { base, value, .. } => {
            collect_free_locals_atom(base, declared, free);
            collect_free_locals_atom(value, declared, free);
        }
        AnfOp::AVariant { args, .. } | AnfOp::AArrayLit(args) => {
            for atom in args {
                collect_free_locals_atom(atom, declared, free);
            }
        }
        AnfOp::AIndex { base, index, .. } => {
            collect_free_locals_atom(base, declared, free);
            collect_free_locals_atom(index, declared, free);
        }
        AnfOp::AInit { value } => collect_free_locals_atom(value, declared, free),
        AnfOp::AAssign { local, value } => {
            if !declared.contains(local) {
                free.insert(*local);
            }
            collect_free_locals_atom(value, declared, free);
        }
    }
}

fn collect_pattern_bindings(pattern: &CorePattern, declared: &mut HashSet<LocalId>) {
    match pattern {
        CorePattern::Var(id) => {
            declared.insert(*id);
        }
        CorePattern::Variant { fields, .. } => {
            for field in fields {
                collect_pattern_bindings(field, declared);
            }
        }
        CorePattern::Wildcard
        | CorePattern::LitInt(_)
        | CorePattern::LitBool(_)
        | CorePattern::LitStr(_) => {}
    }
}

fn collect_free_locals_atom(atom: &Atom, declared: &HashSet<LocalId>, free: &mut HashSet<LocalId>) {
    if let Atom::ALocal(local_id) = atom {
        if !declared.contains(local_id) {
            free.insert(*local_id);
        }
    }
}

fn remap_expr(expr: AnfExpr, remap: &HashMap<LocalId, LocalId>) -> AnfExpr {
    match expr {
        AnfExpr::Let { local, op, body } => AnfExpr::Let {
            local,
            op: Box::new(remap_op(*op, remap)),
            body: Box::new(remap_expr(*body, remap)),
        },
        AnfExpr::Return(value) => AnfExpr::Return(value.map(|atom| remap_atom(atom, remap))),
        AnfExpr::Break(value) => AnfExpr::Break(value.map(|atom| remap_atom(atom, remap))),
        AnfExpr::Continue => AnfExpr::Continue,
        AnfExpr::Atom(atom) => AnfExpr::Atom(remap_atom(atom, remap)),
    }
}

fn remap_op(op: AnfOp, remap: &HashMap<LocalId, LocalId>) -> AnfOp {
    match op {
        AnfOp::ACall { callee, args } => AnfOp::ACall {
            callee: remap_atom(callee, remap),
            args: args.into_iter().map(|arg| remap_atom(arg, remap)).collect(),
        },
        AnfOp::AIf {
            cond,
            then_branch,
            else_branch,
        } => AnfOp::AIf {
            cond: remap_atom(cond, remap),
            then_branch: Box::new(remap_expr(*then_branch, remap)),
            else_branch: Box::new(remap_expr(*else_branch, remap)),
        },
        AnfOp::AMatch { scrutinee, arms } => AnfOp::AMatch {
            scrutinee: remap_atom(scrutinee, remap),
            arms: arms
                .into_iter()
                .map(|AnfMatchArm { pattern, body }| AnfMatchArm {
                    pattern,
                    body: remap_expr(body, remap),
                })
                .collect(),
        },
        AnfOp::ALoop { body } => AnfOp::ALoop {
            body: Box::new(remap_expr(*body, remap)),
        },
        AnfOp::ABinOp {
            op,
            left,
            right,
            operand_ty,
        } => AnfOp::ABinOp {
            op,
            left: remap_atom(left, remap),
            right: remap_atom(right, remap),
            operand_ty,
        },
        AnfOp::AUnOp {
            op,
            expr,
            operand_ty,
        } => AnfOp::AUnOp {
            op,
            expr: remap_atom(expr, remap),
            operand_ty,
        },
        AnfOp::AMakeClosure { func_id, free_vars } => AnfOp::AMakeClosure {
            func_id,
            free_vars: free_vars
                .into_iter()
                .map(|local| remap.get(&local).copied().unwrap_or(local))
                .collect(),
        },
        AnfOp::ARecord { type_id, fields } => AnfOp::ARecord {
            type_id,
            fields: fields
                .into_iter()
                .map(|(field, atom)| (field, remap_atom(atom, remap)))
                .collect(),
        },
        AnfOp::ARecordGet {
            target,
            field,
            type_id,
        } => AnfOp::ARecordGet {
            target: remap_atom(target, remap),
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
            base: remap_atom(base, remap),
            field,
            value: remap_atom(value, remap),
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
            args: args.into_iter().map(|arg| remap_atom(arg, remap)).collect(),
        },
        AnfOp::AArrayLit(args) => {
            AnfOp::AArrayLit(args.into_iter().map(|arg| remap_atom(arg, remap)).collect())
        }
        AnfOp::AIndex {
            base,
            index,
            base_ty,
            result_ty,
        } => AnfOp::AIndex {
            base: remap_atom(base, remap),
            index: remap_atom(index, remap),
            base_ty,
            result_ty,
        },
        AnfOp::AInit { value } => AnfOp::AInit {
            value: remap_atom(value, remap),
        },
        AnfOp::AAssign { local, value } => AnfOp::AAssign {
            local: remap.get(&local).copied().unwrap_or(local),
            value: remap_atom(value, remap),
        },
        AnfOp::ADefer(body) => AnfOp::ADefer(Box::new(remap_expr(*body, remap))),
    }
}

fn remap_atom(atom: Atom, remap: &HashMap<LocalId, LocalId>) -> Atom {
    match atom {
        Atom::ALocal(local) => Atom::ALocal(remap.get(&local).copied().unwrap_or(local)),
        other => other,
    }
}

/// Allocate fresh LocalIds above every LocalId referenced anywhere in `func`.
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
