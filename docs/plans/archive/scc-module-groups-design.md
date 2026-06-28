# SCC-based recursive module groups — design

Status: **archived; implemented in the boot compiler.** stage0 parity is deferred to a separate future plan only if stage0 is revived as an active target.

This supersedes the "Future SCC design" section of
[recursive-module-groups.md](recursive-module-groups.md). The boot compiler now
uses this Tarjan SCC grouping architecture in production; the earlier surgical
back-edge approach (`break_import_cycle` + preliminary interfaces) has been
removed. stage0 remains on its current cycle-rejecting path.

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
- stage0 changes are explicitly out of scope for this change; they are deferred
  unless stage0 parity becomes active work again.

## Key observation

The former `analyze_module_impl` DFS was Tarjan-shaped: it carried an explicit
`stack` and checked `stack.contains(canonical)` for back-edges, and its
post-order `resolve_and_check_local` → `publish_interface` unwinding matched
SCC-ordered resolution for the acyclic case. The implemented SCC frontend makes
that structure explicit: discover the graph first, detect multi-module SCCs, and
resolve each as a group instead of breaking back-edges with signatures-only
preliminary interfaces.

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
- **Recursive group** → group resolution (below).

`module_order` is the flattening of the SCC order; group members appear in
discovery order.

## Group resolution (size > 1)

This generalizes the resolver's in-module three-pass `resolve()`
(`collect_declarations` → `resolve_references` → `detect_circular_aliases`,
resolver.tw:1246) to a group. The cross-module hazard lives *between*
declaration-collection and reference-resolution: for `type A = .{ b: B }` in M1
and `type B = .{ a: A }` in M2, resolving M1's field `b: B` needs only B's
TypeId + arity registered, which collecting M2's declarations provides. So the
group runs **all** declaration-collection before **any** reference-resolution:

- **A — collect declarations, all members.** For each member, run
  `collect_declarations` against the group's *external* env only (its own
  scope), producing a declaration-only interface (type names + TypeIds + arity,
  fn names; `def == .None`). TypeIds stay globally unique across the group by
  threading an **explicit id cursor** between members (a new parameter on the
  collection entry / `next_available_type_id`), **not** by sharing one env — see
  "Why not one flat env" below.
- **B — resolve references, all members.** Build each member's env as
  `base + its external imports + the selectively-merged declaration interfaces of
  the sibling group members it imports` (via the existing `merge_import_interface`
  path). Run `resolve_references` for each member against that env. Mutual
  type/function references now resolve to the real sibling TypeIds from step A.
- **C — circular-alias check, once.** Run `detect_circular_aliases` over the
  combined group view after step B, so type-alias cycles that span modules are
  caught (it is intra-module inside `resolve()` today).
- **D — typecheck + publish, per member.** Typecheck each member's bodies and
  `publish_interface` its final checked interface, with range-aware local-type
  capture (see "Publish" below).

`topo_sort_type_decls` inside `resolve_references` stays intra-module, unchanged:
cross-module field references need only the sibling's TypeId + arity from step A,
not a combined topological order, so no group-wide type sort is required.

Singleton SCCs keep calling the existing welded `resolve()` + `publish_interface`
unchanged.

### Why not one flat combined env

A single env shared across all members is wrong on two counts, both verified
against the code:

- **False collisions.** `collect_declarations` flags `DuplicateName` when a name
  is already `has_type` in the env (resolver.tw:1332). Two members each defining
  a common local name (`Entry`, `Ctx`) would collide spuriously. This also rules
  out "thread one env through sequential `collect_declarations` for free unique
  IDs": that env *is* the shared scope that false-collides. Per-member collection
  against the external env, with an explicit id cursor for uniqueness, avoids it.
- **Visibility leak.** `resolve_references` looks types up by `env.lookup_type`
  with no import filter, so a flat env would let a member reference a sibling's
  type without a `use`. Merging sibling declarations *selectively* (only what a
  member imports) preserves today's module-scoped visibility.

The shape is therefore **per-member scoped envs + a shared TypeId allocator**,
which keeps IDs globally unique while names stay module-private. This is the same
scoping the back-edge path had; what changes is that the whole group's
declarations are allocated up front (no opaque stubs, no later renumbering).

### Publish: range-aware local-type capture

`publish_interface` → `capture_local_types` (analyze.tw:1375) currently
partitions the env by a single `local_type_start` offset: `< start` is
imported/shared, `[start, len)` is "this module's locals." Each member instead
records the `[start, end)` range of TypeIds it allocated in step A, and
`capture_local_types` becomes range-aware, keyed on the member's own allocated
range rather than a global env tail.

## Value-initialization cycle rejection

A multi-module SCC whose members include top-level executable statements has
undefined cross-module init order — a genuine semantic hazard (unlike
type/function cycles). Keep the rejection, but with complete cycle knowledge:
after condensation, for any SCC of size > 1, if **any** member returns true from
`resolver.has_top_level_statements`, reject the whole group with a
`"Top-level initialization cycle"` diagnostic that can name **all** participating
modules (the back-edge version only saw the single module it tripped on).
Type/function-only cycles pass.

This rejection **moves** from `break_import_cycle` to this post-condensation
check — it is not removed; the new site simply has whole-cycle knowledge. When
naming the participating modules, sort them by canonical path so the diagnostic
is stable rather than dependent on discovery reach order (`module_order` still
uses discovery order).

## Caching

Per-module cache keys are retained. Each member of a multi-module SCC folds all
same-SCC sibling source hashes into its `deps_hash`, so editing any member
invalidates the whole group's typecheck/lower artifacts through ordinary hash
invalidation. No new cache data structure. Acyclic programs are all singleton
SCCs, so their caching is unaffected.

## Prelude / stdlib injection

Unchanged from the landed behavior: blanket prelude-into-prelude injection stays
enabled; the planner skips only the current prelude module itself. The prelude is
just a function/type-only SCC like any other, resolved by the same group path.

## Removals

The back-edge *discovery* mechanism in `query/analyze.tw` is deleted:
`break_import_cycle`, `next_preliminary_type_id`, the back-edge branch at the
`stack.contains` check, the opaque-nominal-stub injection, and the "merge
preliminary interface back" block in `analyze_module_impl`. Cycles are no longer
discovered by back-edge; they are known from Tarjan before any resolution begins.

The old preliminary-interface helpers were removed with the back-edge path. Group
resolution now builds declaration-only and signature-complete interfaces directly
from each member's collected/resolved env, using `opaque_type_exports` to hide
incomplete type definitions while preserving the real TypeIds allocated by the
threaded cursor.

## Touch points (boot compiler)

- `query/analyze.tw` — split the DFS into Phase 1 discovery + Phase 2
  SCC-ordered resolution; add the group resolution path; keep the singleton fast
  path; emit `module_order` from the SCC flattening; make
  `capture_local_types`/`local_type_start` range-aware (per-member `[start,end)`);
  delete the back-edge discovery code.
- `resolver.tw` — split the welded `resolve()` so `collect_declarations`,
  `resolve_references`, and `detect_circular_aliases` are separately callable by
  the group path (step A across all members, then B, then C); thread an explicit
  TypeId cursor through `collect_declarations`/`next_available_type_id` for
  group-wide unique IDs.
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

If stage0 parity becomes active work again, mirror the discovery +
group-resolution structure in `src/module/` (`planner.rs`, `compile_module`,
`compile_planned_dependencies`) and the `src/types/` resolver entry points:
Phase 1 discovery + Tarjan condensation + Phase 2 group resolution (steps A–D),
plus the planner tests that currently assert acyclic-only behavior.
