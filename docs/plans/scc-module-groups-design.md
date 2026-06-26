# SCC-based recursive module groups — design

Status: **design approved, not yet implemented.**

This supersedes the "Future SCC design (deferred)" section of
[recursive-module-groups.md](recursive-module-groups.md). That document describes
the *landed* surgical back-edge approach (`break_import_cycle` + preliminary
interfaces). This design replaces that mechanism with proper Tarjan SCC grouping
in the boot compiler. stage0 stays on its current cycle-rejecting path until the
boot implementation is proven, then is mirrored as a follow-up.

## Goal

Lift Twinkle's acyclic-import resolution to acyclic-**group** resolution by
making the frontend driver:

1. discover the full module dependency graph for the compilation closure,
2. condense it into strongly-connected components (Tarjan),
3. resolve and typecheck each SCC in dependency-before-dependent order — a
   singleton SCC exactly as today, a multi-module SCC as one group.

Motivation is architectural hardening: an explicit recursive group makes the
cycle handling first-class (complete-cycle diagnostics, group-granular cache
invalidation, no order-dependent preliminary-interface edge cases) and is far
easier to mirror faithfully into stage0/Rust than the back-edge hack.

## Non-goals

- No change to the runtime, value model, or top-level evaluation semantics.
- No lazy/deferred top-level evaluation.
- No first-class "group" cache object — group invalidation is achieved by folding
  group siblings into each member's existing per-module hash (see Caching).
- stage0 changes are explicitly out of scope for this change; they follow once
  boot is proven.

## Key observation

The current `analyze_module_impl` DFS is *already Tarjan-shaped*: it carries an
explicit `stack` and checks `stack.contains(canonical)` for back-edges, and its
post-order `resolve_and_check_local` → `publish_interface` unwinding is exactly
SCC-ordered resolution **for the acyclic case** (every module is a singleton
SCC). "Proper SCC" therefore means: detect multi-module SCCs and resolve each as
a group, instead of breaking back-edges with signatures-only preliminary
interfaces.

A second observation shapes the architecture: **discovery is env-independent.**
`load_source`, `parse_cached`, and `plan_dependencies` need no `ResolvedEnv` —
only resolution/typechecking threads the env. So the graph can be built in a
clean first phase with no type machinery, which is the part that ports most
easily to Rust.

## Architecture: two passes

### Phase 1 — Discovery (env-independent)

A dedicated walk over the import closure starting at the entry module. Per
module, only:

- `load_source → parse_cached → plan_dependencies`,
- record adjacency `module → [dep canonical paths]`,
- memoize parse/import-plan failures exactly as today's `mark_failed` (a module
  that fails discovery is excluded; its dependents are marked failed).

Outputs: the dependency adjacency graph over canonical module paths, plus the
**discovery order** (first-reach order) used later as the deterministic
intra-group ordering. Parse and plan results land in the query cache, so Phase 2
reads them back without re-parsing.

### Condense — Tarjan over the module graph

Run Tarjan on the adjacency graph (string-keyed by canonical module path).
Reuse the existing Tarjan in `codegen/type_order.tw` — extract a shared
string-graph SCC utility if cleaner than calling it in place. Tarjan naturally
emits SCCs in reverse-topological (dependency-before-dependent) order, matching
the order `module_order` has today.

For each SCC, classify:

- size 1 with no self-loop → **singleton** (today's exact path),
- size > 1, or a self-loop → **recursive group**.

### Phase 2 — Resolution (env-threaded)

Walk SCCs in Tarjan order. External deps of each SCC are already published
(topo order guarantees it), so the env-threading that
`analyze_dependencies`/`merge_import_interface` does today is preserved: a group
is resolved against the already-published interfaces of everything outside it.

- **Singleton SCC** → `resolve_and_check_local` + `publish_interface`,
  byte-identical to the current per-module path.
- **Recursive group** → group two-phase resolution (below).

`module_order` is the flattening of the SCC order; group members appear in
discovery order.

## Group two-phase resolution (size > 1)

The cross-module lift of the resolver's existing in-module two-phase
(`resolver.tw`: Pass 1 collects names/signatures, Pass 2 resolves references and
checks bodies). For a group, with all external deps published:

1. **Group Pass 1** — for every member, resolve **signatures only** (top-level
   type definitions + function signatures) into a *combined* group env. Because
   all members' type names receive real TypeIds here, mutually-recursive types
   across files resolve directly against real definitions. The opaque-nominal
   stub machinery used by the back-edge path is therefore unnecessary and is
   removed.
2. **Group Pass 2** — typecheck each member's bodies against the combined env,
   then `publish_interface` for each member with its final checked interface.

Implementation hook: expose a "collect signatures for a set of modules" entry in
`resolver.tw` that the group path calls for all members before any body
checking. Single-module groups continue to use the existing combined
`resolve_and_check_local` flow unchanged.

## Value-initialization cycle rejection

A multi-module SCC whose members include top-level executable statements has
undefined cross-module init order — a genuine semantic hazard (unlike
type/function cycles). Keep the rejection, but with complete cycle knowledge:
after condensation, for any SCC of size > 1, if **any** member returns true from
`resolver.has_top_level_statements`, reject the whole group with a
`"Top-level initialization cycle"` diagnostic that can name **all** participating
modules (the back-edge version only saw the single module it tripped on).
Type/function-only cycles pass.

## Caching

Per-module cache keys are retained. Each member of a multi-module SCC folds all
its group siblings into its `deps_hash`/`context_hash`, so editing any member
invalidates and re-resolves the whole group through ordinary hash invalidation.
No new cache data structure. Acyclic programs are all singleton SCCs, so their
caching is unaffected.

## Prelude / stdlib injection

Unchanged from the landed behavior: blanket prelude-into-prelude injection stays
enabled; the planner skips only the current prelude module itself. The prelude is
just a function/type-only SCC like any other, resolved by the same group path.

## Removals

The preliminary-interface mechanism in `query/analyze.tw` is deleted:
`break_import_cycle`, `preliminary_type_interface`, `preliminary_type_exports`,
`next_preliminary_type_id`, the back-edge branch at the `stack.contains` check,
and the "merge preliminary interface back" block in `analyze_module_impl`.
Cycles are no longer discovered by back-edge; they are known from Tarjan before
any resolution begins.

## Touch points (boot compiler)

- `query/analyze.tw` — split the DFS into Phase 1 discovery + Phase 2
  SCC-ordered resolution; add the group two-phase path; keep the singleton fast
  path; emit `module_order` from the SCC flattening; delete the
  preliminary-interface code.
- `resolver.tw` — expose group signature collection (Pass 1 over a set of
  modules) ahead of body checking.
- `imports.tw` — surface the per-module dependency list for graph building (data
  is already computed by `plan_dependencies`).
- `codegen/type_order.tw` — reuse/extract the string-graph Tarjan SCC.
- caching (`query/cache.tw`, `stage_runner.tw`, `deps_hash` in `analyze.tw`) —
  fold group siblings into each member's hash for multi-module groups.
- `module_compiler.tw` — unaffected in shape; still consumes `module_order`.

## Validation

- **Acyclic = byte-identical (non-negotiable):** every real program, including
  `boot/main.tw`, is all singleton SCCs. The self-host fixed point and the full
  boot test suite must pass unchanged.
- The existing `multi_module_suite` cycle tests must pass through the new group
  path: import cycles, mutually-recursive functions, mutually-recursive types,
  value-init rejection, and a three-module A→B→C→A cycle.
- Incremental: editing one member of a multi-module group re-resolves the whole
  group and nothing outside it.
- Prelude-into-prelude smoke: a prelude module calling another prelude module's
  function.

## stage0 follow-up (out of scope here)

Once boot is proven on the self-host loop, mirror the two-phase structure in
`src/module/` (`planner.rs`, `compile_module`, `compile_planned_dependencies`)
and the `src/types/` resolver entry points: Phase 1 discovery + Tarjan
condensation + Phase 2 group resolution, plus the planner tests that currently
assert acyclic-only behavior.
