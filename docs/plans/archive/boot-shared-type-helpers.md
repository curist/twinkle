# Boot Shared Type and Backend Fact Helpers

## Goal

Consolidate duplicated `MonoType`, substitution, Wasm value-type equality, and
backend fact helper logic into shared modules.

This is a maintainability refactor. The desired behavior is identical compiler
output with fewer copied implementations across checker, lowering, backend
preparation, planning, and emission.

---

## Motivation

Several helpers currently appear in multiple compiler files, including:

* type parameter substitution
* `MonoType` traversal utilities
* named type/variant/field resolution helpers
* atom-to-mono and slot fact lookup helpers
* `ValType` / `HeapType` equality helpers

Duplication makes type-system and backend changes risky: a fix can land in one
copy while another copy silently diverges.

---

## Non-Goals

* No behavior changes
* No public language changes
* No new backend representation model
* No broad file layout reorganization beyond small helper modules
* No attempt to solve all checker/lowerer decomposition in this plan

---

## Proposed Modules

Names can be adjusted during implementation, but the helper boundaries should be
stable.

```text
boot/compiler/type_util.tw
boot/compiler/backend/facts.tw
boot/compiler/codegen/wasm_type_util.tw
```

Suggested responsibilities:

### `type_util.tw`

* `subst_type_params`
* `subst_named_type_args`
* `mono_contains_var`
* `mono_type_params`
* common `MonoType` traversal helpers

### `backend/facts.tw`

* prepared slot lookup wrappers with consistent error text
* atom mono/repr accessors shared by backend planning and emission where they
  refer to prepared facts
* named variant/field resolution helpers if they are backend-specific

### `codegen/wasm_type_util.tw`

* `val_type_eq`
* `heap_type_eq`
* reference-type comparison helpers
* small helpers needed by verifier, emitter, and binary writer

---

## Work Plan

### Phase 1: Inventory duplicates

- [x] List all local `subst_type_params` implementations.
- [x] List all local `atom_mono`/slot lookup implementations.
- [x] List all `ValType`/`HeapType` equality implementations.
- [x] Mark each duplicate as semantic, backend-prepared, or wasm-encoding
      specific.

### Phase 2: Extract pure `MonoType` helpers

- [x] Add `compiler.type_util`.
- [x] Move one canonical `subst_type_params` implementation into it.
- [x] Replace duplicate substitution helpers one file at a time.
- [x] Keep error messages stable where tests rely on them.

### Phase 3: Extract Wasm type helpers

- [x] Add `compiler.codegen.wasm_type_util`.
- [x] Move value-type and heap-type equality helpers.
- [x] Replace verifier, emitter, and binary writer copies.
- [x] Add focused tests if any equality behavior is not already covered.

### Phase 4: Extract prepared backend facts

- [x] Add `compiler.backend.facts` for helpers that read `PreparedFunc` and
      `SlotInfo` slot facts.
- [x] Add prepared atom/repr accessors where they can stay independent of
      emission-local context.
- [x] Replace local slot/atom fact helpers in planner and emitter.
- [x] Keep backend passes reading from `PreparedModule` facts rather than
      rediscovering facts from ANF.

### Phase 5: Cleanup and documentation

- [x] Remove obsolete local copies.
- [x] Add comments identifying which helpers are type-system-level and which are
      backend-prepared-fact-level.
- [x] Update architecture notes if helper modules become part of the stable
      backend boundary.

---

## Validation

- [x] Boot tests
- [x] Backend prepare/verify suites
- [x] Wasm layout/plan/binary suites
- [x] Codegen integration suites
- [x] `target/twk build boot/main.tw -o /tmp/boot.wasm`

---

## Risks

* Some duplicate helpers may have intentionally different error behavior.
* Backend helpers must not blur the boundary between semantic ANF and prepared
  IR facts.
* Moving helpers too early can create import cycles; extract pure helpers first.
