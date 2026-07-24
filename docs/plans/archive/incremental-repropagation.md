# Incremental Re-Propagation for Loop-Seed Validation Reruns

> **OUTCOME (2026-07-24): SHIPPED.** Both tasks landed and are committed on branch
> `ownership-fixresult-reuse`. SEEDVERIFY was clean across the self-build and the full
> boot suite (3226 tests) on the first attempt, so the Task 3 dirty-subgraph widening
> reset was **not needed**. Same-session A/B (cache-active): the summary stage ~14.2s → ~7.0s
> and `produce_mutable_decisions` ~15.85s → ~8.6s (~50% / ~46%), with the dominant
> `summary:link` rerun set dropping ~3443ms → ~660ms (incremental, no cold fallback).
> Output byte-identical to `main`; FIXVERIFY clean; self-host stable. Results recorded in
> `docs/plans/performance/compiler.md` → "loop-seed rerun incremental re-propagation (shipped)".

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cut the loop-seed-validation rerun cost in `run_fixpoint_validated` by re-propagating only the blocks affected by a seed removal, instead of re-running a full cold fixpoint each rerun — including the dominant `summary:link` function, which warm-start-by-restart structurally could not help.

**Architecture:** Keep pass 1 a full-sweep cold fixpoint (byte-identical). Carry the *entire* solver state (exit maps **and** widening state) across reruns and drive each rerun as a **dirty-gated worklist**: seed the worklist with only the blocks whose optimistic loop-seed was removed, process a block only when it is dirty, and dirty its successors when its exit changes. The returned `FixResult` is still a fresh cold final pass, and seed-set equivalence vs an all-cold reference is enforced by the existing `TWINKLE_SEEDVERIFY` validator.

**Tech Stack:** Twinkle boot compiler (`boot/`). Sole file: `boot/compiler/ownership.tw`. CLI: `target/twk`, `deno` runtime harness.

---

## Spec

### Why the prior lever (warm-start-by-restart) failed, and why this is different

The archived plan `docs/plans/archive/fixpoint-rerun-cost.md` warm-started each rerun by calling a fresh `run_fixpoint` with the prior exit maps but **reset widening counters** and `processed=true`. That was byte-identical and SEEDVERIFY-clean but a **net regression** (`docs/plans/performance/compiler.md` → "loop-seed rerun warm-start (null result)"):

- It gated on `!widened`, so the dominant function (`summary:link`, 243 blocks, ~29% of the summary stage) **fell back to cold** — warm-start never touched the cost that matters.
- Even the functions that did warm re-swept **every** block each round (round 1 ran the full merge over already-large maps), trading cheap-early-rounds for expensive-early-rounds. Net wash-to-slightly-worse.

The realized cost model: a full sweep visits all blocks every round regardless of what changed. **The win requires touching fewer blocks, not fewer rounds.** That is what this plan does — a sparse (dirty-gated) traversal — and it does **not** gate on widening, so it can attack `summary:link`.

### How a loop seed enters the dataflow (verified in code)

`join_entry_ownership_assumed` (`ownership.tw:2329`) applies seeds to **one block only** — the block itself:

```tw
entry := join_entry_ownership(blk, exits, processed)
for lid in blk.entry.live {
  case fact_of_local(entry, lid) {
    .Unknown => if loop_seed_active(seeds, blk.id.id, lid) {
      entry[lid] = own_tag(.Unique)   // optimistic seed, block-local
    },
    _ => {},
  }
}
```

A seed keyed `(blk_id, lid)` (see `loop_seed_key`, `ownership.tw:2220`) therefore affects **only** `blk_id`'s entry. Reruns only ever **remove** seeds (`retain_valid_loop_seeds` returns `kept ⊆ seeds`, `ownership.tw:2995`). So the exact set of blocks whose entry assumption changes on a rerun is:

> `removed_seed_blocks = { blk.id : some (blk.id, lid) was active in `seeds` but not in `kept` }`

That set is the **initial dirty set**. Removing a seed lowers that block's entry (`Unique → Unknown`), which may lower its exit, which must propagate forward to successors — the worklist frontier.

### The design

1. **Pass 1 stays a full-sweep cold fixpoint.** The existing `for changed { for blk in blocks { … } }` chaotic iteration is preserved exactly (same block order, same widening visit-counts), so the pass-1 fixpoint — and therefore the emitted program — is byte-identical. The dirty machinery is present but inert when `dirty0 = .None` (an "all-dirty" sentinel: every block is processed every round, exactly as today).

2. **Reruns carry the *whole* solver state and go sparse.** `run_fixpoint` gains a `warm_state: FixState?` (the 14 carry-over dicts: five exit maps + `prev_*` + `locked_*` + `prev_seen` + `changed_visits` + `processed`) and a `dirty0: Dict<Int, Bool>?`. When both are `.Some`, it does **not** re-initialize; it continues from the prior fixpoint and processes a block in a round **only if it is dirty**, dirtying a block's successors when its exit changes. Because it never restarts and never resets `processed`/widening, a re-visited block is legitimately `already` — matching what a *continued* cold iteration does — so the spurious first-visit meet that would corrupt warm-start-by-restart cannot occur.

3. **The returned `FixResult` is always a fresh cold pass.** As before, the incremental result is consumed **only** by `retain_valid_loop_seeds` (which reads only `fx.exits` + `fx.exit_valid`, `validate_loop_seed` at `ownership.tw:2971`). Once the seed set stabilizes, one **cold** `run_fixpoint(stable_seeds, .None, .None)` produces the returned/cached value. A re-propagation bug can only change *which seeds stabilize* — never taint the returned fields.

4. **`TWINKLE_SEEDVERIFY` is the acceptance gate (reused verbatim).** The committed A/B in `run_fixpoint_validated` already stabilizes the seed set once all-cold and once via the accelerated path and traps `error("seedverify mismatch: <label>")` on any per-function divergence. This plan simply repoints the accelerated path from warm-start to incremental. Acceptance is **zero mismatches** across the full self-build + boot suite. It compares the *old algorithm against the new one*, keyed by function.

### The central correctness risk (this is the GO/NO-GO)

Incremental re-propagation carries the prior **widening state** (`locked_*`, `changed_visits`, `prev_*`). If, during pass 1, a block widened and **locked** the seeded local to `Unique` (`merge_targeted` returns `locked` keys, `ownership.tw:3251`), then re-propagating with the seed removed may keep that local `Unique` via the carried lock — so the seed looks still-valid when a cold restart (which never had that lock) would drop it. That is a genuine **seed-set divergence on exactly the widening functions we care about** (e.g. `summary:link`).

This is empirical, not proven. **Task 1 Step 7 (SEEDVERIFY over the self-build) is the go/no-go for the whole approach:**
- **Clean** → incremental is accepted; proceed to Task 2.
- **Traps on a handful** → add a per-function exclusion that forces `allow_incremental = false` for them, record it, proceed.
- **Traps broadly on widening functions** → the carried-lock hazard is real and dominant. Do **not** ship as-is; attempt the **Task 3 fallback** (reset the widening lock/counter state on the dirty-reachable subgraph before re-propagating, so that region re-widens from scratch while the rest is reused). If the fallback also traps broadly, **stop and record a second null result** — do not weaken the validator.

### Acceptance bar

Validator-gated, empirical: an accepted optimization produces **byte-identical per-function stabilized seed sets** vs the all-cold reference (`TWINKLE_SEEDVERIFY` across self-build + boot suite), then **byte-identical emitted program** (fixed input, before/after compiler), `TWINKLE_FIXVERIFY` clean, and self-host stable. Not a formal proof; the validator is the standing check.

---

## Global Constraints

- Boot-compiler-only: the emitted program must be **byte-identical**. No stage0 (`src/`) changes.
- After editing `.tw`, run `target/twk fmt <file>` and `target/twk lint boot/main.tw` (must report `No findings.`).
- Gating shell commands must **not mask failure**: check exit status explicitly and put `set -o pipefail` at the top of any block whose success is read through a pipe. **Note:** this project's shell treats `status` as read-only — name your own capture variable `rc`, not `status`.
- Heavy verification (full suite, builds, timed/`SEEDVERIFY` runs) runs **one at a time**, never concurrently or backgrounded.
- This plan builds on branch `ownership-fixresult-reuse` at the warm-start commit (`87837efd` + docs). It **replaces** the warm-start-by-restart path with incremental re-propagation (removing the `warm: FixResult?` reset-widening machinery), reusing the committed `seedverify_enabled()` / `SeedRun` / A/B harness.

## Baseline Evidence (cache-active branch compiler, this branch)

```text
[time:mutable:artifacts] summary~11.9s   [time] produce_mutable_decisions ~13.5s
[time:own:fixpoint_validated] func=summary:link total~3.4s reruns=14 blocks=243   (dominant; widens → warm-start could not help)
```

Confirmed in code:
- A loop seed `(blk_id, lid)` affects only `blk_id`'s entry (`join_entry_ownership_assumed`, `ownership.tw:2329`); reruns only remove seeds (`retain_valid_loop_seeds`, `:2995`).
- `validate_loop_seed` reads only `fx.exits` + `fx.exit_valid` (`:2971`) — the cold-final-pass safety argument holds unchanged.
- `run_fixpoint` (committed warm-start form) takes `warm: FixResult?`, returns `FixRun`, and has exactly three call sites, all inside `stabilize_seeds` (`:3336`, `:3353`) and `run_fixpoint_validated` (final, `:3399`).
- Widening locks are produced by `merge_targeted` and stored per-block in `locked_own/valid/prov` (`:3251`), consumed via `locked_get` (`:2451`).
- Predecessor edges: `blk.preds`, where `pe.target.id` is the **predecessor** id (`:2304` comment). A successor map is the transpose.

## File Structure

- Modify `boot/compiler/ownership.tw` only:
  - New `type FixState` (the 14 carry-over dicts); `FixRun` gains a `state: FixState` field.
  - New helpers: `is_dirty`, `succ_get`, `build_succ_map`, `removed_seed_blocks`.
  - `run_fixpoint`: `warm: FixResult?` → `warm_state: FixState?`; add `dirty0: Dict<Int, Bool>?`; init from `warm_state` when present; dirty-gate the per-block body; dirty successors on change; return `state` alongside `fx`.
  - `stabilize_seeds`: `allow_warm` → `allow_incremental`; reruns carry the prior `FixState` + `removed_seed_blocks` dirty set.
  - `run_fixpoint_validated`: repoint the A/B accelerated path (rename `warmed` diagnostic → `incremental`); cold final pass unchanged.

---

### Task 1 — Land incremental machinery + revalidate (production stays cold, byte-identical)

Incremental code lands and is exercised **only by the `TWINKLE_SEEDVERIFY` validator**; the production path stays all-cold (`allow_incremental = false`), so this task is byte-identical. The validator proves incremental ≡ cold seed sets before Task 2 depends on it.

**Files:**
- Modify: `boot/compiler/ownership.tw`

- [ ] **Step 1: Add `FixState`, extend `FixRun`, add the small helpers**

Replace the committed `FixRun` type (just above `SeedRun`, near `type FixResult`) with the `FixState` + extended `FixRun`:

```tw
// Full inner-fixpoint solver state: the five dataflow exit maps plus the widening
// state (prev_*/locked_*/prev_seen/changed_visits) and per-block processed flags.
// Carried unchanged across seed-finding reruns so a rerun continues the prior
// fixpoint instead of restarting cold.
type FixState = .{
  exits: Dict<Int, Dict<Int, Int>>,
  exit_valid: Dict<Int, Dict<Int, Bool>>,
  exit_prov: Dict<Int, Dict<Int, Vector<Int>>>,
  exit_field_own: Dict<Int, Dict<Int, ff.FieldMap>>,
  exit_path_prov: Dict<Int, Dict<Int, Dict<Int, Vector<Int>>>>,
  prev_exits: Dict<Int, Dict<Int, Int>>,
  prev_exit_valid: Dict<Int, Dict<Int, Bool>>,
  prev_exit_prov: Dict<Int, Dict<Int, Vector<Int>>>,
  locked_own: Dict<Int, Vector<Int>>,
  locked_valid: Dict<Int, Vector<Int>>,
  locked_prov: Dict<Int, Vector<Int>>,
  prev_seen: Dict<Int, Bool>,
  changed_visits: Dict<Int, Int>,
  processed: Dict<Int, Bool>,
}

// Inner fixpoint result: the projected FixResult, the full carry-over state (for
// the next incremental rerun), and whether widening (force-lock) fired.
type FixRun = .{ fx: FixResult, state: FixState, widened: Bool }
```

Add these helpers next to `is_processed` / `nested_get` (top-level, anywhere above `run_fixpoint`):

```tw
fn is_dirty(dirty: Dict<Int, Bool>, id: Int) Bool {
  case dirty.get(id) {
    .Some(v) => v,
    .None => false,
  }
}

fn succ_get(succ: Dict<Int, Vector<Int>>, id: Int) Vector<Int> {
  case succ.get(id) {
    .Some(v) => v,
    .None => [],
  }
}

// Transpose of blk.preds: pred_id -> [successor block ids]. Used to push the
// dirty frontier forward during incremental re-propagation.
fn build_succ_map(blocks: Vector<CfgBlock>) Dict<Int, Vector<Int>> {
  succ: Dict<Int, Vector<Int>> = Dict.new()
  for blk in blocks {
    for pe in blk.preds {
      succ[pe.target.id] = succ_get(succ, pe.target.id).append(blk.id.id)
    }
  }
  succ
}

// Blocks whose optimistic loop-seed was dropped this rerun (seeds -> kept).
// Exactly the blocks whose entry assumption changes, i.e. the initial dirty set.
fn removed_seed_blocks(
  blocks: Vector<CfgBlock>,
  seeds: LoopSeedSet,
  kept: LoopSeedSet,
) Dict<Int, Bool> {
  dirty: Dict<Int, Bool> = Dict.new()
  for blk in blocks {
    for lid in blk.entry.live {
      if loop_seed_active(seeds, blk.id.id, lid) and !loop_seed_active(kept, blk.id.id, lid) {
        dirty[blk.id.id] = true
      }
    }
  }
  dirty
}
```

- [ ] **Step 2: `run_fixpoint` — swap `warm` for `warm_state` + `dirty0`, init from carried state**

Change the signature (the two new params are trailing; drop the old `warm`):

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
  label: String,
  warm_state: FixState?,
  dirty0: Dict<Int, Bool>?,
) FixRun {
```

Replace the committed warm-aware init block (from `// Dataflow exit maps seed from ` warm `…` through the per-block `for blk in blocks { … }` loop, `ownership.tw:3086-3138`) with a full-state-aware version. When `warm_state` is present, load **all 14** dicts from it and do **not** re-init any block; otherwise init fresh exactly as the original cold path did:

```tw
  warm_started := case warm_state {
    .Some(_) => true,
    .None => false,
  }
  exits: Dict<Int, Dict<Int, Int>> = case warm_state {
    .Some(w) => w.exits,
    .None => Dict.new(),
  }
  exit_valid: Dict<Int, Dict<Int, Bool>> = case warm_state {
    .Some(w) => w.exit_valid,
    .None => Dict.new(),
  }
  exit_prov: Dict<Int, Dict<Int, Vector<Int>>> = case warm_state {
    .Some(w) => w.exit_prov,
    .None => Dict.new(),
  }
  exit_field_own: Dict<Int, Dict<Int, ff.FieldMap>> = case warm_state {
    .Some(w) => w.exit_field_own,
    .None => Dict.new(),
  }
  exit_path_prov: Dict<Int, Dict<Int, Dict<Int, Vector<Int>>>> = case warm_state {
    .Some(w) => w.exit_path_prov,
    .None => Dict.new(),
  }
  prev_exits: Dict<Int, Dict<Int, Int>> = case warm_state {
    .Some(w) => w.prev_exits,
    .None => Dict.new(),
  }
  prev_exit_valid: Dict<Int, Dict<Int, Bool>> = case warm_state {
    .Some(w) => w.prev_exit_valid,
    .None => Dict.new(),
  }
  prev_exit_prov: Dict<Int, Dict<Int, Vector<Int>>> = case warm_state {
    .Some(w) => w.prev_exit_prov,
    .None => Dict.new(),
  }
  locked_own: Dict<Int, Vector<Int>> = case warm_state {
    .Some(w) => w.locked_own,
    .None => Dict.new(),
  }
  locked_valid: Dict<Int, Vector<Int>> = case warm_state {
    .Some(w) => w.locked_valid,
    .None => Dict.new(),
  }
  locked_prov: Dict<Int, Vector<Int>> = case warm_state {
    .Some(w) => w.locked_prov,
    .None => Dict.new(),
  }
  prev_seen: Dict<Int, Bool> = case warm_state {
    .Some(w) => w.prev_seen,
    .None => Dict.new(),
  }
  changed_visits: Dict<Int, Int> = case warm_state {
    .Some(w) => w.changed_visits,
    .None => Dict.new(),
  }
  processed: Dict<Int, Bool> = case warm_state {
    .Some(w) => w.processed,
    .None => Dict.new(),
  }
  if !warm_started {
    for blk in blocks {
      exits[blk.id.id] = Dict.new()
      exit_valid[blk.id.id] = Dict.new()
      exit_prov[blk.id.id] = Dict.new()
      exit_field_own[blk.id.id] = Dict.new()
      exit_path_prov[blk.id.id] = Dict.new()
      prev_exits[blk.id.id] = Dict.new()
      prev_exit_valid[blk.id.id] = Dict.new()
      prev_exit_prov[blk.id.id] = Dict.new()
      locked_own[blk.id.id] = []
      locked_valid[blk.id.id] = []
      locked_prov[blk.id.id] = []
      prev_seen[blk.id.id] = false
      changed_visits[blk.id.id] = 0
      processed[blk.id.id] = false
    }
  }
  succ := build_succ_map(blocks)
  all_dirty := case dirty0 {
    .None => true,
    .Some(_) => false,
  }
  dirty: Dict<Int, Bool> = case dirty0 {
    .Some(d) => d,
    .None => Dict.new(),
  }
```

> Note: with `warm_state = .None` and `dirty0 = .None` (the cold path), this is behaviorally identical to the original: all 14 maps fresh, `processed=false`, `all_dirty=true`. The `succ`/`dirty` locals are inert in that mode.

- [ ] **Step 3: `run_fixpoint` — dirty-gate the per-block body; dirty successors on change**

Locate the inner `for blk in blocks {` loop body (`ownership.tw:3160` onward, beginning `if timed { block_visits = block_visits + 1 }`). Wrap the **entire** body — from the `entry_own := join_entry_ownership_assumed(...)` line through the final `processed[blk.id.id] = true` — in a dirty gate. Concretely, replace the loop header + first line:

```tw
    for blk in blocks {
      if timed {
        block_visits = block_visits + 1
      }
      entry_own := join_entry_ownership_assumed(blk, exits, processed, seeds)
```

with:

```tw
    for blk in blocks {
      do_block := all_dirty or is_dirty(dirty, blk.id.id)
      if do_block {
        dirty[blk.id.id] = false
        if timed {
          block_visits = block_visits + 1
        }
        entry_own := join_entry_ownership_assumed(blk, exits, processed, seeds)
```

Add one closing `}` for the new `if do_block {` at the end of the loop body — the existing `processed[blk.id.id] = true` is now the last statement inside the gate, so change:

```tw
      processed[blk.id.id] = true
    }
  }
```

to:

```tw
        processed[blk.id.id] = true
      }
    }
  }
```

> Everything between (entry joins, `forward_block`, the `if already { … merge … }` block, the `if timed { max_* }` block, and the exit-changed `if !already or !same_map(...) { … }` block) is **moved verbatim, indented one level deeper**. Do not otherwise alter it.

Then, inside the exit-changed branch, push the dirty frontier. Find the block that sets `changed = true` and writes the new exits (`ownership.tw:3278-3300`, ending with `exit_path_prov[blk.id.id] = next_pp`). Immediately **after** `exit_path_prov[blk.id.id] = next_pp` (still inside that `if`), add:

```tw
        if !all_dirty {
          for s in succ_get(succ, blk.id.id) {
            dirty[s] = true
          }
        }
```

- [ ] **Step 4: `run_fixpoint` — return `state` alongside `fx`**

Change the return (`ownership.tw:3316`, currently `FixRun.{ fx: FixResult.{ … }, widened }`) to:

```tw
  final_state := FixState.{
    exits,
    exit_valid,
    exit_prov,
    exit_field_own,
    exit_path_prov,
    prev_exits,
    prev_exit_valid,
    prev_exit_prov,
    locked_own,
    locked_valid,
    locked_prov,
    prev_seen,
    changed_visits,
    processed,
  }
  FixRun.{
    fx: FixResult.{ exits, exit_valid, exit_prov, exit_field_own, exit_path_prov },
    state: final_state,
    widened,
  }
```

- [ ] **Step 5: `stabilize_seeds` — `allow_incremental`, carry state + dirty set**

Replace the committed `stabilize_seeds` (the `allow_warm` version) with:

```tw
// Find the stable loop-seed set: pass 1 cold, then reruns until the seed set
// stops shrinking. When `allow_incremental`, each rerun continues the prior
// fixpoint (full FixState carried) and re-propagates only from the blocks whose
// seed was dropped (dirty worklist); otherwise each rerun is a cold restart. The
// returned seeds drive the final (cold) fixpoint in run_fixpoint_validated.
fn stabilize_seeds(
  blocks: Vector<CfgBlock>,
  params: Vector<LocalId>,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
  unique_seed: Dict<Int, Bool>,
  label: String,
  allow_incremental: Bool,
) SeedRun {
  seeds := collect_loop_seed_candidates(blocks)
  first := run_fixpoint(blocks, params, table, b, sem, suppress, unique_seed, seeds, label, .None, .None)
  vfx := first.fx
  vstate := first.state
  reruns := 0
  stable := false
  for !stable {
    kept := retain_valid_loop_seeds(blocks, vfx, seeds)
    if same_loop_seed_set(kept, seeds) {
      stable = true
    } else {
      dirty := removed_seed_blocks(blocks, seeds, kept)
      seeds = kept
      reruns = reruns + 1
      warm_state: FixState? = if allow_incremental {
        .Some(vstate)
      } else {
        .None
      }
      dirty0: Dict<Int, Bool>? = if allow_incremental {
        .Some(dirty)
      } else {
        .None
      }
      rr := run_fixpoint(
        blocks,
        params,
        table,
        b,
        sem,
        suppress,
        unique_seed,
        seeds,
        label,
        warm_state,
        dirty0,
      )
      vfx = rr.fx
      vstate = rr.state
    }
  }
  SeedRun.{ seeds, reruns, warmed: allow_incremental and reruns > 0 }
}
```

> `SeedRun.warmed` keeps its name (no type churn); it now means "incremental was used". The diagnostic label is updated in the next step.

- [ ] **Step 6: `run_fixpoint_validated` — repoint the A/B, cold final pass**

In `run_fixpoint_validated`, update the comment + the A/B (the `sr := if seedverify_enabled() { … }` block) and the final cold call. The SEEDVERIFY branch keeps `false` for the reference and now uses `true` = incremental; production stays `false` **this task**:

```tw
  // TASK 1: production is all-cold (allow_incremental = false) — byte-identical.
  // The validator exercises incremental re-propagation and proves seed-set
  // equivalence. TASK 2 flips the production allow_incremental to true.
  sr := if seedverify_enabled() {
    cold := stabilize_seeds(blocks, params, table, b, sem, suppress, unique_seed, label, false)
    incr := stabilize_seeds(blocks, params, table, b, sem, suppress, unique_seed, label, true)
    if !same_loop_seed_set(cold.seeds, incr.seeds) {
      error("seedverify mismatch: ${label}")
    }
    cold
  } else {
    stabilize_seeds(blocks, params, table, b, sem, suppress, unique_seed, label, false)
  }
```

The final cold pass already reads `.None` for warm; add the second `.None` for `dirty0`:

```tw
  fx := run_fixpoint(blocks, params, table, b, sem, suppress, unique_seed, seeds, label, .None, .None).fx
```

Rename the diagnostic field `warmed=${warmed}` → `incremental=${warmed}` in the `[time:own:fixpoint_validated]` `eprintln` (the value binding `warmed := sr.warmed` stays).

- [ ] **Step 7: Format, lint, build**

```bash
set -o pipefail
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
if target/twk build boot/main.tw -o /tmp/ir-t1.wasm; then echo "BUILD OK"; else echo "BUILD FAIL"; exit 1; fi
```

Expected: fmt/lint clean (`No findings.`); `BUILD OK`.

- [ ] **Step 8: Byte-identical output (production still cold — must match `main`)**

`core_lib.tw` is generated + gitignored; copy it into the baseline worktree.

```bash
set -o pipefail
git worktree add /tmp/ir-baseline main
cp boot/lib/module/core_lib.tw /tmp/ir-baseline/boot/lib/module/core_lib.tw
target/twk build /tmp/ir-baseline/boot/main.tw -o /tmp/ir-before.wasm

BOOT_WASM=/tmp/ir-before.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/ir-baseline/boot/main.tw -o /tmp/ir-out-before.wasm
BOOT_WASM=/tmp/ir-t1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/ir-baseline/boot/main.tw -o /tmp/ir-out-t1.wasm
if cmp /tmp/ir-out-before.wasm /tmp/ir-out-t1.wasm; then echo "BYTE-IDENTICAL"; else echo "DIFFER"; exit 1; fi
```

Expected: `BYTE-IDENTICAL` (production is unchanged all-cold; the refactor must not alter behavior). Keep `/tmp/ir-baseline` and `/tmp/ir-before.wasm` for later tasks.

> If this fails, the refactor changed pass-1 behavior — most likely the dirty gate is not truly inert when `all_dirty` (e.g. `block_visits` moved but a state write got left out, or the wrap mis-indented a statement out of the loop). Diff against `main`'s `run_fixpoint` body; do not proceed.

- [ ] **Step 9: SEEDVERIFY across the self-build — GO/NO-GO for the whole approach**

This step decides whether incremental re-propagation is viable (see spec §"The central correctness risk").

```bash
set -o pipefail
TWINKLE_SEEDVERIFY=1 BOOT_WASM=/tmp/ir-t1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/ir-sv.wasm >/tmp/ir-sv.out 2>&1
rc=$?
if [ $rc -eq 0 ] && ! grep -q "seedverify mismatch" /tmp/ir-sv.out; then
  echo "SEEDVERIFY CLEAN"
else
  echo "SEEDVERIFY TRAPPED (rc=$rc)"; grep "seedverify mismatch" /tmp/ir-sv.out | head; exit 1
fi
```

Expected: `SEEDVERIFY CLEAN`. **If it traps, STOP and read the spec §"central correctness risk":** the likely cause is a carried widening **lock** on a seeded local keeping it `Unique` after removal. Record the trapping functions. If only a handful trap, add a per-function exclusion (Task 2 covers the mechanism) forcing `allow_incremental=false` for them and re-run. **If widening functions trap broadly, do Task 3 (dirty-subgraph widening reset) before shipping; if that also traps, stop and record a second null result.**

- [ ] **Step 10: Full boot suite under SEEDVERIFY (fresh payload)**

```bash
set -o pipefail
TWINKLE_SEEDVERIFY=1 BOOT_WASM=/tmp/ir-t1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs run boot/tests/main.tw >/tmp/ir-t1-suite.out 2>&1
rc=$?
perl -pe 's/\e\[[0-9;]*m//g' /tmp/ir-t1-suite.out | grep -vE '^\.*$' | grep -v '^$' | tail -3
if [ $rc -eq 0 ] && ! grep -q "seedverify mismatch" /tmp/ir-t1-suite.out; then
  echo "SUITE+SEEDVERIFY OK"
else
  echo "SUITE FAIL (rc=$rc)"; grep -E "seedverify mismatch|Failed" /tmp/ir-t1-suite.out | head; exit 1
fi
```

Expected: `Ran N tests: N passed` and `SUITE+SEEDVERIFY OK`.

- [ ] **Step 11: Commit**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: incremental re-propagation machinery (production still cold)

Replace warm-start-by-restart with dirty-gated incremental re-propagation:
run_fixpoint carries the full FixState across reruns and, given a dirty set,
processes only dirty blocks and dirties successors on change. Pass 1 stays a
full sweep (dirty0=None) so output is byte-identical. Production stays cold;
the TWINKLE_SEEDVERIFY A/B proves incremental==cold seed sets."
```

---

### Task 2 — Enable incremental in production

Task 1 proved incremental ≡ cold seed sets via SEEDVERIFY. Flipping production to incremental is therefore expected byte-identical, since the returned `fx` is still a cold final pass over the (identical) stabilized seeds.

**Files:**
- Modify: `boot/compiler/ownership.tw`

- [ ] **Step 1: Flip the production `allow_incremental` to `true`**

In `run_fixpoint_validated`, update the comment and both branches so production uses incremental and the SEEDVERIFY branch returns the incremental result (so production/verify agree):

```tw
  // Production re-propagates the seed-finding reruns incrementally
  // (allow_incremental = true); the returned fx is still a cold final pass, so
  // the emitted program is byte-identical. Under TWINKLE_SEEDVERIFY, stabilize
  // both all-cold and incremental and trap on any per-function divergence.
  sr := if seedverify_enabled() {
    cold := stabilize_seeds(blocks, params, table, b, sem, suppress, unique_seed, label, false)
    incr := stabilize_seeds(blocks, params, table, b, sem, suppress, unique_seed, label, true)
    if !same_loop_seed_set(cold.seeds, incr.seeds) {
      error("seedverify mismatch: ${label}")
    }
    incr
  } else {
    stabilize_seeds(blocks, params, table, b, sem, suppress, unique_seed, label, true)
  }
```

- [ ] **Step 2: Format, lint, build**

```bash
set -o pipefail
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
if target/twk build boot/main.tw -o /tmp/ir-t2.wasm; then echo "BUILD OK"; else echo "BUILD FAIL"; exit 1; fi
```

Expected: clean; `BUILD OK`.

- [ ] **Step 3: Byte-identical output (hard accept gate)**

```bash
set -o pipefail
BOOT_WASM=/tmp/ir-t2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/ir-baseline/boot/main.tw -o /tmp/ir-out-t2.wasm
if cmp /tmp/ir-out-before.wasm /tmp/ir-out-t2.wasm; then echo "BYTE-IDENTICAL"; else echo "DIFFER"; exit 1; fi
```

Expected: `BYTE-IDENTICAL`. If it differs despite Task 1's SEEDVERIFY being clean, the divergence is in the *final* fx (not seed selection) — the final pass is cold, so this indicates a refactor bug (e.g. incremental state leaking into the final call). Investigate; do not accept.

- [ ] **Step 4: SEEDVERIFY + FIXVERIFY + suite (fresh payload; each a hard gate)**

```bash
set -o pipefail
# SEEDVERIFY over the self-build
TWINKLE_SEEDVERIFY=1 BOOT_WASM=/tmp/ir-t2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/ir-t2sv.wasm >/tmp/ir-t2sv.out 2>&1
rc=$?
if [ $rc -eq 0 ] && ! grep -q "seedverify mismatch" /tmp/ir-t2sv.out; then echo "SEEDVERIFY CLEAN"; else echo "SEEDVERIFY TRAPPED"; grep "seedverify mismatch" /tmp/ir-t2sv.out | head; exit 1; fi
# FIXVERIFY over the self-build
if TWINKLE_FIXVERIFY=1 BOOT_WASM=/tmp/ir-t2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/ir-t2fv.wasm >/tmp/ir-t2fv.out 2>&1; then echo "FIXVERIFY CLEAN"; else echo "FIXVERIFY TRAPPED"; exit 1; fi
# Boot suite under SEEDVERIFY, via the fresh payload
TWINKLE_SEEDVERIFY=1 BOOT_WASM=/tmp/ir-t2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs run boot/tests/main.tw >/tmp/ir-t2-suite.out 2>&1
rc=$?
perl -pe 's/\e\[[0-9;]*m//g' /tmp/ir-t2-suite.out | grep -vE '^\.*$' | grep -v '^$' | tail -3
if [ $rc -eq 0 ] && ! grep -q "seedverify mismatch" /tmp/ir-t2-suite.out; then echo "SUITE+SEEDVERIFY OK"; else echo "SUITE FAIL"; grep -E "seedverify mismatch|Failed" /tmp/ir-t2-suite.out | head; exit 1; fi
```

Expected: `SEEDVERIFY CLEAN`, `FIXVERIFY CLEAN`, `SUITE+SEEDVERIFY OK`.

- [ ] **Step 5: Confirm the win + incremental/cold split**

```bash
set -o pipefail
TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/ir-t2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/ir-baseline/boot/main.tw -o /tmp/ir-t2t.wasm >/tmp/ir-t2t.err 2>&1 || true
grep -E "produce_mutable_decisions|time:mutable:artifacts\]|fixpoint_validated.*summary:link" /tmp/ir-t2t.err
echo "reported slow funcs — incremental: $(grep -c 'fixpoint_validated.*incremental=true'  /tmp/ir-t2t.err)"
echo "reported slow funcs — cold:        $(grep -c 'fixpoint_validated.*incremental=false' /tmp/ir-t2t.err)"
```

Expected: `[time:mutable:artifacts] summary=` and `[time] produce_mutable_decisions` drop vs the ~11.9s / ~13.5s baseline, and **crucially** the `summary:link` line drops (unlike warm-start, incremental does not fall back to cold on widening functions). Record the `summary:link` total and the incremental/cold split. If the win is small, record it and consider Task 3 or stop.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: incremental re-propagation for loop-seed validation reruns"
```

---

### Task 3 — (Conditional) dirty-subgraph widening reset

**Do this task only if Task 1 Step 9 or Task 2 Step 4 SEEDVERIFY traps broadly on widening functions.** It re-widens the affected region from scratch while reusing the rest of the fixpoint, addressing the carried-lock hazard. If SEEDVERIFY was clean, **skip to Task 4.**

**Files:**
- Modify: `boot/compiler/ownership.tw`

- [ ] **Step 1: Compute the dirty-reachable subgraph and clear its widening state**

Before the incremental rerun in `stabilize_seeds`, expand the initial dirty set to its forward-reachable closure (via `build_succ_map`), and clear `locked_own/valid/prov`, `changed_visits`, `prev_seen`, and `prev_exits/valid/prov` **for blocks in that closure** on the carried `vstate` — leaving their `exits`/`processed` intact so re-propagation re-derives widening locally. Add a helper:

```tw
// Forward-reachable closure of `seed_blocks` over the successor map (inclusive).
fn dirty_closure(succ: Dict<Int, Vector<Int>>, seed_blocks: Dict<Int, Bool>) Dict<Int, Bool> {
  closure: Dict<Int, Bool> = Dict.new()
  work := collect_dirty_ids(seed_blocks)
  for !work.is_empty() {
    id := work[work.len() - 1]
    work = work.slice(0, work.len() - 1)
    if !is_dirty(closure, id) {
      closure[id] = true
      for s in succ_get(succ, id) {
        work = work.append(s)
      }
    }
  }
  closure
}

fn collect_dirty_ids(d: Dict<Int, Bool>) Vector<Int> {
  out: Vector<Int> = []
  for k in d.keys() {
    if is_dirty(d, k) {
      out = out.append(k)
    }
  }
  out
}

// Reset widening state (but not exits/processed) for the closure blocks so the
// re-propagated region widens from scratch instead of reusing pass-1 locks.
fn reset_widening_on(st: FixState, closure: Dict<Int, Bool>) FixState {
  locked_own := st.locked_own
  locked_valid := st.locked_valid
  locked_prov := st.locked_prov
  changed_visits := st.changed_visits
  prev_seen := st.prev_seen
  prev_exits := st.prev_exits
  prev_exit_valid := st.prev_exit_valid
  prev_exit_prov := st.prev_exit_prov
  for k in closure.keys() {
    if is_dirty(closure, k) {
      locked_own[k] = []
      locked_valid[k] = []
      locked_prov[k] = []
      changed_visits[k] = 0
      prev_seen[k] = false
      prev_exits[k] = Dict.new()
      prev_exit_valid[k] = Dict.new()
      prev_exit_prov[k] = Dict.new()
    }
  }
  st.locked_own = locked_own
  st.locked_valid = locked_valid
  st.locked_prov = locked_prov
  st.changed_visits = changed_visits
  st.prev_seen = prev_seen
  st.prev_exits = prev_exits
  st.prev_exit_valid = prev_exit_valid
  st.prev_exit_prov = prev_exit_prov
  st
}
```

Then in `stabilize_seeds`, when `allow_incremental`, expand dirty and reset before the rerun:

```tw
      dirty := removed_seed_blocks(blocks, seeds, kept)
      seeds = kept
      reruns = reruns + 1
      closure := dirty_closure(build_succ_map(blocks), dirty)
      vstate = reset_widening_on(vstate, closure)
      // ... then pass .Some(vstate) + .Some(dirty) into run_fixpoint as before
```

> The worklist still starts from `dirty` (the removed-seed blocks); the closure only governs which blocks had their widening locks cleared. Re-propagation will reach the closure naturally.

- [ ] **Step 2: Re-run SEEDVERIFY GO/NO-GO (self-build + suite)**

```bash
set -o pipefail
target/twk fmt boot/compiler/ownership.tw && target/twk lint boot/main.tw
target/twk build boot/main.tw -o /tmp/ir-t3.wasm
TWINKLE_SEEDVERIFY=1 BOOT_WASM=/tmp/ir-t3.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/ir-t3sv.wasm >/tmp/ir-t3sv.out 2>&1
rc=$?
if [ $rc -eq 0 ] && ! grep -q "seedverify mismatch" /tmp/ir-t3sv.out; then echo "SEEDVERIFY CLEAN"; else echo "SEEDVERIFY TRAPPED"; grep "seedverify mismatch" /tmp/ir-t3sv.out | head; exit 1; fi
```

Expected: `SEEDVERIFY CLEAN`. **If it still traps broadly, STOP** — incremental re-propagation with reuse across a widening region is not viable for this analysis; record the second null result in `docs/plans/performance/compiler.md` and archive this plan without shipping Task 2. If clean, return to **Task 2** (re-run its byte-identity + gates + measurement with `/tmp/ir-t3.wasm`) and commit.

---

### Task 4 — Record results and clean up

**Files:**
- Modify: `docs/plans/performance/compiler.md`

- [ ] **Step 1: Self-host stability**

```bash
set -o pipefail
BOOT_WASM=/tmp/ir-t2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/ir-stage3.wasm
if cmp /tmp/ir-t2.wasm /tmp/ir-stage3.wasm; then echo "SELF-HOST STABLE"; else echo "SELF-HOST DRIFT"; exit 1; fi
```

Expected: `SELF-HOST STABLE`. (Use `/tmp/ir-t3.wasm` if Task 3 ran.)

- [ ] **Step 2: A/B timing (3 runs each, fixed input)**

Compare the pre-incremental branch baseline (build a compiler from `HEAD` before Task 1's commit) against the incremental compiler — both cache-active, so only the rerun change differs:

```bash
set -o pipefail
git worktree add /tmp/ir-branchbase HEAD~2   # the commit before Task 1 (adjust if Task 3 added a commit)
cp boot/lib/module/core_lib.tw /tmp/ir-branchbase/boot/lib/module/core_lib.tw
target/twk build /tmp/ir-branchbase/boot/main.tw -o /tmp/ir-branchbase.wasm
for i in 1 2 3; do TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/ir-branchbase.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build /tmp/ir-baseline/boot/main.tw -o /tmp/ab-b-$i.wasm 2>&1 | grep -E "produce_mutable_decisions|time:mutable:artifacts\]" || true; done
for i in 1 2 3; do TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/ir-t2.wasm     deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build /tmp/ir-baseline/boot/main.tw -o /tmp/ab-a-$i.wasm 2>&1 | grep -E "produce_mutable_decisions|time:mutable:artifacts\]" || true; done
```

> The branch-baseline compiler must be the cache-active one (post FixResult-reuse), **not** `main` — `main` lacks the reuse cache (`ownership` stage ~8.3s vs ~0.65s), which would swamp the measurement. Take medians, not single runs.

- [ ] **Step 3: Record in `docs/plans/performance/compiler.md`**

Append `## Update: loop-seed rerun incremental re-propagation` with: the mechanism (reruns carry the full FixState and re-propagate only from removed-seed blocks via a dirty worklist; pass 1 and the returned fx stay full-sweep cold; validator-gated on seed-set equivalence; does **not** fall back on widening functions, unlike warm-start), the before/after `produce_mutable_decisions` + `[time:mutable:artifacts]` medians from Step 2, the `summary:link` delta specifically, whether Task 3's widening reset was needed, and acceptance (SEEDVERIFY clean, byte-identical, FIXVERIFY clean, self-host stable, full suite green).

- [ ] **Step 4: Clean up and commit**

```bash
git worktree remove /tmp/ir-baseline --force
git worktree remove /tmp/ir-branchbase --force
git add docs/plans/performance/compiler.md
git commit -m "docs: record loop-seed incremental re-propagation results"
```

Then archive this plan: `git mv docs/plans/incremental-repropagation.md docs/plans/archive/` and commit (add a one-line OUTCOME banner at its top first, per the warm-start plan's precedent).

---

## Risks and Stop Rules

- **SEEDVERIFY traps (Task 1 Step 9 / Task 2 Step 4):** incremental selected a different seed set for the named function. Most likely a **carried widening lock** keeping a seeded local `Unique` after removal. A handful → per-function exclusion (`allow_incremental=false` for them). Broad, on widening functions → **Task 3** (dirty-subgraph widening reset). If Task 3 still traps broadly → **stop, second null result.** Never weaken the validator.
- **Byte-identity fails but SEEDVERIFY clean (Task 2 Step 3):** divergence is in the final fx, not seed selection — the final pass is cold, so this is a refactor bug (incremental state leaking into the final call, or the dirty gate not inert under `all_dirty`). Investigate; do not accept.
- **Byte-identity fails at Task 1 Step 8 (production still cold):** the pass-1 refactor changed behavior. The dirty gate must be a no-op when `all_dirty` — check that every original state write is still inside the loop body and correctly re-indented, and that `succ`/`dirty` are never read on the cold path.
- **Win is small even though SEEDVERIFY is clean:** the dirty subgraph is large (removed seeds sit near the top of deep loops, so re-propagation reaches most blocks). Record it; the win is workload-shaped and this is the last cheap lever for the rerun cost.
- **Memory / aliasing:** reruns pass `vstate` by reference to seed the next run; `run_fixpoint` rebinds via new maps (persistent dicts, copy-on-write), so the carried dicts are not mutated in place. The byte-identity + self-host gates cover this.

## Expected Outcome

Seed-finding reruns collapse from full cold sweeps to sparse re-propagations over only the blocks a seed removal actually affects, **including** the dominant `summary:link` (which warm-start-by-restart structurally could not accelerate), taking a real bite out of the ~11.9s summary stage — with the emitted program byte-identical, the seed set provably (empirically) unchanged per function, and both `SEEDVERIFY` and self-host gates green.
