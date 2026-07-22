# 8C Plan 2 — Builder-Region Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn certified empty-seed builder-region candidates (string `acc = ""` / vector `acc = []` accumulator loops) into an actual ANF-to-ANF rewrite that emits `builder_from`/`builder_new` → `builder_extend`/`builder_push` → `builder_freeze`, replacing the persistent per-iteration `String.concat` / `Vector.append` allocation.

**Architecture:** A codegen-track producer joins each detected `RegionCandidate` (from the landed Plan 1 detector) with its `linearly_folded` verdict and emits a `BuilderRegionDecision` naming every region boundary, the helper sequence, and freshly-allocated locals. A structural-only ANF-to-ANF rewrite pass consumes those records — re-checking the recorded shape (including FU-1's fold-result deadness guard) and applying the mechanical transform — running at the top of `link_program` to produce ANF′ before closure conversion and the call-swap producer. Detection stays the sole legality authority; codegen re-proves nothing. FU-2 surfaces a re-folded (non-empty-seed) accumulator as a rejected candidate so it is never silently dropped.

**Tech Stack:** Twinkle boot compiler (`boot/`), self-hosted. Build via `make bundle-cli`; boot tests via `target/twk run boot/tests/main.tw`. No Rust stage0 changes (boot-codegen opt — see the no-stage0-parity rule for backend-only optimizations).

**Design source:** `docs/plans/sound-uniqueness/codegen/builder-region-design.md` (Rev 4). Follow-up disposition: `docs/plans/2026-07-22-8c-plan1-review-followups.md`.

---

## Orientation (read before starting)

The landed Plan 1 code and the reference shapes this plan builds on:

- **Detector** `boot/compiler/builder_region_detect.tw` — `detect_candidates(m: AnfModule, b: BuiltinRegistry) Vector<RegionCandidate>`; each `RegionCandidate` has `func_id, func, family, push_id, accumulator, seed_site, loop_site, fold_sites, structural_ok, reason`. `region_key(c) BuilderRegionKey` already exists (`func_id, seed_site, loop_site, sorted fold_sites, freeze_site=loop_site, family`).
- **Fact** `boot/compiler/builder_region_fact.tw` — `region_verdicts(opt, b) Vector<RegionVerdict>`; `linearly_folded == structural_ok`.
- **Builder metadata** `boot/compiler/builder_family.tw` — `string_builder_config(b)` / `vector_builder_config(b)` give `BuilderConfig { push_id, builder_new_id, builder_from_id, builder_push_id, builder_freeze_id }`. String: `push_id = String.concat`, `builder_from_id = builder_new_id = string$builder_from`, `builder_push_id = string$builder_extend`, `builder_freeze_id = string$builder_freeze`. Vector: `push_id = Vector.append`, `builder_new_id = vector$builder_new`, `builder_from_id = vector$builder_from`, `builder_push_id = vector$builder_push`, `builder_freeze_id = vector$builder_freeze`.
- **Producer pattern to mirror** `boot/compiler/codegen/mutable_produce.tw` (`produce_update_call_decisions`).
- **Pipeline seam** `boot/compiler/codegen/codegen.tw` — `link_program(anf, env, builtins)`. `anf` arrives already optimized (the caller runs `optimize_module`). Steps: (1) `convert_closures(anf, env)`, (2a) `produce_update_call_decisions(anf, builtins)`, (2b) `prepare_backend_with_mutable_config(...)`, (3) verify, (4) plan types, (5) emit.
- **Backend repr seam** `boot/compiler/backend/repr_assign.tw` — `scan_op` marks `vector$builder_new`/`vector$builder_from` result slots as `wrap_slots` → `OpaqueAnyref`. `string$builder_from` is **not** yet recognized.
- **Census render** `boot/commands/ir.tw` — `render_census_report(artifacts, include_sites)`, `render_region_rows`. `region_verdicts` renders under `--census --sites`.
- **ANF types** `boot/compiler/anf.tw` — `AnfExpr = Let(LocalId, AnfOp, AnfExpr) | Atom | Return | Break | Continue`; relevant `AnfOp`: `ACall(Atom, Vector<Atom>)`, `ALoop(AnfExpr)`, `AIf`, `AMatch`, `ADefer`, `AInit(Atom)`, `AAssign(LocalId, Atom)`. `Atom` includes `AGlobalFunc(FuncId)`, `ALocal(LocalId)`, `ALitVoid`, `ALitStr`. `AnfFunctionDef` carries `op_result_mono: Dict<Int, MonoType>` and `body: AnfExpr`.
- **Reference rewrite shape** — the deleted `opt/loop_builder.tw` and `opt/builder_region.tw` (recover with `git show 5d5ac090^:boot/compiler/opt/loop_builder.tw` and `git show 5d5ac090^:boot/compiler/opt/builder_region.tw`). This plan adapts their exact transform.
- **Test suite** `boot/tests/suites/builder_region_suite.tw` (registered at `boot/tests/main.tw:23,302`). Helpers: `candidates_for(src)`, `verdicts_for(src)`, `certified_count(src)`, `clean_count(src)` via `pipeline.compile_source(src)` → `a.opt, a.builtins`.

### Build / verify loop for each task

- Fast structural check (no CLI rebuild): `target/twk run boot/tests/main.tw` runs the boot suite against the **current** `target/twk`. Detector/fact/producer/rewrite unit tests run here **only after** the code they test is bundled. During development, prefer a scratch entry compiled with a rebuilt CLI, or bundle after each code change.
- Bundle after a boot source change: `make quick-bundle-cli` (fast, reuses `target/boot.wasm`) for iterating; `make bundle-cli` (full self-host) before any self-host / byte-identical claim.
- Self-host fixed point: `make stage2` then compare — run heavy verification **sequentially, never backgrounded** (concurrent `twk` pegs CPU).
- Census dry-run inspection: `target/twk ir <entry> --census --sites`.
- WAT / call inspection: `target/twk wat <entry> --func <name> --calls`.

---

## File Structure

- **Create** `boot/compiler/codegen/builder_region.tw` — the ANF-to-ANF rewrite: region-lowering primitives (`BuilderInit`, `BuilderRegion`, `lower_builder_region`), loop-body rewrite (`rewrite_loop_expr`), and the module-level driver (`rewrite_module`) that consumes decisions. Structural-only; no ownership.
- **Create** `boot/compiler/codegen/builder_region_produce.tw` — the producer: `BuilderRegionDecision` records, `BuilderRegionDecisionTable`, centralized fresh-local allocation, non-overlap resolution, FU-1 deadness gate. Mirrors `mutable_produce.tw`.
- **Modify** `boot/compiler/builder_region_detect.tw` — FU-2: surface a re-folded (already-bound, non-empty-seed) accumulator as a rejected `RegionCandidate`.
- **Modify** `boot/compiler/backend/repr_assign.tw` — recognize `string$builder_from` result slots as `OpaqueAnyref`.
- **Modify** `boot/compiler/codegen/codegen.tw` — wire the rewrite at the top of `link_program` (ANF′).
- **Modify** `boot/commands/ir.tw` — extend the `--census --sites` region render to show which certified regions the rewrite consumed.
- **Modify** `boot/tests/suites/builder_region_suite.tw` — all Plan 2 test fixtures.

---

## Task 1: Backend prerequisite — recognize `string$builder_from` as an erased handle

**Why first:** independent, self-contained, and a hard prerequisite — without it a `string$builder_from` result bound into a `String`-typed slot is verifier-invalid, so the string rewrite (the primary win) can't emit. Landing it first de-risks the emit tasks.

**Files:**
- Modify: `boot/compiler/backend/repr_assign.tw`
- Test: `boot/tests/suites/builder_region_suite.tw`

- [ ] **Step 1: Read the current recognition**

Recover context: `sed -n '40,170p' boot/compiler/backend/repr_assign.tw`. `assign_repr_for_module` resolves `builder_new := builtins.id("vector$builder_new")` and `builder_from := builtins.id("vector$builder_from")`, threads both through `assign_repr_for_func` → `scan_body` → `scan_op`, and in `scan_op`'s `.ACall(.AGlobalFunc(fid), _)` arm sets `wrap_slots[result_sid] = true` when `fid.id == builder_new.id or fid.id == builder_from.id`.

- [ ] **Step 2: Add a failing test — a string builder region compiles and verifies**

Add to `builder_region_suite.tw`. This test compiles a string accumulator loop **through the full backend** and asserts no verifier error. It will pass trivially today (rewrite not wired yet), so its real value is as the guard once the rewrite lands — but write the compile helper now:

```
fn links_ok(src: String) Bool {
  case pipeline.compile_source(src) {
    .Ok(a) => {
      link_program(a.opt, a.env, a.builtins)
      true
    },
    .Err(e) => error("compile failed: ${e}"),
  }
}
```

Add `use compiler.codegen.codegen.{link_program}` to the suite imports — `link_program` lives in the module `compiler.codegen.codegen` (existing importers use `use compiler.codegen.codegen.{codegen, runtime_modules}`); `use compiler.codegen as codegen` names a directory, not a module, and will not resolve `link_program`. Then call `link_program(a.opt, a.env, a.builtins)` directly in `links_ok`. Test:

```
.test(
  "string accumulator loop links through the backend",
  fn() Result<Void, String> { assert.is_true(
    links_ok(
      "fn m() Void {\n  acc := \"\"\n  for c in [\"a\", \"b\"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n",
    ),
  ) },
)
```

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: PASS (baseline; the rewrite is not wired, so this only exercises the persistent path today).

- [ ] **Step 3: Thread `string$builder_from` into `repr_assign`**

In `assign_repr_for_module`, resolve it alongside the vector ids:

```
str_builder_from := builtins.id("string$builder_from")
```

Add a `str_builder_from: FuncId` parameter to `assign_repr_for_func`, `scan_body`, and `scan_op` (thread it exactly like `builder_from`). In `scan_op`'s `.ACall` arm, widen the condition:

```
.AGlobalFunc(fid) => if fid.id == builder_new.id or fid.id == builder_from.id
  or fid.id == str_builder_from.id {
  wrap_slots[result_sid] = true
},
```

- [ ] **Step 4: Rebuild and run the suite**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: PASS, no regressions (this change only widens which call results erase to `OpaqueAnyref`; nothing today produces a `string$builder_from` call, so it is inert until the rewrite lands — a pure prerequisite).

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/backend/repr_assign.tw boot/tests/suites/builder_region_suite.tw
git commit -m "backend/repr_assign: erase string\$builder_from result slots to OpaqueAnyref

Prerequisite for the 8C Plan 2 string builder-region rewrite: string\$builder_from
returns rt_types__StrBuilder, which must live in an anyref/OpaqueAnyref slot, not a
String-typed one. repr_assign recognized only the vector seed ids; widen it to the
string seed. Inert until the rewrite emits the call."
```

---

## Task 2: FU-2 — surface a re-folded (non-empty-seed) accumulator as a rejected candidate

**Why:** In Plan 2's empty-seed-only scope, a second loop that folds an already-materialized accumulator (`acc := ""; for {…}; use(acc); for { acc = acc.concat(…) }`) is a **non-empty seed** — deferred to Plan 3 — so it can only be a **rejected** candidate here. Today `seed_family` fires only on a fresh `AInit` empty-seed, so that second loop is silently dropped (not even rendered). FU-2 makes it visible. Emission-invisible; extends Plan 1 inspection; land before the rewrite.

**Files:**
- Modify: `boot/compiler/builder_region_detect.tw`
- Test: `boot/tests/suites/builder_region_suite.tw`

- [ ] **Step 1: Add the failing test — two-loop-same-`acc` surfaces two candidates**

```
.test(
  "re-folded accumulator surfaces as a rejected candidate (FU-2)",
  fn() Result<Void, String> {
    vs := verdicts_for(
      "fn m() Void {\n  acc := \"\"\n  for c in [\"a\"] { acc = acc.concat(c) }\n  println(acc)\n  for c in [\"b\"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n",
    )
    try assert.equal(vs.len(), 2)
    // one certified (first loop), one rejected re-fold (second loop)
    certified := 0
    reused := 0
    for v in vs {
      if v.linearly_folded { certified = certified + 1 }
      if v.reason.contains("re-used accumulator") { reused = reused + 1 }
    }
    try assert.equal(certified, 1)
    try assert.equal(reused, 1)
    // distinct region keys (different loop sites)
    try assert.is_false(vs[0].key.loop_site == vs[1].key.loop_site)
    .Ok({})
  },
)
```

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: FAIL — `vs.len()` is 1 (second loop silently dropped).

- [ ] **Step 2: Surface re-folds by claimed-loop-site, in a dedicated post-pass**

**Why not seed-membership.** In ANF, loop rebinding mutates the *same* accumulator local: `acc := ""` binds `L0`, and **both** loops fold `L0` (verified — loop 1 `ACall(Fn11, [L0, …]); assign L0 = …`, loop 2 `ACall(Fn11, [L0, …]); assign L0 = …`). So a "flag loops folding a local that was *not* freshly seeded" predicate flags *neither* loop (both fold the seed `L0`). The distinguishing fact is not the accumulator but **which loop the primary region already claimed**: `find_region` claims the **first** fold-bearing loop for a seed; any *later* fold-bearing loop over the same local is the re-fold. Key off the loop-site, not the seed.

Do this as a **dedicated post-pass** (avoids threading new params through every `detect_in_expr`/`detect_in_op` arm). Restructure `detect_candidates`:

```
pub fn detect_candidates(m: AnfModule, b: BuiltinRegistry) Vector<RegionCandidate> {
  str_push := string_builder_config(b).push_id
  vec_push := vector_builder_config(b).push_id
  out: Vector<RegionCandidate> = []
  for f in m.functions {
    primaries := detect_in_expr(f.body, f.func_id, f.name, str_push, vec_push, Dict.new(), [])
    // every candidate the seed path produced (certified OR structurally-rejected)
    // has claimed its loop_site; those loops must not re-surface as re-folds.
    claimed: Dict<Int, Bool> = Dict.new()
    for c in primaries {
      claimed[c.loop_site.id] = true
    }
    refolds := scan_refold_loops(f.body, f.func_id, f.name, str_push, vec_push, claimed, [])
    out = out.concat(primaries).concat(refolds)
  }
  out
}
```

`scan_refold_loops` walks the function body; for each `Let(loop_local, ALoop(loop_body), rest)` whose `loop_local.id` is **not** in `claimed` and whose body contains a fold attempt over some local `v`, append a rejected candidate keyed by `loop_local`, then recurse into `loop_body` and `rest` (a re-fold loop can nest further candidates). Recurse through `AIf`/`AMatch`/`ADefer` bodies too.

```
fn scan_refold_loops(
  e: AnfExpr, func_id: FuncId, func: String,
  str_push: FuncId, vec_push: FuncId,
  claimed: Dict<Int, Bool>, acc_cands: Vector<RegionCandidate>,
) Vector<RegionCandidate> {
  case e {
    .Let(loop_local, .ALoop(loop_body), rest) => {
      out := case claimed[loop_local.id] {
        .Some(_) => acc_cands,   // this loop is a claimed primary region
        .None => case first_refold(loop_body, str_push, vec_push) {
          .Some(hit) => acc_cands.append(RegionCandidate.{
            func_id, func,
            family: hit.family,
            push_id: hit.push_id,
            accumulator: hit.acc,
            seed_site: hit.acc,          // no fresh seed; proxy to the accumulator local
            loop_site: loop_local,
            fold_sites: [],
            structural_ok: false,
            reason: "re-used accumulator, not a fresh seed (non-empty; deferred to Plan 3)",
          }),
          .None => acc_cands,
        },
      }
      out2 := scan_refold_loops(loop_body, func_id, func, str_push, vec_push, claimed, out)
      scan_refold_loops(rest, func_id, func, str_push, vec_push, claimed, out2)
    },
    .Let(_, op, rest) => {
      out := scan_refold_loops_op(op, func_id, func, str_push, vec_push, claimed, acc_cands)
      scan_refold_loops(rest, func_id, func, str_push, vec_push, claimed, out)
    },
    _ => acc_cands,
  }
}
```

`scan_refold_loops_op` recurses into `AIf`/`AMatch`/`ADefer` sub-exprs (mirrors `detect_in_op`). `first_refold(loop_body, str_push, vec_push)` finds the first fold `ACall(.AGlobalFunc(push), [.ALocal(v), _])` where `push` matches `str_push` or `vec_push` and returns `(acc: v, family, push_id)`. **It must recurse into the break-dispatch structure the same way `references_fold`/`op_fold_ref` do** — the fold lives inside the loop's `AIf` main arm (verified in the ANF: `if L31 then break else …fold…`), **not** at `loop_body`'s top level. Build `first_refold` by mirroring `op_fold_ref`'s `AIf`/`AMatch`/`ALoop`/`ADefer` recursion, returning the matched `(v, family, push_id)` instead of a `Bool`. It does **not** need the reassign tail (a re-fold is rejected regardless of tail shape; we only need to surface it).

**Note — claimed set includes rejected primaries.** A structurally-rejected primary (e.g. a conditional-fold loop) still claimed its `loop_site` via `detect_in_expr`, so it is in `claimed` and won't double-surface as a re-fold. Only loops with no primary candidate at all become re-folds.

- [ ] **Step 3: Rebuild and run the FU-2 test**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: PASS — two verdicts, one certified, one `re-used accumulator` rejected, distinct loop sites.

- [ ] **Step 4: Add a guard test — a single-loop clean fold is unaffected**

```
.test(
  "single clean fold still surfaces exactly one certified candidate (FU-2 no false positive)",
  fn() Result<Void, String> { assert.equal(
    certified_count(
      "fn m() Void {\n  acc := \"\"\n  for c in [\"a\", \"b\"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n",
    ),
    1,
  ) },
)
```

Run: `target/twk run boot/tests/main.tw`
Expected: PASS (already-bundled CLI).

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/builder_region_detect.tw boot/tests/suites/builder_region_suite.tw
git commit -m "builder_region_detect: surface re-folded accumulators as rejected candidates (FU-2)

A second loop folding an already-materialized accumulator (a non-empty seed,
deferred to Plan 3) was silently dropped by the detector because seed_family
fires only on a fresh empty seed. Surface it as a rejected candidate keyed by
its own loop site so the census renders it instead of losing it. Sound and
conservative: the rewrite still only ever touches the first, empty-seed loop."
```

---

## Task 3: The ANF-to-ANF rewrite pass (unwired, ANF-level tested)

**Why:** Build and test the mechanical transform in isolation before wiring it into the pipeline. Adapts the recovered `opt/loop_builder.tw` + `opt/builder_region.tw` verbatim in shape, driven by explicit region parameters instead of the deleted analysis.

**Files:**
- Create: `boot/compiler/codegen/builder_region.tw`
- Test: `boot/tests/suites/builder_region_suite.tw`

- [ ] **Step 1: Recover the reference shapes**

```bash
git show 5d5ac090^:boot/compiler/opt/builder_region.tw
git show 5d5ac090^:boot/compiler/opt/loop_builder.tw
```

These give `BuilderInit`, `BuilderRegion`, `lower_builder_region`, `rewrite_loop_expr`, `rewrite_loop_op_subexpr`, and `rewrite_loop_region`. This plan reuses them near-verbatim.

- [ ] **Step 2: Write a failing ANF-level test for `rewrite_loop_region`**

The rewrite is easiest to test by asserting the rewritten ANF **contains the builder helper calls and no residual `push_id` fold call**. Add a helper to the suite that compiles, grabs the accumulator loop function's body, runs `rewrite_loop_region`, and counts call targets. Because `rewrite_loop_region` needs the region boundaries, drive it from a detected candidate:

```
fn count_calls_to(e: AnfExpr, target: FuncId) Int { … walk, count ACall(AGlobalFunc(target), _) … }
```

(Write the walker inline in the suite; it mirrors `count_global_call_sites` from the deleted `opt/analysis.tw` — recover it with `git show 5d5ac090^:boot/compiler/opt/analysis.tw` if useful, but a fresh 15-line recursive counter over `AnfExpr`/`AnfOp` is fine and self-contained.)

Test intent: for the clean string loop, after rewriting the region, the loop body contains **one** `string$builder_extend` call and **zero** `String.concat` calls; and the region is wrapped by a `string$builder_from` seed and a `string$builder_freeze`.

Since `rewrite_loop_region` operates on the loop body + continuation, the cleanest ANF-level test drives the **module-level driver** `rewrite_module` (built in Step 3) instead. Write this test against `rewrite_module(anf, builtins)`:

```
.test(
  "rewrite emits string builder calls and drops the concat fold",
  fn() Result<Void, String> {
    a := compile_or_fail("fn m() Void {\n  acc := \"\"\n  for c in [\"a\", \"b\"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n")
    anf2 := builder_region.rewrite_module(a.opt, a.builtins)
    concat_id := a.builtins.method_id("String", "concat")
    extend_id := a.builtins.id("string$builder_extend")
    from_id := a.builtins.id("string$builder_from")
    freeze_id := a.builtins.id("string$builder_freeze")
    fn_m := find_func(anf2, "m")  // helper: linear search m.functions by name
    try assert.equal(count_calls_to(fn_m.body, concat_id), 0)
    try assert.equal(count_calls_to(fn_m.body, extend_id), 1)
    try assert.equal(count_calls_to(fn_m.body, from_id), 1)
    try assert.equal(count_calls_to(fn_m.body, freeze_id), 1)
    .Ok({})
  },
)
```

Add `use compiler.codegen.builder_region` and `use compiler.core_ir.{FuncId}` and small helpers `compile_or_fail(src)`, `find_func(anf, name)`.

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: FAIL — `builder_region.rewrite_module` does not exist.

- [ ] **Step 3: Implement `builder_region.tw`**

Port the region primitives verbatim (from the recovered `opt/builder_region.tw`), then add `rewrite_loop_expr`/`rewrite_loop_op_subexpr` (from `opt/loop_builder.tw`), then a **decision-driven** driver. In this task the driver derives regions directly from certified detector candidates (the producer/records come in Task 4); refactor the driver to consume records there.

Module skeleton:

```
use compiler.anf.{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfModule, AnfOp, Atom}
use compiler.builder_family.{BuilderConfig, string_builder_config, vector_builder_config}
use compiler.builder_region_detect as detect
use compiler.builtins.{BuiltinRegistry}
use compiler.core_ir.{FuncId, LocalId}
use compiler.mono_type.{MonoType}

pub type BuilderInit = { New, FromBase(LocalId) }

pub type BuilderRegion = .{
  bind_local: LocalId, base_local: LocalId, builder_local: LocalId,
  freeze_local: LocalId, assign_local: LocalId, init: BuilderInit,
  loop_body: AnfExpr, cont: AnfExpr,
}
// ... builder_init_call + lower_builder_region: verbatim from opt/builder_region.tw ...
// ... rewrite_loop_expr + rewrite_loop_op_subexpr: verbatim from opt/loop_builder.tw,
//     but recognize the fold by (push_id, base) using the detector's fold shape
//     (ACall(AGlobalFunc(push_id), [ALocal(base), chunk]) followed by AAssign(base, result)),
//     replacing loop_push_reassign_elem (which lived in the deleted analysis) with an
//     inline matcher — the same one the detector uses in fold_chunk. ...
```

Key detail (matcher): the deleted `loop_push_reassign_elem(op, body, base, push_id, local)` returned the chunk atom when `op = ACall(AGlobalFunc(push_id), [ALocal(base), chunk])`, `chunk != base`, and `body = Let(_, AAssign(base, ALocal(local)), _)`. This is exactly `detect.fold_chunk`'s shape — but `fold_chunk` is not `pub`. Either export a minimal matcher from the detector or inline the identical match here. **Inline it** to keep the rewrite self-contained and the detector's internals private.

Driver:

```
// Empty-seed only: string uses builder_from(""), vector uses builder_new().
fn config_for(family: String, b: BuiltinRegistry) BuilderConfig {
  if family == "string" { string_builder_config(b) } else { vector_builder_config(b) }
}

// use_builder_new: vector empty seed → New; string empty seed → FromBase over the "" seed,
// BUT the empty-seed sub-scope uses builder_from("") for string and builder_new() for vector.
// So: string → FromBase(seed_local of the "") ... see note below.
```

**Seed-call note (empty-seed sub-scope):** the design fixes the seed helpers as string `string$builder_from("")` and vector `vector$builder_new()`. The recovered `builder_init_call` models these as `.FromBase(base)` → `builder_from_id(base)` and `.New` → `builder_new_id()`. For **string**, `base` is the `acc` accumulator local seeded to `""`, so `string$builder_from(acc)` where `acc == ""` — identical to `builder_from("")`. For **vector**, use `.New` (never `vector$builder_from` in this slice). So: `init = if family == "string" { .FromBase(accumulator) } else { .New }`.

Module driver:

```
pub fn rewrite_module(m: AnfModule, b: BuiltinRegistry) AnfModule {
  cands := detect.detect_candidates(m, b)
  // group certified candidates by func_id; Task 4 replaces this with records
  new_funcs: Vector<AnfFunctionDef> = collect f in m.functions {
    rewrite_func(f, certified_for(cands, f.func_id), b)
  }
  AnfModule.{ functions: new_funcs, init_func_id: m.init_func_id,
    extern_imports: m.extern_imports, global_monos: m.global_monos, lib_exports: m.lib_exports }
}
```

`rewrite_func` locates each certified region's seed `Let` in `f.body`, splices in the builder seed before the loop, rewrites the loop body via `rewrite_loop_expr`, appends the freeze + `AAssign(acc, freeze)` after the loop, and updates `f.op_result_mono` with `freeze_local → base_mono` (from `f.op_result_mono[accumulator.id]`) and `assign_local → .Void`. Fresh locals: compute `max_local(f) + 1` and allocate 3 per region (see Task 4 for centralized multi-region allocation; single-region is fine here).

**Splice mechanics:** walk `f.body` to the `Let(seed_local, seed_op, after)` that binds the accumulator, then to the `Let(loop_bind, ALoop(loop_body), cont)` in `after`. Replace that stretch with `lower_builder_region(BuilderRegion.{…})` where `loop_body` is `rewrite_loop_expr(loop_body, accumulator, builder_local, push_id, builder_push_id)` and `cont` is the original post-loop continuation. The original `Let(seed_local, seed_op, …)` empty-seed binding is **left in place** (harmless: the accumulator is re-bound by the trailing `AAssign`; dead-seed elimination is not required for correctness and `acc = ""` is cheap). If preferred, drop it — but leaving it is simpler and sound.

- [ ] **Step 4: Coverage guard in the driver**

After `rewrite_loop_expr`, recount `string$builder_extend`/`vector$builder_push` sites in the rewritten body; if it does not equal the recorded `fold_sites.len()` (== 1 in the first slice), **abandon** the region (return the function unchanged) — persistent fallback. This is the structural equality check from the design, not a re-proof.

- [ ] **Step 5: Run the ANF-level test**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: PASS — 0 concat, 1 extend, 1 from, 1 freeze in `m`.

- [ ] **Step 6: Add the vector variant test**

```
.test(
  "rewrite emits vector builder_new/push/freeze and drops append",
  fn() Result<Void, String> {
    a := compile_or_fail("fn m() Void {\n  acc: Vector<Int> = []\n  for x in [1, 2] { acc = acc.append(x) }\n  println(acc.len().to_string())\n}\nm()\n")
    anf2 := builder_region.rewrite_module(a.opt, a.builtins)
    fn_m := find_func(anf2, "m")
    try assert.equal(count_calls_to(fn_m.body, a.builtins.method_id("Vector", "append")), 0)
    try assert.equal(count_calls_to(fn_m.body, a.builtins.id("vector\$builder_new")), 1)
    try assert.equal(count_calls_to(fn_m.body, a.builtins.id("vector\$builder_push")), 1)
    try assert.equal(count_calls_to(fn_m.body, a.builtins.id("vector\$builder_freeze")), 1)
    .Ok({})
  },
)
```

Run: `target/twk run boot/tests/main.tw`
Expected: PASS.

- [ ] **Step 7: Add a negative test — a rejected candidate is not rewritten**

Use an interior-read loop (already rejected by the detector):

```
.test(
  "interior-read loop is left persistent by the rewrite",
  fn() Result<Void, String> {
    a := compile_or_fail("fn m() Void {\n  acc := \"\"\n  for c in [\"a\"] { acc = acc.concat(c)\n  println(acc) }\n  println(acc)\n}\nm()\n")
    anf2 := builder_region.rewrite_module(a.opt, a.builtins)
    fn_m := find_func(anf2, "m")
    // still persistent: concat remains, no builder_extend
    try assert.is_true(count_calls_to(fn_m.body, a.builtins.method_id("String", "concat")) >= 1)
    try assert.equal(count_calls_to(fn_m.body, a.builtins.id("string\$builder_extend")), 0)
    .Ok({})
  },
)
```

Run: `target/twk run boot/tests/main.tw`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add boot/compiler/codegen/builder_region.tw boot/tests/suites/builder_region_suite.tw
git commit -m "codegen/builder_region: ANF-to-ANF builder-region rewrite (unwired)

Port the deleted loop_builder/builder_region transform, now driven by certified
detector candidates instead of the removed uniqueness/liveness analysis. String
empty-seed regions become builder_from(acc=\"\")->extend*->freeze; vector empty-seed
become builder_new->push*->freeze (boxed). Coverage guard abandons a region whose
rewritten push count != recorded fold count. Tested at the ANF level; not yet wired
into link_program."
```

---

## Task 4: Producer — `BuilderRegionDecision` records, keys, fresh-local allocation, non-overlap, FU-1 gate

**Why:** Replace Task 3's inline candidate grouping with real decision records so the rewrite consumes a validated record (the design's Fork 1/Component 2), and add the two soundness/robustness properties: FU-1 (fold-result deadness) and non-overlap.

**Files:**
- Create: `boot/compiler/codegen/builder_region_produce.tw`
- Modify: `boot/compiler/codegen/builder_region.tw` (driver consumes records)
- Test: `boot/tests/suites/builder_region_suite.tw`

- [ ] **Step 1: Define the decision record + table**

In `builder_region_produce.tw`:

```
use compiler.anf.{AnfExpr, AnfFunctionDef, AnfModule, AnfOp, Atom}
use compiler.builder_region_detect as detect
use compiler.builder_region_detect.{BuilderRegionKey, RegionCandidate}
use compiler.builtins.{BuiltinRegistry}
use compiler.core_ir.{FuncId, LocalId}

pub type BuilderRegionDecision = .{
  key: BuilderRegionKey,
  func_id: FuncId,
  family: String,
  accumulator: LocalId,
  seed_site: LocalId,
  loop_site: LocalId,
  fold_sites: Vector<LocalId>,   // the fold-call result locals
  builder_local: LocalId,        // fresh
  freeze_local: LocalId,         // fresh
  assign_local: LocalId,         // fresh
}

pub type BuilderRegionDecisionTable = .{ by_func: Dict<Int, Vector<BuilderRegionDecision>> }
```

- [ ] **Step 2: Failing test — producer yields one decision for the clean string loop, none for a rejected one**

```
fn decisions_for(src: String) builder_region_produce.BuilderRegionDecisionTable {
  a := compile_or_fail(src)
  builder_region_produce.produce_builder_region_decisions(a.opt, a.builtins)
}
fn total_decisions(t: builder_region_produce.BuilderRegionDecisionTable) Int {
  n := 0
  for _, v in t.by_func { n = n + v.len() }
  n
}

.test("producer emits one decision for a clean string loop", fn() Result<Void, String> {
  assert.equal(total_decisions(decisions_for(
    "fn m() Void {\n  acc := \"\"\n  for c in [\"a\"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n")), 1)
}),
.test("producer emits no decision for an interior-read loop", fn() Result<Void, String> {
  assert.equal(total_decisions(decisions_for(
    "fn m() Void {\n  acc := \"\"\n  for c in [\"a\"] { acc = acc.concat(c)\n  println(acc) }\n  println(acc)\n}\nm()\n")), 0)
}),
```

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: FAIL — `produce_builder_region_decisions` undefined.

- [ ] **Step 3: Implement the producer**

```
pub fn produce_builder_region_decisions(m: AnfModule, b: BuiltinRegistry) BuilderRegionDecisionTable {
  cands := detect.detect_candidates(m, b)
  by_func: Dict<Int, Vector<BuilderRegionDecision>> = Dict.new()
  for f in m.functions {
    certified := filter_certified(cands, f.func_id)          // structural_ok only
    non_overlapping := resolve_overlaps(certified)           // Step 4
    live_ok := collect fc in non_overlapping {               // FU-1, Step 5
      fc
    }.filter(fn(c) { fold_results_dead(f, c) })
    decisions := allocate_locals(f, live_ok, b)              // centralized fresh locals
    if decisions.len() > 0 { by_func[f.func_id.id] = decisions }
  }
  BuilderRegionDecisionTable.{ by_func }
}
```

`allocate_locals(f, cands, b)`: compute `next := max_local(f) + 1`; for each candidate in a deterministic order (sorted by `region_key`), assign `builder_local = next`, `freeze_local = next+1`, `assign_local = next+2`, advance `next += 3`. Build a `BuilderRegionDecision` per candidate. `max_local(f)` walks `f.body` + `f.params` for the greatest `LocalId.id` (write a small recursive helper; it must cover every `Let` binder and every `AAssign`/`AInit` target — reuse the same enumeration shape as the detector's walkers).

- [ ] **Step 4: Non-overlap resolution**

```
fn resolve_overlaps(cands: Vector<RegionCandidate>) Vector<RegionCandidate> { … }
```

Two certified candidates **overlap** if they share the same `accumulator` local, the same `loop_site`, or any `fold_site`. Group overlapping candidates and keep the one with the **lowest** `BuilderRegionKey` (define a total order on the key: compare `func_id`, then `seed_site`, then `loop_site`, then `fold_sites` lexicographically, then `family`). Deterministic and independent of detection order. Add a test:

```
.test("two independent clean loops in one function both get decisions", fn() Result<Void, String> {
  assert.equal(total_decisions(decisions_for(
    "fn m() Void {\n  a := \"\"\n  for c in [\"x\"] { a = a.concat(c) }\n  println(a)\n  b := \"\"\n  for c in [\"y\"] { b = b.concat(c) }\n  println(b)\n}\nm()\n")), 2)
}),
```

(Independent accumulators `a`/`b` do not overlap → two decisions with non-colliding fresh locals.)

- [ ] **Step 5: FU-1 — fold-result deadness gate**

```
// Sound gate: the fold-result local of a certified region must have NO use other
// than the `acc = result` reassign. Certification already matched a direct
// AAssign(acc, ALocal result), but a future optimizer could copy-propagate a live
// temp into that position. Reject the region if `result` is read anywhere else.
fn fold_results_dead(f: AnfFunctionDef, c: RegionCandidate) Bool {
  for r in c.fold_sites {
    if count_local_uses(f.body, r) != 1 {   // exactly the reassign
      return false
    }
  }
  true
}
```

`count_local_uses(body, r)` counts occurrences of `.ALocal(r)` across the whole function body (every atom position in every op/expr — reuse the detector's `op_references_deep` traversal shape, but counting rather than short-circuiting). The single legitimate use is the `AAssign(acc, .ALocal(r))` reassign; more than one means the result is live → reject. Add the FU-1 fixture:

```
.test("fold result read after the reassign blocks the decision (FU-1)", fn() Result<Void, String> {
  // synthetic extra read of the concat result via a second binding
  // acc = acc.concat(c) desugars to  r = concat(acc,c); acc = r
  // force r live by also using it: acc2 := acc  (reads acc, i.e. the frozen value) is NOT r;
  // instead read the fold value directly through an explicit let:
  src := "fn m() Void {\n  acc := \"\"\n  for c in [\"a\"] {\n    nxt := acc.concat(c)\n    acc = nxt\n    println(nxt)\n  }\n  println(acc)\n}\nm()\n"
  assert.equal(total_decisions(decisions_for(src)), 0)
}),
```

**Note on this fixture:** if the detector already rejects this shape at Plan-1 level (the explicit `nxt` binding may make it a *named/live temp* with an intervening node, which `fold_chunk` already rejects — see FU-1's "verified" clause), the producer will see **zero certified candidates** and `total_decisions == 0` for a different reason. That is still the correct end state, but it does not exercise the FU-1 gate. During implementation, first check whether this source certifies at Plan 1 (`clean_count(src)`): if it does not certify (count 0), construct the FU-1 fixture by directly building a candidate + ANF where the synthetic single-use temp is made live (a hand-built `AnfFunctionDef` in the test, bypassing source lowering), and assert `fold_results_dead` returns `false`. Prefer a source-level fixture if one certifies-then-fails-FU-1; fall back to the hand-built ANF unit test otherwise. **Do not ship the gate without a test that actually drives it to `false`.**

- [ ] **Step 6: Point the rewrite driver at the records**

Refactor `builder_region.tw`'s `rewrite_module` to take the decision table:

```
pub fn rewrite_module(m: AnfModule, b: BuiltinRegistry) AnfModule {
  table := builder_region_produce.produce_builder_region_decisions(m, b)
  new_funcs := collect f in m.functions {
    rewrite_func_with_decisions(f, decisions_for_func(table, f.func_id), b)
  }
  // reassemble AnfModule (as in Task 3)
}
```

`rewrite_func_with_decisions` uses the record's `builder_local`/`freeze_local`/`assign_local` (no independent `next_local` bump) and re-validates structurally before applying: the recorded `seed_site`/`loop_site`/`fold_sites` still match the ANF shape, and the coverage guard (Task 3 Step 4) holds. Any mismatch → leave the function unchanged (persistent fallback). Keep all Task 3 rewrite tests green (they now flow through the records).

- [ ] **Step 7: Run the full suite**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: PASS — producer tests, non-overlap, FU-1, and all Task 3 rewrite tests green.

- [ ] **Step 8: Commit**

```bash
git add boot/compiler/codegen/builder_region_produce.tw boot/compiler/codegen/builder_region.tw boot/tests/suites/builder_region_suite.tw
git commit -m "codegen/builder_region: decision-record producer with non-overlap + FU-1 gate

Add produce_builder_region_decisions: joins certified candidates with centralized
per-function fresh-local allocation, deterministic non-overlap resolution (lowest
BuilderRegionKey wins on shared acc/loop/fold-site), and the FU-1 fold-result
deadness gate (reject if the fold-result temp is read anywhere but the reassign).
The rewrite driver now consumes records and re-validates the recorded shape before
transforming; any mismatch falls back to persistent."
```

---

## Task 5: Pipeline wiring (ANF′) + census dry-run extension

**Why:** Run the rewrite at the top of `link_program` so both closure conversion and the call-swap producer see ANF′, and surface consumed regions in the inspection before/at enable.

**Files:**
- Modify: `boot/compiler/codegen/codegen.tw`
- Modify: `boot/commands/ir.tw`
- Test: `boot/tests/suites/builder_region_suite.tw`

- [ ] **Step 1: Wire the rewrite at the top of `link_program`**

In `codegen.tw`, add `use compiler.codegen.builder_region`. At the very top of `link_program`, before step 1:

```
// 0. Builder-region rewrite → ANF′. Runs before closure conversion and the
// call-swap producer so both consume the rewritten ANF (its stale-artifact
// guard keys off the ANF fingerprint, which must be ANF′).
anf_prime := builder_region.rewrite_module(anf, builtins)
```

Then replace the two `anf` reads with `anf_prime`:
- `closure_conversion := convert_closures(anf_prime, env)`
- `produced_decisions := mutable_produce.produce_update_call_decisions(anf_prime, builtins)`

Leave everything downstream unchanged (they already read `closure_conversion.anf`).

- [ ] **Step 2: Behavioral round-trip test — string builder region produces the same result**

The boot suite is compiled and executed, so a behavioral assertion exercises the enabled rewrite end-to-end:

```
.test("string builder region round-trips to the persistent result", fn() Result<Void, String> {
  acc := ""
  for c in ["a", "b", "c"] { acc = acc.concat(c) }
  assert.equal(acc, "abc")
}),
.test("vector builder region round-trips to the persistent result", fn() Result<Void, String> {
  v: Vector<Int> = []
  for x in [1, 2, 3] { v = v.append(x) }
  try assert.equal(v.len(), 3)
  try assert.equal(v[0] + v[1] + v[2], 6)
  .Ok({})
}),
```

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: PASS — results identical; the rewrite is now live in the compiled suite itself.

- [ ] **Step 3: Full boot suite + self-host smoke**

Run: `target/twk run boot/tests/main.tw` (full suite green), then `make bundle-cli` (full self-host build succeeds — the compiler compiles itself with the rewrite live).
Expected: both succeed. If `make bundle-cli` fails, the rewrite miscompiled boot source — debug with `TWINKLE_VERIFY_LEVEL=basic target/twk build boot/main.tw -o /tmp/dbg.wat` and inspect the offending function via `twk wat --func <name> --calls`.

- [ ] **Step 4: Extend the census dry-run render to show consumed regions**

In `boot/commands/ir.tw`, the `render_census_report` `include_sites` branch already renders `region_verdicts` via `render_region_rows`. Add, after it, a line per certified region showing whether the producer emitted a decision for it (i.e. it survived FU-1 + non-overlap). Compute `produce_builder_region_decisions(artifacts.opt, artifacts.builtins)` and annotate each certified verdict row with `consumed: yes/no` by matching `BuilderRegionKey`. Extend `render_region_rows` (or add a sibling `render_region_decisions`) with a `consumed` column. Keep rejected candidates rendering as before.

- [ ] **Step 5: Verify the inspection on a scratch entry**

```bash
printf 'fn m() Void {\n  acc := ""\n  for c in ["a","b"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n' > /tmp/br.tw
target/twk ir /tmp/br.tw --census --sites
```

Expected: the builder-regions section lists the string region as `linearly_folded=true` and `consumed=yes`. Also confirm emitted calls:

```bash
target/twk wat /tmp/br.tw --func m --calls
```

Expected: shows `string$builder_from`, `string$builder_extend`, `string$builder_freeze`; no `String$concat` in `m`.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/codegen/codegen.tw boot/commands/ir.tw boot/tests/suites/builder_region_suite.tw
git commit -m "codegen: wire builder-region rewrite into link_program (ANF'), render consumed regions

Run rewrite_module at the top of link_program so closure conversion and the
call-swap producer both consume ANF'. String/vector empty-seed accumulator loops
now emit the builder sequence end-to-end; --census --sites shows which certified
regions the producer consumed. Behavioral round-trip + self-host verified."
```

---

## Task 6: Robustness fixtures + stale-region fallback + boxed-`Vector<Int>` documentation + self-host fixed point

**Why:** Lock down the design's remaining test obligations and the completion gate.

**Files:**
- Test: `boot/tests/suites/builder_region_suite.tw`

- [ ] **Step 1: Boxed-`Vector<Int>` fixture (documents deferred typed routing)**

Assert the typed-int vector accumulator loop stays **boxed** in this slice (`vector$builder_*`, not `*_i64`). This documents that typed routing of the `AAssign`-rebound accumulator is Plan 4:

```
.test("typed Vector<Int> accumulator loop stays boxed (routing deferred to Plan 4)", fn() Result<Void, String> {
  a := compile_or_fail("fn m() Void {\n  acc: Vector<Int> = []\n  for x in [1, 2, 3] { acc = acc.append(x) }\n  println(acc.len().to_string())\n}\nm()\n")
  anf2 := builder_region.rewrite_module(a.opt, a.builtins)
  fn_m := find_func(anf2, "m")
  // boxed builder ops present; no typed *_i64 builder ops
  try assert.equal(count_calls_to(fn_m.body, a.builtins.id("vector\$builder_push")), 1)
  // (no builtins entry for a *_i64 push here means the boxed path is what emitted)
  .Ok({})
}),
```

Run: `target/twk run boot/tests/main.tw`
Expected: PASS.

**Do not probe a `*_i64` builder id via `builtins.id(...)`** — `id`/`method_id`/`id_by_canonical` all `error()` (trap) on a missing name, which would abort the test rather than fail it cleanly. The positive assertion (`vector$builder_push` count == 1, a guaranteed-present id) is sufficient to prove the boxed path emitted. If you want an explicit "no typed builder op" assertion, guard the lookup with `try_method_id` / a name-membership check first and skip if absent; never pass a possibly-absent name to `id()`.

- [ ] **Step 2: Nested candidate loops — fresh locals never collide**

```
.test("nested candidate loops both rewrite without local collision", fn() Result<Void, String> {
  // outer string accumulator; inner independent string accumulator
  acc := ""
  for c in ["a", "b"] {
    inner := ""
    for d in ["1", "2"] { inner = inner.concat(d) }
    acc = acc.concat(inner)
  }
  assert.equal(acc, "1212")
}),
```

Run: `target/twk run boot/tests/main.tw`
Expected: PASS. (Behavioral; a collision would corrupt output or fail verification.)

- [ ] **Step 3: Stale-region fallback unit test**

Build (or reuse) a `BuilderRegionDecision` whose recorded `loop_site`/`fold_sites` do not match the ANF, pass it to `rewrite_func_with_decisions`, and assert the function comes back **unchanged** (persistent). Construct this by producing a real decision, then mutating its `loop_site` to a bogus `LocalId`, then feeding it back:

```
.test("stale region decision falls back to persistent", fn() Result<Void, String> {
  a := compile_or_fail("fn m() Void {\n  acc := \"\"\n  for c in [\"a\"] { acc = acc.concat(c) }\n  println(acc)\n}\nm()\n")
  fn_m := find_func(a.opt, "m")
  table := builder_region_produce.produce_builder_region_decisions(a.opt, a.builtins)
  ds := builder_region.decisions_for_func(table, fn_m.func_id)
  try assert.equal(ds.len(), 1)
  stale := BuilderRegionDecision.{ …ds[0] with loop_site: LocalId.{ id: 999999 } }
  out := builder_region.rewrite_func_with_decisions(fn_m, [stale], a.builtins)
  // unchanged: concat still present, no builder_extend
  try assert.equal(count_calls_to(out.body, a.builtins.id("string\$builder_extend")), 0)
  .Ok({})
}),
```

(Adjust record-update syntax to Twinkle's; `.{ …ds[0] with loop_site: … }` is illustrative — use explicit field copy if spread-update is unavailable.)

Run: `target/twk run boot/tests/main.tw`
Expected: PASS.

- [ ] **Step 4: `collect` remains decision-free**

```
.test("collect comprehension is untouched by builder-region decisions", fn() Result<Void, String> {
  a := compile_or_fail("fn m() Void {\n  xs := collect x in range(3) { x * x }\n  println(xs.len().to_string())\n}\nm()\n")
  table := builder_region_produce.produce_builder_region_decisions(a.opt, a.builtins)
  assert.equal(total_decisions(table), 0)
}),
```

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — `collect`'s semantic builder lowering produces no accumulator-fold candidate.

- [ ] **Step 5: Call-swap non-regression**

Confirm 8A/8B/8D/8E still emit correctly on ANF′. Run the existing mutable/vector/dict suites (they are already in `boot/tests/main.tw`):

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — full suite green (call-swap decisions recomputed on ANF′, no staleness regression).

- [ ] **Step 6: Self-host fixed point + byte-identical-where-expected**

Run **sequentially**:

```bash
make bundle-cli
make stage2
```

Compare stage3 vs stage4 output for a fixed point (self-host stable). The rewrite is an emitted-code change, so output is **not** byte-identical to pre-Plan-2 `main` — that is expected (Plan 2 is the first emitted-code change). Confirm the self-host loop converges and the full boot suite passes on the freshly bundled CLI.

Run: `target/twk run boot/tests/main.tw`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add boot/tests/suites/builder_region_suite.tw
git commit -m "builder_region: robustness fixtures — boxed Vector<Int>, nested, stale fallback, collect-free

Boxed-Vector<Int> fixture documents deferred typed routing (Plan 4); nested
candidate loops confirm fresh-local non-collision; stale-region decision falls
back to persistent; collect stays decision-free; call-swap non-regression. Self-host
fixed point verified."
```

---

## Task 7: Roadmap + docs update

**Files:**
- Modify: `docs/plans/sound-uniqueness/codegen/builder-region-design.md`
- Modify: `docs/plans/sound-uniqueness/README.md`
- Modify: `docs/plans/README.md` (if this plan is listed there)

- [ ] **Step 1: Mark Plan 2 complete in the design doc**

Update the design doc status line and the Plan 2 bullet to reflect landed (string+vector empty-seed rewrite, FU-1, FU-2, repr_assign fix). Note that Plans 3–7 remain the sequenced follow-ups.

- [ ] **Step 2: Update the sound-uniqueness README current-focus**

In `docs/plans/sound-uniqueness/README.md`, update the "Current implementation focus" to note 8C builder-region **rewrite** (Plan 2) landed; next is Plan 3 (non-empty seeds) / records (8F) per the roadmap.

- [ ] **Step 3: Follow the plans-README convention**

Per project convention (`feedback_plans_readme_remove_when_done`): when this plan is fully done, delete its row from `docs/plans/README.md` (don't mark Done) and move this doc to `docs/plans/archive/`.

- [ ] **Step 4: Commit**

```bash
git add docs/plans/sound-uniqueness/ docs/plans/README.md
git commit -m "docs: 8C Plan 2 (builder-region rewrite) landed; update roadmap to Plan 3"
```

---

## Self-Review

**Spec coverage** (against `builder-region-design.md` Rev 4):
- Producer + `BuilderRegionDecision` + `BuilderRegionKey` + non-overlap + centralized fresh-local → Task 4. ✓
- ANF-to-ANF rewrite, concrete void-call shape, neutralize reassign, post-loop freeze, coverage guard → Task 3. ✓
- `repr_assign` `string$builder_from` erasure prereq → Task 1. ✓
- Pipeline ordering (ANF′ before closure conversion + call-swap producer) → Task 5. ✓
- Inspection/dry-run extension (consumed regions) → Task 5 Steps 4–5. ✓
- FU-1 fold-result deadness gate → Task 4 Step 5. ✓
- FU-2 re-folded-accumulator surfacing → Task 2. ✓
- Testing obligations: string+vector emit ✓ (T3/T5), boxed-`Vector<Int>` ✓ (T6.1), multi/nested/overlap ✓ (T4.4/T6.2), stale-region ✓ (T6.3), call-swap non-regression ✓ (T6.5), collect-free ✓ (T6.4), round-trip ✓ (T5.2), self-host fixed point ✓ (T6.6).
- **Deferred (correctly out of scope):** non-empty seeds (Plan 3), typed routing (Plan 4), conditional/`continue` folds (Plan 5), multi-exit (Plan 6), straight-line chains (Plan 7). Not in this plan. ✓

**Known execution risks to watch (not placeholders — flagged for the implementer):**
- **FU-2 keys off claimed loop-site, NOT seed membership (T2.2):** verified in the ANF that both loops fold the *same* accumulator local (the seed `L0`), so any seed-membership predicate flags neither. The re-fold is the fold-bearing loop whose `loop_site` was **not** claimed by a primary candidate. Build `first_refold` to recurse into the break-dispatch `AIf`/`AMatch` (the fold is in the loop's main arm, not at `loop_body` top level).
- **FU-1 fixture reachability (T4.5):** the source-level FU-1 fixture may certify-and-reject at Plan-1 level (zero certified candidates) rather than exercising the gate. The step explicitly instructs: verify with `clean_count`, and fall back to a hand-built ANF unit test that drives `fold_results_dead` to `false` if no certifying-then-FU-1-failing source exists. Do not ship the gate untested.
- **Splice mechanics (T3.3):** locating the exact `Let(seed)…Let(loop)…cont` stretch in a flat ANF let-chain is the fiddliest part. If the optimized ANF interposes unrelated `Let`s between the seed and the loop, the walker must skip them (they don't reference `acc` — the detector already proved `acc` is untouched until the loop). Mirror `find_region`'s skip-unrelated-op traversal.
- **Trapping id lookups (T6, T3, T4):** `builtins.id`/`method_id`/`id_by_canonical` `error()` (trap) on a missing name. Only pass **guaranteed-present** names (`String.concat`, `Vector.append`, `string$builder_*`, `vector$builder_*`) to them; use `try_method_id` or a name guard for anything possibly-absent (e.g. a `*_i64` typed builder op).
- **Record spread-update syntax (T6.3):** `.{ …r with field: v }` is illustrative; use whatever field-copy form Twinkle supports (explicit `BuilderRegionDecision.{ key: ds[0].key, … , loop_site: bogus }`).

**Type consistency:** `rewrite_module`, `rewrite_func_with_decisions`, `decisions_for_func`, `produce_builder_region_decisions`, `BuilderRegionDecision`, `BuilderRegionDecisionTable`, `fold_results_dead`, `resolve_overlaps`, `allocate_locals`, `count_calls_to`, `count_local_uses`, `find_func`, `compile_or_fail`, `links_ok` — names used consistently across tasks. `BuilderRegionKey`/`RegionCandidate`/`region_key` are the landed Plan-1 names.
