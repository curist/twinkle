# Mutable Decision Loop-Seed Filtering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cut `produce_mutable_decisions` time by replacing all-live-loop-header optimistic ownership seeds with a relevance-closed seed set that preserves nested loop-carried mutable verdicts.

**Architecture:** Keep the ownership fixpoint and validation rules as the source of truth. Change only the initial loop-seed candidate set: start from loop-header params and mutable-proof-relevant bases, then close over loop edge support so nested-loop proofs still receive the live-through non-param seeds they require. Existing validation remains unchanged and may only remove optimistic seeds, never add unsound facts.

**Tech Stack:** Twinkle boot compiler (`boot/`), structural CFG ownership analysis (`boot/compiler/ownership.tw`), summary driver (`boot/compiler/summary.tw`), mutable-decision producer (`boot/compiler/codegen/mutable_produce.tw`), self-hosted CLI (`target/twk`), boot test fixtures under `boot/tests/fixtures/cfg/sound_uniqueness/` and suites under `boot/tests/suites/`.

## Global Constraints

- Treat the boot compiler in `boot/` as the primary implementation.
- Preserve soundness by keeping `retain_valid_loop_seeds(...)` / `validate_loop_seed(...)` as the final authority for loop-carried uniqueness.
- Do not use the failed param-only experiment: nested-loop vector fixtures require some non-param live-through support seeds.
- Codegen continues to consume decisions only; it must not re-prove ownership or inspect loop seeds.
- Any missing proof must fall back to persistent operations.
- Timing output is investigation evidence only; correctness acceptance comes from boot tests, targeted CFG fixture output, and mutable audit/census output.
- After editing `.tw` files, run `target/twk fmt <changed.tw>` and `target/twk lint boot/main.tw`.

---

## Evidence From Current Investigation

Representative bundled-CLI baseline with detailed timings:

```text
[time:summary:roots] total=12280ms index=1ms closure=56ms seed=4ms order=23ms run=12195ms funcs=3817 roots=460 wanted=3170
[time:own:selected] total=8604ms clear=4ms analyze=8597ms selected_funcs=460
[time:mutable:artifacts] cfg=772ms summary=12280ms ownership=8604ms scope_roots=460
[time] produce_mutable_decisions: 21857ms
```

The dominant per-function cost is repeated loop-seed validation reruns inside `run_fixpoint_validated(...)`, especially `link`:

```text
summary:link   reruns=14 initial_seeds=875 final_seeds=213
analyze:link   reruns=14 initial_seeds=875 final_seeds=213
```

Seed shape for the worst `link` instance:

```text
initial_param_seeds=78   initial_nonparam_seeds=797
final_param_seeds=23     final_nonparam_seeds=190
```

Param-only seeding was tested as an experiment. It cut the stage from roughly 21.8s to roughly 9.7s, but failed required nested-loop vector tests. This proves two facts:

- Most initial non-param seeds are irrelevant noise.
- Some non-param live-through seeds are required as support for nested-loop mutable proofs.

---

## File Structure

- Modify `boot/compiler/ownership.tw`
  - Add gated loop-seed diagnostics.
  - Replace `collect_loop_seed_candidates(...)` with a relevance-driven seed collector.
  - Keep `validate_loop_seed(...)` unchanged.
- Modify `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`
  - Add assertions that the nested-loop support-seed cases still render owned verdicts.
- Modify or create fixtures under `boot/tests/fixtures/cfg/sound_uniqueness/`
  - Reuse existing nested-loop fixtures when possible.
  - Add a minimal live-through-support fixture only if existing failures do not isolate the support case clearly.
- Optionally modify `docs/plans/performance/compiler.md`
  - Record the before/after timing shape after the fix lands.

---

### Task 1: Land focused loop-seed diagnostics

**Note:** This task may already be partially applied in the active worktree. If `LoopSeedStats`, `loop_seed_stats(...)`, and the extended `[time:own:fixpoint_validated]` log already exist in `boot/compiler/ownership.tw`, reconcile the current implementation with the snippets below instead of adding duplicate helpers.

**Files:**
- Modify: `boot/compiler/ownership.tw`

**Interfaces:**
- Consumes: existing `LoopSeedSet = Dict<String, Bool>`, `collect_loop_seed_candidates(...)`, `run_fixpoint_validated(...)`.
- Produces:
  - private `type LoopSeedStats = .{ headers: Int, param_seeds: Int, nonparam_seeds: Int }`
  - private `fn loop_seed_stats(blocks: Vector<CfgBlock>, seeds: LoopSeedSet) LoopSeedStats`
  - gated `[time:own:fixpoint_validated]` fields for initial/final seed shape.

- [ ] **Step 1: Add seed stats helpers near loop seed helpers**

In `boot/compiler/ownership.tw`, place this immediately after `retain_valid_loop_seeds(...)`:

```tw
// Diagnostics only: characterize how broad the optimistic loop seed set is.
type LoopSeedStats = .{ headers: Int, param_seeds: Int, nonparam_seeds: Int }

fn loop_seed_stats(blocks: Vector<CfgBlock>, seeds: LoopSeedSet) LoopSeedStats {
  headers := 0
  param_seeds := 0
  nonparam_seeds := 0
  for blk in blocks {
    header_has_seed := false
    for lid in blk.entry.live {
      if loop_seed_active(seeds, blk.id.id, lid) {
        header_has_seed = true
        case param_index(blk, lid) {
          .Some(_) => param_seeds = param_seeds + 1,
          .None => nonparam_seeds = nonparam_seeds + 1,
        }
      }
    }
    if header_has_seed {
      headers = headers + 1
    }
  }
  LoopSeedStats.{ headers, param_seeds, nonparam_seeds }
}
```

- [ ] **Step 2: Extend `run_fixpoint_validated(...)` diagnostics**

Inside `run_fixpoint_validated(...)`, after initial seed collection, record initial stats only when timings are enabled:

```tw
seeds := collect_loop_seed_candidates(blocks)
initial_seed_count := seeds.keys().len()
initial_seed_stats := if timed {
  loop_seed_stats(blocks, seeds)
} else {
  LoopSeedStats.{ headers: 0, param_seeds: 0, nonparam_seeds: 0 }
}
```

Then update the existing slow/rerun log to include both initial and final stats:

```tw
if timed {
  t_done := date.now()
  if reruns > 3 or t_done - t0 > 75.0 {
    final_seed_stats := loop_seed_stats(blocks, seeds)
    eprintln(
      "[time:own:fixpoint_validated] func=${label} total=${t_done - t0}ms reruns=${reruns} initial_seeds=${initial_seed_count} final_seeds=${seeds
        .keys()
        .len()} initial_headers=${initial_seed_stats.headers} initial_param_seeds=${initial_seed_stats.param_seeds} initial_nonparam_seeds=${initial_seed_stats.nonparam_seeds} final_headers=${final_seed_stats.headers} final_param_seeds=${final_seed_stats.param_seeds} final_nonparam_seeds=${final_seed_stats.nonparam_seeds} blocks=${blocks.len()} insts=${block_instruction_count(
        blocks,
      )}",
    )
  }
}
```

- [ ] **Step 3: Verify diagnostics compile**

Run:

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
target/twk build boot/main.tw -o /tmp/seedstats-stage2.wasm
```

Expected: formatter succeeds, lint reports `No findings.`, build writes `/tmp/seedstats-stage2.wasm`.

- [ ] **Step 4: Capture seed-shape baseline**

Run the freshly built payload:

```bash
TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/seedstats-stage2.wasm \
  deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs \
  build boot/main.tw -o /tmp/seedstats-stage3.wasm \
  >/tmp/seedstats.out 2>/tmp/seedstats.err

rg "time:own:fixpoint_validated.*(summary:link|analyze:link|emit_module|extract_exports_for_module)|time:mutable:artifacts|produce_mutable_decisions" /tmp/seedstats.err
```

Expected: `summary:link` and `analyze:link` report high `initial_nonparam_seeds` and much smaller `final_nonparam_seeds`.

- [ ] **Step 5: Commit diagnostics**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: instrument loop seed validation cost"
```

---

### Task 2: Add a focused support-seed regression fixture

**Files:**
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/loop_live_through_seed_support.tw`
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes: existing fixture helpers in `cfg_sound_uniqueness_fixtures_suite.tw`.
- Produces: a small regression that fails under param-only seeding and passes under all-live seeding / relevance-closed seeding.

- [ ] **Step 1: Create the fixture**

Create `boot/tests/fixtures/cfg/sound_uniqueness/loop_live_through_seed_support.tw`:

```tw
fn make_flags(n: Int) Vector<Bool> {
  collect _ in range(n) {
    true
  }
}

fn support(n: Int) Int {
  flags := make_flags(n)
  outer := 0
  count := 0
  for outer < n {
    inner := outer + 1
    for inner < n {
      if flags[inner] {
        flags = .set_at(inner, false)
      }
      inner = inner + 1
    }
    outer = outer + 1
  }
  for i in range(n) {
    if flags[i] {
      count = count + 1
    }
  }
  count
}

support(16)
```

This shape deliberately carries `flags` as a non-param live-through local through the outer loop while the inner loop receives it as a loop-carried value.

- [ ] **Step 2: Add the fixture assertion**

In `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`, add a chained test near the existing nested-loop/sieve-shaped tests:

```tw
    .test(
      "loop live-through support seed preserves nested vector set verdict",
      fn() {
        out := try render_entry("loop_live_through_seed_support")
        support := try section_between(out, "fn support", "fn set_at__Bool")
        try assert.str_contains(support, "terminator: loop-back-edge")
        try assert.str_contains(support, "verdict ->")
        try assert.str_contains(support, "[unique:p0]")
        for line in support.lines() {
          if line.contains("facts.in=") or line.contains("facts.out=") {
            try assert.is_false(line.contains(": Shared"))
          }
        }
        .Ok({})
      },
    )
```

This matches the existing suite shape: `render_entry(name)` takes the fixture basename without `.tw`, tests are chained from `runner.suite(...)`, and `section_between(...)` isolates the caller section so helper/wrapper verdicts cannot satisfy the assertion accidentally.

- [ ] **Step 3: Verify current behavior before filtering**

Run:

```bash
target/twk fmt boot/tests/fixtures/cfg/sound_uniqueness/loop_live_through_seed_support.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
target/twk lint boot/main.tw
target/twk test --filter "loop live-through support seed"
```

Expected: the new test passes before seed filtering. If the filter selector is unavailable in the current CLI, run `target/twk test` and inspect the named test.

- [ ] **Step 4: Commit regression fixture**

```bash
git add boot/tests/fixtures/cfg/sound_uniqueness/loop_live_through_seed_support.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git commit -m "ownership: cover live-through loop seed support"
```

---

### Task 3: Implement relevance-closed loop seed collection

**Files:**
- Modify: `boot/compiler/ownership.tw`

**Interfaces:**
- Consumes:
  - `LoopSeedSet`
  - `loop_seed_key(...)`
  - `loop_seed_active(...)`
  - `param_index(...)`
  - `atom_local_id(...)`
  - `loop_header_has_backedge(...)`
  - `AnfOp` variants already imported in `ownership.tw`
  - `call_info(sem, fid)` and `CallSemantics.effect == .Update` for COW update bases
- Produces:
  - private `fn loop_seed_insert(seeds: LoopSeedSet, block_id: Int, local_id: Int) LoopSeedSet`
  - private `fn loop_headers_with_live_local(blocks: Vector<CfgBlock>, local_id: Int) Vector<CfgBlock>`
  - private `fn collect_direct_loop_seed_roots(blocks: Vector<CfgBlock>, sem: OptimizerSemantics) LoopSeedSet`
  - private `fn close_loop_seed_support(blocks: Vector<CfgBlock>, seeds: LoopSeedSet) LoopSeedSet`
  - updated `fn collect_loop_seed_candidates(blocks: Vector<CfgBlock>, sem: OptimizerSemantics) LoopSeedSet`

- [ ] **Step 1: Change the seed collector signature**

Change:

```tw
fn collect_loop_seed_candidates(blocks: Vector<CfgBlock>) LoopSeedSet
```

to:

```tw
fn collect_loop_seed_candidates(blocks: Vector<CfgBlock>, sem: OptimizerSemantics) LoopSeedSet
```

Update the only call in `run_fixpoint_validated(...)`:

```tw
seeds := collect_loop_seed_candidates(blocks, sem)
```

- [ ] **Step 2: Add seed set insertion helper**

Near `loop_seed_active(...)`, add:

```tw
fn loop_seed_insert(seeds: LoopSeedSet, block_id: Int, local_id: Int) LoopSeedSet {
  seeds[loop_seed_key(block_id, local_id)] = true
  seeds
}
```

- [ ] **Step 3: Add loop-header lookup by live local**

Near `loop_header_has_backedge(...)`, add:

```tw
fn loop_headers_with_live_local(blocks: Vector<CfgBlock>, local_id: Int) Vector<CfgBlock> {
  out: Vector<CfgBlock> = []
  for blk in blocks {
    if blk.name == "loop.header" and loop_header_has_backedge(blocks, blk) {
      for lid in blk.entry.live {
        if lid == local_id {
          out = .append(blk)
        }
      }
    }
  }
  out
}
```

This helper may over-approximate by local id within one function, but local ids are function-local and deterministic.

- [ ] **Step 4: Add direct root collection**

Replace the old all-live `collect_loop_seed_candidates(...)` body with direct-root collection plus support closure. First add:

```tw
fn add_loop_header_param_seeds(blocks: Vector<CfgBlock>, seeds: LoopSeedSet) LoopSeedSet {
  out := seeds
  for blk in blocks {
    if blk.name == "loop.header" and loop_header_has_backedge(blocks, blk) {
      for p in blk.params {
        out = loop_seed_insert(out, blk.id.id, p.id)
      }
    }
  }
  out
}

fn add_seed_for_local_live_at_headers(
  blocks: Vector<CfgBlock>,
  seeds: LoopSeedSet,
  local_id: Int,
) LoopSeedSet {
  out := seeds
  for header in loop_headers_with_live_local(blocks, local_id) {
    out = loop_seed_insert(out, header.id.id, local_id)
  }
  out
}

fn collect_update_base_seed_roots(
  blocks: Vector<CfgBlock>,
  sem: OptimizerSemantics,
  seeds: LoopSeedSet,
) LoopSeedSet {
  out := seeds
  for blk in blocks {
    for inst in blk.instructions {
      case inst.op {
        .ACall(callee, args) => case callee_func_id(callee) {
          .Some(fid) => case call_info(sem, fid) {
            .Some(cs) => case cs.effect {
              .Update => case cs.cow_base_arg {
                .Some(bi) => if bi >= 0 and bi < args.len() {
                  case atom_local_id(args[bi]) {
                    .Some(base_id) => out = add_seed_for_local_live_at_headers(blocks, out, base_id),
                    .None => {},
                  }
                },
                .None => {},
              },
              _ => {},
            },
            .None => {},
          },
          .None => {},
        },
        .ARecordUpdate(base, _, _, _, _) => case atom_local_id(base) {
          .Some(base_id) => out = add_seed_for_local_live_at_headers(blocks, out, base_id),
          .None => {},
        },
        _ => {},
      }
    }
  }
  out
}

fn collect_direct_loop_seed_roots(blocks: Vector<CfgBlock>, sem: OptimizerSemantics) LoopSeedSet {
  seeds: LoopSeedSet = Dict.new()
  seeds = add_loop_header_param_seeds(blocks, seeds)
  collect_update_base_seed_roots(blocks, sem, seeds)
}
```

- [ ] **Step 5: Add support closure over loop predecessor edge args**

Add:

```tw
fn add_support_for_seeded_param(
  blocks: Vector<CfgBlock>,
  seeds: LoopSeedSet,
  blk: CfgBlock,
  lid: Int,
) LoopSeedSet {
  pidx := case param_index(blk, lid) {
    .Some(i) => i,
    .None => return seeds,
  }
  out := seeds
  for pe in blk.preds {
    if pidx < pe.args.len() {
      case atom_local_id(pe.args[pidx]) {
        .Some(src) => out = add_seed_for_local_live_at_headers(blocks, out, src),
        .None => {},
      }
    }
  }
  out
}

fn close_loop_seed_support(blocks: Vector<CfgBlock>, seeds: LoopSeedSet) LoopSeedSet {
  out := seeds
  changed := true
  for changed {
    changed = false
    before := out.keys().len()
    for blk in blocks {
      if blk.name == "loop.header" and loop_header_has_backedge(blocks, blk) {
        for lid in blk.entry.live {
          if loop_seed_active(out, blk.id.id, lid) {
            out = add_support_for_seeded_param(blocks, out, blk, lid)
          }
        }
      }
    }
    if out.keys().len() != before {
      changed = true
    }
  }
  out
}
```

This keeps non-param seeds only when they support a seeded loop-header param or a mutable update base carried through loop headers.

- [ ] **Step 6: Wire the new collector**

Implement:

```tw
fn collect_loop_seed_candidates(blocks: Vector<CfgBlock>, sem: OptimizerSemantics) LoopSeedSet {
  seeds := collect_direct_loop_seed_roots(blocks, sem)
  close_loop_seed_support(blocks, seeds)
}
```

- [ ] **Step 7: Run focused validation**

Run:

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
target/twk test --filter "cfg sound uniqueness"
```

Expected: nested-loop and sieve-shaped sound uniqueness tests pass, including the new support fixture.

If the filter selector is unavailable in the current CLI, run:

```bash
target/twk test
```

Expected: all boot tests pass.

- [ ] **Step 8: Measure candidate timing**

Build a fresh payload and run the timed build through it:

```bash
target/twk build boot/main.tw -o /tmp/relevant-seeds-stage2.wasm
TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/relevant-seeds-stage2.wasm \
  deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs \
  build boot/main.tw -o /tmp/relevant-seeds-stage3.wasm \
  >/tmp/relevant-seeds.out 2>/tmp/relevant-seeds.err

rg "time:summary:roots|time:own:selected|time:mutable:artifacts|produce_mutable_decisions|time:own:fixpoint_validated.*(summary:link|analyze:link)" /tmp/relevant-seeds.err
```

Expected: `initial_seeds` for `link` is much closer to `final_seeds` than the current `875 -> 213`, and `produce_mutable_decisions` is materially below the pre-filter baseline. Do not enforce a numeric threshold as correctness.

- [ ] **Step 9: Commit relevance-closed seed collection**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: seed loop uniqueness from relevant support locals"
```

---

### Task 4: Validate full compiler behavior and mutable emission

**Files:**
- No source changes expected unless a test failure exposes a real issue.

**Interfaces:**
- Consumes: relevance-closed loop seed collector from Task 3.
- Produces: validation evidence for correctness and performance.

- [ ] **Step 1: Run the boot test suite**

```bash
target/twk test
```

Expected: all tests pass.

- [ ] **Step 2: Run the mutable census smoke check through the fresh payload**

Use the `/tmp/relevant-seeds-stage2.wasm` payload produced in Task 3 so this check exercises the new compiler, not a stale bundled `target/twk`.

```bash
TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/relevant-seeds-stage2.wasm \
  deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs \
  ir --census --sites boot/main.tw \
  >/tmp/mutable-census.txt 2>/tmp/mutable-census.err

rg "mutable decisions|selected|nested|vector_set|dict_set|record_shell" /tmp/mutable-census.txt | head -n 80
rg "time:mutable|time:own:fixpoint_validated" /tmp/mutable-census.err | head -n 80
```

Expected: census renders mutable decisions and timing output. Existing selected mutable sites should not disappear unless the test suite was intentionally re-baselined for a conservative fallback.

- [ ] **Step 3: Run the build timing check through the fresh payload**

Use `BOOT_WASM=/tmp/relevant-seeds-stage2.wasm` here as well. Running `target/twk build boot/main.tw -o /tmp/stage3-after-seed-filter.wasm` directly is valid only after rebuilding/bundling `target/twk` from the new payload.

```bash
TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/relevant-seeds-stage2.wasm \
  deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs \
  build boot/main.tw -o /tmp/stage3-after-seed-filter.wasm \
  >/tmp/seed-filter-build.out 2>/tmp/seed-filter-build.err

rg "time:summary:roots|time:own:selected|time:mutable:artifacts|produce_mutable_decisions" /tmp/seed-filter-build.err
```

Expected: detailed timings show lower `produce_mutable_decisions` than the current regression baseline. Record the exact numbers in the task summary.

- [ ] **Step 4: Commit validation notes if needed**

If no docs are changed, do not create a validation-only commit. If Task 5 updates performance docs, commit that there.

---

### Task 5: Update performance tracking and trim diagnostic noise

**Files:**
- Modify: `boot/compiler/ownership.tw`
- Modify: `docs/plans/performance/compiler.md`

**Interfaces:**
- Consumes: timing evidence from Task 4.
- Produces: durable performance note and acceptable diagnostic volume under `TWINKLE_TIMINGS`.

- [ ] **Step 1: Keep only useful gated diagnostics**

In `boot/compiler/ownership.tw`, keep these diagnostics under `TWINKLE_TIMINGS`:

- `[time:own:fixpoint_validated]` for slow or high-rerun functions.
- Seed stats on that line.

Remove or raise thresholds for noisy exploratory lines if they are too verbose for normal timed builds. The final timed build should not emit thousands of ownership lines.

- [ ] **Step 2: Run formatting and lint**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
```

Expected: formatter succeeds and lint reports `No findings.`

- [ ] **Step 3: Add a performance note**

Append a section to `docs/plans/performance/compiler.md` using the exact Task 4 timing lines copied from `/tmp/seed-filter-build.err`. The section must include:

- Heading: `## Update: loop-seed relevance for mutable decisions`
- Explanation: Phase 8A mutable-decision production was dominated by loop-seed validation in ownership analysis. The worst `link` CFG started with hundreds of optimistic loop-header seeds and reran the ownership fixpoint repeatedly while validation removed irrelevant seeds. Relevance-closed seed collection keeps the nested-loop support seeds needed for owned vector updates while avoiding most all-live header seeds.
- Baseline block:

```text
[time:mutable:artifacts] cfg=772ms summary=12280ms ownership=8604ms scope_roots=460
[time] produce_mutable_decisions: 21857ms
```

- After block: paste the exact `[time:mutable:artifacts]` and `[time] produce_mutable_decisions` lines from Task 4.
- Correctness block:

```text
target/twk test
```

- [ ] **Step 4: Commit docs and diagnostic cleanup**

```bash
git add boot/compiler/ownership.tw docs/plans/performance/compiler.md
git commit -m "docs: record mutable loop seed performance win"
```

---

## Risks and Stop Rules

- If relevance-closed seeds fail the nested-loop fixtures, stop and inspect which final non-param seeds disappear compared with all-live seeding. Do not re-baseline those tests to persistent fallback.
- If boot tests pass but selected mutable census sites disappear, inspect the affected CFG before accepting the performance win.
- If timing improves only marginally, keep the diagnostics and pivot to the next largest lever: reuse summary analysis results for selected ownership roots instead of analyzing the same large functions twice.
- If seed filtering becomes complex enough to duplicate ownership proof logic, stop. The collector should only choose optimistic candidates; `validate_loop_seed(...)` remains the proof gate.

## Expected Outcome

A successful implementation should reduce `produce_mutable_decisions` substantially by cutting loop-seed validation reruns. The param-only experiment provides an upper-bound signal, but the accepted solution must preserve nested-loop vector set in-place verdicts and pass the boot test suite.
