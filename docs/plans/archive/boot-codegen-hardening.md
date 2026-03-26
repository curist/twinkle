# Boot Codegen Hardening Plan

Last updated: 2026-03-26

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

### Phase 1 — Restore Checked Narrowing Invariants ✓ DONE

`emit_checked_i32_narrow` in `boot/compiler/codegen/emit.tw` now takes
`(atom, ctx)`, emits a real range check (`0 <= val <= 2147483647`, traps via
`unreachable`), then wraps. All 11 call sites updated.
`emit_runtime_call` ABI narrowing also routes through the checked helper (no
more bare `I32WrapI64` on user-controlled paths).

Validation:

- 3 RED→GREEN tests in `codegen_integration_suite.tw` (vector index, string
  index, runtime call)
- `tests/run/large_index_narrowing.tw` fixture added to Rust regression matrix
  (45 cases)

Note: stage0 Rust codegen still has the wrapping bug for `vector_get` with
large i64 indices — the i64-domain bounds check catches most cases, but
`vector_make` and `vector_set_in_place` would silently wrap. Boot codegen is
now stricter than stage0.

### Phase 2 — Finish Closure Representation Cleanup ✓ DONE

Removed the extra `RefNull(.Func)` push in `emit_make_closure`'s universal
fallback path (`emit.tw:1893-1899`). Now correctly pushes 2 operands for the
2-field `rt_types__Closure` base struct.

Validation:

- 3 regression tests added: typed closure struct creation, subtype
  relationship, typed funcref `call_ref`
- The universal fallback is currently unreachable in practice (all closures
  route through the typed 3-field path), but the code was wrong

Known limitation: closures with captures (`fn() { x }`) trigger a separate
pre-existing bug (`lookup_local: unknown LocalId L0`) in the boot codegen —
outside scope of this plan.

### Phase 3 — Make String Pool Planning Deterministic ✓ DONE

Added `string_pool_order: Vector<String>` to `WasmTypeRegistry` alongside the
existing `string_pool: Dict<String, StringPoolEntry>` dedup map. The vector
records insertion order and is the single source of truth for layout and
emission:

- `register_string` appends to `string_pool_order` and computes `byte_offset`
  by iterating the ordered vector (not dict keys)
- `emit_string_pool_globals`, `emit_string_pool_getters`, and
  `emit_string_data_segment` all iterate `string_pool_order` instead of
  `string_pool.keys()`

No `Dict.keys()` calls remain on the string pool path.

### Phase 4 — Unify Match Emission ✓ PARTIAL

Closed the feature gap between the two implementations rather than merging
them, because the nominal type system makes ctx bridging impractical:

- `emit_pattern.tw`'s `EmitCtx` now carries a `string_getter: fn(String) String?`
  callback, enabling `LitStr` pattern support via string pool lookup
- `emit_atom` in `emit_pattern.tw` also handles `ALitStr` via the callback
- `emit_pattern_suite.tw` has a new "literal string pattern emits rt_str__eq
  call" test covering the string pool getter and `rt_str__eq` call emission
- Both implementations (`emit.tw` production, `emit_pattern.tw` testable) now
  support all pattern types: Wildcard, Var, LitInt, LitBool, LitStr, Variant

Full unification (single code path) is blocked by the two modules having
different nominal `EmitCtx` types. The doc comments now document the
relationship and the requirement to keep them in sync.

### Phase 5 — Replace Erased Module Globals With Typed Globals ✓ DONE

Replaced `module_globals: Dict<Int, String>` with
`Dict<Int, ModuleGlobalEntry>` where `ModuleGlobalEntry` carries both `sym`
and `mono: MonoType`. The MonoType is resolved from the init function's
`op_result_mono` during planning.

Changes:

- `emit_module_globals` uses `val_type_of_mono(gentry.mono, env)` to emit
  each global with its concrete Wasm type (I64 for Int, F64 for Float, I32
  for Bool/Byte, typed refs for String/records/sums/etc.)
- `__init__` write path: removed `emit_box_to_anyref` — direct `global.set`
- Non-init read path: removed `emit_unbox_from_anyref` — direct `global.get`
- `wasm_plan_suite.tw` updated: `test_plan_module_globals` now asserts the
  stored MonoType matches the local's type

### Phase 6 — Tighten Boundary Insertion Semantics ✓ DONE

`is_anyref` in `insert_boundaries.tw:59` was too narrow — only matched literal
`.Anyref`, not typed refs like `Ref(true, Named("rt_types__String"))`. Fixed to
recognize typed refs as already-erased.

Validation:

- 3 previously failing tests in `insert_boundaries_suite.tw` (lines 212, 280,
  330) now pass — these tested exactly the typed-ref boundary case

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

1. ~~Phase 1 — narrowing~~ ✓
2. ~~Phase 2 — closure representation~~ ✓
3. ~~Phase 3 — deterministic string pool~~ ✓
4. ~~Phase 4 — match unification~~ ✓
5. ~~Phase 5 — typed module globals~~ ✓
6. ~~Phase 6 — boundary cleanup~~ ✓

All phases complete.

---

## Files Changed

### Phases 1–2, 6

- `boot/compiler/codegen/emit.tw` — checked narrowing + closure fix
- `boot/compiler/codegen/insert_boundaries.tw` — typed ref `is_anyref` fix
- `boot/tests/suites/codegen_integration_suite.tw` — 6 new tests
- `boot/tests/suites/insert_boundaries_suite.tw` — 3 tests fixed
- `tests/run/large_index_narrowing.tw` — new fixture
- `tests/boot_codegen_integration_test.rs` — fixture added to matrix

### Phases 3–5

- `boot/compiler/codegen/wasm_plan.tw` — `string_pool_order` field,
  `ModuleGlobalEntry` type, typed module global planning
- `boot/compiler/codegen/emit.tw` — deterministic string pool iteration,
  typed module global read/write (no boxing/unboxing)
- `boot/compiler/codegen/emit_pattern.tw` — `string_getter` callback on
  EmitCtx, LitStr pattern support
- `boot/tests/suites/wasm_plan_suite.tw` — updated registry constructors,
  typed global assertion
- `boot/tests/suites/emit_pattern_suite.tw` — string literal pattern test
- `boot/tests/suites/insert_boundaries_suite.tw` — updated registry constructor

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
   production emission. **Not yet met** — two implementations exist with
   matching feature coverage but separate code. Tracked in
   [boot-codegen-followup.md](boot-codegen-followup.md) Phase 3.
5. Promoted module globals are typed by default rather than erased by default.
6. Boundary insertion only emits wrappers where a real representation change
   occurs.
7. The Rust boot-codegen integration harness remains green on the regression
   matrix and grows coverage for the hardened paths.
