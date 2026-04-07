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
| [pragmatic-persistent-vector.md](pragmatic-persistent-vector.md) | **Next up** — Replace flat COW vector with persistent bit-partitioned trie using existing `anyref` storage and `rt.arr` ABI |
| [persistent-dict.md](persistent-dict.md) | **Next up** — Replace linear assoc-list dict with persistent HAMT using existing `anyref` storage first, then typed specialization later |
| [backend-anyref-elimination.md](backend-anyref-elimination.md) | Make `anyref` exceptional rather than foundational in the Wasm backend, including typed container/helper families |
| [boot-checker-inference-consistency.md](boot-checker-inference-consistency.md) | Normalize contextual call inference, closure annotation reconciliation, record validation, and ambiguity reporting in the boot checker |
| [deferred-persistence.md](deferred-persistence.md) | Consolidated strategy for uniqueness-based in-place mutation under immutable value semantics |
| [range-literal-syntax.md](range-literal-syntax.md) | Support `m..n` as expression-level range literal (desugars to `range_from`) |
| [defer-implementation-drift.md](defer-implementation-drift.md) | Reconcile defer semantics across docs, interpreter, ANF defer-elim, and tests |

### Deferred — Persistent Data Structure Enhancements

These plans defined a more ambitious architecture (typed per-element families,
`boot/lib` ownership, runtime import boundaries) that was blocking progress.
The pragmatic plans above supersede their **implementation strategy** while
preserving them as future enhancement targets once the base trie/HAMT is
working.

| Plan | Description |
|------|-------------|
| [persistent-vector.md](persistent-vector.md) | Full typed-family persistent vector with per-element specialization |
| [persistent-vector-i64-poc.md](persistent-vector-i64-poc.md) | Stage0-only `Vector<Int>` POC with typed `i64` trie nodes |
| [twinkle-vector-kickoff.md](twinkle-vector-kickoff.md) | Twinkle-authored persistent vector with `boot/lib` ownership |
| [boot-lib-vector-consumption.md](boot-lib-vector-consumption.md) | ABI boundary for stage0 to consume `boot/lib` vector artifact |
| [twinkle-runtime-import-boundary.md](twinkle-runtime-import-boundary.md) | Extern/import mechanism for `boot/lib` to bind runtime substrate symbols |
