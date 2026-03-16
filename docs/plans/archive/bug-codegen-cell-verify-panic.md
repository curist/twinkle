# Bug: Codegen Cell Verification Panic on Large Module Graphs

**Severity:** Low (debug_assert only — release builds unaffected)
**Component:** Codegen verification (`src/codegen/ctx.rs:683`)

---

## Symptom

When compiling a sufficiently large module graph (e.g., boot tests with 16+ suites including the checker module), the codegen verification step panics:

```
codegen verify: add_file (FuncId(194)): L18 has cell ref type cell_T22 but no TypedCell repr or Cell mono
```

The panic occurs in `verify_codegen_metadata` during `setup_locals_with_extra`, called from `collect_capture_mono_by_func` in the WAT emit pipeline.

## Reproduction

```bash
# Fails with debug_assert panic:
cargo run -- run boot/tests/main.tw

# Passes in release mode (debug_assert skipped):
cargo run --release -- run boot/tests/main.tw
# → 368 tests: 368 passed
```

The panic does NOT occur when the checker module is excluded from the test suite (363 tests pass in debug mode). Adding the checker module increases the module graph enough to trigger the issue.

## Key Observations

1. **Release mode works** — all 368 tests pass, producing correct output
2. **Debug-only** — the `debug_assert` at `ctx.rs:683` fires but the actual codegen produces valid code
3. **Threshold-dependent** — only triggers with enough modules linked; the checker module's types (Dict, Vector, closures) push it past the threshold
4. **Cell + type variable** — the failing local has ref type `cell_T22` where `T22` is a Wasm type variable, suggesting a polymorphic Cell usage that isn't fully resolved in codegen metadata

## Likely Cause

In `verify_codegen_metadata`, the check requires that any local with a `cell_*` Wasm ref type has either:
- A `TypedCell` repr in `repr_flow`, OR
- A `Cell` mono type in `local_mono`

For polymorphic Cell closures captured across module boundaries, the monomorphization or typed-cell specialization may not fully populate `local_mono` for all captured Cell locals, even though the actual codegen handles them correctly via the erased path.

## Investigation Areas

- `collect_capture_mono_by_func` (`src/codegen/emit.rs:572`) — check how captured Cell locals get their mono types populated
- Cross-module closure captures involving Cell — the `api_cell_suite` tests exercise Cell, but the additional modules from checker may change FuncId/TypeId assignments
- The verification logic may be too strict for the erased Cell path

## Impact

No impact on correctness — release builds work. Only affects developer experience when running `cargo run` (debug mode) with large boot test suites.

## Workaround

Use `cargo run --release` for boot test execution.
