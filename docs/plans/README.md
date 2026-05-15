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

### Boot Compiler

Self-hosting is complete. Historical design and status docs live in
[archive/README.md](archive/README.md).

| Area | Description | Status | Details |
|------|-------------|--------|---------|
| Boot compiler layout | Reorganize `boot/compiler/` into focused subdirectories with stable end-state names | Planned | [boot-compiler-layout-reorg.md](boot-compiler-layout-reorg.md) |
| Boot performance | Track current compiler bottlenecks and optimization wins | In Progress | [boot-compiler-perf.md](boot-compiler-perf.md) |
| Nested variant lowering | Investigate the remaining nested variant-pattern lowering hazard | In Progress | [boot-nested-variant-pattern-lowering.md](boot-nested-variant-pattern-lowering.md) |
| Unreachable case arms | Diagnose case arms shadowed by earlier catch-all, literal, or covering variant patterns | Planned | [unreachable-case-arms.md](unreachable-case-arms.md) |
| Task concurrency | Add library-first cooperative `Task<T>` concurrency without new syntax | Planned | [task-concurrency.md](task-concurrency.md) |
| JSPI runtime | Move boot JavaScript hosts to a JSPI entry ABI, starting with non-blocking LSP debounce stdin | Planned | [boot-jspi-runtime.md](boot-jspi-runtime.md) |
| JS runtime unification | Share one Node Wasm runtime implementation between library tools and SEA CLI before JSPI migration | Planned | [unify-js-runtime.md](unify-js-runtime.md) |


### Later — Tooling & Ecosystem

| Area | Description | Status | Details |
|------|-------------|--------|---------|
| Tooling | Formatter, linter, LSP, package manager | Planned | [tooling.md](tooling.md) |
| LSP completion | Reliability during partial edits, protocol coverage | In Progress | [lsp-completion.md](lsp-completion.md) |

### Active cross-cutting plans

| Plan | Description |
|------|-------------|
| [persistent-dict.md](persistent-dict.md) | Replace linear assoc-list dict with persistent HAMT using existing `anyref` storage first, then typed specialization later |
| [persistent-vector.md](persistent-vector.md) | Replace the current flat copy-on-write vector backing with a persistent vector runtime and specialized container families |
| [backend-anyref-elimination.md](backend-anyref-elimination.md) | Make `anyref` exceptional rather than foundational in the Wasm backend, including typed container/helper families |
| [wasm-tail-calls.md](wasm-tail-calls.md) | Add Wasm tail-call emission for eligible tail-position calls as a required target feature |
| [boot-uniqueness-mono-sync.md](boot-uniqueness-mono-sync.md) | Keep uniqueness-generated locals and rewritten ANF structure synchronized through the boot backend pipeline |
| [boot-uniqueness-deep-ownership.md](boot-uniqueness-deep-ownership.md) | Separate fresh wrapper values from deep ownership so boot uniqueness only performs in-place collection rewrites when reachable mutable state is truly unaliased |
| [static-uniqueness-plan.md](static-uniqueness-plan.md) | Extend the static uniqueness optimizer to cover more realistic linear-update patterns without changing the runtime model |

### Archived reference docs

Completed plans, superseded strategy docs, and self-hosting milestone records
live in [archive/README.md](archive/README.md).
