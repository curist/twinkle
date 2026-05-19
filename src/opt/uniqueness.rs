use std::collections::{HashMap, HashSet};

use crate::ir::anf::{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfModule, AnfOp, Atom};
use crate::ir::core::{CorePattern, FieldId, FuncId, LocalId};
use crate::ir::lower::prelude;
use crate::opt::liveness::live_after;

// ── Unified operation semantics table ────────────────────────────────────────
//
// Central classification of every known intrinsic for the uniqueness optimizer.
//
// All analysis (taint pre-scan, point rewrites, loop rewrites, wrapper
// summaries, straight-line builder rewrites) queries this single table
// instead of ad-hoc per-operation checks scattered across the pass.

#[derive(Debug, Clone)]
struct OpSemantics {
    /// Index of the "base" collection argument (the one being COW-updated).
    /// `None` for read-only ops and fresh producers.
    base_arg: Option<usize>,

    /// Direct callee-swap in-place variant (e.g. DICT_SET → DICT_SET_IN_PLACE).
    in_place_id: Option<FuncId>,

    /// True if, when the base is shared, the COW path produces a fresh copy.
    /// Enables Phase B "fresh-after-COW" reasoning.
    fresh_if_copied: bool,

    /// True if this op never retains or aliases any of its arguments and is
    /// otherwise observationally read-only with respect to them.
    no_retain_read_only: bool,

    /// True if this call does not retain or alias any argument, even if it is
    /// not a pure read-only query. This stays separate from
    /// `no_retain_read_only` so allocating combinators like VECTOR_CONCAT can
    /// avoid tainting their inputs without being treated as harmless
    /// bookkeeping during builder-region scans.
    no_retain_args: bool,

    /// True if this op always produces a fresh unique value with no base arg
    /// (e.g. DICT_NEW, VECTOR_MAKE, VECTOR_BUILDER_FREEZE).
    fresh_producer: bool,

    /// True if this COW update participates in the vector builder rewrite
    /// (VECTOR_APPEND → VECTOR_BUILDER_PUSH lifecycle).
    builder_rewritable: bool,
}

/// Derived view for COW update operations (those with a base arg).
/// Returned by `resolve_cow_call` so call sites get a guaranteed `usize` base.
struct CowOpInfo {
    base_arg: usize,
    in_place_id: Option<FuncId>,
    fresh_if_copied: bool,
    builder_rewritable: bool,
}

impl OpSemantics {
    fn as_cow_info(&self) -> Option<CowOpInfo> {
        Some(CowOpInfo {
            base_arg: self.base_arg?,
            in_place_id: self.in_place_id,
            fresh_if_copied: self.fresh_if_copied,
            builder_rewritable: self.builder_rewritable,
        })
    }
}

fn op_semantics(func_id: FuncId) -> Option<OpSemantics> {
    // ── COW update ops ───────────────────────────────────────────────────
    if func_id == prelude::VECTOR_SET_UNSAFE {
        return Some(OpSemantics {
            base_arg: Some(0),
            in_place_id: Some(prelude::VECTOR_SET_IN_PLACE),
            fresh_if_copied: true,
            no_retain_read_only: false,
            no_retain_args: false,
            fresh_producer: false,
            builder_rewritable: false,
        });
    }
    if func_id == prelude::DICT_SET {
        return Some(OpSemantics {
            base_arg: Some(0),
            in_place_id: Some(prelude::DICT_SET_IN_PLACE),
            fresh_if_copied: true,
            no_retain_read_only: false,
            no_retain_args: false,
            fresh_producer: false,
            builder_rewritable: false,
        });
    }
    if func_id == prelude::DICT_REMOVE {
        return Some(OpSemantics {
            base_arg: Some(0),
            in_place_id: Some(prelude::DICT_REMOVE_IN_PLACE),
            fresh_if_copied: true,
            no_retain_read_only: false,
            no_retain_args: false,
            fresh_producer: false,
            builder_rewritable: false,
        });
    }
    if func_id == prelude::VECTOR_APPEND {
        return Some(OpSemantics {
            base_arg: Some(0),
            in_place_id: None,
            fresh_if_copied: false,
            no_retain_read_only: false,
            no_retain_args: false,
            fresh_producer: false,
            builder_rewritable: true,
        });
    }
    if func_id == prelude::VECTOR_CONCAT {
        return Some(OpSemantics {
            base_arg: None,
            in_place_id: None,
            fresh_if_copied: false,
            no_retain_read_only: false,
            no_retain_args: false,
            fresh_producer: false,
            builder_rewritable: false,
        });
    }
    // ── Read-only / no-retain ops ────────────────────────────────────────
    if func_id == prelude::VECTOR_LEN
        || func_id == prelude::VECTOR_GET
        || func_id == prelude::DICT_LEN
        || func_id == prelude::DICT_HAS
        || func_id == prelude::DICT_GET
        || func_id == prelude::DICT_GET_UNSAFE
        || func_id == prelude::DICT_KEYS
    {
        return Some(OpSemantics {
            base_arg: None,
            in_place_id: None,
            fresh_if_copied: false,
            no_retain_read_only: true,
            no_retain_args: true,
            fresh_producer: false,
            builder_rewritable: false,
        });
    }
    // ── Fresh producers ──────────────────────────────────────────────────
    if func_id == prelude::VECTOR_MAKE
        || func_id == prelude::VECTOR_BUILDER_FREEZE
        || func_id == prelude::DICT_NEW
    {
        return Some(OpSemantics {
            base_arg: None,
            in_place_id: None,
            fresh_if_copied: false,
            no_retain_read_only: false,
            no_retain_args: false,
            fresh_producer: true,
            builder_rewritable: false,
        });
    }
    None
}

fn is_no_retain_read_only(func_id: FuncId) -> bool {
    op_semantics(func_id).map_or(false, |s| s.no_retain_read_only)
}

fn call_does_not_retain_args(func_id: FuncId) -> bool {
    op_semantics(func_id).map_or(false, |s| s.no_retain_args)
}

fn alloc_local(next_local: &mut u32) -> LocalId {
    let local = LocalId(*next_local);
    *next_local += 1;
    local
}

#[derive(Debug, Clone)]
pub struct TinyWrapperSummary {
    pub wrapped_func: Option<FuncId>,
    pub base_arg: Option<usize>,
    pub arg_map: Vec<usize>,
    pub returns_fresh_record: bool,
}

pub fn collect_tiny_wrapper_summaries(module: &AnfModule) -> HashMap<FuncId, TinyWrapperSummary> {
    let mut summaries = HashMap::new();
    for func in &module.functions {
        if let Some(summary) = summarize_tiny_wrapper(func) {
            summaries.insert(func.func_id, summary);
        }
    }
    summaries
}

fn tail_returns_local(mut expr: &AnfExpr, mut current: LocalId) -> bool {
    loop {
        match expr {
            AnfExpr::Let { local, op, body } => match op.as_ref() {
                AnfOp::AInit {
                    value: Atom::ALocal(source),
                } if *source == current => {
                    current = *local;
                    expr = body;
                }
                _ => return false,
            },
            AnfExpr::Atom(Atom::ALocal(ret)) | AnfExpr::Return(Some(Atom::ALocal(ret))) => {
                return *ret == current;
            }
            _ => return false,
        }
    }
}

fn tail_is_fresh_record(mut expr: &AnfExpr) -> bool {
    loop {
        match expr {
            AnfExpr::Let { local, op, body } => {
                if matches!(op.as_ref(), AnfOp::ARecord { .. }) && tail_returns_local(body, *local)
                {
                    return true;
                }
                expr = body;
            }
            _ => return false,
        }
    }
}

fn summarize_cow_wrapper(func: &AnfFunctionDef) -> Option<(FuncId, usize, Vec<usize>)> {
    let mut param_aliases = HashMap::new();
    for (i, param) in func.params.iter().enumerate() {
        param_aliases.insert(*param, i);
    }

    let mut cursor = &func.body;
    loop {
        let AnfExpr::Let { local, op, body } = cursor else {
            return None;
        };
        match op.as_ref() {
            AnfOp::AInit {
                value: Atom::ALocal(source),
            } => {
                let param_index = *param_aliases.get(source)?;
                param_aliases.insert(*local, param_index);
                cursor = body;
            }
            AnfOp::ACall {
                callee: Atom::AGlobalFunc(wrapped_func),
                args,
            } => {
                let Some(info) = op_semantics(*wrapped_func).and_then(|s| s.as_cow_info()) else {
                    return None;
                };
                if !info.builder_rewritable && info.in_place_id.is_none() {
                    return None;
                }
                if args.len() != func.params.len() {
                    return None;
                }
                let mut arg_map = Vec::with_capacity(args.len());
                for arg in args {
                    let Atom::ALocal(arg_local) = arg else {
                        return None;
                    };
                    arg_map.push(*param_aliases.get(arg_local)?);
                }

                let mut result_local = *local;
                let mut tail = body.as_ref();
                loop {
                    match tail {
                        AnfExpr::Let {
                            local,
                            op,
                            body: next,
                        } => match op.as_ref() {
                            AnfOp::AInit {
                                value: Atom::ALocal(source),
                            } if *source == result_local => {
                                result_local = *local;
                                tail = next;
                            }
                            _ => return None,
                        },
                        AnfExpr::Atom(Atom::ALocal(ret))
                        | AnfExpr::Return(Some(Atom::ALocal(ret)))
                            if *ret == result_local =>
                        {
                            return Some((*wrapped_func, info.base_arg, arg_map));
                        }
                        _ => return None,
                    }
                }
            }
            _ => return None,
        }
    }
}

fn summarize_tiny_wrapper(func: &AnfFunctionDef) -> Option<TinyWrapperSummary> {
    let returns_fresh_record = tail_is_fresh_record(&func.body);
    let cow_wrapper = summarize_cow_wrapper(func);
    match (cow_wrapper, returns_fresh_record) {
        (Some((wrapped_func, base_arg, arg_map)), returns_fresh_record) => {
            Some(TinyWrapperSummary {
                wrapped_func: Some(wrapped_func),
                base_arg: Some(base_arg),
                arg_map,
                returns_fresh_record,
            })
        }
        (None, true) => Some(TinyWrapperSummary {
            wrapped_func: None,
            base_arg: None,
            arg_map: Vec::new(),
            returns_fresh_record: true,
        }),
        (None, false) => None,
    }
}

fn resolve_cow_call<'a>(
    func_id: FuncId,
    args: &'a [Atom],
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) -> Option<(CowOpInfo, FuncId, Vec<&'a Atom>)> {
    if let Some(info) = op_semantics(func_id).and_then(|s| s.as_cow_info()) {
        return Some((info, func_id, args.iter().collect()));
    }
    let summary = wrappers.get(&func_id)?;
    let wrapped_func = summary.wrapped_func?;
    let info = op_semantics(wrapped_func).and_then(|s| s.as_cow_info())?;
    if args.len() != summary.arg_map.len() {
        return None;
    }
    let resolved_args = summary
        .arg_map
        .iter()
        .map(|param_index| args.get(*param_index))
        .collect::<Option<Vec<_>>>()?;
    Some((info, wrapped_func, resolved_args))
}

/// Returns true when `base` is safe for a direct in-place COW swap.
///
/// When `source_fresh` is Some, the presence of `base` in that set acts as an
/// additional bypass: the value is freshly allocated and hasn't been passed to
/// any opaque function yet, so modifying it in-place is safe.
///
/// **Important**: only pass `Some(source_fresh)` for ops that have a real
/// `in_place_id` (DICT_SET, DICT_REMOVE, VECTOR_SET_UNSAFE). For ops without
/// an in-place variant (VECTOR_APPEND), pass `None`. Passing `Some` for
/// VECTOR_APPEND would trigger the `unique.insert(result)` propagation even
/// without an actual swap, which can cascade and incorrectly enable downstream
/// in-place ops after an opaque function call.
fn base_can_rewrite(
    base: LocalId,
    tainted: &HashSet<LocalId>,
    unique: &HashSet<LocalId>,
    refreshed: &HashSet<LocalId>,
    source_fresh: Option<&HashSet<LocalId>>,
) -> bool {
    unique.contains(&base)
        && (!tainted.contains(&base)
            || refreshed.contains(&base)
            || source_fresh.map_or(false, |sf| sf.contains(&base)))
}

// ── Fresh producer detection ─────────────────────────────────────────────────

// ── Termination detection ───────────────────────────────────────────────────

/// Returns true if every execution path through `expr` ends in a terminator
/// (return / break / continue) without reaching the natural fall-through exit.
/// Used by the if-join to avoid destroying state that can't be affected when
/// one branch always exits early.
fn expr_always_terminates(expr: &AnfExpr) -> bool {
    match expr {
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue => true,
        AnfExpr::Atom(_) => false,
        AnfExpr::Let { op, body, .. } => {
            if op_always_terminates(op) {
                return true;
            }
            expr_always_terminates(body)
        }
    }
}

fn op_always_terminates(op: &AnfOp) -> bool {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => expr_always_terminates(then_branch) && expr_always_terminates(else_branch),
        AnfOp::AMatch { arms, .. } => {
            !arms.is_empty() && arms.iter().all(|a| expr_always_terminates(&a.body))
        }
        _ => false,
    }
}

fn is_fresh_producer(op: &AnfOp) -> bool {
    match op {
        AnfOp::AArrayLit(_) | AnfOp::ARecord { .. } | AnfOp::AVariant { .. } => true,
        AnfOp::ACall {
            callee: Atom::AGlobalFunc(id),
            ..
        } => op_semantics(*id).map_or(false, |s| s.fresh_producer),
        _ => false,
    }
}

// ── Pre-scan: collect tainted (aliased / escaped) locals ─────────────────────

fn collect_tainted(
    func: &AnfFunctionDef,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) -> HashSet<LocalId> {
    let mut tainted = HashSet::new();
    // Function params come from outside — never unique.
    for p in &func.params {
        tainted.insert(*p);
    }
    scan_tainted_expr(&func.body, &mut tainted, &HashSet::new(), wrappers);

    // Phase A: reassign-aware taint refinement.
    // The pre-scan above is flow-insensitive — if a local escapes anywhere (e.g.
    // stored into a record at function end), it's tainted everywhere. But for
    // reassigned locals, `assign(d = r)` kills the old value. If every escape of
    // `d` is followed by a reassign before the next use, the escape only affects
    // the final version, not the intermediate ones that the optimizer can rewrite.
    //
    // Walk the top-level Let chain only (no recursion into branches/loops).
    // For each tainted reassign-target, track whether it's currently escaped.
    // A reassign resets the escaped flag. If we reach the end with escaped=false,
    // remove the local from tainted.
    refine_tainted_for_reassigned_locals(&func.body, &mut tainted);

    tainted
}

/// Phase A pass 2: for reassigned locals that were tainted only because of a
/// terminal escape (stored in record/returned at end of function), check if all
/// COW operations happen BEFORE the first escape. If so, the taint doesn't
/// affect the COW ops and can be removed.
///
/// The key insight: if a local `d` is built linearly via COW ops and only
/// escapes at the very end (e.g., stored in a record), the intermediate
/// versions are never aliased. The pre-scan tainted `d` because it saw the
/// final escape, but all COW ops saw a unique value.
fn refine_tainted_for_reassigned_locals(body: &AnfExpr, tainted: &mut HashSet<LocalId>) {
    // Collect locals that are COW-reassign targets on the top-level spine.
    let mut cow_reassigned: HashSet<LocalId> = HashSet::new();
    collect_cow_reassign_targets_spine(body, &mut cow_reassigned);

    let candidates: Vec<LocalId> = cow_reassigned
        .iter()
        .copied()
        .filter(|l| tainted.contains(l))
        .collect();

    if candidates.is_empty() {
        return;
    }

    for local in candidates {
        if all_escapes_after_last_cow_use(body, local) {
            tainted.remove(&local);
        }
    }
}

/// Collect locals that are targets of COW-consume-reassign on the top-level spine:
///   let r = DICT_SET(d, ...) ; assign(d = r)
///   let r = RecordUpdate(d, field, val) ; assign(d = r)
fn collect_cow_reassign_targets_spine(expr: &AnfExpr, targets: &mut HashSet<LocalId>) {
    let mut cursor = expr;
    loop {
        match cursor {
            AnfExpr::Let { local, op, body } => {
                // Check for COW call op followed by assign(target = result)
                if let AnfOp::ACall {
                    callee: Atom::AGlobalFunc(func_id),
                    args,
                } = op.as_ref()
                {
                    if let Some(info) = op_semantics(*func_id).and_then(|s| s.as_cow_info()) {
                        if let Some(Atom::ALocal(base)) = args.get(info.base_arg) {
                            if is_consume_reassign(body, *base, *local) {
                                targets.insert(*base);
                            }
                        }
                    }
                }
                // Check for record update followed by assign(target = result)
                if let AnfOp::ARecordUpdate {
                    base: Atom::ALocal(base),
                    ..
                } = op.as_ref()
                {
                    if is_consume_reassign(body, *base, *local) {
                        targets.insert(*base);
                    }
                }
                cursor = body;
            }
            _ => break,
        }
    }
}

/// Check that all escape points of `local` on the spine occur AFTER the last
/// COW-base use of `local`. Also bail out if `local` appears in any nested
/// scope (branch/loop/closure).
///
/// "Escape" = stored in record/array/variant, passed to non-COW call, captured.
/// "COW-base use" = used as base arg in a COW op.
///
/// If this returns true, the local's taint doesn't affect any COW rewrite
/// decision, so it can be safely removed.
fn all_escapes_after_last_cow_use(body: &AnfExpr, local: LocalId) -> bool {
    // Walk the spine and record positions of COW-base uses and escapes.
    // We track: has_seen_escape (set when we see an escape), and check
    // that no COW-base use occurs after an escape.
    let mut seen_escape = false;
    let mut cursor = body;

    loop {
        match cursor {
            AnfExpr::Let { local: _, op, body } => {
                // Bail out if local appears in a nested scope.
                if op_has_local_in_nested_scope(op, local) {
                    return false;
                }

                // Check if this is a COW-base use of local.
                let is_cow_base = if let AnfOp::ACall {
                    callee: Atom::AGlobalFunc(func_id),
                    args,
                } = op.as_ref()
                {
                    if let Some(info) = op_semantics(*func_id).and_then(|s| s.as_cow_info()) {
                        args.get(info.base_arg)
                            .map_or(false, |a| atom_is_local(a, local))
                    } else {
                        false
                    }
                } else if let AnfOp::ARecordUpdate {
                    base: Atom::ALocal(b),
                    ..
                } = op.as_ref()
                {
                    *b == local
                } else {
                    false
                };

                if is_cow_base && seen_escape {
                    // A COW op on local AFTER an escape — the escape could alias
                    // the value the COW op sees. Not safe to untaint.
                    return false;
                }

                // Check if this op escapes local.
                if op_escapes_local(op, local) {
                    seen_escape = true;
                }

                cursor = body;
            }
            _ => break,
        }
    }

    // We need at least one escape (otherwise the local wouldn't be tainted by
    // the spine, so this function shouldn't have been called — but be safe).
    seen_escape
}

/// Check whether `op` uses `local` in a position that constitutes an escape
/// (stored in container, passed to non-COW/non-read-only call, captured by closure).
/// This mirrors the logic in `scan_tainted_op` but checks a specific local.
fn op_escapes_local(op: &AnfOp, local: LocalId) -> bool {
    match op {
        AnfOp::AMakeClosure { free_vars, .. } => free_vars.contains(&local),
        AnfOp::AArrayLit(elems) => elems.iter().any(|a| atom_is_local(a, local)),
        AnfOp::ARecord { fields, .. } => fields.iter().any(|(_, a)| atom_is_local(a, local)),
        AnfOp::AVariant { args, .. } => args.iter().any(|a| atom_is_local(a, local)),
        AnfOp::ACall {
            callee: Atom::AGlobalFunc(func_id),
            args,
        } => {
            if let Some(info) = op_semantics(*func_id).and_then(|s| s.as_cow_info()) {
                // COW op: only non-base args escape
                args.iter()
                    .enumerate()
                    .any(|(i, a)| i != info.base_arg && atom_is_local(a, local))
            } else if call_does_not_retain_args(*func_id) {
                false
            } else {
                // Unknown call: any arg position is an escape
                args.iter().any(|a| atom_is_local(a, local))
            }
        }
        AnfOp::ACall { args, .. } => {
            // Indirect call: all args escape
            args.iter().any(|a| atom_is_local(a, local))
        }
        // AInit(y = local) is an alias, not an escape in itself — the alias
        // tracking in pass 1 handles that separately. But if the alias target
        // is tainted, that's already reflected. Don't count AInit as escape here.
        _ => false,
    }
}

/// Check whether `local` appears anywhere inside a nested scope (branch, loop,
/// or closure body) within `op`. If so, we can't reason about ordering and must
/// bail out.
fn op_has_local_in_nested_scope(op: &AnfOp, local: LocalId) -> bool {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => expr_mentions_local(then_branch, local) || expr_mentions_local(else_branch, local),
        AnfOp::AMatch { arms, .. } => arms.iter().any(|arm| expr_mentions_local(&arm.body, local)),
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => expr_mentions_local(body, local),
        AnfOp::AMakeClosure { free_vars, .. } => free_vars.contains(&local),
        _ => false,
    }
}

/// Check whether `local` is mentioned anywhere in an expression tree.
fn expr_mentions_local(expr: &AnfExpr, local: LocalId) -> bool {
    match expr {
        AnfExpr::Let { op, body, .. } => {
            op_mentions_local(op, local) || expr_mentions_local(body, local)
        }
        AnfExpr::Return(Some(atom)) | AnfExpr::Break(Some(atom)) | AnfExpr::Atom(atom) => {
            atom_is_local(atom, local)
        }
        AnfExpr::Return(None) | AnfExpr::Break(None) | AnfExpr::Continue => false,
    }
}

/// Check whether `local` is mentioned anywhere in an op (including nested scopes).
fn op_mentions_local(op: &AnfOp, local: LocalId) -> bool {
    if op_uses_local_non_recursive(op, local) {
        return true;
    }
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => expr_mentions_local(then_branch, local) || expr_mentions_local(else_branch, local),
        AnfOp::AMatch { arms, .. } => arms.iter().any(|arm| expr_mentions_local(&arm.body, local)),
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => expr_mentions_local(body, local),
        _ => false,
    }
}

fn scan_tainted_expr(
    expr: &AnfExpr,
    tainted: &mut HashSet<LocalId>,
    live_out: &HashSet<LocalId>,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) {
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
            scan_tainted_op(op, tainted, &live_after_body, wrappers);
            scan_tainted_expr(body, tainted, live_out, wrappers);

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

fn scan_tainted_op(
    op: &AnfOp,
    tainted: &mut HashSet<LocalId>,
    live_out: &HashSet<LocalId>,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) {
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
            if let Some((info, _wrapped_func, resolved_args)) =
                resolve_cow_call(*func_id, args, wrappers)
            {
                for (i, a) in resolved_args.iter().enumerate() {
                    if i != info.base_arg {
                        if let Atom::ALocal(x) = a {
                            tainted.insert(*x);
                        }
                    }
                }
            } else if *func_id == prelude::VECTOR_CONCAT && args.len() == 2 {
                // Keep concat local and conservative: the right-hand side still
                // taints normally, but the left base only needs taint if it
                // remains live after the concat call. This preserves negative
                // cases like vector_set_after_concat while allowing dead-base
                // concat rewrites when the old left value dies immediately.
                if let Atom::ALocal(rhs) = &args[1] {
                    tainted.insert(*rhs);
                }
                if let Atom::ALocal(base) = &args[0] {
                    if live_out.contains(base) {
                        tainted.insert(*base);
                    }
                }
            } else if !call_does_not_retain_args(*func_id) {
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
            scan_tainted_expr(then_branch, tainted, live_out, wrappers);
            scan_tainted_expr(else_branch, tainted, live_out, wrappers);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                scan_tainted_expr(&arm.body, tainted, live_out, wrappers);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            scan_tainted_expr(body, tainted, live_out, wrappers);
        }
        _ => {}
    }
}

// ── Consume-reassign pattern detection ───────────────────────────────────────

/// Checks whether `body` begins with a transparent forward-bind chain ending in
/// `assign(base = current_result)`, e.g.:
///
///   let t2 = init(t1)
///   let _  = assign(base = t2)
fn is_consume_reassign(body: &AnfExpr, base: LocalId, result: LocalId) -> bool {
    let mut current = result;
    let mut cursor = body;
    loop {
        let AnfExpr::Let { local, op, body } = cursor else {
            return false;
        };
        match op.as_ref() {
            AnfOp::AAssign {
                local: target,
                value: Atom::ALocal(v),
            } => return *target == base && *v == current,
            AnfOp::AInit {
                value: Atom::ALocal(v),
            } if *v == current => {
                current = *local;
                cursor = body;
            }
            _ => return false,
        }
    }
}

fn atom_is_local(atom: &Atom, local: LocalId) -> bool {
    matches!(atom, Atom::ALocal(id) if *id == local)
}

/// Returns true when `base` is a fresh record whose remaining uses are limited
/// to non-escaping `ARecordGet`s on fields other than `moved_field`.
///
/// This supports narrow fresh-wrapper destructuring such as:
///   r := alloc_local(ctx)
///   ctx = r.ctx
///   ... r.local ...
///
/// The extracted field may be treated as deeply unique because the wrapper is
/// fresh, never escapes, and the moved field is never read again.
fn can_move_fresh_record_field(base: LocalId, moved_field: FieldId, expr: &AnfExpr) -> bool {
    let mut cursor = expr;
    loop {
        match cursor {
            AnfExpr::Let { op, body, .. } => {
                if op_has_local_in_nested_scope(op, base) {
                    return false;
                }
                match op.as_ref() {
                    AnfOp::ARecordGet {
                        target: Atom::ALocal(source),
                        field,
                        ..
                    } if *source == base => {
                        if *field == moved_field {
                            return false;
                        }
                    }
                    _ if op_uses_local_non_recursive(op, base) => return false,
                    _ => {}
                }
                cursor = body;
            }
            AnfExpr::Atom(atom) => return !atom_is_local(atom, base),
            AnfExpr::Return(Some(atom)) | AnfExpr::Break(Some(atom)) => {
                return !atom_is_local(atom, base);
            }
            AnfExpr::Return(None) | AnfExpr::Break(None) | AnfExpr::Continue => return true,
        }
    }
}

/// Returns true when `field_local` is the base of a COW op whose result is
/// immediately re-stored into a struct via ARecordUpdate, and `field_local` is
/// dead after the COW op.  In that pattern the field has no other live
/// references at the COW site, so it may be treated as unique.
fn is_field_borrow_and_update(
    field_local: LocalId,
    body: &AnfExpr,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) -> bool {
    // Allow transparent lets between the field extraction and the eventual COW
    // op, as long as they do not use or capture the extracted field.
    let mut cursor = body;
    loop {
        let AnfExpr::Let {
            local: new_field,
            op: cow_op,
            body: rest,
        } = cursor
        else {
            return false;
        };
        if let AnfOp::ACall {
            callee: Atom::AGlobalFunc(func_id),
            args,
        } = cow_op.as_ref()
        {
            if let Some((info, _wrapped, resolved_args)) =
                resolve_cow_call(*func_id, args, wrappers)
            {
                // Must have an in-place variant (dict/vector set, not VECTOR_APPEND)
                if info.in_place_id.is_some()
                    && resolved_args
                        .get(info.base_arg)
                        .map_or(false, |a| atom_is_local(a, field_local))
                {
                    // field_local must be dead after this COW op
                    if live_after(rest).contains(&field_local) {
                        return false;
                    }
                    // The updated field value may flow through transparent lets
                    // before the final ARecordUpdate, but must not be used in
                    // any other way.
                    let mut tail = rest.as_ref();
                    loop {
                        match tail {
                            AnfExpr::Let { op: update_op, .. }
                                if matches!(
                                    update_op.as_ref(),
                                    AnfOp::ARecordUpdate {
                                        value: Atom::ALocal(v),
                                        ..
                                    } if *v == *new_field
                                ) =>
                            {
                                return true;
                            }
                            AnfExpr::Let { op, body: next, .. } => {
                                if op_has_local_in_nested_scope(op, *new_field)
                                    || op_uses_local_non_recursive(op, *new_field)
                                {
                                    return false;
                                }
                                tail = next;
                            }
                            _ => return false,
                        }
                    }
                }
            }
        }
        if op_has_local_in_nested_scope(cow_op, field_local)
            || op_uses_local_non_recursive(cow_op, field_local)
        {
            return false;
        }
        cursor = rest;
    }
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

fn analyze_loop_builder_op_subexpr(
    op: &AnfOp,
    base: LocalId,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) -> Option<usize> {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => Some(
            analyze_loop_builder_expr(then_branch, base, wrappers)?
                + analyze_loop_builder_expr(else_branch, base, wrappers)?,
        ),
        AnfOp::AMatch { arms, .. } => {
            let mut sites = 0usize;
            for arm in arms {
                sites += analyze_loop_builder_expr(&arm.body, base, wrappers)?;
            }
            Some(sites)
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            analyze_loop_builder_expr(body, base, wrappers)
        }
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
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) -> bool {
    let Some((info, _wrapped_func, resolved_args)) = resolve_cow_call(func_id, args, wrappers)
    else {
        return false;
    };
    if info.base_arg >= resolved_args.len() {
        return false;
    }
    if !atom_is_local(resolved_args[info.base_arg], base) {
        return false;
    }
    for (i, arg) in resolved_args.iter().enumerate() {
        if i != info.base_arg && atom_is_local(arg, base) {
            return false;
        }
    }
    is_consume_reassign(body, base, result)
}

// ── Dict in-place loop rewrite (simple callee swap, no builder) ─────────────

/// Analyze whether the loop body uses `base` only via in-place-swappable COW ops
/// (dict set/remove). Returns the count of such sites, or None if base is used
/// in any other way (including vector append, which needs builder wrapping).
fn analyze_loop_dict_sites(
    expr: &AnfExpr,
    base: LocalId,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) -> Option<usize> {
    match expr {
        AnfExpr::Let { local, op, body } => {
            if let AnfOp::ACall {
                callee: Atom::AGlobalFunc(func_id),
                args,
            } = op.as_ref()
            {
                if let Some((info, _wrapped_func, _resolved_args)) =
                    resolve_cow_call(*func_id, args, wrappers)
                {
                    if info.in_place_id.is_some()
                        && is_cow_consume_reassign(*func_id, args, body, base, *local, wrappers)
                    {
                        let AnfExpr::Let {
                            body: rest_after_assign,
                            ..
                        } = body.as_ref()
                        else {
                            return None;
                        };
                        return Some(
                            1 + analyze_loop_dict_sites(rest_after_assign, base, wrappers)?,
                        );
                    }
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
            Some(
                analyze_loop_dict_sites_in_op(op, base, wrappers)?
                    + analyze_loop_dict_sites(body, base, wrappers)?,
            )
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

fn analyze_loop_dict_sites_in_op(
    op: &AnfOp,
    base: LocalId,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) -> Option<usize> {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => Some(
            analyze_loop_dict_sites(then_branch, base, wrappers)?
                + analyze_loop_dict_sites(else_branch, base, wrappers)?,
        ),
        AnfOp::AMatch { arms, .. } => {
            let mut sites = 0usize;
            for arm in arms {
                sites += analyze_loop_dict_sites(&arm.body, base, wrappers)?;
            }
            Some(sites)
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            analyze_loop_dict_sites(body, base, wrappers)
        }
        _ => Some(0),
    }
}

/// Rewrite in-place-swappable COW ops in a loop body by swapping the callee ID.
fn rewrite_loop_dict_expr(
    expr: &mut AnfExpr,
    base: LocalId,
    sites: &mut usize,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) {
    let AnfExpr::Let { local, op, body } = expr else {
        return;
    };

    if let AnfOp::ACall {
        callee: Atom::AGlobalFunc(func_id),
        args,
    } = op.as_mut()
    {
        if let Some((info, _wrapped_func, resolved_args)) =
            resolve_cow_call(*func_id, args, wrappers)
        {
            if let Some(in_place_id) = info.in_place_id {
                if info.base_arg < resolved_args.len()
                    && atom_is_local(resolved_args[info.base_arg], base)
                    && is_consume_reassign(body, base, *local)
                {
                    *func_id = in_place_id;
                    *sites += 1;
                    if let AnfExpr::Let {
                        body: rest_after_assign,
                        ..
                    } = body.as_mut()
                    {
                        rewrite_loop_dict_expr(rest_after_assign, base, sites, wrappers);
                        return;
                    }
                }
            }
        }
    }

    rewrite_loop_dict_op_subexpr(op, base, sites, wrappers);
    rewrite_loop_dict_expr(body, base, sites, wrappers);
}

fn rewrite_loop_dict_op_subexpr(
    op: &mut AnfOp,
    base: LocalId,
    sites: &mut usize,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_loop_dict_expr(then_branch, base, sites, wrappers);
            rewrite_loop_dict_expr(else_branch, base, sites, wrappers);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                rewrite_loop_dict_expr(&mut arm.body, base, sites, wrappers);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            rewrite_loop_dict_expr(body, base, sites, wrappers);
        }
        _ => {}
    }
}

// ── Straight-line vector builder rewrite ─────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuilderRegionStep {
    Push,
    Extend,
}

fn match_builder_region_step(
    func_id: FuncId,
    args: &[Atom],
    body: &AnfExpr,
    base: LocalId,
    result: LocalId,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) -> Option<BuilderRegionStep> {
    if let Some((info, _wrapped_func, resolved_args)) = resolve_cow_call(func_id, args, wrappers) {
        if info.builder_rewritable
            && resolved_args.len() == 2
            && atom_is_local(resolved_args[0], base)
            && !atom_is_local(resolved_args[1], base)
            && is_consume_reassign(body, base, result)
        {
            return Some(BuilderRegionStep::Push);
        }
    }

    if func_id == prelude::VECTOR_CONCAT
        && args.len() == 2
        && atom_is_local(&args[0], base)
        && !atom_is_local(&args[1], base)
        && is_consume_reassign(body, base, result)
    {
        return Some(BuilderRegionStep::Extend);
    }

    None
}

/// Returns true if the base local is eligible to start a builder region
/// (builder_new/from → push/extend → freeze → assign pattern).
///
/// When `source_fresh` is provided (Some), it acts as an extra bypass that
/// allows freshly-allocated but tainted locals to participate. This bypass
/// should only be used by the **detection** functions (detect_spine_builder_base,
/// detect_dead_concat_base, loop builder candidate filter). It must NOT be used
/// when checking `can_preserve_builder_uniqueness` inside the regular COW
/// single-step rewrite, because that path propagates uniqueness through the
/// assign even when no builder region will actually be created, which can
/// incorrectly enable downstream in-place vector-set ops after an opaque call.
fn base_can_start_builder_region(
    base: LocalId,
    tainted: &HashSet<LocalId>,
    unique: &HashSet<LocalId>,
    known_empty: &HashSet<LocalId>,
    refreshed: &HashSet<LocalId>,
    builder_safe: &HashSet<LocalId>,
    source_fresh: Option<&HashSet<LocalId>>,
) -> bool {
    unique.contains(&base)
        && ((!tainted.contains(&base) || refreshed.contains(&base))
            || known_empty.contains(&base)
            || builder_safe.contains(&base)
            || source_fresh.map_or(false, |sf| sf.contains(&base)))
}

/// Check if the current expr starts a straight-line builder region:
/// `xs = xs.append(v)` and/or `xs = xs.concat(rhs)` consume-reassign chains.
/// Base must be uniqueness-safe for builder_from/new.
fn detect_spine_builder_base(
    expr: &AnfExpr,
    tainted: &HashSet<LocalId>,
    unique: &HashSet<LocalId>,
    known_empty: &HashSet<LocalId>,
    refreshed: &HashSet<LocalId>,
    builder_safe: &HashSet<LocalId>,
    source_fresh: &HashSet<LocalId>,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) -> Option<LocalId> {
    let AnfExpr::Let { local, op, body } = expr else {
        return None;
    };
    let AnfOp::ACall {
        callee: Atom::AGlobalFunc(func_id),
        args,
    } = op.as_ref()
    else {
        return None;
    };

    let base = match resolve_cow_call(*func_id, args, wrappers) {
        Some((info, _wrapped_func, resolved_args)) if info.builder_rewritable => {
            let Atom::ALocal(base) = resolved_args[0] else {
                return None;
            };
            *base
        }
        _ if *func_id == prelude::VECTOR_CONCAT && args.len() == 2 => {
            let Atom::ALocal(base) = &args[0] else {
                return None;
            };
            *base
        }
        _ => return None,
    };

    if !base_can_start_builder_region(
        base,
        tainted,
        unique,
        known_empty,
        refreshed,
        builder_safe,
        Some(source_fresh),
    ) {
        return None;
    }
    match_builder_region_step(*func_id, args, body, base, *local, wrappers)?;
    Some(base)
}

/// Count mixed append/concat consume-reassign builder sites on the spine.
/// Returns the number of rewriteable steps until a disqualifying use of `base`
/// or control-flow boundary is encountered.
fn count_spine_builder_steps(
    expr: &AnfExpr,
    base: LocalId,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) -> usize {
    let mut count = 0;
    let mut cursor = expr;
    loop {
        let AnfExpr::Let { local, op, body } = cursor else {
            break;
        };
        if let AnfOp::ACall {
            callee: Atom::AGlobalFunc(func_id),
            args,
        } = op.as_ref()
        {
            if match_builder_region_step(*func_id, args, body, base, *local, wrappers).is_some() {
                count += 1;
                if let AnfExpr::Let { body: rest, .. } = body.as_ref() {
                    cursor = rest;
                    continue;
                }
                break;
            }
        }
        if op_uses_local_non_recursive(op, base) {
            let is_read_only = matches!(
                op.as_ref(),
                AnfOp::ACall {
                    callee: Atom::AGlobalFunc(fid),
                    ..
                } if is_no_retain_read_only(*fid)
            );
            if !is_read_only {
                break;
            }
        }
        if op_has_local_in_nested_scope(op, base) {
            break;
        }
        cursor = body;
    }
    count
}

fn spine_builder_region_has_extend(
    expr: &AnfExpr,
    base: LocalId,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) -> bool {
    let mut cursor = expr;
    loop {
        let AnfExpr::Let { local, op, body } = cursor else {
            return false;
        };
        if let AnfOp::ACall {
            callee: Atom::AGlobalFunc(func_id),
            args,
        } = op.as_ref()
        {
            if let Some(step) =
                match_builder_region_step(*func_id, args, body, base, *local, wrappers)
            {
                if step == BuilderRegionStep::Extend {
                    return true;
                }
                if let AnfExpr::Let { body: rest, .. } = body.as_ref() {
                    cursor = rest;
                    continue;
                }
                return false;
            }
        }
        if op_uses_local_non_recursive(op, base) {
            let is_read_only = matches!(
                op.as_ref(),
                AnfOp::ACall {
                    callee: Atom::AGlobalFunc(fid),
                    ..
                } if is_no_retain_read_only(*fid)
            );
            if !is_read_only {
                return false;
            }
        }
        if op_has_local_in_nested_scope(op, base) {
            return false;
        }
        cursor = body;
    }
}

fn detect_dead_concat_base(
    expr: &AnfExpr,
    tainted: &HashSet<LocalId>,
    unique: &HashSet<LocalId>,
    known_empty: &HashSet<LocalId>,
    refreshed: &HashSet<LocalId>,
    builder_safe: &HashSet<LocalId>,
    source_fresh: &HashSet<LocalId>,
) -> Option<(LocalId, LocalId, Atom)> {
    let AnfExpr::Let { local, op, body } = expr else {
        return None;
    };
    let AnfOp::ACall {
        callee: Atom::AGlobalFunc(func_id),
        args,
    } = op.as_ref()
    else {
        return None;
    };
    if *func_id != prelude::VECTOR_CONCAT || args.len() != 2 {
        return None;
    }
    let Atom::ALocal(base) = args[0] else {
        return None;
    };
    if atom_is_local(&args[1], base) {
        return None;
    }
    // Allow the consume-reassign case: `xs = xs.concat(rhs)` where `xs`
    // appears only as the immediate assign target (not read).
    if expr_mentions_local(body, base) && !is_consume_reassign(body, base, *local) {
        return None;
    }
    if !base_can_start_builder_region(
        base,
        tainted,
        unique,
        known_empty,
        refreshed,
        builder_safe,
        Some(source_fresh),
    ) {
        return None;
    }
    Some((base, *local, args[1].clone()))
}

/// Rewrite mixed append/concat consume-reassign sites on the spine to
/// builder_push/builder_extend plus noop assign. Skips intervening non-step ops.
fn rewrite_spine_builder_steps(
    mut expr: &mut AnfExpr,
    base: LocalId,
    builder: LocalId,
    sites: &mut usize,
    target: usize,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) {
    loop {
        if *sites >= target {
            return;
        }
        let AnfExpr::Let { local, op, body } = expr else {
            return;
        };
        if let AnfOp::ACall {
            callee: Atom::AGlobalFunc(func_id),
            args,
        } = op.as_mut()
        {
            if let Some(step) =
                match_builder_region_step(*func_id, args, body, base, *local, wrappers)
            {
                *func_id = match step {
                    BuilderRegionStep::Push => prelude::VECTOR_BUILDER_PUSH,
                    BuilderRegionStep::Extend => prelude::VECTOR_BUILDER_EXTEND,
                };
                args[0] = Atom::ALocal(builder);
                if let AnfExpr::Let {
                    op: assign_op,
                    body: rest,
                    ..
                } = body.as_mut()
                {
                    *assign_op = Box::new(AnfOp::AInit {
                        value: Atom::ALitVoid,
                    });
                    *sites += 1;
                    expr = rest;
                    continue;
                }
                return;
            }
        }
        expr = body;
    }
}

fn is_builder_step(op: &AnfOp) -> bool {
    matches!(
        op,
        AnfOp::ACall {
            callee: Atom::AGlobalFunc(fid),
            ..
        } if *fid == prelude::VECTOR_BUILDER_PUSH || *fid == prelude::VECTOR_BUILDER_EXTEND
    )
}

/// Walk spine to find the insertion point after the last builder step.
/// This is where we'll splice in the FREEZE + assign.
fn spine_tail(expr: &mut AnfExpr) -> &mut AnfExpr {
    // First pass: count how many spine nodes to skip.
    let skip = count_builder_chain_len(expr);
    // Second pass: advance `skip` nodes.
    let mut cursor = expr;
    for _ in 0..skip {
        let AnfExpr::Let { body, .. } = cursor else {
            break;
        };
        cursor = body;
    }
    cursor
}

/// Count spine Let nodes that belong to the rewritten push chain
/// (push ops, noop assigns, and intervening non-push ops before the next push).
fn count_builder_chain_len(expr: &AnfExpr) -> usize {
    let mut count = 0;
    let mut cursor = expr;
    loop {
        let AnfExpr::Let { op, body, .. } = cursor else {
            break;
        };
        if is_builder_step(op.as_ref()) {
            // Count the builder step + its noop assign (2 nodes)
            count += 1;
            if let AnfExpr::Let { body: rest, .. } = body.as_ref() {
                count += 1;
                cursor = rest;
                continue;
            }
            break;
        }
        // Check for noop assign (leftover from rewrite)
        let is_noop = matches!(
            op.as_ref(),
            AnfOp::AInit {
                value: Atom::ALitVoid
            }
        );
        if is_noop {
            count += 1;
            cursor = body;
            continue;
        }
        // Intervening op — only skip if there's a builder step further downstream
        if has_builder_step_downstream(body) {
            count += 1;
            cursor = body;
            continue;
        }
        break;
    }
    count
}

fn has_builder_step_downstream(expr: &AnfExpr) -> bool {
    let mut cursor = expr;
    loop {
        let AnfExpr::Let { op, body, .. } = cursor else {
            return false;
        };
        if is_builder_step(op.as_ref()) {
            return true;
        }
        cursor = body;
    }
}

// ── Vector builder loop rewrite ─────────────────────────────────────────────

fn analyze_loop_builder_expr(
    expr: &AnfExpr,
    base: LocalId,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) -> Option<usize> {
    match expr {
        AnfExpr::Let { local, op, body } => {
            if let AnfOp::ACall {
                callee: Atom::AGlobalFunc(func_id),
                args,
            } = op.as_ref()
            {
                if match_builder_region_step(*func_id, args, body, base, *local, wrappers).is_some()
                {
                    let AnfExpr::Let {
                        body: rest_after_assign,
                        ..
                    } = body.as_ref()
                    else {
                        return None;
                    };
                    return Some(1 + analyze_loop_builder_expr(rest_after_assign, base, wrappers)?);
                }
            }

            if op_uses_local_non_recursive(op, base) {
                return None;
            }
            Some(
                analyze_loop_builder_op_subexpr(op, base, wrappers)?
                    + analyze_loop_builder_expr(body, base, wrappers)?,
            )
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

fn rewrite_loop_builder_op_subexpr(
    op: &mut AnfOp,
    base: LocalId,
    builder: LocalId,
    sites: &mut usize,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_loop_builder_expr(then_branch, base, builder, sites, wrappers);
            rewrite_loop_builder_expr(else_branch, base, builder, sites, wrappers);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                rewrite_loop_builder_expr(&mut arm.body, base, builder, sites, wrappers);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            rewrite_loop_builder_expr(body, base, builder, sites, wrappers);
        }
        _ => {}
    }
}

fn rewrite_loop_builder_expr(
    expr: &mut AnfExpr,
    base: LocalId,
    builder: LocalId,
    sites: &mut usize,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) {
    let AnfExpr::Let { local, op, body } = expr else {
        return;
    };

    if let AnfOp::ACall {
        callee: Atom::AGlobalFunc(func_id),
        args,
    } = op.as_mut()
    {
        if let Some(step) = match_builder_region_step(*func_id, args, body, base, *local, wrappers)
        {
            *func_id = match step {
                BuilderRegionStep::Push => prelude::VECTOR_BUILDER_PUSH,
                BuilderRegionStep::Extend => prelude::VECTOR_BUILDER_EXTEND,
            };
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
                rewrite_loop_builder_expr(rest_after_assign, base, builder, sites, wrappers);
                return;
            }
        }
    }

    rewrite_loop_builder_op_subexpr(op, base, builder, sites, wrappers);
    rewrite_loop_builder_expr(body, base, builder, sites, wrappers);
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
pub fn uniqueness_rewrite(
    func: &mut AnfFunctionDef,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) {
    let tainted = collect_tainted(func, wrappers);
    let mut unique = HashSet::new();
    let mut known_empty = HashSet::new();
    let mut refreshed = HashSet::new();
    let mut builder_safe = HashSet::new();
    let mut source_fresh = HashSet::new();
    let mut next_local = next_local_id(func);
    rewrite_expr(
        &mut func.body,
        &tainted,
        &mut unique,
        &mut known_empty,
        &mut refreshed,
        &mut builder_safe,
        &mut source_fresh,
        &mut next_local,
        wrappers,
    );
}

fn rewrite_expr(
    expr: &mut AnfExpr,
    tainted: &HashSet<LocalId>,
    unique: &mut HashSet<LocalId>,
    known_empty: &mut HashSet<LocalId>,
    refreshed: &mut HashSet<LocalId>,
    builder_safe: &mut HashSet<LocalId>,
    source_fresh: &mut HashSet<LocalId>,
    next_local: &mut u32,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) {
    // Straight-line builder rewrite (Phase F):
    // Sequential `xs = xs.append(v)` chains on the spine → builder_from/push/freeze
    // when xs is unique, non-escaped, and there are ≥2 appends.
    // Must run before destructuring expr to avoid borrow conflicts.
    if let Some(base) = detect_spine_builder_base(
        expr,
        tainted,
        unique,
        known_empty,
        refreshed,
        builder_safe,
        source_fresh,
        wrappers,
    ) {
        let total_steps = count_spine_builder_steps(expr, base, wrappers);
        let has_extend = spine_builder_region_has_extend(expr, base, wrappers);
        if total_steps >= 2 || has_extend {
            let builder_local = alloc_local(next_local);
            let freeze_local = alloc_local(next_local);
            let assign_local = alloc_local(next_local);
            let use_builder_new = known_empty.contains(&base);

            let mut rewritten_sites = 0usize;
            rewrite_spine_builder_steps(
                expr,
                base,
                builder_local,
                &mut rewritten_sites,
                total_steps,
                wrappers,
            );
            if rewritten_sites == total_steps {
                // Splice freeze+assign after the last builder step
                let tail = spine_tail(expr);
                let old_tail = std::mem::replace(tail, AnfExpr::Atom(Atom::ALitVoid));
                *tail = AnfExpr::Let {
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
                        body: Box::new(old_tail),
                    }),
                };

                // Wrap with builder_from/new at the top
                let inner = std::mem::replace(expr, AnfExpr::Atom(Atom::ALitVoid));
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
                    body: Box::new(inner),
                };

                rewrite_expr(
                    expr,
                    tainted,
                    unique,
                    known_empty,
                    refreshed,
                    builder_safe,
                    source_fresh,
                    next_local,
                    wrappers,
                );
                return;
            }
        }
    }

    if let Some((base, result_local, rhs)) = detect_dead_concat_base(
        expr,
        tainted,
        unique,
        known_empty,
        refreshed,
        builder_safe,
        source_fresh,
    ) {
        let builder_local = alloc_local(next_local);
        let step_local = alloc_local(next_local);
        let freeze_local = alloc_local(next_local);
        let use_builder_new = known_empty.contains(&base);

        let old_expr = std::mem::replace(expr, AnfExpr::Atom(Atom::ALitVoid));
        let AnfExpr::Let { body, .. } = old_expr else {
            unreachable!();
        };
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
                local: step_local,
                op: Box::new(AnfOp::ACall {
                    callee: Atom::AGlobalFunc(prelude::VECTOR_BUILDER_EXTEND),
                    args: vec![Atom::ALocal(builder_local), rhs],
                }),
                body: Box::new(AnfExpr::Let {
                    local: freeze_local,
                    op: Box::new(AnfOp::ACall {
                        callee: Atom::AGlobalFunc(prelude::VECTOR_BUILDER_FREEZE),
                        args: vec![Atom::ALocal(builder_local)],
                    }),
                    body: Box::new(AnfExpr::Let {
                        local: result_local,
                        op: Box::new(AnfOp::AInit {
                            value: Atom::ALocal(freeze_local),
                        }),
                        body,
                    }),
                }),
            }),
        };
        rewrite_expr(
            expr,
            tainted,
            unique,
            known_empty,
            refreshed,
            builder_safe,
            source_fresh,
            next_local,
            wrappers,
        );
        return;
    }

    let AnfExpr::Let { local, op, body } = expr else {
        return;
    };
    let bind_local = *local;

    // Region rewrite (Phase 3):
    // Loop accumulator `xs = xs.append(v)` -> builder_new/push/freeze wrapping
    // when `xs` is unique and either not tainted OR known-empty.
    //
    // The known-empty relaxation is safe because:
    // - known_empty proves the local was just allocated (e.g. `xs: Vector<T> = []`)
    // - any taint is from a future escape on the spine (stored in record, returned)
    // - builder_new() is used (not builder_from), so no alias is affected
    // - after freeze, the local is reassigned to the frozen result
    // - subsequent code (including the escape) sees the frozen vector, not the builder
    if let AnfOp::ALoop { body: loop_body } = op.as_ref() {
        let mut candidates = unique
            .iter()
            .copied()
            .filter(|id| {
                base_can_start_builder_region(
                    *id,
                    tainted,
                    unique,
                    known_empty,
                    refreshed,
                    builder_safe,
                    Some(source_fresh),
                )
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|id| id.0);

        for base in candidates {
            let Some(expected_sites) = analyze_loop_builder_expr(loop_body, base, wrappers) else {
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
            rewrite_loop_builder_expr(
                &mut rewritten_loop_body,
                base,
                builder_local,
                &mut rewritten_sites,
                wrappers,
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
            rewrite_expr(
                expr,
                tainted,
                unique,
                known_empty,
                refreshed,
                builder_safe,
                source_fresh,
                next_local,
                wrappers,
            );
            return;
        }

        // Dict in-place loop rewrite: for COW ops with a direct in-place swap
        // (no builder lifecycle needed), just swap the callee inside the loop.
        // Require source_fresh (deep ownership) — same gate as point rewrites.
        for base in unique
            .iter()
            .copied()
            .filter(|id| source_fresh.contains(id))
            .collect::<Vec<_>>()
        {
            let Some(expected_sites) = analyze_loop_dict_sites(loop_body, base, wrappers) else {
                continue;
            };
            if expected_sites == 0 {
                continue;
            }

            let mut rewritten_loop_body = (*loop_body.as_ref()).clone();
            let mut rewritten_sites = 0usize;
            rewrite_loop_dict_expr(
                &mut rewritten_loop_body,
                base,
                &mut rewritten_sites,
                wrappers,
            );
            if rewritten_sites == 0 || rewritten_sites != expected_sites {
                continue;
            }

            *op = Box::new(AnfOp::ALoop {
                body: Box::new(rewritten_loop_body),
            });
            // Process the loop op (this clears source_fresh for
            // assigned-in-loop locals in the ALoop handler).
            rewrite_op(
                op,
                tainted,
                unique,
                known_empty,
                refreshed,
                builder_safe,
                source_fresh,
                next_local,
                wrappers,
            );
            // After a successful in-place loop rewrite, the base's tree
            // nodes are all uniquely owned (built/modified in place from a
            // fresh root). Restore deep ownership so downstream loops
            // (e.g. a remove loop after a build loop) can also fire.
            source_fresh.insert(base);
            // Process the continuation after the loop.
            rewrite_expr(
                body,
                tainted,
                unique,
                known_empty,
                refreshed,
                builder_safe,
                source_fresh,
                next_local,
                wrappers,
            );
            return;
        }
    }

    // Track fresh producers → Unique + source_fresh.
    let returns_fresh_record = matches!(
        op.as_ref(),
        AnfOp::ACall {
            callee: Atom::AGlobalFunc(func_id),
            ..
        } if wrappers
            .get(func_id)
            .map_or(false, |summary| summary.returns_fresh_record)
    );
    if is_fresh_producer(op) || returns_fresh_record {
        unique.insert(bind_local);
        if tainted.contains(&bind_local) {
            builder_safe.insert(bind_local);
        }
        // source_fresh: any freshly-allocated value (array literal, Dict.new,
        // builder freeze, etc.) is locally owned until either aliased via init
        // or passed to an opaque function.  The set is checked (optionally) in
        // base_can_rewrite to allow the first in-place COW op on a fresh local
        // even when it's globally tainted.  Only cleared by AAssign / AInit
        // alias / opaque-call arg / loop-assign.
        source_fresh.insert(bind_local);
    }
    if let AnfOp::AArrayLit(elems) = op.as_ref() {
        if elems.is_empty() {
            known_empty.insert(bind_local);
        }
        // Locals stored inside the array are escaped into the container.
        // Clear source_fresh so they are no longer considered unaliased.
        for a in elems {
            if let Atom::ALocal(x) = a {
                source_fresh.remove(x);
            }
        }
    }
    // Similarly clear source_fresh for locals stored in records, variants,
    // and closure captures.
    if let AnfOp::ARecord { fields, .. } = op.as_ref() {
        for (_, a) in fields {
            if let Atom::ALocal(x) = a {
                source_fresh.remove(x);
            }
        }
    }
    if let AnfOp::AVariant { args, .. } = op.as_ref() {
        for a in args {
            if let Atom::ALocal(x) = a {
                source_fresh.remove(x);
            }
        }
    }
    if let AnfOp::AMakeClosure { free_vars, .. } = op.as_ref() {
        for x in free_vars {
            source_fresh.remove(x);
        }
    }

    // AInit uniqueness transfer: let x = init(y) where y is unique → x is unique
    // (y is moved to x; y should no longer be considered unique)
    if let AnfOp::AInit {
        value: Atom::ALocal(source),
    } = op.as_ref()
    {
        if unique.contains(source) && (!tainted.contains(source) || builder_safe.contains(source)) {
            // Transfer: source dies (moved), target becomes unique.
            // `builder_safe` sources are allowed here even when globally tainted:
            // the taint is only from a later escape, while the current value is
            // still a locally-owned vector builder candidate.
            unique.remove(source);
            unique.insert(bind_local);
            if tainted.contains(&bind_local) && builder_safe.contains(source) {
                builder_safe.insert(bind_local);
            }
            // Transfer source_fresh: the freshly-allocated value moves with the
            // local. The source is dead after this point.
            if source_fresh.contains(source) {
                source_fresh.remove(source);
                source_fresh.insert(bind_local);
            }
        } else {
            // Transfer failed (source is tainted and not builder_safe = aliased,
            // or source is not unique). The source may now be aliased by
            // bind_local; clear source_fresh from the source since we can no
            // longer guarantee unique ownership of the value.
            source_fresh.remove(source);
            if refreshed.contains(source) && live_after(body).contains(source) {
                // Source was refreshed (held a fresh value) but is now aliased.
                // Clear refreshed so downstream checks don't bypass the taint guard.
                refreshed.remove(source);
            }
        }
        if known_empty.contains(source) && !tainted.contains(source) {
            known_empty.remove(source);
            known_empty.insert(bind_local);
        }
        if builder_safe.contains(source) && !tainted.contains(source) {
            builder_safe.remove(source);
            builder_safe.insert(bind_local);
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
        refreshed.remove(target);
        builder_safe.remove(target);
        source_fresh.remove(target);
        if let Atom::ALocal(source) = value {
            if unique.contains(source)
                && (!tainted.contains(source) || builder_safe.contains(source))
            {
                // Treat as move when source is not tainted, or when the source
                // is in `builder_safe` and therefore only tainted by a later
                // escape of a locally-owned vector value.
                unique.remove(source);
                unique.insert(*target);
                // If target is tainted (e.g., a parameter), mark it as
                // refreshed — it now holds a fresh value, not the original
                // tainted one. This enables downstream ARecordUpdate to
                // use can_reuse_in_place even though tainted still contains it.
                if tainted.contains(target) {
                    refreshed.insert(*target);
                    builder_safe.insert(*target);
                }
            } else if unique.contains(source) {
                // Transfer failed: source is tainted and not builder_safe.
                // The assign aliases source into target; source is no longer
                // uniquely owned. Clear unique so downstream ops don't
                // incorrectly treat source as unaliased.
                // (Before source_fresh, base_can_rewrite would still fail
                // via the !tainted||refreshed check, but source_fresh bypasses
                // that check so we must clear unique explicitly here.)
                unique.remove(source);
                source_fresh.remove(source);
            }
            if source_fresh.contains(source) {
                // Transfer deep ownership: the freshly-allocated value moves
                // from source to target. Only transfer when the uniqueness
                // move above succeeded (source is no longer in unique); if it
                // failed, the alias means neither side is deeply owned.
                if unique.contains(target) {
                    source_fresh.remove(source);
                    source_fresh.insert(*target);
                }
            }
            if known_empty.contains(source) && !tainted.contains(source) {
                known_empty.remove(source);
                known_empty.insert(*target);
            }
            if builder_safe.contains(source) && !tainted.contains(source) {
                builder_safe.remove(source);
                builder_safe.insert(*target);
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
        // Allow in-place when base is unique AND either:
        //   (a) base is not tainted (original rule), or
        //   (b) base is tainted but was refreshed — i.e., reassigned from a
        //       fresh source (record update result or COW result) since the
        //       taint-causing event. The taint is about the original parameter
        //       value, not the current one.
        // Record update: source_fresh is not applicable here (records aren't
        // dict/vector containers with an in_place_id in the same sense).
        if base_can_rewrite(*base, tainted, unique, refreshed, None) && can_rewrite {
            *can_reuse_in_place = true;
        }
    }

    // ARecordGet uniqueness propagation (Gap 1 — interproc-uniqueness.md):
    // When extracting a field from a unique/refreshed struct that dies immediately
    // after the extraction, the extracted value is the sole reference to that field.
    // Mark it Shallow-unique: eligible for record shell reuse (can_reuse_in_place)
    // but NOT for deep collection in-place mutation (source_fresh is intentionally
    // not set, so dict_set_in_place / vector_set_in_place remain conservatively gated).
    if let AnfOp::ARecordGet {
        target: Atom::ALocal(base),
        field,
        ..
    } = op.as_ref()
    {
        let base = *base;
        if (unique.contains(&base) || refreshed.contains(&base))
            && !live_after(body).contains(&base)
        {
            unique.insert(bind_local);
            if tainted.contains(&bind_local) {
                refreshed.insert(bind_local);
            }
        } else if source_fresh.contains(&base) && can_move_fresh_record_field(base, *field, body) {
            // Fresh-wrapper destructuring: the base is a fresh record and its
            // remaining uses are limited to disjoint non-escaping field reads.
            // Transfer deep uniqueness to the extracted field, then consume the
            // base's source_fresh marker so only one field gets this treatment.
            unique.insert(bind_local);
            source_fresh.remove(&base);
            if tainted.contains(&bind_local) {
                refreshed.insert(bind_local);
            }
        } else if base_can_rewrite(base, tainted, unique, refreshed, None)
            && is_field_borrow_and_update(bind_local, body, wrappers)
        {
            // Field-borrow pattern: the struct is unique (no aliases), and the
            // extracted field is immediately consumed by a COW op then stored
            // back via ARecordUpdate.  Treat the extracted field as unique so
            // the COW op can use its in-place variant.
            unique.insert(bind_local);
        }
    }

    // Check for COW rewrite / uniqueness-propagation opportunity
    if let AnfOp::ACall {
        callee: Atom::AGlobalFunc(func_id),
        args,
    } = op.as_mut()
    {
        if let Some((info, _wrapped_func, resolved_args)) =
            resolve_cow_call(*func_id, args, wrappers)
        {
            if let Some(Atom::ALocal(base)) = resolved_args.get(info.base_arg) {
                let base = *base;
                let can_rewrite = is_consume_reassign(body, base, bind_local)
                    || !live_after(body).contains(&base);
                // Dict/vector in-place rewrites (those with `in_place_id`) require
                // deep ownership: the base must be in `source_fresh`, proving
                // all reachable container nodes are uniquely owned (not just the
                // top-level reference). Without this, a dict extracted from a
                // dying record or produced by fresh-if-copied COW on a shared
                // base could have shared HAMT/trie nodes, making in-place
                // mutation observable through aliases.
                let can_reuse_in_place_base = if info.in_place_id.is_some() {
                    unique.contains(&base) && source_fresh.contains(&base)
                } else {
                    base_can_rewrite(base, tainted, unique, refreshed, None)
                };
                // Do NOT pass source_fresh to base_can_start_builder_region here.
                let can_preserve_builder_uniqueness = info.builder_rewritable
                    && base_can_start_builder_region(
                        base,
                        tainted,
                        unique,
                        known_empty,
                        refreshed,
                        builder_safe,
                        None,
                    );
                if can_rewrite && (can_reuse_in_place_base || can_preserve_builder_uniqueness) {
                    if can_reuse_in_place_base {
                        if let Some(in_place_id) = info.in_place_id {
                            *func_id = in_place_id;
                        }
                    }
                    // Result inherits uniqueness from the consuming update.
                    unique.insert(bind_local);
                    // Propagate deep ownership: the consumed base's tree nodes
                    // are now owned by the result. This applies to all consuming
                    // COW ops (dict_set, vector_append, etc.), enabling downstream
                    // in-place ops on the result.
                    if source_fresh.contains(&base) {
                        source_fresh.insert(bind_local);
                    }
                    if tainted.contains(&bind_local) && can_preserve_builder_uniqueness {
                        builder_safe.insert(bind_local);
                    }
                    // Any consuming update may change container cardinality/content.
                    known_empty.remove(&bind_local);
                } else if info.fresh_if_copied {
                    // Phase B: base is tainted or not unique, but COW ops with
                    // in-place rewrites guarantee a fresh result (full copy).
                    // Mark result as unique for downstream ops, enabling
                    // subsequent updates on the fresh copy to be in-place.
                    let is_consumed = is_consume_reassign(body, base, bind_local)
                        || !live_after(body).contains(&base);
                    if is_consumed && !tainted.contains(&bind_local) {
                        unique.insert(bind_local);
                        known_empty.remove(&bind_local);
                    }
                }
            }
        } else if *func_id == prelude::VECTOR_CONCAT && args.len() == 2 {
            // Concat always produces a fresh vector result even though we keep
            // it out of the generic one-base COW table. Recording freshness on
            // the result local lets consume-reassign sites refresh a tainted
            // accumulator without changing the conservative treatment of the
            // original left base in unrelated patterns like vector_set_after_concat.
            if !tainted.contains(&bind_local) {
                unique.insert(bind_local);
                known_empty.remove(&bind_local);
            }
            // Conservatively clear source_fresh for the left base when it is
            // tainted (i.e., the pre-scan determined it may be aliased or live
            // after the concat). Without this, source_fresh would bypass the
            // taint guard and allow incorrect in-place ops on the base after
            // the concat, violating the vector_set_after_concat negative invariant.
            if let Atom::ALocal(base) = &args[0] {
                if tainted.contains(base) {
                    source_fresh.remove(base);
                }
            }
        } else if !call_does_not_retain_args(*func_id) {
            // Opaque call: the function may retain any of its arguments.
            // Clear source_fresh for all arg locals so subsequent direct
            // in-place ops don't incorrectly treat them as uniquely owned.
            for arg in args.iter() {
                if let Atom::ALocal(local) = arg {
                    source_fresh.remove(local);
                }
            }
        }
    }
    // Indirect call (closure/fn-value call): all args may be retained.
    if let AnfOp::ACall { callee, args } = op.as_ref() {
        if !matches!(callee, Atom::AGlobalFunc(_)) {
            for arg in args {
                if let Atom::ALocal(local) = arg {
                    source_fresh.remove(local);
                }
            }
        }
    }

    // Recurse into op sub-expressions (branches, loops)
    rewrite_op(
        op,
        tainted,
        unique,
        known_empty,
        refreshed,
        builder_safe,
        source_fresh,
        next_local,
        wrappers,
    );
    // Continue with body
    rewrite_expr(
        body,
        tainted,
        unique,
        known_empty,
        refreshed,
        builder_safe,
        source_fresh,
        next_local,
        wrappers,
    );
}

/// Collect all `AAssign` target locals in an expression tree.
fn collect_assign_targets(expr: &AnfExpr) -> HashSet<LocalId> {
    let mut targets = HashSet::new();
    collect_assign_targets_expr(expr, &mut targets);
    targets
}

fn collect_assign_targets_expr(expr: &AnfExpr, targets: &mut HashSet<LocalId>) {
    let AnfExpr::Let { op, body, .. } = expr else {
        return;
    };
    if let AnfOp::AAssign { local, .. } = op.as_ref() {
        targets.insert(*local);
    }
    collect_assign_targets_op(op, targets);
    collect_assign_targets_expr(body, targets);
}

fn collect_assign_targets_op(op: &AnfOp, targets: &mut HashSet<LocalId>) {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            collect_assign_targets_expr(then_branch, targets);
            collect_assign_targets_expr(else_branch, targets);
        }
        AnfOp::AMatch { arms, .. } => {
            for arm in arms {
                collect_assign_targets_expr(&arm.body, targets);
            }
        }
        AnfOp::ALoop { body } | AnfOp::ADefer(body) => {
            collect_assign_targets_expr(body, targets);
        }
        _ => {}
    }
}

fn intersect_in_place(dst: &mut HashSet<LocalId>, other: &HashSet<LocalId>) {
    dst.retain(|id| other.contains(id));
}

fn rewrite_op(
    op: &mut AnfOp,
    tainted: &HashSet<LocalId>,
    unique: &mut HashSet<LocalId>,
    known_empty: &mut HashSet<LocalId>,
    refreshed: &mut HashSet<LocalId>,
    builder_safe: &mut HashSet<LocalId>,
    source_fresh: &mut HashSet<LocalId>,
    next_local: &mut u32,
    wrappers: &HashMap<FuncId, TinyWrapperSummary>,
) {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
            let mut then_unique = unique.clone();
            let mut else_unique = unique.clone();
            let mut then_empty = known_empty.clone();
            let mut else_empty = known_empty.clone();
            let mut then_refreshed = refreshed.clone();
            let mut else_refreshed = refreshed.clone();
            let mut then_builder_safe = builder_safe.clone();
            let mut else_builder_safe = builder_safe.clone();
            let mut then_source_fresh = source_fresh.clone();
            let mut else_source_fresh = source_fresh.clone();
            rewrite_expr(
                then_branch,
                tainted,
                &mut then_unique,
                &mut then_empty,
                &mut then_refreshed,
                &mut then_builder_safe,
                &mut then_source_fresh,
                next_local,
                wrappers,
            );
            rewrite_expr(
                else_branch,
                tainted,
                &mut else_unique,
                &mut else_empty,
                &mut else_refreshed,
                &mut else_builder_safe,
                &mut else_source_fresh,
                next_local,
                wrappers,
            );
            // If one branch always terminates (return/break/continue), the
            // continuation is only reachable via the other branch. Use that
            // branch's state directly instead of intersecting — intersection
            // would conservatively destroy state that can't be affected.
            // If both branches terminate, the continuation is unreachable;
            // leave the state unchanged.
            if expr_always_terminates(then_branch) && expr_always_terminates(else_branch) {
                // Both branches terminate; continuation is dead code.
            } else if expr_always_terminates(then_branch) {
                *unique = else_unique;
                *known_empty = else_empty;
                *refreshed = else_refreshed;
                *builder_safe = else_builder_safe;
                *source_fresh = else_source_fresh;
            } else if expr_always_terminates(else_branch) {
                *unique = then_unique;
                *known_empty = then_empty;
                *refreshed = then_refreshed;
                *builder_safe = then_builder_safe;
                *source_fresh = then_source_fresh;
            } else {
                intersect_in_place(&mut then_unique, &else_unique);
                intersect_in_place(&mut then_empty, &else_empty);
                intersect_in_place(&mut then_refreshed, &else_refreshed);
                intersect_in_place(&mut then_builder_safe, &else_builder_safe);
                intersect_in_place(&mut then_source_fresh, &else_source_fresh);
                *unique = then_unique;
                *known_empty = then_empty;
                *refreshed = then_refreshed;
                *builder_safe = then_builder_safe;
                *source_fresh = then_source_fresh;
            }
        }
        AnfOp::AMatch { arms, .. } => {
            let mut merged_unique: Option<HashSet<LocalId>> = None;
            let mut merged_empty: Option<HashSet<LocalId>> = None;
            let mut merged_refreshed: Option<HashSet<LocalId>> = None;
            let mut merged_builder_safe: Option<HashSet<LocalId>> = None;
            let mut merged_source_fresh: Option<HashSet<LocalId>> = None;
            for arm in arms {
                let mut arm_unique = unique.clone();
                let mut arm_empty = known_empty.clone();
                let mut arm_refreshed = refreshed.clone();
                let mut arm_builder_safe = builder_safe.clone();
                let mut arm_source_fresh = source_fresh.clone();
                rewrite_expr(
                    &mut arm.body,
                    tainted,
                    &mut arm_unique,
                    &mut arm_empty,
                    &mut arm_refreshed,
                    &mut arm_builder_safe,
                    &mut arm_source_fresh,
                    next_local,
                    wrappers,
                );
                // Arms that always terminate (return/break/continue) can never
                // reach the post-match continuation, so their final state is
                // irrelevant for the continuation. Exclude them from the
                // intersection, preserving facts from the reachable arms.
                if expr_always_terminates(&arm.body) {
                    continue;
                }
                if let Some(m) = merged_unique.as_mut() {
                    intersect_in_place(m, &arm_unique);
                } else {
                    merged_unique = Some(arm_unique);
                }
                if let Some(m) = merged_empty.as_mut() {
                    intersect_in_place(m, &arm_empty);
                } else {
                    merged_empty = Some(arm_empty);
                }
                if let Some(m) = merged_refreshed.as_mut() {
                    intersect_in_place(m, &arm_refreshed);
                } else {
                    merged_refreshed = Some(arm_refreshed);
                }
                if let Some(m) = merged_builder_safe.as_mut() {
                    intersect_in_place(m, &arm_builder_safe);
                } else {
                    merged_builder_safe = Some(arm_builder_safe);
                }
                if let Some(m) = merged_source_fresh.as_mut() {
                    intersect_in_place(m, &arm_source_fresh);
                } else {
                    merged_source_fresh = Some(arm_source_fresh);
                }
            }
            if let Some(m) = merged_unique {
                *unique = m;
            }
            if let Some(m) = merged_empty {
                *known_empty = m;
            }
            if let Some(m) = merged_refreshed {
                *refreshed = m;
            }
            if let Some(m) = merged_builder_safe {
                *builder_safe = m;
            }
            if let Some(m) = merged_source_fresh {
                *source_fresh = m;
            }
        }
        AnfOp::ALoop { body } => {
            // Conservative: don't propagate unique/refreshed into loops
            let mut loop_unique = HashSet::new();
            let mut loop_empty = HashSet::new();
            let mut loop_refreshed = HashSet::new();
            let mut loop_builder_safe = HashSet::new();
            let mut loop_source_fresh = HashSet::new();
            rewrite_expr(
                body,
                tainted,
                &mut loop_unique,
                &mut loop_empty,
                &mut loop_refreshed,
                &mut loop_builder_safe,
                &mut loop_source_fresh,
                next_local,
                wrappers,
            );
            // Invalidate known_empty and source_fresh for locals assigned
            // inside the loop. A loop body may `assign(x = ...)` to outer
            // locals, making these facts stale.
            let assigned_in_loop = collect_assign_targets(body);
            for local in &assigned_in_loop {
                known_empty.remove(local);
                builder_safe.remove(local);
                source_fresh.remove(local);
            }
        }
        _ => {}
    }
}
