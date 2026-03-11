# Codegen Boundary Separation

**Goal:** Split ANF→Wasm codegen into explicit planning, representation-flow analysis, and
instruction emission stages so backend correctness no longer depends on one large mutable pass.

This plan is complementary to:

* [wasm-sum-representation-boundary-unification.md](./wasm-sum-representation-boundary-unification.md)
* [wasm-iterator-representation-boundaries.md](./archive/wasm-iterator-representation-boundaries.md)

Those plans harden specific representation boundaries. This plan targets the broader structural
cause: representation analysis and emission are still tightly coupled.

---

## Problem

`ANF -> ModuleIR` codegen currently mixes multiple responsibilities in one place:

* module-level planning and type registration
* local layout and flow-sensitive representation inference
* runtime import collection
* intrinsic dispatch policy
* final Wasm instruction emission

The result is high coupling and ordering sensitivity. Small behavior changes can accidentally
affect unrelated concerns because they share one mutable context.

Hotspots:

* [`src/codegen/emit.rs`](../../src/codegen/emit.rs) (very large, planning + emission mixed)
* [`src/codegen/ctx.rs`](../../src/codegen/ctx.rs) (layout + inference + flow + imports)

---

## Non-Goals

* Do not redesign Twinkle language semantics.
* Do not remove existing typed fast paths.
* Do not replace ANF IR.

---

## Proposed Architecture

### 1. Introduce an explicit module emit plan

Add a planning artifact generated before emission, e.g.:

```text
ModuleEmitPlan {
  func_plans: Vec<FuncEmitPlan>,
  type_defs_to_emit: Vec<...>,
  helper_funcs_to_emit: Vec<...>,
  import_plan: Vec<ImportDef>,
}
```

Planning computes *what* to emit; emitter performs *how* to emit instructions.

### 2. Split `EmitCtx` into focused contexts

Refactor `EmitCtx` into smaller units:

* `LayoutCtx` (local slots, valtypes, storage policy)
* `ReprFlowCtx` (sum/iterator/closure/cell physical repr tracking)
* `EmitState` (label stack, loop stack, local indices)
* `ImportCollector` (runtime import accumulation)

`emit.rs` should not own global policy decisions via ad hoc mutable fields.

### 3. Centralize intrinsic lowering contracts

Keep intrinsic dispatch/ABI policy separate from instruction building:

* intrinsic registry says runtime vs intrinsic path and expected ABI
* emitter-specific handlers only produce instructions for a chosen lowering kind

This reduces duplicate policy checks scattered across call emission paths.

### 4. Add representation-flow verifier hooks

After planning and before final WAT encoding, run debug assertions that check:

* representation expectations at function/closure boundaries
* sum/iterator conversions only occur through canonical helpers
* no illegal direct casts in known boundary-sensitive paths

---

## Work Plan

### Phase 0: Baseline and safety rails

- [ ] Add characterization tests for current typed/erased closure, iterator, and sum boundary behavior.
- [ ] Add lightweight instrumentation counters for boundary conversions in debug builds.

### Phase 1: Module planning extraction

- [ ] Extract non-instruction planning from `emit_user_module` into a plan builder.
- [ ] Make helper/type registration deterministic from plan outputs.

### Phase 2: `EmitCtx` decomposition

- [ ] Move local layout logic out of the instruction emitter path.
- [ ] Move representation-flow mutation helpers into `ReprFlowCtx`.
- [ ] Keep emitter-facing API minimal and mostly read-only.

### Phase 3: Intrinsic dispatch layering

- [ ] Separate intrinsic policy decisions from instruction handlers.
- [ ] Replace large `FuncId` match blocks in call lowering with table-driven dispatch where practical.

### Phase 4: Verification and cleanup

- [ ] Add debug verifier pass/checks against planned boundaries.
- [ ] Remove path-local emergency guards made obsolete by phase split.

---

## Acceptance Criteria

1. Representation-boundary regressions can be triaged in planning vs emission layers separately.
2. `emit.rs` shrinks materially and no longer owns all policy/state concerns.
3. Boundary-sensitive tests (`Option/Result`, iterator, typed closure/cell) remain green in Wasm.
4. New verifier checks catch illegal boundary behavior before runtime traps.

---

## Immediate Next Steps

1. Extract `emit_user_module` planning steps into a dedicated `plan` module.
2. Introduce `ReprFlowCtx` and migrate flow mutation helpers out of the instruction emitter path.
3. Add one debug verifier for illegal sum/iterator boundary casts as first guardrail.
