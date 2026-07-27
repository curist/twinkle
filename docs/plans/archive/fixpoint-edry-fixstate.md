# run_fixpoint Caller-Side: Own Carriers for merge_targeted (E-DRY / FixState)

> **ARCHIVED (2026-07-27) — NOT implemented.** The caller-side restructure does not flip the maps
> (spike disproved it; `ForwardState` is Published through the transfer tree). Superseded by
> [`sound-uniqueness/`](../sound-uniqueness/README.md); the finding lives in
> [`sound-uniqueness/storage/README.md`](../sound-uniqueness/storage/README.md) (S4 customer). Kept for the record.

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

> **⚠️ This plan is NOT being implemented — its Task 1 spike showed the restructure does not flip the
> maps. The caller restructure (Tasks 3+) does not help: the maps are non-unique because the transfer
> functions (`seed_payload_binding`/`forward_block`) publish the `ForwardState` that holds them, so the
> maps are Shared before the merge. Full decision: `docs/plans/fixpoint-map-inplace.md` → CONCLUSION.
> The `merge_targeted` body rewrite (Task 2) is still landable standalone.**

**Architecture (pre-spike hypothesis — did not hold):** the hypothesis was that the only blocker is
`run_fixpoint` passing `st.own`/`st.valid`/`st.prov` as projections of a live `ForwardState`, so moving
each map out of `st` as a single-reader last-use would make the argument Unique. Task 1 showed it does
not: `arg_unique=false` persisted, because the `ForwardState` is already Published by the transfer
functions it threads through (see SPIKE RESULT).

**Tech stack:** Twinkle self-hosted compiler (`boot/`), `make bundle-cli`, `target/twk
ir --census/--cfg`, `TWINKLE_FIXVERIFY`, `TWINKLE_TIMINGS`, boot suite.

---

## Pre-spike blocker hypothesis (disproven)

The original hypothesis was that `run_fixpoint` lost uniqueness because it read each map twice from a
live `ForwardState` (`next_own := st.own`, then `merge_targeted(..., st.own, ...)`). The planned fix
was to extract each map once and pass that single-reader, last-use local to `merge_targeted`.

Task 1 tested that shape and disproved it: even with every `st.*` field pre-extracted and `st` dead
before the merge calls, `uniform_entry_seeds` still reported `arg_unique=false` for all three
`run_fixpoint → merge_targeted__{Int,Bool,Vec_Int}` sites. The real blocker is the one recorded below:
`ForwardState` is already Published by the transfer functions before the merge region.

---

## SPIKE RESULT (2026-07-27): failed; root cause found. See the CONCLUSION in `fixpoint-map-inplace.md`.

Task 1 was run inline (body rewrite + full merge-region restructure: every `st.*` field pre-extracted,
`next_own := st.own` as `st`'s last use, `st` dead after, merges read the locals). Self-host green. The
`merge_targeted` body rewrite worked (summary → `p1=Consumed paths{[]}`), but the flip did **not**
happen — instrumenting `uniform_entry_seeds` showed `arg_unique=false` at all three
`run_fixpoint → merge_targeted__{Int,Bool,Vec_Int}` sites even for the last-read `own` map out of a
dead `st`.

**Root cause:** the maps are **not** aliased to `exits` (the joins are fresh, `ret=fresh`). They are
non-unique because the `ForwardState` holding them is threaded through `seed_payload_binding`
(`p_st=Published, ret=alias`) and `forward_block` (`p_st=Published, ret=alias`) — the analysis marks
that record, and its map fields, **Published**, so the map is Shared at the merge call.

**Verdict:** achievable in principle (an analysis-depth limit, not a soundness wall) but not worth it —
flipping it needs the core transfer functions to preserve `ForwardState` field-ownership (a large,
delicate refactor) for a bounded, allocation-only win. **This plan is not being implemented.** The full
reasoning and decision live in `docs/plans/fixpoint-map-inplace.md` → "CONCLUSION"; the `merge_targeted`
body rewrite may be cherry-picked standalone. Tasks below are retained only as the record of the
intended approach. All spike code reverted.

## Historical file structure for the abandoned task set

- `boot/compiler/ownership.tw` — the proposed `merge_targeted` body rewrite (`:5180`) and the now-
  disproven merge-region restructure inside `run_fixpoint` (`:6089–6145`). A future attempt would need
  to preserve `ForwardState` field ownership through the transfer functions instead.
- `boot/tests/suites/mutable_produce_suite.tw` — now holds the current-reality guard that asserts the
  boot fixpoint maps stay persistent today.
- `docs/plans/` — closeout context (this file, `fixpoint-map-inplace.md`,
  `owned-variant-codegen-handoff.md`, `README.md`).

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

## Superseded Task 3 — merge-region restructure

Task 3 would have landed the single-reader merge-region restructure for all three maps. Task 1 proved
that shape does not make the caller arguments Unique, so the task is intentionally omitted. Do not
revive it without first addressing `ForwardState` publication through `seed_payload_binding` /
`forward_block`.

---

## Superseded task tail

The remaining pre-spike tasks that would have split the old tracked marker and landed the caller
restructure are intentionally omitted here. They were based on the disproven single-reader/liveness
hypothesis and conflict with the current verdict: `merge_targeted__` and `run_fixpoint` are guarded as
persistent today, not tracked as red in-place targets. Any future attempt should start from the
`ForwardState` publication blocker in the conclusion, with fresh profiling and a new plan.
