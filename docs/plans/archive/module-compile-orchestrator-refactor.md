# Module Compile Orchestrator Refactor

**Goal:** Refactor module compilation orchestration into clearer layers so dependency loading,
cache interactions, stage execution, and environment mutation are separated by explicit contracts.

This plan extends the direction described in
[../../internals/query-pipeline.md](../../internals/query-pipeline.md).

---

## Problem

`compile_module_with_adapter` currently combines many concerns:

* source loading and canonicalization
* recursive dependency traversal and cycle handling
* prelude auto-import policy
* cache key/hit/miss logic
* stage execution (parse/resolve/typecheck/lower)
* mutable environment snapshot/restore
* exports construction and post-stage state cleanup

Main hotspot:

* [`src/module/mod.rs`](../../../src/module/mod.rs)

This makes reasoning and testing difficult, and increases risk when changing any one concern.

---

## Non-Goals

* Do not alter language import semantics.
* Do not replace query-cache infrastructure.
* Do not change Core IR linking behavior in this plan.

---

## Proposed Layering

### 1. Dependency graph layer

Responsible for:

* canonical module key resolution
* dependency list discovery
* prelude auto-import expansion
* cycle detection

No compiler env mutation.

### 2. Stage runner layer

Responsible for:

* stage execution order (parse -> resolve -> typecheck -> lower)
* stage cache lookup/store
* deterministic stage input/output contracts

No recursive dependency traversal.

### 3. Environment projection layer

Responsible for:

* registering dependency exports into compile state
* controlled env snapshot/rollback boundaries
* cleanup of module-local temporary bindings

No filesystem/cache logic.

### 4. Export/lower artifact layer

Responsible for:

* building `ModuleExports`
* persisting lowered module metadata into global compile state

No stage execution details.

---

## Work Plan

### Phase 0: Characterization

- [x] Add high-level tests for import traversal, prelude auto-import, and cache behavior.
- [x] Add deterministic trace assertions for stage execution order on multi-module projects.

### Phase 1: Extract dependency planner

- [x] Move dependency expansion and prelude injection policy into a dedicated planner.
- [x] Keep current behavior exactly (source-order dependencies + deterministic prelude order).

### Phase 2: Extract stage runner

- [x] Introduce a runner API that executes one module stage pipeline from explicit inputs.
- [x] Move cache-key construction/hits/misses behind runner methods.

### Phase 3: Extract env integration

- [x] Isolate snapshot/restore and export registration into dedicated helpers.
- [x] Make env mutation boundaries explicit in function signatures.

### Phase 4: Cleanup

- [x] Reduce `compile_module_with_adapter` to orchestration glue across extracted components.
- [x] Remove now-redundant inline branching and duplicated error formatting paths.

---

## Acceptance Criteria

1. `compile_module_with_adapter` is materially smaller and primarily orchestration glue.
2. Dependency planning, stage execution, and state integration are testable independently.
3. Existing behavior for imports/prelude/cache/lowering remains unchanged.
4. Multi-module compile regressions are covered by dedicated integration tests.

---
## Completion Notes (2026-03-11)

This document acts as an execution record for this refactor.

Acceptance criteria verification:

1. `compile_module_with_adapter` was split so orchestration delegates to
   dedicated helpers plus extracted modules (`planner`, `stage_runner`,
   `env_integration`), leaving glue-focused control flow.
2. Dependency planning, stage execution, and env projection each have focused
   unit coverage in `src/module/planner.rs`, `src/module/stage_runner.rs`, and
   `src/module/env_integration.rs`.
3. Existing behavior parity was validated with targeted integration suites:
   `module_compile_orchestrator_refactor_test`, `modules_test`,
   `query_cache_test`, `source_map_compile_test`.
4. Multi-module ordering and cache behavior regressions are covered by
   `tests/module_compile_orchestrator_refactor_test.rs`.
