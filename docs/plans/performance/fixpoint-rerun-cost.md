# Cutting the Loop-Seed Rerun Cost in `run_fixpoint_validated`

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cut the summary stage of `produce_mutable_decisions` (now ~88% of it) by making the loop-seed-validation reruns in `run_fixpoint_validated` cheaper — first by not computing the field-granular `FixResult` fields nobody reads during validation (Lever 1), then by warm-starting each rerun from the previous iteration's fixpoint instead of restarting cold (Lever 2).

**Architecture:** `run_fixpoint_validated` runs the full 5-field ownership fixpoint once per validation round (`link`: 15 passes). The validator reads only `exits` (ownership) + `exit_valid`. Lever 1 runs the reruns in a `shell_only` mode that leaves `field_own`/`path_prov` empty, then does one full pass with the stabilized seeds. Lever 2 seeds each rerun's initial exit state from the prior `fx` so the monotone re-convergence takes far fewer rounds. Both are validated by the byte-identical build + `TWINKLE_FIXVERIFY` gates already in the tree.

**Tech Stack:** Twinkle boot compiler (`boot/`). Sole file: `boot/compiler/ownership.tw`. CLI: `target/twk`, `deno` runtime harness.

## Global Constraints

- Boot-compiler-only optimization: the emitted program must be **byte-identical**. No stage0 (`src/`) changes.
- After editing `.tw`, run `target/twk fmt <file>` and `target/twk lint boot/main.tw` (must report `No findings.`; apply `target/twk lint boot/main.tw --fix` for inherent-call rewrites, then re-fmt).
- **Hard accept gates for each Lever:** (1) build output byte-identical for a fixed input compiled by the before- and after-compilers; (2) `TWINKLE_FIXVERIFY=1` build completes without trapping; (3) full boot suite green.
- Heavy verification (full suite, builds, timed runs) runs **one at a time**, never concurrently or backgrounded.
- Soundness invariant carried from the reuse work: a `FixResult` consumed by the analyze pass (the cache) must be a **full** 5-field result. Only the internal validation reruns may be `shell_only`; the value returned from `run_fixpoint_validated` is always the full final pass.

## Baseline Evidence (cache-active compiler, this branch)

```text
[time:mutable:artifacts] cfg~0.73s summary~11.7s ownership~0.61s   scope_roots=462
[time] produce_mutable_decisions ~13.3s
```

Summary is 88% of the total. Within it the cost is concentrated in reruns:

```text
[time:own:fixpoint_validated] func=summary:link total~3414ms reruns=14 blocks=243   (~29% of summary alone)
44 reported (slow) functions total ~7.37s (~63% of summary)
```

`run_fixpoint_validated` (`ownership.tw`):

```tw
seeds := collect_loop_seed_candidates(blocks)
fx := run_fixpoint(...)                 // pass 1 (full 5-field)
for !stable_seeds {
  kept := retain_valid_loop_seeds(blocks, fx, seeds)   // reads ONLY fx.exits + fx.exit_valid
  if same { stable } else { seeds = kept; fx = run_fixpoint(...) }   // rerun (full 5-field)
}
```

Key facts confirmed in code:

- `validate_loop_seed` reads only `fx.exits` (ownership) and `fx.exit_valid` (via `pred_local_contribution`). It never reads `exit_prov` / `exit_field_own` / `exit_path_prov`.
- The shell fields (`own`, `prov`) are set independently in `transfer_op`; `field_own`/`path_prov` are a downstream refinement. `publish_local`'s shell effect (own→Shared, cascade to `prov` origins) uses `st.prov`, not `path_prov`. So `own`/`valid`/`prov` are computable with `field_own`/`path_prov` left empty.
- All field-granular writes funnel through two choke-point setters, `set_field_own` and `set_path_prov` (plus `clear_field_own`/`clear_path_prov`). The per-block entry refinement enters via `join_entry_field_own` / `join_entry_path_prov` (called at the two `run_fixpoint` / materialize sites).
- `ForwardState` has exactly **4** constructor literals (`ownership.tw:3131`, `:3499`, `:3806`, `:4662`). `run_fixpoint` has exactly **2** callers (both inside `run_fixpoint_validated`).

## File Structure

- Modify `boot/compiler/ownership.tw` only:
  - `ForwardState` gains a `shell_only: Bool` field; the 4 constructors set it.
  - `set_field_own`/`set_path_prov`/`clear_field_own`/`clear_path_prov` no-op when `st.shell_only`.
  - `run_fixpoint` gains a `shell_only: Bool` param; it constructs its `ForwardState` with that flag and skips the two entry joins when set.
  - `run_fixpoint_validated` runs `shell_only` reruns + one full final pass (Lever 1), then warm-starts the reruns (Lever 2).

---

### Task 1 — Lever 1: `shell_only` reruns

**Files:**
- Modify: `boot/compiler/ownership.tw`

- [ ] **Step 1: Add `shell_only` to `ForwardState`**

Change the `ForwardState` type:

```tw
type ForwardState = .{
  own: Dict<Int, Int>,
  valid: Dict<Int, Bool>,
  prov: Dict<Int, Vector<Int>>,
  field_own: Dict<Int, ff.FieldMap>,
  path_prov: Dict<Int, Dict<Int, Vector<Int>>>,
  shell_only: Bool,
}
```

- [ ] **Step 2: Guard the four field-granular setters**

Change these four methods so they are no-ops in shell-only mode (field maps stay empty). Replace each body:

```tw
fn set_field_own(st: ForwardState, id: Int, m: ff.FieldMap) ForwardState {
  if st.shell_only {
    return st
  }
  st.field_own[id] = m
  st
}

fn clear_field_own(st: ForwardState, id: Int) ForwardState {
  if st.shell_only {
    return st
  }
  st.field_own[id] = ff.empty()
  st
}

fn set_path_prov(st: ForwardState, id: Int, m: Dict<Int, Vector<Int>>) ForwardState {
  if st.shell_only {
    return st
  }
  st.path_prov[id] = m
  st
}

fn clear_path_prov(st: ForwardState, id: Int) ForwardState {
  if st.shell_only {
    return st
  }
  st.path_prov[id] = Dict.new()
  st
}
```

(All field-granular writes funnel through these; the ~20 `set_field_own`/`set_path_prov`/`graft` call sites need no change — they compute `rf`/`pp` and the setter drops them. `publish_local`'s `path_prov_get` returns empty and its shell own/prov cascade is unaffected.)

- [ ] **Step 3: Add `shell_only` to `run_fixpoint` and skip entry joins**

Change the `run_fixpoint` signature to add a trailing `shell_only: Bool`:

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
  shell_only: Bool,
) FixResult {
```

Inside the per-block loop, replace the two entry-refinement joins and the `ForwardState` construction (the block starting `entry_field := join_entry_field_own(...)` through `st := ForwardState.{ ... }`):

```tw
      entry_field := if shell_only {
        Dict.new()
      } else {
        join_entry_field_own(blk, exit_field_own, entry_own, processed)
      }
      entry_pp := if shell_only {
        Dict.new()
      } else {
        join_entry_path_prov(blk, exit_path_prov, processed)
      }
      st := ForwardState.{
        own: entry_own,
        valid: entry_valid,
        prov: entry_prov,
        field_own: entry_field,
        path_prov: entry_pp,
        shell_only,
      }
```

- [ ] **Step 4: Set `shell_only: false` in the other three `ForwardState` constructors**

At `ownership.tw:3499` (in `ownership_stage`'s materialize loop), `:3806` (in `summarize_seeded`'s return-site replay), and `:4662` (in the variant/summary path), add `shell_only: false` to each `ForwardState.{ ... }` literal. These are full-fidelity paths. Example at each site — append the field:

```tw
      st := ForwardState.{
        own: entry_own,
        valid: entry_valid,
        prov: entry_prov,
        field_own: entry_field,
        path_prov: entry_pp,
        shell_only: false,
      }
```

- [ ] **Step 5: Run `shell_only` reruns + one full final pass in `run_fixpoint_validated`**

Replace the fixpoint/rerun block (from `fx := run_fixpoint(...)` through the end of the `for !stable_seeds` loop) with:

```tw
  vfx := run_fixpoint(blocks, params, table, b, sem, suppress, unique_seed, seeds, label, true)
  reruns := 0
  stable_seeds := false
  for !stable_seeds {
    kept := retain_valid_loop_seeds(blocks, vfx, seeds)
    if same_loop_seed_set(kept, seeds) {
      stable_seeds = true
    } else {
      seeds = kept
      reruns = reruns + 1
      vfx = run_fixpoint(blocks, params, table, b, sem, suppress, unique_seed, seeds, label, true)
    }
  }
  fx := run_fixpoint(blocks, params, table, b, sem, suppress, unique_seed, seeds, label, false)
```

`vfx` (validation fx, shell-only) drives the seed loop; `fx` (full) is computed once with the stabilized seeds and is what the function returns. The existing `if timed { ... }` diagnostics block below this uses `fx`, `reruns`, and `seeds` — leave it unchanged; it still reports correctly.

- [ ] **Step 6: Format, lint, build**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
target/twk build boot/main.tw -o /tmp/rc-stage2.wasm
```

Expected: fmt/lint clean; build writes `/tmp/rc-stage2.wasm`.

- [ ] **Step 7: Byte-identical output (hard accept gate)**

Compile a fixed input with the before-compiler (from `main`) and the after-compiler, and compare. (`core_lib.tw` is generated + gitignored, so copy it into the baseline worktree.)

```bash
git worktree add /tmp/rc-baseline main
cp boot/lib/module/core_lib.tw /tmp/rc-baseline/boot/lib/module/core_lib.tw
target/twk build /tmp/rc-baseline/boot/main.tw -o /tmp/rc-before.wasm

BOOT_WASM=/tmp/rc-before.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/rc-baseline/boot/main.tw -o /tmp/rc-out-before.wasm
BOOT_WASM=/tmp/rc-stage2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/rc-baseline/boot/main.tw -o /tmp/rc-out-after.wasm
cmp /tmp/rc-out-before.wasm /tmp/rc-out-after.wasm && echo "BYTE-IDENTICAL"
```

Expected: `BYTE-IDENTICAL`. If they differ, STOP: `own`/`valid` diverged in shell-only mode — some shell computation reads `field_own`/`path_prov` after all. Use `TWINKLE_FIXVERIFY` (next step) to localize, and reconsider the field split (the fallback is to keep `field_own` full and skip only `path_prov`).

- [ ] **Step 8: `TWINKLE_FIXVERIFY` build (hard accept gate)**

```bash
TWINKLE_FIXVERIFY=1 BOOT_WASM=/tmp/rc-stage2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/rc-verify.wasm
echo "exit=$?"
```

Expected: exit 0, no `fixverify mismatch` trap (the reuse cache's full fx still equals a fresh recompute).

- [ ] **Step 9: Full boot suite + measure**

```bash
target/twk test
TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/rc-stage2.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/rc-baseline/boot/main.tw -o /tmp/rc-t.wasm 2>&1 \
  | grep -E "produce_mutable_decisions|time:mutable:artifacts\]|fixpoint_validated.*summary:link"
```

Expected: all tests pass; `[time:mutable:artifacts] summary=` and `[time] produce_mutable_decisions` drop vs the ~11.7s / ~13.3s baseline. Record `summary:link` total. If the win is negligible (<~5%), the field joins were cheap relative to the transfer computation; note it and proceed to Lever 2 (which does not depend on this win).

- [ ] **Step 10: Commit**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: run loop-seed validation reruns shell-only"
```

---

### Task 2 — Lever 2: warm-start reruns

**Files:**
- Modify: `boot/compiler/ownership.tw`

Stacks on Task 1. Each rerun currently rebuilds every block's state from empty. Because removing a seed only *lowers* uniqueness (monotone), the prior iteration's `vfx` is a valid over-approximate starting point; re-converging from it takes far fewer rounds than a cold restart.

- [ ] **Step 1: Let `run_fixpoint` accept a warm-start seed state**

Add a trailing optional prior-exits parameter to `run_fixpoint`:

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
  shell_only: Bool,
  warm: FixResult?,
) FixResult {
```

Replace the initial-state construction (the block that builds `exits`/`exit_valid`/`exit_prov`/`exit_field_own`/`exit_path_prov` as empty and the per-block init loop) so that when `warm` is `.Some(w)` the exit dicts start from `w` (and `processed` starts all-true so joins see the warm predecessors), else start empty as today:

```tw
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
  warm_started := case warm {
    .Some(_) => true,
    .None => false,
  }
```

Then, in the existing per-block initialization loop, only initialize an exit dict for a block when it is not already present from the warm state, and set `processed[blk.id.id] = warm_started` (so warm predecessors contribute on the first sweep):

```tw
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

- [ ] **Step 2: Update Task-1 call sites to pass `.None`, and warm-start the reruns**

In `run_fixpoint_validated`, the first (`vfx`) pass and the full final (`fx`) pass pass `.None`; each rerun passes the prior `vfx` as warm:

```tw
  vfx := run_fixpoint(blocks, params, table, b, sem, suppress, unique_seed, seeds, label, true, .None)
  reruns := 0
  stable_seeds := false
  for !stable_seeds {
    kept := retain_valid_loop_seeds(blocks, vfx, seeds)
    if same_loop_seed_set(kept, seeds) {
      stable_seeds = true
    } else {
      seeds = kept
      reruns = reruns + 1
      vfx = run_fixpoint(blocks, params, table, b, sem, suppress, unique_seed, seeds, label, true, .Some(vfx))
    }
  }
  fx := run_fixpoint(blocks, params, table, b, sem, suppress, unique_seed, seeds, label, false, .None)
```

Update the other two `run_fixpoint` callers (the three `ForwardState`-materialize paths do **not** call `run_fixpoint`; only these two in `run_fixpoint_validated` exist) — both already updated above. Also add the trailing `, .None` to any `run_fixpoint(` call the Task-1 build left without the `warm` argument.

- [ ] **Step 3: Format, lint, build**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
target/twk build boot/main.tw -o /tmp/rc-stage2b.wasm
```

Expected: fmt/lint clean; build writes `/tmp/rc-stage2b.wasm`.

- [ ] **Step 4: Byte-identical output (hard accept gate)**

```bash
BOOT_WASM=/tmp/rc-stage2b.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/rc-baseline/boot/main.tw -o /tmp/rc-out-b.wasm
cmp /tmp/rc-out-before.wasm /tmp/rc-out-b.wasm && echo "BYTE-IDENTICAL"
```

Expected: `BYTE-IDENTICAL`. **If they differ, the risk is the widening machinery:** `run_fixpoint` forces convergence via `locked_own`/`changed_visits`/`fixpoint_widen_cap`, which is reset per call. A warm start reaches the fixpoint in fewer changes, so widening may trigger differently and land on a different (still sound but not identical) fixpoint. STOP and either (a) also carry `changed_visits`/`prev_exits` forward in the warm state, or (b) restrict warm-start to functions where widening never fires (report `rounds` vs `fixpoint_widen_cap`), or (c) drop Lever 2 and keep Lever 1. Do not accept a non-byte-identical result.

- [ ] **Step 5: `TWINKLE_FIXVERIFY` + full suite + measure**

```bash
TWINKLE_FIXVERIFY=1 BOOT_WASM=/tmp/rc-stage2b.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/rc-verify-b.wasm; echo "exit=$?"
target/twk test
TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/rc-stage2b.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/rc-baseline/boot/main.tw -o /tmp/rc-tb.wasm 2>&1 \
  | grep -E "produce_mutable_decisions|time:mutable:artifacts\]|fixpoint_validated.*summary:link"
```

Expected: FIXVERIFY exit 0, all tests pass, `summary=` / `produce_mutable_decisions` drop further vs Task 1. Record `summary:link` total and its `reruns` cost.

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
BOOT_WASM=/tmp/rc-stage2b.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/rc-stage3.wasm
cmp /tmp/rc-stage2b.wasm /tmp/rc-stage3.wasm && echo "SELF-HOST STABLE"
```

Expected: `SELF-HOST STABLE`.

- [ ] **Step 2: A/B timing (3 runs each, fixed input)**

```bash
for i in 1 2 3; do TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/rc-before.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build /tmp/rc-baseline/boot/main.tw -o /tmp/ab-b-$i.wasm 2>&1 | grep -E "produce_mutable_decisions|time:mutable:artifacts\]"; done
for i in 1 2 3; do TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/rc-stage2b.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build /tmp/rc-baseline/boot/main.tw -o /tmp/ab-a-$i.wasm 2>&1 | grep -E "produce_mutable_decisions|time:mutable:artifacts\]"; done
```

- [ ] **Step 3: Record in `docs/plans/performance/compiler.md`**

Append a section `## Update: loop-seed rerun cost` with: the mechanism (validation reads only `own`+`valid`; reruns skip `field_own`/`path_prov` and warm-start from the prior fx), the before/after `produce_mutable_decisions` + `[time:mutable:artifacts]` lines from Step 2, the `summary:link` total/reruns delta, and acceptance (byte-identical, FIXVERIFY clean, self-host stable, full suite green). Attribute the split between Lever 1 and Lever 2.

- [ ] **Step 4: Clean up and commit**

```bash
git worktree remove /tmp/rc-baseline --force
git add docs/plans/performance/compiler.md
git commit -m "docs: record loop-seed rerun cost reduction"
```

---

## Risks and Stop Rules

- **Lever 1 not byte-identical (Task 1 Step 7):** a shell computation reads `field_own`/`path_prov`. Localize with `TWINKLE_FIXVERIFY`. Fallback: keep `field_own` full and skip only `path_prov` (the heavier, triple-nested field) — guard only `set_path_prov`/`clear_path_prov` and the `join_entry_path_prov` site. Never accept a non-byte-identical result.
- **Lever 2 not byte-identical (Task 2 Step 4):** widening-state reset interacts with the warm start (see Step 4 options). Lever 1 stands on its own; Lever 2 may be dropped without losing Lever 1's win.
- **Negligible Lever 1 win:** the per-instruction `rf`/`pp` computation (not the entry joins/storage) dominates the field cost; the choke-point no-op doesn't skip that. Note it and lean on Lever 2, or extend Lever 1 to guard the per-op field computations behind `shell_only` (larger change, same byte-identity gate).
- **Memory:** warm-start reuses the prior `vfx` dicts by reference; ensure no aliasing corruption (the round loop rebinds exit entries via `merge_targeted`, which builds new maps — verify no in-place mutation of the passed-in warm dicts survives into the returned `fx`). The byte-identity + self-host gates cover this.

## Expected Outcome

Lever 1 removes the field-granular work from ~14/15 of each slow function's fixpoint passes; Lever 2 collapses each rerun from a cold full sweep to a short re-convergence. Together they should take a large bite out of the ~11.7s summary stage (dominated by `link`'s 15 passes), with the emitted program byte-identical and both `FIXVERIFY` and self-host gates green.
