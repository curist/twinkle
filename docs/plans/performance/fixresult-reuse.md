# FixResult Reuse Across Summary/Analyze — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the redundant ownership fixpoint that mutable-decision production runs twice per function by caching the summary pass's generic `FixResult` and reusing it in the analyze pass for functions that make no direct in-SCC call.

**Architecture:** The summary SCC driver captures each reusable member's `FixResult` into a `FixCache` threaded out of `compute_for_roots_cached`; `compute_candidate_artifacts` hands that cache to `analyze_selected_with_summaries`, which skips `run_fixpoint_validated` on cache hits and materializes verdicts from the cached fx. A `TWINKLE_FIXVERIFY` flag recomputes-and-compares as a standing guard. Design: `docs/plans/performance/fixresult-reuse-design.md`.

**Tech Stack:** Twinkle boot compiler (`boot/`). Files: `boot/compiler/ownership.tw`, `boot/compiler/summary.tw`, `boot/compiler/codegen/ownership_verdicts.tw`, boot test suites/fixtures. CLI: `target/twk`, `deno` runtime harness.

## Global Constraints

- Treat the boot compiler in `boot/` as the primary implementation. No stage0 (`src/`) changes: this is a boot-codegen-only optimization (the emitted program is unchanged).
- After editing a `.tw` file, run `target/twk fmt <file>` and `target/twk lint boot/main.tw` (must report `No findings.`).
- The gate is `calls_suppressed(blocks, suppress) == false` with `unique_seed` empty. It is equivalent to "singleton, non-self-recursive SCC"; multi-member SCC members always make an in-SCC direct call and are never cached.
- **Soundness invariant:** a cached `FixResult` is consulted only when `unique_seed` is empty. Materialization still reads `unique_seed`.
- Hard accept gates (Task 6): build output byte-identical, `TWINKLE_FIXVERIFY=1` build passes, full boot suite green.
- Heavy verification (full suite, builds, timed runs) runs **one at a time**, never concurrently or backgrounded.

## File Structure

- Modify `boot/compiler/ownership.tw` — cache types/accessors, `calls_suppressed`, `FixResult` serializer, `summarize_function_cached`, `summarize_seeded` return refactor, analyze-side reuse + verify + counter.
- Modify `boot/compiler/summary.tw` — `run_scc` cache threading, `compute_for_roots_cached`, `compute_for_roots` wrapper.
- Modify `boot/compiler/codegen/ownership_verdicts.tw` — wire the cached seam in `compute_candidate_artifacts`.
- Create `boot/tests/fixtures/cfg/fixresult_reuse/{calls,selfrec,mutual}.tw` — gate/driver fixtures.
- Create `boot/tests/suites/fixresult_reuse_suite.tw` — directed gate + driver tests.
- Modify `boot/tests/main.tw` — register the new suite.
- Modify `docs/plans/performance/compiler.md` — record before/after timing + RSS (Task 6).

---

### Task 1: Cache foundation, gate, serializer, and `summarize_seeded` refactor

**Files:**
- Modify: `boot/compiler/ownership.tw`

- [ ] **Step 1: Export `FixResult`**

In `boot/compiler/ownership.tw`, change the type declaration at the `FixResult` definition (currently `type FixResult = .{`):

```tw
pub type FixResult = .{
  exits: Dict<Int, Dict<Int, Int>>,
  exit_valid: Dict<Int, Dict<Int, Bool>>,
  exit_prov: Dict<Int, Dict<Int, Vector<Int>>>,
  exit_field_own: Dict<Int, Dict<Int, ff.FieldMap>>,
  exit_path_prov: Dict<Int, Dict<Int, Dict<Int, Vector<Int>>>>,
}
```

- [ ] **Step 2: Add cache types and accessors**

Immediately after the `FixResult` type, add:

```tw
// Reuse cache: func_id -> the generic (analyze-equivalent) FixResult captured
// during the summary pass. Consumed by the analyze pass to skip re-running the
// ownership fixpoint. See docs/plans/performance/fixresult-reuse-design.md.
pub type FixCache = .{ by_func: Dict<Int, FixResult> }

pub fn empty_fix_cache() FixCache {
  FixCache.{ by_func: Dict.new() }
}

pub fn fix_cache_put(c: FixCache, func_id: Int, fx: FixResult) FixCache {
  c.by_func[func_id] = fx
  c
}

pub fn fix_cache_get(c: FixCache, func_id: Int) FixResult? {
  c.by_func.get(func_id)
}

// Result of summarize_seeded: the whole-function Summary plus the FixResult it
// was derived from (so the SCC driver can cache a reusable fx).
type SeededResult = .{ summary: Summary, fx: FixResult }

// summarize_function_cached: a generic Summary plus its fx and whether that fx is
// analyze-equivalent (reusable). See summarize_function_cached below.
pub type CachedSummary = .{ summary: Summary, fx: FixResult, reusable: Bool }
```

- [ ] **Step 3: Add the `calls_suppressed` gate**

Add near `summarize_function` (any top-level position in the file is fine; place it just before `summarize_function`):

```tw
// The reuse gate: does `f` make a direct call to any func-id in `suppress`?
// When false (and unique_seed is empty), the generic summary run is byte-identical
// to the analyze run — suppress is inert and all direct callees sit in earlier,
// finalized SCCs. Indirect/closure calls (no func-id) never disqualify.
fn calls_suppressed(blocks: Vector<CfgBlock>, suppress: Dict<Int, Bool>) Bool {
  for blk in blocks {
    for inst in blk.instructions {
      case inst.op {
        .ACall(callee, _) => case callee_func_id(callee) {
          .Some(fid) => case suppress.get(fid.id) {
            .Some(active) => if active {
              return true
            },
            .None => {},
          },
          .None => {},
        },
        _ => {},
      }
    }
  }
  false
}
```

- [ ] **Step 4: Add the `FixResult` canonical serializer and verify flag**

Add near the `FixResult` type (top-level). This is used only under `TWINKLE_FIXVERIFY`:

```tw
fn fixverify_enabled() Bool {
  case proc.env("TWINKLE_FIXVERIFY") {
    .Some(_) => true,
    .None => false,
  }
}

fn fixv_isort(xs: Vector<Int>) Vector<Int> {
  out := xs
  n := out.len()
  for i in range(n) {
    j := i
    for j > 0 and out[j - 1] > out[j] {
      tmp := out[j - 1]
      out = out.set_at(j - 1, out[j])
      out = out.set_at(j, tmp)
      j = j - 1
    }
  }
  out
}

fn fixv_join_ints(v: Vector<Int>) String {
  s := ""
  for x in v {
    s = "${s}${x}."
  }
  s
}

fn fixv_int_int(m: Dict<Int, Int>) String {
  s := ""
  for k in fixv_isort(m.keys()) {
    case m.get(k) {
      .Some(v) => s = "${s}${k}:${v},",
      .None => {},
    }
  }
  s
}

fn fixv_int_bool(m: Dict<Int, Bool>) String {
  s := ""
  for k in fixv_isort(m.keys()) {
    case m.get(k) {
      .Some(v) => s = "${s}${k}:${v},",
      .None => {},
    }
  }
  s
}

fn fixv_int_vec(m: Dict<Int, Vector<Int>>) String {
  s := ""
  for k in fixv_isort(m.keys()) {
    case m.get(k) {
      .Some(v) => s = "${s}${k}:[${fixv_join_ints(v)}],",
      .None => {},
    }
  }
  s
}

fn fixv_outer_ii(m: Dict<Int, Dict<Int, Int>>) String {
  s := ""
  for bid in fixv_isort(m.keys()) {
    case m.get(bid) {
      .Some(inner) => s = "${s}B${bid}{${fixv_int_int(inner)}}",
      .None => {},
    }
  }
  s
}

fn fixv_outer_bb(m: Dict<Int, Dict<Int, Bool>>) String {
  s := ""
  for bid in fixv_isort(m.keys()) {
    case m.get(bid) {
      .Some(inner) => s = "${s}B${bid}{${fixv_int_bool(inner)}}",
      .None => {},
    }
  }
  s
}

fn fixv_outer_vv(m: Dict<Int, Dict<Int, Vector<Int>>>) String {
  s := ""
  for bid in fixv_isort(m.keys()) {
    case m.get(bid) {
      .Some(inner) => s = "${s}B${bid}{${fixv_int_vec(inner)}}",
      .None => {},
    }
  }
  s
}

fn fixv_outer_fm(m: Dict<Int, Dict<Int, ff.FieldMap>>) String {
  s := ""
  for bid in fixv_isort(m.keys()) {
    case m.get(bid) {
      .Some(inner) => {
        s = "${s}B${bid}{"
        for lid in fixv_isort(inner.keys()) {
          case inner.get(lid) {
            .Some(fm) => s = "${s}${lid}:(${fixv_int_int(fm.paths)})",
            .None => {},
          }
        }
        s = "${s}}"
      },
      .None => {},
    }
  }
  s
}

fn fixv_outer_pp(m: Dict<Int, Dict<Int, Dict<Int, Vector<Int>>>>) String {
  s := ""
  for bid in fixv_isort(m.keys()) {
    case m.get(bid) {
      .Some(inner) => {
        s = "${s}B${bid}{"
        for lid in fixv_isort(inner.keys()) {
          case inner.get(lid) {
            .Some(pm) => s = "${s}${lid}:(${fixv_int_vec(pm)})",
            .None => {},
          }
        }
        s = "${s}}"
      },
      .None => {},
    }
  }
  s
}

fn render_fix_result(fx: FixResult) String {
  "own[${fixv_outer_ii(fx.exits)}]val[${fixv_outer_bb(fx.exit_valid)}]prov[${fixv_outer_vv(
    fx.exit_prov,
  )}]fo[${fixv_outer_fm(fx.exit_field_own)}]pp[${fixv_outer_pp(fx.exit_path_prov)}]"
}
```

- [ ] **Step 5: Refactor `summarize_seeded` to return `SeededResult`**

Change the `summarize_seeded` signature return type from `Summary` to `SeededResult`:

```tw
fn summarize_seeded(
  f: CfgFunction,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
  unique_seed: Dict<Int, Bool>,
) SeededResult {
```

Change its final expression (currently `Summary.{ params, ret, ret_paths: ret_paths_final }`) to:

```tw
  SeededResult.{ summary: Summary.{ params, ret, ret_paths: ret_paths_final }, fx }
```

(`fx` is the `run_fixpoint_validated(...)` local already computed near the top of the function.)

- [ ] **Step 6: Update `summarize_function` and `summarize_variant` to unwrap `.summary`**

In `summarize_function`, change the body to:

```tw
  summarize_seeded(f, table, b, sem, suppress, Dict.new()).summary
```

In `summarize_variant`, change its final call from `summarize_seeded(f, table, b, sem, suppress, unique_seed)` to:

```tw
  summarize_seeded(f, table, b, sem, suppress, unique_seed).summary
```

- [ ] **Step 7: Add `summarize_function_cached`**

Immediately after `summarize_function`, add:

```tw
// Generic summary plus its fx and reuse eligibility. summarize_function delegates
// to summarize_seeded with an empty unique_seed, so reusable depends only on the
// suppress gate. The SCC driver caches `fx` when `reusable`.
pub fn summarize_function_cached(
  f: CfgFunction,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
) CachedSummary {
  r := summarize_seeded(f, table, b, sem, suppress, Dict.new())
  CachedSummary.{ summary: r.summary, fx: r.fx, reusable: !calls_suppressed(f.blocks, suppress) }
}
```

- [ ] **Step 8: Format, lint, build, and run the full suite (behavior-preserving)**

Run each command, one at a time:

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
target/twk build boot/main.tw -o /tmp/fxr-t1.wasm
target/twk test
```

Expected: fmt succeeds; lint `No findings.`; build writes `/tmp/fxr-t1.wasm`; all boot tests pass (this task only refactors data flow — no behavior change yet).

- [ ] **Step 9: Commit**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: expose FixResult, add reuse cache types and gate"
```

---

### Task 2: Directed gate tests

**Files:**
- Create: `boot/tests/fixtures/cfg/fixresult_reuse/calls.tw`
- Create: `boot/tests/fixtures/cfg/fixresult_reuse/selfrec.tw`
- Create: `boot/tests/suites/fixresult_reuse_suite.tw`
- Modify: `boot/tests/main.tw`

- [ ] **Step 1: Create the fixtures**

Create `boot/tests/fixtures/cfg/fixresult_reuse/calls.tw`:

```tw
fn leaf(x: Int) Int {
  x + 1
}

fn call_leaf(x: Int) Int {
  leaf(x) + 1
}

call_leaf(3)
```

Create `boot/tests/fixtures/cfg/fixresult_reuse/selfrec.tw`:

```tw
fn countdown(n: Int) Int {
  if n <= 0 {
    0
  } else {
    countdown(n - 1)
  }
}

countdown(5)
```

- [ ] **Step 2: Create the test suite**

Create `boot/tests/suites/fixresult_reuse_suite.tw`:

```tw
use @std.testing.assert as assert
use @std.testing as runner

use commands.common.{format_compile_error}
use compiler.builtins.{BuiltinRegistry}
use compiler.cfg
use compiler.opt.semantics as semantics
use compiler.opt.semantics.{OptimizerSemantics}
use compiler.ownership
use compiler.pipeline
use compiler.summary
use lib.module.loader

type Setup = .{ view: cfg.CfgView, table: ownership.SummaryTable, b: BuiltinRegistry, sem: OptimizerSemantics }

fn setup_for(name: String) Result<Setup, String> {
  path := "${loader.find_project_root("boot")}/tests/fixtures/cfg/fixresult_reuse/${name}.tw"
  artifacts := case pipeline.compile_entry_path(path) {
    .Ok(r) => r,
    .Err(e) => return .Err(format_compile_error(e)),
  }
  b := artifacts.builtins
  view := cfg.build_view(artifacts.opt, b)
  view = ownership.prune_dead_merge(view)
  sem := semantics.make_prelude_optimizer_semantics(b)
  table := summary.compute(view, b, sem)
  .Ok(Setup.{ view, table, b, sem })
}

fn find_func(view: cfg.CfgView, name: String) Result<cfg.CfgFunction, String> {
  for f in view.functions {
    if f.name == name {
      return .Ok(f)
    }
  }
  .Err("function ${name} not found")
}

fn suppress_of(ids: Vector<Int>) Dict<Int, Bool> {
  s: Dict<Int, Bool> = Dict.new()
  for id in ids {
    s[id] = true
  }
  s
}

fn has_cache(c: ownership.FixCache, id: Int) Bool {
  case ownership.fix_cache_get(c, id) {
    .Some(_) => true,
    .None => false,
  }
}

pub fn suite() runner.Suite {
  runner
    .suite("fixresult reuse gate")
    .test(
      "call-free leaf is reusable under empty and self suppress",
      fn() {
        s := try setup_for("calls")
        leaf := try find_func(s.view, "leaf")
        r0 := ownership.summarize_function_cached(leaf, s.table, s.b, s.sem, suppress_of([]))
        try assert.ok(r0.reusable, "leaf reusable with empty suppress")
        r1 := ownership.summarize_function_cached(
          leaf,
          s.table,
          s.b,
          s.sem,
          suppress_of([leaf.func_id]),
        )
        try assert.ok(r1.reusable, "leaf makes no calls, so self-suppress is inert")
        .Ok({})
      },
    )
    .test(
      "caller of a suppressed callee is not reusable",
      fn() {
        s := try setup_for("calls")
        leaf := try find_func(s.view, "leaf")
        caller := try find_func(s.view, "call_leaf")
        r := ownership.summarize_function_cached(
          caller,
          s.table,
          s.b,
          s.sem,
          suppress_of([leaf.func_id]),
        )
        try assert.is_false(r.reusable)
        r2 := ownership.summarize_function_cached(caller, s.table, s.b, s.sem, suppress_of([]))
        try assert.ok(r2.reusable, "caller reusable when its callee is not suppressed")
        .Ok({})
      },
    )
    .test(
      "self-recursive function is not reusable when self-suppressed",
      fn() {
        s := try setup_for("selfrec")
        cd := try find_func(s.view, "countdown")
        r := ownership.summarize_function_cached(cd, s.table, s.b, s.sem, suppress_of([cd.func_id]))
        try assert.is_false(r.reusable)
        .Ok({})
      },
    )
}
```

- [ ] **Step 3: Register the suite in the runner**

In `boot/tests/main.tw`, add the import alongside the other `use .suites.*` lines:

```tw
use .suites.fixresult_reuse_suite
```

And add to the `runner.run_all([...])` list (near `cfg_sound_uniqueness_fixtures_suite.suite(),`):

```tw
    fixresult_reuse_suite.suite(),
```

- [ ] **Step 4: Format, lint, run the new suite, and verify it passes**

```bash
target/twk fmt boot/tests/suites/fixresult_reuse_suite.tw boot/tests/fixtures/cfg/fixresult_reuse/calls.tw boot/tests/fixtures/cfg/fixresult_reuse/selfrec.tw
target/twk lint boot/main.tw
target/twk test --filter "fixresult reuse gate"
```

Expected: fmt succeeds; lint `No findings.`; all three gate tests pass. If `--filter` is unavailable, run `target/twk test` and confirm the three named tests pass.

- [ ] **Step 5: Commit**

```bash
git add boot/tests/suites/fixresult_reuse_suite.tw boot/tests/fixtures/cfg/fixresult_reuse boot/tests/main.tw
git commit -m "ownership: cover the FixResult reuse gate (hit / self-recursive / callee suppress)"
```

---

### Task 3: Cache production in the summary SCC driver

**Files:**
- Modify: `boot/compiler/summary.tw`
- Modify: `boot/tests/fixtures/cfg/fixresult_reuse/mutual.tw` (create)
- Modify: `boot/tests/suites/fixresult_reuse_suite.tw`

- [ ] **Step 1: Extend the ownership import in `summary.tw`**

In `boot/compiler/summary.tw`, update the `use compiler.ownership.{ ... }` block: remove `summarize_function` (it is replaced below and would otherwise be an unused import) and add the cache symbols:

```tw
use compiler.ownership.{
  FixCache, ParamRole, ParamSummary, RetVia, ReturnEffect, ReturnOwn, ReturnPathOwn, Summary,
  SummaryTable, analyze_function_with_seed, call_uniques, empty_fix_cache, empty_summary_table,
  fix_cache_put, summarize_function_cached, summarize_variant, summary_get,
}
```

- [ ] **Step 2: Add the `run_scc` result type**

Add near the top of `summary.tw` (after the imports, top-level):

```tw
// run_scc threads a FixCache accumulator so the caller can collect reusable
// members' FixResults captured during the summary fixpoint.
type SccResult = .{ table: SummaryTable, cache: FixCache }

// compute_for_roots_cached returns the summary table plus the reuse cache built
// for the candidate roots. See docs/plans/performance/fixresult-reuse-design.md.
pub type SummaryCacheResult = .{ table: SummaryTable, fix_cache: FixCache }
```

- [ ] **Step 3: Thread the cache through `run_scc`**

Change the `run_scc` signature to add `cache_ids` and `cache_in` and return `SccResult`:

```tw
fn run_scc(
  scc: Vector<Int>,
  index: Dict<Int, CfgFunction>,
  user_ids: Dict<Int, Bool>,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  cache_ids: Dict<Int, Bool>,
  cache_in: FixCache,
) SccResult {
```

Immediately after `scc_set := int_set(members)` (near the top of the body), add:

```tw
  cache := cache_in
```

Replace the member summarization (`next := summarize_function(f, table, b, sem, scc_set)`) with:

```tw
      res := summarize_function_cached(f, table, b, sem, scc_set)
      next := res.summary
      if res.reusable {
        case cache_ids.get(id) {
          .Some(true) => cache = fix_cache_put(cache, id, res.fx),
          _ => {},
        }
      }
```

Change the function's final expression (currently the bare `table` at the end of `run_scc`) to:

```tw
  SccResult.{ table, cache }
```

- [ ] **Step 4: Update `compute` to pass an empty cache set**

In `compute` (`pub fn compute(...)`), replace the SCC loop:

```tw
  for scc in order_sccs(view, index, user_ids) {
    table = run_scc(scc, index, user_ids, table, b, sem)
  }
  table
```

with:

```tw
  no_cache_ids: Dict<Int, Bool> = Dict.new()
  cache := empty_fix_cache()
  for scc in order_sccs(view, index, user_ids) {
    r := run_scc(scc, index, user_ids, table, b, sem, no_cache_ids, cache)
    table = r.table
    cache = r.cache
  }
  table
```

- [ ] **Step 5: Split `compute_for_roots` into a cached body + wrapper**

Rename the existing `pub fn compute_for_roots(...) SummaryTable` to `compute_for_roots_cached` returning `SummaryCacheResult`. Change its signature line:

```tw
pub fn compute_for_roots_cached(
  view: CfgView,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  roots: Dict<Int, Bool>,
) SummaryCacheResult {
```

Inside it, add a cache accumulator before the SCC loop (place next to the `sccs := order_sccs(...)` line):

```tw
  cache := empty_fix_cache()
```

Replace the run branch (`table = run_scc(scc, index, user_ids, table, b, sem)` inside `if run {`) with:

```tw
      r := run_scc(scc, index, user_ids, table, b, sem, roots, cache)
      table = r.table
      cache = r.cache
```

Change its final expression (the trailing `table`) to:

```tw
  SummaryCacheResult.{ table, fix_cache: cache }
```

Then add the thin wrapper directly above `compute_for_roots_cached`:

```tw
pub fn compute_for_roots(
  view: CfgView,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  roots: Dict<Int, Bool>,
) SummaryTable {
  compute_for_roots_cached(view, b, sem, roots).table
}
```

- [ ] **Step 6: Format, lint, build (behavior-preserving so far)**

```bash
target/twk fmt boot/compiler/summary.tw
target/twk lint boot/main.tw
target/twk build boot/main.tw -o /tmp/fxr-t3.wasm
```

Expected: fmt succeeds; lint `No findings.` (confirms `summarize_function` is no longer imported unused); build writes `/tmp/fxr-t3.wasm`.

- [ ] **Step 7: Add the mutual-recursion fixture**

Create `boot/tests/fixtures/cfg/fixresult_reuse/mutual.tw`:

```tw
fn ping(n: Int) Int {
  if n <= 0 {
    0
  } else {
    pong(n - 1)
  }
}

fn pong(n: Int) Int {
  if n <= 0 {
    1
  } else {
    ping(n - 1)
  }
}

fn plain(x: Int) Int {
  x * 2
}

ping(4) + plain(3)
```

- [ ] **Step 8: Add the driver test**

This driver test covers the SCC-miss case. The seeded/variant-miss case (design's fourth directed case) is **structural**: only `run_scc`'s generic `summarize_function_cached` path calls `fix_cache_put`; `summarize_variant` never touches the cache, and the analyze-side `unique_seed`-empty invariant (Task 4) blocks any seeded reuse. It is additionally guarded end-to-end by the `TWINKLE_FIXVERIFY` build (Task 5 Step 4), so no brittle variant-internals test is added.

In `boot/tests/suites/fixresult_reuse_suite.tw`, add this `.test(...)` to the chain (before the final `.Ok`-returning close of `suite()` — i.e. after the last existing `.test(...)`):

```tw
    .test(
      "compute_for_roots_cached caches the singleton and excludes the mutual SCC",
      fn() {
        s := try setup_for("mutual")
        ping := try find_func(s.view, "ping")
        pong := try find_func(s.view, "pong")
        plain := try find_func(s.view, "plain")
        cand := suppress_of([ping.func_id, pong.func_id, plain.func_id])
        res := summary.compute_for_roots_cached(s.view, s.b, s.sem, cand)
        try assert.ok(has_cache(res.fix_cache, plain.func_id), "plain (singleton, call-free) cached")
        try assert.is_false(has_cache(res.fix_cache, ping.func_id), "ping (mutual SCC) not cached")
        try assert.is_false(has_cache(res.fix_cache, pong.func_id), "pong (mutual SCC) not cached")
        .Ok({})
      },
    )
```

- [ ] **Step 9: Format, lint, run the suite**

```bash
target/twk fmt boot/tests/suites/fixresult_reuse_suite.tw boot/tests/fixtures/cfg/fixresult_reuse/mutual.tw
target/twk lint boot/main.tw
target/twk test --filter "fixresult reuse gate"
```

Expected: all four tests pass (three gate + one driver).

- [ ] **Step 10: Commit**

```bash
git add boot/compiler/summary.tw boot/tests/suites/fixresult_reuse_suite.tw boot/tests/fixtures/cfg/fixresult_reuse/mutual.tw
git commit -m "summary: build the FixResult reuse cache in the SCC driver"
```

---

### Task 4: Analyze-side reuse, verify mode, and hit/miss counter

**Files:**
- Modify: `boot/compiler/ownership.tw`

- [ ] **Step 1: Add `cached_fx` to `ownership_stage` and reuse/verify**

Change the `ownership_stage` signature to add a trailing `cached_fx` param:

```tw
fn ownership_stage(
  blocks: Vector<CfgBlock>,
  params: Vector<LocalId>,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  unique_seed: Dict<Int, Bool>,
  label: String,
  cached_fx: FixResult?,
) Vector<CfgBlock> {
```

Replace the fx computation line (`fx := run_fixpoint_validated(blocks, params, table, b, sem, no_suppress, unique_seed, label)`) with:

```tw
  reuse := case cached_fx {
    .Some(cf) => if unique_seed.keys().len() == 0 {
      .Some(cf)
    } else {
      .None
    },
    .None => .None,
  }
  fx := case reuse {
    .Some(cf) => {
      if fixverify_enabled() {
        fresh := run_fixpoint_validated(blocks, params, table, b, sem, no_suppress, unique_seed, label)
        if render_fix_result(fresh) != render_fix_result(cf) {
          error("fixverify mismatch: ${label}")
        }
      }
      cf
    },
    .None => run_fixpoint_validated(blocks, params, table, b, sem, no_suppress, unique_seed, label),
  }
```

(`no_suppress` is the empty dict already declared at the top of `ownership_stage`.)

- [ ] **Step 2: Add `cached_fx` to `analyze_function`**

Change the `analyze_function` signature to add a trailing `cached_fx` param:

```tw
fn analyze_function(
  f: CfgFunction,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  unique_seed: Dict<Int, Bool>,
  cached_fx: FixResult?,
) CfgFunction {
```

Change its `ownership_stage(...)` call (the `blocks = ownership_stage(blocks, f.params, table, b, sem, unique_seed, "analyze:${f.name}")` line) to pass `cached_fx`:

```tw
  blocks = ownership_stage(blocks, f.params, table, b, sem, unique_seed, "analyze:${f.name}", cached_fx)
```

- [ ] **Step 3: Thread the cache through `analyze_selected_with_summaries` (both branches)**

Change the `analyze_selected_with_summaries` signature to add a trailing `cache` param:

```tw
pub fn analyze_selected_with_summaries(
  view: CfgView,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  table: SummaryTable,
  selected: Dict<Int, Bool>,
  cache: FixCache,
) CfgView {
```

In the **untimed** branch, change the `analyze_function(clear_function_analysis(f), table, b, sem, Dict.new())` call to:

```tw
        analyze_function(clear_function_analysis(f), table, b, sem, Dict.new(), fix_cache_get(cache, f.func_id))
```

In the **timed** branch, change the `functions = .append(analyze_function(cleared, table, b, sem, Dict.new()))` call to:

```tw
      functions = .append(
        analyze_function(cleared, table, b, sem, Dict.new(), fix_cache_get(cache, f.func_id)),
      )
```

- [ ] **Step 4: Add the hit/miss counter to the timed branch**

In the timed branch of `analyze_selected_with_summaries`, add two counters next to the existing `selected_funcs := 0` initializers:

```tw
  cache_hits := 0
  cache_misses := 0
```

Inside the `if should_analyze {` block (where `selected_funcs = selected_funcs + 1` is), add:

```tw
      case fix_cache_get(cache, f.func_id) {
        .Some(_) => cache_hits = cache_hits + 1,
        .None => cache_misses = cache_misses + 1,
      }
```

In the final `if timed {` block of the function (where the existing `[time:...]` line for this function is emitted), add an extra `eprintln` after it:

```tw
    eprintln(
      "[time:mutable:fixcache] hits=${cache_hits} misses=${cache_misses} verify=${fixverify_enabled()}",
    )
```

- [ ] **Step 5: Update the remaining `analyze_function` callers to pass `.None`**

Two callers analyze with no cache:

- In `analyze_with_summaries` (the full/test path), change `analyze_function(f, table, b, sem, Dict.new())` to:

```tw
    analyze_function(f, table, b, sem, Dict.new(), .None)
```

- In `analyze_function_with_seed` (the seeded path), change `analyze_function(f, table, b, sem, unique_seed)` to:

```tw
  analyze_function(f, table, b, sem, unique_seed, .None)
```

- [ ] **Step 6: Format, lint, build, full suite (still behavior-preserving)**

Nothing yet passes a populated cache to `analyze_selected_with_summaries`, so behavior is unchanged.

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
target/twk build boot/main.tw -o /tmp/fxr-t4.wasm
target/twk test
```

Expected: fmt/lint clean; build succeeds; all boot tests pass.

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: reuse cached FixResult in analyze with a fixverify guard"
```

---

### Task 5: Wire the cached seam in `compute_candidate_artifacts`

**Files:**
- Modify: `boot/compiler/codegen/ownership_verdicts.tw`

- [ ] **Step 1: Use the cached summary path and pass the cache to analyze**

In `compute_candidate_artifacts`, replace:

```tw
  table := summary.compute_for_roots(view, b, sem, candidate_funcs)
  t_summary := date.now()
  analyzed := ownership.analyze_selected_with_summaries(view, b, sem, table, candidate_funcs)
```

with:

```tw
  summarized := summary.compute_for_roots_cached(view, b, sem, candidate_funcs)
  table := summarized.table
  t_summary := date.now()
  analyzed := ownership.analyze_selected_with_summaries(
    view,
    b,
    sem,
    table,
    candidate_funcs,
    summarized.fix_cache,
  )
```

- [ ] **Step 2: Format, lint, build**

```bash
target/twk fmt boot/compiler/codegen/ownership_verdicts.tw
target/twk lint boot/main.tw
target/twk build boot/main.tw -o /tmp/fxr-stage2.wasm
```

Expected: fmt/lint clean; build writes `/tmp/fxr-stage2.wasm` (this payload now has reuse active on the candidate path).

- [ ] **Step 3: Verify build output is byte-identical (hard accept gate)**

The change is in the compiler; the test is that a **before-compiler** and an **after-compiler** emit identical output for the **same fixed input program**. Use `main`'s `boot/main.tw` as that fixed input `P`, compiled by each compiler payload.

```bash
# Fixed input P + a from-main compiler payload (no cache):
git worktree add /tmp/fxr-baseline main
target/twk build /tmp/fxr-baseline/boot/main.tw -o /tmp/fxr-payload-before.wasm

# Compile the SAME P with both payloads:
BOOT_WASM=/tmp/fxr-payload-before.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/fxr-baseline/boot/main.tw -o /tmp/fxr-out-before.wasm
BOOT_WASM=/tmp/fxr-stage2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/fxr-baseline/boot/main.tw -o /tmp/fxr-out-after.wasm

cmp /tmp/fxr-out-before.wasm /tmp/fxr-out-after.wasm && echo "BYTE-IDENTICAL"
```

Expected: `cmp` prints nothing and `BYTE-IDENTICAL` — for the same input, the cache-active compiler emits the same program as the from-main compiler. If they differ, STOP: the reuse changed a mutable decision; inspect via `TWINKLE_FIXVERIFY` (next step) before proceeding. Keep the `/tmp/fxr-baseline` worktree and `/tmp/fxr-payload-before.wasm` for Task 6; remove the worktree at the end with `git worktree remove /tmp/fxr-baseline --force`.

- [ ] **Step 4: Run a `TWINKLE_FIXVERIFY=1` build (hard accept gate)**

```bash
TWINKLE_FIXVERIFY=1 BOOT_WASM=/tmp/fxr-stage2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/fxr-verify.wasm
echo "exit=$?"
```

Expected: exit 0, no `fixverify mismatch` trap — every cache hit's cached fx equals a fresh recompute on the real hot functions.

- [ ] **Step 5: Confirm the cache is actually exercised**

```bash
TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/fxr-stage2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/fxr-t.wasm 2>&1 \
  | grep "time:mutable:fixcache"
```

Expected: a `[time:mutable:fixcache] hits=<n> misses=<m> verify=false` line with `hits` in the low hundreds (~420 order), confirming reuse is live.

- [ ] **Step 6: Full boot suite green + census collateral-damage check**

```bash
target/twk test
```

Expected: all boot tests pass.

The `--census` command uses the full `compute_artifacts` path (no cache), so a census diff cannot regress from this change — it is a collateral-damage check that the shared `ownership.tw` edits did not disturb the full path:

```bash
target/twk build /tmp/fxr-baseline/boot/main.tw -o /dev/null 2>/dev/null; true  # ensure baseline worktree exists (see Task 5 Step 3)
BOOT_WASM=/tmp/fxr-stage2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs ir --census --sites boot/main.tw >/tmp/fxr-census-after.txt 2>/dev/null
target/twk ir --census --sites /tmp/fxr-baseline/boot/main.tw >/tmp/fxr-census-base.txt 2>/dev/null
diff /tmp/fxr-census-base.txt /tmp/fxr-census-after.txt && echo "CENSUS UNCHANGED"
```

Expected: `CENSUS UNCHANGED` (the full path is untouched).

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/codegen/ownership_verdicts.tw
git commit -m "codegen: reuse summary FixResults in candidate ownership analysis"
```

---

### Task 6: Measure and record

**Files:**
- Modify: `docs/plans/performance/compiler.md`
- Modify: `docs/plans/README.md` (remove the plan row on completion)

- [ ] **Step 1: A/B timing (attributable)**

Before-compiler (`/tmp/fxr-payload-before.wasm`, from Task 5 Step 3) vs after-compiler (`/tmp/fxr-stage2.wasm`), each compiling the same fixed input `/tmp/fxr-baseline/boot/main.tw`, three runs each. (Recreate the payload/worktree via Task 5 Step 3 if removed.)

Before:
```bash
for i in 1 2 3; do TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/fxr-payload-before.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build /tmp/fxr-baseline/boot/main.tw -o /tmp/ab-before-$i.wasm 2>&1 | grep -E "produce_mutable_decisions|time:mutable:artifacts"; done
```

After:
```bash
for i in 1 2 3; do TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/fxr-stage2.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build /tmp/fxr-baseline/boot/main.tw -o /tmp/ab-after-$i.wasm 2>&1 | grep -E "produce_mutable_decisions|time:mutable:artifacts|time:mutable:fixcache"; done
```

Expected: `produce_mutable_decisions` and `[time:mutable:artifacts] ownership=` drop materially after (the analyze fixpoint is skipped for cache hits).

- [ ] **Step 2: Peak RSS (memory acceptance)**

Same before/after payloads, same fixed input:
```bash
/usr/bin/time -l deno run --allow-read --allow-write --allow-env --env=BOOT_WASM=/tmp/fxr-payload-before.wasm tools/js_runtime/deno_main.mjs build /tmp/fxr-baseline/boot/main.tw -o /tmp/rss-before.wasm 2>&1 | grep "maximum resident set size"
/usr/bin/time -l deno run --allow-read --allow-write --allow-env --env=BOOT_WASM=/tmp/fxr-stage2.wasm tools/js_runtime/deno_main.mjs build /tmp/fxr-baseline/boot/main.tw -o /tmp/rss-after.wasm 2>&1 | grep "maximum resident set size"
```

Expected: the after-RSS increase (from retaining ~420 `FixResult`s) is within an acceptable bound. If it is not, apply the design's fallback — in `run_scc` Step 3, add a size predicate before `fix_cache_put` (e.g. skip when `f.blocks.len() > <threshold>`), re-measure, and note the threshold.

- [ ] **Step 3: Self-host stability**

```bash
BOOT_WASM=/tmp/fxr-stage2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/fxr-stage3.wasm
cmp /tmp/fxr-stage2.wasm /tmp/fxr-stage3.wasm && echo "SELF-HOST STABLE"
```

Expected: `SELF-HOST STABLE`.

- [ ] **Step 4: Record results**

Append a section to `docs/plans/performance/compiler.md`:

- Heading: `## Update: FixResult reuse across summary/analyze passes`
- Explanation: mutable-decision production ran the ownership fixpoint twice per function; the summary pass's generic `FixResult` is byte-identical to the analyze run for singleton non-self-recursive functions (gate: no direct in-SCC call). Caching and reusing it skips the analyze fixpoint for those functions.
- Before/after `[time] produce_mutable_decisions` and `[time:mutable:artifacts]` lines from Step 1, the `[time:mutable:fixcache]` hit/miss line, and the RSS delta from Step 2.
- Acceptance: build output byte-identical (`cmp`), `TWINKLE_FIXVERIFY=1` build clean, full boot suite green.

- [ ] **Step 5: Remove the plan row and commit**

Per house rule, on completion delete this plan's row from `docs/plans/README.md` (if present) rather than marking it Done.

```bash
target/twk fmt docs/plans/performance/compiler.md 2>/dev/null || true
git add docs/plans/performance/compiler.md docs/plans/README.md
git commit -m "docs: record FixResult reuse performance results"
```

---

## Risks and Stop Rules

- **Build output differs (Task 5 Step 3):** the reuse changed a mutable decision. Do not accept. Run `TWINKLE_FIXVERIFY=1` to find the first mismatching function; the gate (`calls_suppressed`) or the SCC-ordering assumption is wrong for it. Fix the gate; never loosen it to force byte-identity.
- **`hits=0` (Task 5 Step 5):** the cache is not being populated or not consulted — the seam wiring (Task 5 Step 1) or the `cache_ids` threading (Task 3) is wrong. Investigate before trusting timing.
- **RSS regression (Task 6 Step 2):** apply the block-count-cap predicate in `run_scc`; the largest functions save the most, so a partial cache still captures most of the win.
- **Timing win marginal:** if `produce_mutable_decisions` does not drop despite `hits` in the hundreds, the analyze fixpoint was not the dominant cost for the cached set; record the finding and stop rather than adding complexity.

## Expected Outcome

`produce_mutable_decisions` drops materially by skipping the analyze-pass ownership fixpoint for ~420 singleton, non-self-recursive functions (including `link`), with the emitted program byte-identical and a standing `TWINKLE_FIXVERIFY` guard.
