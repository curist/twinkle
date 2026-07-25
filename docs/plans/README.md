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
| LSP contract hover | Hover information for builtin contract bounds and contract-backed method calls | Done | [archive/lsp-contract-hover.md](archive/lsp-contract-hover.md) |

### Active cross-cutting plans

| Plan | Description |
|------|-------------|
| [sound-uniqueness/](sound-uniqueness/) | Rebuild boot compiler uniqueness analysis and mutable lowering from scratch, with printable ownership facts before codegen |
| [fixpoint-map-inplace.md](fixpoint-map-inplace.md) | Make the ownership fixpoint's own loop-carried maps emit `dict$set_in_place` instead of persistent `dict$set` (self-hosting → faster compiler). In-place codegen works but the fixpoint's maps read as `aliased shell` due to interprocedural helper summaries (worked-examples Case W). Investigation-first: pin the poisoning summary, then targeted precision vs source restructure. |
| [2026-07-24-ownership-borrow-effect-checker-plan.md](2026-07-24-ownership-borrow-effect-checker-plan.md) | Internal borrow/effect framework: prove dict copy-carrier reads are compatible loans so a source-aliased carrier stays in-place. Framework/soundness umbrella. Tasks 1–2 landed (fixtures + red baseline, model types + diagnostics). |
| [2026-07-24-copy-carrier-engine-impl-plan.md](2026-07-24-copy-carrier-engine-impl-plan.md) | IR-grounded execution plan for the borrow/effect engine (parent plan's Tasks 3–6): compositional dedupe-helper certification (contains-guard + sort-insert), per-function loan/write proof, transfer suppression + source-param seeding. In progress — Tasks 1–2 landed; Task 3 primitive recognizer under redesign (see design doc below). |
| [2026-07-25-key-stream-uniqueness-impl-plan.md](2026-07-25-key-stream-uniqueness-impl-plan.md) | TDD implementation plan for the key-stream-uniqueness proof checker (from the design doc): battery-first (all seven review rounds' adversarial negatives as failing tests), then O0–O4 obligation checkers + CFG-view threading, wiring on all summary paths, boot-main/threading gates, and gate-4 independent review before unblocking engine-plan Task 4. Not yet started. |
| [2026-07-25-key-stream-uniqueness-design.md](2026-07-25-key-stream-uniqueness-design.md) | Design (DECIDED — Option A′, general proof checker): how to soundly certify that a loop key stream (built by a dedupe helper like `insert_sorted`/`int_keys_union`) is duplicate-free, so the copy-carrier engine can license in-place dict writes. Documents two confirmed over-certification bugs in the shipped recognizer, the O0–O4 proof obligations, and the acceptance gates (default-deny + certificate + adversarial negative battery + independent review) that must pass before Task 4 consumes `dedupe_helpers`. B (exact-shape) is the sanctioned fallback. |
| [compiler-stack-safety.md](compiler-stack-safety.md) | Make the compiler's recursive IR tree-walks stack-safe so deeply-nested IR (wide `cond`, long side-effecting statement sequences, deep `if/else`) doesn't overflow the V8 Wasm stack. Wide `case` already fixed (flat instruction vector); runtime stack-size mitigation verified non-viable. Phased: depth-guard stopgap → iterative lowering/opt → anf/prepare/emit → serializers |

### Archived reference docs

Completed plans, superseded strategy docs, and self-hosting milestone records
live in [archive/README.md](archive/README.md).
