# ANF Verifier Pass Plan

## Goal

Add a dedicated ANF verifier pass that runs between lowering/optimization/codegen and enforces
structural + typing invariants with actionable diagnostics.

Primary objective: stop Wasm-only panics/traps caused by metadata drift or representation mismatch
from reaching emission.

## Why Now

Recent regressions had the same shape:

- source-level program is valid and interpreter semantics are correct.
- ANF/codegen metadata becomes inconsistent at control-flow or representation boundaries.
- backend fails late (panic or Wasm cast/type failure) instead of producing a clear compiler error.

Examples:

- bare `break` inside value-typed `collect` loop (`Void` synthesized where `ref` is required).
- typed iterator-option payload metadata drifting in `Option.Some(IterItem)` match paths.
- closure-capture optimization corner case around shadowed locals.

## Scope

In scope:

- ANF structural invariants.
- control-flow/value typing invariants.
- backend representation invariants needed by Wasm codegen.
- pass-to-pass validation hooks in optimization pipeline.

Out of scope:

- changing language semantics.
- replacing typechecker.
- full formal proof of optimizer correctness.

## Verifier Invariants

### 1) Control-Flow / Result-Type Invariants

- Loop result type is coherent:
  - value-typed loops: every reachable `break` carries a value assignable to loop result type.
  - unit loops: `break` value must be absent or `Void`.
- `continue` appears only inside loop context.
- `if`/`match` arms that join produce compatible result types.
- `return` atoms are compatible with function return type.
- no impossible stack-shape joins after divergence rewriting.

### 2) Local Binding Invariants

- every used `LocalId` has a declared local mapping and stable physical Wasm type.
- rebind/assign operations do not violate local physical type constraints.
- metadata maps (`local_mono`, value repr, iterator metadata, sum repr) are internally consistent.

### 3) Representation Boundary Invariants

- typed sum representations match local physical ref types.
- typed iterator-next option (`Option<IterItem<T>>`) metadata is consistent with local physical type.
- typed `UnfoldStep<T,S>` metadata is concrete where required.
- closure capture bindings referenced by `free_vars` are preserved.

### 4) Pattern-Binding Invariants

- variant pattern field expectations match selected representation path.
- `Option.Some(IterItem<T>)` pattern locals get compatible `IterItem` physical type metadata.
- no pattern path assumes erased `Variant` layout for a typed local (or vice versa) without explicit conversion.

## Pipeline Integration

Planned insertion points:

1. After Core->ANF lowering (baseline verifier).
2. After each optimization pass in ANF pipeline (debug-mode initially; later always-on for key checks).
3. Immediately before codegen emission (hard gate).

If verification fails, stop compilation with:

- invariant name,
- function id/name,
- local id(s),
- offending ANF fragment span if available,
- expected vs actual representation/type details.

## Implementation Plan

### Phase 1: Framework + Control-Flow Core

- Add `src/ir/anf/verify.rs`.
- Implement verifier context (function return type, loop stack, local type table snapshot).
- Enforce control-flow/result-type invariants.
- Wire into pipeline post-lowering and pre-emission.

### Phase 2: Representation Consistency

- Validate local representation metadata coherence:
  - iterator state / iterator-next state / iter-item state
  - sum repr typed vs erased
  - concrete typed symbols vs local physical types
- Migrate ad-hoc assertions (e.g. unfold-step checks) into verifier rules.

### Phase 3: Optimizer Boundary Checks

- Run verifier after each ANF opt pass in debug mode.
- Add pass attribution in errors (`failed after pass X`).
- Add focused regression tests for known bug families.

### Phase 4: Diagnostics + Hardening

- Improve error text for quick triage.
- Add snapshot tests for verifier diagnostics.
- Turn selected invariants on in release builds (or keep all with low overhead).

## Implementation Checklist

### Setup

- [ ] Create `src/ir/anf/verify.rs` module skeleton.
- [ ] Add verifier entrypoint API (`verify_module` / `verify_function`).
- [ ] Wire verifier invocation into post-lowering pipeline stage.
- [ ] Wire verifier invocation into pre-codegen stage.

### Phase 1 Checklist

- [ ] Track function return-type context during ANF walk.
- [ ] Track loop stack and expected loop result type.
- [ ] Validate `break`/`continue` loop-context legality.
- [ ] Validate `break` value compatibility with loop result type.
- [ ] Validate `return` value compatibility with function return type.
- [ ] Add unit tests for control-flow invariant failures.

### Phase 2 Checklist

- [ ] Validate local mapping existence for every referenced `LocalId`.
- [ ] Validate physical local type stability across rebinding/assign.
- [ ] Validate iterator metadata coherence (`iterator_state`, `iterator_next_state`, `iter_item_state`).
- [ ] Validate sum representation coherence (`typed` vs `erased`).
- [ ] Validate typed symbol ↔ local ref-type consistency.
- [ ] Replace existing ad-hoc unfold-step assertions with verifier checks.

### Phase 3 Checklist

- [ ] Add verifier hook after each ANF optimization pass in debug mode.
- [ ] Annotate verifier errors with pass name when failing post-pass.
- [ ] Add regression fixture for `collect + break + unfold`.
- [ ] Add regression fixture for typed iterator-option pattern path.
- [ ] Add regression fixture for closure shadow/capture propagation path.

### Phase 4 Checklist

- [ ] Add structured verifier error type with invariant identifiers.
- [ ] Include function/local/span context in all verifier errors.
- [ ] Add snapshot tests for verifier diagnostics output.
- [ ] Decide release-mode policy (always-on vs selected invariants).
- [ ] Document verifier guarantees and limitations in internals docs.

## Testing Strategy

- Positive fixtures: existing `tests/run/*.tw` and `tests/run_wasm_test.rs`.
- Negative verifier fixtures: new `tests/anf_verify_fail/*.tw` or synthetic ANF unit tests.
- Regression guards:
  - `collect + break + unfold` path
  - typed iterator option pattern path
  - closure shadow/capture propagation path

## Risks / Tradeoffs

- Verifier can duplicate logic from codegen inference if not carefully centralized.
- Overly strict checks may block valid programs if invariants are underspecified.
- Runtime overhead if always-on after every pass; mitigate via phased rollout.

## Success Criteria

- No backend panic for invariant violations that can be diagnosed earlier.
- New representation/control-flow regressions fail in verifier stage with actionable messages.
- Reduced frequency of Wasm-only divergence from interpreter on regression fixtures.
