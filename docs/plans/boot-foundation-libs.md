# Phase E Foundation Libraries — module, graph, query

These libraries support multi-module compilation and incremental caching in
Phase E (Integration + Self-Hosting) of the [self-hosting plan](self-hosting.md).

**`boot/lib/source`** was split into its own plan: [boot-source-lib.md](boot-source-lib.md)
(immediate prerequisite for Phase A Frontend).

## Why Deferred

These libraries have no consumer until Phase E:

- **module** — single-source compilation in Phases A-D works with source strings
  passed directly; path resolution and project-root detection are only needed
  when wiring up multi-module compilation.
- **graph** — dependency ordering and invalidation serve `ProjectState`, which
  is a Phase E concern.
- **query** — stage caching requires stable stage artifacts to exist first.

Building them before their consumers exist risks designing APIs against
assumptions that won't survive contact with the actual compiler stages.

## Milestone B — `boot/lib/module`

Reference: `src/module/loader.rs`.

### Responsibilities

- Find project root by walking up from a start directory until `twinkle.toml`.
- Honor env overrides (`TWINKLE_ROOT`, `TWINKLE_STDLIB_ROOT`) consistently.
- Resolve module paths to `.tw` source paths.
- Resolve stdlib imports (`@std.*`) to `stdlib/*.tw`.
- Discover prelude modules from prelude root, with deterministic ordering.

### Target API Shape (Twinkle)

- `fn find_project_root(start: String) String`
- `fn resolve_module_path(root: String, module_path: Vector<String>) String`
- `fn resolve_stdlib_root_default() String`
- `fn resolve_prelude_root_default() String`
- `fn resolve_stdlib_module_path_from_root(stdlib_root: String, module_path: Vector<String>) String`
- `fn list_prelude_modules(prelude_root: String) Vector<String>`

### Tests

- New suite: `boot/tests/suites/module_loader_suite.tw`.
- Use temporary fixture trees under `boot/tests/fixtures/` for deterministic path behavior.
- Verify root walk, env override precedence, stdlib `@std.*` mapping, and sorted prelude listing.

### Done Criteria

- Behavior matches stage0 loader semantics for supported cases.
- Path output uses Twinkle logical path conventions (`/`).
- No nondeterministic filesystem ordering leaks.

## Milestone C — `boot/lib/graph`

Reference: `src/query/graph.rs` plus additional topo/cycle checks required for module planning.

### Responsibilities

- Maintain forward and reverse dependency maps.
- Update graph incrementally (`set_dependencies` style).
- Compute reverse-dependent closure for invalidation.
- Produce topological order for module compilation.
- Detect dependency cycles and return structured cycle diagnostics.

### Target API Shape (Twinkle)

- `type DependencyGraph = ...`
- `fn empty() DependencyGraph`
- `fn set_dependencies(g: DependencyGraph, module: String, deps: Vector<String>) DependencyGraph`
- `fn reverse_dependents_closure(g: DependencyGraph, changed: String) Vector<String>`
- `fn topo_sort(g: DependencyGraph, roots: Vector<String>) Vector<String>!GraphError`
- `type GraphError = { Cycle(Vector<String>) }`

### Tests

- New suite: `boot/tests/suites/dependency_graph_suite.tw`.
- Cases: add/remove deps, multi-hop reverse closure, disconnected graphs, stable topo order, self-cycle and multi-node cycle diagnostics.

### Done Criteria

- Deterministic ordering for equivalent graphs.
- Correct cycle detection with actionable cycle path output.
- Ready to back module compile planner in `boot/`.

## Milestone D — `boot/lib/query` (Later)

References: `src/query/keys.rs`, `src/query/cache.rs`.

### Responsibilities

- Deterministic keying/hashing helpers for parse/resolve/typecheck/lower stages.
- Cache records keyed by canonical module path + stage key.
- Stage hit/miss stats.
- Invalidation driven by dependency graph reverse closure.

### Target API Shape (Twinkle)

- `boot/lib/query/keys.tw`
  - `CACHE_SCHEMA_VERSION`
  - `hash_text`, `parse_key`, `resolve_key`, `typecheck_key`, `lower_key`
  - `deps_hash`, `module_hash`, `context_hash`, `with_context`
- `boot/lib/query/cache.tw`
  - `type CacheStats = ...`
  - `type QueryStageCache = ...`
  - `fn clear(cache: QueryStageCache) QueryStageCache`
  - `fn get_*`/`put_*` per stage
  - `fn invalidate_changed_module(cache: QueryStageCache, module: String) QueryStageCache`

### Tests

- New suite: `boot/tests/suites/query_cache_suite.tw`.
- Cases: hit/miss accounting, key stability, context-hash sensitivity, invalidation closure behavior, schema version bump behavior.

### Done Criteria

- Stable hashing and key semantics across runs.
- Correct invalidation for changed modules and dependents.
- No global mutable singleton requirement in Twinkle.

## Cross-Cutting Constraints

- Determinism first: explicit sorting at API boundaries where order is observable.
- Canonical paths as cache identity keys.
- Keep host interaction isolated to `module` and explicit callers.
- Avoid hidden global state in early Twinkle implementation; pass state values explicitly.
- Diagnostic messages should include enough location data to match stage0 quality.

## Suggested File Layout

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
    cache.tw         # lands in Milestone D
```

## Exit Criteria

This plan is complete when:

1. Milestones A-C are implemented and used by initial self-hosted compiler modules.
2. Corresponding suites pass in both backends (`run -i`, `run`).
3. Milestone D is implemented once parse/resolve/typecheck/lower artifacts are available in `boot/`.
4. Stage0 parity checks for path resolution, graph behavior, and cache key semantics are documented and green.
