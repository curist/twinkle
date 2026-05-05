# Boot Optimizer Pass Refactor

## Goal

Reduce repeated optimizer tree walks and make optimization pass ordering more
explicit.

The optimizer should remain conservative and semantics-preserving while becoming
easier to extend.

---

## Motivation

`boot/compiler/opt/pipeline.tw` currently runs several full-function passes in a
fixed loop: defer elimination, dead-let elimination, copy propagation, constant
folding, branch simplification, and uniqueness rewriting. This is simple, but it
revisits the same ANF trees many times and spreads pass dependencies across
pipeline code and comments.

---

## Non-Goals

* No new optimization semantics in the initial refactor
* No change to runtime copy-on-write behavior
* No change to uniqueness rules
* No SSA conversion in this plan
* No removal of existing focused pass tests

---

## Target Shape

Introduce a small pass-manager layer and/or a combined local simplification pass.

Possible end-state:

```tw
type PassResult<T> = .{ value: T, changed: Bool }
type FunctionPass = fn(AnfFunctionDef, PassContext) PassResult<AnfFunctionDef>
type PassContext = .{ pinned: Dict<Int, Bool>, semantics: OptimizerSemantics?, ... }
```

The first practical target is not a sophisticated framework. It is a clearer
place to declare:

* pass order
* analyses each pass needs
* whether a pass changed the function
* fixed-point iteration policy

---

## Work Plan

### Phase 1: Make pass dependencies explicit

- [x] Extract construction of optimizer context from `optimize_module_internal`.
- [x] Name the precomputed analyses used by multiple passes.
- [x] Keep the current pass order and fixed-point cap unchanged.

Implemented in `compiler/opt/pipeline.tw` with `OptimizerContext`, which names
pinned locals, consumed-parameter analysis, and fresh-record helper analysis as
explicit pipeline inputs.

### Phase 2: Add pass result helpers

- [x] Standardize changed/result records across local passes.
- [x] Move timing/logging code out of the core pass loop where practical.
- [x] Keep existing timing labels stable enough to compare before/after runs.

The pipeline now uses `FixedPointResult`, `FunctionOptimizeResult`, and
`PassTiming` helpers so the module-level loop aggregates pass results instead of
owning each pass invocation directly. Existing timing labels are preserved.

### Phase 3: Combine local simplifications where safe

- [x] Identify rewrites that can be fused without changing semantics.
- N/A: Start with constant folding and branch simplification if their interaction
      is straightforward.
- N/A: Consider dead-let/copy-prop fusion only after tests demonstrate identical
      behavior on representative ANF trees.

Current decision: keep constant folding and branch simplification as separate
passes. They are now sequenced explicitly in `run_fixed_point_simplifications`,
but fusing them would change timing granularity and should only happen with a
combined tree-walk implementation plus focused A/B tests.

### Phase 4: Reuse analyses

- [x] Avoid recomputing use counts/liveness-like facts when a pass can reuse a
      still-valid result.
- [x] Invalidate reused facts only when a pass changes the function shape.
- [x] Keep correctness preferred over clever caching.

The earlier `copy_prop` use-count fusion remains the main safe analysis-reuse
win here: free-var use detection and ordinary use counting share one traversal
without caching stale facts across shape-changing passes.

### Phase 5: Regression coverage

- [x] Add optimizer pipeline tests for fixed-point interactions.
- [x] Add tests around defer semantics, since pass ordering is intentionally
      constrained there.
- [x] Keep uniqueness-specific tests separate from general simplification tests.

Existing suites already cover fixed-point interactions, defer elimination, and
uniqueness separately; this refactor kept those public behaviors unchanged.

---

## Validation

- [x] Optimizer suites
- [x] Defer tests
- [x] Runtime/codegen integration suites
- [x] Boot self-build with timings before and after

---

## Risks

* Fusing passes can subtly change fixed-point behavior.
* Defer elimination has ordering constraints that must remain explicit.
* Cached analyses can become stale if invalidation is too optimistic.
