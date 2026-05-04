# Salsa-like Incremental Query Refactor

## Motivation

The boot compiler's LSP currently invalidates at **module granularity**: any change to a
file causes its full parse/resolve/typecheck to re-run, plus all reverse dependents'
resolve/typecheck. Most keystrokes don't change a module's public interface, yet importers
pay full recomputation cost.

A demand-driven query model with **early cutoff** eliminates this waste: if a module's
exports are unchanged after recompilation, downstream dependents are verified unchanged
without re-running.

## Goals

1. **LSP responsiveness**: skip redundant work on every keystroke
2. **Unified path**: one codepath for both `twinkle build` and LSP (no perf penalty — Dict is HAMT O(log₃₂ n))
3. **Fine-grained invalidation**: per-query, not per-module
4. **Preserve correctness**: monotonic TypeId allocation still works
5. **Incremental migration**: each stage is independently shippable

## Non-Goals (for now)

- Incremental monomorphization (whole-program, keep as-is)
- Incremental codegen (backend is fast enough)
- Parallelism (Wasm is single-threaded)

---

## Architecture Overview

### Current Flow

```
source_text ──→ parse ──→ resolve ──→ typecheck ──→ lower ──→ [mono → anf → opt → emit]
                                                                 └── whole-program ──┘
```

Cache: per-module, keyed by content hash + deps hash. Invalidation: eager cascade via
`reverse_dependents_closure`.

### Proposed Flow

```
┌─────────────────────────────────────────────────────────────────────────┐
│  Query Database (Db)                                                     │
│                                                                          │
│  Inputs:   source_text(path) → String                                   │
│                                                                          │
│  Derived:  parsed(path)           → ParsedModule                        │
│            module_deps(path)      → Vector<DepEntry>                    │
│            module_exports(path)   → ModuleExports    ← early cutoff     │
│            resolved(path)         → ResolveResult                       │
│            typed(path)            → CheckResult                         │
│            lowered(path)          → LowerResult                         │
│                                                                          │
│  Each derived query: memoized, tracks deps, supports early cutoff       │
└─────────────────────────────────────────────────────────────────────────┘
```

The key innovation: **`module_exports` is a separate query** that downstream modules
depend on — not the full `typed` result. When a module's internals change but its public
interface stays the same, the exports query returns an identical value → early cutoff →
importers are not recomputed.

---

## Core Concepts

### Query

A named computation `f(db, key) → value` that:
- Is deterministic (same inputs → same output)
- May read other queries (tracked as dependencies)
- Has its result memoized in the database
- Participates in the verification/early-cutoff protocol

### Revision

A monotonically increasing counter. Bumps whenever any input changes.

### MemoEntry

```twinkle
type MemoEntry<T> = .{
  value: T,
  deps: Vector<QueryKey>,   // queries we read during computation
  verified_at: Int,         // revision when we last confirmed value is current
  changed_at: Int,          // revision when value last actually changed
}
```

### Verification (Red-Green Algorithm)

When a query is requested:
1. If `verified_at == current_revision` → return cached value (green)
2. Recursively verify each dependency
3. If all deps have `changed_at <= our verified_at` → mark green, return cached
4. Otherwise: recompute. If new value == old value → update `verified_at` only (early cutoff). If different → update both `verified_at` and `changed_at`.

### Early Cutoff

The crucial optimization. Example:
- User adds a comment in `utils.tw`
- `parsed("utils")` → new AST (spans differ) → `changed_at` bumps
- `resolved("utils")` → must recompute → same result → `changed_at` stays
- `typed("utils")` → must recompute (depends on resolved) → same types → `changed_at` stays
- `module_exports("utils")` → depends on typed → same exports → `changed_at` stays
- `resolved("main")` depends on `module_exports("utils")` → still green → **skip**

---

## Detailed Design

### 1. Query Database

```twinkle
// compiler/query/db.tw

pub type QueryKey = .{
  kind: QueryKind,
  path: String,       // module path (primary key for most queries)
}

pub type QueryKind = {
  SourceText,
  Parsed,
  ModuleDeps,
  ModuleExports,
  Resolved,
  Typed,
  Lowered,
}

pub type Db = .{
  revision: Int,
  inputs: Dict<String, InputEntry>,      // path → source text + revision info
  memo: Dict<Int, MemoSlot>,             // query_key hash → memo
  type_registry: TypeRegistry,           // shared monotonic type state
}

type InputEntry = .{
  text: String,
  hash: Int,
  changed_at: Int,
}

type MemoSlot = .{
  value_hash: Int,          // fingerprint for early cutoff comparison
  verified_at: Int,
  changed_at: Int,
  deps: Vector<Int>,        // query key hashes of dependencies
  // value stored separately per query kind (typed storage)
}
```

### 2. Query Execution Context

```twinkle
// compiler/query/runtime.tw

/// Passed to query functions; records dependencies automatically.
pub type QueryCtx = .{
  db: Db,
  current_key: Int,
  deps_acc: Vector<Int>,    // accumulates deps during execution
}

/// Request a dependency. Records the edge and returns the value.
pub fn request<T>(ctx: QueryCtx, key: QueryKey, compute: fn(QueryCtx) T) .{ ctx: QueryCtx, value: T } {
  // 1. Record dependency edge
  // 2. Check memo: verify or recompute
  // 3. Return value
  ...
}
```

### 3. Query Definitions

Each existing compiler stage becomes a thin wrapper:

```twinkle
// compiler/query/queries.tw

pub fn parsed(ctx: QueryCtx, path: String) .{ ctx: QueryCtx, value: ParsedModule } {
  // depends on: source_text(path) [input query]
  source := input_source_text(ctx.db, path)
  result := parser.parse(source, file_id_for(path))
  .{ ctx, value: .{ module: result.value, diagnostics: result.diagnostics } }
}

pub fn module_deps(ctx: QueryCtx, path: String) .{ ctx: QueryCtx, value: Vector<DepEntry> } {
  // depends on: parsed(path)
  out := request(ctx, .{ kind: .Parsed, path }, fn(c) parsed(c, path))
  ctx = out.ctx
  plan := imports.plan_dependencies(out.value.module, path, project_root(ctx.db))
  .{ ctx, value: plan.dependencies }
}

pub fn module_exports(ctx: QueryCtx, path: String) .{ ctx: QueryCtx, value: ModuleExports } {
  // depends on: typed(path), parsed(path)
  // This is the EARLY CUTOFF boundary — most edits don't change exports
  typed_out := request(ctx, .{ kind: .Typed, path }, fn(c) typed(c, path))
  parsed_out := request(typed_out.ctx, .{ kind: .Parsed, path }, fn(c) parsed(c, path))
  ctx = parsed_out.ctx
  exports := extract_exports_for_module(typed_out.value.env, parsed_out.value.module, path)
  .{ ctx, value: exports }
}

pub fn resolved(ctx: QueryCtx, path: String) .{ ctx: QueryCtx, value: ResolveResult } {
  // depends on: parsed(path), module_exports(each dep)
  parsed_out := request(ctx, .{ kind: .Parsed, path }, fn(c) parsed(c, path))
  deps_out := request(parsed_out.ctx, .{ kind: .ModuleDeps, path }, fn(c) module_deps(c, path))
  ctx = deps_out.ctx

  // Build env from dependency exports (this is where early cutoff pays off)
  env := base_env()
  for dep in deps_out.value {
    exports_out := request(ctx, .{ kind: .ModuleExports, path: dep.canonical_path }, fn(c) module_exports(c, dep.canonical_path))
    ctx = exports_out.ctx
    env = merge_exports(env, dep, exports_out.value)
  }

  result := env.resolve(parsed_out.value.module)
  .{ ctx, value: result }
}
```

### 4. TypeId Registry (Replaces shared_types Threading)

The monotonically-growing `shared_types` vector becomes part of the database state,
accessed through queries rather than threaded through `CompileState`:

```twinkle
// compiler/query/type_registry.tw

pub type TypeRegistry = .{
  types: Vector<TypeEntry>,
  type_names: Vector<String>,
  type_origins: Dict<Int, String>,
}

/// Register types declared by a module. Idempotent — same module re-registering
/// its types after recomputation is a no-op if types haven't changed.
pub fn register_module_types(reg: TypeRegistry, path: String, entries: Vector<TypeEntry>) TypeRegistry {
  // Only append types not already present (by origin path + name)
  ...
}
```

Key insight: TypeId stability. If module A declares `type Foo` and gets TypeId=7, that
must remain stable across recompilations. Options:
- **Option A**: Content-addressed TypeIds (hash of path + name + shape). Fully stable but
  requires changing how TypeIds are compared/used.
- **Option B**: Keep sequential allocation but make it deterministic by processing modules
  in canonical sorted order. Simpler but fragile to new modules being added.
- **Option C (recommended)**: Keep sequential allocation, but the registry remembers
  `(path, type_name) → TypeId` mappings. Re-registering the same type returns the same
  TypeId. New types get the next available ID.

### 5. Fingerprinting for Early Cutoff

Each query needs a way to compare "did my output actually change?" without deep equality
on potentially large structures.

```twinkle
// compiler/query/fingerprint.tw

/// Fingerprint of ModuleExports — the critical cutoff boundary.
pub fn fingerprint_exports(exports: ModuleExports) Int {
  // Hash: exported function names + signatures + exported type names + shapes
  // Skip: internal metadata, spans, diagnostics
  ...
}

/// Fingerprint of ResolveResult — for resolver output comparison.
pub fn fingerprint_resolved(result: ResolveResult) Int {
  // Hash: env.funcs keys + signatures, env.types entries
  ...
}
```

Fingerprints must be **semantically stable**: same logical result → same fingerprint
regardless of incidental differences (e.g., span offsets, allocation order).

---

## Migration Plan

### Phase 1: Introduce Db + Revision Counter (foundation)

**Files to create:**
- `boot/compiler/query/db.tw` — Db type, revision management, input mutation
- `boot/compiler/query/fingerprint.tw` — fingerprint functions

**Files to modify:**
- `boot/compiler/query/cache.tw` — add `revision: Int` and `verified_at`/`changed_at` per entry
- `boot/lib/lsp/server_core.tw` — bump revision on didChange

**Scope**: Add revision tracking to existing cache without changing behavior. Existing
invalidation still works. This is purely additive.

**Validation**: All existing tests pass. LSP behavior unchanged.

### Phase 2: Lazy Verification (replace eager invalidation)

**Key change**: `invalidate_changed_module` no longer eagerly clears downstream entries.
Instead, it marks them as "needs verification" (`verified_at < revision`).

**Files to modify:**
- `boot/compiler/query/cache.tw` — replace `clear_resolved_and_later` with version check
- `boot/compiler/query/stage_runner.tw` — before running a stage, verify deps first

**Behavior change**: On cache hit, we now check whether deps are still valid (recursive
verification) instead of relying on eager pre-invalidation. If deps' `changed_at` <=
our `verified_at`, skip recomputation.

**Validation**: Same results, fewer stage executions. Add cache stats logging to confirm
fewer misses.

### Phase 3: Extract module_exports as Cutoff Boundary

**Key change**: `module_exports(path)` becomes a separately memoized query with its own
fingerprint. Downstream queries (`resolved` of importers) depend on `module_exports`
rather than the full `typed` result.

**Files to create:**
- `boot/compiler/query/exports.tw` — `module_exports` query + fingerprint

**Files to modify:**
- `boot/compiler/query/stage_runner.tw` — add exports stage
- `boot/compiler/query/cache.tw` — add exports cache slot
- `boot/compiler/module_compiler.tw` — use exports query
- `boot/compiler/query/diagnostics.tw` — use exports query

**This is where the real LSP win happens.** After this phase, editing a function body
in module A no longer triggers recompilation of module B (if A's exports are unchanged).

**Validation**: Edit a function body → confirm importers' resolve/typecheck are skipped
(cache stats show hits where before they showed misses).

### Phase 4: TypeId Registry Stabilization

**Key change**: TypeIds allocated via registry lookup (`path + name → stable Id`) instead
of sequential append during compilation order.

**Files to create:**
- `boot/compiler/query/type_registry.tw`

**Files to modify:**
- `boot/compiler/module_compiler.tw` — use registry instead of `capture_local_types`
- `boot/compiler/query/diagnostics.tw` — same

**Why this matters**: Without stable TypeIds, recompiling module A in a different order
could assign different TypeIds, making cached results of module B invalid. The registry
ensures deterministic allocation regardless of compilation order.

### Phase 5: Unify module_compiler and diagnostics paths

**Key change**: Both `compile_entry` (build) and `analyze_workspace` (LSP) use the same
query-driven path. The batch compiler benefits from caching on repeated builds (e.g.,
watch mode). The LSP benefits from the same whole-program capability.

**Files to modify:**
- `boot/compiler/module_compiler.tw` — rewrite to use query Db
- `boot/compiler/query/diagnostics.tw` — thin wrapper over same queries
- `boot/compiler/pipeline.tw` — accept Db, thread through

**Result**: One path, two consumers. Build = compute all queries eagerly. LSP = compute
on demand with verification.

### Phase 6 (future): Extend to Backend

Once the frontend query model is stable, optionally extend to:
- `monomorphized(module)` — cache monomorphized CoreModule
- `anf(module)` — cache ANF lowering
- `optimized(module)` — cache optimization result

These are lower priority since backend phases run once (not on every keystroke).

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Fingerprint collisions | Silent stale cache | Use 64-bit FNV; add debug mode that deep-compares |
| TypeId instability across recompilations | Type confusion bugs | Phase 4 registry; integration test: edit + rebuild = same output |
| Cyclic query dependencies | Infinite loop | Cycle detection in `request()` (already have stack-based detection) |
| Performance regression on cold compile | Slower first build | Benchmark before/after; early phases add negligible overhead |
| Complexity increase | Harder to debug | Add `TWINKLE_QUERY_TRACE=1` logging showing query hits/misses/recomputes |

---

## Success Metrics

1. **LSP hover latency after body-only edit**: should drop to near-zero for importers (currently re-runs full workspace analysis)
2. **Cache hit rate in LSP steady-state**: >80% of resolve/typecheck queries should hit after typical editing
3. **Cold build time**: no more than 5% regression
4. **Warm rebuild time** (watch mode): proportional to changed modules only, not total module count

---

## Appendix: Comparison with Current Cache

| Aspect | Current (`query/cache.tw`) | Proposed (Salsa-like) |
|--------|---------------------------|----------------------|
| Invalidation | Eager cascade (`reverse_dependents_closure`) | Lazy verification on access |
| Granularity | Per-module, per-stage | Per-query (can be sub-module) |
| Early cutoff | None — if input changes, output assumed changed | Yes — compare fingerprints |
| Dep tracking | Module-level (`DependencyGraph`) | Per-query-invocation |
| Cache key | Composite FNV hash (stage + path + source + deps + context + global state like `next_global_local_id`) | Query kind + path (stable) |
| Verification | Hash comparison (recompute if hash differs) | Recursive dep verification |
| TypeId handling | Monotonic growth in `CompileState` | Stable registry with path-based allocation |
