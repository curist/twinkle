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

- [ ] Extract construction of optimizer context from `optimize_module_internal`.
- [ ] Name the precomputed analyses used by multiple passes.
- [ ] Keep the current pass order and fixed-point cap unchanged.

### Phase 2: Add pass result helpers

- [ ] Standardize changed/result records across local passes.
- [ ] Move timing/logging code out of the core pass loop where practical.
- [ ] Keep existing timing labels stable enough to compare before/after runs.

### Phase 3: Combine local simplifications where safe

- [ ] Identify rewrites that can be fused without changing semantics.
- [ ] Start with constant folding and branch simplification if their interaction
      is straightforward.
- [ ] Consider dead-let/copy-prop fusion only after tests demonstrate identical
      behavior on representative ANF trees.

### Phase 4: Reuse analyses

- [ ] Avoid recomputing use counts/liveness-like facts when a pass can reuse a
      still-valid result.
- [ ] Invalidate reused facts only when a pass changes the function shape.
- [ ] Keep correctness preferred over clever caching.

### Phase 5: Regression coverage

- [ ] Add optimizer pipeline tests for fixed-point interactions.
- [ ] Add tests around defer semantics, since pass ordering is intentionally
      constrained there.
- [ ] Keep uniqueness-specific tests separate from general simplification tests.

---

## Validation

- [ ] Optimizer suites
- [ ] Defer tests
- [ ] Runtime/codegen integration suites
- [ ] Boot self-build with timings before and after

---

## Risks

* Fusing passes can subtly change fixed-point behavior.
* Defer elimination has ordering constraints that must remain explicit.
* Cached analyses can become stale if invalidation is too optimistic.
