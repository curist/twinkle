# Phase E Foundation Libraries — module, graph, query

These libraries support multi-module compilation and incremental caching in
Phase E (Integration + Self-Hosting) of the [self-hosting plan](self-hosting.md).

**`boot/lib/source`** was split into its own plan: [boot-source-lib.md](boot-source-lib.md)
(immediate prerequisite for Phase A Frontend).

## Status

- **Milestone B (`boot/lib/module`)** — Done (2026-03-26)
- **Milestone C (`boot/lib/graph`)** — Done (2026-03-26)
- **Milestone D (`boot/lib/query`)** — `keys.tw` done (2026-03-26); `cache.tw` deferred until Phase E multi-module pipeline defines what gets cached

Phases A–D of the self-hosted compiler are complete. All foundation
libraries except `cache.tw` are implemented and tested.

## Milestone B — `boot/lib/module`

Reference: `src/module/loader.rs`. **Status: Done.**

### Design Decision

`stdlib/` and `prelude/` are symlinked into `boot/` so they're always
resolvable relative to the boot project root. No env var fallback chains
or `cwd()` guessing needed — just `path.join(project_root, "stdlib")`.

### Implemented API

- `fn find_project_root(start: String) String` — walks up for `twinkle.toml`, honors `TWINKLE_ROOT`
- `fn resolve_module_path(root: String, module_path: Vector<String>) String`
- `fn resolve_relative_module_path(importing_file: String, module_path: Vector<String>) String`
- `fn resolve_stdlib_root(project_root: String) String`
- `fn resolve_prelude_root(project_root: String) String`
- `fn resolve_stdlib_module_path(stdlib_root: String, module_path: Vector<String>) String`
- `fn list_prelude_modules(prelude_root: String) Vector<String>`

### Tests

- Suite: `boot/tests/suites/module_loader_suite.tw` — 21 tests
- Covers: root walk, env override, path resolution, relative imports, stdlib mapping, sorted prelude listing, edge cases (bare filename, empty segments)

## Milestone C — `boot/lib/graph`

Reference: `src/query/graph.rs` plus topo sort / cycle detection. **Status: Done.**

### Implemented API

- `type DependencyGraph = .{ forward: Dict<String, Vector<String>>, reverse: Dict<String, Vector<String>> }`
- `type GraphError = { Cycle(Vector<String>) }`
- `fn empty() DependencyGraph`
- `fn set_dependencies(g: DependencyGraph, module: String, deps: Vector<String>) DependencyGraph` — incremental update with dedup, maintains forward/reverse consistency
- `fn reverse_dependents_closure(g: DependencyGraph, changed: String) Vector<String>` — BFS transitive closure, sorted output
- `fn topo_sort(g: DependencyGraph, roots: Vector<String>) Result<Vector<String>, GraphError>` — Kahn's algorithm, deterministic ordering, cycle detection

### Tests

- Suite: `boot/tests/suites/dependency_graph_suite.tw` — 20 tests
- Covers: add/remove/update deps, dedup, idempotency, single/multi-hop reverse closure, sorted output, linear/diamond/multi-root topo sort, self-cycle, multi-node cycle, cycle with blocked non-cyclic nodes

### Known Limitation

Cycle diagnostics report all nodes blocked by a cycle (not just the minimal cycle). Nodes that depend on a cyclic component are included in `GraphError.Cycle`. Acceptable for current use; could be refined to extract the minimal strongly-connected component if needed.

## Milestone D — `boot/lib/query`

References: `src/query/keys.rs`, `src/query/cache.rs`.

### `keys.tw` — Done

FNV-1a hashing with hash values verified against the Rust reference implementation via cross-implementation tests.

**Implemented API:**
- `fn hash_text(text: String) Int` — FNV-1a over UTF-8 bytes
- `fn parse_key(path, source_hash) Int`
- `fn resolve_key(path, source_hash, deps_hash) Int`
- `fn typecheck_key(path, source_hash, deps_hash, allow_host_builtins) Int`
- `fn lower_key(path, source_hash, deps_hash, next_global_local_id) Int`
- `fn deps_hash(entries: Vector<DepEntry>) Int` — order-independent
- `fn module_hash(source_hash, deps_hash) Int`
- `fn context_hash(entries: Vector<DepEntry>) Int` — order-independent
- `fn with_context(base_key, ctx_hash) Int`
- `type DepEntry = .{ path: String, hash: Int }`

**Tests:** `boot/tests/suites/query_keys_suite.tw` — 20 tests. Includes cross-reference checks against Rust FNV-1a output values.

**Implementation note:** Twinkle `Int` is i64 with wrapping arithmetic, which produces identical bit patterns to Rust's `u64::wrapping_mul`. Hex literals above `0x7fffffffffffffff` are out of range for i64; use `(high << 32) | low` to express them.

### `cache.tw` — Deferred

Cache storage depends on how Phase E's multi-module pipeline threads state. Building it before the consumer exists risks designing against wrong assumptions. Will implement once the multi-module compilation loop takes shape.

**Planned API (from `src/query/cache.rs`):**
- `type QueryStageCache` — per-stage `Dict<String, CacheEntry>` with key-gated lookup
- `fn get_*/put_*` per stage (parse, resolve, typecheck, lower)
- `fn invalidate_changed_module(cache, module)` — uses `reverse_dependents_closure`
- `type CacheStats` — hit/miss counters

## Cross-Cutting Constraints

- Determinism first: explicit sorting at API boundaries where order is observable.
- Canonical paths as cache identity keys.
- Keep host interaction isolated to `module` and explicit callers.
- Avoid hidden global state; pass state values explicitly.

## File Layout

```text
boot/lib/
  source/
    span.tw
    registry.tw
    diagnostic.tw
  module/
    loader.tw
  graph/
    dependency.tw
  query/
    keys.tw
    cache.tw         # deferred to Phase E
```

## Exit Criteria

This plan is complete when:

1. ~~Milestones B-C are implemented~~ — Done.
2. ~~Corresponding suites pass in both backends (`run -i`, `run`)~~ — Done (61 tests total).
3. ~~`keys.tw` implemented with stage0 parity~~ — Done (20 tests, cross-reference verified).
4. `cache.tw` is implemented once Phase E multi-module pipeline defines caching needs.
