# Cutting the Loop-Seed Rerun Cost in `run_fixpoint_validated`

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cut the summary stage of `produce_mutable_decisions` (now ~88% of it) by making the loop-seed-validation reruns in `run_fixpoint_validated` cheaper — **without changing the stabilized loop-seed set** (and therefore the emitted program). Warm-start each seed-finding rerun from the previous iteration's fixpoint instead of restarting cold; keep the returned `FixResult` a fresh cold pass; gate the whole thing on a seed-set-equivalence validator.

**Tech Stack:** Twinkle boot compiler (`boot/`). Sole file: `boot/compiler/ownership.tw`. CLI: `target/twk`, `deno` runtime harness.

---

## Spec

### The problem

`run_fixpoint_validated` runs a nested fixpoint. The **inner** fixpoint (`run_fixpoint`) computes ownership/validity/provenance over the CFG. The **outer** loop optimistically seeds loop-carried locals as `Unique`, then validates which seeds actually hold, removing invalid ones and re-running the inner fixpoint until the seed set stabilizes:

```tw
seeds := collect_loop_seed_candidates(blocks)
fx := run_fixpoint(...)                              // pass 1 (cold)
for !stable_seeds {
  kept := retain_valid_loop_seeds(blocks, fx, seeds) // reads fx.exits + fx.exit_valid
  if same { stable } else { seeds = kept; fx = run_fixpoint(...) }  // rerun (cold)
}
```

Each rerun is a **cold** inner fixpoint. `link` (243 blocks) runs 15 of them. Reruns dominate: on the cache-active compiler the summary stage is ~11.7s of the ~13.3s `produce_mutable_decisions`; the 44 slowest functions are ~7.4s of that, and `summary:link` alone is ~3.4s / 14 reruns.

### Why the obvious optimizations are unsound (rejected)

A prior draft proposed running reruns "shell-only" (skip `field_own`/`path_prov`, which the validator does not read) and warm-starting naively. Code review killed both. Documented so we do not retry:

- **`own` depends on `path_prov`.** `publish_local` (`ownership.tw:890-905`) iterates `path_prov` and `set_own_st(o, .Shared)` on each nested origin. Empty `path_prov` → those origins are never published → `exits` differs, and the validator reads `exits`.
- **`own` depends on `field_own`.** `ARecordGet` (`ownership.tw:1707-1738`) sets the projected result's ownership from `atom_field_own(base)`; empty `field_own` forces the `Unknown` branch. So the field-granular fields feed back into the shell lattice — there is no clean "shell-only" subset.
- **`FIXVERIFY` cannot validate an algorithm change.** It compares the cached full `FixResult` against a fresh `run_fixpoint_validated`; both would use the new algorithm, so they agree even when the stabilized seed set is wrong. Byte-identity vs `main` fails without localizing.
- **Naive warm-start is not obviously monotone.** `consume_base` (`ownership.tw:956-963`) branches on `local_reusable(st.own, st.valid, ...)`; combined with widening (`fixpoint_widen_cap`, false-sticky validity via `merge_targeted`), a warm start can converge to a different, internally-consistent fixpoint.

### The safe design

Three ideas, each addressing a specific failure above.

1. **The returned `FixResult` is always a fresh cold pass.** Warm-start accelerates *only* the seed-finding reruns, whose `fx` is consumed solely by `retain_valid_loop_seeds`. Once the seed set stabilizes, one **cold** `run_fixpoint(stable_seeds)` produces the value the function returns (and that the reuse cache stores). Consequence: a warm-start bug can only ever change *which seeds stabilize* — never silently taint the returned result's fields.

2. **A seed-set-equivalence validator is the acceptance gate.** Under `TWINKLE_SEEDVERIFY`, `run_fixpoint_validated` stabilizes the seed set **twice** — once all-cold (the reference), once warm — and traps (`error("seedverify mismatch: <label>")`) if the two seed sets differ, keyed by function. This checks the thing that matters (seed selection), localizes divergence to a function, and — unlike FIXVERIFY — compares the *old algorithm against the new one*. Acceptance is **zero mismatches across the full self-build + boot suite**. The validator runs both paths (~2× cost) and is dev-only.

3. **Widening is the only order-dependence; neutralize and gate it.** Each warm rerun **resets the widening counters** (`changed_visits`, `locked_*`, `prev_*`, `prev_seen`) and carries only the dataflow exit maps, so widening fires per-rerun as it would cold. And the outer loop **only warm-starts when the cold pass 1 did not widen** (`run_fixpoint` returns a `widened` flag); widening-prone functions fall back to today's all-cold reruns. The validator is the actual guarantee; the gate is what makes divergence unlikely. If the validator reports a mismatch, tighten the gate (e.g. exclude any function that widens in a cold rerun) rather than weakening the check.

### Acceptance bar

Empirical, validator-gated: an accepted optimization must produce **byte-identical per-function stabilized seed sets** vs the all-cold reference, verified by `TWINKLE_SEEDVERIFY` across the self-build + boot suite, and then **byte-identical emitted program** (fixed input, before/after compiler), `TWINKLE_FIXVERIFY` clean, and self-host stable. Not a formal proof; the validator is the standing check.

---

## Global Constraints

- Boot-compiler-only: the emitted program must be **byte-identical**. No stage0 (`src/`) changes.
- After editing `.tw`, run `target/twk fmt <file>` and `target/twk lint boot/main.tw` (must report `No findings.`; apply `target/twk lint boot/main.tw --fix` for inherent-call rewrites, then re-fmt).
- Gating shell commands must **not mask failure**: check exit status explicitly (`if ! cmd; then echo FAIL; fi`) and put `set -o pipefail` at the top of any block whose success is read through a pipe.
- Heavy verification (full suite, builds, timed/`SEEDVERIFY` runs) runs **one at a time**, never concurrently or backgrounded.

## Baseline Evidence (cache-active compiler, this branch)

```text
[time:mutable:artifacts] cfg~0.73s summary~11.7s ownership~0.61s   scope_roots=462
[time] produce_mutable_decisions ~13.3s
[time:own:fixpoint_validated] func=summary:link total~3414ms reruns=14 blocks=243
44 reported (slow) functions total ~7.37s (~63% of summary)
```

Confirmed in code:
- `validate_loop_seed` reads only `fx.exits` + `fx.exit_valid` (via `pred_local_contribution`).
- `run_fixpoint` returns `FixResult.{ exits, exit_valid, exit_prov, exit_field_own, exit_path_prov }` (`ownership.tw:3271`) and has exactly **2** callers, both inside `run_fixpoint_validated` (`:3297`, `:3307`).
- Widening fires at `force_lock_changed := changed_visits[blk] >= fixpoint_widen_cap` (`ownership.tw:3168`), consumed by `merge_targeted`.
- The inner fixpoint's per-block init (the empty-dict setup) is `ownership.tw:3067-3096`.

## File Structure

- Modify `boot/compiler/ownership.tw` only:
  - `run_fixpoint` gains a trailing `warm: FixResult?` param and returns a new `FixRun = .{ fx: FixResult, widened: Bool }`; it seeds its exit maps from `warm` and always resets the widening counters.
  - New `stabilize_seeds(...) SeedRun` extracts the pass-1 + rerun loop, warm-starting reruns when `allow_warm` and pass 1 did not widen.
  - New `seedverify_enabled()`; `run_fixpoint_validated` runs the `SEEDVERIFY` A/B, then returns a cold final `run_fixpoint`.

---

### Task 1 — Land the machinery + validator (production stays cold, byte-identical)

Warm-start code lands and is exercised **only by the validator**; the production path stays all-cold so this task is byte-identical. The validator proves warm≡cold seed sets before Task 2 depends on it.

**Files:**
- Modify: `boot/compiler/ownership.tw`

- [ ] **Step 1: Add the result/run record types and the env flag**

Add near `type FixResult` (top-level):

```tw
// Inner fixpoint result plus whether widening (force-lock) fired during it.
type FixRun = .{ fx: FixResult, widened: Bool }

// Stabilized loop-seed set plus the rerun count (for diagnostics).
type SeedRun = .{ seeds: LoopSeedSet, reruns: Int }

fn seedverify_enabled() Bool {
  case proc.env("TWINKLE_SEEDVERIFY") {
    .Some(_) => true,
    .None => false,
  }
}
```

- [ ] **Step 2: `run_fixpoint` — add `warm` param, `widened` return, reset widening counters**

Change the signature:

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
  warm: FixResult?,
) FixRun {
```

Replace the exit-map declarations and per-block init loop (`ownership.tw:3067-3096`, the block that declares `exits`…`processed` and the `for blk in blocks { exits[...] = Dict.new() ... }` loop) with a warm-aware version. The **dataflow** exit maps seed from `warm`; the **widening** state (`prev_*`, `locked_*`, `prev_seen`, `changed_visits`) is always fresh:

```tw
  warm_started := case warm {
    .Some(_) => true,
    .None => false,
  }
  exits: Dict<Int, Dict<Int, Int>> = case warm {
    .Some(w) => w.exits,
    .None => Dict.new(),
  }
  exit_valid: Dict<Int, Dict<Int, Bool>> = case warm {
    .Some(w) => w.exit_valid,
    .None => Dict.new(),
  }
  exit_prov: Dict<Int, Dict<Int, Vector<Int>>> = case warm {
    .Some(w) => w.exit_prov,
    .None => Dict.new(),
  }
  exit_field_own: Dict<Int, Dict<Int, ff.FieldMap>> = case warm {
    .Some(w) => w.exit_field_own,
    .None => Dict.new(),
  }
  exit_path_prov: Dict<Int, Dict<Int, Dict<Int, Vector<Int>>>> = case warm {
    .Some(w) => w.exit_path_prov,
    .None => Dict.new(),
  }
  prev_exits: Dict<Int, Dict<Int, Int>> = Dict.new()
  prev_exit_valid: Dict<Int, Dict<Int, Bool>> = Dict.new()
  prev_exit_prov: Dict<Int, Dict<Int, Vector<Int>>> = Dict.new()
  locked_own: Dict<Int, Vector<Int>> = Dict.new()
  locked_valid: Dict<Int, Vector<Int>> = Dict.new()
  locked_prov: Dict<Int, Vector<Int>> = Dict.new()
  prev_seen: Dict<Int, Bool> = Dict.new()
  changed_visits: Dict<Int, Int> = Dict.new()
  processed: Dict<Int, Bool> = Dict.new()
  for blk in blocks {
    if !warm_started {
      exits[blk.id.id] = Dict.new()
      exit_valid[blk.id.id] = Dict.new()
      exit_prov[blk.id.id] = Dict.new()
      exit_field_own[blk.id.id] = Dict.new()
      exit_path_prov[blk.id.id] = Dict.new()
    }
    prev_exits[blk.id.id] = Dict.new()
    prev_exit_valid[blk.id.id] = Dict.new()
    prev_exit_prov[blk.id.id] = Dict.new()
    locked_own[blk.id.id] = []
    locked_valid[blk.id.id] = []
    locked_prov[blk.id.id] = []
    prev_seen[blk.id.id] = false
    changed_visits[blk.id.id] = 0
    processed[blk.id.id] = warm_started
  }
```

Add a `widened` accumulator before the fixpoint loop (next to `changed := true`):

```tw
  widened := false
```

Record widening where `force_lock_changed` is computed (`ownership.tw:3168`, inside `if already {`). Immediately after that line add:

```tw
        if force_lock_changed {
          widened = true
        }
```

Change the return (`ownership.tw:3271`) to:

```tw
  FixRun.{ fx: FixResult.{ exits, exit_valid, exit_prov, exit_field_own, exit_path_prov }, widened }
```

- [ ] **Step 3: Add `stabilize_seeds`**

Add directly above `run_fixpoint_validated`:

```tw
// Find the stable loop-seed set: pass 1 cold, then reruns until the seed set
// stops shrinking. When `allow_warm` and pass 1 did not widen, each rerun
// warm-starts from the prior rerun's fx (dataflow values carried, widening
// counters reset inside run_fixpoint). The returned seeds drive the final
// (cold) fixpoint in run_fixpoint_validated.
fn stabilize_seeds(
  blocks: Vector<CfgBlock>,
  params: Vector<LocalId>,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
  unique_seed: Dict<Int, Bool>,
  label: String,
  allow_warm: Bool,
) SeedRun {
  seeds := collect_loop_seed_candidates(blocks)
  first := run_fixpoint(blocks, params, table, b, sem, suppress, unique_seed, seeds, label, .None)
  vfx := first.fx
  warm_ok := allow_warm and !first.widened
  reruns := 0
  stable := false
  for !stable {
    kept := retain_valid_loop_seeds(blocks, vfx, seeds)
    if same_loop_seed_set(kept, seeds) {
      stable = true
    } else {
      seeds = kept
      reruns = reruns + 1
      warm: FixResult? = if warm_ok {
        .Some(vfx)
      } else {
        .None
      }
      rr := run_fixpoint(blocks, params, table, b, sem, suppress, unique_seed, seeds, label, warm)
      vfx = rr.fx
    }
  }
  SeedRun.{ seeds, reruns }
}
```

- [ ] **Step 4: Rewrite `run_fixpoint_validated` — validator A/B + cold final; production cold this task**

Replace the body of `run_fixpoint_validated` (from `timed := timings_enabled()` through the final `fx`) with:

```tw
  timed := timings_enabled()
  t0 := if timed {
    date.now()
  } else {
    0.0
  }
  initial_seeds := collect_loop_seed_candidates(blocks)
  initial_seed_count := initial_seeds.keys().len()
  initial_seed_stats := if timed {
    loop_seed_stats(blocks, initial_seeds)
  } else {
    LoopSeedStats.{ headers: 0, param_seeds: 0, nonparam_seeds: 0 }
  }
  // TASK 1: production is all-cold (allow_warm = false) — byte-identical. The
  // validator exercises warm-start and proves seed-set equivalence. TASK 2 flips
  // the production allow_warm to true.
  sr := if seedverify_enabled() {
    cold := stabilize_seeds(blocks, params, table, b, sem, suppress, unique_seed, label, false)
    warm := stabilize_seeds(blocks, params, table, b, sem, suppress, unique_seed, label, true)
    if !same_loop_seed_set(cold.seeds, warm.seeds) {
      error("seedverify mismatch: ${label}")
    }
    cold
  } else {
    stabilize_seeds(blocks, params, table, b, sem, suppress, unique_seed, label, false)
  }
  seeds := sr.seeds
  reruns := sr.reruns
  fx := run_fixpoint(blocks, params, table, b, sem, suppress, unique_seed, seeds, label, .None).fx
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
  fx
```

- [ ] **Step 5: Format, lint, build**

```bash
set -o pipefail
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
if target/twk build boot/main.tw -o /tmp/rc-t1.wasm; then echo "BUILD OK"; else echo "BUILD FAIL"; fi
```

Expected: fmt/lint clean (`No findings.`); `BUILD OK`.

- [ ] **Step 6: Byte-identical output (production still cold — must match `main`)**

`core_lib.tw` is generated + gitignored; copy it into the baseline worktree.

```bash
set -o pipefail
git worktree add /tmp/rc-baseline main
cp boot/lib/module/core_lib.tw /tmp/rc-baseline/boot/lib/module/core_lib.tw
target/twk build /tmp/rc-baseline/boot/main.tw -o /tmp/rc-before.wasm

BOOT_WASM=/tmp/rc-before.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/rc-baseline/boot/main.tw -o /tmp/rc-out-before.wasm
BOOT_WASM=/tmp/rc-t1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/rc-baseline/boot/main.tw -o /tmp/rc-out-t1.wasm
if cmp /tmp/rc-out-before.wasm /tmp/rc-out-t1.wasm; then echo "BYTE-IDENTICAL"; else echo "DIFFER"; fi
```

Expected: `BYTE-IDENTICAL` (production is unchanged all-cold; the refactor must not alter behavior). Keep `/tmp/rc-baseline` and `/tmp/rc-before.wasm` for later tasks.

- [ ] **Step 7: SEEDVERIFY across the self-build (proves warm ≡ cold seed sets)**

```bash
set -o pipefail
if TWINKLE_SEEDVERIFY=1 BOOT_WASM=/tmp/rc-t1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/rc-sv.wasm 2>/tmp/rc-sv.err; then
  echo "SEEDVERIFY CLEAN"
else
  echo "SEEDVERIFY TRAPPED"; grep "seedverify mismatch" /tmp/rc-sv.err | head
fi
```

Expected: `SEEDVERIFY CLEAN` — no `seedverify mismatch` trap on any function. **If it traps, STOP:** warm-start diverges for the named function (almost certainly a widening interaction the pass-1 gate didn't catch). Tighten `warm_ok` to also require that no rerun widened (thread a `widened` accumulator through the `stabilize_seeds` loop and clear `warm_ok` once any `rr.widened` is true), re-run, and record which functions fell back.

- [ ] **Step 8: Full boot suite (also runs under SEEDVERIFY)**

```bash
set -o pipefail
if target/twk test; then echo "SUITE OK"; else echo "SUITE FAIL"; fi
```

Expected: `SUITE OK`. (The boot suite compiles many fixtures; SEEDVERIFY need not be on here — Step 7 already exercised warm-start over the largest program. Optionally re-run one suite with `TWINKLE_SEEDVERIFY=1` if the fixtures exercise loopy functions.)

- [ ] **Step 9: Commit**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: warm-start machinery + seed-set validator (production still cold)"
```

---

### Task 2 — Enable warm-start in production

Task 1 proved warm ≡ cold seed sets via SEEDVERIFY. Flipping production to warm is therefore expected byte-identical, since the returned `fx` is still a cold final pass over the (identical) stabilized seeds.

**Files:**
- Modify: `boot/compiler/ownership.tw`

- [ ] **Step 1: Flip the production `allow_warm` to `true`**

In `run_fixpoint_validated`, change the non-SEEDVERIFY branch:

```tw
  } else {
    stabilize_seeds(blocks, params, table, b, sem, suppress, unique_seed, label, true)
  }
```

(Also update the SEEDVERIFY branch's returned value from `cold` to `warm` so the two are interchangeable and production/verify agree: change `cold` on the line after the mismatch check to `warm`.)

- [ ] **Step 2: Format, lint, build**

```bash
set -o pipefail
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
if target/twk build boot/main.tw -o /tmp/rc-t2.wasm; then echo "BUILD OK"; else echo "BUILD FAIL"; fi
```

Expected: clean; `BUILD OK`.

- [ ] **Step 3: Byte-identical output (hard accept gate)**

```bash
set -o pipefail
BOOT_WASM=/tmp/rc-t2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/rc-baseline/boot/main.tw -o /tmp/rc-out-t2.wasm
if cmp /tmp/rc-out-before.wasm /tmp/rc-out-t2.wasm; then echo "BYTE-IDENTICAL"; else echo "DIFFER"; fi
```

Expected: `BYTE-IDENTICAL`. If it differs despite Task 1's SEEDVERIFY being clean, the divergence is in the *final* fx (not seed selection) — investigate, do not accept.

- [ ] **Step 4: SEEDVERIFY + FIXVERIFY + suite**

```bash
set -o pipefail
if TWINKLE_SEEDVERIFY=1 BOOT_WASM=/tmp/rc-t2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/rc-t2sv.wasm 2>/tmp/rc-t2sv.err; then echo "SEEDVERIFY CLEAN"; else echo "SEEDVERIFY TRAPPED"; grep "seedverify mismatch" /tmp/rc-t2sv.err | head; fi
if TWINKLE_FIXVERIFY=1 BOOT_WASM=/tmp/rc-t2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/rc-t2fv.wasm; then echo "FIXVERIFY CLEAN"; else echo "FIXVERIFY TRAPPED"; fi
if target/twk test; then echo "SUITE OK"; else echo "SUITE FAIL"; fi
```

Expected: `SEEDVERIFY CLEAN`, `FIXVERIFY CLEAN`, `SUITE OK`.

- [ ] **Step 5: Confirm the win + how many functions fell back to cold**

```bash
set -o pipefail
TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/rc-t2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/rc-baseline/boot/main.tw -o /tmp/rc-t2t.wasm 2>&1 \
  | grep -E "produce_mutable_decisions|time:mutable:artifacts\]|fixpoint_validated.*summary:link" || true
```

Expected: `[time:mutable:artifacts] summary=` and `[time] produce_mutable_decisions` drop vs the ~11.7s / ~13.3s baseline; record `summary:link` total. If the win is small, most slow functions widened (fell back to cold) — record that finding and consider the incremental-re-propagation follow-up.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: warm-start loop-seed validation reruns"
```

---

### Task 3 — Record results and clean up

**Files:**
- Modify: `docs/plans/performance/compiler.md`

- [ ] **Step 1: Self-host stability**

```bash
set -o pipefail
BOOT_WASM=/tmp/rc-t2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/rc-stage3.wasm
if cmp /tmp/rc-t2.wasm /tmp/rc-stage3.wasm; then echo "SELF-HOST STABLE"; else echo "SELF-HOST DRIFT"; fi
```

Expected: `SELF-HOST STABLE`.

- [ ] **Step 2: A/B timing (3 runs each, fixed input)**

```bash
set -o pipefail
for i in 1 2 3; do TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/rc-before.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build /tmp/rc-baseline/boot/main.tw -o /tmp/ab-b-$i.wasm 2>&1 | grep -E "produce_mutable_decisions|time:mutable:artifacts\]" || true; done
for i in 1 2 3; do TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/rc-t2.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build /tmp/rc-baseline/boot/main.tw -o /tmp/ab-a-$i.wasm 2>&1 | grep -E "produce_mutable_decisions|time:mutable:artifacts\]" || true; done
```

- [ ] **Step 3: Record in `docs/plans/performance/compiler.md`**

Append a section `## Update: loop-seed rerun warm-start` with: the mechanism (reruns warm-start from the prior fixpoint; returned fx is a cold final pass; validator-gated on seed-set equivalence; widening-prone functions fall back to cold), the before/after `produce_mutable_decisions` + `[time:mutable:artifacts]` lines from Step 2, the `summary:link` delta, and acceptance (SEEDVERIFY clean, byte-identical, FIXVERIFY clean, self-host stable, full suite green). Note how many functions fell back to cold.

- [ ] **Step 4: Clean up and commit**

```bash
git worktree remove /tmp/rc-baseline --force
git add docs/plans/performance/compiler.md
git commit -m "docs: record loop-seed rerun warm-start results"
```

---

## Risks and Stop Rules

- **SEEDVERIFY traps (Task 1 Step 7 / Task 2 Step 4):** warm-start selected a different seed set for the named function — a widening interaction. Tighten `warm_ok` to also clear on any rerun that widened; if a function still diverges, exclude it (cold reruns) and record it. Never weaken the validator to pass.
- **Byte-identity fails but SEEDVERIFY clean (Task 2 Step 3):** the divergence is in the final fx, not seed selection — the final pass is cold, so this would indicate a refactor bug in `run_fixpoint` (e.g. warm state leaking into the final call). Investigate; do not accept.
- **Win is small:** most slow functions widen and fall back to cold. Record the count; the follow-up lever is incremental re-propagation (re-process only blocks reachable from removed seeds), which does not depend on the monotone assumption and can be validated by the same SEEDVERIFY harness.
- **Memory / aliasing:** warm-start reuses the prior `vfx` maps by reference to seed the next run's exits; the round loop rebinds exit entries via new maps (`merge_targeted` builds fresh maps), so the passed-in warm dicts must not be mutated in place and survive into the returned `fx`. The byte-identity + self-host gates cover this.

## Expected Outcome

Seed-finding reruns collapse from cold full sweeps to short warm re-convergences for the (majority) non-widening functions, taking a large bite out of the ~11.7s summary stage (dominated by `link`'s 15 passes), with the emitted program byte-identical, the seed set provably (empirically) unchanged per function, and both `SEEDVERIFY` and self-host gates green.
