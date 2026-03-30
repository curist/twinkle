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

### Self-Hosted Compiler

Overall plan: [self-hosting.md](self-hosting.md) (design principles, lessons
from stage0, bootstrapping sequence)

| Area | Description | Status | Details |
|------|-------------|--------|---------|
| Phase E libs | `boot/lib/` module, graph, query (deferred) | Planned | [boot-foundation-libs.md](boot-foundation-libs.md) |
| Integration | Multi-module, CLI, compatibility suite | In Progress | [boot-multi-module.md](boot-multi-module.md) |
| Boot compiler layout | Reorganize `boot/compiler/` into focused subdirectories with stable end-state names | Planned | [boot-compiler-layout-reorg.md](boot-compiler-layout-reorg.md) |
| Tracking | Status snapshots and phase progress | In Progress | [self-hosting-status.md](self-hosting-status.md) |

### Later — Tooling & Ecosystem

| Area | Description | Status | Details |
|------|-------------|--------|---------|
| Tooling | Formatter, linter, LSP, package manager | Planned | [tooling.md](tooling.md) |
| LSP completion | Reliability during partial edits, protocol coverage | In Progress | [lsp-completion.md](lsp-completion.md) |

### Active cross-cutting plans

| Plan | Description |
|------|-------------|
| [backend-anyref-elimination.md](backend-anyref-elimination.md) | Make `anyref` exceptional rather than foundational in the Wasm backend, including typed container/helper families |
| [boot-checker-inference-consistency.md](boot-checker-inference-consistency.md) | Normalize contextual call inference, closure annotation reconciliation, record validation, and ambiguity reporting in the boot checker |
| [boot-lib-vector-consumption.md](boot-lib-vector-consumption.md) | Define the ABI-first artifact boundary by which stage0 consumes a Twinkle-authored `Vector<Int>` implementation from `boot/lib` (blocked on runtime import boundary) |
| [twinkle-runtime-import-boundary.md](twinkle-runtime-import-boundary.md) | Provide extern/import mechanism for Twinkle library modules to bind runtime substrate symbols; prerequisite for boot/lib consumption |
| [persistent-vector.md](persistent-vector.md) | Move vector runtime from flat COW arrays to persistent tree structure |
| [persistent-vector-i64-poc.md](persistent-vector-i64-poc.md) | Stage0-only `Vector<Int>` persistent-vector proof of concept with fixed-width trie nodes and unchanged bootlib ABI |
| [twinkle-vector-kickoff.md](twinkle-vector-kickoff.md) | First implementation slice for a Twinkle-authored persistent vector, integrated through the stage0 Wasm backend first |
| [persistent-dict.md](persistent-dict.md) | Replace linear dict runtime with persistent HAMT |
| [range-literal-syntax.md](range-literal-syntax.md) | Support `m..n` as expression-level range literal (desugars to `range_from`) |
| [defer-implementation-drift.md](defer-implementation-drift.md) | Reconcile defer semantics across docs, interpreter, ANF defer-elim, and tests |
