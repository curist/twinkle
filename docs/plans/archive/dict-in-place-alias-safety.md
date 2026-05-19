# Dict In-Place Alias Safety Prerequisite

## Goal

Prove that optimizer rewrites from persistent dict updates to in-place dict
updates are alias-safe before enabling true in-place HAMT mutation in
`rt.dict`.

This plan is a prerequisite for Phase 4 of
[`dict-performance-enhancements.md`](dict-performance-enhancements.md). Until
this plan is complete, `rt.dict.set_in_place` and `rt.dict.remove_in_place`
should continue delegating to the persistent `set` and `remove` helpers.

## Motivation

True in-place dict mutation changes observable runtime behavior unless the
optimizer proves that no live alias can observe mutation of either:

- the top-level `PDict` header (`size`, `root`, `order`), or
- any HAMT node reachable from the dict root.

A trial implementation showed that enabling real `dict__set_in_place` /
`dict__remove_in_place` can compile successfully but break the self-host fixed
point. Mirroring a simpler header-mutating version in Rust stage0 can also make
stage1 crash while checking `boot/main.tw`. Treat this as evidence that the
current in-place rewrite contract needs a dedicated audit before runtime
mutation is made observable.

## Ownership Contract

Document and enforce the required contract for:

```text
dict__set_in_place(d, k, v)
dict__remove_in_place(d, k)
```

Required properties:

- The `PDict` object is uniquely owned.
- Every HAMT node reachable from `PDict.root` is uniquely owned.
- The `PDict.order` vector is either uniquely owned for in-place header updates
  or remains updated persistently and rebound safely.
- The original dict value is dead after the call except through the returned
  value.
- No aliases are held in locals, records, arrays, dicts, closures, loop-carried
  state, or control-flow join values.

Top-level deadness alone is not sufficient: mutating a shared HAMT node would
violate persistent `Dict<K,V>` semantics even if the `PDict` header is no longer
used.

## Work Plan

### 1. Audit uniqueness analysis for nested ownership

Verify that freshness and transfer rules prove ownership of the whole reachable
container graph, not only the top-level `PDict` reference.

Questions to answer:

- Does a fresh `Dict.new()` imply unique ownership of its root/order graph?
- Does `dict_set` on a unique dict preserve unique ownership of the returned
  dict graph?
- Does copying a dict binding, storing it in an aggregate, or capturing it in a
  closure clear uniqueness for the whole graph?
- Do loop-carried values and branch joins preserve uniqueness only when all
  incoming paths satisfy the contract?

### 2. Audit all rewrite sites

Confirm that rewrites from `dict_set` / `dict_remove` to in-place forms satisfy
post-call liveness requirements in all relevant contexts:

- straight-line rebinding
- nested expressions
- control-flow joins
- loops
- closures
- record fields
- arrays/vectors
- dictionaries containing dictionaries
- early returns and `try` paths

### 3. Add negative alias tests

Add optimizer and runtime tests where a dict must **not** rewrite to in-place:

- copied into another local before update
- used after update through the old binding
- captured by a closure
- stored in a record field
- stored in a vector/array
- stored as a dict value
- passed to a function that may retain it
- shared across branch or loop-carried state

These tests should verify both the optimizer decision and persistent runtime
behavior.

### 4. Add positive ownership tests

Add tests where a dict **should** rewrite once true in-place mutation is enabled:

- fresh dict followed by a linear insert chain
- replacement of an existing key in a linear chain
- remove hit in a linear chain
- remove miss in a linear chain
- nested HAMT paths
- hash collision buckets
- insertion-order preservation after in-place operations

### 5. Keep stage0 and boot behavior aligned

Any enabled in-place runtime path must be mirrored sufficiently in Rust stage0
to preserve the stage1 → stage2 → stage3 fixed point, or the optimizer must be
gated so stage0 and boot produce equivalent output during bootstrapping.

### 6. Gate Phase 4 on fixed-point validation

Do not consider this prerequisite complete until all of the following pass with
the optimizer allowed to emit dict in-place calls:

```bash
target/twk run boot/tests/main.tw
cargo test --release
make stage2
```

## Deliverables

- A documented dict in-place ownership contract.
- Optimizer tests for negative alias cases.
- Optimizer tests for positive linear ownership cases.
- Runtime tests covering persistent behavior when aliases exist.
- Stage0/boot alignment for any enabled in-place runtime behavior.
- A clear go/no-go note for enabling Phase 4 of dict runtime mutation.
