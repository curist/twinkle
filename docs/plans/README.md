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
| Task concurrency | Add library-first cooperative `Task<T>` concurrency without new syntax | Planned | [task-concurrency.md](task-concurrency.md) |


### Later — Tooling & Ecosystem

| Area | Description | Status | Details |
|------|-------------|--------|---------|
| Tooling | Formatter (done), linter, LSP, package manager | In Progress | [tooling.md](tooling.md) |
| Linter | `twk lint`: semantic lints (must-use, ignored Result/Option, record-copy helper, unreachable code) on the existing diagnostic channel | Planned | [linter.md](linter.md) |
| LSP enhancements | Document symbols, references, rename, signature help, semantic tokens, workspace symbols, highlights, inlay hints, folding, and incremental sync | Planned | [lsp-enhancements.md](lsp-enhancements.md) |
| LSP code actions | Quick-fix actions: missing case arms, auto-import, function type annotations | Planned | [lsp-code-actions.md](lsp-code-actions.md) |
| LSP contract hover | Hover information for builtin contract bounds and contract-backed method calls | Done | [archive/lsp-contract-hover.md](archive/lsp-contract-hover.md) |

### Active cross-cutting plans

| Plan | Description |
|------|-------------|
| [backend-anyref-elimination.md](backend-anyref-elimination.md) | Make `anyref` exceptional rather than foundational in the Wasm backend, including typed container/helper families |
| [static-uniqueness-plan.md](static-uniqueness-plan.md) | Extend the static uniqueness optimizer to cover more realistic linear-update patterns without changing the runtime model |
| [rrb-vector-concat.md](rrb-vector-concat.md) | Upgrade `Vector<T>` to an RRB-tree so `concat` and `slice` are O(log n), eliminating the O(n²) prepend-concat and dequeue/trim-slice loops. Boot `arr.tw` leads, stage0 mirrors |
| [access-contracts.md](access-contracts.md) | A general access pattern via parameterized contracts (`IndexRead<E>` / `IntoIterator<E>` / `IndexWrite<E>`) with a `Self → E` functional dependency, so `find`/`fold`/`region_eq` are written once and monomorphized to direct reads over `Vector`/`String`/`View`/`Stack` |
| [stack.md](stack.md) | LIFO stack: an O(log n) `drop_last` vector op (the boot-compiler audit's real need) + a thin `Stack<T>` wrapper; supersedes the dropped queue/deque idea |
| [view.md](view.md) | A generic `View<C>` (backing + start/len) for allocation-free read-only windows over any `IndexRead` backing — closure-free, monomorphized to direct reads, replaces hand-threaded indices in head/tail recursion |
| [slice-performance.md](slice-performance.md) | Boot-compiler slice usage audit (the evidence behind the stack/view/RRB docs) + String-slice performance: allocation-free compare primitives (Tier 1 prefix/suffix **done**), a generic `View` over the access contracts (chosen), or a `String`-as-view repr change |

### Archived reference docs

Completed plans, superseded strategy docs, and self-hosting milestone records
live in [archive/README.md](archive/README.md).
