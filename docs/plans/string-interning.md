# String Interning Plan

## Goal

Reduce allocation and repeated byte-comparison cost for `String` values in the Wasm backend, while keeping Twinkle source semantics unchanged.

## Current State

- `String` is represented as `rt_types__String` (`array (mut i8)`).
- String literals are emitted as fresh `array.new_fixed` at each use site.
- Equality is pointer fast-path plus byte-by-byte fallback (`rt.core.eq` -> `rt.str.eq`).
- There is no global or runtime string interning table.

## Scope

This plan has two deliverables:

1. Compile-time literal interning (required)
2. Runtime interning for dynamically produced strings (optional follow-up)

## Non-Goals

- Changing `String` surface API
- Changing UTF-8 semantics
- Introducing host-specific string storage assumptions

## Design Overview

### Phase 1: Compile-Time Literal Interning

Intern all identical string literals per linked module and emit one allocation site per unique literal.

- Add a literal pool in codegen that assigns stable symbols to unique byte sequences.
- Emit one internal helper/global per pooled literal.
- Replace inline literal materialization with a load/call to the pooled symbol.
- Keep runtime representation unchanged (`array<i8>`).

Expected impact:

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

- Update `src/codegen/emit.rs`:
  - Introduce a module-level string-literal pool.
  - Replace `emit_string_literal_atom` direct emission at use sites with pooled reference materialization.
  - Ensure deterministic symbol naming/order for snapshot stability.
- Add/adjust tests near existing literal-emission tests in `emit.rs`.

### Task B: Link/Emit Stability

- Verify linker and emitter handle added literal helper globals/functions deterministically.
- Update snapshots in:
  - `tests/snapshots/build/*.wat`
  - `tests/snapshots/runtime_dump_test__*.snap`

### Task C: Runtime Interning API (Follow-Up)

- Add intern-table representation in runtime (likely in `rt.str`, or shared with a small hash map helper).
- Add `intern` entry to:
  - `src/runtime/str.rs`
  - `src/codegen/prelude.rs` (if surfaced to codegen)
- Decide whether to intern all dynamic strings or only selected producers.

## Validation

- Functional parity tests pass (`run`, `run_wasm`, snapshots).
- New tests:
  - Repeated identical literals produce shared instance behavior (observable via equality fast-path proxies).
  - Unicode literals still round-trip as UTF-8 bytes.
  - No regressions in host decode/encode paths.

## Rollout

1. Land Phase 1 (literal interning) behind no feature flag.
2. Measure allocation and equality-heavy workloads.
3. Decide on Phase 2 based on measured wins vs added runtime complexity.
