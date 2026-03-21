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
| ANF + codegen | ANF lowering, optimizer, codegen, linker | Planned | — |
| Phase E libs | `boot/lib/` module, graph, query (deferred) | Planned | [boot-foundation-libs.md](boot-foundation-libs.md) |
| Integration | Multi-module, CLI, compatibility suite | Planned | — |
| Tracking | Status snapshots and phase progress | In Progress | [self-hosting-status.md](self-hosting-status.md) |

### Later — Tooling & Ecosystem

| Area | Description | Status | Details |
|------|-------------|--------|---------|
| Tooling | Formatter, linter, LSP, package manager | Planned | [tooling.md](tooling.md) |
| LSP completion | Reliability during partial edits, protocol coverage | In Progress | [lsp-completion.md](lsp-completion.md) |

### Active cross-cutting plans

| Plan | Description |
|------|-------------|
| [persistent-vector.md](persistent-vector.md) | Move vector runtime from flat COW arrays to persistent tree structure |
| [persistent-dict.md](persistent-dict.md) | Replace linear dict runtime with persistent HAMT |
| [range-literal-syntax.md](range-literal-syntax.md) | Support `m..n` as expression-level range literal (desugars to `range_from`) |
| [defer-implementation-drift.md](defer-implementation-drift.md) | Reconcile defer semantics across docs, interpreter, ANF defer-elim, and tests |
