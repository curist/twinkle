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
| Boot compiler layout | Reorganize `boot/compiler/` into focused subdirectories with stable end-state names | Planned | [boot-compiler-layout-reorg.md](boot-compiler-layout-reorg.md) |
| Tracking | Status snapshots and phase progress | Done | [self-hosting-status.md](self-hosting-status.md) |

### Later — Tooling & Ecosystem

| Area | Description | Status | Details |
|------|-------------|--------|---------|
| Tooling | Formatter, linter, LSP, package manager | Planned | [tooling.md](tooling.md) |
| LSP completion | Reliability during partial edits, protocol coverage | In Progress | [lsp-completion.md](lsp-completion.md) |

### Active cross-cutting plans

| Plan | Description |
|------|-------------|
| [persistent-dict.md](persistent-dict.md) | Replace linear assoc-list dict with persistent HAMT using existing `anyref` storage first, then typed specialization later |
| [backend-anyref-elimination.md](backend-anyref-elimination.md) | Make `anyref` exceptional rather than foundational in the Wasm backend, including typed container/helper families |
| [boot-uniqueness-mono-sync.md](boot-uniqueness-mono-sync.md) | Keep uniqueness-generated locals and rewritten ANF structure synchronized through the boot backend pipeline |
| [boot-uniqueness-deep-ownership.md](boot-uniqueness-deep-ownership.md) | Separate fresh wrapper values from deep ownership so boot uniqueness only performs in-place collection rewrites when reachable mutable state is truly unaliased |
| [boot-nested-variant-pattern-lowering.md](boot-nested-variant-pattern-lowering.md) | Investigate and fix the stage2 trap triggered by semantically equivalent nested variant-pattern matches in boot codegen helpers |
| [boot-checker-inference-consistency.md](boot-checker-inference-consistency.md) | Normalize contextual call inference, closure annotation reconciliation, record validation, and ambiguity reporting in the boot checker |
| [boot-first-class-builtin-functions.md](boot-first-class-builtin-functions.md) | Make builtin / prelude functions behave like proper first-class closure values in the boot backend planning, verification, and emission pipeline |
| [boot-wasm-binary-serializer.md](boot-wasm-binary-serializer.md) | Add a Twinkle-implemented serializer from boot Wasm IR to final `.wasm` bytes |
| [node-standalone-runtime.md](node-standalone-runtime.md) | Build a standalone Node.js Twinkle compiler/runtime entry without requiring Rust `twk` |
| [deferred-persistence.md](deferred-persistence.md) | Consolidated strategy for uniqueness-based in-place mutation under immutable value semantics |
| [range-literal-syntax.md](range-literal-syntax.md) | Support `m..n` as expression-level range literal (desugars to `range_from`) |
| [defer-implementation-drift.md](defer-implementation-drift.md) | Reconcile defer semantics across docs, interpreter, ANF defer-elim, and tests |

### Historical / deferred strategy docs

Broader vector/library-boundary explorations and early runtime-comparison work
have been moved to [archive/README.md](archive/README.md). They may still be
useful background, but they are not part of the current active execution set.
