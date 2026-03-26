# Boot Codegen Follow-Up Plan

Last updated: 2026-03-26

## Goal

Consolidate the current Phase D review into one active tracking doc:

- confirm which items from
  [archive/boot-codegen-hardening.md](archive/boot-codegen-hardening.md) are
  actually addressed in code,
- record the remaining structural and documentation drift,
- and define the smallest follow-up work needed before Phase D should be
  treated as cleanly closed.

This document is a follow-up to [boot-codegen.md](boot-codegen.md). It does not
replace the Phase D design there.

---

## Verification Snapshot

Confirmed in release mode during this review:

- `env TWK_TEST_FILTER='codegen integration' ./target/release/twk run -i boot/tests/main.tw`
  is green.
- `./target/release/twk run -i boot/tests/main.tw` runs successfully and
  remains broadly green.

Observed but not completed within this review window:

- `cargo test --release --test boot_codegen_integration_test`
- `env TWK_TEST_FILTER='codegen integration' ./target/release/twk run boot/tests/main.tw`

These two slower paths should remain part of the closure bar, but they are not
counted as freshly re-verified here.

---

## Current Assessment

### Verified addressed from hardening

The following items from
[archive/boot-codegen-hardening.md](archive/boot-codegen-hardening.md) are
implemented as described:

1. Checked `i64 -> i32` narrowing is restored in
   `boot/compiler/codegen/emit.tw`.
2. The closure fallback no longer pushes the wrong operand count for
   `rt_types__Closure`.
3. String-pool planning/emission now uses explicit ordered state
   (`string_pool_order`).
4. Promoted module globals are planned and emitted as typed globals.
5. Boundary insertion now treats typed refs as already-erased.
6. `emit_pattern.tw` gained `LitStr` support and focused coverage for it.

### Not actually closed yet

The following items are still open despite some docs implying otherwise:

1. Pattern matching still has two implementations.
   `emit_pattern.tw` and `emit.tw` must be kept in sync manually.
   This means hardening exit criterion 4 is not met.
2. The boot-side M11 suite is still mostly a compile-to-WAT substring suite.
   It does not yet exercise runtime behavior or structural validation the way
   the Rust regression harness does.
3. Phase D still threads a separate `abi_table` derived from `BuiltinEntry.abi`
   into emission. That is redundant metadata and contradicts the “single source
   of truth” claim in `boot-codegen.md`.
4. The active docs are stale.
   `boot-codegen.md` still says M11 is not closed and links to
   `boot-codegen-m11-gap-closure.md` in the wrong location.
5. The Rust regression harness still contains debug/test scaffolding that
   should be removed or converted:
   `debug_dump_return_if_wat()` and the unused `validated_module()`.

### Known limitation still worth tracking

`archive/boot-codegen-hardening.md` explicitly calls out closure-with-captures
codegen failures as outside scope. This is feature-level backend work, not
structural drift — it belongs in a separate active plan, not this follow-up.
A dedicated plan should be created when we choose to tackle it.

---

## Workstreams

### Phase 1 — Reconcile Phase D Status Docs

Update the active Phase D docs so they match reality.

Required changes:

- remove the stale “M11 is not fully closed yet” wording from
  `boot-codegen.md`,
- replace the broken follow-up link with the correct active tracking doc,
- align `archive/boot-codegen-hardening.md` with its real closure state, or add
  a clear note that only items 1, 2, 3, 5, and 6 are fully closed while match
  unification remains partial.

Acceptance:

- active docs no longer claim open bugs that are already covered by the current
  regression matrix,
- active docs no longer claim full closure where duplicated implementation
  paths still exist.

### Phase 2 — Collapse ABI Metadata to One Channel

Remove the extra `abi_table` plumbing and have emission consult
`BuiltinRegistry` / `BuiltinEntry.abi` directly.

Targets:

- `boot/compiler/codegen/codegen.tw`
- `boot/compiler/codegen/emit.tw`
- `boot/compiler/codegen/insert_boundaries.tw`

Acceptance:

- `make_abi_table()` is deleted,
- `EmitCtx` no longer carries a duplicated ABI map,
- runtime-call narrowing and result adaptation still pass existing coverage.

### Phase 3 — Finish Match-Emitter Consolidation

Move from “two implementations kept in sync” to one production path.

Direction: merge `emit_pattern.tw` into `emit.tw`. `emit_pattern.tw` exists only
as a testable extraction — its sole caller is the boot test suite
(`boot/tests/suites/emit_pattern_suite.tw`), not production emission. Rather than
keeping a second module, fold its tested logic back into `emit.tw` and update or
remove the test suite that depended on it.

Steps:

1. Review any coverage in `emit_pattern_suite.tw` that is not already covered by
   the Rust integration harness or the boot M11 suite. Port missing cases.
2. Merge `emit_pattern.tw` helpers into `emit.tw`, replacing the mirrored copies.
3. Delete `emit_pattern.tw` and `emit_pattern_suite.tw`.
4. Verify all pattern families (string, variant, scalar, wildcard) remain green.

Acceptance:

- `emit_pattern.tw` no longer exists,
- one implementation path exists for match emission logic in `emit.tw`,
- string, variant, and scalar pattern coverage remains green,
- hardening exit criterion 4 becomes true in code, not just in prose.

### Phase 4 — Strengthen Boot-Side M11 Verification

Upgrade the boot-side suite so it proves more than WAT text shape.

Prerequisite: the boot-side harness currently has no way to compile and execute
generated WAT. `boot/tests/helpers/emit_boot_wat.tw` only prints WAT text. All
actual WAT parsing, Wasmtime validation, and execution live on the Rust side
(`tests/boot_codegen_integration_test.rs`). Behavior-level validation from
Twinkle would require new host capabilities (e.g. a `run_wat` builtin or an
external validator invocation).

Achievable without new infrastructure:

- structural checks on emitted WAT (e.g. asserting specific instruction
  sequences, section presence, or export names in the text output),
- verifying that `twk run boot/tests/main.tw` in Wasm mode covers the full
  suite (this exercises the boot test code under stage0 Wasm execution, though
  it does not validate boot-generated user WAT).

Deferred until host support exists:

- behavior-level execution of boot-generated WAT from within the boot suite,
- structural validation via Wasmtime or equivalent from Twinkle code.

Acceptance:

- the boot-side suite adds at least one structural validation path beyond
  substring matching,
- the scope boundary between "what boot tests can verify now" and "what needs
  new host infra" is documented.

### Phase 5 — Clean Regression Harness Artifacts

Remove leftover debug-only pieces from the Rust harness.

Targets:

- `tests/boot_codegen_integration_test.rs`

Acceptance:

- `debug_dump_return_if_wat()` is removed or converted to a real assertion,
- `validated_module()` is either used or deleted,
- release-mode harness runs without dead-code noise.

---

## Recommended Order

1. Reconcile docs first.
2. Remove the ABI side channel.
3. Consolidate match emission.
4. Strengthen the boot-side M11 suite.
5. Clean the Rust harness artifacts.

---

## Exit Criteria

This follow-up is complete when all of the following are true:

1. The active Phase D docs accurately describe what is closed and what remains.
2. `BuiltinEntry.abi` is the only ABI metadata channel used by planning,
   boundary insertion, and emission.
3. Match emission has one implementation path.
4. The boot-side M11 suite adds at least one structural validation path beyond
   substring matching, and the scope boundary for future host-dependent
   verification is documented.
5. The Rust regression harness has no debug-only or dead-code leftovers.
