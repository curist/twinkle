# Boot Codegen Hardening Plan

Last updated: 2026-03-25

## Goal

Close the correctness and implementation-drift gaps found while reviewing the
current boot Phase D implementation against
[boot-codegen.md](boot-codegen.md).

This plan is a follow-up to [boot-codegen.md](boot-codegen.md), not a
replacement. The original document still defines the intended Phase D
architecture. This document focuses on places where the current code either:

- does not yet implement the documented invariant,
- relies on a transitional workaround that should be removed,
- or has duplicated logic that is likely to drift further.

The intent is to make the current bootstrap backend safer and more auditable
before expanding Phase D scope again.

---

## Problems To Fix

### 1. Narrowing is still lossy in the backend

The Phase D plan explicitly requires a checked `i64 -> i32` narrowing helper
for all index/length/ABI `i32` paths. The current implementation still has:

- `emit_checked_i32_narrow()` implemented as plain `I32WrapI64`
- direct `I32WrapI64` insertion in runtime-call ABI adaptation

This recreates the stage0 bug family the plan was supposed to eliminate.

Primary files:

- `boot/compiler/codegen/emit.tw`

### 2. Closure representation has a partial fallback path

The planner and typed-call path assume a 3-field typed closure subtype:

- universal funcref
- env
- typed funcref

But the runtime base closure is still 2-field, while the fallback branch in
`emit_make_closure()` still tries to build a base closure with 3 pushed
operands. That leaves the backend depending on a partially completed closure
representation story.

Primary files:

- `boot/compiler/codegen/wasm_plan.tw`
- `boot/compiler/codegen/runtime/types.tw`
- `boot/compiler/codegen/emit.tw`

### 3. String-pool offsets are not deterministic by construction

String literal offsets are assigned by iterating `string_pool.keys()`, and the
data segment is later rebuilt by iterating the same map again. If iteration
order changes, `byte_offset` no longer matches the emitted bytes.

That is a correctness risk and a code smell even if the current `Dict`
happens to behave deterministically.

Primary files:

- `boot/compiler/codegen/wasm_plan.tw`
- `boot/compiler/codegen/emit.tw`

### 4. Pattern-match emission exists in two divergent implementations

The dedicated M5 module exists and has its own suite, but production emission
in `emit.tw` reimplements matching separately. The two copies have already
drifted:

- `emit_pattern.tw` still rejects string literal patterns
- `emit.tw` supports them directly

This is primarily a maintainability problem today, but it will turn into a
correctness problem as one path evolves without the other.

Primary files:

- `boot/compiler/codegen/emit_pattern.tw`
- `boot/compiler/codegen/emit.tw`
- `boot/tests/suites/emit_pattern_suite.tw`

### 5. Module globals still use an erased side channel

Module-global locals are currently:

- boxed to `anyref` on writes in `__init__`
- stored in `anyref` globals
- unboxed again on reads in non-init functions

This works as a bootstrap strategy, but it is representation drift from the
"decide once, early" design and it creates avoidable boxing paths.

Primary files:

- `boot/compiler/codegen/wasm_plan.tw`
- `boot/compiler/codegen/emit.tw`

### 6. Boundary insertion over-wraps typed refs

The current boundary pass treats only raw `Anyref` as already-erased. Typed ref
values like `String`, record refs, sum refs, vector refs, and dict refs still
acquire explicit `AWrapAnyref` lets even though emission later reduces many of
them to no-op upcasts.

That is not the highest-priority correctness issue, but it adds IR noise and
makes boundary traces harder to audit.

Primary files:

- `boot/compiler/codegen/insert_boundaries.tw`
- `boot/compiler/codegen/emit.tw`

---

## Out of Scope

This plan does not include:

- replacing the shared bootstrap runtime container families with fully typed
  per-instantiation families
- the persistent vector or persistent dict work
- Phase E library/module work
- broad frontend feature expansion unrelated to the issues above

If a fix naturally reduces future anyref reliance, that is good, but this plan
is about hardening the current Phase D implementation, not finishing the
post-bootstrap architecture.

---

## Workstreams

### Phase 1 — Restore Checked Narrowing Invariants

Implement the narrowing rule described in `boot-codegen.md` and remove direct
lossy wraps from ABI lowering.

Tasks:

1. Replace `emit_checked_i32_narrow()` with a real checked helper.
   - Input: `i64`
   - Check: `0 <= value <= i32::MAX`
   - Failure mode: trap via `Unreachable` or the standard trap path
   - Output: `i32`

2. Route all backend narrowing through that helper.
   - array index operations
   - string index/slice operations
   - vector/dict/string runtime helper calls with `i32` ABI parameters
   - intrinsic helpers that currently emit bare `I32WrapI64`

3. Delete direct ABI-path narrowing in `emit_runtime_call()` unless the caller
   is already proven to be in a checked-safe context.

4. Add negative-path regressions using large positive `Int` values and negative
   values for all user-visible index/slice/make paths.

Acceptance:

- no bare `I32WrapI64` remains on user-controlled narrowing paths
- large positive and negative out-of-range indices trap instead of wrapping
- the Rust boot-codegen equivalence harness covers the large-index regression
  matrix explicitly

### Phase 2 — Finish Closure Representation Cleanup

Make closure construction and invocation use one coherent representation story.

Tasks:

1. Choose one bootstrap closure policy and apply it consistently.
   - preferred: typed closure subtype plus universal base closure, as described
     in `boot-codegen.md`
   - fallback option: explicitly route unsupported closure shapes through the
     universal base path without pretending they have typed field 2

2. Remove the invalid mixed fallback in `emit_make_closure()`.

3. Make closure-call emission check whether a local is:
   - definitely a typed closure subtype,
   - or only guaranteed to satisfy the universal closure ABI.

4. Ensure planner/runtime/emitter agree on:
   - field count
   - field order
   - functype registration
   - trampoline generation requirements

5. Add focused regressions for:
   - direct first-class function values
   - captured closures
   - closure arguments crossing call boundaries
   - closure returns

Acceptance:

- no closure constructor emits a field count that disagrees with its type def
- typed and universal closure calls both validate and run correctly
- closure-related code paths no longer depend on partially initialized
  "typed-funcref maybe null" behavior

### Phase 3 — Make String Pool Planning Deterministic

Turn string literal pooling into an ordered plan rather than a map-order
accident.

Tasks:

1. Extend the registry with explicit string-pool emission order.
   - Example: `string_entries: Vector<StringPoolEntry>` or equivalent
   - Keep map lookup for deduplication, but do not derive layout from map order

2. Assign `byte_offset` from that explicit ordered sequence at plan time.

3. Emit globals, getters, and data segments from the same stable sequence.

4. Add tests that verify:
   - stable offsets across multiple strings
   - emitted data bytes line up with the planned offsets
   - duplicate literals reuse the same pool entry

5. Prefer deterministic ordering suitable for future WAT snapshotting.

Acceptance:

- data segment byte order is explicit and test-covered
- getter offsets always match actual emitted bytes
- string pool behavior no longer depends on `Dict.keys()` ordering

### Phase 4 — Unify Match Emission

Remove duplicated match compilation logic so the tested matcher is the one used
by production emission.

Tasks:

1. Make `emit.tw` delegate `AMatch` lowering to `emit_pattern.tw`, or move the
   shared logic into a single reusable implementation unit.

2. Preserve current production-only behavior that the dedicated module lacks.
   - especially string literal pattern support
   - any body-emission integration hooks required by the main emitter

3. Delete or collapse duplicated helpers after the unified path is working.

4. Expand the matcher suite to include:
   - string literal patterns
   - nested variant patterns
   - diverging-arm matches
   - short-circuit payload access cases

Acceptance:

- there is one authoritative match-emission implementation
- `emit_pattern_suite.tw` and end-to-end codegen exercise the same path
- string literal patterns are supported without special-case duplication

### Phase 5 — Replace Erased Module Globals With Typed Globals

Remove the ad hoc "store everything as anyref" global bridge where possible.

Tasks:

1. Extend module-global planning so each promoted global carries its concrete
   `MonoType` and final `ValType`, not just a symbol name.

2. Emit globals with their real Wasm type when the type is storable as a global.
   - scalars
   - strings
   - records/sums/closures/vector/dict refs

3. Keep a narrow erased fallback only for types that truly cannot be represented
   directly in bootstrap globals, and make that fallback explicit in the plan.

4. Remove unnecessary boxing/unboxing on non-erased global reads and writes.

5. Add regressions covering:
   - scalar globals
   - string globals
   - record/sum global reads from non-init functions

Acceptance:

- most promoted globals are emitted with concrete Wasm types
- non-init reads do not universally round-trip through `anyref`
- any remaining erased fallback is explicit, justified, and test-covered

### Phase 6 — Tighten Boundary Insertion Semantics

Keep explicit boundaries, but stop generating obvious no-op wrappers.

Tasks:

1. Refine `needs_wrap()` / `needs_unwrap()` so "already a ref subtype of
   anyref" is treated differently from "still needs boxing".

2. Preserve explicit boundaries where they still add audit value.
   - scalar boxing into element/key/value positions
   - scalar unboxing from erased results

3. Avoid generating extra wrapper locals for:
   - `String`
   - record/sum/closure refs
   - vector/dict/container self refs

4. Extend the boundary suite with "no-op wrap not inserted" assertions for
   typed ref cases.

Acceptance:

- boundary IR is smaller and easier to inspect
- scalar anyref crossings remain explicit
- typed ref crossings no longer produce redundant wrapper lets

---

## Validation

Each phase above should land with both focused and end-to-end validation.

### Focused suites

- `boot/tests/suites/wasm_plan_suite.tw`
- `boot/tests/suites/insert_boundaries_suite.tw`
- `boot/tests/suites/emit_pattern_suite.tw`
- targeted additions to `boot/tests/suites/codegen_integration_suite.tw`

### Rust integration harness

Extend `tests/boot_codegen_integration_test.rs` to cover:

- large-index narrowing regressions
- closure direct/indirect call shapes
- string literal pattern matches
- promoted global reads/writes across functions

### Structural validation

Keep Wasmtime compilation validation in place and add `wasm-tools validate`
when practical so that malformed fallback paths fail faster and more locally.

---

## Recommended Order

1. Phase 1 — narrowing
2. Phase 2 — closure representation
3. Phase 3 — deterministic string pool
4. Phase 4 — match unification
5. Phase 5 — typed module globals
6. Phase 6 — boundary cleanup

This order fixes correctness hazards first, then removes drift-inducing
workarounds.

---

## Exit Criteria

This hardening pass is complete when all of the following are true:

1. Narrowing no longer relies on lossy wraps for user-controlled `i64 -> i32`
   paths.
2. Closure construction/call/trampoline code uses one internally consistent
   representation story.
3. String pool offsets are deterministic and derived from explicit ordered
   planning state.
4. Pattern matching has one implementation path used by both unit tests and
   production emission.
5. Promoted module globals are typed by default rather than erased by default.
6. Boundary insertion only emits wrappers where a real representation change
   occurs.
7. The Rust boot-codegen integration harness remains green on the regression
   matrix and grows coverage for the hardened paths.
