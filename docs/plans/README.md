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
| [sound-uniqueness-recursive-summary-ownership.md](sound-uniqueness-recursive-summary-ownership.md) | Close the recursive-summary ownership gap (`graph_scc.visit`): a state record threaded through a self/mutual-recursive call is pinned `Published` by the least-fixed-point summary driver. Builds Phase 6 Stage 5 (a `VariantId`-keyed SCC fixpoint) so the recursive call resolves to an owned variant |
| [compiler-stack-safety.md](compiler-stack-safety.md) | Make the compiler's recursive IR tree-walks stack-safe so deeply-nested IR (wide `cond`, long side-effecting statement sequences, deep `if/else`) doesn't overflow the V8 Wasm stack. Wide `case` already fixed (flat instruction vector); runtime stack-size mitigation verified non-viable. Phased: depth-guard stopgap → iterative lowering/opt → anf/prepare/emit → serializers |

### Archived reference docs

Completed plans, superseded strategy docs, and self-hosting milestone records
live in [archive/README.md](archive/README.md).
