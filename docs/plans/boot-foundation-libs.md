# Stage 10 Support Plan — `boot/lib` Foundation Libraries

## Goal

Define and land four foundational Twinkle libraries under `boot/lib/` that unblock the self-hosted compiler implementation:

1. `boot/lib/source` — spans, source files, file registry, line/col, snippets, diagnostics helpers.
2. `boot/lib/module` — project-root detection, module path resolution, stdlib/prelude discovery.
3. `boot/lib/graph` — dependency graph, reverse dependents, topo ordering, cycle checks.
4. `boot/lib/query` — deterministic cache keys and stage cache (later milestone).

This mirrors the Rust stage0 architecture while staying Twinkle-native:

- `src/syntax/span.rs`
- `src/module/loader.rs`
- `src/query/graph.rs`
- `src/query/keys.rs`
- `src/query/cache.rs`

## Why This Plan

`boot/` currently has test infrastructure and `lib/argparse`, but the self-hosted compiler still needs reusable infrastructure for source mapping, module loading, dependency orchestration, and incremental stage caching.

Without these libraries, compiler stages in `boot/` will either duplicate logic or hard-code behavior that later blocks incremental and multi-module compilation.

## Scope

In scope:

- API design and implementation plan for all four libraries.
- Delivery order with explicit dependencies.
- Test strategy in `boot/tests/suites/`.
- Determinism and portability constraints.

Out of scope:

- Full self-hosted compiler implementation (`boot/main.tw`, parser, checker, backend).
- LSP/editor features.
- Persistent on-disk cache format.

## Delivery Order

1. `boot/lib/source`
2. `boot/lib/module`
3. `boot/lib/graph`
4. `boot/lib/query` (after parser/resolve/typecheck artifacts exist)

Rationale:

- `source` and `module` are immediate prerequisites for parsing and file discovery.
- `graph` enables correct multi-module compile order and invalidation.
- `query` depends on stage artifacts and graph behavior, so it should land after early compiler stages exist.

## Milestone A — `boot/lib/source`

Reference: `src/syntax/span.rs`.

### Responsibilities

- Represent `FileId` and `Span`.
- Span utilities: merge, contains, length, empty check.
- File registry with file text and line start offsets.
- Lookup helpers: file name, source text, snippet by span, line/col conversion, full line text.
- Diagnostics helpers that convert spans into stable human-readable location data.

### Target API Shape (Twinkle)

- `type FileId = Int`
- `type Span = .{ file_id: FileId, start: Int, end: Int }`
- `type FileRegistry = ...`
- `fn span_merge(a: Span, b: Span) Span`
- `fn span_contains(s: Span, offset: Int) Bool`
- `fn span_len(s: Span) Int`
- `fn span_is_empty(s: Span) Bool`
- `fn add_file(reg: FileRegistry, name: String, source: String) AddFileResult`
- `fn snippet(reg: FileRegistry, span: Span) String?`
- `fn line_col(reg: FileRegistry, span: Span) .{ line: Int, column: Int }?`
- `fn line_text(reg: FileRegistry, span: Span) String?`

### Tests

- New suite: `boot/tests/suites/source_suite.tw`.
- Cover line start computation, line/col boundaries, multi-line snippets, empty spans, and out-of-bounds behavior.
- Run in both backends: `run -i` and `run`.

### Done Criteria

- API supports parser/typechecker diagnostic formatting needs.
- Deterministic outputs for same input text.
- No host interaction required.

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
