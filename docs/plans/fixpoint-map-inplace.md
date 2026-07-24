# Making the Ownership Fixpoint's Own Maps Mutate In-Place

**Status:** Investigation plan (root cause not yet pinned; the deliverable of Phase 0 is the pinned cause + a go/no-go, not code)

**Goal:** Get the compiler's hottest analysis loop — the ownership fixpoint in `run_fixpoint` — to emit in-place dict mutation (`dict$set_in_place`) for its own loop-carried maps, instead of persistent HAMT `dict$set`. Because the compiler is self-hosted, this makes the compiler itself faster.

**Why this doc exists:** the in-place codegen (sound-uniqueness Phases 8A/8B/8D/8E/8F) is landed on `main` and *works* — but it does **not** fire on the fixpoint's own maps, which fall back to persistent because the analysis marks them `aliased shell`. The fix is an **analysis-precision** problem, and we don't yet know the exact poisoning point. This plan front-loads the diagnosis before any code.

---

## Context (self-contained)

### Where we are

The loop-seed rerun cost in `run_fixpoint_validated` was just cut ~2× by incremental re-propagation (merged; `docs/plans/archive/incremental-repropagation.md`, results in `docs/plans/performance/compiler.md`). After that, the dominant compile-time cost is the summary stage:

```text
[time:summary:roots] total≈6.86s … run≈6.78s funcs=3841 roots=464 wanted=3194
[time:summary:scc]   first=<big fn> total≈50ms   (one large single-member SCC's fixpoint)
```

`summary:roots run` is the `for scc in sccs { run_scc(...) }` loop in `summary.tw:1388` — running the ownership fixpoint (`run_fixpoint_validated` → `run_fixpoint`) over ~3194 wanted functions. It is now *the* lever.

### The self-hosting insight

The boot compiler is a Twinkle program compiled by itself. So an optimization that makes **owned-collection code emit in-place mutation** speeds up any Twinkle program that does dict-heavy owned updates — **including the compiler's own ownership fixpoint.** This is not a generated-code-only win; it is a compiler-speed win via self-hosting.

### What the fixpoint does, and why dicts dominate

`run_fixpoint` (`ownership.tw`) maintains **14 loop-carried maps** (`exits`, `exit_valid`, `exit_prov`, `exit_field_own`, `exit_path_prov`, `prev_exits`, `prev_exit_valid`, `prev_exit_prov`, `locked_own`, `locked_valid`, `locked_prov`, `prev_seen`, `changed_visits`, `processed`), most `Dict<Int, Dict<Int, T>>`. Every round, for every block, it does `nested_get` reads, a `merge_targeted` (which builds a new dict), and ~5 `exits[blk.id.id] = …` outer-dict index-sets. Each persistent `dict$set` rebuilds a HAMT spine. This runs over 243 blocks × several rounds × 3194 functions.

Profile of `run_fixpoint_validated` (instrumented run, since reverted), summed over all calls:

```text
total funcs analyzed: 3875
  reruns==0:          2256 (58%)
  reruns>0:           1619
sum stabilize (pass1+reruns): 3337 ms
sum final cold pass:          2045 ms
   of which reruns==0 (provably redundant, same seeds/both cold): 360 ms
   of which reruns>0  (needed: incremental vfx may differ off-seed):1685 ms
```

Two takeaways:
1. The **algorithmic** redundancy is small — only ~360 ms (the reruns==0 final pass duplicates pass 1). Worth a cheap win (see Appendix A) but not the prize.
2. The real ~5.4 s is **genuine fixpoint computation dominated by persistent dict churn** — exactly what in-place would cut.

### The empirical finding: in-place works, but not here

The in-place machinery is real and fires broadly — the emitted `boot.wasm` has **350 `set_in_place` ops** vs 342 persistent `rt_dict__set`. Per-site decisions (`target/twk ir boot/main.tw --census --sites`) show:

- **Works for a simple intraprocedural loop:** `join_entry_ownership` builds its `entry` dict in a loop →
  `L1417 = update L1354 base=reuse(unique)` → **`dict$set_in_place selected`**, proof id
  `phase8b-loop:join_entry_ownership:carry L1354:site L1417:depth 1`.
- **Fails for the fixpoint:** all **30** dict-set sites in `run_fixpoint`, spanning **all 15** distinct base maps, are
  `base=persistent(aliased shell)` — **zero** in-place. Same for `merge_targeted` (3/3 aliased) and
  `join_entry_ownership_assumed` (`absent` decision → persistent).

Boot-wide, the `dict_set` verdict splits almost exactly in half:

```text
341  base=persistent(aliased shell)
338  base=reuse(unique)
```

So half of all dict-sets are blocked by *aliased shell*, and the fixpoint is the hottest cluster of them.

### Why — and it matches the sound-uniqueness worked examples

The contrast is the tell:
- `join_entry_ownership` — dict built and updated **locally** → in-place.
- `join_entry_ownership_assumed` — takes that dict **across a call boundary** and re-updates → aliased / absent.
- `run_fixpoint` — threads 14 maps through ~10 helpers per block-visit (`join_entry_*`, `nested_get`, `merge_targeted`, `forward_block`, `same_map`, `seed_param_*`, …) → every map reads as escaped → aliased shell.

This is `docs/plans/sound-uniqueness/analysis/worked-examples.md` **Case W (transport-wrapper)** and **cross-cutting finding #6** verbatim: *"Thin-wrapper, transport-wrapper, variant-wrapper, and recursive summaries are on the critical path."* The gap is **interprocedural summary precision at the helper boundary** — the analysis can't yet say "this helper *borrows* its map param and returns a *fresh/unique* value," so the caller conservatively treats the map as aliased. Codegen is not the problem; it already does the right thing when handed a unique proof.

`aliased shell` is a **sound** verdict — mutating a genuinely aliased dict in place corrupts the other alias (worked-examples Case C, the linearity hinge). So the fix is to **prove non-aliasing more precisely**, never to force in-place past the verdict.

---

## Hypotheses (to confirm or kill in Phase 0)

Ranked most→least likely:

1. **Conservative callee summaries for read-mostly helpers.** `join_entry_ownership_assumed`, `join_entry_valid`, `join_entry_prov`, `nested_get`, `same_map`, `forward_block`, `merge_targeted` take a map param and read it; if their summary says the param is *published* (or the return *aliases* a param) rather than *borrowed / fresh-unique*, every caller map is poisoned. **Primary suspect.**
2. **Return double-embedding (a self-inflicted, possibly cheap contributor).** `run_fixpoint` now returns both `FixResult.{ exits, … }` and `FixState.{ exits, … }` sharing the **same** five exit-map objects (introduced by the incremental-re-propagation change). Publishing one map into two aggregates aliases it. This can explain at most the 5 exit maps — **not** the other 9 (which are single-embedded yet still flagged), so it is secondary, but it is a quick experiment.
3. **`merge_targeted`'s `out := next` alias.** `out := next; out[k] = …` where `next` is a param passed the caller's `st.own`; if `st.own` is not proven dead-after-call, `out`/`next` is aliased. Localized to the merge helpers.
4. **Inherent breadth.** The maps are read by so many distinct helpers that no single summary change suffices, and the true fix is a broader borrow/uniqueness precision pass. (Worst case; determines whether this is a small or large effort.)

---

## Phase 0 — Diagnose (no code changes to emission; the deliverable is a pinned cause + go/no-go)

**Files/tools:** `target/twk ir <entry> --cfg` (renders ownership verdicts & summaries), `target/twk ir boot/main.tw --census --sites`, `target/twk wat boot/main.tw --func <name> --calls`, `boot/compiler/summary.tw`, `boot/compiler/ownership.tw`, `boot/compiler/codegen/ownership_verdicts.tw`, `boot/compiler/census.tw`.

- [x] **Step 1 — Trace one map to its poisoning point.** Done. `exits` = **L1899** (`case warm_state { .Some(w) => w.exits, .None => Dict.new() }`). The analysis blames **the helper calls that read/thread the map**, not the writes or the init: `nested_get(exits,…)`, `join_entry_ownership_assumed(_, exits,…)`, the `join_entry_*` family, `merge_targeted`, plus the return double-embed. All 30 dict-set sites are `persistent(aliased shell)`.

- [x] **Step 2 — Read the summaries of the threaded helpers.** Done (baseline, real analysis output): `nested_get p0=Published ret=shared`; `join_entry_ownership_assumed p1=Published ret=shared`; `join_entry_valid/prov p1=Published`; `merge_targeted p0/p1/p2/p3/p6=Published, ret_paths=.f0=from(p1) .f1=from(p3)`; `is_processed p0=Published ret=shared` (its **sole** op is the dict read). Contrast: `join_entry_ownership`'s in-place win is on its *own fresh* `entry` local, not the `exits` param.

- [x] **Step 3 — superseded by the broader Step-1/2 finding.** The double-embed (Hyp 2) is real (the 5 exit maps go into both `FixState` L2510 and `FixResult` L2511) but **redundant**: a stronger source probe (see Findings) makes `nested_get` fully borrow, flipping `join_entry_*` map params to `Borrowed` — yet **zero** run_fixpoint sites flip, because the maps stay published by `merge_targeted` + `is_processed` + the return double-embed. No single change flips anything ⇒ Hyp 4 (breadth).

- [ ] **Step 4 — Quantify the ceiling.** NOT DONE. Baseline captured: `summary:roots run = 6593 ms` (total 6672 ms, wanted=3194). A faithful ceiling needs the forced-in-place probe, which requires **rebuilding `target/twk`** (see methodology gotcha in Findings) — deferred, given the fix is now clearly deep (low ROI as a standalone win).

- [x] **Step 5 — Write up + go/no-go.** See **Phase 0 Findings** below. Recommendation: **(A-deep) / lean (C)** — fold into the sound-uniqueness track, not a standalone quick win.

---

## Phase 0 Findings (2026-07-24)

**Pinned cause — Hypothesis 4 (breadth), rooted in read-op return-provenance.** `run_fixpoint`'s loop-carried maps are marked `aliased shell` because the analysis publishes a `Dict` param whenever a value read out of it flows to a helper's return. Every fixpoint read helper does this: `nested_get`/`lat_get` return the interior (a genuine reference alias for `Dict<Int,Dict<…>>`); `is_processed`/`fact_of`/`valid_of_local` return a value extracted from the dict via `.get` and the analysis conservatively treats even that as aliasing the receiver (`p0=Published, ret=shared`). The maps are published through **multiple independent routes**: (1) the read-helper family, (2) `merge_targeted`'s return aliasing its `next`/`locked` map params, (3) the `FixState`+`FixResult` return double-embed. Because any one route suffices, fixing one flips nothing.

**Decisive evidence (valid source-body probes; `target/twk` re-analyzes edited *function bodies*):**
- Break `nested_get`'s return-alias (copy the inner dict): its summary → `ret=fresh` but `p0` **stays Published** (it still reads `m` via `.get`); run_fixpoint unchanged.
- Make `nested_get` ignore `m` entirely: its summary → `p0=Borrowed ret=fresh`, and the `join_entry_*` map params **do** flip to `Borrowed` — but run_fixpoint stays 30/30 aliased and boot-wide stays 341 aliased / 338 reuse. ⇒ the maps are re-published elsewhere; single-leaf fixes are insufficient.

**Methodology gotcha (important, cost me several inert probes):** `target/twk ir --census/--cfg` computes ownership with `target/twk`'s **already-compiled** analysis logic; it only treats `boot/main.tw` as *input source*. So editing **analysis logic** (`opt/semantics.tw` classifications, `ownership.tw` transfer/`publish_call`) has **no effect** on the output until `target/twk` is rebuilt (`make bundle-cli`). Only edits to the **analyzed functions' bodies** (e.g. `nested_get`) are valid no-rebuild probes. This is why classifying `dict$get` as `ReadOnly` and neutering `publish_call` appeared to "do nothing" — the probes were inert, not disproven. Testing any analysis-logic fix (or the forced-in-place perf ceiling) **requires a rebuild**.

**Go/no-go recommendation:** This is **not** the small targeted-summary tweak Hyp 1 hoped for. The real lever is read-op return-provenance precision — proving that a value read out of a `Dict` (a scalar, or a nested sub-container) does **not** force the `Dict` to be treated as aliased (worked-examples **Case W / path liveness**) — applied across the whole read-helper family *simultaneously*, plus a source restructure for the return double-embed. That belongs in the **sound-uniqueness analysis track** (`docs/plans/sound-uniqueness/`), gated by its round-trip/equivalence guards. As a standalone effort the ROI is low and the fix is deep ⇒ lean **(C) defer / (A-deep) fold into sound-uniqueness**, not a quick win. Perf ceiling remains unquantified (needs the rebuild-based forced probe if the deeper work is greenlit).

---

## Later phases (shape only — do not expand until Phase 0 picks A/B/C)

- **If (A):** a focused analysis-precision change in the summary computation (`summary.tw` / the ownership summary in `ownership.tw`) to give the fixpoint helpers borrow-param / fresh-return summaries, gated by: no `aliased shell → reuse(unique)` flip on any site that is *actually* aliased (soundness — verify with the existing in-place equivalence/round-trip guards used by 8D/8E, `TWINKLE_FIXVERIFY`, self-host, full suite), then measure `summary:roots run`. This overlaps the sound-uniqueness analysis track; coordinate with `docs/plans/sound-uniqueness/analysis/`.
- **If (B):** the specific source restructure, gated by census flip + behavioral equivalence (in-place changes emitted bytes, so **byte-identity is not the gate here** — the guard is the 8D/8E-style round-trip/equivalence check + self-host + suite) and a `summary:roots run` measurement.

### Required exit condition for any later fix

The red read-helper regressions are only the micro gate. A later fix is not done until the **original target in the compiler itself** also reports in-place mutation after rebuilding `target/twk`:

```bash
make bundle-cli
target/twk ir boot/main.tw --census --sites | rg -n "^run_fixpoint\t|^merge_targeted|^join_entry_ownership_assumed"
```

Acceptance requires the current compiler report to show the targeted `run_fixpoint` loop-carried map updates (and any selected helper sites in scope for that fix) flipping from `dict$set` / `absent_fallback` / `persistent(aliased shell)` to `dict$set_in_place` / `selected` / `base=reuse(unique)`. If the microfixtures pass but `run_fixpoint` stays persistent, the fix is incomplete; return to Phase 0-style tracing for the remaining publishing route.

---

## Global constraints & gotchas

- **Byte-identity is *not* the acceptance gate for this work.** Turning a `dict$set` into `dict$set_in_place` changes the emitted WASM by design. The gate is **behavioral equivalence** (the round-trip/equivalence guards the 8D/8E slices already use), `TWINKLE_FIXVERIFY` clean, self-host stable, and the full boot suite green — plus a real `summary:roots run` improvement.
- **`aliased shell` is sound; never force past it.** The goal is a more precise *proof* of non-aliasing, not suppressing the verdict. A wrong in-place here silently corrupts ownership analysis of the program being compiled.
- **Codegen is already correct.** Do not touch emission — `join_entry_ownership` proves in-place fires end-to-end when handed a unique proof. The work is upstream, in summaries/facts.
- **Boot-compiler-only.** No stage0 (`src/`) changes expected.
- After editing `.tw`: `target/twk fmt <file>` then `target/twk lint boot/main.tw` (must be `No findings.`).
- Heavy verification (suite, self-host, timed runs) runs one at a time, never backgrounded. Note: this shell treats `status` as read-only — capture exit codes in a var named `rc`.

## Appendix A — the ~360 ms reruns==0 final-pass elision (independent, cheap)

Separate from the in-place work: when `reruns == 0`, `stabilize_seeds` already computed the cold fixpoint over the (unchanged) stable seed set as pass 1, so `run_fixpoint_validated`'s final cold pass recomputes an identical `fx`. Returning `stabilize_seeds`'s pass-1 `fx` directly for the `reruns == 0` case is provably byte-identical and saves ~360 ms. (For `reruns > 0`, the incremental `vfx` may differ off-seed, so the cold final pass stays.) Small, safe, and orthogonal — take it opportunistically, but it is not the prize.

## References

- `docs/plans/performance/compiler.md` — perf history incl. incremental re-propagation results.
- `docs/plans/archive/incremental-repropagation.md` — the just-landed rerun optimization.
- `docs/plans/sound-uniqueness/analysis/worked-examples.md` — Case W (transport-wrapper) & finding #6, the pattern this matches.
- `docs/plans/sound-uniqueness/README.md` — the analysis/codegen track this precision work belongs to.
- Verdict/census tooling: `boot/compiler/census.tw`, `boot/compiler/codegen/ownership_verdicts.tw`.
