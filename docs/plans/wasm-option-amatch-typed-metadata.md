# Wasm Option `AMatch` Typed Metadata

**Goal:** Preserve typed `Option<T>` metadata for `AMatch`-produced locals so Wasm codegen can keep typed-option fast paths instead of falling back to erased `$Variant`.

This is a focused follow-up to:

* [archive/wasm-type-erasure-reduction.md](./archive/wasm-type-erasure-reduction.md)
* [archive/wasm-sum-representation-boundary-unification.md](./archive/wasm-sum-representation-boundary-unification.md)

---

## Problem

`AMatch` results that semantically produce `Option<T>` are currently safe but often lose typed-option flow metadata. That forces erased boundary paths and misses optimization opportunities already available for other `Option<T>` producers.

---

## Scope

In scope:

* detect `Option<T>`-typed `AMatch` results in emit flow metadata
* seed and preserve `SumRepr::TypedOption` when local Wasm storage can hold typed option structs
* keep existing safety behavior for non-concrete or mismatched-storage cases
* add regression coverage for match-produced options through assignment and boundary crossings

Out of scope:

* new runtime ABI shapes
* broad `Result<T,E>` refactors (already covered by prior plan)

---

## Implementation Outline

1. Extend typed-sum source detection to include `AnfOp::AMatch` when inferred op mono is a typed option candidate.
2. Ensure `emit_let_expr` flow seeding (`push_flow_typed_option_binding`) applies for that case.
3. Validate `emit_local_atom`/`emit_sum_local_to_erased` behavior remains unchanged except for now-available metadata.
4. Add tests for:
   * `AMatch` returning `.Some/.None` into a local, then reused across branches
   * function boundary roundtrip from an `AMatch`-produced option
   * wasm parity with interpreter output

---

## Acceptance Criteria

1. `AMatch`-produced concrete `Option<T>` locals can take typed-option paths when local storage allows it.
2. `run_test` and `run_wasm_test` stay green.
3. No new cast-failure regressions in option assignment/match boundary fixtures.
