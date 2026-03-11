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

* [`src/opt/pipeline.rs`](../../../src/opt/pipeline.rs)
* [`src/codegen/emit.rs`](../../../src/codegen/emit.rs)
* [`src/codegen/ctx.rs`](../../../src/codegen/ctx.rs)

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

- [x] Enumerate all duplicated ANF traversal helpers and their semantic differences.
  Inventory summary: free-local/pattern, bound-local, assigned-local, and
  divergence walkers were duplicated across optimizer/codegen. The only
  semantic variant was empty-arm `AMatch` divergence policy, now made explicit
  via `DivergenceOptions.empty_match_diverges`.
- [x] Add characterization tests for corner cases (`AIf`, `AMatch`, `AAssign`, `ADefer`, nested loops).

### Phase 1: Shared utility extraction

- [x] Add shared traversal module under `ir::anf`.
- [x] Port one representative consumer (optimizer) first to validate API shape.

### Phase 2: Consumer migration

- [x] Migrate codegen capture/free-local and assigned-local collectors.
- [x] Migrate context setup collectors used for local layout heuristics.
- [x] Keep behavior-equivalence tests green during migration.
  Progress: free-local + pattern-binding + bound-local + assigned-local
  collectors plus divergence checks are shared and migrated for
  `opt/pipeline`, `opt/defer_elim`, `opt/use_count`, `codegen/emit`, and
  `codegen/ctx` (with explicit empty-match divergence policy in codegen ctx).

### Phase 3: Cleanup

- [x] Delete duplicated traversal implementations.
- [x] Add lint/test guardrails to discourage new local reimplementations without justification.

---

## Acceptance Criteria

1. Core ANF traversals are implemented once and reused by codegen + optimizer.
2. Existing behavior remains unchanged for current passes.
3. Future ANF op additions require updating one shared traversal surface.

---

## Immediate Next Steps

### Phase 0: Inventory and Parity (Complete)

- [x] Enumerate duplicated traversal helpers and semantic differences.
- [x] Add characterization tests for `AIf`, `AMatch`, `AAssign`, `ADefer`, and nested-loop cases.

### Phase 1: Shared Utility Extraction (Complete)

- [x] Add shared `ir::anf::analysis` module.
- [x] Validate API shape via initial optimizer migration.

### Phase 2: Consumer Migration (Complete)

- [x] Migrate codegen + optimizer free-local/pattern/bound/assigned collectors.
- [x] Migrate context setup collectors used in local layout heuristics.
- [x] Keep migration behavior-equivalent via targeted and `--lib` test runs.

### Phase 3: Cleanup and Guardrails (Complete)

- [x] Delete duplicated local traversal/divergence implementations in key consumers.
- [x] Add guardrail test to catch local re-implementations in migrated files.

### Phase 4: Optional Hardening (Follow-Up)

- [ ] Expand guardrail file coverage if new analysis consumers are added.
- [ ] Add contributor guidance to prefer `ir::anf::analysis` helpers for new ANF traversals.
