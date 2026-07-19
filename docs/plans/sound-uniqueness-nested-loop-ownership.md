# Sound Uniqueness: Nested Loop-Carried Ownership Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the boot compiler's CFG ownership analysis render the sound `verdict -> fN[unique:p0]` decision for real AWFY `sieve`, where a vector is carried by an outer loop and mutated through `set_at` in an inner loop.

**Architecture:** The previous sieve gap plan fixed vector update summaries and whole-return moves; straight-line, sequential, and single-loop vector updates now converge. The remaining failure is a CFG fixpoint seeding gap: nested loop headers can start from `Unknown`, make the inner update publish to `Shared`, and feed that pessimism back to the outer loop. Fix this with provisional loop-header ownership assumptions that may bootstrap `Unknown` to `Unique` during iteration, then validate the converged predecessor contributions and retract any assumption that is not proven by all entry and backedge paths. Backedges are classified by predecessor terminator, not SCC membership.

**Tech Stack:** Twinkle boot compiler (`boot/compiler/ownership.tw`), CFG structure (`boot/compiler/cfg.tw`), `target/twk ir <file>.tw --cfg`, boot fixtures under `boot/tests/fixtures/cfg/sound_uniqueness/`, boot test suite `target/twk run boot/tests/main.tw`, opt-in census `cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture`, self-host `make stage2`, and lint `target/twk lint boot/main.tw`.

## Global Constraints

- Treat `docs/plans/archive/sound-uniqueness-sieve-cfg-gap.md` and `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md` as the evidence baseline.
- Do not change Gap A/Gap B summary behavior: `set_at__Bool` must remain `p0=Consumed paths{[]} p1=Borrowed p2=Published ret=alias(p0)`.
- Provisional assumptions may be collected broadly for loop-header block params, but final acceptance requires every predecessor contribution for that param to be `Unique` after convergence.
- Retraction is mandatory: if any predecessor contributes `Shared` or `Unknown` after convergence, remove the assumption, rerun, and render no owned verdict from that assumption.
- `graph_scc.visit` remains out of scope. Its recursive/SCC summary specialization gap is separate from nested loop-header seeding.
- Never use rendered FuncIds, block ids, or local ids in durable test assertions. Assert stable text such as function names, `loop.header`, `terminator: loop-back-edge`, `verdict ->`, `[unique:p0]`, and absence of `: Shared` in scoped fact lines.
- Write CFG dumps under `/tmp/twinkle-cfg-gap/`, not inside the repository.
- After editing `.tw` files, run `target/twk fmt` on changed Twinkle files, `target/twk lint boot/main.tw`, and explicit `target/twk lint` commands for any new fixture entries because `boot/main.tw` does not compile the fixture files.
- Run verification commands one at a time, never concurrently.

---

## File Map

- `boot/compiler/ownership.tw`
  - Add loop-backedge classification helpers for loop-header optimistic seeding.
  - Thread block-edge context into the ownership entry join used during `run_fixpoint`.
  - Keep final materialization in `ownership_stage` non-optimistic so rendered facts are derived from converged exits.
- `boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_set.tw`
  - Minimal positive nested-loop vector fixture without benchmark noise.
- `boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_sieve_shape.tw`
  - Positive nested-loop vector fixture matching real `sieve` control flow more closely.
- `boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_param.tw`
  - Negative nested-loop fixture where the vector comes from a parameter; no owned verdict may render.
- `boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_fresh_alias.tw`
  - Negative nested-loop fixture where a fresh vector is aliased before the nested update; no owned verdict may render.
- `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`
  - Add positive and negative regression tests.
- `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md`
  - Update residual status after the fix lands.

---

### Task 1: Preserve current nested-loop evidence

**Files:**
- Read: `examples/performance/awfy/twinkle/sieve.tw`
- Read: `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md`
- Create: `/tmp/twinkle-cfg-gap/sieve-nested-before.cfg`

**Interfaces:**
- Consumes: current real `sieve` CFG output.
- Produces: stable evidence that the remaining gap is loop nesting, not vector summary or whole-return move.

- [ ] **Step 1: Regenerate the real sieve CFG**

```bash
mkdir -p /tmp/twinkle-cfg-gap
target/twk ir examples/performance/awfy/twinkle/sieve.tw --cfg > /tmp/twinkle-cfg-gap/sieve-nested-before.cfg
```

Expected: exit 0 and the dump is outside the repository.

- [ ] **Step 2: Extract stable evidence**

```bash
rg -n "^fn run|^fn set_at__Bool|summary:|loop\.header|if\.join|facts\.(in|out)=.*(Unknown|Shared|Unique)|terminator: loop-back-edge|verdict ->|unique:" /tmp/twinkle-cfg-gap/sieve-nested-before.cfg
```

Expected today:
- `set_at__Bool` summary is already the fixed target: `p0=Consumed paths{[]} p1=Borrowed p2=Published ret=alias(p0)`.
- No in-loop `verdict -> ...[unique:p0]` renders for real `sieve`.
- Nested loop facts show the inner/nested header entering as `Unknown`, with `Shared` appearing at the inner join/back-edge path.

- [ ] **Step 3: Confirm single-loop coverage still passes**

```bash
target/twk run boot/tests/main.tw 2>&1 | rg -n "loop-carried vector set_at|FAIL|fail|passed"
```

Expected: the existing single-loop fixture test passes. If it fails, stop and fix the regression before working on nested-loop seeding.

- [ ] **Step 4: Commit only if notes changed**

If this task only regenerated `/tmp` evidence, do not commit. If you corrected wording in `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md`, run:

```bash
git status --short
git add docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md
git commit -m "docs: refresh nested sieve ownership evidence"
```

Expected: documentation-only commit.

---

### Task 2: Add nested-loop positive and negative fixtures

**Files:**
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_set.tw`
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_sieve_shape.tw`
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_param.tw`
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes: `render_entry`, `section_between`, and `section_from` helpers in the existing suite.
- Produces: failing positive tests for the nested-loop gap, including a real-sieve-shaped fixture, plus a safety guard that prevents seeding `Unknown` parameter inputs as unique.

- [ ] **Step 1: Add the simple nested-loop positive fixture**

Create `boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_set.tw`:

```tw
pub fn nested_loop_set(n: Int) Int {
  flags: Vector<Bool> = collect _ in range(n) { true }
  i := 0
  for i < n {
    step := i + 1
    k := step
    for k < n {
      if flags[k] {
        flags = .set_at(k, false)
      }
      k = k + step
    }
    i = i + 1
  }
  i
}
```

This keeps the minimal nested-loop shape small: `flags` is fresh before the outer loop, carried by the outer loop, and mutated through `set_at` inside an inner loop.

- [ ] **Step 2: Add the real-sieve-shaped positive fixture**

Create `boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_sieve_shape.tw`:

```tw
pub fn nested_loop_sieve_shape(n: Int) Int {
  flags: Vector<Bool> = collect _ in range(n) { true }
  count := 0
  i := 0
  for i < n {
    if flags[i] {
      count = count + 1
      step := i + 1
      k := i + step
      for k < n {
        flags = .set_at(k, false)
        k = k + step
      }
    }
    i = i + 1
  }
  count
}
```

This matches the real `sieve` control-flow shape more closely than the minimal fixture: outer loop, `if flags[i]`, inner loop under the `if`, and `set_at` inside the inner loop.

- [ ] **Step 3: Add the parameter negative fixture**

Create `boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_param.tw`:

```tw
pub fn nested_loop_param(flags: Vector<Bool>, n: Int) Bool {
  i := 0
  for i < n {
    step := i + 1
    k := step
    for k < n {
      if flags[k] {
        flags = .set_at(k, false)
      }
      k = k + step
    }
    i = i + 1
  }
  flags[0]
}
```

This is the same nested shape but the vector enters from a function parameter. The generic function entry is not proven unique, so no optimistic seed may license an owned verdict.

- [ ] **Step 4: Preflight the current render**

```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_set.tw --cfg | rg -n "^fn nested_loop_set|^fn set_at__Bool|summary:|loop\.header|if\.join|facts\.(in|out)=.*(Unknown|Shared|Unique)|verdict ->|unique:"
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_sieve_shape.tw --cfg | rg -n "^fn nested_loop_sieve_shape|^fn set_at__Bool|summary:|loop\.header|if\.join|facts\.(in|out)=.*(Unknown|Shared|Unique)|verdict ->|unique:"
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_param.tw --cfg | rg -n "^fn nested_loop_param|^fn set_at__Bool|summary:|loop\.header|if\.join|facts\.(in|out)=.*(Unknown|Shared|Unique)|verdict ->|unique:"
```

Expected before the fix:
- `nested_loop_set` does not render the owned in-place verdict inside the nested loop.
- `nested_loop_sieve_shape` does not render the owned in-place verdict inside the nested loop.
- `nested_loop_param` does not render the owned in-place verdict.
- `set_at__Bool` still has the Gap A/Gap B fixed summary.

- [ ] **Step 5: Add tests to the suite**

In `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`, add these tests immediately after the existing single-loop `sieve_loop_set` test:

```tw
    .test(
      "nested loop-carried vector set_at renders an owned in-place verdict",
      fn() {
        out := try render_entry("nested_loop_set")
        nested := try section_between(out, "fn nested_loop_set", "fn set_at__Bool")
        try assert.str_contains(nested, "terminator: loop-back-edge")
        try assert.str_contains(nested, "verdict ->")
        try assert.str_contains(nested, "[unique:p0]")
        for line in nested.lines() {
          if line.contains("facts.in=") or line.contains("facts.out=") {
            try assert.is_false(line.contains(": Shared"))
          }
        }
        .Ok({})
      },
    )
    .test(
      "real-sieve-shaped nested loop renders an owned in-place verdict",
      fn() {
        out := try render_entry("nested_loop_sieve_shape")
        nested := try section_between(out, "fn nested_loop_sieve_shape", "fn set_at__Bool")
        try assert.str_contains(nested, "terminator: loop-back-edge")
        try assert.str_contains(nested, "verdict ->")
        try assert.str_contains(nested, "[unique:p0]")
        for line in nested.lines() {
          if line.contains("facts.in=") or line.contains("facts.out=") {
            try assert.is_false(line.contains(": Shared"))
          }
        }
        .Ok({})
      },
    )
    .test(
      "nested loop parameter vector stays conservative",
      fn() {
        out := try render_entry("nested_loop_param")
        nested := try section_between(out, "fn nested_loop_param", "fn set_at__Bool")
        try assert.str_contains(nested, "terminator: loop-back-edge")
        try assert.is_false(nested.contains("verdict ->"))
        try assert.is_false(nested.contains("[unique:p0]"))
        .Ok({})
      },
    )
```

- [ ] **Step 6: Run the tests and confirm the positive tests fail**

```bash
target/twk run boot/tests/main.tw 2>&1 | rg -n "nested loop-carried vector|real-sieve-shaped nested loop|nested loop parameter|FAIL|fail|passed"
```

Expected before implementation: the positive nested-loop tests fail because no owned verdict renders; the negative test should pass. If the negative test fails by rendering a verdict, stop — the current analysis is already unsound for parameter inputs.

Do not commit the failing tests by themselves; continue to Task 3 and commit fixtures + implementation together after the suite passes.

---

### Task 3: Implement provisional loop-header assumptions with validation and retraction

**Files:**
- Modify: `boot/compiler/ownership.tw`

**Interfaces:**
- Consumes: CFG `preds`, predecessor `terminator` values, and existing `join_entry_ownership` / `run_fixpoint` flow.
- Produces: an outer assume/validate/retract loop around the existing fixpoint. Active assumptions may bootstrap `Unknown` loop-header params to `Unique` during iteration, but final rendered facts are accepted only after every converged predecessor contribution validates as `Unique`.

- [ ] **Step 1: Do not use the plain join as the acceptance gate**

The implementation must not require the plain `join_entry_ownership` result to already be `Unique` before seeding; that is a no-op. Instead:
- collect provisional loop-header param assumptions up front;
- run the normal fixpoint with those assumptions allowed to turn an `Unknown` loop-header param into `Unique`;
- validate the converged result by inspecting every predecessor contribution for each assumed param;
- retract any assumption whose final predecessor contributions are not all `Unique`;
- rerun until no assumptions are retracted.

This makes the assumption capable of bootstrapping a cycle, while validation prevents function parameters, aliases, and genuinely shared paths from being accepted.

- [ ] **Step 2: Add loop-edge and seed-set helpers near the fixpoint helpers**

Add these helpers before `join_entry_ownership`:

```tw
type LoopSeedSet = Dict<String, Bool>

fn loop_seed_key(block_id: Int, local_id: Int) String {
  "${block_id}:${local_id}"
}

fn loop_seed_active(seeds: LoopSeedSet, block_id: Int, local_id: Int) Bool {
  case seeds[loop_seed_key(block_id, local_id)] {
    .Some(v) => v,
    .None => false,
  }
}

fn is_loop_backedge_to_header(blocks: Vector<CfgBlock>, pred_id: Int, header_id: Int) Bool {
  pred := block_by_id(blocks, pred_id)
  case pred.terminator {
    .Some(term) => case term {
      .LoopBackEdge(target, _) => target.id == header_id,
      _ => false,
    },
    .None => false,
  }
}

fn loop_header_has_backedge(blocks: Vector<CfgBlock>, blk: CfgBlock) Bool {
  for pe in blk.preds {
    if is_loop_backedge_to_header(blocks, pe.target.id, blk.id.id) {
      return true
    }
  }
  false
}

type PredParamContribution = .{ own: Ownership, valid: Bool }

fn pred_param_contribution(
  exits: Dict<Int, Dict<Int, Int>>,
  exit_valid: Dict<Int, Dict<Int, Bool>>,
  pe: CfgEdge,
  pidx: Int,
) PredParamContribution {
  pred_exit := own_map_get(exits, pe.target.id)
  pred_valid := valid_map_get(exit_valid, pe.target.id)
  if pidx < pe.args.len() {
    arg := pe.args[pidx]
    ok := case atom_local_id(arg) {
      .Some(id) => valid_of_local(pred_valid, id),
      .None => true,
    }
    PredParamContribution.{ own: fact_of(pred_exit, arg), valid: ok }
  } else {
    PredParamContribution.{ own: .Unknown, valid: false }
  }
}
```

Backedge classification is by predecessor terminator, not SCC membership. In nested loops, an inner-loop entry predecessor and inner-loop backedge can both belong to the same enclosing cyclic region, so SCC membership is not precise enough. Validation tracks binding validity as well as ownership: an invalid/consumed edge argument must never validate a `Unique` seed.

- [ ] **Step 3: Collect provisional loop-header param assumptions**

Add this helper near the seed-set helpers:

```tw
fn collect_loop_seed_candidates(blocks: Vector<CfgBlock>) LoopSeedSet {
  seeds: LoopSeedSet = Dict.new()
  for blk in blocks {
    if blk.name == "loop.header" and loop_header_has_backedge(blocks, blk) {
      for p in blk.params {
        if live_contains_int(blk.entry.live, p.id) {
          seeds[loop_seed_key(blk.id.id, p.id)] = true
        }
      }
    }
  }
  seeds
}
```

This intentionally collects candidates broadly. Soundness comes from validation and retraction, not from trying to prove the candidate up front.

- [ ] **Step 4: Add an assumption-aware ownership entry join**

Keep `join_entry_ownership` as the plain join used for final materialization. Add a wrapper used only inside `run_fixpoint`:

```tw
fn join_entry_ownership_assumed(
  blk: CfgBlock,
  exits: Dict<Int, Dict<Int, Int>>,
  processed: Dict<Int, Bool>,
  seeds: LoopSeedSet,
) Dict<Int, Int> {
  entry := join_entry_ownership(blk, exits, processed)
  for lid in blk.entry.live {
    case param_index(blk, lid) {
      .Some(_) => {
        current := fact_of_local(entry, lid)
        case current {
          .Unknown => if loop_seed_active(seeds, blk.id.id, lid) {
            entry[lid] = own_tag(.Unique)
          },
          _ => {},
        }
      },
      .None => {},
    }
  }
  entry
}
```

This is the actual bootstrap: an active assumption may turn `Unknown` into `Unique`. It must not overwrite `Shared`; if a processed contribution already made the plain join `Shared`, final validation will retract the rejected assumption and rerun cleanly before rendering.

- [ ] **Step 5: Thread assumptions into `run_fixpoint`**

Change `run_fixpoint`'s signature to accept `seeds: LoopSeedSet`:

```tw
fn run_fixpoint(
  blocks: Vector<CfgBlock>,
  params: Vector<LocalId>,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
  unique_seed: Dict<Int, Bool>,
  seeds: LoopSeedSet,
) FixResult {
```

Then replace the iterative ownership entry line:

```tw
      entry_own := join_entry_ownership(blk, exits, processed)
```

with:

```tw
      entry_own := join_entry_ownership_assumed(blk, exits, processed, seeds)
```

Do not change the final entry joins in `ownership_stage`; those must continue to call plain `join_entry_ownership` with `all_processed(blocks)` so rendered facts are based on validated converged exits, not raw assumptions.

- [ ] **Step 6: Centralize destructive-reuse eligibility and include binding validity**

Before adding validation, make every destructive reuse gate require ownership, binding validity, and last-use through one helper. Add these helpers near `own_is_unique` / `is_last_use`:

```tw
fn local_reusable(own: Dict<Int, Int>, valid: Dict<Int, Bool>, id: Int, last: Vector<Int>) Bool {
  own_is_unique(own, id) and valid_of_local(valid, id) and is_last_use(last, id)
}

fn atom_reusable(st: ForwardState, a: Atom, last: Vector<Int>) Bool {
  case atom_local_id(a) {
    .Some(id) => local_reusable(st.own, st.valid, id, last),
    .None => false,
  }
}
```

Then update direct destructive reuse and direct verdict gates:

- `init_hinge`: move only when the source local is `local_reusable(st.own, st.valid, src, last)`. If the source is a local but not reusable, take the alias/publish path (`publish_local(src)` and result `Shared`) rather than moving from an invalid binding. Non-local inputs remain `Unknown`.

- `single_retention`: require `valid_of_local(st.valid, id)` in addition to `Unique`, last-use, and single-store. This keeps stored-element field facts and the Gap A publish gate from treating an invalid consumed local as a clean unique move.

- `consume_base`: require `local_reusable(st.own, st.valid, bid, last)` before setting the result `Unique` and invalidating the base. Otherwise set the result `Unknown`. This covers direct `ARecordUpdate` and COW `.Update` builtins through `consume_call_base`.

```tw
fn consume_base(st: ForwardState, result: Int, base: Atom, last: Vector<Int>) ForwardState {
  case atom_local_id(base) {
    .Some(bid) => if local_reusable(st.own, st.valid, bid, last) {
      st = .set_own_st(result, .Unique)
      st.set_valid(bid, false)
    } else {
      st.set_own_st(result, .Unknown)
    },
    .None => st.set_own_st(result, .Unknown),
  }
}
```

- `shell_verdict`: render `reuse(unique)` only when `local_reusable(st.own, st.valid, bid, last)` is true. If ownership is `Unique` but the local is invalid, render a conservative persistent reason such as `persistent(base consumed)` rather than `reuse(unique)`.

```tw
fn shell_verdict(st: ForwardState, base: Atom, last: Vector<Int>) String {
  case atom_local_id(base) {
    .Some(bid) => case fact_of(st.own, base) {
      .Unique => if local_reusable(st.own, st.valid, bid, last) {
        "reuse(unique)"
      } else if !valid_of_local(st.valid, bid) {
        "persistent(base consumed)"
      } else {
        "persistent(base still live)"
      },
      _ => "persistent(aliased shell)",
    },
    .None => "persistent(aliased shell)",
  }
}
```

- `ARecordGet` field projection move path in `transfer_op`: the destructive whole-record last-use case must use `local_reusable(st.own, st.valid, bid, last)` instead of bare `is_last_use(last, bid)`. Quartet/transport-licensed projection moves must also require `valid_of_local(st.valid, bid)` unless their recognizers are updated in the same task to prove validity explicitly. Do not move field ownership out of an invalid base.

- In `block_verdicts`, any direct field/backing move marker that currently uses `is_last_use(last, bid)` as a destructive move proof must use `local_reusable(pre.own, pre.valid, bid, last)` instead. Keep non-destructive licensed paths (`quartet_has(...)` / `transport_has(...)`) unchanged only if their recognizers already prove validity; otherwise add `valid_of_local(pre.valid, bid)` before rendering a move.

Then tighten summarized user-call unique gates using the same predicate shape. In `transfer_summarized_call`, snapshot `pre_valid := st.valid` next to `pre_own` and update `arg_unique`:

```tw
  pre_own := st.own
  pre_valid := st.valid
  pre_prov := st.prov
  arg_unique: Vector<Bool> = collect a in args {
    case atom_local_id(a) {
      .Some(id) => local_reusable(pre_own, pre_valid, id, last),
      .None => false,
    }
  }
```

Also update the call-verdict recording path in `block_verdicts` where it builds `au` for `select_variant`:

```tw
              au: Vector<Bool> = collect a in args {
                case atom_local_id(a) {
                  .Some(id) => local_reusable(pre.own, pre.valid, id, last),
                  .None => false,
                }
              }
```

- [ ] **Step 7: Add seed validation and retraction**

Add these helpers near `run_fixpoint`:

```tw
fn validate_loop_seed(
  blocks: Vector<CfgBlock>,
  fx: FixResult,
  blk: CfgBlock,
  lid: Int,
) Bool {
  pidx := case param_index(blk, lid) {
    .Some(i) => i,
    .None => return false,
  }

  saw_entry := false
  saw_backedge := false
  for pe in blk.preds {
    if is_loop_backedge_to_header(blocks, pe.target.id, blk.id.id) {
      saw_backedge = true
    } else {
      saw_entry = true
    }
    contributed := pred_param_contribution(fx.exits, fx.exit_valid, pe, pidx)
    if !contributed.valid {
      return false
    }
    case contributed.own {
      .Unique => {},
      _ => return false,
    }
  }

  saw_entry and saw_backedge
}

fn retain_valid_loop_seeds(blocks: Vector<CfgBlock>, fx: FixResult, seeds: LoopSeedSet) LoopSeedSet {
  kept: LoopSeedSet = Dict.new()
  for blk in blocks {
    for p in blk.params {
      if loop_seed_active(seeds, blk.id.id, p.id) and validate_loop_seed(blocks, fx, blk, p.id) {
        kept[loop_seed_key(blk.id.id, p.id)] = true
      }
    }
  }
  kept
}

fn same_loop_seed_set(a: LoopSeedSet, b: LoopSeedSet) Bool {
  if a.keys().len() != b.keys().len() {
    return false
  }
  for k in a.keys() {
    case a[k] {
      .Some(av) => case b[k] {
        .Some(bv) => if av != bv {
          return false
        },
        .None => return false,
      },
      .None => {},
    }
  }
  true
}
```

Validation inspects every predecessor contribution after convergence. Function-parameter entry edges, alias-created `Shared` edges, invalid/consumed edge arguments, and any backedge that still contributes `Unknown` or `Shared` retract the assumption.

- [ ] **Step 8: Run the outer assume/validate/retract loop in `ownership_stage`**

In `ownership_stage`, replace the direct call to `run_fixpoint(...)` with an outer loop:

```tw
  seeds := collect_loop_seed_candidates(blocks)
  fx := run_fixpoint(blocks, params, table, b, sem, no_suppress, Dict.new(), seeds)
  stable_seeds := false
  for !stable_seeds {
    kept := retain_valid_loop_seeds(blocks, fx, seeds)
    if same_loop_seed_set(kept, seeds) {
      stable_seeds = true
    } else {
      seeds = kept
      fx = run_fixpoint(blocks, params, table, b, sem, no_suppress, Dict.new(), seeds)
    }
  }
```

The loop terminates because each failed validation only removes seeds. Never add new seeds after the initial candidate collection.

Then update the other `run_fixpoint` call in `summarize_seeded` to pass an empty seed set, keeping function summaries conservative until a separate summary-specialization plan proves optimistic summary assumptions sound:

```tw
  empty_seeds: LoopSeedSet = Dict.new()
  fx := run_fixpoint(blocks, f.params, table, b, sem, suppress, unique_seed, empty_seeds)
```

- [ ] **Step 9: Rebuild the boot payload and CLI, then run focused tests**

Because `boot/compiler/ownership.tw` changes the self-hosted compiler payload, do not use `make quick-bundle-cli` alone. Run:

```bash
make bundle-cli
target/twk run boot/tests/main.tw 2>&1 | rg -n "nested loop-carried vector|real-sieve-shaped nested loop|nested loop parameter|loop-carried vector set_at|vector set_at return aliases|FAIL|fail|passed"
```

Expected:
- Positive nested-loop fixtures now pass, including the real-sieve-shaped fixture.
- Negative parameter fixture still passes with no owned verdict.
- Existing single-loop and vector summary tests still pass.

- [ ] **Step 10: Inspect real sieve with the rebuilt CLI**

```bash
target/twk ir examples/performance/awfy/twinkle/sieve.tw --cfg > /tmp/twinkle-cfg-gap/sieve-nested-after.cfg
rg -n "^fn run|^fn set_at__Bool|summary:|loop\.header|if\.join|facts\.(in|out)=.*(Unknown|Shared|Unique)|terminator: loop-back-edge|verdict ->|unique:" /tmp/twinkle-cfg-gap/sieve-nested-after.cfg
```

Expected:
- Real `sieve` renders an in-loop `verdict -> ...[unique:p0]` for the `set_at` call.
- The scoped `run` facts for the carried vector no longer show `: Shared` along the nested loop path.
- `set_at__Bool` summary remains `p0=Consumed paths{[]} p1=Borrowed p2=Published ret=alias(p0)`.

If real `sieve` still fails while the fixture passes, add a smaller fixture for the missing shape before changing the algorithm.

- [ ] **Step 11: Format and commit**

```bash
target/twk fmt \
  boot/compiler/ownership.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_set.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_sieve_shape.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_param.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git status --short
git add \
  boot/compiler/ownership.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_set.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_sieve_shape.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_param.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git commit -m "ownership: validate optimistic nested loop-carried facts"
```

Expected: implementation and fixture changes only.

---

### Task 4: Add explicit alias safety regression coverage

**Files:**
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_fresh_alias.tw`
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes: Task 3's optimistic seeding implementation.
- Produces: regression coverage that retraction works when an observable alias remains live across the inner-loop backedge.

- [ ] **Step 1: Add an aliasing negative fixture**

Create `boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_fresh_alias.tw`:

```tw
pub fn nested_loop_fresh_alias(n: Int) Bool {
  flags: Vector<Bool> = collect _ in range(n) { true }
  i := 0
  for i < n {
    alias := flags
    step := i + 1
    k := step
    for k < n {
      if alias[k] {
        flags = .set_at(k, false)
      }
      k = k + step
    }
    i = i + 1
  }
  flags[0]
}
```

The vector starts fresh, so broad provisional seed collection will consider it. The `alias` is created before the inner loop and read inside the inner loop on each iteration, keeping an observable alias across the inner-loop backedge; validation must retract the seed and keep the nested update conservative.

- [ ] **Step 2: Add the negative assertion**

Add this test after `nested loop parameter vector stays conservative`:

```tw
    .test(
      "nested loop fresh aliased vector stays conservative",
      fn() {
        out := try render_entry("nested_loop_fresh_alias")
        nested := try section_between(out, "fn nested_loop_fresh_alias", "fn set_at__Bool")
        try assert.str_contains(nested, "terminator: loop-back-edge")
        try assert.is_false(nested.contains("verdict ->"))
        try assert.is_false(nested.contains("[unique:p0]"))
        .Ok({})
      },
    )
```

- [ ] **Step 3: Run focused suite output**

```bash
target/twk run boot/tests/main.tw 2>&1 | rg -n "nested loop.*conservative|nested loop fresh aliased|nested loop-carried vector|real-sieve-shaped nested loop|FAIL|fail|passed"
```

Expected: all nested-loop tests pass.

- [ ] **Step 4: Format and commit**

```bash
target/twk fmt boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_fresh_alias.tw boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git status --short
git add boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_fresh_alias.tw boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git commit -m "test: guard nested loop uniqueness seeding against fresh aliases"
```

Expected: the alias fixture and suite update are committed; `git status --short` no longer shows those files.

---

### Task 5: Update docs and final verification

**Files:**
- Modify: `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md`
- Modify when needed: `docs/plans/sound-uniqueness/analysis/README.md`
- Any files changed by Tasks 2-4

**Interfaces:**
- Consumes: passing nested-loop implementation and real-sieve CFG evidence.
- Produces: documentation that the nested-loop residual is closed, plus final validation results.

- [ ] **Step 1: Update the residual notes**

In `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md`, replace the `## residual loop-carried merge gap (NESTED loops) — follow-up` section with:

```markdown
## nested loop-carried merge gap (closed)

The real `examples/performance/awfy/twinkle/sieve.tw` now renders the in-loop
`verdict -> ...[unique:p0]` for the inner `set_at` call. The fix is conservative
provisional loop-header seeding: loop-carried block params may start as `Unique`
during iteration, but an assumption is kept only when every entry and backedge
predecessor contribution validates as both `Unique` and binding-valid after convergence.
Function-parameter and fresh-alias nested-loop fixtures stay conservative.
```

- [ ] **Step 2: Update the analysis README deferral**

If `docs/plans/sound-uniqueness/analysis/README.md` still lists nested loop-carried ownership as active, change it to a closed note or remove it from the active deferral table. Keep `graph_scc.visit` as a separate follow-up.

- [ ] **Step 3: Run final verification one command at a time**

```bash
target/twk run boot/tests/main.tw
cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture
make stage2
target/twk lint boot/main.tw
target/twk lint boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_set.tw
target/twk lint boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_sieve_shape.tw
target/twk lint boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_param.tw
target/twk lint boot/tests/fixtures/cfg/sound_uniqueness/nested_loop_fresh_alias.tw
```

Expected:
- Boot suite passes.
- Census stays within the committed ceiling; investigate any increase before changing the baseline.
- Self-host reaches a fixed point.
- Lint reports no new issues in files changed by this plan. `boot/main.tw` may still report pre-existing `variant_id.tw` inherent-call findings; record those as pre-existing unless this plan touched that file. The explicit fixture lint commands must be clean.

- [ ] **Step 4: Confirm no stray artifacts**

```bash
git status --short
find . -path './.git' -prune -o -name '*.cfg' -print
ls /tmp/twinkle-cfg-gap 2>/dev/null
```

Expected: no CFG dumps inside the repository; `/tmp/twinkle-cfg-gap` contains the temporary dumps.

- [ ] **Step 5: Commit docs and report**

```bash
git status --short
git add -u docs/plans
git add \
  docs/plans/README.md \
  docs/plans/archive/README.md \
  docs/plans/archive/sound-uniqueness-sieve-cfg-gap.md \
  docs/plans/sound-uniqueness-nested-loop-ownership.md \
  docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md \
  docs/plans/sound-uniqueness/analysis/README.md
git commit -m "docs: close nested sieve ownership gap"
```

Expected: documentation-only commit if implementation/test commits were already made. Include the archive/index files if they were not already committed before executing this implementation plan.

Final report should state:
- whether real `sieve` now renders `verdict -> ...[unique:p0]`;
- whether parameter/alias nested-loop guards stayed conservative;
- boot suite, census, self-host, and lint outcomes;
- any residual risk or deferred work, especially `graph_scc.visit` if still conservative.
