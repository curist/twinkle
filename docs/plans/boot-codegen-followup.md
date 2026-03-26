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
codegen failures as outside scope. That limitation should remain visible until
it is either fixed or moved into a separate active plan.

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

Preferred outcome:

- `emit.tw` delegates match emission to `emit_pattern.tw`, or
- shared logic is extracted so both call sites use the same implementation
  bodies rather than mirrored copies.

Acceptance:

- one implementation path exists for match emission logic,
- string, variant, and scalar pattern coverage remains green,
- hardening exit criterion 4 becomes true in code, not just in prose.

### Phase 4 — Strengthen Boot-Side M11 Verification

Upgrade the boot-side suite so it proves more than WAT text shape.

Required additions:

- at least one behavior-level execution path for the core M11 regression
  families,
- at least one structural validation path for emitted modules,
- explicit runner verification in both interpreter mode and Wasm mode, using
  the preferred release binaries.

Acceptance:

- the boot-side suite exercises behavior, not just text output,
- the boot-side suite provides a meaningful local signal when Rust integration
  tests are too slow to run routinely.

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
4. The boot-side M11 suite validates behavior and at least one structural path
   in release mode.
5. The Rust regression harness has no debug-only or dead-code leftovers.
