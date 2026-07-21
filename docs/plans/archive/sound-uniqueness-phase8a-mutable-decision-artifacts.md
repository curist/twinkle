# Sound Uniqueness Phase 8A Mutable Decision Artifacts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor Phase 8A mutable-decision production so build/reporting paths share ownership artifacts, query only candidate verdicts, and avoid whole-program ownership work when it cannot affect mutable emission.

**Architecture:** Keep the ownership engine as the only proof source and keep backend codegen mechanical. Phase 8A first collects `VectorSet` candidate sites from optimized semantic ANF. Only then does it compute ownership artifacts, and those artifacts carry an optimized-ANF fingerprint; any artifact/input mismatch produces no mutable decisions and therefore persistent fallback. Build may scope final ownership analysis to candidate-bearing functions while summaries are computed for the transitive callee closure needed by those roots.

**Tech Stack:** Twinkle boot compiler (`boot/`), self-hosted CLI (`target/twk`), structural CFG (`compiler.cfg`), summary analysis (`compiler.summary`), ownership analysis (`compiler.ownership`), mutable codegen seam (`compiler.codegen.mutable_*`), deterministic hashing via `lib.query.keys`.

## Global Constraints

- Treat the boot compiler in `boot/` as the primary implementation.
- Codegen consumes decisions; it does not re-prove uniqueness, last-use, field ownership, or escape.
- Absence, ambiguity, staleness, unsupported families, aliases, and any uncomputed proof state emit the persistent operation.
- Phase 8A may emit only the `VectorSet` family; dicts, builders, record shells, and function variants stay persistent.
- Ownership/summary analysis remains defined over optimized semantic ANF, before closure conversion.
- Backend preparation and emission may only validate prepared-site shape against an existing decision.
- Public artifact-fed APIs must validate an optimized-ANF artifact key before using verdicts.
- After editing `.tw` files, run `target/twk fmt <changed.tw>` and `target/twk lint boot/main.tw`.
- Use timing output as regression-investigation evidence only; correctness acceptance still comes from tests, census/audit output, and WAT/call inspection.

---

## Codebase-Verified Corrections (2026-07-21)

These override the illustrative snippets below wherever they conflict. Every symbol was checked against the current boot source.

**C1 — `qkeys.mix_many` does not exist.** `lib.query.keys` exports only `hash_text(text: String) Int` and `mix_word(h: Int, word: Int) Int`. Replace every `qkeys.mix_many([...])` in Task 2 with a local fold helper defined once in `ownership_verdicts.tw`:

```tw
fn mix_all(parts: Vector<Int>) Int {
  h := 0
  for p in parts {
    h = qkeys.mix_word(h, p)
  }
  h
}
```

Then `mix_all([qkeys.hash_text("ALocal"), l.id])` etc. Import as `use lib.query.keys as qkeys`.

**C2 — `SummaryTable` is defined in `compiler.ownership`, not `compiler.summary`** (`pub type SummaryTable = .{ by_func: Dict<Int, Summary> }`). `summary.tw` already imports it *unqualified* (`use compiler.ownership.{ ..., SummaryTable, empty_summary_table, ... }`). Consequences for Task 3:
- `compute_for_roots(...)` returns bare `SummaryTable`, NOT `ownership.SummaryTable`.
- Do NOT add `use compiler.ownership` to `summary.tw` — it would duplicate the existing destructuring import and does not bind a module alias anyway.
- `empty_summary_table()`, `table_put(...)`, `conservative_summary(...)`, `run_scc(...)`, `order_sccs(...)`, `build_func_index(...)`, `user_id_set(...)`, `callee_ids(...)` are all already in scope in `summary.tw`.
- In `ownership_verdicts.tw` (which imports `compiler.ownership` as a module) the qualified form `ownership.SummaryTable` is correct — keep it in `OwnershipArtifacts`.

**C3 — `hash_expr` vs `hash_op` split.** `AnfExpr` has exactly `{ Let(LocalId, AnfOp, AnfExpr), Atom(Atom), Return(Atom?), Break(Atom?), Continue }`. `AIf`/`AMatch`/`ALoop`/`ADefer` are `AnfOp` variants, not `AnfExpr`. So:
- `hash_expr` is exhaustive over the 5 `AnfExpr` variants. `Return`/`Break` carry `Atom?` — case `.Some`/`.None`. `Continue` is a bare tag. `Let` mixes tag + `local.id` + `hash_op(op)` + `hash_expr(body)`.
- `hash_op` is exhaustive over the 19 `AnfOp` variants; `AIf`/`AMatch`/`ALoop`/`ADefer` recurse into `hash_expr`. `AMatch` arms are `AnfMatchArm.{ pattern: CorePattern, body: AnfExpr }`.

**C4 — Concrete leaf-payload encodings** (the plan's "local integer tags" hand-wave). Use exhaustive local helpers so a new enum variant forces a compile error:
- Id types (`LocalId`/`GlobalId`/`FuncId`/`FieldId`/`VariantId`/`TypeId`) are all `.{ id: Int }` — hash via `.id`.
- `Bool` → `if b { 1 } else { 0 }`; `Atom?` → case `.Some`/`.None`.
- `MonoType` → `qkeys.hash_text(mono_type.ty_to_string(ty))` (add `use compiler.mono_type`).
- `OpKind = { Int, Float, Bool, Str }`, `IndexKind = { Array, Dict, Str }`, `UnOp = { Neg, Not }`, `BinOp = { Add, Sub, Mul, Div, Mod, Eq, Ne, Lt, Le, Gt, Ge, And, Or, BitAnd, BitOr, BitXor, Shl, Shr }` — small exhaustive `*_tag(...) Int` helpers.
- `ARecord` carries `Vector<FieldAtom>` where `FieldAtom = .{ field: FieldId, value: Atom }`.

**C5 — Imports for `ownership_verdicts.tw` Task 2.** Beyond the plan's list, add `AnfExpr`, `AnfOp`, `Atom`, `AnfMatchArm`, `FieldAtom`, `OpKind`, `IndexKind` from `compiler.anf`; `BinOp`, `UnOp`, `CorePattern`, and the id types from `compiler.core_ir`; and `use compiler.mono_type`. `cfg`, `ownership`, `summary` are already imported (module form).

**C6 — Confirmed matches (no change):** join key `"${f.func_id}#${local_id}"` (CfgFunction.func_id is `Int`, verdicts keyed by `Int`); `analyze_function(f, table, b, sem, unique_seed)` (private, in `ownership.tw`); `BlockFacts` fields = `{ ownership, binding_valid, live, field_own, verdicts, verdict_reusable_shell }`; `CfgView = .{ functions }`; `render_cfg(source, generic_analyzed, b, sem, table, variants)`; `compute_variants(view, b, sem, generic)`; fixtures `phase8a_vector_set_{fresh,alias,loop}.tw` and suite helpers `fixtures_dir()`/`format_compile_error`/`pipeline.compile_entry_path` all exist.

---

## File Structure

- Modify `boot/compiler/codegen/ownership_verdicts.tw`
  - Own the shared ownership-artifact API.
  - Add `ArtifactKey`, `OwnershipArtifacts`, `VerdictTable`, and optimized-ANF fingerprinting.
  - Keep `update_verdicts(...)` as a compatibility wrapper.
- Modify `boot/compiler/codegen/mutable_produce.tw`
  - Split candidate collection from artifact computation.
  - Add an empty-candidate fast path.
  - Do not expose a raw candidate/verdict mixing API. Expose artifact-fed production only through fingerprint validation.
- Modify `boot/compiler/summary.tw`
  - Add root dependency-closure and root-scoped summary computation.
  - Use the `SummaryTable` type exported by `compiler.ownership`.
- Modify `boot/compiler/ownership.tw`
  - Add selected-function ownership analysis that clears all facts on unselected functions.
- Modify `boot/compiler/codegen/dry_run.tw`
  - Add a fingerprint-validating artifact-fed dry-run path.
- Modify `boot/commands/ir.tw`
  - Reuse a single full artifact computation for `--cfg` and `--census --sites` report generation.
- Modify tests under `boot/tests/suites/`
  - Add artifact freshness, no-candidate, transitive-closure, selected-ownership, producer, dry-run, and reporting checks.
- Add fixtures under `boot/tests/fixtures/sound_uniqueness/`
  - `phase8a_no_vector_set.tw`
  - `phase8a_summary_closure.tw`

---

### Task 1: Candidate-first producer seam and empty-candidate proof

**Files:**
- Modify: `boot/compiler/codegen/mutable_produce.tw`
- Modify: `boot/tests/suites/mutable_produce_suite.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8a_no_vector_set.tw`

**Interfaces:**
- Consumes: existing private `Candidate`, `collect_candidates(...)`, `ProducedDecisions`.
- Produces:
  - `pub type Candidate`
  - `pub type CandidateSet = .{ candidates: Vector<Candidate> }`
  - `pub type ProducedDecisions = .{ table: mutable_select.MutableDecisionTable, rows: Vector<DecisionRenderRow>, artifact_computed: Bool }`
  - `pub fn phase8a_vector_set_candidates(opt: AnfModule, b: BuiltinRegistry) CandidateSet`

- [ ] **Step 1: Add a no-candidate fixture**

Create `boot/tests/fixtures/sound_uniqueness/phase8a_no_vector_set.tw`:

```tw
fn compute(x: Int) Int {
  y := x + 1
  y * 2
}

compute(20)
```

- [ ] **Step 2: Add candidate and no-candidate tests**

In `boot/tests/suites/mutable_produce_suite.tw`, ensure these import lines are present at the top:

```tw
use compiler.codegen.ownership_verdicts
use compiler.opt.semantics as semantics
```

Add tests to the existing suite:

```tw
.test(
  "phase 8A candidate collection is available before ownership analysis",
  fn() {
    artifacts := case pipeline.compile_entry_path("${fixtures_dir()}/phase8a_vector_set_fresh.tw") {
      .Ok(result) => result,
      .Err(err) => return .Err(format_compile_error(err)),
    }
    set := mutable_produce.phase8a_vector_set_candidates(artifacts.opt, artifacts.builtins)
    try assert.equal(set.candidates.len(), 1)
    try assert.equal(set.candidates[0].family, "vector_set")
    try assert.equal(set.candidates[0].loop_depth, 0)
    .Ok({})
  },
)
.test(
  "no phase 8A candidates skip ownership artifact computation",
  fn() {
    artifacts := case pipeline.compile_entry_path("${fixtures_dir()}/phase8a_no_vector_set.tw") {
      .Ok(result) => result,
      .Err(err) => return .Err(format_compile_error(err)),
    }
    set := mutable_produce.phase8a_vector_set_candidates(artifacts.opt, artifacts.builtins)
    try assert.equal(set.candidates.len(), 0)

    produced := mutable_produce.produce_phase8a_vector_set_decisions(
      artifacts.opt,
      artifacts.builtins,
    )
    try assert.equal(produced.table.by_site.keys().len(), 0)
    try assert.equal(produced.rows.len(), 0)
    try assert.equal(produced.artifact_computed, false)
    .Ok({})
  },
)
```

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: failure because `CandidateSet`, public candidate collection, and `artifact_computed` do not exist.

- [ ] **Step 3: Export candidate collection and production stats**

In `boot/compiler/codegen/mutable_produce.tw`, make the existing candidate type public:

```tw
pub type Candidate = .{
  func_id: FuncId,
  func: String,
  result: LocalId,
  base: LocalId,
  arg_count: Int,
  family: String,
  base_arg_index: Int,
  persistent_func: FuncId,
  mutable_func: FuncId,
  loop_depth: Int,
}

pub type CandidateSet = .{ candidates: Vector<Candidate> }
```

Extend `ProducedDecisions`:

```tw
pub type ProducedDecisions = .{
  table: mutable_select.MutableDecisionTable,
  rows: Vector<DecisionRenderRow>,
  artifact_computed: Bool,
}
```

Add:

```tw
pub fn phase8a_vector_set_candidates(opt: AnfModule, b: BuiltinRegistry) CandidateSet {
  sem := make_prelude_optimizer_semantics(b)
  cat := mutable_catalog.build(b, sem)
  CandidateSet.{ candidates: collect_candidates(opt, cat) }
}
```

Update every `ProducedDecisions.{ ... }` constructor in `mutable_produce.tw` and tests to include `artifact_computed`.

- [ ] **Step 4: Add the empty-candidate fast path**

Change `produce_phase8a_vector_set_decisions(...)` so candidate collection happens before ownership artifacts:

```tw
pub fn produce_phase8a_vector_set_decisions(opt: AnfModule, b: BuiltinRegistry) ProducedDecisions {
  set := phase8a_vector_set_candidates(opt, b)
  if set.candidates.len() == 0 {
    return ProducedDecisions.{
      table: mutable_select.empty_decision_table(),
      rows: [],
      artifact_computed: false,
    }
  }

  sem := make_prelude_optimizer_semantics(b)
  artifacts := ownership_verdicts.compute_artifacts(opt, b, sem)
  produce_phase8a_vector_set_decisions_with_artifacts(opt, b, artifacts)
}
```

`produce_phase8a_vector_set_decisions_with_artifacts(...)` is added in Task 2. In this task, temporarily keep the old `update_verdicts(...)` path after the empty-candidate guard, and set `artifact_computed: true` on non-empty candidates.

- [ ] **Step 5: Verify and commit**

```bash
target/twk fmt boot/compiler/codegen/mutable_produce.tw boot/tests/suites/mutable_produce_suite.tw boot/tests/fixtures/sound_uniqueness/phase8a_no_vector_set.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
git add boot/compiler/codegen/mutable_produce.tw boot/tests/suites/mutable_produce_suite.tw boot/tests/fixtures/sound_uniqueness/phase8a_no_vector_set.tw
git commit -m "Collect mutable candidates before ownership analysis"
```

---

### Task 2: Fingerprinted ownership artifacts and stale-artifact fallback

**Files:**
- Modify: `boot/compiler/codegen/ownership_verdicts.tw`
- Modify: `boot/compiler/codegen/mutable_produce.tw`
- Modify: `boot/tests/suites/mutable_produce_suite.tw`

**Interfaces:**
- Consumes: `cfg.build_view`, `ownership.prune_dead_merge`, `summary.compute`, `ownership.analyze_with_summaries`, `lib.query.keys`.
- Produces:
  - `pub type ArtifactKey = .{ anf_hash: Int }`
  - `pub type OwnershipArtifacts = .{ key: ArtifactKey, view: cfg.CfgView, table: ownership.SummaryTable, analyzed: cfg.CfgView }`
  - `pub type VerdictTable = .{ key: ArtifactKey, by_key: Dict<String, SiteVerdict> }`
  - `pub fn artifact_key_for_anf(opt: AnfModule) ArtifactKey`
  - `pub fn compute_artifacts(opt: AnfModule, b: BuiltinRegistry, sem: OptimizerSemantics) OwnershipArtifacts`
  - `pub fn verdicts_from_artifacts(artifacts: OwnershipArtifacts) VerdictTable`
  - `pub fn verdicts_for_keys(artifacts: OwnershipArtifacts, keys: Dict<String, Bool>) VerdictTable`
  - `pub fn same_artifact_key(a: ArtifactKey, b: ArtifactKey) Bool`
  - `pub fn produce_phase8a_vector_set_decisions_with_artifacts(opt: AnfModule, b: BuiltinRegistry, artifacts: ownership_verdicts.OwnershipArtifacts) ProducedDecisions`

- [ ] **Step 1: Add stale-artifact test before exposing artifact-fed production**

Add this test to `boot/tests/suites/mutable_produce_suite.tw`:

```tw
.test(
  "stale ownership artifacts produce no mutable decisions",
  fn() {
    fresh := case pipeline.compile_entry_path("${fixtures_dir()}/phase8a_vector_set_fresh.tw") {
      .Ok(result) => result,
      .Err(err) => return .Err(format_compile_error(err)),
    }
    alias := case pipeline.compile_entry_path("${fixtures_dir()}/phase8a_vector_set_alias.tw") {
      .Ok(result) => result,
      .Err(err) => return .Err(format_compile_error(err)),
    }
    sem_alias := semantics.make_prelude_optimizer_semantics(alias.builtins)
    stale := ownership_verdicts.compute_artifacts(alias.opt, alias.builtins, sem_alias)

    produced := mutable_produce.produce_phase8a_vector_set_decisions_with_artifacts(
      fresh.opt,
      fresh.builtins,
      stale,
    )
    try assert.equal(produced.table.by_site.keys().len(), 0)
    try assert.equal(produced.artifact_computed, false)
    rendered := mutable_produce.render_candidate_rows(produced.rows)
    try assert.str_contains(rendered, "stale ownership artifact")
    .Ok({})
  },
)
```

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: failure because artifact types and artifact-fed production do not exist.

- [ ] **Step 2: Implement optimized-ANF artifact keys**

In `boot/compiler/codegen/ownership_verdicts.tw`, add imports:

```tw
use compiler.anf.{AnfExpr, AnfModule, AnfOp, Atom}
use lib.query.keys as qkeys
```

Add deterministic hash helpers. The implementation must cover every current `Atom`, `AnfExpr`, and `AnfOp` variant; new variants should cause a compiler exhaustiveness error until added.

```tw
pub type ArtifactKey = .{ anf_hash: Int }

pub fn same_artifact_key(a: ArtifactKey, b: ArtifactKey) Bool {
  a.anf_hash == b.anf_hash
}

fn hash_atom(a: Atom) Int {
  case a {
    .ALocal(l) => qkeys.mix_many([qkeys.hash_text("ALocal"), l.id]),
    .AGlobalLocal(g) => qkeys.mix_many([qkeys.hash_text("AGlobalLocal"), g.id]),
    .AGlobalFunc(f) => qkeys.mix_many([qkeys.hash_text("AGlobalFunc"), f.id]),
    .ALitInt(n) => qkeys.mix_many([qkeys.hash_text("ALitInt"), n]),
    .ALitFloat(f) => qkeys.mix_many([qkeys.hash_text("ALitFloat"), qkeys.hash_text(f.to_string())]),
    .ALitBool(v) => qkeys.mix_many([qkeys.hash_text("ALitBool"), if v { 1 } else { 0 }]),
    .ALitStr(s) => qkeys.mix_many([qkeys.hash_text("ALitStr"), qkeys.hash_text(s)]),
    .ALitVoid => qkeys.hash_text("ALitVoid"),
  }
}

fn hash_atoms(atoms: Vector<Atom>) Int {
  parts: Vector<Int> = [qkeys.hash_text("atoms"), atoms.len()]
  for a in atoms {
    parts = .append(hash_atom(a))
  }
  qkeys.mix_many(parts)
}
```

Add `hash_pattern(...)`, `hash_expr(...)`, and `hash_op(...)` in the same style. Cover every variant explicitly. For patterns, import `CorePattern` and use this shape:

```tw
fn hash_pattern(p: CorePattern) Int {
  case p {
    .Wildcard => qkeys.hash_text("PWildcard"),
    .Var(l) => qkeys.mix_many([qkeys.hash_text("PVar"), l.id]),
    .LitInt(n) => qkeys.mix_many([qkeys.hash_text("PLitInt"), n]),
    .LitBool(v) => qkeys.mix_many([qkeys.hash_text("PLitBool"), if v { 1 } else { 0 }]),
    .LitStr(s) => qkeys.mix_many([qkeys.hash_text("PLitStr"), qkeys.hash_text(s)]),
    .Variant(tid, vid, args) => {
      parts: Vector<Int> = [qkeys.hash_text("PVariant"), tid.id, vid.id, args.len()]
      for a in args {
        parts = .append(hash_pattern(a))
      }
      qkeys.mix_many(parts)
    },
  }
}
```

For expressions and ops, include exactly these fields:

- `.Let(local, op, body)`: tag, local id, op hash, body hash.
- `.Atom`, `.Return`, `.Break`, `.Continue`: tag and atom hash when present.
- `.ACall`: callee hash and argument hashes.
- `.AIf`, `.AMatch`, `.ALoop`, `.ADefer`: tag and nested expression hashes; for match arms include arm index, `hash_pattern(arm.pattern)`, and body hash.
- `.ABinOp`, `.AUnOp`: tag plus local integer tags for operator and kind.
- `.AMakeClosure`, `.ARecord`, `.ARecordGet`, `.ARecordUpdate`, `.AVariant`, `.AArrayLit`, `.AIndex`, `.AInit`, `.AAssign`, `.AGlobalSet`, `.AWrapAnyref`, `.AUnwrapAnyref`: tag plus all ids, atom hashes, nested vectors, type ids, field ids, and in-place flags.

Add the module key:

```tw
pub fn artifact_key_for_anf(opt: AnfModule) ArtifactKey {
  parts: Vector<Int> = [qkeys.hash_text("phase8a-artifact-v1"), opt.functions.len()]
  case opt.init_func_id {
    .Some(fid) => parts = .append(qkeys.mix_many([qkeys.hash_text("init"), fid.id])),
    .None => parts = .append(qkeys.hash_text("no-init")),
  }
  for f in opt.functions {
    parts = .append(qkeys.mix_many([
      qkeys.hash_text("func"),
      f.func_id.id,
      qkeys.hash_text(f.name),
      f.params.len(),
      hash_expr(f.body),
    ]))
  }
  ArtifactKey.{ anf_hash: qkeys.mix_many(parts) }
}
```

This key is intentionally tied to optimized semantic ANF. It rejects stale artifacts even when local/function ids happen to match.

- [ ] **Step 3: Implement artifact and verdict table types**

In `ownership_verdicts.tw`, add:

```tw
pub type OwnershipArtifacts = .{
  key: ArtifactKey,
  view: cfg.CfgView,
  table: ownership.SummaryTable,
  analyzed: cfg.CfgView,
}

pub type VerdictTable = .{ key: ArtifactKey, by_key: Dict<String, SiteVerdict> }

pub fn compute_artifacts(
  opt: AnfModule,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
) OwnershipArtifacts {
  view := cfg.build_view(opt, b)
  view = ownership.prune_dead_merge(view)
  table := summary.compute(view, b, sem)
  analyzed := ownership.analyze_with_summaries(view, b, sem, table)
  OwnershipArtifacts.{ key: artifact_key_for_anf(opt), view, table, analyzed }
}
```

Extract verdicts from artifacts, not from a bare analyzed CFG:

```tw
pub fn verdicts_from_artifacts(artifacts: OwnershipArtifacts) VerdictTable {
  out: Dict<String, SiteVerdict> = Dict.new()
  for f in artifacts.analyzed.functions {
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
  VerdictTable.{ key: artifacts.key, by_key: out }
}

pub fn verdicts_for_keys(artifacts: OwnershipArtifacts, keys: Dict<String, Bool>) VerdictTable {
  all := verdicts_from_artifacts(artifacts)
  out: Dict<String, SiteVerdict> = Dict.new()
  for k in keys.keys() {
    case all.by_key.get(k) {
      .Some(v) => out[k] = v,
      .None => {},
    }
  }
  VerdictTable.{ key: artifacts.key, by_key: out }
}
```

Keep compatibility:

```tw
pub fn update_verdicts(
  opt: AnfModule,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
) Dict<String, SiteVerdict> {
  verdicts_from_artifacts(compute_artifacts(opt, b, sem)).by_key
}
```

- [ ] **Step 4: Add candidate key helper and validated artifact-fed production**

In `mutable_produce.tw`, add:

```tw
pub fn candidate_keys(set: CandidateSet) Dict<String, Bool> {
  keys: Dict<String, Bool> = Dict.new()
  for c in set.candidates {
    keys["${c.func_id.id}#${c.result.id}"] = true
  }
  keys
}

fn stale_artifact_rows(candidates: Vector<Candidate>) Vector<DecisionRenderRow> {
  rows: Vector<DecisionRenderRow> = []
  for c in candidates {
    rows = .append(DecisionRenderRow.{
      func: c.func,
      local: c.result.id,
      family: c.family,
      persistent: "fid#${c.persistent_func.id}",
      mutable: "fid#${c.mutable_func.id}",
      state: "ignored",
      reason: "persistent fallback: stale ownership artifact",
      proof: "-",
    })
  }
  rows
}
```

Add the validated public API. This is the only artifact-fed producer API; do not expose a public raw `candidates + verdicts` joiner.

```tw
pub fn produce_phase8a_vector_set_decisions_with_artifacts(
  opt: AnfModule,
  b: BuiltinRegistry,
  artifacts: ownership_verdicts.OwnershipArtifacts,
) ProducedDecisions {
  set := phase8a_vector_set_candidates(opt, b)
  if set.candidates.len() == 0 {
    return ProducedDecisions.{ table: mutable_select.empty_decision_table(), rows: [], artifact_computed: false }
  }

  expected := ownership_verdicts.artifact_key_for_anf(opt)
  if !ownership_verdicts.same_artifact_key(expected, artifacts.key) {
    return ProducedDecisions.{
      table: mutable_select.empty_decision_table(),
      rows: stale_artifact_rows(set.candidates),
      artifact_computed: false,
    }
  }

  vt := ownership_verdicts.verdicts_for_keys(artifacts, candidate_keys(set))
  produce_phase8a_vector_set_decisions_from_validated_verdicts(set.candidates, b, vt.by_key, true)
}
```

Keep the raw join helper private:

```tw
fn produce_phase8a_vector_set_decisions_from_validated_verdicts(
  candidates: Vector<Candidate>,
  b: BuiltinRegistry,
  verdicts: Dict<String, ownership_verdicts.SiteVerdict>,
  artifact_computed: Bool,
) ProducedDecisions {
  table := mutable_select.empty_decision_table()
  rows: Vector<DecisionRenderRow> = []

  for c in candidates {
    key := "${c.func_id.id}#${c.result.id}"
    verdict := case verdicts.get(key) {
      .Some(v) => v,
      .None => ownership_verdicts.SiteVerdict.{ text: "-", reusable_shell: false },
    }
    persistent_name := name_of(b, c.persistent_func)
    mutable_name := name_of(b, c.mutable_func)

    row: DecisionRenderRow = if verdict.reusable_shell and c.loop_depth == 0 {
      table = table.with_decision(mutable_select.MutableDecision.{
        site: mutable_select.Site.{ func: c.func_id, local: c.result },
        family: mutable_select.OperationFamily.VectorSet,
        source_local: c.base,
        result_local: c.result,
        arg_count: c.arg_count,
        base_arg_index: .Some(c.base_arg_index),
        persistent_func: .Some(c.persistent_func),
        mutable_func: .Some(c.mutable_func),
        variant_key: .None,
        field_path_key: "",
        proof_debug_id: "phase8a:${c.func}:L${c.result.id}",
      })
      DecisionRenderRow.{ func: c.func, local: c.result.id, family: c.family, persistent: persistent_name, mutable: mutable_name, state: "candidate", reason: "decision produced", proof: verdict.text }
    } else if verdict.reusable_shell and c.loop_depth > 0 {
      DecisionRenderRow.{ func: c.func, local: c.result.id, family: c.family, persistent: persistent_name, mutable: mutable_name, state: "deferred", reason: "loop-contained candidate deferred to Phase 8B", proof: verdict.text }
    } else {
      DecisionRenderRow.{ func: c.func, local: c.result.id, family: c.family, persistent: persistent_name, mutable: mutable_name, state: "ignored", reason: "persistent fallback: ownership verdict did not certify reusable base", proof: verdict.text }
    }
    rows = rows.append(row)
  }

  ProducedDecisions.{ table, rows, artifact_computed }
}
```

- [ ] **Step 5: Verify and commit**

```bash
target/twk fmt boot/compiler/codegen/ownership_verdicts.tw boot/compiler/codegen/mutable_produce.tw boot/tests/suites/mutable_produce_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
git add boot/compiler/codegen/ownership_verdicts.tw boot/compiler/codegen/mutable_produce.tw boot/tests/suites/mutable_produce_suite.tw
git commit -m "Validate ownership artifact freshness"
```

---

### Task 3: Root-scoped summaries with transitive closure coverage

**Files:**
- Modify: `boot/compiler/summary.tw`
- Modify: `boot/tests/suites/cfg_summary_suite.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8a_summary_closure.tw`

**Interfaces:**
- Consumes: `callee_ids(...)`, `order_sccs(...)`, `run_scc(...)`, `conservative_summary(...)`, `ownership.SummaryTable`.
- Produces:
  - `pub fn dependency_closure(view: CfgView, roots: Dict<Int, Bool>) Dict<Int, Bool>`
  - `pub fn compute_for_roots(view: CfgView, b: BuiltinRegistry, sem: OptimizerSemantics, roots: Dict<Int, Bool>) ownership.SummaryTable`

- [ ] **Step 1: Add a transitive-callee fixture**

Create `boot/tests/fixtures/sound_uniqueness/phase8a_summary_closure.tw`:

```tw
fn helper_b(i: Int) Int {
  i
}

fn helper_a(i: Int) Int {
  helper_b(i)
}

fn candidate_root() Vector<Int> {
  xs := [10, 20]
  idx := helper_a(0)
  xs[idx] = 99
  xs
}

fn unrelated(x: Int) Int {
  x + 1
}

candidate_root().len() + unrelated(1)
```

This fixture intentionally has a `VectorSet` candidate in `candidate_root`, a helper chain `candidate_root -> helper_a -> helper_b`, and an unrelated function reachable from top-level but not from the candidate root.

- [ ] **Step 2: Add exact imports and tests**

In `boot/tests/suites/cfg_summary_suite.tw`, ensure these import lines are present at the top:

```tw
use compiler.cfg
use compiler.codegen.mutable_produce
use compiler.ownership
use compiler.opt.semantics as semantics
use lib.module.loader
```

Add these local helpers:

```tw
fn sound_uniqueness_fixtures_dir() String {
  root := loader.find_project_root("boot")
  "${root}/tests/fixtures/sound_uniqueness"
}

fn func_id_by_name(view: cfg.CfgView, name: String) Result<Int, String> {
  for f in view.functions {
    if f.name == name {
      return .Ok(f.func_id)
    }
  }
  .Err("missing function ${name}")
}
```

Add tests:

```tw
.test(
  "dependency closure includes candidate callees and excludes unrelated functions",
  fn() {
    artifacts := case pipeline.compile_entry_path("${sound_uniqueness_fixtures_dir()}/phase8a_summary_closure.tw") {
      .Ok(result) => result,
      .Err(err) => return .Err(format_compile_error(err)),
    }
    b := artifacts.builtins
    view := ownership.prune_dead_merge(cfg.build_view(artifacts.opt, b))
    root_id := try func_id_by_name(view, "candidate_root")
    helper_a_id := try func_id_by_name(view, "helper_a")
    helper_b_id := try func_id_by_name(view, "helper_b")
    unrelated_id := try func_id_by_name(view, "unrelated")

    roots: Dict<Int, Bool> = Dict.new()
    roots[root_id] = true
    closure := summary.dependency_closure(view, roots)

    try assert.equal(dict_bool(closure, root_id), true)
    try assert.equal(dict_bool(closure, helper_a_id), true)
    try assert.equal(dict_bool(closure, helper_b_id), true)
    try assert.equal(dict_bool(closure, unrelated_id), false)
    .Ok({})
  },
)
```

Define `dict_bool` next to `func_id_by_name`:

```tw
fn dict_bool(d: Dict<Int, Bool>, key: Int) Bool {
  case d.get(key) {
    .Some(v) => v,
    .None => false,
  }
}
```

Add summary equivalence for the same fixture:

```tw
.test(
  "root-scoped summaries match full summaries for candidate roots",
  fn() {
    artifacts := case pipeline.compile_entry_path("${sound_uniqueness_fixtures_dir()}/phase8a_summary_closure.tw") {
      .Ok(result) => result,
      .Err(err) => return .Err(format_compile_error(err)),
    }
    b := artifacts.builtins
    sem := semantics.make_prelude_optimizer_semantics(b)
    view := ownership.prune_dead_merge(cfg.build_view(artifacts.opt, b))
    root_id := try func_id_by_name(view, "candidate_root")
    roots: Dict<Int, Bool> = Dict.new()
    roots[root_id] = true

    full := summary.compute(view, b, sem)
    scoped := summary.compute_for_roots(view, b, sem, roots)
    full_s := case ownership.summary_get(full, root_id) {
      .Some(s) => summary.render_summary(s),
      .None => return .Err("missing full summary for candidate_root"),
    }
    scoped_s := case ownership.summary_get(scoped, root_id) {
      .Some(s) => summary.render_summary(s),
      .None => return .Err("missing scoped summary for candidate_root"),
    }
    try assert.equal(scoped_s, full_s)
    .Ok({})
  },
)
```

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: failure because `dependency_closure` and `compute_for_roots` do not exist.

- [ ] **Step 3: Implement closure and scoped summaries**

In `boot/compiler/summary.tw`, ensure this import line is present at the top:

```tw
use compiler.ownership
```

Use `ownership.SummaryTable` in the public signature:

```tw
pub fn dependency_closure(view: CfgView, roots: Dict<Int, Bool>) Dict<Int, Bool> {
  index := build_func_index(view)
  user_ids := user_id_set(view)
  closed: Dict<Int, Bool> = Dict.new()
  work: Vector<Int> = collect id in roots.keys() { id }
  work = work.sort()

  for work.len() > 0 {
    id := work[0]
    next_work: Vector<Int> = []
    for item, i in work {
      if i > 0 {
        next_work = .append(item)
      }
    }
    work = next_work

    already := case closed.get(id) {
      .Some(v) => v,
      .None => false,
    }
    if !already {
      closed[id] = true
      case index.get(id) {
        .Some(f) => {
          for callee in callee_ids(f, user_ids) {
            seen := case closed.get(callee) {
              .Some(v) => v,
              .None => false,
            }
            if !seen {
              work = .append(callee)
            }
          }
          work = work.sort()
        },
        .None => {},
      }
    }
  }

  closed
}

pub fn compute_for_roots(
  view: CfgView,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  roots: Dict<Int, Bool>,
) ownership.SummaryTable {
  index := build_func_index(view)
  user_ids := user_id_set(view)
  wanted := dependency_closure(view, roots)

  table := empty_summary_table()
  for f in view.functions {
    table = table_put(table, f.func_id, conservative_summary(f))
  }

  for scc in order_sccs(view, index, user_ids) {
    run := false
    for id in scc {
      case wanted.get(id) {
        .Some(true) => run = true,
        _ => {},
      }
    }
    if run {
      table = run_scc(scc, index, user_ids, table, b, sem)
    }
  }
  table
}
```

Functions outside the dependency closure keep conservative seeded summaries. That cannot create a mutable decision; it can only make downstream verdicts less precise if a caller unexpectedly depends on them.

- [ ] **Step 4: Verify and commit**

```bash
target/twk fmt boot/compiler/summary.tw boot/tests/suites/cfg_summary_suite.tw boot/tests/fixtures/sound_uniqueness/phase8a_summary_closure.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
git add boot/compiler/summary.tw boot/tests/suites/cfg_summary_suite.tw boot/tests/fixtures/sound_uniqueness/phase8a_summary_closure.tw
git commit -m "Scope summaries to mutable candidate dependencies"
```

---

### Task 4: Selected-function ownership analysis with cleared unselected facts

**Files:**
- Modify: `boot/compiler/ownership.tw`
- Modify: `boot/compiler/codegen/ownership_verdicts.tw`
- Modify: `boot/tests/suites/mutable_produce_suite.tw`

**Interfaces:**
- Consumes: existing private `analyze_function(...)` and public `analyze_with_summaries(...)`.
- Produces:
  - `pub fn analyze_selected_with_summaries(view: CfgView, b: BuiltinRegistry, sem: OptimizerSemantics, table: SummaryTable, selected: Dict<Int, Bool>) CfgView`
  - `pub fn compute_candidate_artifacts(opt: AnfModule, b: BuiltinRegistry, sem: OptimizerSemantics, candidate_funcs: Dict<Int, Bool>) OwnershipArtifacts`

- [ ] **Step 1: Add selected/full verdict equivalence tests**

In `boot/tests/suites/mutable_produce_suite.tw`, add helper:

```tw
fn candidate_roots(set: mutable_produce.CandidateSet) Dict<Int, Bool> {
  roots: Dict<Int, Bool> = Dict.new()
  for c in set.candidates {
    roots[c.func_id.id] = true
  }
  roots
}

fn assert_selected_verdicts_match_full(name: String) Result<Void, String> {
  artifacts := case pipeline.compile_entry_path("${fixtures_dir()}/${name}.tw") {
    .Ok(result) => result,
    .Err(err) => return .Err(format_compile_error(err)),
  }
  b := artifacts.builtins
  sem := semantics.make_prelude_optimizer_semantics(b)
  set := mutable_produce.phase8a_vector_set_candidates(artifacts.opt, b)
  keys := mutable_produce.candidate_keys(set)

  full_artifacts := ownership_verdicts.compute_artifacts(artifacts.opt, b, sem)
  scoped_artifacts := ownership_verdicts.compute_candidate_artifacts(
    artifacts.opt,
    b,
    sem,
    candidate_roots(set),
  )
  full := ownership_verdicts.verdicts_for_keys(full_artifacts, keys)
  scoped := ownership_verdicts.verdicts_for_keys(scoped_artifacts, keys)

  try assert.equal(scoped.by_key.keys().len(), full.by_key.keys().len())
  for k in full.by_key.keys() {
    expected := case full.by_key.get(k) {
      .Some(v) => v,
      .None => return .Err("missing full verdict for ${k}"),
    }
    actual := case scoped.by_key.get(k) {
      .Some(v) => v,
      .None => return .Err("missing scoped verdict for ${k}"),
    }
    try assert.equal(actual.text, expected.text)
    try assert.equal(actual.reusable_shell, expected.reusable_shell)
  }
  .Ok({})
}
```

Register tests for `phase8a_vector_set_fresh`, `phase8a_vector_set_alias`, and `phase8a_vector_set_loop`.

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: failure because `compute_candidate_artifacts` does not exist.

- [ ] **Step 2: Clear facts for unselected functions**

In `boot/compiler/ownership.tw`, add helpers near `analyze_with_summaries(...)`:

```tw
fn cleared_facts() cfg.BlockFacts {
  cfg.BlockFacts.{
    ownership: Dict.new(),
    binding_valid: Dict.new(),
    live: [],
    field_own: Dict.new(),
    verdicts: Dict.new(),
    verdict_reusable_shell: Dict.new(),
  }
}

fn clear_block_analysis(blk: CfgBlock) CfgBlock {
  blk.entry = cleared_facts()
  blk.exit = cleared_facts()
  blk
}

fn clear_function_analysis(f: CfgFunction) CfgFunction {
  f.blocks = collect blk in f.blocks {
    clear_block_analysis(blk)
  }
  f
}
```

Add the selected analyzer:

```tw
pub fn analyze_selected_with_summaries(
  view: CfgView,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  table: SummaryTable,
  selected: Dict<Int, Bool>,
) CfgView {
  functions := collect f in view.functions {
    should_analyze := case selected.get(f.func_id) {
      .Some(v) => v,
      .None => false,
    }
    if should_analyze {
      analyze_function(clear_function_analysis(f), table, b, sem, Dict.new())
    } else {
      clear_function_analysis(f)
    }
  }
  CfgView.{ functions }
}
```

This API is safe on either fresh or previously analyzed views because it clears unselected facts and clears selected inputs before re-analysis.

- [ ] **Step 3: Add candidate artifacts**

In `boot/compiler/codegen/ownership_verdicts.tw`, add:

```tw
pub fn compute_candidate_artifacts(
  opt: AnfModule,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  candidate_funcs: Dict<Int, Bool>,
) OwnershipArtifacts {
  view := cfg.build_view(opt, b)
  view = ownership.prune_dead_merge(view)
  table := summary.compute_for_roots(view, b, sem, candidate_funcs)
  analyzed := ownership.analyze_selected_with_summaries(view, b, sem, table, candidate_funcs)
  OwnershipArtifacts.{ key: artifact_key_for_anf(opt), view, table, analyzed }
}
```

- [ ] **Step 4: Verify and commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/compiler/codegen/ownership_verdicts.tw boot/tests/suites/mutable_produce_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
git add boot/compiler/ownership.tw boot/compiler/codegen/ownership_verdicts.tw boot/tests/suites/mutable_produce_suite.tw
git commit -m "Analyze ownership for mutable candidate roots"
```

---

### Task 5: Wire scoped artifacts into default mutable production

**Files:**
- Modify: `boot/compiler/codegen/mutable_produce.tw`
- Modify: `boot/compiler/codegen/codegen.tw`
- Modify: `boot/tests/suites/mutable_produce_suite.tw`

**Interfaces:**
- Consumes: `compute_candidate_artifacts(...)`, artifact key validation, private validated verdict join.
- Produces:
  - Default `produce_phase8a_vector_set_decisions(...)` uses scoped artifacts.
  - Diagnostic `produce_phase8a_vector_set_decisions_full(...)` remains available for equivalence tests.

- [ ] **Step 1: Add full/scoped producer equivalence tests**

In `mutable_produce_suite.tw`, add:

```tw
fn assert_scoped_producer_matches_full(name: String) Result<Void, String> {
  artifacts := case pipeline.compile_entry_path("${fixtures_dir()}/${name}.tw") {
    .Ok(result) => result,
    .Err(err) => return .Err(format_compile_error(err)),
  }
  full := mutable_produce.produce_phase8a_vector_set_decisions_full(
    artifacts.opt,
    artifacts.builtins,
  )
  scoped := mutable_produce.produce_phase8a_vector_set_decisions(
    artifacts.opt,
    artifacts.builtins,
  )

  try assert.equal(scoped.table.by_site.keys().len(), full.table.by_site.keys().len())
  try assert.equal(scoped.artifact_computed, full.artifact_computed)
  try assert.equal(
    mutable_produce.render_candidate_rows(scoped.rows),
    mutable_produce.render_candidate_rows(full.rows),
  )
  .Ok({})
}
```

Register tests for `phase8a_vector_set_fresh`, `phase8a_vector_set_alias`, and `phase8a_vector_set_loop`.

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: failure because `produce_phase8a_vector_set_decisions_full` does not exist.

- [ ] **Step 2: Implement root extraction and scoped/default wrappers**

In `mutable_produce.tw`, add:

```tw
fn candidate_func_roots(set: CandidateSet) Dict<Int, Bool> {
  roots: Dict<Int, Bool> = Dict.new()
  for c in set.candidates {
    roots[c.func_id.id] = true
  }
  roots
}
```

Implement full diagnostic wrapper:

```tw
pub fn produce_phase8a_vector_set_decisions_full(
  opt: AnfModule,
  b: BuiltinRegistry,
) ProducedDecisions {
  set := phase8a_vector_set_candidates(opt, b)
  if set.candidates.len() == 0 {
    return ProducedDecisions.{ table: mutable_select.empty_decision_table(), rows: [], artifact_computed: false }
  }
  sem := make_prelude_optimizer_semantics(b)
  artifacts := ownership_verdicts.compute_artifacts(opt, b, sem)
  produce_phase8a_vector_set_decisions_with_artifacts(opt, b, artifacts)
}
```

Change default production to scoped artifacts:

```tw
pub fn produce_phase8a_vector_set_decisions(opt: AnfModule, b: BuiltinRegistry) ProducedDecisions {
  set := phase8a_vector_set_candidates(opt, b)
  if set.candidates.len() == 0 {
    return ProducedDecisions.{ table: mutable_select.empty_decision_table(), rows: [], artifact_computed: false }
  }
  sem := make_prelude_optimizer_semantics(b)
  artifacts := ownership_verdicts.compute_candidate_artifacts(opt, b, sem, candidate_func_roots(set))
  produce_phase8a_vector_set_decisions_with_artifacts(opt, b, artifacts)
}
```

`boot/compiler/codegen/codegen.tw` keeps calling the same default function:

```tw
produced_decisions := mutable_produce.produce_phase8a_vector_set_decisions(anf, builtins)
```

- [ ] **Step 3: Verify correctness and inspect timing evidence**

```bash
target/twk fmt boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/codegen.tw boot/tests/suites/mutable_produce_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/stage2.wasm >/tmp/stage2.out 2>/tmp/stage2.err
rg "produce_mutable_decisions" /tmp/stage2.err
```

Expected correctness: lint and boot tests pass. Timing output is investigation evidence only; if `produce_mutable_decisions` remains dominated by candidate-root dependency closure, stop and report closure size before attempting further approximation.

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/codegen.tw boot/tests/suites/mutable_produce_suite.tw
git commit -m "Use scoped ownership artifacts for mutable decisions"
```

---

### Task 6: Reuse validated artifacts in IR reports

**Files:**
- Modify: `boot/compiler/codegen/dry_run.tw`
- Modify: `boot/commands/ir.tw`
- Modify: `boot/tests/suites/dry_run_suite.tw`

**Interfaces:**
- Consumes: `OwnershipArtifacts`, `artifact_key_for_anf(...)`, `verdicts_from_artifacts(...)`.
- Produces:
  - `pub fn dry_run_sites_with_artifacts(opt: AnfModule, b: BuiltinRegistry, artifacts: ownership_verdicts.OwnershipArtifacts) Vector<DryRunSite>`
  - `render_cfg_artifacts(...)` uses `ownership_verdicts.compute_artifacts(...)`.
  - `render_census_report(... include_sites=true)` computes full artifacts once and shares them with dry-run and producer/audit.

- [ ] **Step 1: Add dry-run artifact equivalence and stale-artifact tests**

In `boot/tests/suites/dry_run_suite.tw`, ensure these import lines are present at the top:

```tw
use compiler.codegen.ownership_verdicts
use compiler.opt.semantics as semantics
```

Add tests:

```tw
.test(
  "dry run with validated artifacts matches wrapper output",
  fn() {
    artifacts := case pipeline.compile_entry_path("${sound_uniqueness_fixtures_dir()}/phase8a_vector_set_fresh.tw") {
      .Ok(result) => result,
      .Err(err) => return .Err(format_compile_error(err)),
    }
    sem := semantics.make_prelude_optimizer_semantics(artifacts.builtins)
    owned := ownership_verdicts.compute_artifacts(artifacts.opt, artifacts.builtins, sem)
    wrapped := dry_run.dry_run_sites(artifacts.opt, artifacts.builtins)
    supplied := dry_run.dry_run_sites_with_artifacts(artifacts.opt, artifacts.builtins, owned)
    try assert.equal(dry_run.render_dry_run(supplied), dry_run.render_dry_run(wrapped))
    .Ok({})
  },
)
.test(
  "dry run with stale artifacts falls back to absent verdicts",
  fn() {
    fresh := case pipeline.compile_entry_path("${sound_uniqueness_fixtures_dir()}/phase8a_vector_set_fresh.tw") {
      .Ok(result) => result,
      .Err(err) => return .Err(format_compile_error(err)),
    }
    alias := case pipeline.compile_entry_path("${sound_uniqueness_fixtures_dir()}/phase8a_vector_set_alias.tw") {
      .Ok(result) => result,
      .Err(err) => return .Err(format_compile_error(err)),
    }
    sem_alias := semantics.make_prelude_optimizer_semantics(alias.builtins)
    stale := ownership_verdicts.compute_artifacts(alias.opt, alias.builtins, sem_alias)
    supplied := dry_run.dry_run_sites_with_artifacts(fresh.opt, fresh.builtins, stale)
    rendered := dry_run.render_dry_run(supplied)
    try assert.str_contains(rendered, "false")
    try assert.str_contains(rendered, "-")
    .Ok({})
  },
)
```

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: failure because `dry_run_sites_with_artifacts` does not exist.

- [ ] **Step 2: Implement validated dry-run artifacts**

In `boot/compiler/codegen/dry_run.tw`, add:

```tw
fn absent_verdicts_for(opt: AnfModule) ownership_verdicts.VerdictTable {
  ownership_verdicts.VerdictTable.{
    key: ownership_verdicts.artifact_key_for_anf(opt),
    by_key: Dict.new(),
  }
}

pub fn dry_run_sites_with_artifacts(
  opt: AnfModule,
  b: BuiltinRegistry,
  artifacts: ownership_verdicts.OwnershipArtifacts,
) Vector<DryRunSite> {
  expected := ownership_verdicts.artifact_key_for_anf(opt)
  verdicts := if ownership_verdicts.same_artifact_key(expected, artifacts.key) {
    ownership_verdicts.verdicts_from_artifacts(artifacts)
  } else {
    absent_verdicts_for(opt)
  }
  dry_run_sites_with_verdict_table(opt, b, verdicts)
}

fn dry_run_sites_with_verdict_table(
  opt: AnfModule,
  b: BuiltinRegistry,
  verdicts: ownership_verdicts.VerdictTable,
) Vector<DryRunSite> {
  sem := make_prelude_optimizer_semantics(b)
  cat := mutable_catalog.build(b, sem)
  sites := census.census_sites(opt, b)

  out: Vector<DryRunSite> = []
  for s in sites {
    v := case verdicts.by_key.get("${s.func_id}#${s.local}") {
      .Some(verdict) => verdict,
      .None => ownership_verdicts.SiteVerdict.{ text: "-", reusable_shell: false },
    }
    t := cat.targets_for_actual(b, s.actual_fid)
    out = .append(DryRunSite.{
      func: s.func,
      family: s.family,
      persistent: t.persistent,
      mutable: t.mutable,
      verdict: v.text,
      reusable_shell: v.reusable_shell,
      would_use: v.reusable_shell and t.has_mutable,
      local: s.local,
    })
  }
  out
}
```

Update existing `dry_run_sites(...)`:

```tw
pub fn dry_run_sites(opt: AnfModule, b: BuiltinRegistry) Vector<DryRunSite> {
  sem := make_prelude_optimizer_semantics(b)
  dry_run_sites_with_artifacts(opt, b, ownership_verdicts.compute_artifacts(opt, b, sem))
}
```

- [ ] **Step 3: Reuse full artifacts in CFG rendering**

In `boot/commands/ir.tw`, add import:

```tw
use compiler.codegen.ownership_verdicts
```

Change `render_cfg_artifacts(...)` to:

```tw
fn render_cfg_artifacts(artifacts: PipelineArtifacts) String {
  b := artifacts.builtins
  s := semantics.make_prelude_optimizer_semantics(b)
  owned := ownership_verdicts.compute_artifacts(artifacts.opt, b, s)
  variants := summary.compute_variants(owned.view, b, s, owned.table)
  summary.render_cfg(owned.view, owned.analyzed, b, s, owned.table, variants)
}
```

- [ ] **Step 4: Reuse one artifact computation in census-with-sites**

In `render_census_report(...)`, keep the plain census path unchanged. In `include_sites` mode, compute full artifacts once:

```tw
if include_sites {
  s := semantics.make_prelude_optimizer_semantics(artifacts.builtins)
  owned := ownership_verdicts.compute_artifacts(artifacts.opt, artifacts.builtins, s)

  dry := dry_run.dry_run_sites_with_artifacts(artifacts.opt, artifacts.builtins, owned)
  out = .concat(dry_run.render_dry_run(dry))

  cc := convert_closures(artifacts.opt, artifacts.env)
  produced := mutable_produce.produce_phase8a_vector_set_decisions_with_artifacts(
    artifacts.opt,
    artifacts.builtins,
    owned,
  )
  prepared := prepare_backend_with_mutable_config(
    cc.anf,
    artifacts.env,
    artifacts.builtins,
    cc.captures,
    produced.table,
    mutable_select.phase8a_policy(),
  )
  audit_rows := mutable_audit.audit_prepared_calls(prepared, artifacts.env, artifacts.builtins)
  out = .concat("\nmutable decisions\n")
  out = .concat(mutable_audit.render_audit_rows(audit_rows))
}
```

- [ ] **Step 5: Verify IR reports including CFG smoke**

```bash
target/twk fmt boot/compiler/codegen/dry_run.tw boot/commands/ir.tw boot/tests/suites/dry_run_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
target/twk ir --cfg boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw >/tmp/phase8a-cfg.txt
rg "CFG ownership view|summary|set_fresh" /tmp/phase8a-cfg.txt
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw >/tmp/phase8a-census-fresh.txt
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_alias.tw >/tmp/phase8a-census-alias.txt
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw >/tmp/phase8a-census-loop.txt
rg "mutable decisions|vector_set|selected|policy_disabled|stale_or_ignored|absent_fallback" /tmp/phase8a-census-fresh.txt /tmp/phase8a-census-alias.txt /tmp/phase8a-census-loop.txt
```

Expected: CFG renders, fresh reports `selected`, alias/loop report persistent fallback states.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/codegen/dry_run.tw boot/commands/ir.tw boot/tests/suites/dry_run_suite.tw
git commit -m "Reuse ownership artifacts in IR reports"
```

---

### Task 7: Mutable producer timing diagnostics

**Files:**
- Modify: `boot/compiler/codegen/mutable_produce.tw`
- Modify: `boot/compiler/codegen/ownership_verdicts.tw`

**Interfaces:**
- Consumes: `@std.date.now()` and `@std.proc.env("TWINKLE_TIMINGS")`.
- Produces diagnostic-only timing lines:
  - `[time:mutable] candidate_scan=...ms artifact=...ms decision_join=...ms candidates=... roots=...`
  - `[time:mutable:artifacts] cfg=...ms summary=...ms ownership=...ms scope_roots=...`

- [ ] **Step 1: Add local timing helpers**

In both files, ensure these import lines are present at the top:

```tw
use @std.date
use @std.proc
```

Add helper:

```tw
fn timings_enabled() Bool {
  case proc.env("TWINKLE_TIMINGS") {
    .Some(_) => true,
    .None => false,
  }
}
```

- [ ] **Step 2: Log producer subphases**

In the default scoped producer path, record timestamps around candidate scan, artifact computation, and decision join. Emit:

```tw
if timings_enabled() {
  eprintln(
    "[time:mutable] candidate_scan=${t_candidates - t0}ms artifact=${t_artifacts - t_candidates}ms decision_join=${t_done - t_artifacts}ms candidates=${set.candidates.len()} roots=${roots.keys().len()}",
  )
}
```

In the empty-candidate path, emit:

```tw
if timings_enabled() {
  eprintln("[time:mutable] candidate_scan=${t_candidates - t0}ms artifact=0ms decision_join=0ms candidates=0 roots=0")
}
```

- [ ] **Step 3: Log artifact subphases**

In `compute_artifacts(...)` and `compute_candidate_artifacts(...)`, emit:

```tw
if timings_enabled() {
  eprintln(
    "[time:mutable:artifacts] cfg=${t_view - t0}ms summary=${t_summary - t_view}ms ownership=${t_own - t_summary}ms scope_roots=${candidate_funcs.keys().len()}",
  )
}
```

For full artifacts, use `scope_roots=-1`.

- [ ] **Step 4: Verify timing output manually and commit**

```bash
target/twk fmt boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/ownership_verdicts.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/stage2.wasm >/tmp/stage2.out 2>/tmp/stage2.err
rg "time:mutable|produce_mutable_decisions" /tmp/stage2.err
git add boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/ownership_verdicts.tw
git commit -m "Trace mutable decision producer subphases"
```

---

### Task 8: Final verification, docs, review

**Files:**
- Modify: `docs/plans/sound-uniqueness/README.md`
- Modify: `docs/plans/sound-uniqueness/codegen/README.md`
- Modify: `tests/cow_analysis.rs` only if audit output shape changes.

**Interfaces:**
- Consumes: all prior task outputs.
- Produces: final evidence that artifact sharing/scoping preserves Phase 8A behavior and removes unscoped ownership work from normal build production.

- [ ] **Step 1: Run full verification**

```bash
target/twk fmt boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/ownership_verdicts.tw boot/compiler/codegen/dry_run.tw boot/compiler/codegen/codegen.tw boot/compiler/summary.tw boot/compiler/ownership.tw boot/commands/ir.tw boot/tests/suites/mutable_produce_suite.tw boot/tests/suites/dry_run_suite.tw boot/tests/suites/cfg_summary_suite.tw boot/tests/fixtures/sound_uniqueness/phase8a_no_vector_set.tw boot/tests/fixtures/sound_uniqueness/phase8a_summary_closure.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
cargo test --release
```

Expected: formatter completes, lint reports no findings, boot tests pass, Rust tests pass.

- [ ] **Step 2: Verify Phase 8A call behavior remains unchanged**

```bash
target/twk ir --cfg boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_fresh.tw >/tmp/phase8a-cfg.txt
rg "CFG ownership view|summary|set_fresh" /tmp/phase8a-cfg.txt

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
- Fresh fixture audit reports `selected`; WAT calls `rt_arr__set_in_place`.
- Alias fixture reports fallback; WAT calls `rt_arr__set`.
- Loop fixture reports fallback/deferred Phase 8B behavior; WAT calls `rt_arr__set`.

- [ ] **Step 3: Record regression-investigation timing evidence**

```bash
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/stage2.wasm >/tmp/stage2.out 2>/tmp/stage2.err
rg "time:mutable|produce_mutable_decisions" /tmp/stage2.err
```

Expected: timing output identifies mutable subphases. Do not use a numeric threshold as correctness acceptance. If `scope_roots` or summary closure evidence shows nearly the whole compiler is still analyzed, record that as the remaining performance blocker instead of claiming the regression is fixed.

- [ ] **Step 4: Update docs**

In `docs/plans/sound-uniqueness/codegen/README.md`, add:

```md
- Phase 8A mutable decision production now uses fingerprinted shared ownership
  artifacts. Build codegen still consumes decision records only; CFG ownership
  remains the proof source, and stale artifact keys, absent verdicts,
  stale prepared sites, ambiguous decisions, and aliases fall back persistently.
```

In `docs/plans/sound-uniqueness/README.md`, mention artifact sharing as Phase 8A maintenance/refactor work and keep Phase 8B as the next semantic expansion.

- [ ] **Step 5: Request review**

Use a fresh-context reviewer. Ask specifically for:

- artifact fingerprint soundness,
- absence of public raw candidate/verdict mixing APIs,
- selected-function ownership clearing semantics,
- transitive summary closure coverage,
- unchanged fallback behavior,
- verification evidence.

The reviewer must not edit files.

- [ ] **Step 6: Final commit**

```bash
git add docs/plans/sound-uniqueness/README.md docs/plans/sound-uniqueness/codegen/README.md tests/cow_analysis.rs
git commit -m "Document mutable decision artifact sharing"
```

If no docs/parser files changed in this final task, skip this commit and include that in the final report.

---

## Acceptance Checklist

- [ ] Empty candidate sets return empty decisions with `artifact_computed == false`.
- [ ] Public artifact-fed producer/dry-run APIs validate `ArtifactKey` before using verdicts.
- [ ] There is no public raw `candidates + verdicts` API that can silently mix stale artifacts.
- [ ] `OwnershipArtifacts.table` uses `ownership.SummaryTable`.
- [ ] Test snippets and implemented tests use Twinkle module aliases, not root-qualified calls.
- [ ] Root dependency closure test includes `candidate_root -> helper_a -> helper_b` and excludes `unrelated`.
- [ ] Selected-function ownership analysis clears facts on unselected functions and before re-analyzing selected functions.
- [ ] Scoped candidate verdicts match full verdicts on fresh, alias, and loop Phase 8A fixtures.
- [ ] `twk ir --cfg` smoke still renders ownership/summary output.
- [ ] `twk ir --census --sites` shares artifacts between dry-run and mutable audit.
- [ ] Phase 8A fresh/alias/loop WAT call inspection remains unchanged.
- [ ] `target/twk lint boot/main.tw`, `target/twk run boot/tests/main.tw`, and `cargo test --release` pass before completion is claimed.

## Risks and Stop Conditions

- If the optimized-ANF fingerprint omits an ANF/operation field that can affect ownership or candidate validity, stop and fix the key before using artifact-fed APIs.
- If candidate roots pull in nearly the whole compiler call graph, scoped summaries may not materially reduce build time. Report the measured closure instead of adding unsound approximations.
- Do not replace summary computation with a local codegen proof. Conservative summaries are allowed only when they force fallback.
- Do not analyze closure-converted ANF for ownership decisions.
- Do not expand Phase 8A beyond `VectorSet`.
- If scoped and full verdicts differ on any Phase 8A fixture, treat the scoped path as unsound until root-caused.
