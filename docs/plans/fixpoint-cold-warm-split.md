# Cold/Warm `run_fixpoint` Split — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use `superpowers:subagent-driven-development`
> or `superpowers:executing-plans` to implement this task-by-task. Steps use checkbox
> (`- [ ]`) syntax. This is a compiler-analysis change: the "tests" are census probes,
> `TWINKLE_FIXVERIFY`, the self-host fixed point, and the boot suite — not unit tests.
> Read `docs/plans/fixpoint-map-inplace.md` first for the full diagnosis; this plan
> implements **Lever E** from it.
>
> **Hot code warning:** every edit here is inside the compiler's hottest analysis loop.
> Do not paraphrase the deltas below — apply them verbatim, rebuild, and gate on the
> census before continuing.

**Goal:** Make `run_fixpoint`'s loop-carried map updates emit `dict$set_in_place`
instead of persistent `dict$set`, by giving the two statically-cold callers map *locals*
that are provably fresh `Dict.new()` — which fires the **already-landed Phase 8B**
loop-carried in-place path (the probe's flipped rows carried the
`phase8b-loop:run_fixpoint:carry` proof id; no variant machinery is involved).

**Architecture:** `run_fixpoint` today initializes its 14 maps via
`case warm_state { .Some(w) => w.<map>, .None => Dict.new() }` — joining a cold
`Dict.new()` (Unique) arm with a warm `FixState`-loaded (Unknown) arm — so the ownership
pass summarizes the maps `Unknown` and refuses in-place. The maps must stay **locals**
(param maps are never Unique, and a helper returning a fresh `FixState` aggregate is not
an owned-variant candidate — see "Alternatives"). Two forms deliver on current infra,
decided by a perf measurement:
- **E-simplest** — remove the incremental warm re-propagation entirely; `run_fixpoint`
  becomes unconditionally cold. Mostly deletion, no duplication. Costs the incremental
  rerun optimization (measure it is a net win).
- **E-plain** — keep the warm path for the rare incremental rerun; add a duplicate
  `run_fixpoint_cold` for the two statically-cold callers. Keeps both optimizations at
  the cost of a dual-maintained hot loop.

**Tech stack:** Twinkle self-hosted compiler (`boot/`), `make bundle-cli` self-host loop,
`target/twk ir --census/--cfg` ownership probes, `TWINKLE_FIXVERIFY` /
`TWINKLE_SEEDVERIFY` equivalence guards.

---

## Ground truth (verified 2026-07-26; re-confirm before starting)

```bash
# Clean baseline: 30/30 persistent.
target/twk ir boot/main.tw --census --sites \
  | awk -F'\t' '$1=="run_fixpoint" && $2=="dict_set"{print $5}' | sort | uniq -c
# → 30 false

# Boot suite currently reports ONE expected failure (the tracked marker) → exit 1.
target/twk test 2>&1 | tail -2
# → Failed tests: … boot ownership fixpoint maps should produce in-place dict decisions
# → Ran 3258 tests: 3257 passed, 1 failed   (exit status 1)
```

The cold-only probe (re-verified 2026-07-26, full `make bundle-cli`) flips
**28 true / 2 false** with flipped rows reading `base=reuse(unique)` and the
`phase8b-loop:run_fixpoint:carry` proof id; the 2 survivors update `L3593`, the `dirty0`
worklist map (a separate `dirty0` optional join at `ownership.tw:6017`, **out of scope**).

Key locations (re-grep before editing — the file shifts):

- `boot/compiler/ownership.tw`
  - `fn run_fixpoint(` — signature at ~`:5910`; params include `warm_state: FixState?`
    and `dirty0: Dict<Int, Bool>?` (last two).
  - Map initializers — the 14 `case warm_state { … }` blocks at ~`:5934–5993`; then
    `if !warm_started { for blk in blocks { … per-block seed … } }` at ~`:5994–6011`;
    then `all_dirty`/`dirty` from `dirty0` at ~`:6012–6020`.
  - The loop body **uses `params`** at ~`:6054–6055`
    (`seed_param_prov(entry_prov, params)`, `seed_param_own(entry_own, unique_seed, params)`)
    and **mutates `dirty`** at ~`:6046` (`dirty[blk.id.id] = false`) and ~`:6187`
    (`dirty[s] = true`).
  - Return at ~`:6208–6228`: builds `FixState.{…14 maps…}` and
    `FixRun.{ fx: FixResult.{…5 exit maps…}, state: final_state, widened }` — the 5 exit
    maps are embedded in both (the `FixState`/`FixResult` double-embed; harmless to 8B).
  - Callers: `stabilize_seeds` first pass ~`:6249` (`.None,.None`), rerun ~`:6285`
    (warm iff `allow_incremental`), `run_fixpoint_validated` final pass ~`:6370`
    (`.None,.None`). `stabilize_seeds` signature at ~`:6236` has `allow_incremental: Bool`;
    the `SEEDVERIFY` double-stabilize is at ~`:6335–6363`.
- `boot/tests/suites/mutable_produce_suite.tw` — the tracked marker `.test(` at ~`:380`
  asserts **both** `merge_targeted__` and `run_fixpoint` produce a selected in-place dict
  decision (both currently `0`).

**The canonical 14 cold map initializers** (used by both E-simplest and E-plain — apply
verbatim in place of the `case warm_state { … }` blocks):

```tw
warm_started := false
exits: Dict<Int, Dict<Int, Int>> = Dict.new()
exit_valid: Dict<Int, Dict<Int, Bool>> = Dict.new()
exit_prov: Dict<Int, Dict<Int, Vector<Int>>> = Dict.new()
exit_field_own: Dict<Int, Dict<Int, ff.FieldMap>> = Dict.new()
exit_path_prov: Dict<Int, Dict<Int, Dict<Int, Vector<Int>>>> = Dict.new()
prev_exits: Dict<Int, Dict<Int, Int>> = Dict.new()
prev_exit_valid: Dict<Int, Dict<Int, Bool>> = Dict.new()
prev_exit_prov: Dict<Int, Dict<Int, Vector<Int>>> = Dict.new()
locked_own: Dict<Int, Vector<Int>> = Dict.new()
locked_valid: Dict<Int, Vector<Int>> = Dict.new()
locked_prov: Dict<Int, Vector<Int>> = Dict.new()
prev_seen: Dict<Int, Bool> = Dict.new()
changed_visits: Dict<Int, Int> = Dict.new()
processed: Dict<Int, Bool> = Dict.new()
```

and the dirty/worklist locals become an unconditional full sweep:

```tw
all_dirty := true
dirty: Dict<Int, Bool> = Dict.new()
```

`warm_started` is always `false`, so the existing `if !warm_started { … }` per-block seed
block runs unchanged.

---

## Task 0 — Measurement: is incremental re-propagation worth keeping?

E-simplest and E-plain differ only in whether we keep the incremental warm rerun. That
optimization only speeds up seed-stabilization **reruns** (the final pass is already
cold; reruns are chain-depth-bound and zero for singleton SCCs). Measure whether the
in-place win exceeds the incremental win before choosing.

**Files:** Modify (throwaway): `boot/compiler/ownership.tw` — `run_fixpoint` body only.

- [ ] **Step 1: Apply the always-cold probe.** Replace the 14 `case warm_state { … }`
  initializers (~`:5934–5993`) with the canonical 14 cold initializers above, and replace
  the `all_dirty`/`dirty` block (~`:6012–6020`) with the unconditional full-sweep locals
  above. Leave `warm_state`/`dirty0` in the signature (unused) for this throwaway probe.

- [ ] **Step 2: Rebuild + confirm the flip.**

```bash
make bundle-cli 2>&1 | tail -3   # expect: Fixed point reached: stage3 == stage4
target/twk ir boot/main.tw --census --sites \
  | awk -F'\t' '$1=="run_fixpoint" && $2=="dict_set"{print $5}' | sort | uniq -c
```
Expected: `2 false` / `28 true` (matches the probe). If not, stop and re-diagnose.

- [ ] **Step 3: Measure `summary:roots run` vs the clean baseline.**

```bash
# Baseline: on a clean tree (git stash the probe), TWINKLE_TIMINGS=1 build and record.
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/probe.wasm 2>&1 \
  | grep -E 'summary:roots|own:fixpoint'
```
Record probe-vs-baseline `summary:roots run` total. **Decision:**
- probe ≤ baseline (always-cold in-place is a net win) → **E-simplest (Task 1)**.
- probe > baseline (incremental reruns matter more) → **E-plain (Task 2)**.

- [ ] **Step 4: Revert the probe + restore clean `target/twk`.**

```bash
git checkout boot/compiler/ownership.tw
make bundle-cli 2>&1 | tail -1
```
Record the measurement + decision in `docs/plans/fixpoint-map-inplace.md` (Lever E entry).

---

## Task 1 — E-simplest: remove incremental re-propagation, land always-cold

Do this only if Task 0 chose E-simplest. Otherwise skip to Task 2. Land Task 3 (marker
split) in the **same commit** so the suite goes 1-failed → 1-failed cleanly.

**Files:** Modify: `boot/compiler/ownership.tw`, `boot/tests/suites/mutable_produce_suite.tw`.

- [ ] **Step 1: Make `run_fixpoint` unconditionally cold.** In `run_fixpoint`
  (~`:5910`): drop the `warm_state: FixState?` and `dirty0: Dict<Int, Bool>?` params;
  replace the 14 `case warm_state { … }` initializers with the canonical 14 cold
  initializers; replace the `all_dirty`/`dirty` block with the unconditional full-sweep
  locals (both shown in Ground Truth). Leave the loop body, `params` usage, and the
  `FixState`/`FixRun` return untouched.

- [ ] **Step 2: Collapse the incremental rerun in `stabilize_seeds`.** In
  `stabilize_seeds` (~`:6236`): remove the `allow_incremental: Bool` param; delete the
  `warm_state`/`dirty0` locals (~`:6275–6284`); call the now-2-arg-shorter `run_fixpoint`
  at both the first pass (~`:6249`) and the rerun (~`:6285`). Set `warmed: false` in the
  returned `SeedRun` (reruns are now always cold restarts).

- [ ] **Step 3: Delete the `SEEDVERIFY` double-stabilize** (~`:6335–6363` in
  `run_fixpoint_validated`): with no incremental path there is nothing to compare, so
  replace the `if seedverify_enabled() { … cold vs incr … }` block with a single
  `sr := stabilize_seeds(blocks, params, table, b, sem, suppress, cc_suppress, unique_seed, label)`.
  Update the final-pass `run_fixpoint(...)` call (~`:6370`) to drop the trailing
  `.None, .None`. Remove now-unused `seedverify_enabled` references if this was the last one.

- [ ] **Step 4: fmt + lint.**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
```
Expected: idempotent fmt; no new lint findings.

- [ ] **Step 5: Split the marker (Task 3), then rebuild + census gate.**

Apply Task 3's marker split now, then:
```bash
make bundle-cli 2>&1 | tail -3
target/twk ir boot/main.tw --census --sites \
  | awk -F'\t' '$1=="run_fixpoint" && $2=="dict_set"{print $5}' | sort | uniq -c
```
Expected: `2 false` / `28 true`; explain any drift from the probe.

- [ ] **Step 6: Behavioral gates (Task 4), then commit.**

Run Task 4's gates. Then:
```bash
git add boot/compiler/ownership.tw boot/tests/suites/mutable_produce_suite.tw
git commit -m "ownership: make run_fixpoint unconditionally cold so its loop-carried maps mutate in-place

Removes incremental seed re-propagation (measured net win vs the in-place gain);
run_fixpoint maps are now fresh Dict.new() locals, firing the Phase 8B loop-carried
in-place path. merge_targeted__ half of the marker stays red (separate follow-up)."
```

---

## Task 2 — E-plain: duplicate a cold body, keep the warm path

Do this only if Task 0 chose E-plain. Land Task 3 in the same commit.

**Files:** Modify: `boot/compiler/ownership.tw`, `boot/tests/suites/mutable_produce_suite.tw`.

- [ ] **Step 1: Add `run_fixpoint_cold`.** Copy the entire `run_fixpoint` function
  verbatim to a new `run_fixpoint_cold`, then apply exactly this delta to the copy:
  drop the `warm_state`/`dirty0` params from its signature; replace the 14
  `case warm_state { … }` initializers with the canonical 14 cold initializers; replace
  the `all_dirty`/`dirty` block with the unconditional full-sweep locals. Everything else
  (the loop, `params` usage, the `FixState`/`FixRun` return) is byte-identical to
  `run_fixpoint`.

- [ ] **Step 2: Add a mirror-edit guard** immediately above both functions:

```tw
// MIRROR: run_fixpoint (warm-capable) and run_fixpoint_cold share the hot loop body
// verbatim below the map initializers. Any edit to the iteration MUST be applied to
// both. See docs/plans/fixpoint-map-inplace.md.
```

- [ ] **Step 3: Repoint the two statically-cold callers** to `run_fixpoint_cold`:
  `stabilize_seeds` first pass (~`:6249`) and `run_fixpoint_validated` final pass
  (~`:6370`) — each drops its trailing `.None, .None` args. Leave the incremental rerun
  (~`:6285`) calling `run_fixpoint` unchanged.

- [ ] **Step 4: fmt + lint.**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
```

- [ ] **Step 5: Split the marker (Task 3), then rebuild + census gate.**

```bash
make bundle-cli 2>&1 | tail -3
target/twk ir boot/main.tw --census --sites \
  | awk -F'\t' '$1=="run_fixpoint_cold" && $2=="dict_set"{print $5}' | sort | uniq -c
```
Expected: `2 false` / `28 true` on `run_fixpoint_cold`; the warm `run_fixpoint` stays
`30 false` (that is fine — the cold body is what the hot callers now use).

- [ ] **Step 6: Behavioral gates (Task 4), then commit** (message analogous to Task 1,
  s/unconditionally cold/split a cold body/, noting the warm path is retained).

---

## Task 3 — Split the tracked marker (part of the landing commit)

The marker at `mutable_produce_suite.tw` ~`:380` asserts *both* symbols in one test, so
it cannot go green until both flip. Split it so the `run_fixpoint` half lands green while
the `merge_targeted__` half stays the single known-red target.

**Files:** Modify: `boot/tests/suites/mutable_produce_suite.tw`.

- [ ] **Step 1: Replace the single `.test(` (~`:380–388`)** with two tests. Use the new
  cold symbol name for the E-plain path (`run_fixpoint_cold`) or keep `run_fixpoint` for
  E-simplest:

```tw
    .test(
      "boot ownership run_fixpoint maps produce in-place dict decisions",
      fn() {
        produced := try produce_boot_main()
        // E-simplest: "run_fixpoint"; E-plain: "run_fixpoint_cold".
        try assert.is_true(selected_decision_count(produced, "run_fixpoint", "dict_set") > 0)
        .Ok({})
      },
    )
    // Tracked target marker for docs/plans/fixpoint-map-inplace.md — intentionally red
    // until merge_targeted__'s copy-carrier body proves its carrier Unique (needs the
    // owned-variant aggregate-field extension; same gap as E-DRY). Not a regression.
    .test(
      "boot ownership merge_targeted maps produce in-place dict decisions",
      fn() {
        produced := try produce_boot_main()
        try assert.is_true(selected_decision_count(produced, "merge_targeted__", "dict_set") > 0)
        .Ok({})
      },
    )
```

- [ ] **Step 2: fmt.**

```bash
target/twk fmt boot/tests/suites/mutable_produce_suite.tw
```

(The rebuild + suite run happen in Task 4 — do not rebuild separately here.)

---

## Task 4 — Behavioral equivalence + perf acceptance

The census flip is only half the gate. `dict$set → dict$set_in_place` changes emitted
bytes by design, so byte-identity is **not** the gate — behavioral equivalence is.

**Files:** none (verification only).

- [ ] **Step 1: Fixpoint equivalence guard.**

```bash
TWINKLE_FIXVERIFY=1 target/twk build boot/main.tw -o /tmp/fixverify.wasm 2>&1 | tail -5
```
Expected: no `fixverify` trap/mismatch.

- [ ] **Step 2 (E-plain only): seed equivalence guard.** Skip for E-simplest (the
  incremental path and `SEEDVERIFY` were removed).

```bash
TWINKLE_SEEDVERIFY=1 target/twk build boot/main.tw -o /tmp/seedverify.wasm 2>&1 | tail -5
```
Expected: no `seedverify mismatch`.

- [ ] **Step 3: Self-host fixed point.**

```bash
make bundle-cli 2>&1 | tail -3
```
Expected: `Fixed point reached: stage3 == stage4`.

- [ ] **Step 4: Boot suite — expect EXACTLY ONE known failure.** The boot suite is not
  fully green and is not expected to be: the `merge_targeted__` marker stays red by
  design.

```bash
target/twk test 2>&1 | tail -3
```
Expected: `Ran 3258 tests: 3257 passed, 1 failed`, exit status `1`, and the one failure
is **exactly** `boot ownership merge_targeted maps produce in-place dict decisions`. The
`run_fixpoint` marker must now be in the passed set. Any other failure, or a different
failing-test name, is a regression — stop.

- [ ] **Step 5: Rust reference suite (targeted; full run is too slow).**

```bash
cargo test --release ownership 2>&1 | tail -20
```
Expected: pass.

- [ ] **Step 6: Perf — the actual prize.** Confirm a real `summary:roots run`
  improvement, not just a census flip:

```bash
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/stage2.wasm 2>&1 \
  | grep -E 'summary:roots|own:fixpoint'
```
Expected: `summary:roots run` total drops materially vs the pre-change baseline (~6.78s).
Record before/after in `docs/plans/performance/compiler.md`.

- [ ] **Step 7: Doc cleanup.** Update `docs/plans/fixpoint-map-inplace.md` (status →
  `run_fixpoint` LANDED via E-simplest/E-plain; `merge_targeted__` remains the open
  follow-up), remove this plan's row from `docs/plans/README.md`, and move this doc to
  `docs/plans/archive/` per the house rule.

---

## Alternatives considered (do not re-litigate without new evidence)

- **E-DRY — extract `fixpoint_iterate(maps…) FixState` so owned variants seed the maps
  Unique. NOT viable on current infra.** `candidate_variants` (`summary.tw:689`) only
  admits a param when `ret_aliases_exactly_param(s.ret, k)` (`summary.tw:559`), i.e. the
  whole return is `.MayAliasParams([k])`. A helper returning a fresh `FixState.{…}` yields
  `ret=fresh ret_paths=.fN=from(pK)…`, which the candidate model ignores — the identical
  gap that keeps `merge_targeted__` persistent. E-DRY would require extending owned-variant
  candidate/validation/selection to **returned-aggregate fields**; pursue that as the
  shared `merge_targeted__` follow-up, not here.
- **Lever F — seed the never-aliased `FixState?` source Unique and move its fields out.**
  Spiked and deprioritized: the payload-move machinery exists (`seed_payload_binding`,
  `ownership.tw:4280`) but (1) no candidate seeds `warm_state`, (2) the 14-separate-`case`
  shape borrows 13/14 even if seeded, and (3) the cold prize sites pass `.None`, needing
  whole-program never-aliased-param or `.None`-constant specialization — an **explicit
  non-goal** of the sound-uniqueness track (`architecture.md:939`). See
  `docs/plans/fixpoint-map-inplace.md`.

## Out of scope

- The `dirty0`/worklist map (`L3593`) — a separate optional join; the probe left it
  persistent and that is expected.
- `merge_targeted__`'s own body — the copy-carrier / owned-variant aggregate-field
  follow-up (tracked in `fixpoint-map-inplace.md`), not enabled by the cold/warm split.

## Self-review notes

- No placeholders: the 14 map initializers, the dirty locals, and every call-site delta
  are spelled out; the loop body is preserved by "copy verbatim, apply this delta."
- Findings addressed: E-DRY reclassified as not-viable (candidate-model gap, verified at
  `summary.tw:559`); `params` kept in every signature and `dirty` given a concrete local;
  boot-test acceptance states the true state (1 known failure, exit 1) rather than "green".
- E-simplest vs E-plain is decided by measurement (Task 0), not assumption. The honest
  cost of E-simplest is removing a landed optimization; Task 0 gates on it being a net win.
