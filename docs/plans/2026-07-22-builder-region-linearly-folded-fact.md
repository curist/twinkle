# Builder-Region `linearly_folded` Fact + Inspection — Implementation Plan (8C Plan 1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the analysis-owned `linearly_folded` safety fact for loop-carried builder regions (empty-seed, unconditional-fold `String.concat` / `Vector.append` accumulators) and render certified/rejected regions in `twk ir --census --sites`. No emitted-code change.

**Architecture:** A shared, pure-ANF detector resurrects the deleted `analyze_loop_push_sites` scan (sound **by rejection** — it recurses through all control flow and rejects interior reads, publication edges, and self-concat). The detector output is composed with the current sound ownership analysis's uniqueness fact — computed by extending the existing per-block `block_verdicts` pass with a `fold_reusable` map, so no new fixpoint is introduced — to produce `linearly_folded` per region. The fact is surfaced through `ownership_verdicts.tw` keyed by a deterministic `BuilderRegionKey`, and rendered in the `ir` census. Plan 2 (a later plan) adds the codegen producer/records and the rewrite; this plan stops at the analysis fact + inspection.

**Tech Stack:** Twinkle (`.tw`), boot compiler (`boot/`). Tests via `@std.testing` suites compiled by `pipeline.compile_source`. Build/verify with `make boot-test`, then `make bundle-cli` for the inspection/self-host steps.

---

## Background the engineer needs

- **Immutability model:** all values immutable; `acc = acc.concat(c)` is *rebind*, lowered in ANF to `Let(r, ACall(concat, [acc, chunk]), Let(_, AAssign(acc, r), ...))`. Loop bodies are `AnfOp.ALoop(body)`.
- **Why this fact:** builder-region lowering steals a proven-unique, linearly-folded accumulator into a transient builder. It is unsafe if the accumulator is read mid-loop (a builder doesn't materialize until `freeze`) or published on an exit edge. The old `opt/loop_builder.tw`/`opt/liveness.tw` did this but were deleted in `5d5ac090` because the *uniqueness* input (`liveness.tw`) was unsound. We keep the sound structural scan and replace the uniqueness input with the current ownership analysis.
- **Design doc (read first):** `docs/plans/sound-uniqueness/codegen/builder-region-design.md`. This plan implements its "Plan 1" — Component 1 (the fact) + the inspection gate. First-slice scope: empty-seed, unconditional-fold loops; single post-loop freeze; reject all intra-region publication/early-exit.
- **The five conditions** (`builder-region-design.md` → "The safety fact"): (1) seed uniqueness — vacuous for empty seeds; (2) carried uniqueness — the ownership query; (3) linear fold / no interior observation; (4) single post-loop freeze, reject intra-region publication; (5) no self-alias. Conditions 3/4/5 are the structural scan (sound by rejection); condition 2 is the ownership query.

## Key ANF/API facts (verified, cite in code)

- ANF (`boot/compiler/anf.tw:19-88`): `Atom` = `ALocal(LocalId) | AGlobalFunc(FuncId) | ALitInt | ALitFloat | ALitBool | ALitStr(String) | ALitVoid | AGlobalLocal`. `AnfExpr` = `Let(LocalId, AnfOp, AnfExpr) | Atom(Atom) | Return(Atom?) | Break(Atom?) | Continue`. `AnfOp` includes `ACall(Atom, Vector<Atom>)`, `AIf`, `AMatch`, `ALoop(AnfExpr)`, `ADefer`, `AAssign(LocalId, Atom)`, `AInit(Atom)`, `AArrayLit(Vector<Atom>)`, `ARecordGet`, `ARecordUpdate`, etc. `AnfFunctionDef.{ func_id, name, body, ... }`, `AnfModule.{ functions, ... }`.
- Builder families (`boot/compiler/builder_family.tw`): `string_builder_config(b).push_id == b.method_id("String","concat")`; `vector_builder_config(b).push_id == b.method_id("Vector","append")`.
- Ownership per-block verdicts (`boot/compiler/ownership.tw:1943` `block_verdicts`): forward walk with `st: ForwardState`, `last := last_use_at(inst.op, scan.live_after[i])`, and the query `pre.shell_reusable(base, last)` (line 1774) → true iff `base` is `Unique` + valid + last-use. Results land in `BlockFacts` (`cfg.tw:37-54`) and are written at `ownership.tw:3340-3343`.
- Artifacts (`boot/compiler/codegen/ownership_verdicts.tw:296-360`): `compute_artifacts` / `compute_candidate_artifacts` return `OwnershipArtifacts.{ key, analyzed, ... }`; `verdicts_from_artifacts` flattens `blk.exit.*` keyed by `"${func_id}#${local_id}"`; `ArtifactKey` fingerprints the optimized ANF.
- Inspection (`boot/commands/ir.tw:65-97` `render_census_report`): appends dry-run + mutable-audit tables when `include_sites`.
- Test harness (`boot/tests/suites/dry_run_suite.tw`): `use @std.testing.assert as assert`, `use @std.testing as runner`; `pub fn suite() runner.Suite` = `runner.suite("name").test("desc", fn() Result<Void,String> { try assert.equal(...) ... .Ok({}) })`; `pipeline.compile_source(src) Result<PipelineArtifacts>` gives `.opt` (AnfModule) + `.builtins`. Register in `boot/tests/main.tw` via `use .suites.<name>` + adding `<name>.suite()` to `runner.run_all([...])`.

## Build / test loop

- **Unit tests (Phase A/B):** `make boot-test` — `target/twk` compiles the edited `boot/compiler/*` modules from source at test time, so no rebuild is needed to test new/changed compiler modules.
- **Inspection + self-host (Phase C):** requires `make bundle-cli` first (rebuilds `target/twk`), then `target/twk ir ... --census --sites` and the self-host check. Run heavy commands (`make bundle-cli`, full suite, `make stage2`) **one at a time, never concurrently**.

## File Structure

- **Create `boot/compiler/builder_region_detect.tw`** — pure ANF detector: `RegionShape`, `BuilderRegionKey`, resurrected scan helpers (`analyze_loop_push_sites` + friends, sound by rejection), and `detect_regions(func, b) Vector<RegionShape>`. No ownership, no CFG. Analysis-track (the structural conditions 3/4/5 live here). Shared by this plan's fact and Plan 2's producer.
- **Modify `boot/compiler/cfg.tw`** — add a `fold_reusable: Dict<Int, Bool>` map to `BlockFacts` + `empty_block_facts()`.
- **Modify `boot/compiler/ownership.tw`** — extend `BlockVerdicts` + `block_verdicts` to record `fold_reusable` for `concat`/`append` fold calls, and write it into `blk.exit.fold_reusable`.
- **Create `boot/compiler/builder_region_fact.tw`** — compose `RegionShape` + converged `fold_reusable` (condition 2) + empty seed (condition 1) → `RegionVerdict{ key, linearly_folded, reason }`; `region_verdicts(opt, b, artifacts)`; artifact-key fingerprinted.
- **Modify `boot/compiler/codegen/ownership_verdicts.tw`** — surface `fold_reusable` in the flattened verdict table (so the fact module can read it via the artifact key path), if not consumed directly from `artifacts.analyzed`.
- **Modify `boot/commands/ir.tw`** — render certified/rejected regions in `render_census_report` under `--census --sites`.
- **Create `boot/tests/suites/builder_region_suite.tw`** + register in `boot/tests/main.tw`.

---

## Phase A — Shared pure-ANF detector (conditions 3/4/5 + shape)

### Task A1: `RegionShape` + `BuilderRegionKey` types and family config

**Files:**
- Create: `boot/compiler/builder_region_detect.tw`
- Test: `boot/tests/suites/builder_region_suite.tw`

- [ ] **Step 1: Create the module skeleton with the types.**

```tw
//! Pure-ANF builder-region detector: identifies empty-seed, unconditional-fold
//! loop accumulator regions and the structural safety conditions (no interior
//! observation, no publication before the single post-loop freeze, no
//! self-concat). Sound BY REJECTION: any shape it cannot classify is not
//! emitted. Shared by the linearly_folded fact (builder_region_fact.tw) and the
//! Plan 2 codegen producer.

use compiler.anf.{AnfExpr, AnfMatchArm, AnfModule, AnfFunctionDef, AnfOp, Atom}
use compiler.builder_family.{BuilderConfig, string_builder_config, vector_builder_config}
use compiler.builtins.{BuiltinRegistry}
use compiler.core_ir.{FuncId, LocalId}

// A detected region. `family` is "string" or "vector"; `accumulator` is the
// folded local; `seed_site`/`loop_site` are the accumulator's empty-seed bind
// and the loop-result bind; `fold_sites` are the fold-call result locals (their
// count is the push-site consistency number). Structural conditions 3/4/5 hold
// for every RegionShape this module returns.
pub type RegionShape = .{
  func_id: FuncId,
  func: String,
  family: String,
  push_id: FuncId,
  accumulator: LocalId,
  seed_site: LocalId,
  loop_site: LocalId,
  fold_sites: Vector<LocalId>,
}

// Deterministic region identity: equal keys denote the same region across runs.
pub type BuilderRegionKey = .{
  func_id: Int,
  seed_site: Int,
  loop_site: Int,
  fold_sites: Vector<Int>,
  freeze_site: Int,
  family: String,
}

pub fn region_key(s: RegionShape) BuilderRegionKey {
  ids: Vector<Int> = collect fs in s.fold_sites { fs.id }
  BuilderRegionKey.{
    func_id: s.func_id.id,
    seed_site: s.seed_site.id,
    loop_site: s.loop_site.id,
    fold_sites: ids,
    freeze_site: s.loop_site.id,
    family: s.family,
  }
}

fn atom_is_local(a: Atom, local: LocalId) Bool {
  case a {
    .ALocal(id) => id.id == local.id,
    _ => false,
  }
}
```

- [ ] **Step 2: Create the test suite skeleton and register it.**

```tw
// boot/tests/suites/builder_region_suite.tw
use @std.testing.assert as assert
use @std.testing as runner

use compiler.builder_region_detect as detect
use compiler.pipeline

fn regions_for(src: String) Vector<detect.RegionShape> {
  case pipeline.compile_source(src) {
    .Ok(a) => detect.detect_regions(a.opt, a.builtins),
    .Err(e) => error("compile failed: ${e}"),
  }
}

pub fn suite() runner.Suite {
  runner.suite("builder_region")
    .test("placeholder", fn() Result<Void, String> {
      assert.is_true(true)
    })
}
```

In `boot/tests/main.tw`: add `use .suites.builder_region_suite` with the other suite imports, and add `builder_region_suite.suite(),` to the `runner.run_all([...])` list.

- [ ] **Step 3: Run the suite to confirm wiring.**

Run: `make boot-test`
Expected: PASS (placeholder test), suite `builder_region` appears in output.

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/builder_region_detect.tw boot/tests/suites/builder_region_suite.tw boot/tests/main.tw
git commit -m "builder-region: detector module skeleton + RegionShape/BuilderRegionKey types"
```

---

### Task A2: Resurrect the sound-by-rejection scan (conditions 3/4/5)

**Files:**
- Modify: `boot/compiler/builder_region_detect.tw`
- Test: `boot/tests/suites/builder_region_suite.tw`

- [ ] **Step 1: Write failing tests for the scan predicate via `detect_regions`.** (The scan is exercised through detection; a clean fold is detected, an interior read / early return / self-concat is rejected.)

```tw
// replace the placeholder test with these
.test("clean string fold is detected", fn() Result<Void, String> {
  rs := regions_for("acc = \"\"\nfor c in [\"a\", \"b\"] { acc = acc.concat(c) }\nprintln(acc)\n")
  try assert.equal(rs.len(), 1)
  try assert.equal(rs[0].family, "string")
  .Ok({})
})
.test("interior read of accumulator rejects", fn() Result<Void, String> {
  rs := regions_for("acc = \"\"\nfor c in [\"a\"] { acc = acc.concat(c)\nprintln(acc) }\nprintln(acc)\n")
  assert.equal(rs.len(), 0)
})
.test("early return of accumulator rejects", fn() Result<Void, String> {
  rs := regions_for("acc = \"\"\nfor c in [\"a\"] { acc = acc.concat(c)\nif c == \"a\" { return acc }\n}\nprintln(acc)\n")
  assert.equal(rs.len(), 0)
})
.test("self concat rejects", fn() Result<Void, String> {
  rs := regions_for("acc = \"\"\nfor c in [\"a\"] { acc = acc.concat(acc) }\nprintln(acc)\n")
  assert.equal(rs.len(), 0)
})
```

- [ ] **Step 2: Run to verify failure.**

Run: `make boot-test`
Expected: FAIL — `detect_regions` not defined.

- [ ] **Step 3: Add the scan helpers (resurrected from `5d5ac090^:boot/compiler/opt/analysis.tw`, sound by rejection).**

```tw
// `op_uses_local_non_recursive`: does `op` read `local` in a way that is NOT
// the fold-base position? Enumerate EVERY AnfOp variant with no wildcard so a
// new variant forces a compile error here (a silent `_ => false` would be
// unsound — an unclassified use must be treated as an observation).
fn op_uses_local_non_recursive(op: AnfOp, local: LocalId) Bool {
  case op {
    .ACall(callee, args) => {
      if atom_is_local(callee, local) { return true }
      for a in args { if atom_is_local(a, local) { return true } }
      false
    },
    .ABinOp(_, l, r, _) => atom_is_local(l, local) or atom_is_local(r, local),
    .AUnOp(_, inner, _) => atom_is_local(inner, local),
    .AMakeClosure(_, free_vars) => {
      for v in free_vars { if v.id == local.id { return true } }
      false
    },
    .ARecord(_, fields) => {
      for f in fields { if atom_is_local(f.value, local) { return true } }
      false
    },
    .ARecordGet(t, _, _) => atom_is_local(t, local),
    .ARecordUpdate(base, _, value, _, _) => atom_is_local(base, local) or atom_is_local(value, local),
    .AVariant(_, _, args) => {
      for a in args { if atom_is_local(a, local) { return true } }
      false
    },
    .AArrayLit(elems) => {
      for a in elems { if atom_is_local(a, local) { return true } }
      false
    },
    .AIndex(base, index, _, _) => atom_is_local(base, local) or atom_is_local(index, local),
    .AInit(v) => atom_is_local(v, local),
    .AAssign(target, v) => target.id == local.id or atom_is_local(v, local),
    .AGlobalSet(_, v) => atom_is_local(v, local),
    .AIf(cond_e, _, _) => atom_is_local(cond_e, local),
    .AMatch(scrutinee, _) => atom_is_local(scrutinee, local),
    .ALoop(_) => false,
    .ADefer(_) => false,
    .AWrapAnyref(a, _) => atom_is_local(a, local),
    .AUnwrapAnyref(a, _) => atom_is_local(a, local),
  }
}

// The consume-reassign tail: `Let(_, AAssign(base, result), _)`.
fn is_consume_reassign(body: AnfExpr, base: LocalId, result: LocalId) Bool {
  case body {
    .Let(_, .AAssign(target, .ALocal(v)), _) => target.id == base.id and v.id == result.id,
    _ => false,
  }
}

// A fold step: `ACall(push_id, [base, chunk])` with chunk != base (rejects
// self-concat, condition 5) whose result is consume-reassigned into base. On
// match returns the chunk atom.
fn loop_push_reassign_elem(
  op: AnfOp,
  body: AnfExpr,
  base: LocalId,
  push_id: FuncId,
  result: LocalId,
) Atom? {
  case op {
    .ACall(.AGlobalFunc(f), args) => if f.id == push_id.id
      and args.len() == 2 and atom_is_local(args[0], base) and !atom_is_local(args[1], base)
      and is_consume_reassign(body, base, result) {
      .Some(args[1])
    } else {
      .None
    },
    _ => .None,
  }
}

// Count fold sites in a loop body, REJECTING (returns .None) on any interior
// observation (condition 3) or publication of `base` on an exit edge
// (condition 4). Recurses through all control flow.
pub fn analyze_loop_push_sites(expr: AnfExpr, base: LocalId, push_id: FuncId) Int? {
  case expr {
    .Let(local, op, body) => case loop_push_reassign_elem(op, body, base, push_id, local) {
      .Some(_) => case body {
        .Let(_, _, rest) => {
          rest_sites := try analyze_loop_push_sites(rest, base, push_id)
          .Some(1 + rest_sites)
        },
        _ => .None,
      },
      .None => {
        if op_uses_local_non_recursive(op, base) { return .None }
        sub := try analyze_loop_push_sites_in_op(op, base, push_id)
        rest := try analyze_loop_push_sites(body, base, push_id)
        .Some(sub + rest)
      },
    },
    .Atom(a) => if atom_is_local(a, base) { .None } else { .Some(0) },
    .Return(.Some(a)) => if atom_is_local(a, base) { .None } else { .Some(0) },
    .Break(.Some(a)) => if atom_is_local(a, base) { .None } else { .Some(0) },
    .Return(.None) => .Some(0),
    .Break(.None) => .Some(0),
    .Continue => .Some(0),
  }
}

fn analyze_loop_push_sites_in_op(op: AnfOp, base: LocalId, push_id: FuncId) Int? {
  case op {
    .AIf(_, then_e, else_e) => {
      t := try analyze_loop_push_sites(then_e, base, push_id)
      e := try analyze_loop_push_sites(else_e, base, push_id)
      .Some(t + e)
    },
    .AMatch(_, arms) => {
      sites := 0
      for arm in arms {
        s := try analyze_loop_push_sites(arm.body, base, push_id)
        sites = sites + s
      }
      .Some(sites)
    },
    .ALoop(body) => analyze_loop_push_sites(body, base, push_id),
    .ADefer(body) => analyze_loop_push_sites(body, base, push_id),
    _ => .Some(0),
  }
}
```

Note: `detect_regions` is added in Task A3; the tests still fail until then. That is expected — this step adds the scan the detector uses.

- [ ] **Step 4: (defer running to A3, where `detect_regions` exists). Commit the scan.**

```bash
git add boot/compiler/builder_region_detect.tw
git commit -m "builder-region: resurrect sound-by-rejection fold scan (conditions 3/4/5)"
```

---

### Task A3: `detect_regions` — find empty-seed accumulator loops

**Files:**
- Modify: `boot/compiler/builder_region_detect.tw`
- Test: `boot/tests/suites/builder_region_suite.tw` (the Task A2 tests now run)

- [ ] **Step 1: Add empty-seed recognition + fold-site collection + the detection walk.**

```tw
// An empty seed is `""` for the string family or `[]` for the vector family.
fn empty_seed_family(op: AnfOp, str_push: FuncId, vec_push: FuncId) (FuncId, String)? {
  case op {
    .AInit(.ALitStr(s)) => if s.len() == 0 { .Some((str_push, "string")) } else { .None },
    .AArrayLit(elems) => if elems.len() == 0 { .Some((vec_push, "vector")) } else { .None },
    _ => .None,
  }
}

// Collect the fold-call result locals in a loop body (already known clean).
fn collect_fold_sites(expr: AnfExpr, base: LocalId, push_id: FuncId, acc: Vector<LocalId>) Vector<LocalId> {
  case expr {
    .Let(local, op, body) => case loop_push_reassign_elem(op, body, base, push_id, local) {
      .Some(_) => {
        acc2 := acc.append(local)
        case body {
          .Let(_, _, rest) => collect_fold_sites(rest, base, push_id, acc2),
          _ => acc2,
        }
      },
      .None => {
        acc2 := collect_fold_sites_in_op(op, base, push_id, acc)
        collect_fold_sites(body, base, push_id, acc2)
      },
    },
    _ => acc,
  }
}

fn collect_fold_sites_in_op(op: AnfOp, base: LocalId, push_id: FuncId, acc: Vector<LocalId>) Vector<LocalId> {
  case op {
    .AIf(_, then_e, else_e) => {
      a := collect_fold_sites(then_e, base, push_id, acc)
      collect_fold_sites(else_e, base, push_id, a)
    },
    .AMatch(_, arms) => {
      cur := acc
      for arm in arms { cur = collect_fold_sites(arm.body, base, push_id, cur) }
      cur
    },
    .ALoop(body) => collect_fold_sites(body, base, push_id, acc),
    .ADefer(body) => collect_fold_sites(body, base, push_id, acc),
    _ => acc,
  }
}

// From the seed binding of `a`, scan forward: `a` must be UNTOUCHED until an
// ALoop whose body folds `a` cleanly (analyze_loop_push_sites Some(n>0)). Any
// use of `a` before that loop rejects (the seed value would be observed before
// the builder freeze). Returns (loop_site, fold_sites).
fn find_fold_loop(expr: AnfExpr, a: LocalId, push_id: FuncId) (LocalId, Vector<LocalId>)? {
  case expr {
    .Let(local, op, body) => case op {
      .ALoop(loop_body) => case analyze_loop_push_sites(loop_body, a, push_id) {
        .Some(n) => if n > 0 {
          .Some((local, collect_fold_sites(loop_body, a, push_id, [])))
        } else {
          .None
        },
        .None => .None,
      },
      _ => if op_uses_local_non_recursive(op, a) {
        .None
      } else {
        find_fold_loop(body, a, push_id)
      },
    },
    _ => .None,
  }
}

fn detect_in_expr(
  expr: AnfExpr,
  func_id: FuncId,
  func: String,
  str_push: FuncId,
  vec_push: FuncId,
  acc: Vector<RegionShape>,
) Vector<RegionShape> {
  case expr {
    .Let(local, op, body) => {
      acc2 := case empty_seed_family(op, str_push, vec_push) {
        .Some((push_id, family)) => case find_fold_loop(body, local, push_id) {
          .Some((loop_site, fold_sites)) => acc.append(RegionShape.{
            func_id,
            func,
            family,
            push_id,
            accumulator: local,
            seed_site: local,
            loop_site,
            fold_sites,
          }),
          .None => acc,
        },
        .None => acc,
      }
      acc3 := detect_in_op(op, func_id, func, str_push, vec_push, acc2)
      detect_in_expr(body, func_id, func, str_push, vec_push, acc3)
    },
    _ => acc,
  }
}

fn detect_in_op(
  op: AnfOp,
  func_id: FuncId,
  func: String,
  str_push: FuncId,
  vec_push: FuncId,
  acc: Vector<RegionShape>,
) Vector<RegionShape> {
  case op {
    .AIf(_, then_e, else_e) => {
      a := detect_in_expr(then_e, func_id, func, str_push, vec_push, acc)
      detect_in_expr(else_e, func_id, func, str_push, vec_push, a)
    },
    .AMatch(_, arms) => {
      cur := acc
      for arm in arms { cur = detect_in_expr(arm.body, func_id, func, str_push, vec_push, cur) }
      cur
    },
    .ALoop(body) => detect_in_expr(body, func_id, func, str_push, vec_push, acc),
    .ADefer(body) => detect_in_expr(body, func_id, func, str_push, vec_push, acc),
    _ => acc,
  }
}

pub fn detect_regions(m: AnfModule, b: BuiltinRegistry) Vector<RegionShape> {
  str_push := string_builder_config(b).push_id
  vec_push := vector_builder_config(b).push_id
  out: Vector<RegionShape> = []
  for f in m.functions {
    out = detect_in_expr(f.body, f.func_id, f.name, str_push, vec_push, out)
  }
  out
}
```

- [ ] **Step 2: Run the Task A2 + A3 tests.**

Run: `make boot-test`
Expected: PASS — clean string fold detected (1 region), interior-read/early-return/self-concat all reject (0 regions).

- [ ] **Step 3: Add a vector-family + nested-loop positive test.**

```tw
.test("clean vector fold is detected", fn() Result<Void, String> {
  rs := regions_for("acc: Vector<Int> = []\nfor x in [1, 2, 3] { acc = acc.append(x) }\nprintln(acc.len().to_string())\n")
  try assert.equal(rs.len(), 1)
  try assert.equal(rs[0].family, "vector")
  .Ok({})
})
```

- [ ] **Step 4: Run and confirm PASS.**

Run: `make boot-test`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/builder_region_detect.tw boot/tests/suites/builder_region_suite.tw
git commit -m "builder-region: detect empty-seed unconditional-fold accumulator loops"
```

---

## Phase B — The `linearly_folded` fact (rides the ownership pass)

### Task B1: Add `fold_reusable` to `BlockFacts`

**Files:**
- Modify: `boot/compiler/cfg.tw:37-54`

- [ ] **Step 1: Add the field to `BlockFacts` and `empty_block_facts`.**

In `BlockFacts` (after `verdict_reusable_shell`):

```tw
  verdict_reusable_shell: Dict<Int, Bool>,
  // Set at a fold-step call (String.concat / Vector.append) whose base is the
  // accumulator: true iff the base is Unique + valid + at last use there
  // (condition 2 for builder regions). Keyed by the fold-call result local.
  fold_reusable: Dict<Int, Bool>,
```

In `empty_block_facts()` (after `verdict_reusable_shell: Dict.new(),`):

```tw
    verdict_reusable_shell: Dict.new(),
    fold_reusable: Dict.new(),
```

- [ ] **Step 2: Run the suite to confirm nothing broke (record literals updated).**

Run: `make boot-test`
Expected: PASS (no behavior change yet; every `BlockFacts.{...}` construction in the tree is either `empty_block_facts()` or a copy-update, so the new field is covered — if the compiler reports a missing field at another `BlockFacts.{...}` literal, add `fold_reusable: Dict.new()` there).

- [ ] **Step 3: Commit.**

```bash
git add boot/compiler/cfg.tw
git commit -m "cfg: add fold_reusable BlockFacts map for builder-region condition 2"
```

---

### Task B2: Record `fold_reusable` in `block_verdicts`

**Files:**
- Modify: `boot/compiler/ownership.tw` — `BlockVerdicts` (line 1941), `block_verdicts` (1943-2059), and the write site (3340-3343).

- [ ] **Step 1: Extend the `BlockVerdicts` record and its constructor.**

At `ownership.tw:1941`:

```tw
type BlockVerdicts = .{ texts: Dict<Int, String>, reusable_shell: Dict<Int, Bool>, fold_reusable: Dict<Int, Bool> }
```

In `block_verdicts`, add near the other accumulators (after `reusable_shell: Dict<Int, Bool> = Dict.new()`):

```tw
  fold_reusable: Dict<Int, Bool> = Dict.new()
```

- [ ] **Step 2: Recognize fold-step calls in the instruction loop.** Inside the `case inst.op { .ACall(callee, args) => ...` arm, before the existing `case callee_func_id(callee)` handling, record fold reusability when the callee is `String.concat` / `Vector.append` and arg0 is a local. Add this block at the top of the `.ACall(callee, args)` arm:

```tw
      .ACall(callee, args) => {
        case callee_func_id(callee) {
          .Some(fid) => if is_fold_push(sem, fid) and args.len() == 2 {
            fold_reusable[inst.anf_local.id] = pre.shell_reusable(args[0], last)
          } else {},
          .None => {},
        }
        // ... existing `case callee_func_id(callee) { ... }` body stays below ...
```

(Adjust the arm so the existing `case callee_func_id(callee) { ... }` logic that follows is preserved; the new lines only *add* a `fold_reusable` write and do not change any existing `verdicts`/`reusable_shell` write.)

- [ ] **Step 3: Add the `is_fold_push` helper** (near the other `sem`/builtin helpers in `ownership.tw`). It compares against the builder-family push ids:

```tw
// The fold-step ops for builder regions: String.concat and Vector.append.
fn is_fold_push(sem: OptimizerSemantics, fid: FuncId) Bool {
  fid.id == sem.string_builder.push_id.id or fid.id == sem.builder.push_id.id
}
```

(`OptimizerSemantics` already carries `builder` and `string_builder` `BuilderConfig`s — see `boot/compiler/opt/semantics.tw:20-21,209`.)

- [ ] **Step 4: Return and store the new map.** Change the `block_verdicts` return (line 2059) to:

```tw
  BlockVerdicts.{ texts: verdicts, reusable_shell, fold_reusable }
```

And at the write site (`ownership.tw:3343`, after `blk.exit.verdict_reusable_shell = verdicts.reusable_shell`):

```tw
    blk.exit.verdict_reusable_shell = verdicts.reusable_shell
    blk.exit.fold_reusable = verdicts.fold_reusable
```

- [ ] **Step 5: Write a failing test** that a clean loop's fold site is reusable and an aliased one is not. Add to the suite (uses `ownership_verdicts.compute_artifacts` + a small helper to read `fold_reusable`):

```tw
// in builder_region_suite.tw
use compiler.codegen.ownership_verdicts
use compiler.opt.semantics.{make_prelude_optimizer_semantics}

fn fold_reusable_true_count(src: String) Int {
  case pipeline.compile_source(src) {
    .Ok(a) => {
      sem := make_prelude_optimizer_semantics(a.builtins)
      arts := ownership_verdicts.compute_artifacts(a.opt, a.builtins, sem)
      n := 0
      for f in arts.analyzed.functions {
        for blk in f.blocks {
          for _k, v in blk.exit.fold_reusable {
            if v { n = n + 1 }
          }
        }
      }
      n
    },
    .Err(e) => error("compile failed: ${e}"),
  }
}
```

```tw
.test("clean fold site is reusable", fn() Result<Void, String> {
  n := fold_reusable_true_count("acc = \"\"\nfor c in [\"a\", \"b\"] { acc = acc.concat(c) }\nprintln(acc)\n")
  assert.is_true(n >= 1)
})
```

- [ ] **Step 6: Run.**

Run: `make boot-test`
Expected: PASS. If the fold site is not marked reusable, verify `is_fold_push` matches (`String.concat`/`Vector.append` ids) and that `pre.shell_reusable` is evaluated *before* `transfer_op` consumes the base (it is — `pre := st` is captured at the top of the loop body).

- [ ] **Step 7: Commit.**

```bash
git add boot/compiler/ownership.tw boot/tests/suites/builder_region_suite.tw
git commit -m "ownership: record fold-site reusability for builder-region condition 2"
```

---

### Task B3: Compose `linearly_folded` per region

**Files:**
- Create: `boot/compiler/builder_region_fact.tw`
- Test: `boot/tests/suites/builder_region_suite.tw`

- [ ] **Step 1: Write the fact module.** A region is `linearly_folded` iff detection produced it (structural conditions 3/4/5 + empty seed = condition 1) AND every fold site is `fold_reusable` (condition 2) in the converged facts.

```tw
//! Compose builder-region detection (conditions 1/3/4/5) with the ownership
//! analysis's fold-site reusability (condition 2) into the authoritative
//! linearly_folded fact. Analysis-owned; codegen (Plan 2) consumes it.

use compiler.anf.{AnfModule}
use compiler.builder_region_detect as detect
use compiler.builder_region_detect.{RegionShape, BuilderRegionKey}
use compiler.builtins.{BuiltinRegistry}
use compiler.codegen.ownership_verdicts
use compiler.opt.semantics.{OptimizerSemantics}

pub type RegionVerdict = .{
  key: BuilderRegionKey,
  func: String,
  family: String,
  linearly_folded: Bool,
  reason: String,
}

// Flatten converged fold_reusable into a "<func_id>#<local_id>" -> Bool table.
fn fold_reusable_table(artifacts: ownership_verdicts.OwnershipArtifacts) Dict<String, Bool> {
  out: Dict<String, Bool> = Dict.new()
  for f in artifacts.analyzed.functions {
    for blk in f.blocks {
      for local_id, v in blk.exit.fold_reusable {
        out["${f.func_id}#${local_id}"] = v
      }
    }
  }
  out
}

fn all_fold_sites_reusable(s: RegionShape, tbl: Dict<String, Bool>) Bool {
  for fs in s.fold_sites {
    ok := case tbl.get("${s.func_id.id}#${fs.id}") {
      .Some(v) => v,
      .None => false,
    }
    if !ok { return false }
  }
  s.fold_sites.len() > 0
}

pub fn region_verdicts(
  opt: AnfModule,
  b: BuiltinRegistry,
  artifacts: ownership_verdicts.OwnershipArtifacts,
) Vector<RegionVerdict> {
  shapes := detect.detect_regions(opt, b)
  tbl := fold_reusable_table(artifacts)
  out: Vector<RegionVerdict> = []
  for s in shapes {
    folded := all_fold_sites_reusable(s, tbl)
    reason := if folded {
      "certified: empty seed, clean fold, all fold sites unique"
    } else {
      "rejected: a fold site is not proven unique (condition 2)"
    }
    out = out.append(RegionVerdict.{
      key: s.region_key(),
      func: s.func,
      family: s.family,
      linearly_folded: folded,
      reason,
    })
  }
  out
}
```

- [ ] **Step 2: Write failing tests.**

```tw
// in builder_region_suite.tw
use compiler.builder_region_fact as region_fact

fn verdicts_for(src: String) Vector<region_fact.RegionVerdict> {
  case pipeline.compile_source(src) {
    .Ok(a) => {
      sem := make_prelude_optimizer_semantics(a.builtins)
      arts := ownership_verdicts.compute_artifacts(a.opt, a.builtins, sem)
      region_fact.region_verdicts(a.opt, a.builtins, arts)
    },
    .Err(e) => error("compile failed: ${e}"),
  }
}
```

```tw
.test("clean fold is certified linearly_folded", fn() Result<Void, String> {
  vs := verdicts_for("acc = \"\"\nfor c in [\"a\", \"b\"] { acc = acc.concat(c) }\nprintln(acc)\n")
  try assert.equal(vs.len(), 1)
  try assert.is_true(vs[0].linearly_folded)
  .Ok({})
})
```

- [ ] **Step 3: Run.**

Run: `make boot-test`
Expected: PASS — one certified region.

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/builder_region_fact.tw boot/tests/suites/builder_region_suite.tw
git commit -m "builder-region: compose linearly_folded fact (structural scan + ownership uniqueness)"
```

---

### Task B4: Stale-artifact guard + candidate-scoped equivalence

**Files:**
- Modify: `boot/compiler/builder_region_fact.tw`
- Test: `boot/tests/suites/builder_region_suite.tw`

- [ ] **Step 1: Add the fingerprint guard.** The fact must reject artifacts computed over a different ANF. Add at the top of `region_verdicts`, before using `artifacts`:

```tw
  expected := ownership_verdicts.artifact_key_for_anf(opt)
  if !expected.same_artifact_key(artifacts.key) {
    // Stale artifacts: every detected region falls back to not-folded.
    shapes := detect.detect_regions(opt, b)
    stale: Vector<RegionVerdict> = []
    for s in shapes {
      stale = stale.append(RegionVerdict.{
        key: s.region_key(),
        func: s.func,
        family: s.family,
        linearly_folded: false,
        reason: "rejected: stale ownership artifact",
      })
    }
    return stale
  }
```

- [ ] **Step 2: Write a scoped-vs-full equivalence test.** The scoped artifacts (`compute_candidate_artifacts` rooted on the region functions) must certify the same regions as full artifacts.

```tw
.test("scoped artifacts match full for region certification", fn() Result<Void, String> {
  src := "acc = \"\"\nfor c in [\"a\", \"b\"] { acc = acc.concat(c) }\nprintln(acc)\n"
  case pipeline.compile_source(src) {
    .Ok(a) => {
      sem := make_prelude_optimizer_semantics(a.builtins)
      full := ownership_verdicts.compute_artifacts(a.opt, a.builtins, sem)
      shapes := detect.detect_regions(a.opt, a.builtins)
      roots: Dict<Int, Bool> = Dict.new()
      for s in shapes { roots[s.func_id.id] = true }
      scoped := ownership_verdicts.compute_candidate_artifacts(a.opt, a.builtins, sem, roots)
      vf := region_fact.region_verdicts(a.opt, a.builtins, full)
      vs := region_fact.region_verdicts(a.opt, a.builtins, scoped)
      try assert.equal(vf.len(), vs.len())
      try assert.equal(vf[0].linearly_folded, vs[0].linearly_folded)
      .Ok({})
    },
    .Err(e) => .Err(e),
  }
})
```

- [ ] **Step 3: Run.**

Run: `make boot-test`
Expected: PASS.

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/builder_region_fact.tw boot/tests/suites/builder_region_suite.tw
git commit -m "builder-region: stale-artifact guard + scoped/full equivalence test"
```

---

## Phase C — Inspection render + verification

### Task C1: Render certified/rejected regions in `twk ir --census --sites`

**Files:**
- Modify: `boot/commands/ir.tw:65-97`

- [ ] **Step 1: Add a render helper + wire it into `render_census_report`.** After the mutable-audit block (around line 97, still inside `if include_sites`), append a builder-region table.

```tw
// add imports at the top of ir.tw
use compiler.builder_region_fact as region_fact
use compiler.opt.semantics.{make_prelude_optimizer_semantics}
```

```tw
// helper near render_census_report
fn render_region_rows(vs: Vector<region_fact.RegionVerdict>) String {
  out := "\nbuilder regions:\nfunc\tfamily\tlinearly_folded\treason\n"
  for v in vs {
    out = out.concat("${v.func}\t${v.family}\t${v.linearly_folded}\t${v.reason}\n")
  }
  out
}
```

Inside `render_census_report`, within `if include_sites` after the audit-rows concat:

```tw
    sem2 := make_prelude_optimizer_semantics(artifacts.builtins)
    region_arts := ownership_verdicts.compute_artifacts(artifacts.opt, artifacts.builtins, sem2)
    region_vs := region_fact.region_verdicts(artifacts.opt, artifacts.builtins, region_arts)
    out = out.concat(render_region_rows(region_vs))
```

(If `ownership_verdicts` / `make_prelude_optimizer_semantics` are already imported in `ir.tw`, do not re-import.)

- [ ] **Step 2: Rebuild the CLI (heavy; run alone).**

Run: `make bundle-cli`
Expected: builds `target/twk` with no errors.

- [ ] **Step 3: Manually verify the render on a fixture.**

```bash
printf 'acc = ""\nfor c in ["a","b"] { acc = acc.concat(c) }\nprintln(acc)\n' > /tmp/br.tw
target/twk ir /tmp/br.tw --census --sites
```

Expected: a `builder regions:` table with one row, `family=string`, `linearly_folded=true`.

- [ ] **Step 4: Commit.**

```bash
git add boot/commands/ir.tw
git commit -m "ir: render builder-region certified/rejected verdicts under --census --sites"
```

---

### Task C2: Full-suite + self-host + emission-invisible verification

**Files:** none (verification only).

- [ ] **Step 1: Run the full boot suite (alone).**

Run: `make boot-test`
Expected: all suites pass, including `builder_region`.

- [ ] **Step 2: Run the Rust reference suite for the analysis crate guardrails (alone, targeted).**

Run: `cargo test --release cow`
Expected: PASS — the census/COW guardrail (`tests/cow_analysis.rs`) is unchanged; Plan 1 adds no emission, so the census baseline must not move.

- [ ] **Step 2b: Confirm emission-invisible (byte-identical).** Plan 1 must not change generated code. Build a representative module before and after is impractical mid-branch; instead assert the self-host fixed point in Step 3, which subsumes byte-identity of the compiler's own output.

- [ ] **Step 3: Self-host fixed point (heavy; run alone).**

Run: `make stage2`
Expected: rebuilds `target/boot.wasm`; the self-host loop reaches a fixed point (stage3 == stage4). Since Plan 1 adds a fact + inspection only (no rewrite, no emission path), the compiler's generated output is unchanged and the fixed point holds.

- [ ] **Step 4: Commit any regenerated artifacts if the build tracks them.**

```bash
git status --short
# if target/boot.wasm or generated core_lib is tracked and changed by make stage2, commit it:
git add -A && git commit -m "builder-region: rebuild self-host payload after Plan 1 fact"
```

(If `git status` shows nothing, skip — nothing to commit.)

---

## Self-Review checklist (run before handing off)

- **Spec coverage:** conditions 1 (empty seed — `empty_seed_family`), 2 (`fold_reusable` + `all_fold_sites_reusable`), 3/4/5 (`analyze_loop_push_sites` rejection) all mapped to tasks; inspection gate = Task C1; stale guard = B4; scoped equivalence = B4. Publication/self-concat/interior-read negatives = A2 tests. ✓
- **Deferred to Plan 2 (not this plan):** the `BuilderRegionDecision` record, the ANF-to-ANF rewrite, `repr_assign` string-seed erasure, pipeline ANF′ ordering, and the boxed-`Vector<Int>` emission fixture. This plan surfaces the fact only.
- **Type consistency:** `RegionShape`/`BuilderRegionKey`/`RegionVerdict` field names match across A1/A3/B3/C1; `region_key` called as `s.region_key()` (inherent method — `RegionShape` is defined in `builder_region_detect`, so the method resolves). `fold_reusable` field name identical in `cfg.tw`, `ownership.tw`, `builder_region_fact.tw`.
- **Placeholder scan:** the only "placeholder" test (A1 Step 2) is explicitly replaced in A2 Step 1.

## Notes / risks for the executor

- **`block_verdicts` edit (B2) is the one soundness-sensitive change.** It must be purely *additive* — add a `fold_reusable` write, never alter an existing `verdicts` / `reusable_shell` write — so existing 8A/8B/8D/8E verdicts stay byte-identical. The self-host fixed point (C2) is the guard.
- **`shell_reusable` timing:** the fold-site query uses `pre` (state before the fold call), captured at the top of the instruction loop, and `last` from `last_use_at`. The incoming accumulator value is at last use at the fold call (consumed and rebound), so a unique carried accumulator yields `true` — exactly the 8B loop-carried property, applied to `concat`/`append` instead of `set_at`.
- **Detector conservatism:** `find_fold_loop` requires `a` untouched between its empty-seed bind and the folding loop, and the loop immediately foldable. This detects a sound subset; missed regions fall back persistently (acceptable for the first slice). Broadening seeds/observation is Plan 3+.
- **If `pipeline.compile_source` is not the exact symbol** (verify against `boot/tests/suites/dry_run_suite.tw`, which uses it), mirror that suite's `sites_for` helper import list.
