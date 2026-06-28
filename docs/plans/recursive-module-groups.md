# Recursive module groups — allow mutually-recursive modules

Status: **SCC-based boot-compiler implementation landed**. The boot frontend now
discovers the import closure, condenses it with Tarjan SCCs, and resolves each
component in dependency-before-dependent order. Singleton SCCs use the normal
per-module path; multi-module SCCs use group declaration collection, reference
resolution, alias checking, typechecking, and per-member publish. Function/type
cycles compile, top-level value-initialization cycles are rejected with a
whole-cycle diagnostic, and group members fold sibling source hashes into their
cache keys. stage0 remains deferred. Resolves open-question #3
([../open-questions.md](../open-questions.md)). See
[scc-module-groups-design.md](scc-module-groups-design.md) for the implemented
architecture and [scc-module-groups-plan.md](scc-module-groups-plan.md) for the
execution record.

## Landed in boot compiler

- `graph_scc.strongly_connected` provides the shared Tarjan SCC utility used by
  module grouping and type ordering.
- `discover_closure` performs env-independent load/parse/import planning and
  records the module dependency graph.
- `analyze_module_scc` is the production driver: discover → condense → resolve
  SCCs in dependency-before-dependent order.
- Singleton SCCs retain the normal per-module path, including progress events,
  timings, closure snapshots, unused-import diagnostics, and lint/rewrite data.
- Multi-module SCCs run group resolution: declaration collection for all members
  with a shared TypeId cursor, reference resolution with sibling declaration
  interfaces, circular-alias checking, typechecking, and range-aware publish for
  each member.
- Top-level value-init cycles are rejected before group resolution, and the
  diagnostic names the modules in the cycle.
- Group members mix sibling source hashes into their cache keys so editing any
  member invalidates the whole group.
- The old DFS/back-edge preliminary-interface mechanism has been removed from the
  boot compiler.

Still open: the **stage0 mirror** (stage0 still rejects all cycles; fine for
bootstrap since `boot/main.tw` is acyclic).

## The restriction is architectural, not semantic

Twinkle rejects circular imports today. This is **not** a runtime safety issue —
self- and mutually-recursive *functions* are fine. It's an artifact of how modules
are resolved:

- The frontend resolves modules in **topological order**: to check module A it
  needs the published *interface* (exported types + signatures) of everything A
  imports.
- `analyze.tw` drives this as a DFS with a stack; a module already on the stack
  triggers `"Circular import detected"` (`analyze.tw:240`).
- A cycle has no topological order, so the DFS aborts.

Meanwhile, **within a single module** mutual recursion already works, because the
resolver is two-phase (`resolver.tw:3-4`):

- **Pass 1** — collect every top-level type + function *name/signature*.
- **Pass 2** — resolve references and check bodies, with all names already visible.

So the machinery to resolve a mutually-recursive set already exists; it just runs
at single-module scope. **This plan lifts that two-phase resolution to a *group*
of modules.**

## The one genuine hazard: top-level value initialization

There is a case where a cycle is more than an ordering nuisance. Twinkle runs
top-level statements, so a module can have top-level *value* bindings. If module A's
top-level value reads B's top-level value and B's reads A's, "which initializes
first?" is genuinely ill-defined — a real semantic hazard, not just a resolution
problem.

Crucially, **types and functions have no such hazard** (they're resolved, not
executed in order), and **prelude/stdlib modules are function/type-only** — so a
cycle among them is completely safe.

**MVP rule:** allow cycles through **types and function signatures**; **reject**
import cycles that participate in a **top-level value initialization** cycle, with
a clear diagnostic. (A later phase could define a deterministic init order or lazy
initialization — see open questions.)

## SCC design (implemented in boot)

The SCC approach below is now the boot compiler's production architecture — see
[scc-module-groups-design.md](scc-module-groups-design.md) for the canonical
design. It replaced the earlier surgical DFS/back-edge approach.

### 1. Build the module dependency graph

`imports.plan_dependencies` already yields each module's deps. Instead of resolving
strictly depth-first, build the full directed graph of `(module → its deps)` for
the compilation closure.

### 2. Condense into SCCs

Run Tarjan over the module graph to get strongly-connected components. **Reuse the
existing Tarjan implementation** in `codegen/type_order.tw` (currently used for the
type-ordering worklist) rather than writing a second one. The condensed graph
(SCC DAG) gives a topological order *of groups*:

- SCC of size 1 with no self-loop → today's exact path (unchanged).
- SCC of size > 1 (or a self-loop) → a **recursive module group**, resolved together.

### 3. Group-scoped two-phase resolution

For each group, in SCC-DAG topological order (so all *external* deps of the group
are already published):

1. **Group Pass 1** — collect the exported types + function signatures of *every*
   module in the group into a combined interface, and make that interface visible
   to all members. This is the cross-module analogue of resolver Pass 1.
2. **Group Pass 2** — resolve references and typecheck each member's bodies against
   the combined env, then publish each member's final interface
   (`publish_interface`, `analyze.tw:661`).

Single-module groups collapse to the current `resolve_and_check_local` +
`publish_interface` flow.

### 4. Value-initialization cycle check

After grouping, within each multi-module group detect whether top-level *value*
bindings form a cycle across modules (value in A referencing a value in B and back).
If so, emit a targeted diagnostic (distinct from today's blanket "circular import").
Type/function-only cycles pass.

## Touch points

**Boot compiler**
- `imports.tw` — expose the full dependency graph for the closure (the data is
  already computed per module).
- `query/analyze.tw` — replace the DFS-with-stack circular check (`:240`) with
  SCC grouping; add the group two-phase path; keep the single-module fast path.
- `module_compiler.tw` — orchestrate compilation per SCC group (it already drives
  per-module analyze → lower → link; now drives per-group).
- `resolver.tw` — generalize Pass-1 signature collection to accept a set of
  modules (group interface) before Pass-2 body checking.
- caching (`query/cache.tw`, `stage_runner.tw`, `deps_hash` `analyze.tw:140`) —
  the **invalidation unit becomes the group**: editing any member re-resolves the
  whole group (its members are mutually dependent, so their `deps_hash` should
  fold in the group, not just acyclic ancestors).
- Tarjan reuse from `codegen/type_order.tw` (extract to a shared graph util if
  cleaner).

**Stage0 (reference, for parity / self-host bootstrap)**
- `src/module/planner.rs`, `src/module/mod.rs` (`compile_module`,
  `compile_planned_dependencies`) — mirror the SCC grouping + group resolution.
- `src/types/` resolver entry points — group-scoped signature collection.
- Update the planner tests asserting acyclic behavior.

## Interaction with prelude/stdlib injection

This is now landed in the boot compiler: stdlib and prelude modules both receive
blanket prelude injection, and the planner skips only the current prelude module
itself to avoid a trivial self-import edge. The prelude is therefore a safe
function/type-only recursive dependency set, so prelude modules may freely use
each other's exported functions without explicit one-off injection rules.

## Testing

- Mutually-recursive **types** split across two files (e.g. `Expr` ↔ `Stmt`).
- Mutually-recursive **functions** across files.
- Three-module cycle (A→B→C→A) resolves; SCC grouping correct.
- **Value-init cycle** across modules is rejected with the targeted diagnostic.
- Acyclic programs are byte-identical to before (no regression; single-module
  fast path unchanged) — verify via self-host fixed point.
- Prelude-into-prelude smoke: a prelude module calling another prelude module's
  function once injection is enabled.
- Incremental: editing one member of a group re-resolves the whole group and
  nothing outside it.

## Open questions

- **Top-level value cycles** — MVP rejects them. Do we ever want to allow them via
  a defined initialization order (SCC-DAG order + intra-group source order) or lazy
  initialization? Most languages with module-level code either order by dependency
  or trap on access-before-init. Likely keep rejecting for now.
- **Cache granularity** — group-as-invalidation-unit is correct but coarsens
  incremental rebuilds for large cyclic groups. Acceptable? (Prelude is small.)
- **Diagnostics UX** — when a value-init cycle is rejected, point at the offending
  value bindings, not just the imports.
- **Scope of the first cut** — ship type/function cycles only, or also wire the
  prelude-injection flip in the same change? Lean: land groups first (with prelude
  staying explicit-only), then flip injection in a follow-up once groups are
  proven on the self-host loop.
- **Tarjan extraction** — reuse `codegen/type_order.tw`'s SCC in place, or extract
  a shared `graph`/`scc` util used by both the type-order pass and the module
  grouper.

## Non-goals

- No change to the runtime or value model.
- No lazy/deferred top-level evaluation in the MVP.
- Not a package/namespace redesign — purely lifting the acyclic-import restriction
  to acyclic-*group* ordering.
