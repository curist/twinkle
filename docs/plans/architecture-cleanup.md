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

### Files to change

- `boot/compiler/pipeline.tw` — rewrite `compile_source`
- `boot/tests/helpers/codegen_harness.tw` — verify tests still pass

---

## 2. Structured Diagnostics (warnings: done, errors: pending)

### Warnings — done

`PipelineArtifacts` now carries a `warnings: Vector<AnalysisDiag>` field.
`compile_entry` collects analysis warnings and returns them in the result
instead of printing via `eprintln`. CLI commands (`build`, `run`, `ir`,
`check`) call `print_warnings` from `commands/common.tw` to display them.
The compiler library no longer has warning side effects.

Also removed the unused `check_entry_path` wrapper — `check` now calls
`compile_entry_path` directly.

### Errors — pending

Public APIs still return `Result<PipelineArtifacts, String>` for errors.
`format_analysis_errors` and `format_stage_error` still collapse structured
diagnostics into strings inside `module_compiler.tw`.

Remaining work:

1. Define a `CompileFailure` type carrying stage name and
   `Vector<AnalysisDiag>`.
2. Change error returns from `String` to `CompileFailure`.
3. Move error string formatting into CLI command handlers.
4. Update ~30 test callsites in `multi_module_suite.tw` that assert on
   error strings.

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

### Files to change

- `boot/compiler/builtins.tw` — refactor into shared table
- `boot/compiler/base_env.tw` — derive env from shared table
- `boot/tests/suites/signature_drift_suite.tw` — update/simplify

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
| 3 | Structured diagnostics (#2) | Warnings done, errors pending | Unblocks tooling integration |
| 4 | Unify frontend pipelines (#1) | Pending | Eliminates divergence risk |
| 5 | Consolidate builtins (#4) | Pending | Reduces maintenance burden |
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
