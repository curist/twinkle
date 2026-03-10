# Wasm Sum Representation Boundary Unification

**Goal:** Eliminate recurring Wasm `ref.cast` traps caused by drift between typed and erased
sum-value representations (`Option`, `Result`, and iterator-adjacent variants) by
centralizing representation decisions and boundary conversions.

This plan is a follow-up to:

* [wasm-type-erasure-reduction.md](./wasm-type-erasure-reduction.md)
* [wasm-iterator-representation-boundaries.md](./wasm-iterator-representation-boundaries.md)

Those plans improved specialization and performance, but they also exposed a persistent
correctness problem: representation policy is still spread across multiple codegen paths.

---

## Problem

The backend currently tracks value shape through multiple partially-overlapping channels:

* semantic type (`MonoType`)
* physical local storage type (`ValType` from local allocation)
* side metadata (`local_typed_option`, iterator state metadata, closure repr metadata)

When these diverge, codegen can emit direct casts at the wrong boundary, e.g. treating an
`anyref` local as erased `$Variant` even when the runtime value is a typed option struct.

This causes runtime cast failures in otherwise valid programs.

---

## Pain Today

Recent failures share the same pattern:

1. Interpreter succeeds (semantic model is correct).
2. Wasm traps with `wasm trap: cast failure`.
3. Failing WAT shows direct `ref.cast (ref null $rt_types__Variant)` where value may be
   typed option/typed helper payload.

Observed examples:

* `tests/run/closure_capture_cross_module/main.tw`
* `examples/argparse/main.tw`
* `Option` reassignment / flow-merge paths (`AAssign`) crossing function boundaries

Operational pain:

* Regressions reappear in neighboring paths after local fixes.
* Each fix adds path-specific guards, increasing complexity and maintenance cost.
* Confidence degrades because there is no single representation invariant to verify.

---

## Root Cause

Backend representation policy is not centralized.

Representation choices (typed vs erased) are currently made independently in:

* literal emission (`AVariant`)
* local emission (`emit_local_atom`)
* match lowering
* assignment/init handling
* helper generation
* ABI-related coercions

Without one source of truth, paths can disagree about the same value.

---

## Non-Goals

This plan is **not** primarily a performance plan.

* Keep existing typed fast paths where safe.
* Preserve current runtime ABI compatibility.
* Do not redesign language semantics or typechecking.

Primary success criterion is **correctness and stability** at representation boundaries.

---

## Proposed Solution

Introduce a unified sum-representation model and make all typed/erased crossings explicit.

### 1. Add explicit sum representation metadata

Add a shared backend representation enum for sum-like values:

* `ErasedVariant`
* `TypedOption(MonoType)`
* `TypedResult(MonoType, MonoType)` (future-enabled)
* `TypedIterOption(IteratorStateInfo)` (can remain mapped through existing iterator metadata if preferred)
* `Unknown` / `ErasedAnyref`

This metadata must represent **physical runtime shape**, not just semantic type.

### 2. Centralize boundary conversion helpers

Define one module-level conversion surface:

* typed option/result -> erased variant
* erased variant -> typed option/result
* iterator-adjacent typed forms <-> erased variant forms

Callers use helpers instead of hand-emitting direct `ref.cast`/payload extraction logic.

### 3. Make local emission boundary-aware by contract

`emit_local_atom` should:

* query representation metadata
* choose conversion helper if a boundary is crossed
* avoid direct “best guess” casts for sum values

### 4. Separate local storage repr from ABI repr

Keep local specialization and boundary ABI independent:

* locals may stay typed for in-function optimization
* boundary crossings (function returns/params, record field expectations, erased runtime helpers)
  explicitly convert

### 5. Add representation invariant checks

Add debug-time validation pass over emitted ModuleIR/WAT intent:

* no direct cast from `anyref` local to `$Variant` for sum-typed locals without boundary helper
* no typed-option field extraction unless local is proven typed-option representation

This catches drift before runtime.

---

## Execution Plan

## Phase 0: Baseline and guardrails

* Document canonical boundary rules in code comments near conversion helpers.
* Add failing regression fixtures for known shapes (already started with option assign cases).
* Add debug assertions in key emission sites where repr + expected type disagree.

## Phase 1: Representation unification in context

* Extend codegen context with explicit sum representation metadata.
* Replace ad hoc `local_typed_option` checks with repr queries where possible.
* Keep compatibility shims to avoid large one-shot rewrites.

## Phase 2: Conversion API adoption

* Route `emit_local_atom`, `emit_variant_literal`, assignment/init paths through shared conversion helpers.
* Remove duplicated conversion snippets and path-local heuristics.

## Phase 3: Match and flow-merge normalization

* Ensure `match` on sum values always chooses typed or erased path from shared repr metadata.
* Normalize branch/loop merge behavior for repr metadata, not only `MonoType`.

## Phase 4: ABI boundary hardening

* Audit function call/return and record field boundaries for sum representations.
* Ensure boundary conversion is explicit and symmetric.

## Phase 5: Cleanup and simplification

* Remove obsolete one-off guards added during bug-fix iterations.
* Keep one canonical boundary conversion path per representation pair.

---

## Test Strategy

Add/maintain matrix coverage for:

* same-module vs cross-module record fields containing sum values
* captured closures + generic calls + sum-containing records
* reassignment (`AAssign`) and branch merges with `Option`/`Result`
* iterator-specialized and erased iterator fallback paths
* function boundary roundtrips (`Option<T>` in/out)

For each matrix row:

* interpreter run must succeed
* wasm run must match interpreter output
* no runtime cast failure

---

## Risks

* Partial migration can temporarily increase complexity.
* Repr metadata bugs can silently route through wrong helper if not asserted.
* Overly strict invariants may flag currently-valid transitional paths.

Mitigation:

* phase-by-phase rollout with focused regressions
* debug assertions before cleanup
* keep compatibility paths until invariant checks are green

---

## Acceptance Criteria

1. `examples/argparse/main.tw` and known reproductions run on Wasm without cast failures.
2. `run_wasm_test` remains green after removing path-specific emergency guards.
3. New invariant checks pass in CI for representative fixtures.
4. No direct ad hoc sum boundary casts remain in core emission paths.

---

## Immediate Next Steps

1. Introduce explicit sum repr metadata in `EmitCtx` (minimum viable shape).
2. Move all Option boundary handling in `emit_local_atom` to shared conversion helpers.
3. Add a small debug verifier for illegal direct casts in sum-boundary contexts.
4. Expand regression fixtures for `Result<T,E>` assignment/merge boundaries.

---

## Implementation Checklist

### Phase 0: Baseline and Guardrails

- [ ] Add doc-comments in [src/codegen/emit.rs](../../src/codegen/emit.rs) near:
  `emit_local_atom`, `emit_variant_literal`, and boundary conversion helpers
  defining allowed sum-boundary conversions.
- [ ] Add a brief invariant comment in [src/codegen/ctx.rs](../../src/codegen/ctx.rs)
  documenting the distinction between semantic `MonoType` and physical local `ValType`.
- [ ] Add/keep focused regression fixtures for current repro classes in `tests/run/`:
  cross-module closure capture + option assignment/merge boundaries.

### Phase 1: Representation Unification in Context

- [ ] Introduce explicit sum repr metadata in [src/codegen/ctx.rs](../../src/codegen/ctx.rs):
  a `SumRepr` enum and storage on local backend info.
- [ ] Add helpers in `EmitCtx`:
  `local_sum_repr(local_id)`, `set_local_sum_repr(local_id, repr)`,
  and flow push/restore wrappers.
- [ ] Map existing `local_typed_option` usage to `SumRepr` reads/writes behind compatibility shims.

### Phase 2: Conversion API Adoption

- [ ] Add centralized conversion helpers in [src/codegen/emit.rs](../../src/codegen/emit.rs):
  typed option/result/iterator-option <-> erased variant.
- [ ] Route `emit_local_atom` through these helpers for sum boundary crossings.
- [ ] Route `emit_variant_literal` through these helpers when destination repr differs from source repr.
- [ ] Remove duplicated path-local conversion snippets once covered by shared helpers.

### Phase 3: Match and Flow-Merge Normalization

- [ ] Update match lowering in [src/codegen/emit.rs](../../src/codegen/emit.rs):
  choose typed vs erased pattern path from unified sum repr metadata.
- [ ] Ensure branch/loop merge logic reconciles sum repr metadata consistently,
  not only `MonoType`.
- [ ] Add regressions for branch merge + `AAssign` + `match` combinations.

### Phase 4: ABI Boundary Hardening

- [ ] Audit direct-call and closure-call boundaries in [src/codegen/emit.rs](../../src/codegen/emit.rs)
  for implicit sum casts.
- [ ] Ensure function boundary paths explicitly convert sum repr where needed.
- [ ] Audit record literal/get/update paths to ensure sum-typed fields cross boundaries explicitly.

### Phase 5: Verification and Cleanup

- [ ] Add a debug verifier pass (or debug assertions) in [src/codegen/emit.rs](../../src/codegen/emit.rs)
  to reject illegal direct sum boundary casts.
- [ ] Remove obsolete emergency guards that are superseded by unified repr + conversion APIs.
- [ ] Re-run and keep green:
  `run_wasm_test`, `typed_closure_test`, and targeted boundary fixtures.
- [ ] Update plan status notes in this document with completed checkpoints.
