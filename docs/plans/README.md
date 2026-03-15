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
| Frontend | Lexer, parser, resolver, type checker | Planned | — |
| Backend | Core IR, ANF, optimizer, codegen, linker | Planned | — |
| Phase E libs | `boot/lib/` module, graph, query (deferred) | Planned | [boot-foundation-libs.md](boot-foundation-libs.md) |
| Integration | Multi-module, CLI, compatibility suite | Planned | — |

### Later — Tooling & Ecosystem

| Area | Description | Status | Details |
|------|-------------|--------|---------|
| Tooling | Formatter, linter, LSP, package manager | Planned | [tooling.md](tooling.md) |
| LSP completion | Reliability during partial edits, protocol coverage | In Progress | [lsp-completion.md](lsp-completion.md) |

### Active cross-cutting plans

| Plan | Description |
|------|-------------|
| [string-interning.md](string-interning.md) | String literal pooling landed; runtime interning remains optional follow-up |
| [persistent-vector.md](persistent-vector.md) | Move vector runtime from flat COW arrays to persistent tree structure |
| [persistent-dict.md](persistent-dict.md) | Replace linear dict runtime with persistent HAMT |
| [destructuring-imports.md](destructuring-imports.md) | Add `use module.{...}` for value/type/mixed imports with per-item aliasing (no `self`/wildcard) |
