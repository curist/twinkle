# Phase 8H — Record-Backed Field Collection Updates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Emit existing vector/dict in-place helpers for proven-owned collections reached through record fields, while keeping record-shell reuse and deep field-backing mutation as separate decisions.

**Architecture:** Extend the current ownership-artifact seam so record-update verdicts expose a structured `field_backing_reusable` bit alongside the existing `reusable_shell` bit. Teach codegen-owned seeded analysis to seed field paths from exact `VariantId` requirements, then add a field-backed quartet producer that recognizes `record_get` → collection update → `record_update` shapes and emits an ordinary call-swap `MutableDecision` only when the matching record-update site carries the structured field-path proof. The backend selector and emit path stay mechanical: vector/dict call sites still use the existing call selector and helpers; record updates still use the existing shell selector; absence, stale artifacts, duplicate decisions, field-path mismatch, and aliasing fall back persistently.

**Tech Stack:** Twinkle boot compiler (`boot/`), self-hosted. Build via `make quick-bundle-cli` while iterating; run boot tests with `target/twk run boot/tests/main.tw`; inspect with `target/twk ir <fixture> --census --sites` and `target/twk wat <fixture> --func <name> --calls`. No Rust stage0 changes: this is a boot-codegen optimization over existing hooks.

**Design source:** `docs/plans/sound-uniqueness/codegen/README.md` §"Codegen Phase 8H", `docs/plans/sound-uniqueness/analysis/records-fields.md`, `docs/plans/sound-uniqueness/codegen/handoff-contract.md`, and the as-built 8A–8G seams (`mutable_produce.tw`, `ownership_verdicts.tw`, `variant_specialize.tw`).

## Global Constraints

- Codegen consumes structured ownership decisions; it must not parse verdict text to decide mutation.
- Missing, stale, ambiguous, or mismatched decisions emit the ordinary persistent path.
- Shell reuse and field-backing mutation are independent: shell-only wins must stay possible when deep field mutation is rejected.
- A field-backed collection helper may emit only when a decision names the record shell path, projected collection local, collection update result, record write-back site, persistent fallback, mutable target, and proof id.
- A record field path seed must come from the exact canonical `VariantId`; a unique record shell never implies unique reference-typed field storage.
- 8H's first path-granular routing requires whole-argument last-use before claiming field paths; path-level consume-dead that allows post-call sibling reads is deferred.
- Do not add Set-specific optimizer logic: `Set<K>` wins must come through its `entries: Dict<K, Void>` field path and ordinary dict helper selection.
- Do not change source semantics, public APIs, or runtime helper ABIs.
- Do not run tree-sitter tests.
- After editing `.tw` files, run `target/twk fmt <changed .tw files>` and `target/twk lint boot/main.tw`.
- Performance comparisons are out of scope for 8H; use correctness, census rows, WAT/call inspection, and self-host fixed point as gates.

---

## Orientation

### Existing seams to preserve

- `compiler.codegen.mutable_select.MutableDecision` already has `OperationFamily.RecordBackedFieldCollection`, `field_path_key`, `variant_key`, and centralized fallback checks.
- Call emission already swaps `VectorSet`, `DictSet`, and `DictRemove` through `emit.mutable_sites.select_call_for_emit` and `mutable_select.select_call_with_policy`.
- Record shell emission already flips `ARecordUpdate.can_reuse` through `emit.mutable_sites.select_record_update_for_emit` and `mutable_select.select_record_update_with_policy`.
- `ownership.tw` already computes the text verdict `field=in-place([.fN] unique)` for accepted quartets; 8H must make that proof structured rather than text-parsed.
- `variant_id.VariantId` already represents field-path requirements with downward-closed keys such as `f123|0:;0:0` (param 0 shell plus param 0 field 0).
- `summary.seed_for_variant` currently seeds only shell ownership. That is intentional for analysis summaries; 8H adds codegen-owned field-path seeding for clones and mutable artifact production.

### First-slice shape

The first 8H slice covers same-block, ANF-visible quartets:

```text
Lproj = ARecordGet(Lrecord, field F, type T)
Lupd  = ACall(persistent vector/dict update, args where base arg is Lproj)
Lrec  = ARecordUpdate(Lrecord, field F, value Lupd, type T)
```

The quartet may appear inside loops or cloned function bodies, but the three sites must be visible in the same straight-line ANF `Let` chain. Broader cross-block reconstruction is deferred until a fixture demonstrates the need; the ownership proof still governs loops and aliases inside the accepted first-slice shape.

### Decision shape

8H produces an ordinary call-swap decision at the collection update site so existing emission can select the existing helper:

```twinkle
mutable_select.MutableDecision.{
  site: mutable_select.Site.{ func: c.func_id, local: c.update_result },
  family: c.decision_family,                    // VectorSet / DictSet / DictRemove
  source_local: c.projected_collection,          // local from ARecordGet
  result_local: c.update_result,                 // collection update result
  arg_count: c.arg_count,
  base_arg_index: .Some(c.base_arg_index),
  persistent_func: .Some(c.persistent_func),
  mutable_func: .Some(c.mutable_func),
  variant_key: .None,                            // stamp_variant_keys fills clone decisions later
  field_path_key: c.field_path_key,              // e.g. "42:f0"
  proof_debug_id: "phase8h:${c.func}:record L${c.record_base.id}.f${c.fid.id}:get L${c.projected_collection.id}:update L${c.update_result.id}:write L${c.record_update_result.id}",
}
```

`MutableDecision.family` remains the real call family because the existing call selector expects `VectorSet`, `DictSet`, or `DictRemove`. The render row names the source as `record_backed_<family>` so inspection can distinguish it from a whole-value local update.

---

## File Structure

- **Modify** `boot/compiler/cfg.tw` — add structured `verdict_field_backing_reusable: Dict<Int, Bool>` to `BlockFacts`.
- **Modify** `boot/compiler/ownership.tw` — record `field_backing_reusable` at `ARecordUpdate` verdict sites; add field-path entry seed plumbing for codegen-owned analysis.
- **Modify** `boot/compiler/codegen/ownership_verdicts.tw` — carry `field_backing_reusable` through `SiteVerdict` and artifact projections.
- **Modify** `boot/compiler/codegen/variant_route.tw` — extend `CloneSpec` with field-path seed data while preserving shell seeds.
- **Modify** `boot/compiler/summary.tw` — add a public field-seed helper derived from `VariantId` requirements.
- **Modify** `boot/compiler/codegen/mutable_produce.tw` — add field-backed quartet candidates, structured proof join, non-overlap with ordinary call decisions, and rows.
- **Modify** `boot/compiler/codegen/mutable_audit.tw` and `boot/commands/ir.tw` — render field-backed call decisions in `--census --sites` audit output.
- **Modify** `boot/compiler/codegen/variant_specialize.tw` — ensure the structural `updatable_funcs` filter sees field-backed collection candidates.
- **Create** `boot/tests/suites/field_backed_collection_suite.tw` — producer/emission/inspection tests for 8H.
- **Modify** `boot/tests/main.tw` — register the new suite.
- **Create** fixtures under `boot/tests/fixtures/cfg/sound_uniqueness/`:
  - `field_dict_update.tw`
  - `field_dict_alias_old.tw`
  - `field_vector_update.tw`
  - `field_vector_alias_old.tw`
  - `field_shell_only.tw`
  - `field_visit_rec.tw`
  - `field_set_wrapper.tw`
  - `field_transport_ctx.tw`

---

## Task 1: Structured field-backing verdicts

**Files:** Modify `boot/compiler/cfg.tw`; Modify `boot/compiler/ownership.tw`; Modify `boot/compiler/codegen/ownership_verdicts.tw`; Modify `boot/tests/suites/cfg_field_facts_suite.tw`.

**Interfaces:**
- Produces: `ownership_verdicts.SiteVerdict.{ text: String, reusable_shell: Bool, field_backing_reusable: Bool }`.
- Produces: `cfg.BlockFacts.verdict_field_backing_reusable: Dict<Int, Bool>`.
- Consumers: Task 5 joins field-backed candidates with this structured bit.

- [ ] **Step 1: Add failing structured-verdict assertions.** In `cfg_field_facts_suite.tw`, add this helper near existing verdict helpers:

```twinkle
fn field_backing_reusable(f: cfg.CfgFunction, local: Int) Bool {
  for blk in f.blocks {
    case blk.exit.verdict_field_backing_reusable.get(local) {
      .Some(v) => return v,
      .None => {},
    }
  }
  false
}
```

Add one positive assertion to the existing quartet positive test:

```twinkle
try assert.is_true(field_backing_reusable(f, 4))
```

Add one negative assertion to the existing `replacing an unowned field with a fresh value is not field-in-place` test:

```twinkle
try assert.is_false(field_backing_reusable(analyzed_func(m), 3))
```

- [ ] **Step 2: Run and verify the red state.**

```bash
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: compile failure naming `verdict_field_backing_reusable` as unknown.

- [ ] **Step 3: Add the CFG storage field.** In `cfg.BlockFacts`, add:

```twinkle
verdict_field_backing_reusable: Dict<Int, Bool>,
```

In `empty_block_facts()`, initialize it with `Dict.new()`.

- [ ] **Step 4: Populate the field bit in ownership verdict production.** In `ownership.tw`, change `BlockVerdicts` from:

```twinkle
type BlockVerdicts = .{ texts: Dict<Int, String>, reusable_shell: Dict<Int, Bool> }
```

to:

```twinkle
type BlockVerdicts = .{
  texts: Dict<Int, String>,
  reusable_shell: Dict<Int, Bool>,
  field_backing_reusable: Dict<Int, Bool>,
}
```

Initialize `field_backing_reusable: Dict<Int, Bool> = Dict.new()` in `block_verdicts`. In the `ARecordUpdate` arm, after computing `field_backing_moved`, set:

```twinkle
field_backing_reusable[inst.anf_local.id] = field_backing_moved
```

Return the new field from `BlockVerdicts`, and assign it to `blk.exit.verdict_field_backing_reusable` beside `blk.exit.verdict_reusable_shell`.

- [ ] **Step 5: Carry the bit through codegen artifacts.** In `ownership_verdicts.tw`, change `SiteVerdict` to:

```twinkle
pub type SiteVerdict = .{ text: String, reusable_shell: Bool, field_backing_reusable: Bool }
```

Update all fallback constructors to set `field_backing_reusable: false`. In `verdicts_from_artifacts`, read `blk.exit.verdict_field_backing_reusable` for each local.

- [ ] **Step 6: Run and verify green.**

```bash
target/twk fmt boot/compiler/cfg.tw boot/compiler/ownership.tw boot/compiler/codegen/ownership_verdicts.tw boot/tests/suites/cfg_field_facts_suite.tw
target/twk lint boot/main.tw
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: suite passes; existing rendered verdict text remains byte-for-byte unchanged for the tested lines.

- [ ] **Step 7: Commit.**

```bash
git add boot/compiler/cfg.tw boot/compiler/ownership.tw boot/compiler/codegen/ownership_verdicts.tw boot/tests/suites/cfg_field_facts_suite.tw
git commit -m "analysis: expose structured field-backing verdicts for codegen"
```

---

## Task 2: Path-granular variant publication and selection

**Files:** Modify `boot/compiler/variant_id.tw`; Modify `boot/compiler/summary.tw`; Modify `boot/compiler/ownership.tw`; Modify `boot/compiler/codegen/variant_specialize.tw`; Create `boot/tests/suites/field_backed_collection_suite.tw`; Create `boot/tests/fixtures/cfg/sound_uniqueness/field_visit_rec.tw`; Modify `boot/tests/main.tw`.

**Interfaces:**
- Produces: `summary.select_variant_for_arg_paths(vt, callee_id, arg_paths: Vector<vid.PathSet>) vid.VariantId?`.
- Produces: `ownership.SitedCallUniq.arg_paths: Vector<vid.PathSet>` while preserving `arg_unique: Vector<Bool>` for existing callers.
- Changes: `summary.compute_variants` publishes exact downward-closed field-path keys discovered in each converged variant summary's `in_place_paths`, not just shell-only keys.
- Consumers: Task 3 field seeds may trust `VariantId` field requirements because Task 2 proves those exact paths at each routed call site.

- [ ] **Step 1: Create the field-recursive fixture and failing path-granular selection test.** Create `field_visit_rec.tw`:

```twinkle
pub type State = .{ xs: Vector<Int>, spare: Vector<Int> }

pub fn visit(cur: State, n: Int) State {
  cur.xs[0] = n
  if n <= 0 { cur } else { visit(cur, n - 1) }
}

pub fn go() Int {
  xs: Vector<Int> = collect _ in range(1) { 0 }
  spare: Vector<Int> = collect _ in range(1) { 99 }
  out := visit(State.{ xs, spare }, 3)
  out.xs.at(0)
}

println(go().to_string())
```

Create `field_backed_collection_suite.tw` with the standard imports and helpers from `variant_specialize_suite.tw`, plus `use compiler.variant_id as vid` and a `wat_func_body_result` wrapper:

```twinkle
fn wat_func_body_result(wat: String, marker: String) Result<String, String> {
  case wat_func_body(wat, marker) {
    .Some(body) => .Ok(body),
    .None => .Err("function body marker not found: ${marker}"),
  }
}
```

Register the suite in `boot/tests/main.tw`. Add this failing test; it must assert both sides:

```twinkle
.test(
  "field-path variant is selected only when the call arg proves that field path",
  fn() Result<Void, String> {
    art := try compile_fixture("field_visit_rec")
    vt := variants_of(art)
    ids := summary.variant_ids_of(vt)
    field_variant := ids.find(fn(v) {
      v.unique.any(fn(req) { !req.path.is_shell() })
    })
    case field_variant {
      .Some(v) => {
        shell_only: Vector<vid.PathSet> = [vid.shell_set()]
        try assert.equal(summary.select_variant_for_arg_paths(vt, v.func, shell_only), .None)
        exact: Vector<vid.PathSet> = [vid.shell_set().add(vid.field(0))]
        try assert.equal(summary.select_variant_for_arg_paths(vt, v.func, exact), .Some(v))
        .Ok({})
      },
      .None => .Err("expected a published field-path variant"),
    }
  },
)
```

- [ ] **Step 2: Run and verify the red state.**

```bash
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: compile failure naming `select_variant_for_arg_paths` or failure because no field-path variant is published.

- [ ] **Step 3: Publish exact field-path variants from converged variant summaries.** In `summary.run_scc_variants`, keep the existing shell candidate as the optimistic seed for discovering a variant. When a candidate survives validation, construct the final published key from the converged summary's `params[k].in_place_paths`:

```twinkle
fn variant_for_paths(func_id: Int, param: Int, paths: vid.PathSet) vid.VariantId {
  reqs: vid.UniqueKey = []
  for p in paths.paths {
    reqs = .append(vid.UniqueReq.{ param, path: p })
  }
  vid.canonicalize_variant(vid.VariantId.{ func: func_id, unique: reqs })
}
```

Use `variant_for_paths(m, k, s.params[k].in_place_paths)` for the `VariantEntry.variant` and `by_func` key instead of the original shell-only candidate. Keep the shell-only optimistic hypothesis: it is a discovery mechanism, not the final published key. Add this concrete test to `field_backed_collection_suite.tw`:

```twinkle
.test(
  "published recursive field variant includes the exact field path",
  fn() Result<Void, String> {
    art := try compile_fixture("field_visit_rec")
    vt := variants_of(art)
    ids := summary.variant_ids_of(vt)
    field_variant := ids.find(fn(v) { v.unique.any(fn(req) { !req.path.is_shell() }) })
    case field_variant {
      .Some(v) => {
        key := vid.variant_canonical_string(v)
        try assert.str_contains(key, "|0:;0:0")
        .Ok({})
      },
      .None => .Err("expected canonical field-path variant for visit"),
    }
  },
)
```

- [ ] **Step 4: Add path-set satisfaction.** In `summary.tw`, add:

```twinkle
fn variant_paths_satisfied(v: vid.VariantId, arg_paths: Vector<vid.PathSet>) Bool {
  if v.unique.len() == 0 { return false }
  for req in v.unique {
    if req.param < 0 or req.param >= arg_paths.len() {
      return false
    }
    if !arg_paths[req.param].contains(req.path) {
      return false
    }
  }
  true
}

pub fn select_variant_for_arg_paths(
  vt: VariantSummaryTable,
  callee_id: Int,
  arg_paths: Vector<vid.PathSet>,
) vid.VariantId? {
  best: vid.VariantId? = .None
  best_spec := 0 - 1
  best_key := ""
  case vt.by_func.get(callee_id) {
    .Some(keys) => for k in keys {
      case vt.by_key.get(k) {
        .Some(e) => if variant_paths_satisfied(e.variant, arg_paths) {
          sp := variant_specificity(e.variant)
          if sp > best_spec or sp == best_spec and (best_key.len() == 0 or k < best_key) {
            best_spec = sp
            best_key = k
            best = .Some(e.variant)
          }
        },
        .None => {},
      }
    },
    .None => {},
  }
  best
}
```

Add `pub fn contains(self: PathSet, p: ParamPath) Bool` to `variant_id.tw`:

```twinkle
pub fn contains(self: PathSet, p: ParamPath) Bool {
  for q in self.paths {
    if q.eq(p) { return true }
  }
  false
}
```

Reimplement `variant_args_satisfied` and `select_variant_for_args` as compatibility wrappers that convert `true` to `vid.shell_set()` and `false` to `vid.empty_set()`.

- [ ] **Step 5: Extend call-site uniqueness facts with exact paths.** In `ownership.tw`, extend `SitedCallUniq`:

```twinkle
arg_paths: Vector<vid.PathSet>,
```

At each `ACall`, compute one path set per argument with a helper shaped as:

```twinkle
fn call_arg_paths(pre: ForwardState, args: Vector<Atom>, arg_unique: Vector<Bool>) Vector<vid.PathSet> {
  collect arg, i in args {
    paths := if i < arg_unique.len() and arg_unique[i] { vid.shell_set() } else { vid.empty_set() }
    if i < arg_unique.len() and arg_unique[i] {
      fmap := pre.atom_field_own(arg)
      for k in fmap.sorted_keys() {
        p := ff.path_of_key(k)
        if p.segs.len() == 1 {
          case p.segs[0] {
            .Field(f) => paths = paths.add(vid.field(f)),
            _ => {},
          }
        }
      }
    }
    paths
  }
}
```

Do not include `Elem`, `Val`, or payload paths in `arg_paths` for 8H. This deliberately requires the whole argument to be last-use (`arg_unique[i]`) before any field path is claimed, so a call followed by a sibling-field read remains on the generic function in 8H even when a future path-level consume-dead analysis could prove it safe.

- [ ] **Step 6: Route clones with path-granular proof.** In `variant_specialize.collect_groups` and `recursive_routes_for`, call `summary.select_variant_for_arg_paths(vt, s.callee, s.arg_paths)`. Keep `select_variant_for_args` only for shell-only compatibility helpers. This is the safety boundary: a shell-unique record with a shared field must not route to a clone whose `VariantId` requires that field.

- [ ] **Step 7: Update variant reachability and diagnostic rendering to use path sets.** In `summary.reachable_variants`, replace both `call_uniques(...).arg_unique` scans with `call_uniques_sited(...).arg_paths` scans. Change `mark_site_variants` to take `Vector<vid.PathSet>` and use `variant_paths_satisfied`. Keep `unique_seed_for_variant` shell-only for diagnostic re-analysis: it may render less field ownership than codegen's Task 3 seeded artifacts, but it must not drop field-path variants from reachability or print a shell-only key for them. Add a small public test hook, `summary.reachable_variant_keys_for_test(view, b, sem, generic, variants) Vector<String>`, that returns the canonical keys from `reachable_variants`; use it only from tests. Add this test to `field_backed_collection_suite.tw`:

```twinkle
.test(
  "variant reachability keeps field-path variants visible in cfg render inputs",
  fn() Result<Void, String> {
    art := try compile_fixture("field_visit_rec")
    vt := variants_of(art)
    view := ownership.prune_dead_merge(cfg.build_view(art.opt, art.builtins))
    sem := semantics.make_prelude_optimizer_semantics(art.builtins)
    generic := summary.compute(view, art.builtins, sem)
    keys := summary.reachable_variant_keys_for_test(view, art.builtins, sem, generic, vt)
    try assert.ok(keys.any(fn(k) { k.contains("|0:;0:0") }), "field-path variant key must remain reachable for cfg rendering")
    .Ok({})
  },
)
```

- [ ] **Step 8: Run and verify green.**

```bash
target/twk fmt boot/compiler/variant_id.tw boot/compiler/summary.tw boot/compiler/ownership.tw boot/compiler/codegen/variant_specialize.tw boot/tests/suites/field_backed_collection_suite.tw boot/tests/main.tw boot/tests/fixtures/cfg/sound_uniqueness/field_visit_rec.tw
target/twk lint boot/main.tw
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: field-path variants publish only when `in_place_paths` contains that path; shell-only `arg_unique` no longer satisfies field-path variants; existing 8G shell-only routing tests still pass.

- [ ] **Step 9: Commit.**

```bash
git add boot/compiler/variant_id.tw boot/compiler/summary.tw boot/compiler/ownership.tw boot/compiler/codegen/variant_specialize.tw boot/tests/suites/field_backed_collection_suite.tw boot/tests/main.tw boot/tests/fixtures/cfg/sound_uniqueness/field_visit_rec.tw
git commit -m "analysis: publish and select ownership variants by exact field paths"
```

---

## Task 3: Codegen-owned field-path entry seeds for variants

**Files:** Modify `boot/compiler/codegen/variant_route.tw`; Modify `boot/compiler/summary.tw`; Modify `boot/compiler/ownership.tw`; Modify `boot/compiler/codegen/ownership_verdicts.tw`; Modify `boot/compiler/codegen/variant_specialize.tw`; Modify `boot/tests/suites/field_backed_collection_suite.tw`; Modify `boot/tests/suites/variant_specialize_suite.tw`.

**Interfaces:**
- Produces: `summary.field_seed_for_variant(f: CfgFunction, v: vid.VariantId) Dict<Int, ff.FieldMap>`.
- Produces: `ownership.EntrySeedFacts = .{ unique_locals: Dict<Int, Bool>, field_own: Dict<Int, ff.FieldMap> }` plus wrappers preserving existing `Dict<Int, Bool>` callers.
- Changes: `variant_route.CloneSpec` carries both shell and field seeds.

- [ ] **Step 1: Add the failing variant field-seed test.** In `field_backed_collection_suite.tw`:

```twinkle
.test(
  "variant field requirements seed deep field ownership in clones",
  fn() Result<Void, String> {
    art := try compile_fixture("field_visit_rec")
    spec := variant_specialize.specialize_module(art.opt, art.builtins)
    routed := spec.routes.find(fn(r) { r.fallback_reason == .None })
    case routed {
      .Some(r) => {
        seed := case spec.seeds.by_func.get(r.clone_func) {
          .Some(s) => s,
          .None => return .Err("clone missing seed"),
        }
        try assert.ok(seed.field_seed_paths.keys().len() > 0, "clone must carry a field seed")
        .Ok({})
      },
      .None => .Err("expected a routed field-backed recursive variant"),
    }
  },
)
```

- [ ] **Step 2: Run and verify the red state.**

```bash
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: compile failure naming `field_seed_paths` or `field_seed_for_variant` as unknown.

- [ ] **Step 3: Extend `CloneSpec` without removing shell seeds.** In `variant_route.tw`, change `CloneSpec` to:

```twinkle
use compiler.field_facts as ff

pub type CloneSpec = .{
  seed_locals: Dict<Int, Bool>,
  field_seed_paths: Dict<Int, ff.FieldMap>,
  variant: vid.VariantId,
}
```

Update all construction sites in `variant_specialize.tw` and `variant_specialize_suite.tw` to pass `field_seed_paths: Dict.new()` until Step 7 fills real data. In `variant_specialize_suite.tw`, the existing manual construction becomes:

```twinkle
spec := variant_route.CloneSpec.{ seed_locals: seed, field_seed_paths: Dict.new(), variant }
```

- [ ] **Step 4: Add the summary field-seed helper.** In `summary.tw`, import `compiler.field_facts as ff` and add:

```twinkle
pub fn field_seed_for_variant(f: CfgFunction, v: vid.VariantId) Dict<Int, ff.FieldMap> {
  out: Dict<Int, ff.FieldMap> = Dict.new()
  for req in v.unique {
    if !req.path.is_shell() and req.param >= 0 and req.param < f.params.len() and req.path.segs.len() == 1 {
      local := f.params[req.param].id
      existing := case out.get(local) { .Some(m) => m, .None => ff.empty() }
      out[local] = existing.set_path(ff.field_path(req.path.segs[0]), 0)
    }
  }
  out
}
```

The tag value is `0` because `field_facts.tw` stores only Unique paths and documents `0` as the Unique tag. Keep this helper field-only and depth-one because `VariantId` does not publish `Elem`/`Val` requirements.

- [ ] **Step 5: Add field-aware entry seeds to ownership analysis.** In `ownership.tw`, add:

```twinkle
pub type EntrySeedFacts = .{
  unique_locals: Dict<Int, Bool>,
  field_own: Dict<Int, ff.FieldMap>,
}

pub fn empty_entry_seed_facts() EntrySeedFacts {
  EntrySeedFacts.{ unique_locals: Dict.new(), field_own: Dict.new() }
}
```

Add a `seed_param_field_own(entry_field, field_seed, params)` helper that copies only field maps for parameter locals present in `params`. Add field-aware twins of the existing entry-seed APIs, then keep the current public functions as wrappers:

```twinkle
pub fn analyze_with_summaries_and_entry_seed_facts(..., entry_seeds: Dict<Int, EntrySeedFacts>) CfgView
pub fn analyze_selected_with_summaries_and_entry_seed_facts(..., entry_seeds: Dict<Int, EntrySeedFacts>, cache: FixCache) CfgView
```

Inside block-0 entry seeding, apply both:

```twinkle
entry_own = seed_param_own(entry_own, seed.unique_locals, params)
entry_field = seed_param_field_own(entry_field, seed.field_own, params)
```

Only disable fix-cache reuse when either `unique_locals` or `field_own` is non-empty.

- [ ] **Step 6: Thread field seeds into codegen artifacts.** In `ownership_verdicts.compute_candidate_artifacts_seeded`, build `Dict<Int, ownership.EntrySeedFacts>` from uniform shell seeds plus `owned_seeds.by_func`:

```twinkle
facts := ownership.EntrySeedFacts.{ unique_locals: merged_shell_seed, field_own: spec.field_seed_paths }
entry_seed_facts[fid] = facts
```

Call `ownership.analyze_selected_with_summaries_and_entry_seed_facts` instead of the shell-only wrapper.

- [ ] **Step 7: Populate clone field seeds.** In `variant_specialize.tw`, when constructing each `CloneSpec`, compute:

```twinkle
field_seed := summary.field_seed_for_variant(ccfg, g.variant)
seeds = .set_clone(clone_id, variant_route.CloneSpec.{
  seed_locals: seed,
  field_seed_paths: field_seed,
  variant: g.variant,
})
```

- [ ] **Step 8: Run and verify green.**

```bash
target/twk fmt boot/compiler/codegen/variant_route.tw boot/compiler/summary.tw boot/compiler/ownership.tw boot/compiler/codegen/ownership_verdicts.tw boot/compiler/codegen/variant_specialize.tw boot/tests/suites/field_backed_collection_suite.tw boot/tests/suites/variant_specialize_suite.tw
target/twk lint boot/main.tw
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: new variant field-seed test passes; existing 8G tests still pass.

- [ ] **Step 9: Commit.**

```bash
git add boot/compiler/codegen/variant_route.tw boot/compiler/summary.tw boot/compiler/ownership.tw boot/compiler/codegen/ownership_verdicts.tw boot/compiler/codegen/variant_specialize.tw boot/tests/suites/field_backed_collection_suite.tw boot/tests/suites/variant_specialize_suite.tw
git commit -m "codegen(8H): seed variant-owned record field paths"
```

---

## Task 4: Field-backed quartet candidate detection

**Files:** Modify `boot/compiler/codegen/mutable_produce.tw`; Modify `boot/tests/suites/field_backed_collection_suite.tw`; Create `field_dict_update.tw`; Create `field_vector_update.tw`.

**Interfaces:**
- Produces: `FieldBackedCandidateSet` with one candidate per accepted ANF-visible quartet shape.
- Consumes: existing mutable catalog entries for vector set, dict set, and dict remove.
- No emitted-code change in this task.

- [ ] **Step 1: Create positive local fixtures.** `field_dict_update.tw`:

```twinkle
pub type Env = .{ types: Dict<Int, Int>, values: Dict<Int, Int> }

pub fn go() Int {
  env := Env.{ types: Dict.new(), values: Dict.new() }
  env.types[1] = 20
  case env.types.get(1) {
    .Some(v) => v,
    .None => 0,
  }
}
```

`field_vector_update.tw`:

```twinkle
pub type State = .{ xs: Vector<Int>, spare: Vector<Int> }

pub fn go() Int {
  xs: Vector<Int> = collect _ in range(1) { 0 }
  spare: Vector<Int> = collect _ in range(1) { 99 }
  st := State.{ xs, spare }
  st.xs[0] = 7
  st.xs.at(0)
}
```

- [ ] **Step 2: Add failing candidate tests.** In the new suite:

```twinkle
.test(
  "detects field-backed dict and vector quartets",
  fn() Result<Void, String> {
    dict_art := try compile_fixture("field_dict_update")
    dict_candidates := mutable_produce.field_backed_collection_candidates(dict_art.opt, dict_art.builtins)
    try assert.equal(dict_candidates.candidates.len(), 1)
    try assert.str_contains(dict_candidates.candidates[0].family, "dict")

    vec_art := try compile_fixture("field_vector_update")
    vec_candidates := mutable_produce.field_backed_collection_candidates(vec_art.opt, vec_art.builtins)
    try assert.equal(vec_candidates.candidates.len(), 1)
    try assert.str_contains(vec_candidates.candidates[0].family, "vector")
    .Ok({})
  },
)
```

- [ ] **Step 3: Run and verify the red state.**

```bash
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: compile failure naming `field_backed_collection_candidates` as unknown.

- [ ] **Step 4: Add candidate types.** In `mutable_produce.tw`, add:

```twinkle
pub type FieldBackedCandidate = .{
  func_id: FuncId,
  func: String,
  record_base: LocalId,
  projected_collection: LocalId,
  update_result: LocalId,
  record_update_result: LocalId,
  tid: TypeId,
  fid: FieldId,
  field_path_key: String,
  arg_count: Int,
  family: String,
  base_arg_index: Int,
  decision_family: mutable_select.OperationFamily,
  persistent_func: FuncId,
  mutable_func: FuncId,
  loop_depth: Int,
}

pub type FieldBackedCandidateSet = .{ candidates: Vector<FieldBackedCandidate> }
```

- [ ] **Step 5: Implement the straight-line detector.** Walk the same `Let`/`AIf`/`AMatch`/`ALoop`/`ADefer` skeleton as `update_call_candidates`. Within each straight-line `Let` chain, track projected record fields:

```twinkle
type ProjectionInfo = .{ record_base: LocalId, tid: TypeId, fid: FieldId, projected: LocalId, loop_depth: Int }
```

On `ARecordGet(.ALocal(record_base), fid, tid)`, remember `result local -> ProjectionInfo`. On an update `ACall` whose catalog entry has a supported call family and whose base arg is a projected local, remember `update result -> projection + catalog target`. On `ARecordUpdate(.ALocal(record_base), fid, .ALocal(update_result), _, tid)`, emit a `FieldBackedCandidate` only when record base, field id, type id, and update result all match. Do not cross a nested control-flow boundary with the projection map; nested branches get their own recursive walk.

- [ ] **Step 6: Run and verify green.**

```bash
target/twk fmt boot/compiler/codegen/mutable_produce.tw boot/tests/suites/field_backed_collection_suite.tw boot/tests/fixtures/cfg/sound_uniqueness/field_dict_update.tw boot/tests/fixtures/cfg/sound_uniqueness/field_vector_update.tw
target/twk lint boot/main.tw
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: candidate tests pass; emitted WAT is unchanged because no decisions are produced from the new candidates yet.

- [ ] **Step 7: Commit.**

```bash
git add boot/compiler/codegen/mutable_produce.tw boot/tests/suites/field_backed_collection_suite.tw boot/tests/fixtures/cfg/sound_uniqueness/field_dict_update.tw boot/tests/fixtures/cfg/sound_uniqueness/field_vector_update.tw
git commit -m "codegen(8H): detect record-backed collection update quartets"
```

---

## Task 5: Produce explicit field-backed call decisions

**Files:** Modify `boot/compiler/codegen/mutable_produce.tw`; Modify `boot/tests/suites/field_backed_collection_suite.tw`; Create `field_dict_alias_old.tw`; Create `field_vector_alias_old.tw`.

**Interfaces:**
- Produces: `produce_field_backed_collection_decisions_with_artifacts(opt, b, artifacts) ProducedDecisions`.
- Changes: `produce_mutable_decisions_seeded` merges ordinary call, record shell, and field-backed decisions without duplicate call-site decisions.
- Preserves: ordinary whole-local vector/dict decisions outside record-backed quartets.

- [ ] **Step 1: Create alias-negative fixtures.** `field_dict_alias_old.tw`:

```twinkle
pub type Env = .{ types: Dict<Int, Int>, values: Dict<Int, Int> }

pub fn go() Bool {
  env := Env.{ types: Dict.new(), values: Dict.new() }
  old := env.types
  env.types[1] = 20
  old.has(1)
}

println(go().to_string())
```

`field_vector_alias_old.tw`:

```twinkle
pub type State = .{ xs: Vector<Int>, spare: Vector<Int> }

pub fn go() Int {
  xs: Vector<Int> = collect _ in range(1) { 0 }
  spare: Vector<Int> = collect _ in range(1) { 99 }
  st := State.{ xs, spare }
  old := st.xs
  st.xs[0] = 7
  old.at(0)
}

println(go().to_string())
```

- [ ] **Step 2: Add failing producer tests.** In the new suite:

```twinkle
fn selected_field_backed_count(p: mutable_produce.ProducedDecisions) Int {
  n := 0
  for r in p.rows {
    if r.family.contains("record_backed_") and r.reason.contains("field-backed decision produced") {
      n = n + 1
    }
  }
  n
}

.test(
  "field-backed decisions require structured field proof",
  fn() Result<Void, String> {
    dict_art := try compile_fixture("field_dict_update")
    dict_prod := mutable_produce.produce_mutable_decisions(dict_art.opt, dict_art.builtins)
    try assert.equal(selected_field_backed_count(dict_prod), 1)

    alias_art := try compile_fixture("field_dict_alias_old")
    alias_prod := mutable_produce.produce_mutable_decisions(alias_art.opt, alias_art.builtins)
    try assert.equal(selected_field_backed_count(alias_prod), 0)
    .Ok({})
  },
)
```

- [ ] **Step 3: Run and verify the red state.**

```bash
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: test fails because no row says `field-backed decision produced` and the call decision has an empty `field_path_key`.

- [ ] **Step 4: Produce field-backed decisions from structured verdicts.** Add `produce_field_backed_collection_decisions_with_artifacts`. It must:

1. collect `FieldBackedCandidate`s;
2. validate `artifact_key_for_anf(opt)` against `artifacts.key`;
3. join on the matching **record update** key `${func_id}#${record_update_result}`;
4. accept only when `SiteVerdict.field_backing_reusable` is true;
5. create the call-site `MutableDecision` shown in the Decision shape section;
6. render rows with `family: "record_backed_${c.family}"`, `persistent`, `mutable`, `state: "candidate"`, `reason: "field-backed decision produced"`, and `proof_debug_id` in the `proof` column;
7. render rejected rows with concrete reasons: stale artifact, no structured field proof, missing mutable target, or unsupported family.

- [ ] **Step 5: Prevent duplicate decisions at the same call site.** Add a helper that collects the accepted field-backed call site keys. Modify ordinary call decision production so a whole-local call decision is not produced for a site that already has an accepted field-backed decision. This keeps the selector from seeing two decisions for the same call and falling back as `Ambiguous`.

- [ ] **Step 6: Merge field-backed rows in the build producer.** In `produce_mutable_decisions_seeded`, compute call, record, and field-backed candidates up front. Include field-backed candidate function roots in the artifact scope. Merge produced tables in this order:

```text
field-backed call decisions
ordinary call decisions excluding accepted field-backed sites
record-shell decisions
```

Call `stamp_variant_keys` after the full merge so clone decisions keep their `VariantId` audit trail.

- [ ] **Step 7: Run and verify green.**

```bash
target/twk fmt boot/compiler/codegen/mutable_produce.tw boot/tests/suites/field_backed_collection_suite.tw boot/tests/fixtures/cfg/sound_uniqueness/field_dict_alias_old.tw boot/tests/fixtures/cfg/sound_uniqueness/field_vector_alias_old.tw
target/twk lint boot/main.tw
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: positive field-backed fixtures produce one accepted field-backed call decision; alias fixtures do not.

- [ ] **Step 8: Commit.**

```bash
git add boot/compiler/codegen/mutable_produce.tw boot/tests/suites/field_backed_collection_suite.tw boot/tests/fixtures/cfg/sound_uniqueness/field_dict_alias_old.tw boot/tests/fixtures/cfg/sound_uniqueness/field_vector_alias_old.tw
git commit -m "codegen(8H): produce explicit field-backed collection decisions"
```

---

## Task 6: Emit local field-backed dict/vector helpers and preserve alias semantics

**Files:** Modify `boot/tests/suites/field_backed_collection_suite.tw`.

**Interfaces:**
- Consumes: Task 5 field-backed call decisions.
- Verifies: existing call selector emits `dict$set_in_place` / `vector$set_in_place` for accepted field-backed decisions because their `family` remains the real call family.

- [ ] **Step 1: Add WAT helper-call tests.** In the suite:

```twinkle
.test(
  "local field-backed dict update emits dict set in-place",
  fn() Result<Void, String> {
    wat := try compile_fixture_wat("field_dict_update")
    body := try wat_func_body_result(wat, "go")
    try assert.ok(wat_has_instr(body, "rt_dict__set_in_place"), "field dict update must call dict set in-place")
    .Ok({})
  },
)
.test(
  "local field-backed vector update emits vector set in-place",
  fn() Result<Void, String> {
    wat := try compile_fixture_wat("field_vector_update")
    body := try wat_func_body_result(wat, "go")
    try assert.ok(wat_has_instr(body, "rt_arr__set_in_place"), "field vector update must call vector set in-place")
    .Ok({})
  },
)
```

- [ ] **Step 2: Add alias WAT guards.** In the suite, assert aliased field updates keep the persistent helpers:

```twinkle
.test(
  "field dict alias keeps persistent helper",
  fn() Result<Void, String> {
    wat := try compile_fixture_wat("field_dict_alias_old")
    body := try wat_func_body_result(wat, "go")
    try assert.ok(wat_has_instr(body, "rt_dict__set"), "aliased field dict must call persistent set")
    try assert.ok(!wat_has_instr(body, "rt_dict__set_in_place"), "aliased field dict must stay persistent")
    .Ok({})
  },
)
.test(
  "field vector alias keeps persistent helper",
  fn() Result<Void, String> {
    wat := try compile_fixture_wat("field_vector_alias_old")
    body := try wat_func_body_result(wat, "go")
    try assert.ok(wat_has_instr(body, "rt_arr__set"), "aliased field vector must call persistent set")
    try assert.ok(!wat_has_instr(body, "rt_arr__set_in_place"), "aliased field vector must stay persistent")
    .Ok({})
  },
)
```

The manual verification step for this task runs both alias fixtures and checks their printed outputs, so old-version observability is still tested end-to-end without adding a process-spawning helper to the boot test suite.

- [ ] **Step 3: Run and verify.**

```bash
target/twk fmt boot/tests/suites/field_backed_collection_suite.tw
target/twk lint boot/main.tw
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: positive WAT bodies contain the in-place helpers; alias fixture WAT keeps persistent helpers. Then run these manual commands and confirm outputs are `false` and `0` respectively:

```bash
target/twk run boot/tests/fixtures/cfg/sound_uniqueness/field_dict_alias_old.tw
target/twk run boot/tests/fixtures/cfg/sound_uniqueness/field_vector_alias_old.tw
```

- [ ] **Step 4: Commit.**

```bash
git add boot/tests/suites/field_backed_collection_suite.tw
git commit -m "codegen(8H): emit local record-field collection helpers safely"
```

---

## Task 7: Shell-only and sibling-safety guards

**Files:** Create `field_shell_only.tw`; Create `field_dict_sibling_shared.tw`; Modify `boot/tests/suites/field_backed_collection_suite.tw`.

**Interfaces:**
- Verifies: record-shell decisions remain independent from field-backed call decisions.
- Verifies: a shared sibling field does not block an owned updated field; a shared updated field blocks only the field-backed helper.

- [ ] **Step 1: Create the shell-only fixture.** `field_shell_only.tw`:

```twinkle
pub type Env = .{ types: Dict<Int, Int>, values: Dict<Int, Int> }

pub fn replace_shared_field() Env {
  shared := Dict.new().set(1, 10)
  env := Env.{ types: shared, values: shared }
  fresh := Dict.new().set(2, 20)
  env.types = fresh
  env
}
```

This shape reuses the `Env` shell when the shell is unique, but must not mutate the old `types` backing because `types` and `values` initially share the same dict.

- [ ] **Step 2: Add the shell-only test.**

```twinkle
.test(
  "record shell reuse does not require deep field reuse",
  fn() Result<Void, String> {
    prod := mutable_produce.produce_mutable_decisions((try compile_fixture("field_shell_only")).opt, (try compile_fixture("field_shell_only")).builtins)
    try assert.ok(prod.rows.any(fn(r) { r.family == "record_shell" and r.reason.contains("decision produced") }))
    try assert.ok(!prod.rows.any(fn(r) { r.family.contains("record_backed") and r.reason.contains("decision produced") }))
    wat := try compile_fixture_wat("field_shell_only")
    body := try wat_func_body_result(wat, "replace_shared_field")
    try assert.ok(wat_has_instr(body, "struct.set"), "shell update may reuse the record shell")
    try assert.ok(!wat_has_instr(body, "rt_dict__set_in_place"), "shared field backing must not mutate")
    .Ok({})
  },
)
```

- [ ] **Step 3: Add the sibling-sharing positive fixture and assertions.** Create `field_dict_sibling_shared.tw`:

```twinkle
pub type Env = .{ types: Dict<Int, Int>, values: Dict<Int, Int> }

pub fn go() Int {
  shared := Dict.new().set(9, 90)
  owned_types: Dict<Int, Int> = Dict.new()
  env := Env.{ types: owned_types, values: shared }
  env.types[1] = 20
  case env.types.get(1) {
    .Some(v) => v,
    .None => 0,
  }
}
```

Add this test to `field_backed_collection_suite.tw`:

```twinkle
.test(
  "shared sibling field does not block owned updated field",
  fn() Result<Void, String> {
    wat := try compile_fixture_wat("field_dict_sibling_shared")
    body := try wat_func_body_result(wat, "go")
    try assert.ok(
      wat_has_instr(body, "rt_dict__set_in_place"),
      "owned types field should set in-place even when values sibling is shared",
    )
    sites := ir_sites_text("field_dict_sibling_shared")
    try assert.str_contains(sites, "record_backed_dict")
    .Ok({})
  },
)
```

This proves sibling sharing is not treated as whole-record sharing.

- [ ] **Step 4: Run and verify.**

```bash
target/twk fmt boot/tests/suites/field_backed_collection_suite.tw boot/tests/fixtures/cfg/sound_uniqueness/field_shell_only.tw boot/tests/fixtures/cfg/sound_uniqueness/field_dict_sibling_shared.tw
target/twk lint boot/main.tw
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: shell-only fixture emits `struct.set` without dict/vector in-place helper; sibling-sharing positive still emits the helper for the owned field.

- [ ] **Step 5: Commit.**

```bash
git add boot/tests/suites/field_backed_collection_suite.tw boot/tests/fixtures/cfg/sound_uniqueness/field_shell_only.tw boot/tests/fixtures/cfg/sound_uniqueness/field_dict_sibling_shared.tw
git commit -m "codegen(8H): guard shell-only and sibling-safe field lowering"
```

---

## Task 8: Recursive variant field-backed emission

**Files:** Modify `boot/compiler/codegen/variant_specialize.tw`; Modify `boot/tests/suites/field_backed_collection_suite.tw`.

**Interfaces:**
- Consumes: Tasks 2–3 path-granular variants/seeds and Task 5 field-backed producer.
- Verifies: 8G clone routing plus 8H field decisions unlock recursive record-field vector mutation.

- [ ] **Step 1: Add the failing cloned-field WAT test.**

```twinkle
.test(
  "recursive owned record-field clone emits vector set in-place",
  fn() Result<Void, String> {
    wat := try compile_fixture_wat("field_visit_rec")
    body := try wat_func_body_result(wat, "visit_v")
    try assert.ok(wat_has_instr(body, "rt_arr__set_in_place"), "owned field clone must update xs in place")
    routes := ir_sites_text("field_visit_rec")
    try assert.str_contains(routes, "variant routes")
    try assert.str_contains(routes, "record_backed_vector")
    .Ok({})
  },
)
```

Implement `ir_sites_text(name)` using the same compile/render path as `commands.ir.render_census_report(artifacts, true)`.

- [ ] **Step 2: Run and verify the red state.**

```bash
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected failure modes before the fix: no clone is considered updatable for field-backed sites, or the clone lacks the field-backed decision row.

- [ ] **Step 3: Ensure `updatable_funcs` includes field-backed candidates.** In `variant_specialize.tw`, extend `updatable_funcs`:

```twinkle
field_set := mutable_produce.field_backed_collection_candidates(anf, b)
for c in field_set.candidates { out[c.func_id.id] = true }
```

This is a structural filter only. It must not prove ownership and must not inspect verdicts.

- [ ] **Step 4: Ensure `--census --sites` audits the specialized module consistently.** If `render_census_report` still audits unspecialized `artifacts.opt`, change the sites path to mirror `link_program`: builder rewrite, variant specialize, then seeded mutable production over the specialized ANF for the mutable decision audit. Keep the pre-specialization dry-run table as-is or label it clearly; the post-prepare audit is the authority for emitted decisions.

- [ ] **Step 5: Run and verify green plus runtime parity.**

```bash
target/twk fmt boot/compiler/codegen/variant_specialize.tw boot/commands/ir.tw boot/tests/suites/field_backed_collection_suite.tw
target/twk lint boot/main.tw
make quick-bundle-cli
target/twk run boot/tests/main.tw
target/twk run boot/tests/fixtures/cfg/sound_uniqueness/field_visit_rec.tw
TWINKLE_VARIANT_SPECIALIZE=0 target/twk run boot/tests/fixtures/cfg/sound_uniqueness/field_visit_rec.tw
```

Expected: clone body contains `rt_arr__set_in_place`; specialize on/off produces the same output (`3`).

- [ ] **Step 6: Commit.**

```bash
git add boot/compiler/codegen/variant_specialize.tw boot/commands/ir.tw boot/tests/suites/field_backed_collection_suite.tw
git commit -m "codegen(8H): route recursive record-field collection variants"
```

---

## Task 9: Set wrapper and transport-wrapper coverage

**Files:** Create `field_set_wrapper.tw`; Create `field_transport_ctx.tw`; Modify `boot/tests/suites/field_backed_collection_suite.tw`.

**Interfaces:**
- Verifies: `Set<K>` wins through its `entries` dict field with no Set-specific optimizer.
- Verifies: transported `out.ctx` ownership can feed an 8H field-backed update after projection.

- [ ] **Step 1: Create the Set wrapper fixture.** `field_set_wrapper.tw`:

```twinkle
pub fn go() Bool {
  seen: Set<Int> = Set.new()
  seen = seen.insert(1)
  seen.contains(1)
}
```

The compiler should lower `Set.insert` through the ordinary record field `entries` and `Dict.set`; no new Set-specific code is allowed.

- [ ] **Step 2: Create the transport-wrapper fixture.** `field_transport_ctx.tw`:

```twinkle
pub type Ctx = .{ types: Dict<Int, Int>, values: Dict<Int, Int> }
pub type Out = .{ ctx: Ctx, tag: Int }

pub fn pass(ctx: Ctx) Out {
  Out.{ ctx, tag: 1 }
}

pub fn go() Int {
  ctx := Ctx.{ types: Dict.new(), values: Dict.new() }
  out := pass(ctx)
  next := out.ctx
  next.types[2] = 40
  case next.types.get(2) {
    .Some(v) => v + out.tag,
    .None => out.tag,
  }
}

println(go().to_string())
```

The sibling `out.tag` read is allowed; re-reading `out.ctx` after moving it is not.

- [ ] **Step 3: Add WAT and census tests.**

```twinkle
.test(
  "Set.insert uses record-backed dict path without Set-specific lowering",
  fn() Result<Void, String> {
    wat := try compile_fixture_wat("field_set_wrapper")
    body := try wat_func_body_result(wat, "go")
    try assert.ok(wat_has_instr(body, "rt_dict__set_in_place"), "Set.insert should inherit dict in-place through entries")
    sites := ir_sites_text("field_set_wrapper")
    try assert.str_contains(sites, "record_backed_dict")
    .Ok({})
  },
)
.test(
  "transported ctx field update emits through moved payload field",
  fn() Result<Void, String> {
    wat := try compile_fixture_wat("field_transport_ctx")
    body := try wat_func_body_result(wat, "go")
    try assert.ok(wat_has_instr(body, "rt_dict__set_in_place"), "transported ctx.types should set in-place")
    sites := ir_sites_text("field_transport_ctx")
    try assert.str_contains(sites, "record_backed_dict")
    .Ok({})
  },
)
```

The manual verification step runs `field_transport_ctx.tw` and confirms output `41`.

- [ ] **Step 4: Run and verify.**

```bash
target/twk fmt boot/tests/suites/field_backed_collection_suite.tw boot/tests/fixtures/cfg/sound_uniqueness/field_set_wrapper.tw boot/tests/fixtures/cfg/sound_uniqueness/field_transport_ctx.tw
target/twk lint boot/main.tw
make quick-bundle-cli
target/twk run boot/tests/main.tw
target/twk run boot/tests/fixtures/cfg/sound_uniqueness/field_transport_ctx.tw
```

Expected: both fixtures emit dict in-place through field paths; the manual transport-wrapper run prints `41`.

- [ ] **Step 5: Commit.**

```bash
git add boot/tests/suites/field_backed_collection_suite.tw boot/tests/fixtures/cfg/sound_uniqueness/field_set_wrapper.tw boot/tests/fixtures/cfg/sound_uniqueness/field_transport_ctx.tw
git commit -m "codegen(8H): cover Set wrappers and transported ctx fields"
```

---

## Task 10: Inspection, docs, and self-host gate

**Files:** Modify `boot/compiler/codegen/mutable_audit.tw`; Modify `boot/commands/ir.tw`; Modify `docs/plans/sound-uniqueness/codegen/README.md`; Modify `docs/plans/sound-uniqueness/README.md`; Modify `docs/plans/README.md`; Move this plan to `docs/plans/archive/` after completion.

**Interfaces:**
- Produces: `twk ir --census --sites` rows showing selected/persistent field-backed decisions, emitted helper names, field path keys, and proof ids.
- Completes: Phase 8H checklist in codegen README.

- [ ] **Step 1: Add audit coverage for record-backed decisions.** Extend `mutable_audit.audit_prepared_calls` so rows for call decisions with non-empty `field_path_key` render as `record_backed_<catalog family>` and include the field path in the proof or reason column. Do not create separate emit logic.

- [ ] **Step 2: Verify census output manually.**

```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/field_dict_update.tw --census --sites
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/field_visit_rec.tw --census --sites
target/twk wat boot/tests/fixtures/cfg/sound_uniqueness/field_dict_update.tw --func go --calls
target/twk wat boot/tests/fixtures/cfg/sound_uniqueness/field_visit_rec.tw --func visit --calls
```

Expected: census names `record_backed_dict` / `record_backed_vector`, field path keys such as `T:f0`, `MutableSelected`, and `phase8h:` proof ids; WAT call inspection shows the matching in-place helpers only in positive fixtures.

- [ ] **Step 3: Run the full verification gate.**

```bash
target/twk lint boot/main.tw
make bundle-cli
target/twk run boot/tests/main.tw
make stage2
```

Expected: lint reports no new house-rule violations; bundle succeeds; boot suite passes; self-host reaches a fixed point (stage3 equals stage4 by the repository's stage2 comparison output).

- [ ] **Step 4: Update docs.** In `docs/plans/sound-uniqueness/codegen/README.md`, mark Phase 8H done and record:

- field-backed decisions use structured `field_backing_reusable`, not verdict text parsing;
- field-path seeds are codegen-owned and come from exact `VariantId` keys;
- emitted helpers are ordinary vector/dict call swaps with non-empty `field_path_key` audit data;
- shell-only wins remain independent;
- Set wrappers and transported `ctx`/`state` shapes are covered by ordinary field paths.

In `docs/plans/sound-uniqueness/README.md`, move current focus to Phase 8I verification gate. In `docs/plans/README.md`, remove this active plan row if one was added during execution. Move this plan to `docs/plans/archive/2026-07-28-8h-record-backed-field-collections.md`.

- [ ] **Step 5: Commit.**

```bash
git add -A
git commit -m "codegen(8H): record-backed field collection lowering landed"
```

---

## Scope Boundary

8H delivers existing-helper emission for direct record-field collection updates. It does not deliver:

- private mutable collection storage or mutable/transient dict representation;
- typed/unboxed vector storage preservation across record fields;
- vector concat/extend or non-empty builder-region follow-ups from 8C;
- cross-block quartet reconstruction beyond the ANF-visible first slice;
- Set-specific optimizer rules;
- nested-value mutation for dict/vector element values (`Elem` / `Val` paths);
- full mutual-recursive SCC clone closure beyond the independently cloned peers supported by 8G.

All of those remain storage-representation, builder-region follow-up, analysis-precision, or later codegen work as already documented in the sound-uniqueness track.

---

## Self-review checklist

1. **Spec coverage:** Structured field verdicts (Task 1), path-granular variant publication/selection (Task 2), field-path seeding for variants (Task 3), explicit field-backed candidate and decision production (Tasks 4–5), local dict/vector emission (Task 6), shell-only/sibling safety (Task 7), recursive variant field emission (Task 8), Set/transport coverage (Task 9), inspection/docs/self-host (Task 10). ✓
2. **No text parsing:** Every codegen decision uses `SiteVerdict.field_backing_reusable`, not `verdict.text.contains(...)`. ✓
3. **Type consistency:** `field_seed_paths`, `field_backing_reusable`, `FieldBackedCandidate`, `field_backed_collection_candidates`, and `produce_field_backed_collection_decisions_with_artifacts` are named consistently across tasks. ✓
4. **Fallback safety:** Alias fixtures preserve old-version observability; duplicate call-site decisions are explicitly suppressed; shell-only fixture proves record-shell reuse does not force deep mutation. ✓
5. **8G composition:** Variant clones gain field seeds from exact `VariantId` requirements and use the normal seeded mutable producer; emit remains variant-unaware. ✓
6. **No stage0 or runtime expansion:** The plan uses existing vector/dict helpers and record shell `struct.set`; no Rust or runtime ABI changes are requested. ✓
