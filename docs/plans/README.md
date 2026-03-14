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

### Stage-aligned active plans

| Stage | Description | Status | Details |
|-------|-------------|--------|---------|
| 10 | Self-Hosted Compiler (`boot/`) | In Progress | [self-hosting.md](self-hosting.md) |
| Later | Tooling & Ecosystem | Planned | [tooling.md](tooling.md) |

### Active cross-cutting plans

| Plan | Description |
|------|-------------|
| [string-interning.md](string-interning.md) | Reduce duplicate string allocations with literal/runtime interning |
| [persistent-vector.md](persistent-vector.md) | Move vector runtime from flat COW arrays to persistent tree structure |
| [persistent-dict.md](persistent-dict.md) | Replace linear dict runtime with persistent HAMT |
| [pre-selfhost-cleanup.md](pre-selfhost-cleanup.md) | Refactoring and cleanup before Stage 10 self-hosting |
| [boot-foundation-libs.md](boot-foundation-libs.md) | Stage 10 support libs in `boot/lib` (`source`, `module`, `graph`, `query`) |
| [lsp-completion.md](lsp-completion.md) | LSP completion follow-up plan focused on reliability during partial/broken edits and protocol coverage |
