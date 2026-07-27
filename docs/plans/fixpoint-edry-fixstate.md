# run_fixpoint Caller-Side: Own Carriers for merge_targeted (E-DRY / FixState)

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development`
> (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax. **Read first, in order:**
> `docs/plans/owned-variant-codegen-handoff.md` → "⛔ SPIKE RESULT (2026-07-27)" (why merge_targeted
> can't flip today), then `docs/plans/fixpoint-map-inplace.md` (the umbrella goal). This plan is the
> **caller-side prerequisite** that the spike identified: make `run_fixpoint` hand `merge_targeted`
> its map arguments as **owned, moved-out values** instead of projections of a live `ForwardState`.

**Goal:** Restructure `run_fixpoint`'s per-block merge region so the `next` argument to
`merge_targeted` (`own`/`valid`/`prov`) is Unique + last-use at the call, so that — with
`merge_targeted` body-rewritten to a clean carrier — its in-place dict update is selected. Proven by
all three `merge_targeted__` monomorphs flipping in the census and the `merge_targeted__` half of the
tracked marker going green, self-host stable, and no ownership-analysis perf regression.

**Architecture:** The spike proved the analysis + body rewrite are sufficient; the only blocker is
that `run_fixpoint` reads `st.own`/`st.valid`/`st.prov` (fields of a live `ForwardState`) and passes
those projections to `merge_targeted`, so uniqueness is never available. This plan (a) lands the
`merge_targeted` body rewrite, then (b) restructures the merge region so each map is **moved out of
`st` exactly once and read only by `merge_targeted`** with `st` dead afterward — making the argument
a Unique last-use. It reuses the existing analysis end-to-end (`uniform_entry_seeds` +
`seed_param_indices`' `target_params` path); **no new analysis, no cloning, no `compute_variants`
consumption.** The exact restructure shape is the load-bearing unknown, so Task 1 is a measured spike.

**Tech stack:** Twinkle self-hosted compiler (`boot/`), `make bundle-cli`, `target/twk
ir --census/--cfg`, `TWINKLE_FIXVERIFY`, `TWINKLE_TIMINGS`, boot suite.

---

## The exact blocker (verified, with line references at HEAD)

`run_fixpoint`'s per-block merge region (`boot/compiler/ownership.tw`):

```
6089    next_own := st.own            // reader #1 of st.own
6090    next_valid := st.valid
6091    next_prov := st.prov
...
6100    own_merge := merge_targeted(old_own, st.own, ...)     // reader #2 of st.own  ← the carrier arg
6111    valid_merge := merge_targeted(old_valid, st.valid, ...)
6122    prov_merge := merge_targeted(old_prov, st.prov, ...)
6133    next_own = own_merge.map      // result stored back
6134    next_valid = valid_merge.map
6135    next_prov = prov_merge.map
```

`st: ForwardState` (`ownership.tw:3511`) bundles `own`/`valid`/`prov`/`field_own`/`path_prov` as
`Dict` fields. Each map is read at least twice off the **same live `st`** (lines 6089 + 6102, etc.),
so `st.own` at the `merge_targeted` call is aliased ⇒ `uniform_entry_seeds` cannot seed
`merge_targeted`'s `p1` Unique ⇒ `persistent(aliased shell)`. Fixing this means giving
`merge_targeted` a single-reader, last-use `next` argument.

**Ground truth to record before starting:**

```bash
# merge_targeted persistent across all three monomorphs today:
target/twk ir boot/main.tw --census --sites 2>/dev/null \
  | grep -E '^merge_targeted__(Int|Bool|Vec_Int)' | grep dict_set
# → all three: ... false ... base=persistent(aliased shell) borrow-effect copy-carrier source ...

# Ownership-analysis timing baseline (this is the hot fixpoint — guard against regressions):
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/stage2.wasm 2>&1 \
  | grep -E 'own:fixpoint|summary:roots|time:mutable:artifacts'
# Record the numbers.

# Pre-existing tracked-red fixverify baseline (measure DELTA, not absolute):
TWINKLE_FIXVERIFY=1 target/twk build boot/main.tw -o /tmp/fv.wasm 2>&1 | tail -1
# → fixverify mismatch: analyze:unique_analysis_diags
```

---

## File structure

- `boot/compiler/ownership.tw` — (1) `merge_targeted` body rewrite (`:5180`); (2) merge-region
  restructure inside `run_fixpoint` (`:6089–6145`). Possibly a small helper if the spike shows a
  clean extraction is what grants uniqueness. No `ForwardState` type change unless the spike proves
  it necessary (prefer the localized restructure).
- `boot/tests/suites/mutable_produce_suite.tw` — split the tracked marker so the `merge_targeted__`
  half goes green.
- `docs/plans/` — closeout (this file, `fixpoint-map-inplace.md`, `owned-variant-codegen-handoff.md`,
  `README.md`).

---

## Task 1 — SPIKE (throwaway): which restructure makes `st.own` Unique at the merge call?

This is a **hot, delicate function**; do not guess the final shape. This spike lands the body rewrite
(needed regardless) then tries the **leading restructure hypothesis** on the `own` map only, measures
whether `merge_targeted__Int` flips, and records what worked. Revert the restructure at the end
(keep nothing uncommitted); the body rewrite is re-landed cleanly in Task 2.

**Leading hypothesis (H1 — single-reader move):** the second read (`st.own` at `:6102`) is the alias.
Bind the map into a local **once**, ensure `st` is dead before the merge, and pass that local. The
pre-read at `:6089` (`next_own := st.own`) is dead in the `already` branch (overwritten at `:6133`),
so hoist it out of the alias set.

- [ ] **Step 1: Apply the `merge_targeted` body rewrite** (`ownership.tw:5180`) — hoist the `keys`
  read above `out := next`, read values via `out`:

```tw
  keys := int_keys_union(old.keys(), next.keys())
  out := next
  next_locked := locked
  for k in keys {
    old_x := lat_get(old, k, default_value)
    next_x := lat_get(out, k, default_value)
    prev_x := lat_get(prev, k, default_value)
    changed := !eq(old_x, next_x)
    oscillates := has_prev and changed and eq(next_x, prev_x)
    if live_contains_int(next_locked, k) or oscillates or force_lock_changed and changed {
      next_locked = insert_sorted(next_locked, k)
      out[k] = join(old_x, next_x)
    }
  }
  MergeOut.{ map: out, locked: next_locked }
```
  (Confirmed by the prior spike to move `merge_targeted`'s summary to `p1=Consumed paths{[]}`.)

- [ ] **Step 2: Try H1 on the `own` map only.** Restructure `ownership.tw:6089–6135` so `st.own` has
  a single last-use reader that feeds `merge_targeted`. Move the `next_own := st.own` bind **inside**
  the `!already` path, and in the `already` path pass the map moved out of `st`:

```tw
        old_own := nested_get(exits, blk.id.id)
        old_valid := nested_get(exit_valid, blk.id.id)
        old_prov := nested_get(exit_prov, blk.id.id)
        old_field := nested_get(exit_field_own, blk.id.id)
        old_pp := nested_get(exit_path_prov, blk.id.id)
        // move own out of st BEFORE any second read; st.valid/st.prov/... still read below,
        // so this spike only proves the `own` map — Task 3 generalizes to all three.
        cur_own := st.own
        next_valid := st.valid
        next_prov := st.prov
        next_field := st.field_own
        next_pp := st.path_prov
        next_own := cur_own
        if already {
          has_prev := prev_seen_get(prev_seen, blk.id.id)
          force_lock_changed := change_count_get(changed_visits, blk.id.id) >= fixpoint_widen_cap
          if force_lock_changed {
            widened = true
          }
          own_merge := merge_targeted(
            old_own,
            cur_own,                       // ← single-reader last-use candidate
            nested_get(prev_exits, blk.id.id),
            locked_get(locked_own, blk.id.id),
            has_prev,
            force_lock_changed,
            own_tag(.Unknown),
            join_own_tag,
            int_eq,
          )
          // ... valid_merge / prov_merge unchanged (still st.valid/st.prov) ...
          next_own = own_merge.map
```
  **Note the likely catch:** `next_own := cur_own` then `merge_targeted(..., cur_own, ...)` reads
  `cur_own` twice again. If H1 fails for this reason, try **H1b:** drop the `next_own := cur_own`
  pre-bind and instead initialize `next_own` only where consumed — i.e. in the `already` branch
  `next_own := own_merge.map`, and in the `!already` branch `next_own := cur_own`. Then in the
  `already` branch `cur_own`'s only reader is `merge_targeted`. Record which of H1/H1b (if either)
  works.

- [ ] **Step 3: Rebuild and measure (own map only).**

```bash
target/twk fmt boot/compiler/ownership.tw
make bundle-cli 2>&1 | tail -2   # must reach stage3 == stage4
target/twk ir boot/main.tw --census --sites 2>/dev/null \
  | grep -E '^merge_targeted__Int' | grep dict_set | head -1
```
  Also confirm the caller side actually got seeded:

```bash
target/twk ir boot/main.tw --cfg 2>/dev/null | grep -A1 '^fn merge_targeted__Int' | grep summary:
# want: p1=Consumed paths{[]} (from Step 1) — and the census row flipped to reuse(unique)
```

**Decision (record the exact winning restructure + census line into this section):**
- **FLIPPED to `reuse(unique)`:** H1/H1b works. Record which, and proceed — Tasks 2–4 land it cleanly
  and generalize to `valid`/`prov`.
- **Still `persistent(aliased shell)`:** the single-reader-move is not enough. Before trying anything
  heavier, dump *why* the arg is still non-unique:
  ```bash
  # Is st itself still live past the merge (so cur_own aliases via st)? Inspect uses of `st`
  # after line ~6145 in run_fixpoint; if st flows onward, cur_own can't be a move.
  grep -n "\bst\b" boot/compiler/ownership.tw | awk -F: '$1>6145 && $1<6260'
  ```
  - If `st` is used after the merge region → the maps can't be moved out while `st` lives; the fix is
    structural: `forward_block` must hand back the maps as separate owned values (or the merge must
    happen before `st` is reused). Escalate to **H2 (structural)** below as a *new* spike step; do not
    proceed to Task 2 on H1.
  - If `st` is dead but the arg is still Shared → the copy-bind uniqueness guard is not crediting the
    move; capture the `--cfg` summary of `run_fixpoint` and open a uniqueness-analysis question. STOP.

- [ ] **Step 4: Record the result, then REVERT the restructure (keep nothing).**

```bash
git checkout -- boot/compiler/ownership.tw
git add docs/plans/fixpoint-edry-fixstate.md
git commit -m "docs: record run_fixpoint merge-region spike result (H1/H1b: <worked|failed>)"
```

> **H2 (structural fallback), only if H1/H1b fails because `st` outlives the merge:** thread
> `own`/`valid`/`prov` as top-level owned locals across the fixpoint loop rather than inside
> `ForwardState` — i.e. `forward_block` returns them separately (a `FixState`-shaped result) and the
> loop passes them into the merge and back. This is the full E-DRY extraction and is a much larger
> change; if the spike lands here, **stop and re-scope this plan around H2** (its own task breakdown),
> because Tasks 2–4 below assume the localized H1 shape.

---

## Task 2 — Land the `merge_targeted` body rewrite (standalone, behavior-preserving)

**Files:** `boot/compiler/ownership.tw` (`merge_targeted`, `:5180`).

This is valuable and safe on its own (moves the summary to `p1=Consumed paths{[]}`), independent of
the caller restructure.

- [ ] **Step 1: Apply the rewrite** — exactly Task 1 Step 1.

- [ ] **Step 2: fmt + self-host (this fn runs inside the compiler).**

```bash
target/twk fmt boot/compiler/ownership.tw
make bundle-cli 2>&1 | tail -2   # MUST reach stage3 == stage4
```

- [ ] **Step 3: Behavioral-equivalence gate.**

```bash
target/twk test 2>&1 | tail -1   # identical to baseline (merge_targeted marker still red — caller not fixed yet)
```
Expected: no test changes vs baseline. If any test other than the tracked marker moves, the rewrite
is not equivalent — revert and stop.

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: hoist merge_targeted next reads so out is unique at the in-place write

Behavior-preserving (union keys unique ⇒ out[k] equals original next[k] until its own
iteration). Moves the generic summary to p1=Consumed paths{[]}, making next a proper
whole-value carrier; the in-place flip still needs a Unique caller (Task 3)."
```

---

## Task 3 — Restructure the merge region so all three maps are Unique last-use args

**Files:** `boot/compiler/ownership.tw` (`run_fixpoint` merge region, `:6089–6145`).

Apply the winning shape from Task 1 (H1 or H1b) to **all three** merged maps (`own`, `valid`,
`prov`), so each is moved out of `st` exactly once and read only by its `merge_targeted` call.

- [ ] **Step 1: Apply the restructure to `own`, `valid`, `prov`.** Use the exact shape Task 1 proved.
  For H1b (the more likely winner), that means: remove the unconditional `next_own := st.own` /
  `next_valid := st.valid` / `next_prov := st.prov` pre-binds, bind `cur_own`/`cur_valid`/`cur_prov`
  once, and set `next_own`/`next_valid`/`next_prov` in each branch (`= *_merge.map` in `already`,
  `= cur_*` in `!already`) so the `already` branch's only reader of each `cur_*` is `merge_targeted`.
  (Paste the concrete winning code recorded in Task 1 here at execution time — do not improvise a
  different shape.)

- [ ] **Step 2: fmt, lint, rebuild, and measure the flip across ALL monomorphs.**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
make bundle-cli 2>&1 | tail -2
target/twk ir boot/main.tw --census --sites 2>/dev/null \
  | grep -E '^merge_targeted__(Int|Bool|Vec_Int)' | grep dict_set
```
Expected: **all three** rows flip to `... true ... reuse(unique)`. If only some flip, the three maps
differ in their alias shape — investigate before proceeding.

- [ ] **Step 3: fixverify delta gate.** The pre-existing `analyze:unique_analysis_diags` mismatch is
  the baseline; the flip must not add a *different* mismatch key.

```bash
TWINKLE_FIXVERIFY=1 target/twk build boot/main.tw -o /tmp/fv.wasm 2>&1 | tail -3
```
Expected: the **same** single `analyze:unique_analysis_diags` line (or fewer). A new key = unsound
in-place mutation of a still-aliased map — STOP and diagnose.

- [ ] **Step 4: Behavioral + perf gate (this is the hot fixpoint).**

```bash
target/twk test 2>&1 | tail -3                    # only the run_fixpoint marker half may remain red
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/stage2.wasm 2>&1 \
  | grep -E 'own:fixpoint|summary:roots|time:mutable:artifacts'
```
Expected: no correctness regressions; `own:fixpoint` time **not worse** than the recorded baseline
(in-place should be neutral-to-faster). A regression means the restructure changed allocation
behavior adversely — investigate.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: pass merge_targeted its maps as owned last-use args in run_fixpoint

Moves own/valid/prov out of the live ForwardState so each is a single-reader, last-use argument
to merge_targeted, letting uniform_entry_seeds seed its p1 Unique. merge_targeted's dict update
now emits in-place across all monomorphs. No analysis change — reuses the existing target_params
+ uniform-caller seed path."
```

---

## Task 4 — Land the `merge_targeted` half of the tracked marker

**Files:** `boot/tests/suites/mutable_produce_suite.tw` (the marker at `:380–388`).

- [ ] **Step 1: Split the marker.** The current single test asserts both `merge_targeted__` and
  `run_fixpoint` flip and fails because neither did. Replace it with two tests — `merge_targeted__`
  (now green, all monomorphs) and `run_fixpoint` (still tracked-red):

```tw
    .test(
      "boot merge_targeted produces in-place dict decisions (all monomorphs)",
      fn() {
        produced := try produce_boot_main()
        try assert.is_true(selected_decision_count(produced, "merge_targeted__Int", "dict_set") > 0)
        try assert.is_true(selected_decision_count(produced, "merge_targeted__Bool", "dict_set") > 0)
        try assert.is_true(selected_decision_count(produced, "merge_targeted__Vec_Int", "dict_set") > 0)
        .Ok({})
      },
    )
    // Tracked target marker for docs/plans/fixpoint-map-inplace.md — the run_fixpoint half.
    // Intentionally red until run_fixpoint's OWN loop-carried maps (exits/locked writes, the
    // dirty0 worklist) are proven Unique. Not a regression.
    .test(
      "boot run_fixpoint maps should produce in-place dict decisions",
      fn() {
        produced := try produce_boot_main()
        try assert.is_true(selected_decision_count(produced, "run_fixpoint", "dict_set") > 0)
        .Ok({})
      },
    )
```

- [ ] **Step 2: fmt + suite.**

```bash
target/twk fmt boot/tests/suites/mutable_produce_suite.tw
target/twk test 2>&1 | tail -3
```
Expected: the `merge_targeted` test PASSES; exactly one known failure remains and it is the
`run_fixpoint` marker (exit 1 expected). Any other failing test → stop.

- [ ] **Step 3: Commit.**

```bash
git add boot/tests/suites/mutable_produce_suite.tw
git commit -m "test: land merge_targeted in-place marker; run_fixpoint half remains tracked-red"
```

---

## Task 5 — Docs closeout

**Files:** `docs/plans/owned-variant-codegen-handoff.md`, `docs/plans/fixpoint-map-inplace.md`,
`docs/plans/README.md`, this file.

- [ ] **Step 1: Record the landing.** In `owned-variant-codegen-handoff.md`, note the caller-side
  prerequisite is done via this plan (uniform-caller seed path, no cloning). In
  `fixpoint-map-inplace.md`, mark the `merge_targeted` half of the boundary cleared; the remaining
  target is `run_fixpoint`'s OWN maps (`exits`/`locked` writes + the `dirty0` worklist), which are a
  separate, narrower follow-up (they are not `merge_targeted` carriers).

- [ ] **Step 2: Commit.**

```bash
git add docs/plans/
git commit -m "docs: merge_targeted flips via owned-carrier run_fixpoint restructure"
```

---

## Out of scope

- **`run_fixpoint`'s own loop-carried maps** (`exits[blk]=`, `locked_*[blk]=`, the `dirty0`
  worklist). Making those in-place is the remaining `run_fixpoint` marker half; it is a *different*
  shape (nested-dict writes / worklist mutation, not `merge_targeted` carriers) and gets its own plan
  once this lands.
- **The H2 structural extraction** (thread all 5 `ForwardState` maps as top-level owned locals via a
  `FixState`-returning `forward_block`). Only pursue if Task 1's spike shows H1/H1b cannot grant
  uniqueness because `st` outlives the merge — in which case re-scope this plan around H2.
- **Field-granular / `field_own` / `path_prov` carriers.** Those two maps are merged via
  `merge_field_own_exit` / `merge_path_prov_exit`, not `merge_targeted`; unrelated.

## Self-review notes

- **Spec coverage:** caller-side blocker → Task 1 spike (proves the restructure) + Task 3 (lands it);
  body rewrite → Task 2; acceptance → Task 3 census (all monomorphs) + Task 4 marker; risk controls →
  Task 3 fixverify-delta + perf gates (hot fixpoint). Docs → Task 5.
- **Why spike-first:** the exact restructure that grants uniqueness (H1 vs H1b vs structural H2)
  cannot be known without measuring — the copy-bind uniqueness guard's behavior on
  `cur_own := st.own` with `st` dead is the unknown. Task 1 measures it on one map before touching
  all three, and has an explicit escalation to H2 with a STOP.
- **No new analysis / no new risk surface:** this plan only changes source shape
  (`merge_targeted` body + the merge region). The seed path (`target_params` → `uniform_entry_seeds`)
  and the verdict pass are unchanged; the prior spike already showed the body rewrite alone yields
  `p1=Consumed paths{[]}`. Every flip is still gated by `uniform_entry_seeds` (all-callers-Unique)
  and the verdict pass (reusable-at-site), plus the fixverify-delta check.
- **Type consistency:** no new types. `merge_targeted` signature unchanged; `run_fixpoint` locals
  `cur_own`/`cur_valid`/`cur_prov` are `Dict<...>` matching `st.own`/`st.valid`/`st.prov`
  (`ForwardState`, `ownership.tw:3511`); `MergeOut.{ map, locked }` result usage unchanged.
