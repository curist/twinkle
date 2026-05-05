# Boot Wasm Type Ordering Refactor

## Goal

Move Wasm GC type dependency analysis and recursive type-group ordering into one
shared implementation used by WAT emission, binary emission, and Wasm planning.

---

## Motivation

Wasm GC type ordering is backend infrastructure, not a format-specific concern.
Keeping similar SCC/topological ordering logic in multiple codegen files risks
WAT and binary output drifting apart.

---

## Non-Goals

* No change to generated Wasm type semantics
* No change to runtime type definitions
* No change to textual WAT formatting except incidental ordering preservation
* No new Wasm features

---

## Target Shape

Add a shared module such as:

```text
boot/compiler/codegen/type_order.tw
```

Responsibilities:

* extract a type definition name
* collect referenced type names from a type definition
* compute SCC groups for recursive type definitions
* produce dependency-before-dependent group order
* expose helpers that both WAT and binary encoders can consume

---

## Work Plan

### Phase 1: Extract without changing callers

- [x] Copy the current canonical ordering behavior into `type_order.tw`.
- [x] Add small wrapper types/functions if needed so both WAT and binary paths can
      call the same API.
- [x] Keep old local implementations until the shared module is tested.

### Phase 2: Switch WAT emission

- [x] Replace local type ordering in `codegen/wat.tw`.
- [x] Preserve existing WAT output ordering.
- [x] Run WAT-focused suites.

### Phase 3: Switch binary emission

- [x] Replace local type ordering in `codegen/wasm.tw`.
- [x] Preserve binary type-section behavior.
- [x] Run Wasm binary/linking suites.

### Phase 4: Decide planner ownership

- [x] Decide whether `wasm_plan_impl.tw` should compute ordered groups once and
      store them in the plan/registry.
- N/A: If yes, move order computation earlier and make emitters consume the plan.
- [x] If no, keep the shared helper as an emitter utility and document why.

Current decision: keep SCC grouping as an emitter utility. `wasm_plan_impl.tw`
still produces a simple dependency-sorted type list for stable planning, while
WAT and binary emission need recursive type groups and now share that
format-independent grouping logic directly.

---

## Validation

- [x] Wasm layout suite
- [x] Wasm plan suite
- [x] WAT suite
- [x] Codegen integration suite
- [x] Boot self-build to Wasm

---

## Risks

* Recursive type groups are subtle; ordering changes can break binary validation.
* WAT and binary paths may currently rely on slightly different incidental order.
* Moving ordering into the planner too early may make the registry API broader
  than necessary.
