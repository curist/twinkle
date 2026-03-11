# Intrinsic Registry Unification

**Goal:** Make intrinsic/prelude metadata a single source of truth so lowering, type contracts,
and Wasm emission cannot drift independently.

---

## Problem

Intrinsic and prelude behavior is currently defined in multiple places:

* prelude IDs and constants in [`src/ir/lower.rs`](../../../src/ir/lower.rs)
* default function table bootstrap in [`src/module/context.rs`](../../../src/module/context.rs)
* type/ABI contracts in [`src/intrinsics/contracts.rs`](../../../src/intrinsics/contracts.rs)
* runtime binding map in [`src/codegen/prelude.rs`](../../../src/codegen/prelude.rs)
* lowering dispatch logic in [`src/codegen/emit.rs`](../../../src/codegen/emit.rs)

This duplication increases maintenance cost and creates drift risk when adding/changing intrinsics.

---

## Non-Goals

* Do not change user-facing intrinsic semantics.
* Do not renumber existing stable prelude IDs.
* Do not force every intrinsic to be runtime-backed.

---

## Proposed Solution

Introduce a canonical intrinsic registry entry model used by all layers:

```text
IntrinsicSpec {
  func_id
  twinkle_name
  dispatch_kind (runtime | intrinsic)
  signature (type params, params, ret)
  runtime_binding? (module, name, sym, wasm params/results)
  lowering_kind (enum key for emitter handler selection)
}
```

Key rules:

* IDs remain in one stable place (existing prelude ID policy).
* `contracts`, `default_func_table`, and `build_prelude_map` are derived from `IntrinsicSpec`.
* call-lowering dispatch keys off `lowering_kind`, not repeated ad hoc `FuncId` matches.

---

## Work Plan

### Phase 0: Parity guardrails

- [x] Add tests that compare names/signatures/dispatch metadata across current sources.
- [x] Snapshot existing registry behavior for high-risk intrinsics (`Iterator.*`, `Cell.*`, string/byte conversions).

### Phase 1: Canonical spec introduction

- [x] Add a new intrinsic spec module exposing stable iteration over all intrinsic entries.
- [x] Keep existing APIs (`contracts::*`, `build_prelude_map`) as compatibility wrappers initially.

### Phase 2: Consumer migration

- [x] Migrate `default_func_table` and lowerer bootstrap to the canonical spec.
- [x] Migrate `contracts` helpers to derive from canonical entries.
- [x] Migrate codegen prelude runtime binding map to derive from canonical entries.

### Phase 3: Dispatch cleanup

- [x] Replace repetitive call dispatch checks with table-driven `lowering_kind` routing.
- [x] Keep special-case handlers only where semantics genuinely differ.

### Phase 4: Cleanup and policy hardening

- [x] Remove obsolete duplicated registration code.
- [x] Add retired-ID policy checks against the canonical intrinsic list.

---

## Acceptance Criteria

1. A new intrinsic is added by editing one canonical list, with consumers derived.
2. No duplicated manual bootstrap lists remain for default function table and prelude runtime map.
3. Existing intrinsic behavior and ABI remain unchanged (tests/snapshots green).
4. Retired prelude ID policy remains enforced.
