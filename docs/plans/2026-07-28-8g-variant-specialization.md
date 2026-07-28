# Phase 8G — Ownership-Specialized Function Variants Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `compute_variants`' published ownership variants into real, emitted functions: physically clone a function's ANF body under a proven owned precondition, route callers whose argument is proven unique+last-use to the clone, and let the existing 8A–8F in-place emission fire inside the clone — while the generic body stays the persistent fallback.

**Architecture:** A new ANF→ANF pass (`codegen/variant_specialize.tw`) runs in `link_program` **between the builder-region rewrite and closure conversion**. It runs the existing `compute_variants` analysis on ANF′, deep-copies each **published, reachable** `VariantId`'s function body to a fresh `FuncId` (retargeting in-SCC recursive calls), retargets caller `ACall` sites whose argument uniqueness satisfies the variant, and records a per-clone **owned entry-seed** plus its source `VariantId`. `produce_mutable_decisions` then runs **once over the whole ANF″ module** (clones included) with those owned seeds applied to the clone func ids; the clone's owned params seed `Unique`, so the ordinary 8A–8F producer emits `Site`-keyed in-place decisions inside the clone (disjoint key space, distinct `func_id`) that flow through the **unchanged** selector/emit path. A clone that yields zero in-place decisions is pruned and its routes re-pointed to the generic. Correctness rests on the routing gate reusing the analysis's real caller-side uniqueness + last-use proof, never a heuristic.

**Tech Stack:** Twinkle boot compiler (`boot/`), self-hosted. Build via `make bundle-cli`; boot tests via `target/twk run boot/tests/main.tw`. No Rust stage0 changes (boot-codegen optimization; stage0 keeps emitting the persistent path — the no-stage0-parity rule for backend-only optimizations applies).

**Design source:** This session's physical-clone-vs-virtual spike and two review rounds, including empirical `compute_variants` probing (recorded below), plus `docs/plans/sound-uniqueness/codegen/README.md` §"Codegen Phase 8G" and `docs/plans/sound-uniqueness/codegen/handoff-contract.md` §"Variant routing records".

---

## What actually publishes a variant (empirically verified — read this first)

A throwaway probe ran `summary.compute_variants(view, b, sem, table)` on four fixtures and dumped `vt.by_func`. Results (canonical keys like `f295|0:` = func 295, param 0, shell path):

| Fixture shape | Example | `vt.by_func` |
|---|---|---|
| Prelude wrapper call | `flags = .set_at(0, false)` (`vector_once`) | **empty** — no variant |
| User **vector** fn, **non-recursive** | `fn bump(xs: Vector<Int>, i) { xs[i]=0; xs }` | **empty** — no variant |
| User **record** fn, non-recursive | `fn setb(s: S, v) { s.b=v; s }` | **`f295|0:`** — publishes |
| User **vector** fn, **recursive** | `fn visit(seen: Vector<Int>, n) { seen[0]=n; …visit(seen,n-1) }` | **`f295|0:`** — publishes |

Consequences that shape this plan:

1. **`set_at`/`vector_once` is NOT a valid anchor.** `compute_variants` processes only **user** functions (`user_id_set`), so prelude helpers never publish. The earlier draft's `set_at` fixtures are removed.
2. **The non-recursive win is record-shell reuse.** `setb` publishes and, under an owned seed, `s.b=v` lowers to record-shell `struct.set` reuse (the **live 8F** path). This is 8G.2's anchor.
3. **The vector win requires the recursive shape.** `visit` publishes and, under an owned seed, `seen[0]=n` lowers to `vector$set_in_place` (the **live 8A/8B** path). This is 8G.3's anchor — and it is exactly the Case V idiom.
4. **Out of 8G scope (do not attempt here):** non-recursive user vector wrappers (`bump`) and prelude wrappers (`set_at`) do not publish a variant today. Making them publish is an *analysis-track* (Phase 6 candidate-publishing) change, not codegen. Collections behind a **record field** (`s.xs[i]=v`, rendered `field=persistent(insufficient deep ownership)`) are **Phase 8H**. Record both as follow-ups; assert nothing about them here.

Because 8G's emit wins reuse **already-live** 8F (record shell) and 8A/8B (vector index) machinery, the pass adds cloning + routing + seeding only — no new emit family.

---

## Design rationale (physical clone, verified against code)

Downstream keying is `Dict<Int,...>` by func-id, never a fixed-size vector: `emit_module` builds `func_sym_map`/`prepared_funcs` as `Dict<Int,_>` keyed by `func.func_id.id` (`emit.tw:107–122`); symbols are `$f{id}_{name}` resolved by the linker; `BuiltinRegistry.by_id` is a `Dict` and user funcs are absent (`USER_FUNC_START = 41`). A fresh `FuncId` at `max(existing)+1` flows through the whole backend as an ordinary function and needs no builtins registration. Virtual is strictly worse: emission is one `PreparedFunc` → one wasm func per `func_id`, so an owned variant needs a second func identity regardless; sharing one `func_id` collides generic+variant decisions in `by_site: Dict<Int,...>` keyed by `site_key(func_id, local)` and pushes variant selection into emit, violating the mechanical-emit invariant. Monomorphization already ran, so the clone body is a pure structural copy (only self/peer recursive `AGlobalFunc` targets get retargeted).

**Soundness obligation:** a clone emits in-place mutation, observably identical to the generic **only if** the caller passed a unique, last-use value. Routing is load-bearing for soundness. The gate must be the analysis's real proof, never a heuristic. Failure modes are asymmetric: over-conservative routing (stay generic) only costs performance; an unreachable clone is dropped by the existing Wasm DCE.

**Note on `select_variant` vs the variant table.** The `--cfg` line `verdict -> f<id>[unique:pN]` is built by `ownership.select_variant` (a *pure* per-call decision from the callee's generic summary; it "demands no summary"), **not** from `vt.by_func`. Do not use that verdict as evidence a variant is published — only `vt.by_func` membership counts, which is why the probe above is the authority.

---

## Orientation (read before starting)

### Pipeline seam
`boot/compiler/codegen/codegen.tw` — `link_program(anf, env, builtins)`:
1. `anf_prime := builder_region.rewrite_module(anf, builtins)` (line 84)
2. `convert_closures(anf_prime, env)` (93)
3. `mutable_produce.produce_mutable_decisions(anf_prime, builtins)` (105)
4. `prepare_backend_with_mutable_config(closure.anf, env, builtins, captures, table, enabled_emit_policy())` (113)

**8G inserts between (1) and (2):** `spec := variant_specialize.specialize_module(anf_prime, builtins)` → `spec.anf` (ANF″ = clones + routed calls) replaces `anf_prime` into (2) and (3); `spec.seeds` is threaded into (3) via the seeded producer.

### Analysis facts
- `summary.tw:1036` — `pub fn compute_variants(view, b, sem, generic: SummaryTable) VariantSummaryTable`. Processes **user functions only**.
- `VariantSummaryTable` (`summary.tw:508`): `by_key: Dict<String, VariantEntry>`, `by_func: Dict<Int, Vector<String>>` (func_id → canonical keys). `VariantEntry = .{ variant: vid.VariantId, summary: Summary }`. Canonical key string is `f{func}|{param}:{segs}` (e.g. `f295|0:`) — **not** the rendered `unique:pN`.
- **Private today — Task 1 wraps them, does not leak them:** `variant_args_satisfied` (1078), `variant_specificity` (1090), `variant_summary_for` (1102), `unique_seed_for_variant` (1193). `variant_get` (553) is already public.
- `ownership.tw:6689` — `pub fn call_uniques(f, table, b, sem, suppress, unique_seed) Vector<CallUniq>`; `CallUniq = .{ callee: Int, arg_unique: Vector<Bool> }` (**no site key** — Task 2 adds a sited scan).
- `ownership.tw:7955` — `pub fn select_variant(func_id, s: Summary, arg_unique) vid.VariantId` (pure per-call decision; used by `--cfg` render, not the table).
- `ownership_verdicts.tw:535` — `compute_candidate_artifacts(opt, b, sem, candidate_funcs)` builds `entry_seeds := uniform_entry_seeds(...)` then `analyze_selected_with_summaries_and_entry_seeds(...)`. **Owned-seed injection point.**

### Decision production + emission (unchanged by 8G)
- `mutable_produce.tw:462` — `produce_mutable_decisions(opt, b)`. `MutableDecision` keyed by `Site = site_key(func_id, result_local)` (injective; clone sites disjoint). Sets `variant_key: .None` today (369, 666).
- `mutable_select.tw:25` — `MutableDecision.variant_key: own_variant.VariantId?` present. `enabled_emit_policy()` (177) enables vector-set/dict-set/dict-remove/**record-shell**.

### ANF shape
`anf.tw`: `AnfModule = .{ functions: Vector<AnfFunctionDef>, init_func_id, extern_imports, global_monos, lib_exports }`; `AnfFunctionDef = .{ func_id, name, is_init, params, op_result_mono, body, return_ty }`; `AnfExpr = Let(LocalId, AnfOp, AnfExpr) | Atom | Return | Break | Continue`; call op `ACall(Atom, Vector<Atom>)` with target `AGlobalFunc(FuncId)`; **call args are `Atom` (can be literals like `9`/`false`), not `LocalId`**; nested-`AnfExpr` ops: `ALoop`, `AIf`, `AMatch`, `ADefer`. **Routing rewrites `AGlobalFunc` targets only.** Mirror the exhaustive walk in `mutable_produce.tw` (`// If a new AnfOp/AnfExpr variant is added, update this walk`, 152/229). `FuncId = .{ id: Int }`. No allocator in `link_program`: `next := max(f.func_id.id)+1`.

### Inherent-method caveat for fixtures
A function whose **first param is a builtin collection** (`Vector<Int>`) cannot be called via dot sugar (`seen.visit(...)` → "unknown method") — inherent methods resolve only for types defined in the same module. Call such functions directly: `visit(seen, n)`. A function whose first param is a **user record** (`setb(s: S, …)`) *can* use dot sugar: `m.setb(9)`.

### Test suite style (match exactly)
```twinkle
use @std.testing.assert as assert
use @std.testing as runner
use compiler.opt.semantics as semantics    // NOTE: opt.semantics, not compiler.semantics
pub fn suite() runner.Suite {
  runner.suite("variant_specialize")
    .test("desc", fn() Result<Void, String> { try assert.equal(a, b); .Ok({}) })
}
```
Register in `boot/tests/main.tw`: `use .suites.variant_specialize_suite` (~23) and `variant_specialize_suite.suite(),` (~304). Fixtures: `boot/tests/fixtures/cfg/sound_uniqueness/*.tw`, compiled via `pipeline.compile_entry_path(path)`.

### Build / verify loop
- Fast: `target/twk run boot/tests/main.tw` (against current `target/twk`; new-suite tests run only after bundling).
- Bundle: `make quick-bundle-cli` (iterating) / `make bundle-cli` (before byte-identical claims).
- Self-host: `make stage2`, diff stage3 vs stage4 — **sequential, never backgrounded**.
- Inspection: `target/twk ir <entry> --census --sites`; `target/twk wat <entry> --func <name> --calls`.

---

## File Structure

- **Create** `boot/compiler/codegen/variant_route.tw` — LEAF types: `CloneSpec = .{ seed_locals: Dict<Int, Bool>, variant: vid.VariantId }`; `OwnedSeedTable = .{ by_func: Dict<Int, CloneSpec> }` (carries seed **and** source `VariantId`, fixing the `variant_key` gap); `VariantRoute`; `FallbackReason = { AbsentProof, StaleKey, OverCap, Unsupported, Ambiguous, ZeroWin }`.
- **Create** `boot/compiler/codegen/variant_specialize.tw` — `pub fn specialize_module(anf, b) SpecializeResult` (`SpecializeResult = .{ anf, seeds, routes }`), `render_routes`, `variant_cap`.
- **Modify** `boot/compiler/summary.tw` — public façade: `select_variant_for_args`, `seed_for_variant`, `variant_ids_of(vt) Vector<vid.VariantId>` (enumerate published variants — avoids test string-matching).
- **Modify** `boot/compiler/ownership.tw` — `pub fn call_uniques_sited(...) Vector<SitedCallUniq>` with `SitedCallUniq = .{ caller_func: Int, site_local: Int, site_key: Int, callee: Int, args: Vector<Atom>, arg_unique: Vector<Bool> }`.
- **Modify** `boot/compiler/codegen/ownership_verdicts.tw` — `compute_candidate_artifacts_seeded(..., owned_seeds)`.
- **Modify** `boot/compiler/codegen/mutable_produce.tw` — `produce_mutable_decisions_seeded(opt, b, owned_seeds)`; set `variant_key` from `CloneSpec.variant`.
- **Modify** `boot/compiler/codegen/codegen.tw` — insert `specialize_module` behind `variant_specialize_enabled()`.
- **Modify** `boot/commands/ir.tw` — render `render_routes`.
- **Create** `boot/tests/suites/variant_specialize_suite.tw`; register in `boot/tests/main.tw`.
- **Create** fixtures `setb.tw`, `setb_aliased.tw`, `visit_rec.tw`, `visit_rec_run.tw` under `boot/tests/fixtures/cfg/sound_uniqueness/`.

---

## Slice map

| Slice | Tasks | Anchor | Outcome |
|---|---|---|---|
| 8G.0 Analysis API surface | 1–2 | — | Public route/seed façade + variant enumeration + sited call-uniqueness. No behavior change. |
| 8G.1 Seed plumbing | 3 | `setb` | Seeding a published variant's owned param adds an in-place decision. |
| 8G.2 Clone + route + wire (record-shell, non-recursive) | 4–6 | `setb` | Cloned `setb` emits `struct.set`; aliased caller stays `struct.new`. |
| 8G.3 Recursive vector routing | 7 | `visit` | Cloned `visit` emits `vector$set_in_place`; self-call routes to clone; run-parity. |
| 8G.4 Inspection, cap, self-host | 8–10 | — | Route dry-runs, capped, `make stage2` fixed point + docs. |

---

## Task 1: Public façade + variant enumeration + suite scaffold (8G.0)

**Files:** Create `boot/compiler/codegen/variant_route.tw`; Create `boot/tests/fixtures/cfg/sound_uniqueness/setb.tw`; Modify `boot/compiler/summary.tw`; Create `boot/tests/suites/variant_specialize_suite.tw`; Modify `boot/tests/main.tw`.

- [ ] **Step 1: Create the `setb` fixture** (empirically publishes `f295|0:`):

```twinkle
// boot/tests/fixtures/cfg/sound_uniqueness/setb.tw
pub type S = .{ a: Int, b: Int }
pub fn setb(s: S, v: Int) S { s.b = v; s }
pub fn caller() Int {
  m := S.{ a: 1, b: 2 }
  m.setb(9).b
}
```

- [ ] **Step 2: Create `variant_route.tw`:**

```twinkle
use compiler.core_ir.{FuncId}
use compiler.variant_id as vid

pub type CloneSpec = .{ seed_locals: Dict<Int, Bool>, variant: vid.VariantId }
pub type OwnedSeedTable = .{ by_func: Dict<Int, CloneSpec> }
pub fn empty_seed_table() OwnedSeedTable { OwnedSeedTable.{ by_func: Dict.new() }}
pub fn set_clone(self: OwnedSeedTable, func_id: Int, spec: CloneSpec) OwnedSeedTable {
  self.by_func[func_id] = spec
  self
}

pub type FallbackReason = { AbsentProof, StaleKey, OverCap, Unsupported, Ambiguous, ZeroWin }
pub type VariantRoute = .{
  generic_func: Int, variant: vid.VariantId, clone_func: Int, clone_name: String,
  route_sites: Vector<Int>, recursive_routes: Vector<Int>,
  fallback_reason: FallbackReason?, proof_debug_id: String,
}
```

- [ ] **Step 3: Register an empty suite** so red runs execute. Create `variant_specialize_suite.tw`:

```twinkle
use @std.testing.assert as assert
use @std.testing as runner
use commands.common.{format_compile_error}
use compiler.cfg
use compiler.opt.semantics as semantics
use compiler.ownership
use compiler.pipeline
use compiler.summary
use compiler.variant_id as vid
use compiler.codegen.ownership_verdicts
use compiler.codegen.variant_route
use lib.module.loader

fn fixtures_dir() String { "${loader.find_project_root("boot")}/tests/fixtures/cfg/sound_uniqueness" }
fn compile_fixture(name: String) Result<pipeline.PipelineArtifacts, String> {
  case pipeline.compile_entry_path("${fixtures_dir()}/${name}.tw") {
    .Ok(a) => .Ok(a),
    .Err(e) => .Err(format_compile_error(e)),
  }
}
fn variants_of(art: pipeline.PipelineArtifacts) summary.VariantSummaryTable {
  b := art.builtins
  view := ownership.prune_dead_merge(cfg.build_view(art.opt, b))
  sem := semantics.make_prelude_optimizer_semantics(b)
  table := summary.compute(view, b, sem)
  summary.compute_variants(view, b, sem, table)
}

pub fn suite() runner.Suite { runner.suite("variant_specialize") }
```
In `boot/tests/main.tw`: add `use .suites.variant_specialize_suite` (~23) and `variant_specialize_suite.suite(),` (~304).

- [ ] **Step 4: Add the failing façade test:**

```twinkle
    .test(
      "setb publishes a variant selectable when the record arg is unique",
      fn() Result<Void, String> {
        art := try compile_fixture("setb")
        vt := variants_of(art)
        ids := summary.variant_ids_of(vt)                 // enumerate published variants (no string match)
        try assert.ok(ids.len() >= 1, "setb must publish at least one variant")
        setb_id := ids[0].func
        v := summary.select_variant_for_args(vt, setb_id, [true, true])
        try assert.ok(v != .None, "unique record arg must select the owned variant")
        .Ok({})
      },
    )
```

- [ ] **Step 5: Run — verify fail.** `make quick-bundle-cli && target/twk run boot/tests/main.tw`. Expected: FAIL (`variant_ids_of`/`select_variant_for_args` undefined).

- [ ] **Step 6: Implement the façade** in `summary.tw` (public wrappers; do not alter the private fns):

```twinkle
pub fn variant_ids_of(vt: VariantSummaryTable) Vector<vid.VariantId> {
  out: Vector<vid.VariantId> = []
  for _fid, keys in vt.by_func {
    for k in keys {
      case vt.by_key.get(k) { .Some(e) => out = .append(e.variant), .None => {} }
    }
  }
  out
}
pub fn select_variant_for_args(vt: VariantSummaryTable, callee_id: Int, arg_unique: Vector<Bool>) vid.VariantId? {
  best: vid.VariantId? = .None
  best_spec := 0 - 1
  case vt.by_func.get(callee_id) {
    .Some(keys) => for k in keys {
      case vt.by_key.get(k) {
        .Some(e) => if variant_args_satisfied(e.variant, arg_unique) {
          sp := variant_specificity(e.variant)
          if sp > best_spec { best_spec = sp; best = .Some(e.variant) }
        },
        .None => {},
      }
    },
    .None => {},
  }
  best
}
pub fn seed_for_variant(f: CfgFunction, v: vid.VariantId) Dict<Int, Bool> { unique_seed_for_variant(f, v) }
```

- [ ] **Step 7: Run — verify pass.** `make quick-bundle-cli && target/twk run boot/tests/main.tw`. Expected: PASS.

- [ ] **Step 8: Commit.**

```bash
git add boot/compiler/codegen/variant_route.tw boot/compiler/summary.tw boot/tests/suites/variant_specialize_suite.tw boot/tests/main.tw boot/tests/fixtures/cfg/sound_uniqueness/setb.tw
git commit -m "codegen(8G): public variant façade + enumeration + suite scaffold"
```

---

## Task 2: Site-keyed call-uniqueness scan (8G.0)

**Files:** Modify `boot/compiler/ownership.tw`; Test in the suite.

- [ ] **Step 1: Add the failing test.** `setb`'s `caller` has a `setb` call whose record base is unique+last-use.

```twinkle
    .test(
      "call_uniques_sited reports setb's unique-base call site",
      fn() Result<Void, String> {
        art := try compile_fixture("setb")
        sited := try sited_calls_in(art, "caller")
        hit := sited.first_where(fn(s) { s.arg_unique.len() > 0 and s.arg_unique[0] })
        case hit {
          .Some(s) => { try assert.ok(s.site_key >= 0, "site_key present"); .Ok({}) },
          .None => .Err("expected a unique-base call site in caller"),
        }
      },
    )
```
Define `sited_calls_in(art, fn_name)` in this step: build `view`/`sem`/`table` as in `variants_of`, find the `CfgFunction` named `fn_name` in `view.functions`, and call `ownership.call_uniques_sited(f, table, b, sem, Dict.new(), Dict.new())`.

- [ ] **Step 2: Run — verify fail.** Expected: FAIL (`call_uniques_sited` undefined).

- [ ] **Step 3: Implement `call_uniques_sited`** in `ownership.tw` beside `call_uniques` (6689), reusing its liveness + per-call `call_arg_unique`, additionally capturing the enclosing `Let` result local and `vid.site_key(f.func_id, result_local)`:

```twinkle
pub type SitedCallUniq = .{
  caller_func: Int, site_local: Int, site_key: Int,
  callee: Int, args: Vector<Atom>, arg_unique: Vector<Bool>,
}
pub fn call_uniques_sited(
  f: CfgFunction, table: SummaryTable, b: BuiltinRegistry, sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>, unique_seed: Dict<Int, Bool>,
) Vector<SitedCallUniq> { /* same walk as call_uniques; push a SitedCallUniq per ACall */ }
```
Leave `call_uniques` unchanged.

- [ ] **Step 4: Run — verify pass.** Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/ownership.tw boot/tests/suites/variant_specialize_suite.tw
git commit -m "codegen(8G): site-keyed call-uniqueness scan (routing proof data)"
```

---

## Task 3: Seeded artifacts + seeded decision production (8G.1)

**Files:** Modify `boot/compiler/codegen/ownership_verdicts.tw`, `boot/compiler/codegen/mutable_produce.tw`; Test in the suite.

- [ ] **Step 1: Add the failing test.** Seeding `setb`'s published-variant owned param `Unique` adds an in-place (record-shell) decision the unseeded run lacks.

```twinkle
    .test(
      "seeding setb's owned param adds a record-shell in-place decision",
      fn() Result<Void, String> {
        art := try compile_fixture("setb")
        vt := variants_of(art)
        variant := summary.variant_ids_of(vt)[0]
        setb_cfg := try cfg_func_of(art, variant.func)          // helper: CfgFunction by func id
        seed := summary.seed_for_variant(setb_cfg, variant)      // NOTE: not dict_true([0]) — param->local mapping is not identity
        spec := variant_route.CloneSpec.{ seed_locals: seed, variant }
        seeds := variant_route.empty_seed_table().set_clone(variant.func, spec)
        generic := mutable_produce.produce_mutable_decisions(art.opt, art.builtins)
        seeded := mutable_produce.produce_mutable_decisions_seeded(art.opt, art.builtins, seeds)
        try assert.ok(decision_count(seeded) > decision_count(generic),
          "owned seed must add a record-shell in-place decision")
        .Ok({})
      },
    )
```
Define `cfg_func_of(art, func_id)` and `decision_count(produced)` (sum `produced.table.by_site` value-vector lengths) in this step. **This seeds the generic `setb` id purely to prove the seed mechanism; Task 4 seeds the clone id.**

- [ ] **Step 2: Run — verify fail.** Expected: FAIL (`produce_mutable_decisions_seeded` undefined).

- [ ] **Step 3: Seeded artifacts.** In `ownership_verdicts.tw`, refactor `compute_candidate_artifacts` into `compute_candidate_artifacts_seeded(opt, b, sem, candidate_funcs, owned_seeds: variant_route.OwnedSeedTable)`. After `entry_seeds := uniform_entry_seeds(...)`:

```twinkle
for fid, spec in owned_seeds.by_func {
  merged := case entry_seeds.get(fid) { .Some(m) => m, .None => Dict.new() }
  for loc, _ in spec.seed_locals { merged[loc] = true }
  entry_seeds[fid] = merged
  candidate_funcs[fid] = true          // ensure seeded funcs are in analysis scope
}
```
Keep `compute_candidate_artifacts(...)` as the empty-seed wrapper.

- [ ] **Step 4: Seeded producer.** In `mutable_produce.tw`, add `produce_mutable_decisions_seeded(opt, b, owned_seeds)` mirroring `produce_mutable_decisions` but calling `compute_candidate_artifacts_seeded(..., owned_seeds)` and merging seeded funcs into `roots`. `produce_mutable_decisions(opt, b)` delegates with `empty_seed_table()`. For an emitted decision whose site func is in `owned_seeds.by_func`, set `variant_key: .Some(owned_seeds.by_func[fid].variant)` (audit only). **Confirm no emit-path code branches on `variant_key`.**

- [ ] **Step 5: Run — verify pass.** Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add boot/compiler/codegen/ownership_verdicts.tw boot/compiler/codegen/mutable_produce.tw boot/tests/suites/variant_specialize_suite.tw
git commit -m "codegen(8G): seeded ownership artifacts + seeded decision production"
```

---

## Task 4: Physical clone + caller routing (non-recursive, record) (8G.2)

**Files:** Create `boot/compiler/codegen/variant_specialize.tw`; Test in the suite.

- [ ] **Step 1: Add the failing test** on `setb`: one clone added, `caller`'s `setb` call routed, clone has an owned seed.

```twinkle
    .test(
      "specialize clones setb and routes the caller site",
      fn() Result<Void, String> {
        art := try compile_fixture("setb")
        spec := variant_specialize.specialize_module(art.opt, art.builtins)
        try assert.equal(spec.anf.functions.len(), art.opt.functions.len() + 1)
        accepted := spec.routes.filter(fn(r) { r.fallback_reason == .None })
        try assert.equal(accepted.len(), 1)
        r := accepted[0]
        try assert.ok(in_dict(spec.seeds.by_func, r.clone_func), "clone has owned seed")
        try assert.ok(r.route_sites.len() >= 1, "caller site routed")
        try assert.ok(calls_target(spec.anf, "caller", r.clone_func), "caller routed to clone")
        .Ok({})
      },
    )
```
Define `in_dict(d, k)` and `calls_target(anf, caller_name, target_func)` in this step.

- [ ] **Step 2: Run — verify fail.** Expected: FAIL.

- [ ] **Step 3: Implement `specialize_module(anf, b)`:**
  1. `next := max(f.func_id.id for f in anf.functions) + 1`.
  2. `view := ownership.prune_dead_merge(cfg.build_view(anf, b))`; `sem := make_prelude_optimizer_semantics(b)`; `owned := ownership_verdicts.compute_artifacts(anf, b, sem)`; `vt := summary.compute_variants(owned.view, b, sem, owned.table)`.
  3. For each caller `f` in `owned.view.functions`: `sited := ownership.call_uniques_sited(f, owned.table, b, sem, Dict.new(), Dict.new())`. For each site, `v := summary.select_variant_for_args(vt, site.callee, site.arg_unique)`; if `.Some(variant)` (non-generic ⇒ `variant.unique.len() > 0`), mark `(callee, variant)` **needed** and remember `site.site_key` for it.
  4. For each needed `(callee, variant)`: `clone_id := next; next += 1`; deep-copy the callee `AnfFunctionDef` → `func_id = clone_id`, `name = "${orig.name}$v${clone_id}"`; **non-recursive slice: copy body verbatim (no in-body retarget — Task 7)**; append clone; `seed := summary.seed_for_variant(cfg_func_for(callee), variant)`; `seeds.set_clone(clone_id, CloneSpec.{ seed_locals: seed, variant })`; record provisional `VariantRoute`.
  5. Rewrite caller bodies: retarget each recorded route site's `ACall(AGlobalFunc(callee))` → `AGlobalFunc(clone_id)`.
  6. **Decision gate (whole-module):** `produce_mutable_decisions_seeded(anf_with_clones, b, seeds)`; any clone with zero decisions → drop clone, drop seed, re-point its route sites to `callee`, mark route `fallback_reason: .Some(.ZeroWin)`. Survivors: `fallback_reason: .None`.
  7. Return `SpecializeResult.{ anf: pruned, seeds: pruned_seeds, routes }`.

  Use the exhaustive ANF walk from `mutable_produce.tw` for deep-copy and route rewrites.

- [ ] **Step 4: Run — verify pass.** Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/codegen/variant_specialize.tw boot/tests/suites/variant_specialize_suite.tw
git commit -m "codegen(8G): physical clone + caller routing with whole-module gate"
```

---

## Task 5: Wire into link_program + WAT/aliasing gates (8G.2)

**Files:** Modify `boot/compiler/codegen/codegen.tw`; Create `setb_aliased.tw`; Test in the suite.

- [ ] **Step 1: Add the failing golden test.** Through `link_program`, the cloned `setb` emits `struct.set` shell reuse. Use a **token-exact, clone-scoped** WAT check, not a bare substring:

```twinkle
    .test(
      "cloned setb emits struct.set shell reuse end-to-end",
      fn() Result<Void, String> {
        wat := try compile_fixture_wat("setb")
        body := try wat_func_body(wat, "setb$v")           // isolate the clone body by name substring
        try assert.ok(wat_has_instr(body, "struct.set"), "clone body must reuse the shell")
        .Ok({})
      },
    )
```
Define `compile_fixture_wat(name)` (compile fixture, `codegen.emit_wat(link_program(...))`), `wat_func_body(wat, name_sub)` (slice from the matching `(func $…name_sub…` to its closing), and `wat_has_instr(body, tok)` (token-boundary match, not raw `contains`) in this step.

- [ ] **Step 2: Run — verify fail.** Expected: FAIL (pass not wired).

- [ ] **Step 3: Wire `link_program`.** After `anf_prime := builder_region.rewrite_module(...)`:

```twinkle
spec := if variant_specialize_enabled() {
  variant_specialize.specialize_module(anf_prime, builtins)
} else {
  variant_specialize.SpecializeResult.{ anf: anf_prime, seeds: variant_route.empty_seed_table(), routes: [] }
}
anf_spec := spec.anf
```
Replace the `anf_prime` inputs to `convert_closures(...)` and decision production with `anf_spec`; call `mutable_produce.produce_mutable_decisions_seeded(anf_spec, builtins, spec.seeds)`. Add `fn variant_specialize_enabled() Bool` reading `TWINKLE_VARIANT_SPECIALIZE` (default **on**; kill-switch).

- [ ] **Step 4: Run — verify pass.** `make quick-bundle-cli && target/twk run boot/tests/main.tw`; manually `target/twk wat boot/tests/fixtures/cfg/sound_uniqueness/setb.tw --func 'setb' --calls`. Expected: PASS + clone shows `struct.set`.

- [ ] **Step 5: Aliasing negative guard.** Create `setb_aliased.tw` where the record is read after the call (not last-use), so `arg_unique[0]` is false; assert no accepted route and the clone (if any) is absent — the caller keeps `struct.new`.

```twinkle
// setb_aliased.tw
pub type S = .{ a: Int, b: Int }
pub fn setb(s: S, v: Int) S { s.b = v; s }
pub fn caller() Int {
  m := S.{ a: 1, b: 2 }
  updated := m.setb(9)
  m.a + updated.b        // m read after the call -> setb arg not last-use
}
```
Test: `specialize_module` on `setb_aliased` yields `accepted.len() == 0`; WAT keeps `struct.new` in `caller`. Run: PASS.

- [ ] **Step 6: Commit.**

```bash
git add boot/compiler/codegen/codegen.tw boot/tests/fixtures/cfg/sound_uniqueness/setb_aliased.tw boot/tests/suites/variant_specialize_suite.tw
git commit -m "codegen(8G): wire specialization into link_program + WAT/aliasing gates"
```

---

## Task 6: Runtime correctness parity (8G.2)

**Files:** Create `boot/tests/fixtures/cfg/sound_uniqueness/setb_run.tw`.

- [ ] **Step 1: Create a runnable program** whose output depends on the routed clone:

```twinkle
// setb_run.tw
pub type S = .{ a: Int, b: Int }
pub fn setb(s: S, v: Int) S { s.b = v; s }
r := S.{ a: 1, b: 2 }
println((r.setb(9).a + r.setb(9).b).to_string())   // second setb rebuilds from a fresh literal each call
```
(If the double-call obscures ownership, use a single-call program that prints `S.{a:1,b:2}.setb(9).b` → `9`.)

- [ ] **Step 2: Specialize ON.** `make quick-bundle-cli && target/twk run boot/tests/fixtures/cfg/sound_uniqueness/setb_run.tw`. Record the output.
- [ ] **Step 3: Specialize OFF.** `TWINKLE_VARIANT_SPECIALIZE=0 target/twk run .../setb_run.tw`. Expected: identical output (in-place is semantically invisible).
- [ ] **Step 4: Commit.**

```bash
git add boot/tests/fixtures/cfg/sound_uniqueness/setb_run.tw
git commit -m "codegen(8G): runtime parity guard (specialize on/off)"
```

---

## Task 7: Recursive vector routing (Case V) (8G.3)

**Files:** Modify `boot/compiler/codegen/variant_specialize.tw`; Create `visit_rec.tw`, `visit_rec_run.tw`; Test in the suite.

- [ ] **Step 1: Create the recursive fixture** (empirically publishes `f295|0:`; first param is a builtin vector, so calls are direct, not dot-sugar):

```twinkle
// visit_rec.tw
pub fn visit(seen: Vector<Int>, n: Int) Vector<Int> {
  seen[0] = n
  if n <= 0 { seen } else { visit(seen, n - 1) }
}
pub fn go() Int {
  m: Vector<Int> = collect _ in range(1) { 0 }
  visit(m, 3).at(0)
}
```

- [ ] **Step 2: Add the failing test:**

```twinkle
    .test(
      "recursive clone retargets its self-call and emits set_in_place",
      fn() Result<Void, String> {
        art := try compile_fixture("visit_rec")
        spec := variant_specialize.specialize_module(art.opt, art.builtins)
        r := spec.routes.first_where(fn(x) { x.fallback_reason == .None and x.recursive_routes.len() >= 1 })
        try assert.ok(r != .None, "expected an accepted recursive variant with a retargeted self-call")
        wat := try compile_fixture_wat("visit_rec")
        body := try wat_func_body(wat, "visit$v")
        try assert.ok(wat_has_instr(body, "rt_arr__set_in_place"), "clone body must set in place")
        .Ok({})
      },
    )
```

- [ ] **Step 3: Run — verify fail.** Expected: FAIL (Task 4 copies the body verbatim; the self-call still targets the generic).

- [ ] **Step 4: Implement in-clone retargeting.** In the clone deep-copy, after copying, run `ownership.call_uniques_sited` on the clone **under its owned seed** (`seed_locals` seeded Unique). For each in-body `ACall(AGlobalFunc(peer_generic), args)` where `peer_generic` shares the original's SCC and `select_variant_for_args(vt, peer_generic, arg_unique)` returns the peer's owned variant, retarget to the peer clone id (self → this clone) and record `site_key` in `recursive_routes`. Ensure the peer clone exists (add to the needed set when a recursive route requires it).

- [ ] **Step 5: Run — verify pass.** Then create `visit_rec_run.tw` (a runnable variant of the fixture) and confirm identical output with `TWINKLE_VARIANT_SPECIALIZE` on and off. Expected: PASS.

- [ ] **Step 6: Commit.**

```bash
git add boot/compiler/codegen/variant_specialize.tw boot/tests/fixtures/cfg/sound_uniqueness/visit_rec.tw boot/tests/fixtures/cfg/sound_uniqueness/visit_rec_run.tw boot/tests/suites/variant_specialize_suite.tw
git commit -m "codegen(8G): recursive/SCC vector clone routing (Case V)"
```

---

## Task 8: Variant-route dry-run inspection (deferred 7E item) (8G.4)

**Files:** Modify `boot/compiler/codegen/variant_specialize.tw`, `boot/commands/ir.tw`; Test in the suite.

- [ ] **Step 1: Add the failing test:**

```twinkle
    .test(
      "render_routes names generic, clone, and the canonical key",
      fn() Result<Void, String> {
        art := try compile_fixture("setb")
        spec := variant_specialize.specialize_module(art.opt, art.builtins)
        out := variant_specialize.render_routes(spec.routes)
        try assert.str_contains(out, "setb")
        try assert.str_contains(out, "-> f")
        try assert.str_contains(out, "|0:")            // canonical variant key fragment
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run — verify fail.** Expected: FAIL (`render_routes` undefined).
- [ ] **Step 3: Implement `render_routes`** — one line per route: `generic f{g} -> clone f{c} "{name}" [{vid.variant_canonical_string(variant)}] sites={n} {accepted|fallback:{reason}} proof={id}` — and call it from the `--census --sites` path in `ir.tw` under a `variant routes` heading.
- [ ] **Step 4: Run — verify pass.** Inspect `target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/setb.tw --census --sites`. Expected: PASS + readable route table.
- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/codegen/variant_specialize.tw boot/commands/ir.tw boot/tests/suites/variant_specialize_suite.tw
git commit -m "codegen(8G): variant-route dry-run inspection (closes deferred 7E item)"
```

---

## Task 9: Per-function variant cap with test override (8G.4)

**Files:** Modify `boot/compiler/codegen/variant_specialize.tw`; Test in the suite.

- [ ] **Step 1: Add a direct unit test of the cap selection** (no fixture can produce >cap variants for one func, so test the pure selector on a synthetic candidate list, and exercise the env override):

```twinkle
    .test(
      "cap keeps at most variant_cap clones per generic and marks overflow OverCap",
      fn() Result<Void, String> {
        // Synthetic: 3 candidate variants for one generic func, cap forced to 1.
        cands := synth_candidates(700, 3)                        // helper: Vector of (callee=700, variant_i, decisions_i)
        kept := variant_specialize.apply_cap(cands, 1)           // pure selector under test
        accepted := kept.filter(fn(c) { c.fallback_reason == .None })
        overcap := kept.filter(fn(c) { c.fallback_reason == .Some(variant_route.FallbackReason.OverCap) })
        try assert.equal(accepted.len(), 1)
        try assert.equal(overcap.len(), 2)
        .Ok({})
      },
    )
```
Define `synth_candidates(func, n)` in this step. `apply_cap(cands, cap)` is the pure selector extracted from Task 4's needed-set logic.

- [ ] **Step 2: Run — verify fail.** Expected: FAIL (`apply_cap`/`variant_cap` undefined).
- [ ] **Step 3: Implement.** `pub fn variant_cap() Int` reads `TWINKLE_VARIANT_CAP` (default `4`); `pub fn apply_cap(cands, cap)` groups by callee, orders by (produced-decision count desc, then `variant_specificity` desc), accepts up to `cap`, marks the rest `OverCap`. Wire `specialize_module` to call `apply_cap(..., variant_cap())` in the needed-set step. The whole-module gate already prunes `ZeroWin`; the cap bounds the profitable-but-many case.
- [ ] **Step 4: Run — verify pass.** Expected: PASS.
- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/codegen/variant_specialize.tw boot/tests/suites/variant_specialize_suite.tw
git commit -m "codegen(8G): per-function variant cap (env-overridable) + selector unit test"
```

---

## Task 10: Self-host fixed point + docs (8G.4)

**Files:** Modify the sound-uniqueness READMEs and `docs/plans/README.md`.

- [ ] **Step 1: Full self-host.** `make bundle-cli` (sequential). Expected: succeeds.
- [ ] **Step 2: Fixed point.** `make stage2`, diff stage3 vs stage4. Expected: byte-identical.
- [ ] **Step 3: Full boot suite.** `target/twk run boot/tests/main.tw`. Expected: all pass incl. `variant_specialize`.
- [ ] **Step 4: Targeted Rust reference.** Codegen-filtered Rust tests (not full `cargo test`). Expected: pass.
- [ ] **Step 5: Update docs.** Mark 8G `[x]` in `codegen/README.md`; update both sound-uniqueness READMEs' focus to Phase 8H (record-backed field collections — incl. the `s.xs[i]=v` case), and note the two 8G scope-outs as analysis follow-ups: (a) non-recursive user vector wrappers and (b) prelude-wrapper calls do not publish variants today. Delete this plan's row from `docs/plans/README.md` and `git mv` this doc into `docs/plans/archive/`.
- [ ] **Step 6: Commit.**

```bash
git add -A
git commit -m "codegen(8G): ownership-specialized function variants landed; self-host fixed point"
```

---

## Scope boundary (what 8G does NOT do)

- **Prelude-wrapper calls** (`flags = .set_at(...)`): `set_at` is not a user function, so `compute_variants` never publishes it. Out of scope.
- **Non-recursive user vector wrappers** (`fn bump(xs: Vector<Int>, i) { xs[i]=0; xs }`): empirically does not publish a variant today. Making it publish is an **analysis-track** (Phase 6 candidate-publishing) change, not codegen 8G.
- **Collections behind a record field** (`cur.indices[node] = idx`, `s.xs[i]=v`): render `field=persistent(insufficient deep ownership)` — **Phase 8H** field-path machinery.

8G delivers: **record-shell reuse** for user record functions (`setb`, via live 8F) and **vector/record in-place** for recursive user functions (`visit`/Case V, via live 8A/8B). Both reuse already-live emit families; the pass adds only cloning, routing, and seeding.

---

## Standing invariants

- **Routing is the soundness boundary.** Route to a clone only when the analysis proves the argument `Unique` **and** last-use at that site (`select_variant_for_args` over real `call_uniques_sited` facts). Never a heuristic. Over-conservative is safe; over-eager corrupts a live copy.
- **Only `vt.by_func` membership licenses a clone.** The `--cfg` `select_variant` verdict is a pure per-call hint and does NOT prove a variant is published; never clone from it.
- **Emit stays variant-unaware.** No emit-path code branches on `variant_key`; the clone is an ordinary function with ordinary `Site`-keyed decisions.
- **Persistent fallback is default.** Absent proof, stale key, over cap, zero win, ambiguous → the caller keeps the generic callee.
- **The generic body is never rewritten in place;** a fully-collapsed generic is dropped by existing Wasm DCE.
- **The decision gate uses the real module** (ANF″ with clones at their real ids + deps), never a single-func projection.
- **Performance is an end-of-track signal;** judge by correctness, inspection, and the self-host fixed point.

---

## Self-review checklist (run before execution)

1. **Spec coverage:** public route/seed API (T1) + sited proof (T2) precede use; clone by published `VariantId` (T4), route from decisions (T4/T5), whole-module gate (T4), recursive routes (T7), capped + inspectable (T8/T9), deferred 7E dry-runs (T8). ✓
2. **Fixture correctness (empirically grounded):** `setb` (record, non-recursive) and `visit_rec` (vector, recursive) are the only anchors — both **verified to publish** `f295|0:`. `set_at`/`vector_once` and non-recursive vector wrappers are explicitly scoped out. ✓
3. **API visibility:** `variant_args_satisfied`/`variant_specificity`/`unique_seed_for_variant` stay private; the pass uses only `select_variant_for_args`/`seed_for_variant`/`variant_ids_of`/`variant_get` and `call_uniques_sited`. ✓
4. **Review round-2 fixes:** #1 anchors re-based (setb/visit); #2 `variant_ids_of` enumerates instead of string-matching; #3 `use compiler.opt.semantics as semantics`; #4 `SitedCallUniq.args: Vector<Atom>`; #5 seed via `seed_for_variant(cfg_func, variant)`; #6 `wat_has_instr` + `wat_func_body` (token-exact, clone-scoped); #7 `apply_cap` unit test + `TWINKLE_VARIANT_CAP` override. ✓
5. **Type consistency:** `SpecializeResult{anf,seeds,routes}`, `OwnedSeedTable{by_func: Dict<Int, CloneSpec>}`, `CloneSpec{seed_locals, variant}`, `SitedCallUniq{...}`, `produce_mutable_decisions_seeded`, `compute_candidate_artifacts_seeded`, `apply_cap`, `variant_cap` named identically across tasks; `variant_key` sources from `CloneSpec.variant`. ✓
6. **Test style:** all `.test("desc", fn() Result<Void,String> { … .Ok({}) })` on `runner.suite("variant_specialize")`; helpers defined in the step that first uses them. ✓
