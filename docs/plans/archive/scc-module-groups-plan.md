# SCC-based recursive module groups — Implementation Plan

Status: **archived; completed for the boot compiler.** stage0 parity is deferred to a separate future plan only if stage0 is revived as an active target.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the boot compiler's surgical back-edge cycle handling with proper Tarjan SCC grouping — an explicit two-pass driver (env-independent discovery → Tarjan condensation → SCC-ordered group resolution).

**Architecture:** Phase 1 discovers the module dependency graph by load/parse/plan only (no env). Tarjan condenses it into SCCs in dependency-before-dependent order. Phase 2 resolves each SCC: singletons via the existing per-module path (byte-identical), multi-module groups via a four-step group resolution (collect declarations for all members with a shared TypeId allocator → resolve references for all → circular-alias check once → typecheck + publish per member). Group members fold each other into their cache hashes. stage0 is out of scope here.

**Tech stack:** Twinkle (`.tw`), boot compiler in `boot/compiler/`. Tests in `boot/tests/suites/`, run via `make boot-test`. Self-host fixed point via `make stage2`. Final CLI via `make bundle-cli`.

**Design reference:** [`scc-module-groups-design.md`](scc-module-groups-design.md).

---

## Progress / Resume Here (updated 2026-06-28)

Executing on branch `scc-module-groups` (branched from `main` after the design+plan commits). Tasks 1–11 are **DONE** for the boot compiler. The production frontend uses the SCC two-pass driver, the old recursive back-edge mechanism has been removed, group cache invalidation is wired, docs are updated, and the bundled CLI has been rebuilt. stage0 parity is deferred to separate future work only if needed.

- [x] **Task 1** — `boot/compiler/graph_scc.tw` (reusable Tarjan SCC); `type_order.tw` routed through it; hardened tests (self-loop/membership/disconnected). Commits `a21fe64b`, `5036bdd4` (restored root-first intra-component order for byte-parity), `b10d57e4`.
- [x] **Task 2** — resolver passes exposed (`resolve_references`/`detect_circular_aliases` pub) + `collect_declarations_from(env, module, id_start)` / `DeclCollection` / `next_type_id` threading a TypeId cursor; `collect_declarations` delegates. Commit `6cefb982`. (Accepted-minor: `id_start` vs `start_id` naming; `next_type_id` wraps `next_available_type_id`.)
- [x] **Task 3** — range-aware `capture_local_types_range` / `publish_interface_range`; singleton wrappers unchanged. Commit `a1a4d243`.
- [x] **Task 4** — env-independent `discover_closure` (Phase 1) + `Discovery` type; deduped identity, `record_failure` helper, edge-assert test. Commits `ba904ae5`, `4129ea79`.
- [x] **Task 5** — `build_import_env` extracted (named result `ImportEnvResult`; stage0 has no anon-record returns) and `analyze_dependencies` retrofitted to use it; byte-identical self-host. Commit `83436934`.
- [x] **Task 6** — `resolve_group` (steps A–D) + `resolve_singleton` + new `pub fn analyze_module_scc` driver in `analyze.tw`, not yet wired at that point. Commits `a3463a29`, `fe633fe8`. New `group_resolution_suite.tw` proves cycle fixtures compile through the SCC path and value-init cycles are rejected. Implementer caught a real bug: the TypeId cursor must seed from `extend_env_from_shared(base, cur).next_type_id()` (NOT `base`), else collected decls alias onto prelude TypeIds. Spec review verified per-member scoped envs (no flat-env collision), disjoint cursor ranges, step-B own-decls+merged-siblings env, value-init naming, and step-D via `stage_runner.typecheck` (no re-allocation).
- [x] **Task 7 — DONE, committed `b19b2678`.** `analyze_module` now delegates to `analyze_module_scc`, making Phase 1 discovery → Tarjan condensation → Phase 2 SCC-ordered resolution the production path. Singleton fidelity gaps are closed: closure snapshot capture, progress events, timings, dependency-failure cache clearing/`mark_failed`, unused-import warnings, and lint/rewrite findings are restored. Existing back-edge code is intentionally left present but unreferenced for Task 8 deletion. The implementation did **not** extract Phase 2 into `analyze_scc.tw`; keep that as an optional cleanup after deleting the dead back-edge code if the split is still worthwhile.

**Carry forward:**
- **INVARIANT (documented in code):** group publish captures the member's `[idx_start, idx_end)` env.types INDEX range computed on the *declaration* env but applied to the *typechecked* env. Sound ONLY because `resolve_references`/`check` never insert type entries within that slice (they fill defs in place + append only functions/sibling types past it). If a future resolver/checker change appends a member-owned type within the slice, group capture must be recomputed on the typechecked env. (Record this for the stage0 mirror too.)
- **Step C is per-member** `detect_circular_aliases`, not the design's combined-view — cross-module type-alias cycles uncaught (bounded by `expand_alias` depth-10, can't hang). Close when mirroring to stage0.

- [x] **Task 8 — DONE.** Deleted the dead back-edge mechanism: `analyze_module_impl`, `analyze_dependencies`, `break_import_cycle`, `next_preliminary_type_id`, preliminary-interface helpers, opaque stubs, the stack/back-edge branch, and public `analyze_module` stack/alias plumbing. `resolve_group` keeps the reusable `opaque_type_exports` helper for declaration/signature-complete group interfaces. Validation passed: `make boot-test`, `make stage2` (already up to date after boot-test), `target/twk lint boot/main.tw`, and `make rust-test`.

- [x] **Task 9 — DONE.** Added fixtures/tests for a three-module cycle, cycle members that reuse the same private local type name, and a value-init diagnostic that names all participating modules.
- [x] **Task 10 — DONE.** `resolve_group` folds same-SCC sibling source hashes into each member's deps hash so editing any cycle member invalidates the whole group's typecheck/lower artifacts. Added an incremental query test that edits one cycle member and verifies another member's typed artifact is refreshed.
- [x] **Task 11 — DONE.** Rebuilt `target/twk` with `make bundle-cli`, reran `make boot-test`, updated recursive-module-group docs/design/index status, and reran lint/Rust validation. stage0 mirror remains pending.

---

## Orientation (read before starting)

Key files and current behavior:

- `boot/compiler/query/analyze.tw` — the frontend driver. `analyze_module` now
  delegates to `analyze_module_scc`: Phase 1 `discover_closure` loads/parses/plans
  the closure, Tarjan condenses it, and Phase 2 resolves singleton SCCs via
  `resolve_singleton` or multi-module SCCs via `resolve_group`. Range-aware
  `publish_interface_range` captures each group's member-owned type slice, and
  `module_order` is appended as SCCs publish.
- `boot/compiler/resolver.tw` — `resolve()` (~1246) welds three passes:
  `collect_declarations` (~1316) → `resolve_references` (~1425) →
  `detect_circular_aliases` (~2877). `next_available_type_id` (~1283) is max+1
  over the env. `has_top_level_statements` (~1235). Declarations register types
  with `def == .None`; references fill in field/variant types.
- `boot/compiler/query/stage_runner.tw` — `resolve` (~49) wraps `env.resolve(module)`
  with caching keyed on `source_hash`/`deps_hash`/`context_hash`; `typecheck`
  (~69) wraps `checker.check(module, resolved.env, lint_mode)`.
- `boot/compiler/graph_scc.tw` — shared string-keyed Tarjan SCC utility used by
  both module grouping and `boot/compiler/codegen/type_order.tw`.
- `boot/compiler/imports.tw` — `plan_dependencies` yields a `DependencyPlan`
  whose `dependencies` carry `canonical_path`, `alias`, `kind`, `items`, `span`.
- `boot/compiler/module_compiler.tw` — `compile_entry` calls `analyze_module`
  then walks `module_order`. Unchanged in shape by this work.
- `boot/tests/suites/multi_module_suite.tw` — cycle tests call
  `module_compiler.compile_entry("${dir}/fixture.tw")` and assert `.Ok`/`.Err`.
  Fixtures in `boot/tests/fixtures/multi/`: `circular_a/b`, `cycle_fn_a/b`,
  `cycle_type_a/b`, `cycle_value_a/b`.

**Non-negotiable invariant:** acyclic programs are all singleton SCCs. The
self-host fixed point (`make stage2`) and full boot suite (`make boot-test`) must
stay green at every commit. Multi-module groups must resolve after their external
dependencies and publish each member independently.

**Commands used throughout:**
- Build CLI in use: `target/twk` (already built).
- Run boot tests: `make boot-test`
- Self-host fixed point: `make stage2`
- Format: `target/twk fmt <file>` after editing any `.tw`
- Lint: `target/twk lint boot/main.tw`

---

## Task 1: Extract a reusable string-graph SCC utility

Pull the Tarjan in `type_order.tw` into a standalone string-graph SCC so both the
type-order pass and the module grouper share it. Behavior-preserving for types.

**Files:**
- Create: `boot/compiler/graph_scc.tw`
- Modify: `boot/compiler/codegen/type_order.tw` (route its Tarjan through the new util)
- Test: `boot/tests/suites/graph_scc_suite.tw` (create), registered in `boot/tests/main.tw`

- [ ] **Step 1: Write the failing test**

Create `boot/tests/suites/graph_scc_suite.tw`:

```tw
use @std.testing.{ Suite }
use ...compiler.graph_scc

pub fn suite() Suite {
  Suite.new("graph_scc")
    .test(
      "singletons in reverse-topological order",
      fn() {
        // a -> b -> c (a depends on b, b on c)
        edges: Dict<String, Vector<String>> = Dict.new()
        edges["a"] = ["b"]
        edges["b"] = ["c"]
        edges["c"] = []
        comps := graph_scc.strongly_connected(["a", "b", "c"], edges)
        // dependency-before-dependent: c, then b, then a
        try assert.eq(comps.len(), 3)
        try assert.eq(comps[0], ["c"])
        try assert.eq(comps[1], ["b"])
        try assert.eq(comps[2], ["a"])
        .Ok({})
      },
    )
    .test(
      "a 2-cycle is one component",
      fn() {
        edges: Dict<String, Vector<String>> = Dict.new()
        edges["a"] = ["b"]
        edges["b"] = ["a"]
        comps := graph_scc.strongly_connected(["a", "b"], edges)
        try assert.eq(comps.len(), 1)
        try assert.eq(comps[0].len(), 2)
        .Ok({})
      },
    )
    .test(
      "three-module cycle a->b->c->a is one component",
      fn() {
        edges: Dict<String, Vector<String>> = Dict.new()
        edges["a"] = ["b"]
        edges["b"] = ["c"]
        edges["c"] = ["a"]
        comps := graph_scc.strongly_connected(["a", "b", "c"], edges)
        try assert.eq(comps.len(), 1)
        try assert.eq(comps[0].len(), 3)
        .Ok({})
      },
    )
}
```

(Match the exact import path/prefix style used by sibling suites — check the top
of `boot/tests/suites/multi_module_suite.tw` for the `use` prefix convention.)

- [ ] **Step 2: Run to verify it fails**

Register `graph_scc_suite.suite()` in `boot/tests/main.tw` alongside the others,
then run `make boot-test`. Expected: FAIL (module `graph_scc` undefined).

- [ ] **Step 3: Implement `graph_scc.tw`**

Port the Tarjan structure from `codegen/type_order.tw` (`tarjan_visit`), but
parameterized over a node list + an adjacency `Dict<String, Vector<String>>`.
Return `Vector<Vector<String>>` in dependency-before-dependent order (the order
Tarjan emits components as roots pop — same as `type_order.tw` produces today).

```tw
//! Generic string-keyed strongly-connected-components (Tarjan).
//!
//! Components are returned in reverse-topological order: a component appears
//! before any component that depends on it. A single node with no self-edge is
//! its own singleton component.

type State = .{
  next_index: Int,
  indices: Dict<String, Int>,
  lowlinks: Dict<String, Int>,
  on_stack: Dict<String, Bool>,
  stack: Vector<String>,
  components: Vector<Vector<String>>,
}

fn dict_int(d: Dict<String, Int>, key: String, msg: String) Int {
  case d[key] {
    .Some(v) => v,
    .None => error("graph_scc: ${msg}: missing '${key}'"),
  }
}

fn visit(st: State, node: String, edges: Dict<String, Vector<String>>) State {
  cur := st
  idx := cur.next_index
  cur.indices[node] = idx
  cur.lowlinks[node] = idx
  cur.next_index = idx + 1
  cur.stack = .append(node)
  cur.on_stack[node] = true

  deps := case edges[node] {
    .Some(ds) => ds,
    .None => [],
  }

  for dep in deps {
    visited := cur.indices.has(dep)

    if !visited {
      cur = visit(cur, dep, edges)
      dep_low := dict_int(cur.lowlinks, dep, "lowlink")
      self_low := dict_int(cur.lowlinks, node, "lowlink")
      if dep_low < self_low {
        cur.lowlinks[node] = dep_low
      }
    } else {
      is_on := case cur.on_stack[dep] {
        .Some(b) => b,
        .None => false,
      }
      if is_on {
        dep_idx := dict_int(cur.indices, dep, "index")
        self_low := dict_int(cur.lowlinks, node, "lowlink")
        if dep_idx < self_low {
          cur.lowlinks[node] = dep_idx
        }
      }
    }
  }

  name_low := dict_int(cur.lowlinks, node, "lowlink")
  name_idx := dict_int(cur.indices, node, "index")

  if name_low == name_idx {
    component: Vector<String> = []
    popping := true
    for popping {
      top := case cur.stack[cur.stack.len() - 1] {
        .Some(t) => t,
        .None => error("graph_scc: empty stack"),
      }
      cur.stack = .drop_last()
      cur.on_stack[top] = false
      component = .append(top)
      if top == node {
        popping = false
      }
    }
    cur.components = .append(component)
  }

  cur
}

pub fn strongly_connected(
  nodes: Vector<String>,
  edges: Dict<String, Vector<String>>,
) Vector<Vector<String>> {
  st := State.{
    next_index: 0,
    indices: Dict.new(),
    lowlinks: Dict.new(),
    on_stack: Dict.new(),
    stack: [],
    components: [],
  }

  for node in nodes {
    if !st.indices.has(node) {
      st = visit(st, node, edges)
    }
  }

  st.components
}
```

(Verify `Vector.drop_last` and `Vector` literal syntax against an existing suite;
adjust `error`/`assert` imports to match repo conventions.)

- [ ] **Step 4: Run to verify it passes**

Run `make boot-test`. Expected: the three `graph_scc` tests PASS.

- [ ] **Step 5: Route `type_order.tw` through the new util**

Replace `codegen/type_order.tw`'s internal `tarjan_visit` usage with a call to
`graph_scc.strongly_connected`, building the `edges` dict from `type_def_deps`.
Keep `type_def_has_self_edge` handling. Run `make boot-test` — the existing
type-order/codegen tests must still pass (this is behavior-preserving).

- [ ] **Step 6: Format, self-host, commit**

```bash
target/twk fmt boot/compiler/graph_scc.tw boot/compiler/codegen/type_order.tw boot/tests/suites/graph_scc_suite.tw
make stage2 && make boot-test
git add boot/compiler/graph_scc.tw boot/compiler/codegen/type_order.tw boot/tests/suites/graph_scc_suite.tw boot/tests/main.tw
git commit -m "compiler: extract reusable string-graph Tarjan SCC util

Pull the Tarjan in type_order.tw into graph_scc.strongly_connected so the
upcoming module grouper and the type-order pass share one implementation.
Behavior-preserving for type ordering."
```

---

## Task 2: Expose resolver passes + thread a TypeId cursor

Make `collect_declarations`, `resolve_references`, and `detect_circular_aliases`
separately callable, and let collection start TypeId allocation at a caller-given
cursor so a group can keep IDs globally unique. `resolve()` stays unchanged for
the singleton path.

**Files:**
- Modify: `boot/compiler/resolver.tw`
- Test: `boot/tests/suites/resolver_passes_suite.tw` (create) + register in `boot/tests/main.tw`

- [ ] **Step 1: Write the failing test**

```tw
// Two parsed modules; collect declarations for both against a shared cursor and
// assert their TypeIds do not overlap, then references resolve cross-module.
.test(
  "group declaration collection allocates disjoint TypeIds",
  fn() {
    // Build env + two single-type modules M1 (type A) and M2 (type B).
    // (Use the suite's existing parse helper; see resolver-related suites.)
    base := resolver.builtin_env_for_test()   // or the suite's base-env helper
    c1 := resolver.collect_declarations_from(base, mod_a, base.next_type_id())
    c2 := resolver.collect_declarations_from(base, mod_b, c1.next_id)
    try assert.is_true(c1.next_id <= c2.start_id)
    try assert.is_true(c2.start_id > c1.start_id)  // disjoint ranges
    .Ok({})
  },
)
```

(Adapt to whatever base-env + parse helpers the resolver suites already use; the
assertion that matters is **disjoint TypeId ranges across members**.)

- [ ] **Step 2: Run to verify it fails**

`make boot-test` → FAIL (`collect_declarations_from`/`next_type_id` undefined).

- [ ] **Step 3: Add `pub` pass entries with a cursor**

In `resolver.tw`:

1. Add `pub fn next_type_id(env: ResolvedEnv) Int { env.next_available_type_id() }`
   (thin public wrapper; keep `next_available_type_id` private).
2. Add a cursor-aware collection entry that resolves the "where do new IDs start"
   from a parameter instead of always `next_available_type_id(env)`:

```tw
pub type DeclCollection = .{
  result: ResolveResult,   // env with new decls (def == .None) + diags
  start_id: Int,           // first TypeId this module allocated
  next_id: Int,            // first free TypeId after this module
}

/// Collect declarations starting TypeId allocation at `id_start`, so a group
/// can thread the cursor across members and keep IDs globally unique.
pub fn collect_declarations_from(
  env: ResolvedEnv,
  module: Module,
  id_start: Int,
) DeclCollection {
  // Same body as collect_declarations, but every `cur.next_available_type_id()`
  // is replaced by a threaded local cursor seeded from `id_start`.
  ...
}
```

Refactor the existing private `collect_declarations(env, module)` to delegate:
```tw
fn collect_declarations(env: ResolvedEnv, module: Module) ResolveResult {
  collect_declarations_from(env, module, env.next_available_type_id()).result
}
```

3. Add `pub fn resolve_references_pub(env, module, diags)` and
   `pub fn detect_circular_aliases_pub(env, diags)` thin wrappers over the
   existing private fns (or just make the existing ones `pub`).

Leave `resolve()` (the welded singleton path) exactly as-is.

- [ ] **Step 4: Run to verify it passes**

`make boot-test` → the new disjoint-range test PASSES; all existing resolver +
multi-module tests still PASS (singleton `resolve()` unchanged).

- [ ] **Step 5: Format, self-host, commit**

```bash
target/twk fmt boot/compiler/resolver.tw boot/tests/suites/resolver_passes_suite.tw
make stage2 && make boot-test
git add boot/compiler/resolver.tw boot/tests/suites/resolver_passes_suite.tw boot/tests/main.tw
git commit -m "resolver: expose passes + cursor-threaded declaration collection

Make collect_declarations/resolve_references/detect_circular_aliases callable by
the upcoming group path, and add collect_declarations_from(env, module, id_start)
so a group can thread a shared TypeId cursor and keep IDs disjoint across
members. resolve() (singleton path) delegates unchanged."
```

---

## Task 3: Range-aware local-type capture on publish

Teach `capture_local_types`/`publish_interface` to capture a member's local types
by an explicit `[start, end)` range instead of a single tail offset, so group
members' interleaved types are attributed correctly. Singleton callers pass
`end = env.types.len()` for identical behavior.

**Files:**
- Modify: `boot/compiler/query/analyze.tw`
- Test: covered by the full suite staying green (behavior-identical for singletons); explicit group coverage arrives in Task 9.

- [ ] **Step 1: Add a range-aware capture**

In `analyze.tw`, change `capture_local_types(state, env, canonical, local_type_start)`
to `capture_local_types_range(state, env, canonical, local_type_start, local_type_end)`,
replacing the `for i in local_type_start..env.types.len()` loop bound with
`local_type_end`. Keep a thin wrapper:

```tw
fn capture_local_types(state: AnalysisState, env: ResolvedEnv, canonical: String, local_type_start: Int) AnalysisState {
  state.capture_local_types_range(env, canonical, local_type_start, env.types.len())
}
```

- [ ] **Step 2: Thread an optional range through `publish_interface`**

Add an internal `publish_interface_range(..., local_type_start, local_type_end)`
that calls `capture_local_types_range`; keep `publish_interface(...)` delegating
with `local_type_end = checked.env.types.len()`. The singleton call site at
`analyze.tw:566` stays on `publish_interface` (unchanged behavior).

- [ ] **Step 3: Run to verify no regression**

`make boot-test` → all green (singletons unchanged). `make stage2` → fixed point holds.

- [ ] **Step 4: Format, commit**

```bash
target/twk fmt boot/compiler/query/analyze.tw
git add boot/compiler/query/analyze.tw
git commit -m "analyze: range-aware local-type capture on publish

capture_local_types_range/publish_interface_range take an explicit [start,end)
TypeId range so interleaved group-member types are attributed to the right
module. Singleton callers pass end = env.types.len() for identical behavior."
```

---

## Task 4: Phase 1 discovery pass (env-independent)

Add a function that walks the import closure doing only load/parse/plan, recording
the dependency adjacency and the discovery order. No `ResolvedEnv` involved.

**Files:**
- Modify: `boot/compiler/query/analyze.tw` (add discovery; reuse `load_source`/`parse_cached`/`plan_dependencies`)
- Test: `boot/tests/suites/discovery_suite.tw` (create) + register

- [ ] **Step 1: Define the discovery result type and entry**

```tw
pub type Discovery = .{
  state: AnalysisState,                 // cache now holds parsed + dep plans
  order: Vector<String>,                // first-reach (discovery) order
  edges: Dict<String, Vector<String>>,  // canonical -> dep canonical paths
  failed: Dict<String, Bool>,           // modules that failed load/parse/plan
  diagnostics: Vector<AnalysisDiag>,
}

/// Phase 1: build the module dependency graph for the closure rooted at `id`.
/// Only loads/parses/plans — no resolution, no env.
pub fn discover_closure(
  id: identity.SourceIdentity,
  state: AnalysisState,
) Discovery { ... }
```

Implementation: an explicit work-stack DFS over canonical paths. For each unseen
module: `load_source` → `parse_cached` → `plan_dependencies` (the existing
helpers, which take no env). Record `edges[canonical] = [dep.canonical_path ...]`,
append to `order` on first reach, push unseen deps. On any load/parse/plan error,
record the diagnostic, set `failed[canonical] = true`, and do not descend.

- [ ] **Step 2: Write the failing test**

```tw
.test(
  "discovery records a three-module cycle's edges and order",
  fn() {
    dir := fixtures_dir()
    st := analyze.new_state(...)   // mirror compile_entry's state setup
    d := analyze.discover_closure(identity.from_path("${dir}/cycle_type_a.tw"), st)
    try assert.is_true(d.edges.has("${dir}/cycle_type_a.tw"))
    try assert.is_true(d.order.len() >= 2)
    try assert.is_true(d.diagnostics.len() == 0)
    .Ok({})
  },
)
```

- [ ] **Step 3: Run to verify it fails, then passes**

`make boot-test` → FAIL (`discover_closure` undefined) → implement → PASS.

- [ ] **Step 4: Format, self-host, commit**

```bash
target/twk fmt boot/compiler/query/analyze.tw boot/tests/suites/discovery_suite.tw
make stage2 && make boot-test
git add boot/compiler/query/analyze.tw boot/tests/suites/discovery_suite.tw boot/tests/main.tw
git commit -m "analyze: add env-independent Phase 1 discovery pass

discover_closure walks the import closure doing only load/parse/plan, recording
the dependency adjacency graph and first-reach order. No resolution, no env —
the input to Tarjan condensation. Not yet wired into compile_entry."
```

---

## Task 5: Extract the per-module import-env builder

`analyze_dependencies` builds each module's env by folding its already-published
dependency interfaces (`merge_import_interface`) plus shared types. Factor the
"build env for module M from already-published interfaces" out so Phase 2 can
reuse it for both singletons and group members. Behavior-identical refactor.

**Files:**
- Modify: `boot/compiler/query/analyze.tw`

- [ ] **Step 1: Add the helper**

```tw
/// Build the resolution env for `canonical` from the base env + shared types +
/// the already-published interfaces of the deps in `dep_plan`. Returns the env
/// and `ok=false` if any required dependency interface is missing.
fn build_import_env(
  base: ResolvedEnv,
  state: AnalysisState,
  canonical: String,
  dep_plan: imports.DependencyPlan,
) .{ env: ResolvedEnv, ok: Bool } { ... }
```

Move the env-construction logic from `analyze_dependencies`' per-dep loop
(`extend_env_from_shared` + `extend_new_shared_types_from` + `merge_import_interface`
keyed off `state.interfaces[dep.canonical_path]`) into this helper. Have
`analyze_dependencies` call it (it still also recurses to analyze each dep first).

- [ ] **Step 2: Run to verify no regression**

`make boot-test` → all green. `make stage2` → fixed point holds. (Pure refactor —
the acyclic byte-identical bar is the test.)

- [ ] **Step 3: Format, commit**

```bash
target/twk fmt boot/compiler/query/analyze.tw
git add boot/compiler/query/analyze.tw
git commit -m "analyze: extract build_import_env helper

Factor per-module import-env construction (shared types + selectively merged
published dep interfaces) out of analyze_dependencies so Phase 2 SCC resolution
can reuse it for singletons and group members. Behavior-identical."
```

---

## Task 6: Group resolution (steps A–D) for a multi-module SCC

Add a function that resolves and typechecks a multi-module SCC. Tested in
isolation by feeding it a group derived from discovery, before the driver swap.

**Files:**
- Modify: `boot/compiler/query/analyze.tw`
- Test: `boot/tests/suites/group_resolution_suite.tw` (create) + register

- [ ] **Step 1: Define the entry**

```tw
/// Resolve + typecheck every member of a multi-module SCC together.
/// Precondition: all deps OUTSIDE `members` are already published in `state`.
/// Returns updated state with each member's interface published (module_order
/// extended in `members` order) or a failure with diagnostics.
fn resolve_group(
  base: ResolvedEnv,
  state: AnalysisState,
  members: Vector<String>,         // canonical paths, discovery order
  plans: Dict<String, imports.DependencyPlan>,
  parsed: Dict<String, cache.ParsedModule>,
) GroupResult { ... }
```

- [ ] **Step 2: Implement steps A–D**

- **Value-init guard (pre-A):** if any member has `resolver.has_top_level_statements(parsed[m].module)`,
  return failure with one diagnostic naming **all** members sorted by canonical
  path: `"Top-level initialization cycle: modules <m1>, <m2>, ... form an import cycle and have top-level statements"`.
- **A — collect declarations, all members.** Thread an id cursor seeded from
  `base.next_available_type_id()`. For each member, build its *external-only*
  env via `build_import_env` over the deps **not** in `members`, then
  `resolver.collect_declarations_from(ext_env, parsed[m].module, cursor)`; record
  each member's `DeclCollection` (`start_id`/`next_id`) and advance the cursor.
  Publish each member's **declaration-only interface** into `state.interfaces`
  (reuse the generalized `preliminary_type_*` construction) so step B can merge
  siblings selectively.
- **B — resolve references, all members.** For each member, build its env via
  `build_import_env` over **all** its deps (siblings now have declaration
  interfaces published), then `resolver.resolve_references_pub(env, parsed[m].module, c.result.diagnostics)`.
  Keep each member's resolved env.
- **C — circular-alias check, once.** Run `resolver.detect_circular_aliases_pub`
  over a combined view of the members' resolved type space; fold any diagnostics in.
- **D — typecheck + publish, per member.** For each member: `checker.check(parsed[m].module, member_env, state.lint_mode)`,
  write the resolved + typed artifacts into the cache under the member's runner
  key (so lowering finds them — mirror `resolve_and_check_local`'s
  `put_resolved`/`put_typed` via the member's `stage_runner.Runner`), then
  `publish_interface_range(..., start_id, next_id)` (range-aware capture from
  Task 3). Append each member to `module_order` in `members` order.

If any step produces error diagnostics, mark all members failed and return the
diagnostics.

- [ ] **Step 3: Write the failing test**

```tw
.test(
  "resolve_group compiles mutually-recursive types across two files",
  fn() {
    dir := fixtures_dir()
    st := analyze.new_state(...)
    d := analyze.discover_closure(identity.from_path("${dir}/cycle_type_a.tw"), st)
    comps := graph_scc.strongly_connected(d.order, d.edges)
    group := find_multi_member(comps)            // the size>1 component
    res := analyze.resolve_group_for_test(d.state, group)  // test shim that loads plans/parsed from cache
    try assert.is_true(res.ok)
    .Ok({})
  },
)
```

Add a small `pub fn resolve_group_for_test(...)` shim if needed to assemble
`plans`/`parsed` from the discovery cache for the test.

- [ ] **Step 4: Run fail → implement → pass**

`make boot-test` → the group-resolution test PASSES.

- [ ] **Step 5: Format, self-host, commit**

```bash
target/twk fmt boot/compiler/query/analyze.tw boot/tests/suites/group_resolution_suite.tw
make stage2 && make boot-test
git add boot/compiler/query/analyze.tw boot/tests/suites/group_resolution_suite.tw boot/tests/main.tw
git commit -m "analyze: add group resolution (steps A-D) for multi-module SCCs

Collect declarations for all members against a shared TypeId cursor (per-member
scoped envs, no flat-env false collisions), resolve references with siblings
merged selectively, run the circular-alias check once over the group, then
typecheck + publish each member with range-aware capture. Rejects value-init
cycles up front, naming all members. Not yet wired into the driver."
```

---

## Task 7: Swap the driver to two-pass (Phase 1 → condense → Phase 2)

Replace `analyze_module_impl`'s recursive resolution with: discover → Tarjan →
resolve each SCC in order (singleton via existing path, group via `resolve_group`).
This is the high-risk integration. Acceptance gate: full suite + self-host.

**Files:**
- Modify: `boot/compiler/query/analyze.tw` (`analyze_module` becomes the two-pass driver)

- [ ] **Step 1: Implement the two-pass `analyze_module`**

```tw
pub fn analyze_module(id, alias, base, state, stack) ModuleResult {
  // Phase 1
  disc := discover_closure(id, state)
  if disc.diagnostics has errors -> return failure(disc)

  // Condense
  comps := graph_scc.strongly_connected(disc.order, disc.edges)

  // Phase 2: resolve each SCC in dependency-before-dependent order
  cur := disc.state
  env := base
  for comp in comps {
    if comp.len() == 1 and !self_loop(comp[0], disc.edges) {
      // singleton: existing per-module path
      env, cur = resolve_singleton(env, cur, comp[0], disc)   // build_import_env + resolve_and_check_local + publish_interface
    } else {
      // group: steps A-D
      gres := resolve_group(env, cur, comp, plans_from(disc), parsed_from(disc))
      env, cur = (gres.env, gres.state)
      if !gres.ok -> accumulate diagnostics / mark failed
    }
  }
  // module_order is now the flattened SCC order (singletons + group members)
  ...
}
```

`resolve_singleton` reuses `build_import_env` (Task 5) +
`resolve_and_check_local` + `publish_interface` exactly as the old per-module
tail did, so singleton output is byte-identical. Preserve `entry_snapshot`
capture (now after the closure resolves), progress events, timings, and
`mark_failed` short-circuiting for downstream-of-failure modules.

- [ ] **Step 2: Keep the old recursive helpers temporarily**

Leave `analyze_module_impl`, `analyze_dependencies`, `break_import_cycle`, and the
`preliminary_type_*`/back-edge code in place but unreferenced for now (deleted in
Task 8). This keeps the diff reviewable and lets you bisect if the swap regresses.

- [ ] **Step 3: Full validation (the real gate)**

```bash
make boot-test    # entire suite, incl. existing cycle fixtures NOW via the SCC path
make stage2       # self-host fixed point — acyclic byte-identical bar
```

Expected: all boot tests pass (the four existing cycle tests in
`multi_module_suite` now succeed through singleton/group SCC paths), and the
self-host loop reaches its fixed point. If a singleton regresses, diff its
published interface against the pre-swap behavior; the env-threading order is the
usual culprit.

- [ ] **Step 4: Format, commit**

```bash
target/twk fmt boot/compiler/query/analyze.tw
git add boot/compiler/query/analyze.tw
git commit -m "analyze: switch frontend to two-pass SCC driver

analyze_module now discovers the closure (Phase 1), condenses it with Tarjan,
and resolves each SCC in dependency-before-dependent order: singletons via the
existing per-module path (byte-identical), multi-module groups via resolve_group.
Old back-edge recursion left in place, unreferenced, for one more commit."
```

---

## Task 8: Delete the back-edge mechanism

Remove the now-dead back-edge discovery code. The declaration-only interface
construction stays (generalized into `resolve_group`).

**Files:**
- Modify: `boot/compiler/query/analyze.tw`

- [ ] **Step 1: Delete**

Remove `analyze_module_impl`, `analyze_dependencies` (if fully superseded by
`resolve_singleton` + `build_import_env`), `break_import_cycle`,
`next_preliminary_type_id`, the opaque-nominal-stub injection, the `stack.contains`
back-edge branch, and the "merge preliminary interface back" block (~534–543).
Keep `preliminary_type_exports`/`preliminary_type_interface` **only if**
`resolve_group` calls them for its step-A declaration interface; otherwise move
that construction into `resolve_group` and delete the originals. Remove the now
unused `stack` parameter plumbing if nothing else needs it.

- [ ] **Step 2: Full validation**

```bash
make boot-test
make stage2
target/twk lint boot/main.tw   # no new warnings (e.g. unused fns)
```

Expected: all green; no unused-function lint hits from leftovers.

- [ ] **Step 3: Format, commit**

```bash
target/twk fmt boot/compiler/query/analyze.tw
git add boot/compiler/query/analyze.tw
git commit -m "analyze: remove back-edge cycle mechanism

Cycles are known from Tarjan before resolution, so the reactive back-edge path
(break_import_cycle, next_preliminary_type_id, opaque stubs, the merge-preliminary
block) is dead and removed. The declaration-only interface construction lives on
inside resolve_group."
```

---

## Task 9: New fixtures + tests (group correctness)

Add coverage the back-edge path never had: a 3-module cycle, mutual types that
share a local type name (proves no flat-env false collision), and a value-init
diagnostic that names all participants.

**Files:**
- Create: `boot/tests/fixtures/multi/cycle3_a.tw`, `cycle3_b.tw`, `cycle3_c.tw`
- Create: `boot/tests/fixtures/multi/cycle_shared_name_a.tw`, `cycle_shared_name_b.tw`
- Modify: `boot/tests/suites/multi_module_suite.tw`

- [ ] **Step 1: 3-module cycle fixtures**

`cycle3_a.tw` imports `cycle3_b`, `b` imports `c`, `c` imports `a`; each defines a
type/function the next uses, forming A→B→C→A with no top-level statements.

- [ ] **Step 2: shared-local-name fixtures**

Both files define a distinct local `type Entry = .{ ... }` and import the other's
public type under an alias, forming a type cycle. This must compile — a flat env
would have raised `DuplicateName` on `Entry`.

- [ ] **Step 3: Add tests**

```tw
.test(
  "three-module cycle a->b->c->a compiles",
  fn() {
    result := module_compiler.compile_entry("${fixtures_dir()}/cycle3_a.tw")
    case result {
      .Ok(_) => .Ok({}),
      .Err(err) => .Err("expected 3-module cycle to compile, got: ${format_compile_error(err)}"),
    }
  },
)
.test(
  "mutually-recursive modules may reuse a local type name",
  fn() {
    result := module_compiler.compile_entry("${fixtures_dir()}/cycle_shared_name_a.tw")
    case result {
      .Ok(_) => .Ok({}),
      .Err(err) => .Err("expected shared local name across cycle to compile, got: ${format_compile_error(err)}"),
    }
  },
)
.test(
  "value-init cycle diagnostic names all participating modules",
  fn() {
    result := module_compiler.compile_entry("${fixtures_dir()}/cycle_value_a.tw")
    case result {
      .Ok(_) => .Err("expected value-init cycle rejection"),
      .Err(err) => {
        msg := format_compile_error(err)
        try assert.is_true(msg.contains("initialization cycle"))
        try assert.is_true(msg.contains("cycle_value_a"))
        try assert.is_true(msg.contains("cycle_value_b"))
        .Ok({})
      },
    }
  },
)
```

- [ ] **Step 4: Run fail → implement fixtures → pass**

`make boot-test` → the three new tests PASS; the four existing cycle tests still PASS.

- [ ] **Step 5: Format, self-host, commit**

```bash
target/twk fmt boot/tests/fixtures/multi/cycle3_a.tw boot/tests/fixtures/multi/cycle3_b.tw boot/tests/fixtures/multi/cycle3_c.tw boot/tests/fixtures/multi/cycle_shared_name_a.tw boot/tests/fixtures/multi/cycle_shared_name_b.tw boot/tests/suites/multi_module_suite.tw
make stage2 && make boot-test
git add boot/tests/fixtures/multi/cycle3_a.tw boot/tests/fixtures/multi/cycle3_b.tw boot/tests/fixtures/multi/cycle3_c.tw boot/tests/fixtures/multi/cycle_shared_name_a.tw boot/tests/fixtures/multi/cycle_shared_name_b.tw boot/tests/suites/multi_module_suite.tw
git commit -m "tests: cover 3-module cycle, shared local names, whole-cycle value-init diag"
```

---

## Task 10: Fold group siblings into cache hashes

Multi-module group members must invalidate together: editing any member
re-resolves the whole group. Fold group sibling source hashes into each member's
`deps_hash`/`context_hash`.

**Files:**
- Modify: `boot/compiler/query/analyze.tw` (where `resolve_group` builds each member's `stage_runner.Runner`)
- Test: `boot/tests/suites/multi_module_suite.tw` (incremental re-resolve)

- [ ] **Step 1: Fold siblings into the hash**

In `resolve_group`, when constructing each member's runner, mix the other
members' source hashes into the member's `deps_hash` (or `context_hash`) — e.g.
fold each sibling's `keys.hash_text(source)` via `keys.mix_word`. This mirrors the
landed back-edge note that "preliminary resolve hashes include in-cycle siblings."

- [ ] **Step 2: Incremental test**

Add a test that compiles a cycle fixture, then re-compiles with one member's
overlay source changed, and asserts the result reflects the change (e.g. a
renamed exported symbol now resolves / a removed one now errors). Use the overlay
mechanism the LSP/query suites already use for in-memory edits.

- [ ] **Step 3: Run fail → implement → pass; self-host; commit**

```bash
make boot-test && make stage2
target/twk fmt boot/compiler/query/analyze.tw boot/tests/suites/multi_module_suite.tw
git add boot/compiler/query/analyze.tw boot/tests/suites/multi_module_suite.tw
git commit -m "analyze: fold group siblings into member cache hashes

Each multi-module SCC member mixes its siblings' source hashes into its
deps_hash, so editing any member invalidates and re-resolves the whole group."
```

---

## Task 11: Final integration — bundle CLI, docs, memory

**Files:**
- Modify: `docs/plans/README.md`, `docs/plans/recursive-module-groups.md`
- Build: `make bundle-cli`

- [ ] **Step 1: Rebuild the CLI and run the full gate**

```bash
make bundle-cli      # self-host loop + standalone twk
make boot-test
```

Expected: bundle succeeds, full suite green.

- [ ] **Step 2: Update docs**

- In `docs/plans/recursive-module-groups.md`, flip the status to note the SCC
  approach has replaced the back-edge mechanism in boot (link the design doc).
- In `docs/plans/scc-module-groups-design.md`, set status to "implemented (boot);
  stage0 mirror pending."
- Per house rule, when fully done remove the plan's row from `docs/plans/README.md`
  and move completed plan docs to `docs/plans/archive/` — but keep the design +
  this plan active until the stage0 mirror is also scheduled/decided.

- [ ] **Step 3: Commit**

```bash
git add docs/plans/README.md docs/plans/recursive-module-groups.md docs/plans/scc-module-groups-design.md
git commit -m "docs: SCC module groups landed in boot compiler

Tarjan SCC grouping replaces the back-edge preliminary-interface mechanism.
stage0 mirror remains a follow-up."
```

- [ ] **Step 4: Update session memory**

Update the In-Progress memory entry for recursive module groups to record: SCC
two-pass driver landed in boot (graph_scc.tw + Phase 1 discovery + group
resolution steps A–D + range-aware capture + sibling-folded cache hashes);
back-edge mechanism removed; stage0 mirror still pending.

---

## stage0 follow-up (separate plan, only if boot proves out)

Mirror Phase 1 discovery + Tarjan condensation + Phase 2 group resolution (steps
A–D) in `src/module/` (`planner.rs`, `compile_module`,
`compile_planned_dependencies`) and `src/types/` resolver entries, and update the
planner tests that assert acyclic-only behavior. Write this as its own plan once
the boot implementation is validated on the self-host loop and in real use.

---

## Self-review notes

- **Spec coverage:** Phase 1 discovery (T4), Tarjan condense (T1), singleton path
  (T7 `resolve_singleton`), group steps A–D (T6), per-member scoped envs + id
  cursor (T2/T6), range-aware capture (T3), value-init rejection moved + named
  (T6/T9), `topo_sort` left intra-module (no task needed — explicitly unchanged),
  Pass-3 once over group (T6 step C), removals (T8), caching fold (T10), prelude
  unchanged (no task — falls out of singleton/group paths), validation gates
  (every task: `make boot-test` + `make stage2`). stage0 explicitly deferred.
- **Naming consistency:** `collect_declarations_from`, `resolve_references_pub`,
  `detect_circular_aliases_pub`, `next_type_id`, `DeclCollection`,
  `capture_local_types_range`, `publish_interface_range`, `build_import_env`,
  `discover_closure`, `Discovery`, `resolve_group`, `GroupResult`,
  `graph_scc.strongly_connected` are used consistently across tasks.
- **Known soft spots (validate during execution, not placeholders):** exact
  `use`-prefix style and test/base-env helpers in suites (check a sibling suite);
  the precise cache `put_resolved`/`put_typed` calls in T6 step D (mirror
  `resolve_and_check_local` + `stage_runner`); and whether `analyze_dependencies`
  is fully superseded in T8 or partly retained. These are integration details to
  confirm against the code while executing, anchored by the suite + self-host gate.
```
