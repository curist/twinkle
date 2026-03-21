# String Interning Plan

## Goal

Reduce allocation and repeated byte-comparison cost for `String` values in the Wasm backend, while keeping Twinkle source semantics unchanged.

## Current State

- `String` is represented as `rt_types__String` (`array (mut i8)`).
- Compile-time literal interning is implemented in codegen:
  - identical UTF-8 literal bytes are pooled per emitted module,
  - one mutable global + getter function are emitted per pooled literal,
  - use sites call the getter instead of materializing inline each time.
- Equality is pointer fast-path plus byte-by-byte fallback (`rt.core.eq` -> `rt.str.eq`).
- There is still no runtime intern table for dynamically produced strings.

## Status Snapshot

- Phase 1 (compile-time literal interning): landed.
- Phase 2 (runtime interning): not started, still optional.

## Scope

This plan has two deliverables:

1. Compile-time literal interning (required, complete)
2. Runtime interning for dynamically produced strings (optional follow-up, pending)

## Non-Goals

- Changing `String` surface API
- Changing UTF-8 semantics
- Introducing host-specific string storage assumptions

## Design Overview

### Phase 1: Compile-Time Literal Interning

Intern all identical string literals per linked module and emit one allocation site per unique literal.

- Implemented:
  - codegen literal pool with stable symbol suffixes derived from UTF-8 bytes,
  - one helper global/getter per pooled literal,
  - use-site lowering to pooled getter calls,
  - unchanged runtime `String` representation (`array<i8>`).

Observed/expected impact:

- Fewer allocations for repeated literals.
- More pointer-equality hits before byte compare.
- Smaller generated WAT for literal-heavy code.

### Phase 2: Runtime Interning (Optional)

Add a runtime intern table so dynamically produced strings can canonicalize to shared instances.

- Add `rt.str.intern(s: String) -> String`.
- Route selected constructors (`concat`, `substring`, numeric/bool conversions, host `f64_to_string`) through `intern`.
- Keep canonicalization semantics purely observational (no source-visible behavior change).

Expected impact:

- Lower allocation churn for repeated dynamic strings (keys, formatted values, etc.).
- Faster equality in hot string-compare paths due to increased pointer identity hits.

## Implementation Tasks

### Task A: Literal Pool in Codegen

- Status: complete.
- Landed in:
  - `src/codegen/ctx.rs` (literal pool bookkeeping + deterministic symbol suffixes),
  - `src/codegen/emit.rs` (pooled getter emission and use-site lowering).
- Unit tests exist in `src/codegen/emit.rs` for:
  - UTF-8 byte emission,
  - literal dedup through pooled getters,
  - lazy global initialization in pooled getters.

### Task B: Link/Emit Stability

- Status: complete for current pipeline.
- Snapshots include pooled literal globals/getters (for example in `tests/snapshots/build/*.wat`).
- Deterministic ordering is maintained via map ordering in emission.

### Task C: Runtime Interning API (Follow-Up)

- Status: pending.
- Add intern-table representation in runtime (likely in `rt.str`, or shared with a small hash map helper).
- Add `intern` entry to:
  - `src/runtime/str.rs`
  - `src/codegen/prelude.rs` (if surfaced to codegen)
- Decide whether to intern all dynamic strings or only selected producers.

## Validation

- Current validation for landed Phase 1:
  - Existing `run` / `run_wasm` / snapshot suites continue to pass.
  - Emitter tests cover literal pooling mechanics and UTF-8 byte handling.
  - Build snapshots show pooled-literal globals/getters.
- Remaining validation to add only if/when Phase 2 is implemented:
  - Dynamic-string canonicalization behavior checks.
  - Intern-table performance/overhead characterization.
  - Regression checks around host decode/encode paths with interning enabled.

## Rollout

1. Keep Phase 1 as default behavior (no feature flag).
2. Measure allocation/equality-heavy workloads to quantify additional upside from Phase 2.
3. Decide whether to implement Phase 2 based on measured wins vs runtime complexity/memory cost.
