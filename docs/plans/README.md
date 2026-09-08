# Twinkle Implementation Plan

## Goal

Drive Twinkle toward a self-hosted compiler (`twc.wasm`) while keeping stage0
delivery practical and the active plan set actionable.

## Architecture Reference

Architecture details are consolidated in
[docs/design/compiler-architecture.md](../design/compiler-architecture.md):

* goal and high-level pipeline
* runtime/linker and host-interface shape
* design principles
* current repository layout

---

## Plan Lifecycle

To keep this directory actionable:

* `docs/plans/` top level contains active WIP/planned documents.
* completed plans are moved to `docs/plans/archive/`.
* archived stage/history indexes live in [archive/README.md](archive/README.md).

---

## Active Plan Index

Historical/completed indexes are in [archive/README.md](archive/README.md).

### Performance

Self-hosting is complete. Active performance tracking now lives under one parent
folder, split by outcome: generated-program runtime speed and compiler
throughput.

| Area | Description | Status | Details |
|------|-------------|--------|---------|
| Performance | Track compiled-program runtime performance, compiler performance, vector/order-by work, and dataframe benchmark context | In Progress | [performance/](performance/README.md) |


### Later — Tooling & Ecosystem

| Area | Description | Status | Details |
|------|-------------|--------|---------|
| Tooling | Formatter (done), linter, LSP, package manager | In Progress | [tooling.md](tooling.md) |
| Embeddable lib build | `twk build --lib` exports the entry's `pub` primitive functions/values as named wasm exports; compiler-free `loadLib` + Node/web scaffold harness | Done | [embeddable-lib-build.md](embeddable-lib-build.md) |
| Full lib-export ABI | Widen lib exports beyond primitives: `String`, callbacks, compounds (`Vector`/`Dict`/records), and returned closures, with bridge-backed `loadLib` marshalling | Done | [lib-export-abi.md](lib-export-abi.md) |
| LSP enhancements | Document symbols, references, rename, signature help, semantic tokens, workspace symbols, highlights, inlay hints, folding, and incremental sync | Planned | [lsp-enhancements.md](lsp-enhancements.md) |
| LSP code actions | Quick-fix actions: missing case arms, auto-import, function type annotations | Planned | [lsp-code-actions.md](lsp-code-actions.md) |
| Rebinding-ceremony fixers | Skeleton: three `twk lint`/`fix` rules from manual "tidy up" rewrites — named-ctor to anonymous `.{}` (A), numbered-accumulator rebind (B), full-record reconstruction to field rebind (C). A landed as `redundant-record-prefix` (three anchors: annotated `let`, declared return, record-field value; call-argument anchoring deferred from v1), applied across boot (2026-08-06, re-applied 2026-08-07). C's return-position variant landed via `record-copy-helper` (2026-08-06); B and C's let-binding/nested/block-expr variants remain | Planned (skeleton; A done, C partial) | [lint-rebinding-fixers.md](lint-rebinding-fixers.md) |
| LSP contract hover | Hover information for builtin contract bounds and contract-backed method calls | Done | [archive/lsp-contract-hover.md](archive/lsp-contract-hover.md) |
| Runtime stack traces | Source-mapped trap reporting: catch runtime traps (all kinds) and render a Twinkle stack trace with `file:line:col` + snippet & caret, reusing the compile-diagnostic renderer. Approach A — `name` + `twinkle.debug` custom sections, host symbolication in `twk run`. `twk run`/Deno first. Phases 0–2 done (end-to-end for error/div0/OOB). | In Progress | [runtime-stack-traces.md](runtime-stack-traces.md) |
| Disk-backed debug info | Follow-on: drop inline source text from `twinkle.debug`. **Milestone 1** — keep absolute paths + line/col + PC maps, and read snippets from disk at render time (scoped to `twk run`, 3-arg renderer unchanged, no host-boundary change). Shrinks the boot compiler's ~2× debug bloat; `twk build` artifacts and other runs degrade to `file:line:col` + backtrace. **Milestone 2 (deferred)** — project-relative paths + `source_root` safe-join + `find_project_root` fix for portable/shipped artifacts. No hashes/strip mode; staleness documented not detected. | Planned | [disk-backed-debug-info.md](disk-backed-debug-info.md) |

### Active cross-cutting plans

| Plan | Description |
|------|-------------|
| [sound-uniqueness/](sound-uniqueness/) | Rebuild boot compiler uniqueness analysis and mutable lowering from scratch, with printable ownership facts before codegen. **Single home for all owned-collection in-place work** — the `fixpoint-map-inplace` / aggregate-field / owned-variant cluster was consolidated here 2026-07-27 (findings folded into `storage/README.md` + `analysis/worked-examples.md`; the standalone plans archived). The `run_fixpoint`/`merge_targeted` in-place goal is a storage-track **S4** customer, not a separate plan. |
| [compiler-stack-safety.md](compiler-stack-safety.md) | Make the compiler's recursive IR tree-walks stack-safe so deeply-nested IR (wide `cond`, long side-effecting statement sequences, deep `if/else`) doesn't overflow the V8 Wasm stack. Wide `case` already fixed (flat instruction vector); runtime stack-size mitigation verified non-viable. Phased: depth-guard stopgap → iterative lowering/opt → anf/prepare/emit → serializers. **Phase 2 also removes the interim MutVec deep-module bailout guard.** |
| [mutvec-checklist.md](mutvec-checklist.md) | **Living progress tracker for MutVec**, independently phase-numbered (1..N, not tied to storage `S*`/codegen `8*`). `Int`/`Bool`/`Float`/`Byte` shipped end-to-end and unconditional; boxed (Phase 7 spike done, impl deferred), Float/Byte `set_in_place` write-routing (Phase 8), and param-sourced thaw (Phase 9, cross-track) remain. Points at mutvec-later-slices.md / mutvec-slice1-design.md for detail. |
| [mutvec-later-slices.md](mutvec-later-slices.md) | Successor to the landed MutVec slice-1 (i64, unconditional): generalize the seven `mutvec_*` ops over `PVecFamily` for `Bool`/`Float`/`Byte`/boxed element families. rt-arr-family-dedup (its `PVecFamily` prerequisite: `suffix`/`leaf_store`/`elem_box`) is **DONE** (archived 2026-08-03); Phases 1–3 unblocked. Float and Byte each need a new typed `PVec` first (`PVecF64` / `PVecByte` = `array i8`) — same shape of work, can land together; Byte unboxes the `readfile` `Vector<Byte>` result and complements (not replaces) `@std.buffer`. Captures S4 param-sourced thaw and append-only unification as deferred. |
| [vector-byte-buffer-interop.md](vector-byte-buffer-interop.md) | **Placeholder / deferred.** Once `Vector<Byte>` is unboxed (`PVecByte`, mutvec-later-slices Phase 4), Buffer's ~30× codec edge collapses toward single-digit× (loses the ref-cast/unbox half), narrowing Buffer to FFI / shared-memory / hottest-loop territory. Records the hard ceiling — no Wasm-GC array↔linear-memory bulk copy exists, so conversions stay O(n) — and the two narrow follow-ups: typed-leaf `from_bytes`/`to_bytes`, and re-benching the codec go/no-go against the unboxed baseline. Blocked on `PVecByte`. |

### Archived reference docs

Completed plans, superseded strategy docs, and self-hosting milestone records
live in [archive/README.md](archive/README.md).
