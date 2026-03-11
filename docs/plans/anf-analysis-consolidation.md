# ANF Analysis Consolidation

**Goal:** Consolidate repeated ANF tree analyses into shared utilities so codegen, optimization,
and closure/inference helpers operate on one consistent set of traversal rules.

---

## Problem

Multiple modules reimplement similar ANF traversals:

* free-local/bound-local collection
* assigned-local tracking
* pattern-binding traversal
* expression divergence checks

Current duplication exists in at least:

* [`src/opt/pipeline.rs`](../../src/opt/pipeline.rs)
* [`src/codegen/emit.rs`](../../src/codegen/emit.rs)
* [`src/codegen/ctx.rs`](../../src/codegen/ctx.rs)

As ANF grows (`ADefer`, iterator-specific ops, future ops), duplicated walkers can diverge.

---

## Non-Goals

* Do not redesign ANF syntax.
* Do not replace optimization passes.
* Do not enforce one monolithic visitor for all use-cases.

---

## Proposed Solution

Create shared ANF analysis helpers (e.g. `src/ir/anf/analysis.rs`) with focused APIs:

* `collect_free_locals(expr, declared_seed) -> HashSet<LocalId>`
* `collect_bound_locals(expr) -> HashSet<LocalId>`
* `collect_assigned_locals(expr) -> HashSet<LocalId>`
* `collect_pattern_bindings(pattern, out)`
* `expr_always_diverges(expr) -> bool` (if kept as shared utility)

Guidelines:

* Keep utilities pure and side-effect-free.
* Keep module-specific filtering outside shared utilities.
* Require unit tests for each traversal behavior.

---

## Work Plan

### Phase 0: Inventory and parity tests

- [ ] Enumerate all duplicated ANF traversal helpers and their semantic differences.
- [ ] Add characterization tests for corner cases (`AIf`, `AMatch`, `AAssign`, `ADefer`, nested loops).

### Phase 1: Shared utility extraction

- [ ] Add shared traversal module under `ir::anf`.
- [ ] Port one representative consumer (optimizer) first to validate API shape.

### Phase 2: Consumer migration

- [ ] Migrate codegen capture/free-local and assigned-local collectors.
- [ ] Migrate context setup collectors used for local layout heuristics.
- [ ] Keep behavior-equivalence tests green during migration.

### Phase 3: Cleanup

- [ ] Delete duplicated traversal implementations.
- [ ] Add lint/test guardrails to discourage new local reimplementations without justification.

---

## Acceptance Criteria

1. Core ANF traversals are implemented once and reused by codegen + optimizer.
2. Existing behavior remains unchanged for current passes.
3. Future ANF op additions require updating one shared traversal surface.

---

## Immediate Next Steps

1. Extract `collect_pattern_bindings` and free-local traversal first (highest duplication).
2. Migrate optimizer and codegen helper sites.
3. Add fixture-based regression tests around `AAssign` and branch/match traversal behavior.

