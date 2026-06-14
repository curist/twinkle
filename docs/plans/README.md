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
| Boot typed builtin type refs | Extend the typed-builtin-reference pattern (Option/Result variant refs, now done) to the remaining builtin *types* still referenced by raw id — `Order`, `Iterator`, `Range` — and audit `IterItem`/`UnfoldStep`/`Task` | Planned | [boot-typed-builtin-type-refs.md](boot-typed-builtin-type-refs.md) |


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
| [recursive-module-groups.md](recursive-module-groups.md) | Allow mutually-recursive modules by condensing the module graph into SCCs and resolving each group with the two-phase signatures-then-bodies pass; rejects only top-level value-init cycles. Resolves open-question #3 and unblocks prelude-into-prelude injection |
| [backend-anyref-elimination.md](backend-anyref-elimination.md) | Make `anyref` exceptional rather than foundational in the Wasm backend, including typed container/helper families |
| [static-uniqueness-plan.md](static-uniqueness-plan.md) | Extend the static uniqueness optimizer to cover more realistic linear-update patterns without changing the runtime model |
| [sliceable.md](sliceable.md) | `foo[a..b]` range-slice syntax via a Self-only `Sliceable` contract — the lone open piece of the (now-archived) collection-access cluster; adds a `Range`-index arm to `synth_index` plus `Vector`/`String`/`View` satisfiers. Background in [archive/collections-access.md](archive/collections-access.md) |
| [vector-perf/](vector-perf/README.md) | **Ongoing endeavor.** Make idiomatic `Vector`/`sort_by`/`order_by` code fast (vs ~7× behind Clojure). Measurement points at vector read cost + typed representation as the master lever; comparator and allocation micro-opts proven small. Gathers all sort/native-sort/typed-vector plans and the rejected approaches |

### Archived reference docs

Completed plans, superseded strategy docs, and self-hosting milestone records
live in [archive/README.md](archive/README.md).
