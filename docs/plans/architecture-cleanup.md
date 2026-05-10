# Architecture Cleanup Plan

## Goal

Consolidate ownership boundaries and tighten invariants across the compiler
without altering fundamental architecture. All items are grounded in concrete
issues visible in today's codebase.

---

## 1. Unify Frontend Pipelines

### Current state (verified)

Two frontend paths exist:

| Path | Location | Used by |
|------|----------|---------|
| `pipeline.compile_source` | `boot/compiler/pipeline.tw:14-50` | Test harness only (`boot/tests/helpers/codegen_harness.tw`) |
| `module_compiler.compile_entry` | `boot/compiler/module_compiler.tw:38-191` | All CLI commands (`build`, `run`, `ir`, `check`) |

`compile_source` inlines parse/resolve/check/lower directly.
`compile_entry` delegates to `analyze.analyze_module` with stage runners and
query caching.

They share post-frontend stages (monomorphize, ANF lower, optimize) but the
frontend (parse/resolve/typecheck) is **not shared**.

### Risk

Fixes to analysis, diagnostics, imports, overlays, or caching only affect the
multi-module path. `compile_source` silently diverges. In practice the blast
radius is limited to tests, but it weakens confidence that tests exercise the
real pipeline.

### Plan

1. Rewrite `compile_source` to create an overlay-backed virtual source and call
   `analyze_module`, reusing stage runners and caches.
2. Keep the simplified API surface (`fn compile_source(src: String)`) so test
   callsites don't change.
3. Delete the inline parse/resolve/check code from `pipeline.tw`.

**Status: done.** `compile_source` now uses an overlay-backed virtual path
(`/__virtual__/input.tw`) and delegates to `analyze_module` → `stage_runner.lower`
→ `core_linker.link` → monomorphize → ANF → optimize — the same pipeline as
`compile_entry`. Test callsites unchanged.

### Files changed

- `boot/compiler/pipeline.tw` — rewrote `compile_source`

---

## 2. Structured Diagnostics

`compile_entry` now returns `Result<PipelineArtifacts, CompileError>` where:

- `PipelineArtifacts.warnings: Vector<AnalysisDiag>` carries structured
  warnings (no more `eprintln` side effects in the compiler library).
- `CompileError` is an enum: `Diagnostics(Vector<AnalysisDiag>)` for
  analysis/lowering failures, `Internal(String)` for invariant violations.

CLI commands format errors via `format_compile_error` and warnings via
`print_warnings`, both in `commands/common.tw`. The compiler library has
no presentation side effects.

`compile_source` (test-only path) still returns `Result<..., String>` —
this will be resolved when the frontend pipelines are unified (item #1).

**Status: done** (except `compile_source`, deferred to item #1).

---

## 4. Consolidate Builtin Definitions

### Current state (verified)

Two overlapping systems define builtin knowledge:

| System | Location | Purpose |
|--------|----------|---------|
| `make_builtin_registry()` | `boot/compiler/builtins.tw:253-391` | FuncId assignment, dispatch kind (runtime/intrinsic), ABI types |
| `builtin_env()` | `boot/compiler/base_env.tw:197-281` | Type signatures, method bindings, function origins for resolver/checker |

Both define the same ~65 builtins independently. `signature_drift_suite.tw`
exists specifically to detect divergence between them.

### Risk

Adding or changing a builtin requires updating both systems. ABI types in the
registry and MonoType signatures in the env are built independently — drift is
possible despite the test suite.

### Plan

1. Define a single canonical builtin table that carries: name, canonical name,
   dispatch kind, ABI types, and MonoType signature.
2. Generate `BuiltinRegistry` and the builtin portion of `ResolvedEnv` from
   this table.
3. Keep `signature_drift_suite.tw` as a safety net during migration, then
   simplify once the single source is established.

**Status: done (phase 1).** `builtin_specs()` is now the single authoritative
table defining all builtins: internal name, canonical name, and dispatch kind
(runtime with wasm module/name, or intrinsic). `make_builtin_registry()` builds
from this table. ABI types remain keyed by name via `builtin_abi()` (a future
phase could derive ABI from MonoType signatures). The `signature_drift_suite`
continues to validate that every canonical entry traces to a signature file.

### Files changed

- `boot/compiler/builtins.tw` — introduced `BuiltinSpec`/`BuiltinDispatch`,
  `builtin_specs()` table, rewrote `make_builtin_registry()` to iterate specs

---

## 5. Harden Linker Post-Validation Invariants

### Current state (verified)

The duplicate FuncId allocation bug (Step 1 vs Step 3) has been **fixed**
(commits `56f5afe`, `11fe9fa`).

Three dict-key-then-lookup `.None => {}` patterns in `core_linker.tw`
(lines 190, 256, 266) have been hardened to `error(...)` traps.
Four other `.None => {}` sites (101, 224, 340, 513) are legitimate optional
handling and were left as-is.

**Status: done.**

---

## 6. DCE-Aware Extern Imports

### Current state (verified)

The boot linker (`core_linker.tw`) already filters extern imports by
reachability at lines 260-268.

**Status: done.**

---

## 7. Reduce Parallel Vector Fragility in Type Storage

### Current state (verified)

`AnalysisState` (`boot/compiler/query/analyze.tw:50-52`) uses three parallel
structures:

```twinkle
shared_types: Vector<TypeEntry>,
shared_type_names: Vector<String>,
shared_type_origins: Dict<Int, String>,
```

`shared_types` and `shared_type_names` are appended in lockstep via
`capture_local_types` (lines 442-446) and read in lockstep via
`extend_new_shared_types_from` (lines 425-428). Index synchronization is
implicit.

The existing plan `docs/plans/archive/boot-module-type-identity.md` documents
this fragility and its root cause (type identity is recreated per importer).

### Risk

Any code path that appends to one vector without the other silently breaks
indexing. Refactors that reorder or filter entries invalidate length-based
boundary tracking (`local_type_start`, `shared_type_start`).

### Plan

1. Introduce a `SharedTypeEntry` struct bundling `TypeEntry` and name together.
2. Replace `shared_types` + `shared_type_names` with a single
   `Vector<SharedTypeEntry>`.
3. Keep `shared_type_origins` as a separate dict (it's keyed by TypeId, not
   index).
4. Update `capture_local_types` and `extend_new_shared_types_from` accordingly.

### Files to change

- `boot/compiler/query/analyze.tw` — introduce `SharedTypeEntry`, consolidate
  vectors
- `boot/compiler/resolver.tw` — update `extend_types_from` if needed

---

## Priority Order

| Priority | Item | Status | Rationale |
|----------|------|--------|-----------|
| 1 | Harden linker invariants (#5) | Done | Small change, prevents silent corruption |
| 2 | DCE-aware extern imports (#6) | Done | Already implemented in boot linker |
| 3 | Structured diagnostics (#2) | Done | Unblocks tooling integration |
| 4 | Unify frontend pipelines (#1) | Done | Eliminates divergence risk |
| 5 | Consolidate builtins (#4) | Done | Reduces maintenance burden |
| 6 | Type storage cleanup (#7) | Pending | Existing plan covers root cause; this is incremental |

---

## Non-goals

This plan does not cover:

- Parallel compilation, Salsa-style queries, daemon mode
- Incremental scheduling beyond existing query cache
- Effect systems, protocol systems, macro systems
- Boot compiler layout reorganization (covered by `boot-compiler-layout-reorg.md`)
- Type identity canonicalization (covered by `static-uniqueness-plan.md` and
  `archive/boot-module-type-identity.md`)
