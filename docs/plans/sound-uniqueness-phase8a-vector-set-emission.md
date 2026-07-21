# Sound Uniqueness Phase 8A Vector Set Emission Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Emit the existing `vector$set_in_place` helper for the first proven local owned `Vector.set_at` sites, while preserving persistent fallback for every absent, stale, ambiguous, unsupported, or aliased case.

**Architecture:** Keep codegen mechanical: ownership analysis produces ANF-keyed `MutableDecisionTable` entries, the existing selector validates site shape and catalog mapping, and emission selects a mutable callee only when Phase 8A policy allows the exact `VectorSet` family. The first emission slice also closes the deferred 7E decision-rendering gate with a post-prepare backend audit that reports selected, policy-disabled, stale/ignored, and absent fallback states before relying on WAT inspection.

**Tech Stack:** Twinkle boot compiler (`boot/`), `target/twk`, ANF ownership analysis (`compiler.ownership`), backend preparation (`compiler.backend.prepare`), codegen mutable seam (`compiler.codegen.mutable_*`), WAT/call inspection via `target/twk build ... -o /tmp/file.wat` and `target/twk wat ... --func ... --calls`.

## Global Constraints

- Treat the boot compiler in `boot/` as the primary implementation.
- Codegen consumes decisions; it does not re-prove uniqueness, last-use, field ownership, or escape.
- Absence, ambiguity, or staleness of a decision emits the persistent operation.
- Phase 8A may emit only the `VectorSet` family; dicts, builders, record shells, and function variants stay persistent.
- Variant-qualified diagnostics never license rewriting a generic function body.
- After editing `.tw` files, run `target/twk fmt <changed.tw>` and `target/twk lint boot/main.tw`.
- Do not use timing/performance as acceptance evidence for this phase; use tests and IR/WAT/call inspection.

---

## File Structure

- Modify `boot/compiler/codegen/mutable_select.tw`
  - Add an explicit emission policy so Phase 8A can enable only `VectorSet` without enabling dict/record families.
  - Add distinct selection reasons for selected vs policy-disabled mutable targets.
  - Keep the existing `select_call(...)` wrapper persistent-only for current tests and non-emission uses.
- Modify `boot/compiler/codegen/emit/context.tw`
  - Thread the mutable emission policy through `EmitCtx`.
- Modify `boot/compiler/backend/prepare.tw`
  - Carry the mutable emission policy in `PreparedModule` next to `mutable_decisions`.
  - Keep `prepare_backend(...)` persistent-only by default.
- Modify `boot/compiler/codegen/emit/mutable_sites.tw`
  - Pass `ctx.mutable_emit_policy` into the selector for call sites.
- Modify `boot/compiler/codegen/emit.tw`
  - Initialize `EmitCtx.mutable_emit_policy` from `PreparedModule`.
- Create `boot/compiler/codegen/ownership_verdicts.tw`
  - Share the current dry-run ownership verdict extraction between dry-run rendering and the new decision producer.
- Modify `boot/compiler/codegen/dry_run.tw`
  - Use `ownership_verdicts.update_verdicts(...)` instead of keeping a private duplicate analysis runner.
- Create `boot/compiler/codegen/mutable_produce.tw`
  - Build Phase 8A `MutableDecisionTable` entries for accepted local `VectorSet` sites.
  - Exclude loop-contained vector updates so Phase 8B keeps ownership of loop-carried lowering.
- Create `boot/compiler/codegen/mutable_audit.tw`
  - Walk prepared call sites and invoke the same selector inputs used by emission.
  - Render selected, policy-disabled, stale/ignored, and absent fallback decision states for inspection.
- Modify `boot/compiler/codegen/codegen.tw`
  - Use the producer after closure conversion and before backend preparation.
  - Call a new preparation entry point that carries both decisions and the Phase 8A policy.
- Modify `boot/tests/suites/codegen_emit_suite.tw`
  - Add selector-policy tests and WAT emission/fallback tests.
- Create or modify `boot/tests/suites/mutable_produce_suite.tw`
  - Test the producer and renderer without requiring full Wasm emission.
- Create `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw`
  - Positive fixture: a fresh local vector update that is currently reusable.
- Create `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_alias.tw`
  - Negative fixture: an aliased vector update that must stay persistent.
- Create `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw`
  - Negative fixture: a loop-contained fresh vector update that dry-run can mark reusable but Phase 8A must leave persistent.
- Modify `boot/tests/main.tw`
  - Register the new suite if a new file is created.
- Modify `docs/plans/sound-uniqueness/codegen/README.md`
  - Mark the 8A inspection gate and emission slice complete only after the implementation and verification pass.

---

### Task 1: Add an explicit Phase 8A mutable emission policy

**Files:**
- Modify: `boot/compiler/codegen/mutable_select.tw`
- Modify: `boot/compiler/codegen/emit/context.tw`
- Modify: `boot/compiler/backend/prepare.tw`
- Modify: `boot/compiler/codegen/emit/mutable_sites.tw`
- Modify: `boot/compiler/codegen/emit.tw`
- Modify: `boot/tests/suites/codegen_emit_suite.tw`

**Interfaces:**
- Consumes: existing `MutableDecisionTable`, `MutableDecision`, `select_call(...)`, and `select_call_for_emit(...)`.
- Produces:
  - `mutable_select.MutableEmitPolicy`
  - `mutable_select.persistent_only_policy() MutableEmitPolicy`
  - `mutable_select.phase8a_policy() MutableEmitPolicy`
  - `mutable_select.select_call_with_policy(...) CallSelection`
  - `PreparedModule.mutable_emit_policy`
  - `EmitCtx.mutable_emit_policy`

- [ ] **Step 1: Write failing selector-policy tests**

Add tests to `boot/tests/suites/codegen_emit_suite.tw` near the existing mutable selector tests:

```tw
fn test_select_call_with_phase8a_policy_emits_vector_set() Result<Void, String> {
  b := make_builtin_registry()
  set_id := b.id("vector$set_unsafe")
  in_place_id := b.id("vector$set_in_place")
  site := mutable_select.Site.{ func: fid(1), local: lid(3) }
  decisions := mutable_select.empty_decision_table().with_decision(mutable_select.MutableDecision.{
    site,
    family: mutable_select.OperationFamily.VectorSet,
    source_local: lid(2),
    result_local: lid(3),
    arg_count: 3,
    base_arg_index: .Some(0),
    persistent_func: .Some(set_id),
    mutable_func: .Some(in_place_id),
    variant_key: .None,
    field_path_key: "",
    proof_debug_id: "policy-vector-set",
  })

  selected := mutable_select.select_call_with_policy(
    decisions,
    site,
    mutable_select.OperationFamily.VectorSet,
    set_id,
    .Some(0),
    lid(2),
    lid(3),
    3,
    mutable_select.phase8a_policy(),
  )

  try assert.equal(selected.emit_func.id, in_place_id.id)
  try assert.equal(selected.would_func.id, in_place_id.id)
  try assert.equal(
    mutable_select.selection_reason_tag(selected.reason),
    mutable_select.selection_reason_tag(.MutableSelected),
  )
  .Ok({})
}

fn test_select_call_with_phase8a_policy_keeps_dict_set_persistent() Result<Void, String> {
  b := make_builtin_registry()
  set_id := b.method_id("Dict", "set")
  in_place_id := b.id("dict$set_in_place")
  site := mutable_select.Site.{ func: fid(1), local: lid(3) }
  decisions := mutable_select.empty_decision_table().with_decision(mutable_select.MutableDecision.{
    site,
    family: mutable_select.OperationFamily.DictSet,
    source_local: lid(2),
    result_local: lid(3),
    arg_count: 3,
    base_arg_index: .Some(0),
    persistent_func: .Some(set_id),
    mutable_func: .Some(in_place_id),
    variant_key: .None,
    field_path_key: "",
    proof_debug_id: "policy-dict-set",
  })

  selected := mutable_select.select_call_with_policy(
    decisions,
    site,
    mutable_select.OperationFamily.DictSet,
    set_id,
    .Some(0),
    lid(2),
    lid(3),
    3,
    mutable_select.phase8a_policy(),
  )

  try assert.equal(selected.emit_func.id, set_id.id)
  try assert.equal(selected.would_func.id, in_place_id.id)
  try assert.equal(
    mutable_select.selection_reason_tag(selected.reason),
    mutable_select.selection_reason_tag(.PolicyDisabled),
  )
  .Ok({})
}
```

Register both tests in the suite's test list.

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: failure because `MutableEmitPolicy`, `phase8a_policy`, and `select_call_with_policy` do not exist.

- [ ] **Step 3: Implement policy types and policy-aware selection**

In `boot/compiler/codegen/mutable_select.tw`, extend `SelectionReason` with distinct audit states:

```tw
  MutableSelected,
  PolicyDisabled,
```

Update `selection_reason_tag(...)` and any tests that assert old reason tags. Existing persistent-only selector tests that previously expected `.MutableAvailableNotEmitted` for a live would-be mutable target should now expect `.PolicyDisabled`, because the selector found a valid decision but the active policy did not allow emission. Phase 8A emission tests should expect `.MutableSelected`.

Then add:

```tw
pub type MutableEmitPolicy = .{
  emit_vector_set: Bool,
  emit_dict_set: Bool,
  emit_dict_remove: Bool,
  emit_record_shell_update: Bool,
}

pub fn persistent_only_policy() MutableEmitPolicy {
  MutableEmitPolicy.{
    emit_vector_set: false,
    emit_dict_set: false,
    emit_dict_remove: false,
    emit_record_shell_update: false,
  }
}

pub fn phase8a_policy() MutableEmitPolicy {
  MutableEmitPolicy.{
    emit_vector_set: true,
    emit_dict_set: false,
    emit_dict_remove: false,
    emit_record_shell_update: false,
  }
}

fn policy_allows_call(policy: MutableEmitPolicy, family: OperationFamily) Bool {
  case family {
    .VectorSet => policy.emit_vector_set,
    .DictSet => policy.emit_dict_set,
    .DictRemove => policy.emit_dict_remove,
    _ => false,
  }
}
```

Refactor the body of `select_call(...)` into `select_call_with_policy(...)`. Keep `select_call(...)` as a wrapper that passes `persistent_only_policy()` so current Phase 7D tests retain their persistent-only behavior.

At the final `.Some(mf)` branch, use distinct reasons for selected vs policy-disabled paths:

```tw
case d.mutable_func {
  .Some(mf) => if policy.policy_allows_call(expected_family) {
    CallSelection.{
      emit_func: mf,
      would_func: mf,
      reason: .MutableSelected,
    }
  } else {
    CallSelection.{
      emit_func: persistent_fallback,
      would_func: mf,
      reason: .PolicyDisabled,
    }
  },
  .None => reject(.MissingMutableTarget),
}
```

- [ ] **Step 4: Thread policy through preparation and emission contexts**

In `boot/compiler/backend/prepare.tw`, add `mutable_emit_policy: mutable_select.MutableEmitPolicy` to `PreparedModule` and set it to `mutable_select.persistent_only_policy()` in existing `prepare_backend(...)` paths.

Add a new entry point:

```tw
pub fn prepare_backend_with_mutable_config(
  anf: AnfModule,
  env: ResolvedEnv,
  builtins: BuiltinRegistry,
  closure_captures: Dict<Int, Vector<CaptureParam>>,
  mutable_decisions: mutable_select.MutableDecisionTable,
  mutable_emit_policy: mutable_select.MutableEmitPolicy,
) PreparedModule {
  prepared := prepare_backend_with_mutable_decisions(anf, env, builtins, closure_captures, mutable_decisions)
  prepared.mutable_emit_policy = mutable_emit_policy
  prepared
}
```

In every `PreparedModule` literal inside `prepare_backend_with_mutable_decisions(...)`, include:

```tw
mutable_emit_policy: mutable_select.persistent_only_policy(),
```

In `boot/compiler/codegen/emit/context.tw`, add:

```tw
mutable_emit_policy: mutable_select.MutableEmitPolicy,
```

In `boot/compiler/codegen/emit.tw`, set the base context field:

```tw
mutable_emit_policy: prepared.mutable_emit_policy,
```

In `boot/tests/suites/codegen_emit_suite.tw`, update `mk_emit_ctx(...)` with:

```tw
mutable_emit_policy: mutable_select.persistent_only_policy(),
```

Run this search and update every remaining literal, not only the obvious prepare path:

```bash
rg -n "PreparedModule\.\{|EmitCtx\.\{" boot -S
```

Expected current hits include `boot/compiler/codegen/emit.tw` base/per-function contexts, `boot/tests/suites/codegen_emit_suite.tw` helper and hand-built `PreparedModule` values, and `boot/tests/suites/backend_verify_suite.tw` hand-built `PreparedModule` values. Either add the new field to every literal or introduce a local default-constructor helper and update all literals to use it.

- [ ] **Step 5: Use the policy at prepared call sites**

In `boot/compiler/codegen/emit/mutable_sites.tw`, change the selector call from `mutable_select.select_call(...)` to `mutable_select.select_call_with_policy(...)` and pass `ctx.mutable_emit_policy`.

- [ ] **Step 6: Run and format**

Run:

```bash
target/twk fmt boot/compiler/codegen/mutable_select.tw boot/compiler/codegen/emit/context.tw boot/compiler/backend/prepare.tw boot/compiler/codegen/emit/mutable_sites.tw boot/compiler/codegen/emit.tw boot/tests/suites/codegen_emit_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

Expected: lint reports no blocking parser/type issues; the boot test suite passes. Existing tests that asserted persistent emission with hand-built vector decisions must still pass because their prepared modules use the persistent-only policy.

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/codegen/mutable_select.tw boot/compiler/codegen/emit/context.tw boot/compiler/backend/prepare.tw boot/compiler/codegen/emit/mutable_sites.tw boot/compiler/codegen/emit.tw boot/tests/suites/codegen_emit_suite.tw
git commit -m "Add mutable emission policy for phase 8A"
```

---

### Task 2: Extract reusable ownership verdict collection for decision production

**Files:**
- Create: `boot/compiler/codegen/ownership_verdicts.tw`
- Modify: `boot/compiler/codegen/dry_run.tw`
- Modify: `boot/tests/suites/dry_run_suite.tw`

**Interfaces:**
- Consumes: `cfg.build_view`, `ownership.prune_dead_merge`, `summary.compute`, `ownership.analyze_with_summaries`.
- Produces:
  - `ownership_verdicts.SiteVerdict`
  - `ownership_verdicts.update_verdicts(opt: AnfModule, b: BuiltinRegistry, sem: OptimizerSemantics) Dict<String, SiteVerdict>`

- [ ] **Step 1: Write/adjust dry-run regression test**

In `boot/tests/suites/dry_run_suite.tw`, keep the existing assertions that `dry_run.render_dry_run(...)` includes the header:

```tw
try assert.is_true(with_sites.contains("func\tfamily\tpersistent\tmutable\twould_use\tverdict"))
```

Add a focused regression that calls `dry_run.dry_run_sites(...)` for a local owned vector set and asserts the site still reports `would_use == true`. Use the existing source pattern from the suite's owned vector test.

- [ ] **Step 2: Run test to verify current behavior before extraction**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: current dry-run tests pass before refactoring.

- [ ] **Step 3: Move verdict collection into a shared module**

Create `boot/compiler/codegen/ownership_verdicts.tw`:

```tw
//! Shared ownership-verdict extraction for codegen dry-runs and mutable decision production.

use compiler.anf.{AnfModule}
use compiler.builtins.{BuiltinRegistry}
use compiler.cfg
use compiler.opt.semantics.{OptimizerSemantics}
use compiler.ownership
use compiler.summary

pub type SiteVerdict = .{ text: String, reusable_shell: Bool }

pub fn update_verdicts(
  opt: AnfModule,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
) Dict<String, SiteVerdict> {
  view := cfg.build_view(opt, b)
  view = ownership.prune_dead_merge(view)
  table := summary.compute(view, b, sem)
  analyzed := ownership.analyze_with_summaries(view, b, sem, table)

  out: Dict<String, SiteVerdict> = Dict.new()
  for f in analyzed.functions {
    for blk in f.blocks {
      for local_id, text in blk.exit.verdicts {
        reusable := case blk.exit.verdict_reusable_shell.get(local_id) {
          .Some(flag) => flag,
          .None => false,
        }
        out["${f.func_id}#${local_id}"] = SiteVerdict.{ text, reusable_shell: reusable }
      }
    }
  }
  out
}
```

Modify `boot/compiler/codegen/dry_run.tw` to import `compiler.codegen.ownership_verdicts`, delete its private `SiteVerdict` and `collect_verdicts(...)`, and call:

```tw
verdicts := ownership_verdicts.update_verdicts(opt, b, sem)
```

- [ ] **Step 4: Run and format**

Run:

```bash
target/twk fmt boot/compiler/codegen/ownership_verdicts.tw boot/compiler/codegen/dry_run.tw boot/tests/suites/dry_run_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

Expected: dry-run output is unchanged and the boot test suite passes.

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/codegen/ownership_verdicts.tw boot/compiler/codegen/dry_run.tw boot/tests/suites/dry_run_suite.tw
git commit -m "Share ownership verdict extraction for mutable codegen"
```

---

### Task 3: Produce Phase 8A vector-set decisions and reject non-8A shapes

**Files:**
- Create: `boot/compiler/codegen/mutable_produce.tw`
- Create: `boot/tests/suites/mutable_produce_suite.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_alias.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw`
- Modify: `boot/tests/main.tw`

**Interfaces:**
- Consumes: `ownership_verdicts.update_verdicts(...)`, `mutable_catalog.build(...)`, optimized closure-converted `AnfModule`.
- Produces:
  - `mutable_produce.ProducedDecisions`
  - `mutable_produce.DecisionRenderRow`
  - `mutable_produce.produce_phase8a_vector_set_decisions(opt: AnfModule, b: BuiltinRegistry) ProducedDecisions`
  - `mutable_produce.render_candidate_rows(rows: Vector<DecisionRenderRow>) String`

- [ ] **Step 1: Add fixture files and write failing producer tests**

Create `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw`. The top-level `println(...)` is intentional: full linked WAT runs linker DCE, so the fixture must root the function body being inspected.

```tw
fn set_fresh() Vector<Int> {
  xs := [1, 2]
  xs[0] = 9
  xs
}

println(set_fresh().len().to_string())
```

Create `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_alias.tw`. The top-level call roots the fixture through linked DCE.

```tw
fn set_alias(xs: Vector<Int>) Vector<Int> {
  old := xs
  xs[0] = 9
  n := old.len()
  if n >= 0 { xs } else { xs }
}

println(set_alias([1, 2]).len().to_string())
```

Create `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw`. The top-level call roots the fixture through linked DCE.

```tw
fn loop_fresh() Vector<Int> {
  xs := [1, 2]
  i := 0
  for i < 2 {
    xs[i] = i
    i = i + 1
  }
  xs
}

println(loop_fresh().len().to_string())
```

Create `boot/tests/suites/mutable_produce_suite.tw` with tests for those fixture files. Compile fixtures through `pipeline.compile_entry_path(...)`, run closure conversion with `convert_closures(artifacts.opt, artifacts.env)`, and pass `closure_conversion.anf` into the producer so tests exercise the same ANF stage as normal emission.

The owned test should assert:

```tw
produced := mutable_produce.produce_phase8a_vector_set_decisions(closure_conversion.anf, artifacts.builtins)
try assert.equal(produced.table.by_site.keys().len(), 1)
rendered := mutable_produce.render_candidate_rows(produced.rows)
try assert.str_contains(rendered, "vector_set")
try assert.str_contains(rendered, "candidate")
try assert.str_contains(rendered, "decision produced")
try assert.str_contains(rendered, "vector$set_unsafe")
try assert.str_contains(rendered, "vector$set_in_place")
```

The aliased test should assert:

```tw
produced := mutable_produce.produce_phase8a_vector_set_decisions(closure_conversion.anf, artifacts.builtins)
try assert.equal(produced.table.by_site.keys().len(), 0)
rendered := mutable_produce.render_candidate_rows(produced.rows)
try assert.str_contains(rendered, "vector_set")
try assert.str_contains(rendered, "ignored")
try assert.str_contains(rendered, "persistent fallback")
```

Add a loop-contained fixture test that asserts no decision is produced even if the ownership verdict is reusable:

```tw
produced := mutable_produce.produce_phase8a_vector_set_decisions(closure_conversion.anf, artifacts.builtins)
try assert.equal(produced.table.by_site.keys().len(), 0)
rendered := mutable_produce.render_candidate_rows(produced.rows)
try assert.str_contains(rendered, "loop-contained candidate deferred to Phase 8B")
```

Register the suite in `boot/tests/main.tw`.

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: failure because `mutable_produce` does not exist.

- [ ] **Step 3: Implement the producer**

Create `boot/compiler/codegen/mutable_produce.tw`:

```tw
//! Phase 8A mutable-decision producer.
//!
//! Produces backend decisions from already-computed ownership verdicts. This module
//! does not prove ownership; it joins ANF update sites with analysis verdict flags
//! and the mutable operation catalog.

use compiler.anf.{AnfExpr, AnfModule, AnfOp, Atom}
use compiler.builtins.{BuiltinRegistry}
use compiler.codegen.mutable_catalog
use compiler.codegen.mutable_select
use compiler.codegen.ownership_verdicts
use compiler.core_ir.{FuncId, LocalId}
use compiler.opt.semantics.{make_prelude_optimizer_semantics}

pub type DecisionRenderRow = .{
  func: String,
  local: Int,
  family: String,
  persistent: String,
  mutable: String,
  state: String,
  reason: String,
  proof: String,
}

pub type ProducedDecisions = .{
  table: mutable_select.MutableDecisionTable,
  rows: Vector<DecisionRenderRow>,
}

type VectorSetCandidate = .{
  func_id: FuncId,
  func: String,
  result: LocalId,
  base: LocalId,
  arg_count: Int,
  persistent_func: FuncId,
  mutable_func: FuncId,
  loop_depth: Int,
}

fn atom_local(a: Atom) LocalId? {
  case a {
    .ALocal(l) => .Some(l),
    _ => .None,
  }
}
```

Implement `produce_phase8a_vector_set_decisions(...)` so it:

1. Builds `sem := make_prelude_optimizer_semantics(b)` and `cat := mutable_catalog.build(b, sem)`.
2. Calls `ownership_verdicts.update_verdicts(opt, b, sem)`.
3. Walks each function's ANF body with an explicit `loop_depth` parameter. Increment it when entering `.ALoop(body)` and preserve it through `.AIf`, `.AMatch`, and `.ADefer` children.
4. Collects only `VectorSet` calls whose catalog entry has a mutable target and whose base argument is an `ALocal`.
5. Looks up the verdict key with `"${func_id.id}#${result.id}"`.
6. Adds a decision only when `verdict.reusable_shell == true` and `candidate.loop_depth == 0`.
7. Adds a render row for every candidate: `candidate` when a decision is produced, `ignored` when ownership is not reusable, and `deferred` when a reusable candidate is loop-contained and belongs to Phase 8B.

The produced decision must use:

```tw
mutable_select.MutableDecision.{
  site: mutable_select.Site.{ func: candidate.func_id, local: candidate.result },
  family: mutable_select.OperationFamily.VectorSet,
  source_local: candidate.base,
  result_local: candidate.result,
  arg_count: candidate.arg_count,
  base_arg_index: .Some(0),
  persistent_func: .Some(candidate.persistent_func),
  mutable_func: .Some(candidate.mutable_func),
  variant_key: .None,
  field_path_key: "",
  proof_debug_id: "phase8a:${candidate.func}:L${candidate.result.id}",
}
```

The ignored row for an aliased or unavailable verdict must use:

```tw
DecisionRenderRow.{
  func: candidate.func,
  local: candidate.result.id,
  family: "vector_set",
  persistent: "vector$set_unsafe",
  mutable: "vector$set_in_place",
  state: "ignored",
  reason: "persistent fallback: ownership verdict did not certify reusable base",
  proof: verdict_text,
}
```

Implement `render_candidate_rows(...)` as a tab-separated producer-candidate table. This is not the backend-consumption audit table; it only explains which raw producer candidates did or did not become decisions.

```tw
pub fn render_candidate_rows(rows: Vector<DecisionRenderRow>) String {
  out := "func\tlocal\tfamily\tpersistent\tmutable\tproducer_state\treason\tproof\n"
  for r in rows {
    out = .concat("${r.func}\tL${r.local}\t${r.family}\t${r.persistent}\t${r.mutable}\t${r.state}\t${r.reason}\t${r.proof}\n")
  }
  out
}
```

- [ ] **Step 4: Run and format**

Run:

```bash
target/twk fmt boot/compiler/codegen/mutable_produce.tw boot/tests/suites/mutable_produce_suite.tw boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_alias.tw boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw boot/tests/main.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

Expected: producer tests pass; no emitted-code behavior changes yet because normal codegen still uses `prepare_backend(...)` with an empty table and persistent-only policy.

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/codegen/mutable_produce.tw boot/tests/suites/mutable_produce_suite.tw boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_alias.tw boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw boot/tests/main.tw
git commit -m "Produce phase 8A vector set decisions"
```

---

### Task 4: Wire Phase 8A decisions into normal codegen without emitting other families

**Files:**
- Modify: `boot/compiler/codegen/codegen.tw`
- Modify: `boot/tests/suites/codegen_emit_suite.tw`
- Test fixtures from Task 3: `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw`, `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_alias.tw`, `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw`

**Interfaces:**
- Consumes: `mutable_produce.produce_phase8a_vector_set_decisions(...)`, `prepare_backend_with_mutable_config(...)`, `mutable_select.phase8a_policy()`.
- Produces: normal `link_program(...)` builds can emit `vector$set_in_place` for selected Phase 8A decisions.

- [ ] **Step 1: Write failing WAT tests for positive and negative Phase 8A emission**

Add tests to `boot/tests/suites/codegen_emit_suite.tw` that compile through the full `codegen.link_program(...)` path, not the hand-built `prepare_backend_with_mutable_decisions(...)` test path.

Positive fixture: `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw`.

Expected WAT call-site assertions:

```tw
wat := compile_fixture_to_linked_wat("boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw")
try assert.is_true(wat_has_call(wat, "rt_arr__set_in_place"))
try assert.is_false(wat_has_call(wat, "rt_arr__set"))
```

Negative aliasing fixture: `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_alias.tw`.

Expected WAT call-site assertions:

```tw
wat := compile_fixture_to_linked_wat("boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_alias.tw")
try assert.is_true(wat_has_call(wat, "rt_arr__set"))
try assert.is_false(wat_has_call(wat, "rt_arr__set_in_place"))
```

Negative loop fixture: `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw`.

Expected WAT call-site assertions:

```tw
wat := compile_fixture_to_linked_wat("boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw")
try assert.is_true(wat_has_call(wat, "rt_arr__set"))
try assert.is_false(wat_has_call(wat, "rt_arr__set_in_place"))
```

If there is no existing helper for linked WAT in the suite, add:

```tw
fn compile_fixture_to_linked_wat(path: String) Result<String, String> {
  artifacts := try pipeline.compile_entry_path(path)
  linked := codegen.link_program(artifacts.opt, artifacts.env, artifacts.builtins)
  .Ok(emit_wat(linked))
}

fn wat_has_call(wat: String, target: String) Bool {
  needle := "call $${target}"
  for line in wat.lines() {
    if line.trim() == needle {
      return true
    }
  }
  false
}
```

Use `wat_has_call(...)` for token-exact call-site assertions so tests do not confuse a runtime function definition such as `(func $rt_arr__set ...)` with an actual emitted call, and so `call $rt_arr__set_in_place` does not prefix-match `call $rt_arr__set`.

- [ ] **Step 2: Run tests to verify the positive case fails**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: the positive WAT test fails because `link_program(...)` still calls `prepare_backend(...)` with no decisions and persistent-only policy.

- [ ] **Step 3: Wire producer into `link_program(...)`**

In `boot/compiler/codegen/codegen.tw`, change the imports:

```tw
use compiler.backend.prepare.{prepare_backend_with_mutable_config}
use compiler.codegen.mutable_produce
use compiler.codegen.mutable_select
```

Replace the backend preparation call:

```tw
produced_mutable := mutable_produce.produce_phase8a_vector_set_decisions(
  closure_conversion.anf,
  builtins,
)
prepared := prepare_backend_with_mutable_config(
  closure_conversion.anf,
  env,
  builtins,
  closure_conversion.captures,
  produced_mutable.table,
  mutable_select.phase8a_policy(),
)
```

Do not thread `produced_mutable.rows` into emission in this task. The rows are for inspection and tests; emission consumes only `PreparedModule.mutable_decisions` plus `PreparedModule.mutable_emit_policy`.

- [ ] **Step 4: Run and inspect emitted call targets**

Run:

```bash
target/twk fmt boot/compiler/codegen/codegen.tw boot/tests/suites/codegen_emit_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
target/twk wat boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw --func set_fresh --calls >/tmp/phase8a-positive.calls
target/twk wat boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_alias.tw --func set_alias --calls >/tmp/phase8a-negative-alias.calls
target/twk wat boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw --func loop_fresh --calls >/tmp/phase8a-negative-loop.calls
rg '^\s*call \$rt_arr__set$|^\s*call \$rt_arr__set_in_place$' /tmp/phase8a-positive.calls /tmp/phase8a-negative-alias.calls /tmp/phase8a-negative-loop.calls
```

Expected:
- Positive fixture call list contains `call $rt_arr__set_in_place` and not `call $rt_arr__set`.
- Aliased fixture call list contains `call $rt_arr__set` and not `call $rt_arr__set_in_place`.
- Loop-contained fixture call list contains `call $rt_arr__set` and not `call $rt_arr__set_in_place`.
- Boot tests pass.

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/codegen/codegen.tw boot/tests/suites/codegen_emit_suite.tw
git commit -m "Emit owned local vector updates in place"
```

---

### Task 5: Audit post-prepare decision consumption in inspection output

**Files:**
- Create: `boot/compiler/codegen/mutable_audit.tw`
- Modify: `boot/commands/ir.tw`
- Modify: `boot/tests/suites/uniqueness_census_suite.tw`
- Modify: `boot/tests/suites/mutable_produce_suite.tw`

**Interfaces:**
- Consumes: `PreparedModule.mutable_decisions`, `PreparedModule.mutable_emit_policy`, `mutable_sites.select_call_for_emit(...)`, and the same closure-converted/prepared ANF shape used by emission.
- Produces:
  - `mutable_audit.AuditRow`
  - `mutable_audit.audit_prepared_calls(prepared: PreparedModule, env: ResolvedEnv, builtins: BuiltinRegistry) Vector<AuditRow>`
  - `mutable_audit.render_audit_rows(rows: Vector<AuditRow>) String`

- [ ] **Step 1: Confirm the `--census --sites` renderer entry point**

Run:

```bash
rg -n "render_census_report|render_dry_run|--sites|census" boot/commands/ir.tw boot/main.tw -S
```

Expected: `boot/commands/ir.tw:render_census_report(...)` appends `dry_run.render_dry_run(...)` when `include_sites` is true. Keep that existing pre-closure dry-run table unchanged, and add a separate post-prepare `mutable decisions` section below it.

- [ ] **Step 2: Write failing audit tests**

In `boot/tests/suites/mutable_produce_suite.tw`, add tests that compile fixtures through the same stages as emission:

```tw
artifacts := try pipeline.compile_entry_path("boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw")
cc := convert_closures(artifacts.opt, artifacts.env)
produced := mutable_produce.produce_phase8a_vector_set_decisions(cc.anf, artifacts.builtins)
prepared := prepare_backend_with_mutable_config(
  cc.anf,
  artifacts.env,
  artifacts.builtins,
  cc.captures,
  produced.table,
  mutable_select.phase8a_policy(),
)
rows := mutable_audit.audit_prepared_calls(prepared, artifacts.env, artifacts.builtins)
rendered := mutable_audit.render_audit_rows(rows)
try assert.str_contains(rendered, "selected")
try assert.str_contains(rendered, "vector$set_unsafe")
try assert.str_contains(rendered, "vector$set_in_place")
```

Add a stale-decision fallback test by taking the produced table for the fresh fixture, adding or substituting a deliberately mismatched decision at the same site (for example `arg_count: 99` or `source_local: LocalId.{ id: 999999 }`), preparing with that stale table, and asserting the audit row reports a fallback reason such as `ArgumentShapeMismatch` or `SourceLocalMismatch` and `emit_func` remains `vector$set_unsafe`.

Add an explicit policy-disabled audit test: prepare the valid fresh-fixture vector decision table with `mutable_select.persistent_only_policy()` instead of `phase8a_policy()`, run `mutable_audit.audit_prepared_calls(...)`, and assert the rendered audit includes `policy_disabled`, `vector$set_unsafe`, `vector$set_in_place`, and an emit target of `vector$set_unsafe`. This proves the audit distinguishes “valid decision found but policy blocked emission” from both stale rejection and selected emission.

In `boot/tests/suites/uniqueness_census_suite.tw`, add a CLI-level assertion for the new audit header:

```tw
try assert.is_true(with_sites.contains("func\tlocal\tfamily\tpersistent\tmutable\temit\tstate\treason\tproof"))
```

Keep controlled `selected`, `ignored`, `stale`, and `policy-disabled` content assertions in `mutable_produce_suite.tw`, where fixture inputs are deterministic.

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: failure because `mutable_audit` and the new inspection section do not exist yet.

- [ ] **Step 4: Implement the post-prepare audit walker**

Create `boot/compiler/codegen/mutable_audit.tw`. It must walk `prepared.funcs`, recurse through each `PreparedExpr`, and inspect `.Let(result, .ACall(.AGlobalFunc(fid), args), body)` nodes. Define a local `empty_registry() WasmTypeRegistry` helper using the same field defaults as `boot/compiler/backend/prepare.tw` or factor a shared helper if one already exists by implementation time.

Do not import the test-only `local_map_from_slots(...)` helper from `boot/tests/suites/codegen_emit_suite.tw`. Define the production audit helper directly in `mutable_audit.tw`:

```tw
fn local_map_from_slots(slots: Dict<Int, SlotInfo>) Dict<Int, LocalEntry> {
  out: Dict<Int, LocalEntry> = Dict.new()
  for key, info in slots {
    out[key] = LocalEntry.{
      index: key,
      val_type: info.wasm_type,
      mono: info.mono,
      repr: info.repr,
      source_local: info.source_local,
    }
  }
  out
}
```

Import the production types this helper needs from `compiler.backend.prepared_ir` and `compiler.codegen.emit.context`:

```tw
use compiler.backend.prepared_ir.{PreparedExpr, PreparedOp, SlotInfo}
use compiler.codegen.emit.context.{EmitCtx, LocalEntry}
```

For each recognized mutable-catalog call family, build the same minimal `EmitCtx` inputs that emission uses:

```tw
local_map := local_map_from_slots(pf.slots)
ctx := EmitCtx.{
  registry: empty_registry(),
  builtins,
  env,
  local_map,
  next_local: 0,
  wasm_locals: [],
  label_stack: [],
  current_func_id: .Some(pf.func_id),
  current_pf: .Some(pf),
  func_sym_map: Dict.new(),
  prepared_funcs: Dict.new(),
  extern_imports: Dict.new(),
  is_init_func: false,
  loop_depth: 0,
  scratch_anyref_local: -1,
  intrinsic_table: Dict.new(),
  mutable_decisions: prepared.mutable_decisions,
  mutable_emit_policy: prepared.mutable_emit_policy,
}
```

Use `mutable_sites.select_call_for_emit(prepared.mutable_decisions, fid, args, result_entry, ctx)`. Derive audit state from the returned selector:

- `selected` when `selection.reason == .MutableSelected` and `emit_func.id == would_func.id`.
- `policy_disabled` when `selection.reason == .PolicyDisabled`.
- `stale_or_ignored` for mismatch reasons such as `ArgumentShapeMismatch`, `SourceLocalMismatch`, `WrongPersistentTarget`, `Ambiguous`, or `MissingMutableTarget`.
- `absent_fallback` when no decision exists for a recognized call site.

Render a tab-separated table:

```tw
pub fn render_audit_rows(rows: Vector<AuditRow>) String {
  out := "func\tlocal\tfamily\tpersistent\tmutable\temit\tstate\treason\tproof\n"
  for r in rows {
    out = .concat("${r.func}\tL${r.local}\t${r.family}\t${r.persistent}\t${r.mutable}\t${r.emit}\t${r.state}\t${r.reason}\t${r.proof}\n")
  }
  out
}
```

- [ ] **Step 5: Add the audit table to `twk ir --census --sites` using the emission ANF stage**

In `boot/commands/ir.tw:render_census_report(...)`, keep the existing census tally and dry-run table over `artifacts.opt`. For the new audit section, run the same closure conversion and preparation shape as `codegen.link_program(...)`:

```tw
cc := convert_closures(artifacts.opt, artifacts.env)
produced := mutable_produce.produce_phase8a_vector_set_decisions(cc.anf, artifacts.builtins)
prepared := prepare_backend_with_mutable_config(
  cc.anf,
  artifacts.env,
  artifacts.builtins,
  cc.captures,
  produced.table,
  mutable_select.phase8a_policy(),
)
audit_rows := mutable_audit.audit_prepared_calls(prepared, artifacts.env, artifacts.builtins)
out = out.concat("\nmutable decisions\n")
out = out.concat(mutable_audit.render_audit_rows(audit_rows))
```

This makes inspection and emission consume decisions over the same closure-converted/prepared site shape. The old dry-run ownership table can remain pre-closure because it is candidate context, not the backend-consumption audit.

- [ ] **Step 6: Run inspection commands manually**

Run:

```bash
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw >/tmp/phase8a-sites-positive.txt
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_alias.tw >/tmp/phase8a-sites-alias.txt
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw >/tmp/phase8a-sites-loop.txt
rg "mutable decisions|selected|policy_disabled|stale_or_ignored|absent_fallback|vector_set" /tmp/phase8a-sites-positive.txt /tmp/phase8a-sites-alias.txt /tmp/phase8a-sites-loop.txt
```

Expected:
- Fresh fixture audit includes `selected` for `vector_set`.
- Alias fixture audit includes `absent_fallback` or another persistent fallback state, not `selected`.
- Loop fixture audit includes persistent fallback state, not `selected`.
- Stale fallback is covered by the unit test that injects a mismatched decision.

- [ ] **Step 7: Run and format**

Run:

```bash
target/twk fmt boot/compiler/codegen/mutable_audit.tw boot/commands/ir.tw boot/tests/suites/uniqueness_census_suite.tw boot/tests/suites/mutable_produce_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

Expected: boot tests pass and the decision-state renderer proves backend selector consumption, not just producer output.

- [ ] **Step 8: Commit**

```bash
git add boot/compiler/codegen/mutable_audit.tw boot/commands/ir.tw boot/tests/suites/uniqueness_census_suite.tw boot/tests/suites/mutable_produce_suite.tw
git commit -m "Audit phase 8A mutable decision consumption"
```

---

### Task 6: Final Phase 8A verification and docs update

**Files:**
- Modify: `docs/plans/sound-uniqueness/codegen/README.md`
- Modify: `docs/plans/sound-uniqueness/README.md` only if the current-focus paragraph needs a completion note after implementation.

**Interfaces:**
- Consumes: all implemented Phase 8A behavior and inspection evidence.
- Produces: updated docs marking 8A done only when tests and WAT/call inspection pass.

- [ ] **Step 1: Run full correctness verification**

Run:

```bash
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
cargo test --release
```

Expected: all commands exit successfully.

- [ ] **Step 2: Run Phase 8A inspection commands**

Run:

```bash
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw >/tmp/phase8a-census-fresh.txt
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_alias.tw >/tmp/phase8a-census-alias.txt
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw >/tmp/phase8a-census-loop.txt
rg "mutable decisions|vector_set|selected|policy_disabled|stale_or_ignored|absent_fallback" /tmp/phase8a-census-fresh.txt /tmp/phase8a-census-alias.txt /tmp/phase8a-census-loop.txt

target/twk wat boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw --func set_fresh --calls >/tmp/phase8a-positive.calls
target/twk wat boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_alias.tw --func set_alias --calls >/tmp/phase8a-negative-alias.calls
target/twk wat boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw --func loop_fresh --calls >/tmp/phase8a-negative-loop.calls
rg '^\s*call \$rt_arr__set$|^\s*call \$rt_arr__set_in_place$' /tmp/phase8a-positive.calls /tmp/phase8a-negative-alias.calls /tmp/phase8a-negative-loop.calls
```

Expected:
- Census output includes the post-prepare decision-state section.
- Fresh-vector fixture call list contains the in-place helper call.
- Alias and loop-contained fixture call lists stay persistent.

- [ ] **Step 3: Update codegen README checkboxes**

In `docs/plans/sound-uniqueness/codegen/README.md`, mark Phase 8A items done only if Step 1 and Step 2 succeeded:

```md
## Codegen Phase 8A — Local vector indexed-update emission ✅ done
```

In the bullets, state that post-prepare selected/policy-disabled/stale/absent decision rendering was added as the 8A revisit of the deferred 7E item.

- [ ] **Step 4: Run docs/status scan**

Run:

```bash
rg -n "Phase 8A|deferred 7E|selected|policy_disabled|stale_or_ignored|absent_fallback|Next:" docs/plans/sound-uniqueness -S
```

Expected: no stale wording says Phase 8A is still the next step after it has been completed; deferred 7E variant-routing remains attached to Phase 8G.

- [ ] **Step 5: Commit**

```bash
git add docs/plans/sound-uniqueness/codegen/README.md docs/plans/sound-uniqueness/README.md
git commit -m "Document phase 8A vector emission"
```

---

## Acceptance Checklist

- [ ] `MutableEmitPolicy` prevents Phase 8A from enabling dict, record, builder, or variant families.
- [ ] A real producer creates `MutableDecisionTable` entries for owned local vector indexed updates.
- [ ] Aliased vector updates produce no mutable decision and remain persistent.
- [ ] `twk ir --census --sites` includes post-prepare decision-state rendering that audits selected, policy-disabled, stale/ignored, and absent fallback Phase 8A candidates.
- [ ] Positive WAT/call inspection shows `vector$set_in_place` / `rt_arr__set_in_place` for a selected fresh local owned vector update.
- [ ] Negative WAT/call inspection shows the persistent vector set path for aliasing, loop-contained, stale, or unsupported cases.
- [ ] Full boot tests and Rust tests pass before docs mark Phase 8A done.

## Out of Scope for This Plan

- Loop-carried vector updates; they start in Phase 8B.
- Vector/string builder-region lowering; that starts in Phase 8C.
- Dict set/remove emission; those start in Phase 8D/8E.
- Record shell reuse; that starts in Phase 8F.
- Ownership-specialized function clone routing and variant dry-runs; those start in Phase 8G.
- Storage-representation work; that starts after existing-hook codegen proves the seam.
