# Builder-Region `linearly_folded` Fact + Inspection — Implementation Plan (8C Plan 1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the analysis-owned `linearly_folded` safety fact for loop-carried builder regions (empty-seed, **unconditional**-fold `String.concat` / `Vector.append` accumulators) and render certified/rejected candidates in `twk ir --census --sites`. No emitted-code change.

**Architecture:** A shared, pure-ANF detector recognizes the empty-seed accumulator + break-dispatch loop skeleton, then applies a sound-**by-rejection** scan: the accumulator's only reference on the loop's main (non-break) path must be a single top-level fold — any other reference (interior read, nested/conditional fold, publication via `return`/value-`break`/closure/store/call) rejects. The detector output is composed with the current sound ownership analysis's uniqueness fact — computed by extending the existing per-block `block_verdicts` pass with a `fold_reusable` map, so **no new fixpoint** — into `linearly_folded`. The fact is surfaced through `ownership_verdicts.tw` keyed by a deterministic `BuilderRegionKey`, and rendered in the `ir` census (certified *and* rejected candidates). Plan 2 (later) adds the codegen producer/records and the rewrite; this plan stops at the fact + inspection.

**Tech Stack:** Twinkle (`.tw`), boot compiler (`boot/`). Tests via `@std.testing` suites compiled by `pipeline.compile_source`. Build/verify with `make boot-test`, then `make bundle-cli` for inspection/self-host.

---

## Background the engineer needs

- **Immutability:** `acc = acc.concat(c)` is *rebind*, lowered to `Let(r, ACall(concat, [acc, chunk]), Let(_, AAssign(acc, r), …))`. Loop bodies are `AnfOp.ALoop(body)`.
- **`=` vs `:=`:** `:=` declares; `=` rebinds. Fixtures MUST declare with `:=` and be **function-local** (top-level bindings lower to `AGlobalLocal`/`AGlobalSet`, which the detector does not match).
- **Design doc (read first):** `docs/plans/sound-uniqueness/codegen/builder-region-design.md`. This plan implements its "Plan 1" (Component 1 fact + inspection gate). First-slice scope: empty-seed, **unconditional single fold**, single post-loop freeze, reject all intra-region publication/early-exit. The five conditions: (1) seed uniqueness — vacuous for empty seeds; (2) carried uniqueness — the ownership query; (3) linear fold / no interior observation; (4) reject intra-region publication; (5) no self-alias.
- **Why:** builder lowering steals a proven-unique, linearly-folded accumulator into a transient builder; unsafe if the accumulator is observed mid-loop or published on an exit edge. The old `opt/loop_builder.tw`/`opt/liveness.tw` (deleted `5d5ac090`) were unsound only in the *uniqueness* input; we keep the structural rejection and replace uniqueness with the current ownership analysis.

## VERIFIED real ANF shapes (from `target/twk ir <fixture> --opt`)

For `fn m() Void { acc := ""; for c in ["a","b"] { acc = acc.concat(c) }; println(acc) }`:

```
let L0: String = init ""                       # string seed: AInit(ALitStr "")
let L1 = init L9 (L9 = ["a","b"]); L10 = call len(L1); L3 = init 0
let L18: Void = loop
    let L11: Bool = int.ge L3, L10              # exit condition
    let L17: Void = if L11 then break else       # BREAK-DISPATCH skeleton
        let L12 = index[array] L1, L3
        let L13 = call concat(L0, L12)           # FOLD at top level of the else (main) arm
        let L14 = assign L0 = L13                # consume-reassign
        ... L15 = int.add L3,1; L16 = assign L3=L15; continue
    L17
let L19 = call println(L0)                       # post-loop use
```

For the **vector** fixture `acc: Vector<Int> = []; … acc = acc.append(x)`:

```
let L9: Vector<Int> = []                        # empty AArrayLit temp
let L0: Vector<Int> = init L9                    # VECTOR seed: AInit(ALocal L9)
... loop ... let L14 = call append(L0, L13); assign L0 = L14 ...
```

For a **user-conditional** fold `if c == "a" { acc = acc.concat(c) }`, the fold nests in a **second** `AIf` inside the exit-else arm:

```
else (main arm):
    L12 = index; L4 = init L12; L13 = string.eq L4 "a"
    L16 = if L13 then { L14 = call concat(L0, L4); assign L0 = L14; void } else void   # NESTED → reject
    ... continue
```

**Detector rules that follow directly:**
1. Empty-seed accumulator = local bound by `AInit(x)` where `x` is `ALitStr("")` OR `ALocal(v)` with `v` bound by empty `AArrayLit([])`.
2. Loop body has a break-dispatch `AIf(cond, armA, armB)` where exactly one arm is `Break(.None)`; the other is the **main arm**.
3. On the main arm, the accumulator's ONLY reference must be a single top-level fold (`ACall(push,[acc,chunk])`, `chunk != acc`, consume-reassigned). Any other reference — nested `AIf`/`AMatch`, `return acc`, value `break acc`, closure capture, store, call arg — rejects (sound by rejection; conditions 3/4/5).

## Key API facts (verified)

- ANF (`boot/compiler/anf.tw:19-88`): `Atom = ALocal(LocalId)|AGlobalFunc(FuncId)|ALitInt|ALitFloat|ALitBool|ALitStr(String)|ALitVoid|AGlobalLocal`. `AnfExpr = Let(LocalId,AnfOp,AnfExpr)|Atom(Atom)|Return(Atom?)|Break(Atom?)|Continue`. `AnfOp` includes `ACall(Atom,Vector<Atom>)`, `AIf(Atom,AnfExpr,AnfExpr)`, `AMatch(Atom,Vector<AnfMatchArm>)`, `ALoop(AnfExpr)`, `ADefer`, `AAssign(LocalId,Atom)`, `AInit(Atom)`, `AArrayLit(Vector<Atom>)`, `ABinOp`, `AUnOp`, `AMakeClosure(FuncId,Vector<LocalId>)`, `ARecord`, `ARecordGet`, `ARecordUpdate`, `AVariant`, `AIndex`, `AGlobalSet`, `AWrapAnyref`, `AUnwrapAnyref`. `AnfFunctionDef.{ func_id, name, body }`, `AnfModule.{ functions }`.
- Builder families (`boot/compiler/builder_family.tw:43-51`): `string_builder_config(b).push_id == b.method_id("String","concat")`; `vector_builder_config(b).push_id == b.method_id("Vector","append")`. These are the exact FuncIds the lowered fold calls use.
- Ownership per-block verdicts (`boot/compiler/ownership.tw:1943` `block_verdicts`): forward walk with `st: ForwardState`, `pre := st` captured pre-op, `last := last_use_at(inst.op, scan.live_after[i])`, query `pre.shell_reusable(base, last)` (line 1774) → `Unique` + valid + last-use. Written to `BlockFacts` at `ownership.tw:3340-3343`.
- `OptimizerSemantics` carries `builder`/`string_builder` `BuilderConfig`s (`boot/compiler/opt/semantics.tw:20-21,209`).
- Artifacts (`ownership_verdicts.tw:296-360`): `compute_artifacts` / `compute_candidate_artifacts` → `OwnershipArtifacts.{ key, analyzed }`; `artifact_key_for_anf` + `same_artifact_key` gate staleness.
- Inspection (`boot/commands/ir.tw:65-97`): `render_census_report` computes `owned := dry_run…`/artifacts once and appends dry-run + audit tables under `include_sites`.
- Test harness (`boot/tests/suites/dry_run_suite.tw`): `use @std.testing.assert as assert`, `use @std.testing as runner`; `pub fn suite() runner.Suite` = `runner.suite("n").test("d", fn() Result<Void,String> { try assert.equal(…); .Ok({}) })`; `pipeline.compile_source(src) Result<PipelineArtifacts>` → `.opt`,`.builtins`. Register in `boot/tests/main.tw` (`use .suites.<n>` + add `<n>.suite()` to `runner.run_all([…])`).
- **Twinkle has no tuple types.** Use nominal `.{ … }` records for multi-value returns.

## Build / test loop

- **Unit tests (Phase A/B):** `make boot-test` — `target/twk` compiles edited `boot/compiler/*` from source at test time; no rebuild needed.
- **Inspection + self-host (Phase C):** `make bundle-cli` first, then `target/twk ir … --census --sites` / self-host. Run heavy commands (`make bundle-cli`, full suite, `make stage2`) **one at a time, never concurrently.**

## File Structure

- **Create `boot/compiler/builder_region_detect.tw`** — pure ANF detector: types (`RegionCandidate`, `BuilderRegionKey`, helper records), the break-dispatch skeleton + main-arm scan (conditions 3/4/5, sound by rejection), and `detect_candidates(m, b) Vector<RegionCandidate>` which emits **both** clean candidates and structurally-rejected near-misses (with reason). Analysis-track; shared by the fact and Plan 2's producer.
- **Modify `boot/compiler/cfg.tw:37-54`** — add `fold_reusable: Dict<Int, Bool>` to `BlockFacts` + `empty_block_facts()`.
- **Modify `boot/compiler/ownership.tw`** — extend `BlockVerdicts`/`block_verdicts` to record `fold_reusable` for `concat`/`append` folds; write into `blk.exit.fold_reusable`.
- **Create `boot/compiler/builder_region_fact.tw`** — compose candidate + converged `fold_reusable` (cond 2) + empty seed (cond 1) → `RegionVerdict{ key, family, func, linearly_folded, reason, helper_sequence }`; `region_verdicts(opt, b, artifacts)`; artifact-key fingerprinted. **Architecture note (intentional):** the design says "surface the fact through `ownership_verdicts.tw`." We instead put the region-fact surface in its own module that *consumes* `ownership_verdicts`' fingerprinted `OwnershipArtifacts` — the same fingerprint/staleness discipline, just a region-shaped verdict alongside the per-site ones. A forwarding API inside `ownership_verdicts.tw` would create an import cycle (`ownership_verdicts` → `builder_region_fact` → `ownership_verdicts`), so the dependency is kept one-directional. This matches the design's *intent* (facts surfaced via a fingerprinted-artifact consumer) if not the literal module name.
- **Modify `boot/commands/ir.tw:65-97`** — render certified/rejected candidates (full `BuilderRegionKey`, boundaries, helper sequence, proof id, reason) reusing the already-computed `owned` artifacts.
- **Create `boot/tests/suites/builder_region_suite.tw`** + register in `boot/tests/main.tw`.

---

## Phase A — Shared pure-ANF detector

### Task A1: Types + module skeleton

**Files:** Create `boot/compiler/builder_region_detect.tw`; create `boot/tests/suites/builder_region_suite.tw`.

- [ ] **Step 1: Module skeleton with nominal records (no tuples).**

```tw
//! Pure-ANF builder-region detector. Recognizes empty-seed accumulator loops
//! with a break-dispatch skeleton and applies a sound-BY-REJECTION scan: the
//! accumulator's only reference on the loop main path must be one top-level
//! fold. Emits clean candidates AND structurally-rejected near-misses (reason).
//! Shared by builder_region_fact.tw and the Plan 2 producer.

use compiler.anf.{AnfExpr, AnfMatchArm, AnfModule, AnfFunctionDef, AnfOp, Atom}
use compiler.builder_family.{string_builder_config, vector_builder_config}
use compiler.builtins.{BuiltinRegistry}
use compiler.core_ir.{FuncId, LocalId}

pub type BuilderRegionKey = .{
  func_id: Int,
  seed_site: Int,
  loop_site: Int,
  fold_sites: Vector<Int>,   // sorted ascending
  freeze_site: Int,          // Plan 1 has no rewrite/freeze: set = loop_site (documented proxy)
  family: String,
}

// A detected region candidate. `structural_ok` is true when conditions 3/4/5
// hold; when false, `reason` explains the structural rejection. `fold_sites`
// are the fold-call result locals (sorted).
pub type RegionCandidate = .{
  func_id: FuncId,
  func: String,
  family: String,
  push_id: FuncId,
  accumulator: LocalId,
  seed_site: LocalId,
  loop_site: LocalId,
  fold_sites: Vector<LocalId>,
  structural_ok: Bool,
  reason: String,
}

type SeedFamily = .{ push_id: FuncId, family: String }
type ScanResult = .{ ok: Bool, fold_sites: Vector<LocalId>, reason: String }

pub fn region_key(c: RegionCandidate) BuilderRegionKey {
  ids: Vector<Int> = collect fs in c.fold_sites { fs.id }
  BuilderRegionKey.{
    func_id: c.func_id.id,
    seed_site: c.seed_site.id,
    loop_site: c.loop_site.id,
    fold_sites: sort_ints(ids),
    freeze_site: c.loop_site.id,
    family: c.family,
  }
}

fn sort_ints(xs: Vector<Int>) Vector<Int> {
  // small insertion sort; fold_sites is tiny (first slice: 1)
  out := xs
  i := 1
  for i < out.len() {
    j := i
    for j > 0 and out[j - 1] > out[j] {
      tmp := out[j - 1]
      out[j - 1] = out[j]
      out[j] = tmp
      j = j - 1
    }
    i = i + 1
  }
  out
}

fn atom_is_local(a: Atom, local: LocalId) Bool {
  case a {
    .ALocal(id) => id.id == local.id,
    _ => false,
  }
}
```

- [ ] **Step 2: Test suite skeleton + registration.**

```tw
// boot/tests/suites/builder_region_suite.tw
use @std.testing.assert as assert
use @std.testing as runner

pub fn suite() runner.Suite {
  runner.suite("builder_region")
    .test("placeholder", fn() Result<Void, String> { assert.is_true(true) })
}
```

**Important (Twinkle gotcha):** the suite file must contain ONLY the placeholder here — do NOT
add `candidates_for`/`clean_count` helpers or `use compiler.builder_region_detect` yet. Twinkle
typechecks every top-level function in a compiled module even if unused, so referencing
`detect.detect_candidates` before A3 defines it is a hard compile error. The helpers + imports
are added in A3 Step 1 alongside the first tests that use them.

In `boot/tests/main.tw`: add `use .suites.builder_region_suite` and `builder_region_suite.suite(),` to `runner.run_all([…])`.

- [ ] **Step 3: Run.** `make boot-test` → PASS (placeholder), `builder_region` listed.
- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/builder_region_detect.tw boot/tests/suites/builder_region_suite.tw boot/tests/main.tw
git commit -m "builder-region: detector module skeleton + types (nominal records)"
```

---

### Task A2: Deep reference scan + main-arm unconditional-fold scan

**Files:** Modify `boot/compiler/builder_region_detect.tw`; tests in the suite.

- [ ] **Step 1: Add the reference scanners.** `expr_references` / `op_references_deep` detect ANY use of `local` (including inside branch arms), with a no-wildcard `AnfOp` enumeration so a new variant forces a compile error (silent `_ => false` would be unsound).

```tw
fn expr_references(e: AnfExpr, local: LocalId) Bool {
  case e {
    .Let(_, op, body) => op_references_deep(op, local) or expr_references(body, local),
    .Atom(a) => atom_is_local(a, local),
    .Return(.Some(a)) => atom_is_local(a, local),
    .Return(.None) => false,
    .Break(.Some(a)) => atom_is_local(a, local),
    .Break(.None) => false,
    .Continue => false,
  }
}

fn any_arg_is_local(args: Vector<Atom>, local: LocalId) Bool {
  for a in args { if atom_is_local(a, local) { return true } }
  false
}

fn op_references_deep(op: AnfOp, local: LocalId) Bool {
  case op {
    .ACall(callee, args) => atom_is_local(callee, local) or any_arg_is_local(args, local),
    .ABinOp(_, l, r, _) => atom_is_local(l, local) or atom_is_local(r, local),
    .AUnOp(_, inner, _) => atom_is_local(inner, local),
    .AMakeClosure(_, fvs) => {
      for v in fvs { if v.id == local.id { return true } }
      false
    },
    .ARecord(_, fields) => {
      for f in fields { if atom_is_local(f.value, local) { return true } }
      false
    },
    .ARecordGet(t, _, _) => atom_is_local(t, local),
    .ARecordUpdate(base, _, v, _, _) => atom_is_local(base, local) or atom_is_local(v, local),
    .AVariant(_, _, args) => any_arg_is_local(args, local),
    .AArrayLit(elems) => any_arg_is_local(elems, local),
    .AIndex(base, idx, _, _) => atom_is_local(base, local) or atom_is_local(idx, local),
    .AInit(v) => atom_is_local(v, local),
    .AAssign(target, v) => target.id == local.id or atom_is_local(v, local),
    .AGlobalSet(_, v) => atom_is_local(v, local),
    .AIf(cond, then_e, else_e) => atom_is_local(cond, local)
      or expr_references(then_e, local) or expr_references(else_e, local),
    .AMatch(scrutinee, arms) => {
      if atom_is_local(scrutinee, local) { return true }
      for arm in arms { if expr_references(arm.body, local) { return true } }
      false
    },
    .ALoop(body) => expr_references(body, local),
    .ADefer(body) => expr_references(body, local),
    .AWrapAnyref(a, _) => atom_is_local(a, local),
    .AUnwrapAnyref(a, _) => atom_is_local(a, local),
  }
}
```

- [ ] **Step 2: Add the fold matcher + main-arm scan.** The accumulator's only reference on the main arm must be one top-level fold.

```tw
// A fold: ACall(push,[acc,chunk]) with chunk != acc, whose result is
// consume-reassigned into acc in the immediately following Let.
fn fold_chunk(op: AnfOp, body: AnfExpr, acc: LocalId, push_id: FuncId, result: LocalId) Atom? {
  case op {
    .ACall(.AGlobalFunc(f), args) => if f.id == push_id.id
      and args.len() == 2 and atom_is_local(args[0], acc) and !atom_is_local(args[1], acc)
      and is_consume_reassign(body, acc, result) {
      .Some(args[1])
    } else {
      .None
    },
    _ => .None,
  }
}

fn is_consume_reassign(body: AnfExpr, acc: LocalId, result: LocalId) Bool {
  case body {
    .Let(_, .AAssign(target, .ALocal(v)), _) => target.id == acc.id and v.id == result.id,
    _ => false,
  }
}

// Control-flow ops are rejected on the loop main arm (first slice: unconditional).
fn is_control_flow_op(op: AnfOp) Bool {
  case op {
    .AIf(_, _, _) => true,
    .AMatch(_, _) => true,
    .ALoop(_) => true,
    .ADefer(_) => true,
    _ => false,
  }
}

// Scan the loop main arm: exactly one top-level fold, and acc referenced
// NOWHERE else on the arm (condition 3/4/5, sound by rejection).
fn scan_main_arm(arm: AnfExpr, acc: LocalId, push_id: FuncId, folds: Vector<LocalId>) ScanResult {
  case arm {
    .Let(local, op, body) => case fold_chunk(op, body, acc, push_id, local) {
      .Some(_) => {
        // fold at top level; skip past the AAssign, continue scanning the tail
        case body {
          .Let(_, _, rest) => scan_main_arm(rest, acc, push_id, folds.append(local)),
          _ => ScanResult.{ ok: false, fold_sites: [], reason: "malformed fold tail" },
        }
      },
      .None => if is_control_flow_op(op) {
        // Any branch/loop on the main arm makes the fold conditional or
        // continue-guarded (verified: a `if skip { continue }` guard lowers to a
        // top-level AIf here, then the fold follows). First slice is UNCONDITIONAL
        // only, so reject all control flow on the main arm. Deferred to Plan 5.
        ScanResult.{ ok: false, fold_sites: [], reason: "control flow on the loop main arm (conditional/continue-guarded fold) — deferred to Plan 5" }
      } else if op_references_deep(op, acc) {
        ScanResult.{ ok: false, fold_sites: [], reason: "accumulator referenced outside the fold (interior read / capture / publication)" }
      } else {
        scan_main_arm(body, acc, push_id, folds)
      },
    },
    .Atom(a) => reject_if_acc(a, acc, folds),
    .Return(.Some(a)) => reject_if_acc(a, acc, folds),
    // Value-carrying break is checker-rejected in source ("break with a value is
    // not supported"), so unreachable from first-slice source; kept defensive.
    .Break(.Some(a)) => reject_if_acc(a, acc, folds),
    .Return(.None) => finish(folds),
    .Break(.None) => finish(folds),
    .Continue => finish(folds),
  }
}

fn reject_if_acc(a: Atom, acc: LocalId, folds: Vector<LocalId>) ScanResult {
  if atom_is_local(a, acc) {
    ScanResult.{ ok: false, fold_sites: [], reason: "accumulator published on an exit edge" }
  } else {
    finish(folds)
  }
}

// First slice: exactly one unconditional fold.
fn finish(folds: Vector<LocalId>) ScanResult {
  if folds.len() == 1 {
    ScanResult.{ ok: true, fold_sites: folds, reason: "" }
  } else {
    ScanResult.{ ok: false, fold_sites: [], reason: "expected exactly one unconditional fold, found ${folds.len()}" }
  }
}
```

- [ ] **Step 3: (No standalone test yet — exercised via A3.) Commit.**

```bash
git add boot/compiler/builder_region_detect.tw
git commit -m "builder-region: deep reference scan + unconditional main-arm fold scan"
```

---

### Task A3: Break-dispatch skeleton + empty-seed detection + `detect_candidates`

**Files:** Modify `boot/compiler/builder_region_detect.tw`; tests in the suite.

- [ ] **Step 1: Failing tests** (real ANF-grounded fixtures, function-local `:=`).

First add the imports + helpers to `builder_region_suite.tw` (these were intentionally held back
from A1 — `detect_candidates` did not exist yet). At the top with the other `use` lines:

```tw
use compiler.builder_region_detect as detect
use compiler.pipeline
```

Above `pub fn suite()`, add the helpers:

```tw
fn candidates_for(src: String) Vector<detect.RegionCandidate> {
  case pipeline.compile_source(src) {
    .Ok(a) => detect.detect_candidates(a.opt, a.builtins),
    .Err(e) => error("compile failed: ${e}"),
  }
}

// count only structurally-clean candidates
fn clean_count(src: String) Int {
  n := 0
  for c in candidates_for(src) { if c.structural_ok { n = n + 1 } }
  n
}
```

Then replace the placeholder test with these (note: `detect_candidates` is added in this task's
Step 3, so these tests fail until then — that is the intended TDD red state):

```tw
// replace the placeholder test
.test("clean string fold is detected", fn() Result<Void, String> {
  assert.equal(clean_count("fn m() Void {\n  acc := \"\"\n  for c in [\"a\", \"b\"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n"), 1)
})
.test("clean vector fold is detected", fn() Result<Void, String> {
  assert.equal(clean_count("fn m() Void {\n  acc: Vector<Int> = []\n  for x in [1, 2] { acc = acc.append(x) }\n  println(acc.len().to_string())\n}\nm()\n"), 1)
})
.test("interior read rejects", fn() Result<Void, String> {
  assert.equal(clean_count("fn m() Void {\n  acc := \"\"\n  for c in [\"a\"] { acc = acc.concat(c)\n  println(acc) }\n  println(acc)\n}\nm()\n"), 0)
})
.test("conditional fold rejects (deferred to plan 5)", fn() Result<Void, String> {
  assert.equal(clean_count("fn m() Void {\n  acc := \"\"\n  for c in [\"a\"] { if c == \"a\" { acc = acc.concat(c) } }\n  println(acc)\n}\nm()\n"), 0)
})
.test("early return of accumulator rejects", fn() Result<Void, String> {
  assert.equal(clean_count("fn m() String {\n  acc := \"\"\n  for c in [\"a\"] { acc = acc.concat(c)\n  return acc }\n  acc\n}\nprintln(m())\n"), 0)
})
.test("self concat rejects", fn() Result<Void, String> {
  assert.equal(clean_count("fn m() Void {\n  acc := \"\"\n  for c in [\"a\"] { acc = acc.concat(acc) }\n  println(acc)\n}\nm()\n"), 0)
})
```

- [ ] **Step 2: Run to verify failure.** `make boot-test` → FAIL (`detect_candidates` undefined).

- [ ] **Step 3: Add empty-seed recognition, skeleton match, and the walk.**

```tw
// Track locals bound to an empty array literal so `AInit(ALocal v)` seeds are
// recognized (vector seeds lower via a temp; string seeds are direct).
fn is_empty_array_value(op: AnfOp) Bool {
  case op {
    .AArrayLit(elems) => elems.len() == 0,
    _ => false,
  }
}

// Is `op` an empty seed for `local`? Returns the family. `empty_arr` is the set
// of locals bound to an empty AArrayLit earlier in the same chain.
fn seed_family(op: AnfOp, str_push: FuncId, vec_push: FuncId, empty_arr: Dict<Int, Bool>) SeedFamily? {
  case op {
    .AInit(.ALitStr(s)) => if s.len() == 0 { .Some(SeedFamily.{ push_id: str_push, family: "string" }) } else { .None },
    .AInit(.ALocal(v)) => case empty_arr.get(v.id) {
      .Some(_) => .Some(SeedFamily.{ push_id: vec_push, family: "vector" }),
      .None => .None,
    },
    _ => .None,
  }
}

// The exit arm of a loop's break-dispatch is exactly `Break(.None)`.
fn is_break_exit(e: AnfExpr) Bool {
  case e { .Break(.None) => true, _ => false }
}

// Find the break-dispatch AIf on the loop body's top-level chain and return its
// non-break (main) arm. `acc` must not be referenced before that AIf.
fn loop_main_arm(body: AnfExpr, acc: LocalId) AnfExpr? {
  case body {
    .Let(_, op, rest) => case op {
      .AIf(_, then_e, else_e) => if is_break_exit(then_e) and !is_break_exit(else_e) {
        .Some(else_e)
      } else {
        if is_break_exit(else_e) and !is_break_exit(then_e) { .Some(then_e) } else { .None }
      },
      _ => if op_references_deep(op, acc) { .None } else { loop_main_arm(rest, acc) },
    },
    _ => .None,
  }
}

type FoundRegion = .{ loop_site: LocalId, scan: ScanResult }

// From `acc`'s seed binding, scan forward for the loop that folds `acc`. A loop
// that folds `acc` yields the candidate (a rejected scan still yields one so
// near-misses render). A loop or op that merely *references* `acc` without
// folding it rejects (interior observation). An unrelated loop/op that never
// touches `acc` is skipped and scanning continues. `.None` when no fold-bearing
// loop for `acc` is reached before the chain ends or `acc` is observed.
fn find_region(after_seed: AnfExpr, acc: LocalId, push_id: FuncId) FoundRegion? {
  case after_seed {
    .Let(local, op, body) => case op {
      .ALoop(loop_body) => {
        main_opt := loop_main_arm(loop_body, acc)
        folds := case main_opt {
          .Some(m) => references_fold(m, acc, push_id),
          .None => references_fold(loop_body, acc, push_id),
        }
        if folds {
          scan := case main_opt {
            .Some(m) => scan_main_arm(m, acc, push_id, []),
            .None => ScanResult.{ ok: false, fold_sites: [], reason: "fold not reachable via a break-dispatch main arm" },
          }
          .Some(FoundRegion.{ loop_site: local, scan })
        } else if op_references_deep(op, acc) {
          .None   // acc observed in an unrelated loop — reject
        } else {
          find_region(body, acc, push_id)   // unrelated loop — keep scanning
        }
      },
      _ => if op_references_deep(op, acc) { .None } else { find_region(body, acc, push_id) },
    },
    _ => .None,
  }
}

// Does `e` contain a fold call ACall(push,[acc,...]) anywhere (to distinguish a
// rejected fold-bearing region from an unrelated loop)?
fn references_fold(e: AnfExpr, acc: LocalId, push_id: FuncId) Bool {
  case e {
    .Let(local, op, body) => case fold_chunk(op, body, acc, push_id, local) {
      .Some(_) => true,
      .None => op_fold_ref(op, acc, push_id) or references_fold(body, acc, push_id),
    },
    _ => false,
  }
}

fn op_fold_ref(op: AnfOp, acc: LocalId, push_id: FuncId) Bool {
  case op {
    .AIf(_, t, e) => references_fold(t, acc, push_id) or references_fold(e, acc, push_id),
    .AMatch(_, arms) => {
      for arm in arms { if references_fold(arm.body, acc, push_id) { return true } }
      false
    },
    .ALoop(b) => references_fold(b, acc, push_id),
    .ADefer(b) => references_fold(b, acc, push_id),
    _ => false,
  }
}
```

Now the driver that produces candidates (tracking empty-array-value locals):

```tw
fn detect_in_expr(
  expr: AnfExpr,
  func_id: FuncId,
  func: String,
  str_push: FuncId,
  vec_push: FuncId,
  empty_arr: Dict<Int, Bool>,
  acc: Vector<RegionCandidate>,
) Vector<RegionCandidate> {
  case expr {
    .Let(local, op, body) => {
      // record empty-array-value locals for vector-seed recognition
      ea := empty_arr
      if is_empty_array_value(op) { ea[local.id] = true }

      out := case seed_family(op, str_push, vec_push, ea) {
        .Some(sf) => case find_region(body, local, sf.push_id) {
          .Some(fr) => acc.append(RegionCandidate.{
            func_id,
            func,
            family: sf.family,
            push_id: sf.push_id,
            accumulator: local,
            seed_site: local,
            loop_site: fr.loop_site,
            fold_sites: fr.scan.fold_sites,
            structural_ok: fr.scan.ok,
            reason: fr.scan.reason,
          }),
          .None => acc,
        },
        .None => acc,
      }
      out2 := detect_in_op(op, func_id, func, str_push, vec_push, ea, out)
      detect_in_expr(body, func_id, func, str_push, vec_push, ea, out2)
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
  empty_arr: Dict<Int, Bool>,
  acc: Vector<RegionCandidate>,
) Vector<RegionCandidate> {
  case op {
    .AIf(_, t, e) => {
      a := detect_in_expr(t, func_id, func, str_push, vec_push, empty_arr, acc)
      detect_in_expr(e, func_id, func, str_push, vec_push, empty_arr, a)
    },
    .AMatch(_, arms) => {
      cur := acc
      for arm in arms { cur = detect_in_expr(arm.body, func_id, func, str_push, vec_push, empty_arr, cur) }
      cur
    },
    .ALoop(b) => detect_in_expr(b, func_id, func, str_push, vec_push, empty_arr, acc),
    .ADefer(b) => detect_in_expr(b, func_id, func, str_push, vec_push, empty_arr, acc),
    _ => acc,
  }
}

pub fn detect_candidates(m: AnfModule, b: BuiltinRegistry) Vector<RegionCandidate> {
  str_push := string_builder_config(b).push_id
  vec_push := vector_builder_config(b).push_id
  out: Vector<RegionCandidate> = []
  for f in m.functions {
    out = detect_in_expr(f.body, f.func_id, f.name, str_push, vec_push, Dict.new(), out)
  }
  out
}
```

- [ ] **Step 4: Run.** `make boot-test` → all A3 tests PASS (string+vector detected; interior-read / conditional / early-return / self-concat all reject).

If a test fails, dump the fixture's ANF (`target/twk ir /tmp/x.tw --opt`) and reconcile the skeleton match — the break-dispatch shape is verified above for index-based `for` loops.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/builder_region_detect.tw boot/tests/suites/builder_region_suite.tw
git commit -m "builder-region: empty-seed + break-dispatch detection with rejected near-misses"
```

---

### Task A4: Publication/capture negative coverage

**Files:** tests only.

- [ ] **Step 1: Add the design's required negatives** (all must reject via `op_references_deep`).

```tw
.test("continue-guarded fold rejects (deferred to plan 5)", fn() Result<Void, String> {
  assert.equal(clean_count("fn m() Void {\n  acc := \"\"\n  for c in [\"a\", \"b\"] { if c == \"b\" { continue }\n  acc = acc.concat(c) }\n  println(acc)\n}\nm()\n"), 0)
})
.test("Cell publication of accumulator rejects", fn() Result<Void, String> {
  assert.equal(clean_count("fn m() Void {\n  cell := Cell.new(\"\")\n  acc := \"\"\n  for c in [\"a\"] { acc = acc.concat(c)\n  cell.set(acc) }\n  println(cell.get())\n}\nm()\n"), 0)
})
.test("chunk aliasing the accumulator rejects", fn() Result<Void, String> {
  assert.equal(clean_count("fn m() Void {\n  acc := \"\"\n  for c in [\"a\"] { d := acc\n  acc = acc.concat(d) }\n  println(acc)\n}\nm()\n"), 0)
})
.test("nested loop over the accumulator rejects", fn() Result<Void, String> {
  assert.equal(clean_count("fn m() Void {\n  acc := \"\"\n  for c in [\"a\"] { for d in [\"x\"] { acc = acc.concat(d) } }\n  println(acc)\n}\nm()\n"), 0)
})
.test("closure capture of accumulator rejects", fn() Result<Void, String> {
  assert.equal(clean_count("fn m() Void {\n  acc := \"\"\n  for c in [\"a\"] { acc = acc.concat(c)\n  f := fn() String { acc }\n  println(f()) }\n  println(acc)\n}\nm()\n"), 0)
})
.test("call publication of accumulator rejects", fn() Result<Void, String> {
  assert.equal(clean_count("fn use_it(s: String) Void { println(s) }\nfn m() Void {\n  acc := \"\"\n  for c in [\"a\"] { acc = acc.concat(c)\n  use_it(acc) }\n  println(acc)\n}\nm()\n"), 0)
})
.test("non-empty seed is not detected", fn() Result<Void, String> {
  assert.equal(clean_count("fn m() Void {\n  acc := \"x\"\n  for c in [\"a\"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n"), 0)
})
.test("two independent regions in one function both detected", fn() Result<Void, String> {
  assert.equal(clean_count("fn m() Void {\n  a := \"\"\n  for c in [\"x\"] { a = a.concat(c) }\n  b := \"\"\n  for c in [\"y\"] { b = b.concat(c) }\n  println(a)\n  println(b)\n}\nm()\n"), 2)
})
```

- [ ] **Step 2: Run.** `make boot-test` → PASS.
- [ ] **Step 3: Commit.**

```bash
git add boot/tests/suites/builder_region_suite.tw
git commit -m "builder-region: publication/capture/non-empty-seed/multi-region negatives"
```

---

## Phase B — The `linearly_folded` fact (rides the ownership pass)

### Task B1: Add `fold_reusable` to `BlockFacts`

**Files:** Modify `boot/compiler/cfg.tw:37-54`.

- [ ] **Step 1:** In `BlockFacts` after `verdict_reusable_shell: Dict<Int, Bool>,`:

```tw
  // Set at a fold call (String.concat / Vector.append) whose base is the
  // accumulator: true iff the base is Unique + valid + at last use there
  // (builder-region condition 2). Keyed by the fold-call result local.
  fold_reusable: Dict<Int, Bool>,
```

In `empty_block_facts()` after `verdict_reusable_shell: Dict.new(),`: `fold_reusable: Dict.new(),`.

- [ ] **Step 2:** `make boot-test` → PASS (if another `BlockFacts.{…}` literal exists and the compiler flags a missing field, add `fold_reusable: Dict.new()` there).
- [ ] **Step 3:** Commit `git commit -am "cfg: add fold_reusable BlockFacts map (builder-region condition 2)"`.

---

### Task B2: Record `fold_reusable` in `block_verdicts`

**Files:** Modify `boot/compiler/ownership.tw` (`BlockVerdicts` 1941; `block_verdicts` 1943-2059; write 3340-3343).

- [ ] **Step 1:** `BlockVerdicts` gains a field:

```tw
type BlockVerdicts = .{ texts: Dict<Int, String>, reusable_shell: Dict<Int, Bool>, fold_reusable: Dict<Int, Bool> }
```

In `block_verdicts`, after `reusable_shell: Dict<Int, Bool> = Dict.new()` add `fold_reusable: Dict<Int, Bool> = Dict.new()`.

- [ ] **Step 2:** Add the `is_fold_push` helper (near other `sem` helpers):

```tw
fn is_fold_push(sem: OptimizerSemantics, fid: FuncId) Bool {
  fid.id == sem.string_builder.push_id.id or fid.id == sem.builder.push_id.id
}
```

- [ ] **Step 3:** In the instruction loop's `.ACall(callee, args) =>` arm, **add** a fold-reusability write at the top of the arm without altering the existing body. Wrap the arm:

```tw
      .ACall(callee, args) => {
        case callee_func_id(callee) {
          .Some(fid) => if is_fold_push(sem, fid) and args.len() == 2 {
            fold_reusable[inst.anf_local.id] = pre.shell_reusable(args[0], last)
          } else {},
          .None => {},
        }
        // >>> the existing `case callee_func_id(callee) { ... }` body is preserved verbatim here <<<
      },
```

(This only *adds* a `fold_reusable` write; it must not change any existing `verdicts`/`reusable_shell` write — that keeps 8A/8B/8D/8E byte-identical.)

- [ ] **Step 4:** Return the map: `BlockVerdicts.{ texts: verdicts, reusable_shell, fold_reusable }` (line 2059). Write it at 3343: `blk.exit.fold_reusable = verdicts.fold_reusable`.

- [ ] **Step 5:** Test — a clean fold site is reusable:

```tw
// in the suite
use compiler.codegen.ownership_verdicts
use compiler.opt.semantics.{make_prelude_optimizer_semantics}

fn fold_reusable_count(src: String) Int {
  case pipeline.compile_source(src) {
    .Ok(a) => {
      sem := make_prelude_optimizer_semantics(a.builtins)
      arts := ownership_verdicts.compute_artifacts(a.opt, a.builtins, sem)
      n := 0
      for f in arts.analyzed.functions {
        for blk in f.blocks {
          for _k, v in blk.exit.fold_reusable { if v { n = n + 1 } }
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
  assert.is_true(fold_reusable_count("fn m() Void {\n  acc := \"\"\n  for c in [\"a\", \"b\"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n") >= 1)
})
```

- [ ] **Step 6:** `make boot-test` → PASS.
- [ ] **Step 7:** Commit `git commit -am "ownership: record fold-site reusability (builder-region condition 2)"`.

---

### Task B3: Compose `linearly_folded`

**Files:** Create `boot/compiler/builder_region_fact.tw`; tests in the suite.

- [ ] **Step 1: Fact module.** A candidate is `linearly_folded` iff `structural_ok` (conditions 1/3/4/5) AND every fold site is `fold_reusable` (condition 2). Emits certified **and** rejected verdicts.

```tw
//! Compose builder-region detection (conditions 1/3/4/5) with the ownership
//! analysis's fold-site reusability (condition 2) into linearly_folded.

use compiler.anf.{AnfModule}
use compiler.builder_region_detect as detect
use compiler.builder_region_detect.{RegionCandidate, BuilderRegionKey}
use compiler.builtins.{BuiltinRegistry}
use compiler.codegen.ownership_verdicts

pub type RegionVerdict = .{
  key: BuilderRegionKey,
  func: String,
  family: String,
  linearly_folded: Bool,
  helper_sequence: String,
  proof_id: String,
  reason: String,
}

fn helper_sequence(family: String) String {
  if family == "string" {
    "string$builder_from(\"\") -> builder_extend* -> builder_freeze"
  } else {
    "vector$builder_new -> builder_push* -> builder_freeze"
  }
}

fn fold_reusable_table(artifacts: ownership_verdicts.OwnershipArtifacts) Dict<String, Bool> {
  out: Dict<String, Bool> = Dict.new()
  for f in artifacts.analyzed.functions {
    for blk in f.blocks {
      for local_id, v in blk.exit.fold_reusable { out["${f.func_id}#${local_id}"] = v }
    }
  }
  out
}

fn all_folds_reusable(c: RegionCandidate, tbl: Dict<String, Bool>) Bool {
  for fs in c.fold_sites {
    ok := case tbl.get("${c.func_id.id}#${fs.id}") { .Some(v) => v, .None => false }
    if !ok { return false }
  }
  c.fold_sites.len() > 0
}

// Pure composition, exposed for condition-2 unit tests: given candidates, the
// staleness flag, and the fold-reusable table, produce verdicts.
pub fn region_verdicts_from_table(
  cands: Vector<RegionCandidate>,
  stale: Bool,
  tbl: Dict<String, Bool>,
) Vector<RegionVerdict> {
  out: Vector<RegionVerdict> = []
  for c in cands {
    folded := c.structural_ok and !stale and all_folds_reusable(c, tbl)
    reason := if !c.structural_ok {
      "rejected (structural): ${c.reason}"
    } else if stale {
      "rejected: stale ownership artifact"
    } else if folded {
      "certified: empty seed, unconditional clean fold, unique base"
    } else {
      "rejected: fold base not proven unique (condition 2)"
    }
    proof_id := "phase8c-region:${c.func}:seed L${c.seed_site.id}:loop L${c.loop_site.id}"
    out = out.append(RegionVerdict.{
      key: c.region_key(),
      func: c.func,
      family: c.family,
      linearly_folded: folded,
      helper_sequence: helper_sequence(c.family),
      proof_id,
      reason,
    })
  }
  out
}

pub fn region_verdicts(
  opt: AnfModule,
  b: BuiltinRegistry,
  artifacts: ownership_verdicts.OwnershipArtifacts,
) Vector<RegionVerdict> {
  cands := detect.detect_candidates(opt, b)
  expected := ownership_verdicts.artifact_key_for_anf(opt)
  stale := !expected.same_artifact_key(artifacts.key)
  tbl := if stale { Dict.new() } else { fold_reusable_table(artifacts) }
  region_verdicts_from_table(cands, stale, tbl)
}
```

- [ ] **Step 2: Tests** (certified + a condition-2 rejection exercised directly via the pure `region_verdicts_from_table` with a stubbed fold-reusable table — this is the reliable way to cover condition 2, since empty-seed clean-structure folds are almost always unique in practice):

```tw
// in the suite
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

fn certified_count(src: String) Int {
  n := 0
  for v in verdicts_for(src) { if v.linearly_folded { n = n + 1 } }
  n
}
```

```tw
.test("clean fold is certified", fn() Result<Void, String> {
  assert.equal(certified_count("fn m() Void {\n  acc := \"\"\n  for c in [\"a\", \"b\"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n"), 1)
})
.test("rejected candidate still appears (as not-folded)", fn() Result<Void, String> {
  vs := verdicts_for("fn m() Void {\n  acc := \"\"\n  for c in [\"a\"] { if c == \"a\" { acc = acc.concat(c) } }\n  println(acc)\n}\nm()\n")
  // conditional fold: appears as a rejected candidate, not certified
  try assert.equal(vs.len(), 1)
  try assert.is_false(vs[0].linearly_folded)
  try assert.str_contains(vs[0].reason, "structural")
  .Ok({})
})
.test("condition 2: non-unique fold base rejects (stubbed table)", fn() Result<Void, String> {
  src := "fn m() Void {\n  acc := \"\"\n  for c in [\"a\", \"b\"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n"
  case pipeline.compile_source(src) {
    .Ok(a) => {
      cands := detect.detect_candidates(a.opt, a.builtins)
      try assert.equal(cands.len(), 1)
      // structurally clean, but every fold site marked NOT reusable → rejected
      false_tbl: Dict<String, Bool> = Dict.new()
      for fs in cands[0].fold_sites { false_tbl["${cands[0].func_id.id}#${fs.id}"] = false }
      rejected := region_fact.region_verdicts_from_table(cands, false, false_tbl)
      try assert.is_false(rejected[0].linearly_folded)
      try assert.str_contains(rejected[0].reason, "condition 2")
      // same candidate, fold site marked reusable → certified
      true_tbl: Dict<String, Bool> = Dict.new()
      for fs in cands[0].fold_sites { true_tbl["${cands[0].func_id.id}#${fs.id}"] = true }
      certified := region_fact.region_verdicts_from_table(cands, false, true_tbl)
      try assert.is_true(certified[0].linearly_folded)
      .Ok({})
    },
    .Err(e) => .Err(e),
  }
})
```

- [ ] **Step 3:** `make boot-test` → PASS.
- [ ] **Step 4:** Commit `git add boot/compiler/builder_region_fact.tw boot/tests/suites/builder_region_suite.tw && git commit -m "builder-region: compose linearly_folded fact + stale guard"`.

---

### Task B4: Stale-artifact + scoped/full equivalence tests

**Files:** tests only (stale handling already in B3).

- [ ] **Step 1: Tests.**

```tw
.test("scoped artifacts match full for certification", fn() Result<Void, String> {
  src := "fn m() Void {\n  acc := \"\"\n  for c in [\"a\", \"b\"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n"
  case pipeline.compile_source(src) {
    .Ok(a) => {
      sem := make_prelude_optimizer_semantics(a.builtins)
      full := ownership_verdicts.compute_artifacts(a.opt, a.builtins, sem)
      cands := detect.detect_candidates(a.opt, a.builtins)
      roots: Dict<Int, Bool> = Dict.new()
      for c in cands { roots[c.func_id.id] = true }
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
.test("stale artifact rejects", fn() Result<Void, String> {
  // artifacts for one module used against a different opt fingerprint
  src_a := "fn m() Void {\n  acc := \"\"\n  for c in [\"a\"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n"
  src_b := "fn m() Void {\n  acc := \"\"\n  for c in [\"a\", \"b\", \"c\"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n"
  case pipeline.compile_source(src_a) {
    .Ok(a) => case pipeline.compile_source(src_b) {
      .Ok(bb) => {
        sem := make_prelude_optimizer_semantics(a.builtins)
        arts_b := ownership_verdicts.compute_artifacts(bb.opt, bb.builtins, sem)
        vs := region_fact.region_verdicts(a.opt, a.builtins, arts_b) // mismatched
        for v in vs { try assert.is_false(v.linearly_folded) }
        .Ok({})
      },
      .Err(e) => .Err(e),
    },
    .Err(e) => .Err(e),
  }
})
```

- [ ] **Step 2:** `make boot-test` → PASS.
- [ ] **Step 3:** Commit `git commit -am "builder-region: scoped/full equivalence + stale-artifact tests"`.

---

## Phase C — Inspection render + verification

### Task C1: Render certified/rejected candidates in `--census --sites`

**Files:** Modify `boot/commands/ir.tw:65-97`.

- [ ] **Step 1: Reuse the already-computed `owned` artifacts** (do NOT recompute ownership). In `render_census_report`, the `include_sites` branch already binds `owned` (the dry-run artifacts). Add after the audit block:

```tw
// imports at top of ir.tw (skip if already present)
use compiler.builder_region_fact as region_fact
```

```tw
fn render_region_rows(vs: Vector<region_fact.RegionVerdict>) String {
  out := "\nbuilder regions:\nfunc\tfamily\tlinearly_folded\tkey\thelpers\tproof\treason\n"
  for v in vs {
    fs := "["
    for id, i in v.key.fold_sites { fs = fs.concat(if i > 0 { ",${id}" } else { "${id}" }) }
    fs = fs.concat("]")
    key := "fn${v.key.func_id} seed L${v.key.seed_site} loop L${v.key.loop_site} folds ${fs} freeze L${v.key.freeze_site} ${v.key.family}"
    out = out.concat("${v.func}\t${v.family}\t${v.linearly_folded}\t${key}\t${v.helper_sequence}\t${v.proof_id}\t${v.reason}\n")
  }
  out
}
```

Inside `render_census_report`, after the `mutable_audit` concat, reusing `owned`:

```tw
    region_vs := region_fact.region_verdicts(artifacts.opt, artifacts.builtins, owned)
    out = out.concat(render_region_rows(region_vs))
```

(`owned` is the `OwnershipArtifacts` bound at `ir.tw:72` — verified — and already shared by the dry-run and audit tables. Reuse it; do not call `compute_artifacts` again.)

- [ ] **Step 2:** `make bundle-cli` (heavy, alone) → builds `target/twk`.
- [ ] **Step 3: Manual verify.**

```bash
printf 'fn m() Void {\n  acc := ""\n  for c in ["a","b"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n' > /tmp/br.tw
target/twk ir /tmp/br.tw --census --sites
```

Expected: a `builder regions:` row, `family=string`, `linearly_folded=true`, a full key, helper sequence, proof id.

- [ ] **Step 4:** Commit `git add boot/commands/ir.tw && git commit -m "ir: render builder-region certified/rejected candidates (reusing artifacts)"`.

---

### Task C2: Full-suite + self-host + emission-invisible verification

**Files:** none.

- [ ] **Step 1:** `make boot-test` (alone) → all suites pass incl. `builder_region`.
- [ ] **Step 2:** `cargo test --release cow` (alone) → the COW/census guardrail (`tests/cow_analysis.rs`) baseline is unchanged (Plan 1 adds no emission).
- [ ] **Step 3:** `make stage2` (heavy, alone) → self-host fixed point holds (stage3 == stage4); since Plan 1 adds only a fact + inspection (no rewrite), generated output is unchanged.
- [ ] **Step 4:** `git status --short`; if `make stage2` regenerated a tracked artifact (e.g. `target/boot.wasm`), commit it; else skip.

---

## Self-Review checklist

- **Blockers fixed:** (1) all fixtures are function-local `:=`; (2) no tuples — `SeedFamily`/`FoundRegion`/`ScanResult` records; (3) vector seed recognized via `AInit(ALocal empty_arr_local)`, string via `AInit(ALitStr "")`.
- **Gap 4 (conditional/continue deferral):** verified against the conditional AND continue-guard ANF dumps — the fold nests (conditional) or a top-level guard `AIf` precedes it (continue-guard). Fix: `scan_main_arm` rejects ANY control-flow op (`is_control_flow_op`) on the main arm, and `finish` requires exactly one top-level fold. Negative tests: conditional (A3), continue-guarded + nested-loop (A4).
- **Gap 5 (inspection):** renders certified AND rejected candidates with full `BuilderRegionKey`, boundaries, helper sequence, proof id, and reason (C1).
- **Gap 6 (coverage) — actual tests:** interior-read, conditional-fold, early `return`, self-concat, vector+string positives (A3); continue-guarded, Cell publication, chunk-alias, nested-loop, closure capture, call publication, non-empty-seed, two-region (A4); fold-site reusable (B2); certified + rejected-candidate-still-rendered + **condition-2 both directions via stubbed table** (B3); scoped/full equivalence + stale-artifact (B4). **Value-carrying `break` is checker-rejected in source** (verified), so it's covered by the defensive scan code, not a source fixture. `try` early-return and task/channel publication are subsumed by the call-publication test (any call taking `acc` as an arg rejects via `op_references_deep`); not separately fixtured.
- **Nit 7:** `fold_sites` sorted in `region_key`; `freeze_site = loop_site` documented as a Plan-1 proxy (no rewrite/freeze site exists yet).
- **Nit 8:** C1 reuses the `owned` artifacts (`ir.tw:72`, verified); no second `compute_artifacts`.
- **Type consistency:** `RegionCandidate`/`BuilderRegionKey`/`RegionVerdict`/`ScanResult`/`FoundRegion` field names consistent across A1/A2/A3/B3/C1; `fold_reusable` identical in `cfg.tw`/`ownership.tw`/`builder_region_fact.tw`. `region_verdicts_from_table` (pure) is called by `region_verdicts` and by the B3 condition-2 test.
- **Deferred to Plan 2:** `BuilderRegionDecision`, the ANF rewrite, `repr_assign` string-seed erasure, ANF′ ordering, boxed-`Vector<Int>` emission fixture. This plan surfaces the fact only.

## Risks for the executor

- **`block_verdicts` edit (B2) is the one soundness-sensitive change** — additive only (a new `fold_reusable` write; never touch existing `verdicts`/`reusable_shell`). Self-host (C2) is the guard.
- **Skeleton coupling:** detection assumes the verified break-dispatch loop shape (`if <exit> { break } else { … continue }`). Non-matching loops fall back (no candidate) — sound. If a fixture unexpectedly yields 0 candidates, dump `--opt` and reconcile against the verified shapes above.
- **`condition-2` in practice:** empty-seed + clean-structure folds are almost always unique, so a *natural* source fixture that is structurally clean yet non-unique is hard to construct (pre-loop aliasing rejects structurally). Condition 2 is therefore tested directly through the pure `region_verdicts_from_table` with a stubbed fold-reusable table (B3), exercising both the reject and certify paths deterministically. The B2 reusable-count test guards the real ownership wiring.
- **`pipeline.compile_source` + `owned`** are verified (`dry_run_suite` uses `compile_source`; `ir.tw:72` binds `owned`). No further verification needed for those.
