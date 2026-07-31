# Safe 8G→Producer FixResult Reuse — Soundness Investigation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax. **Investigation-first: the naive version is already known to fail `TWINKLE_FIXVERIFY`. Phase 0 decides whether a sound version can exist at all — a null-result exit is an expected outcome, not a failure.**

**Goal:** Determine whether Phase 8G's per-function `FixResult`s (computed inside `summary.compute` and currently discarded) can be reused by the mutable-decision producer for a **provably-characterizable subset** of functions, skipping the producer's re-run of the ownership fixpoint over those functions. If such a subset exists and is cheap to compute, land it; otherwise document why not.

**Architecture:** The naive lever — cache every candidate root's `FixResult` in 8G, pre-seed the producer, skip the all-safe root SCCs — was prototyped and reverted (see [compiler.md](compiler.md), "Null result: reuse 8G's FixResults"). It produced byte-identical output on the boot self-build but `TWINKLE_FIXVERIFY=1` flagged `analyze:unique_analysis_diags`: 8G computes each `FixResult` against the **whole-program** summary table, the producer against a **scoped** table, and a `FixResult`'s dependency footprint is broader than the Summary it projects to — so a function's summary can be soundly reusable while its fix is not. This plan does **not** re-attempt the naive version; it first roots-causes the exact divergence class, then asks whether the safe subset (fixes that provably match under both tables) is cheaply identifiable.

**Tech Stack:** Twinkle self-hosted boot compiler (`boot/compiler/summary.tw`, `ownership.tw`, `codegen/variant_specialize.tw`, `codegen/ownership_verdicts.tw`); `make stage2`; `TWINKLE_FIXVERIFY` recompute-and-compare guard (`ownership.tw:5735`).

---

## Why this is separate and risky

- The sibling plan [2026-07-31-ownership-cfg-liveness-reuse.md](2026-07-31-ownership-cfg-liveness-reuse.md) covers the **scope-independent** structural reuse (CFG, liveness) that is byte-identical *by construction*. This plan is the opposite: FixResult reuse across the 8G→producer boundary is **scope-dependent** and already has a known counterexample.
- The likely outcome (based on the reverted attempt and the analogy to compiler.md rec #1, `call_uniques` reuse) is that the reusable subset has **no cheap static characterization** — the render/scope divergence set is broader than any single call-graph predicate. Phase 0 exists to confirm or refute this quickly, before any implementation effort.
- **Non-negotiable gate:** the deliverable ships only if `TWINKLE_FIXVERIFY=1` is **clean over the whole self-build** (not just byte-identical output on one input — byte-identity held for the reverted version *and it was still unsound*).

---

## Verification harness

- **FIXVERIFY delta (the authoritative gate):** `analyze:unique_analysis_diags` is a
  **pre-existing tracked-red baseline** (it mismatches on plain `main`, flag off,
  even with `TWINKLE_SUMMARY_REUSE=0`; the archived owned-variant plans document it
  as "measure the delta, not the absolute"). Plain `TWINKLE_FIXVERIFY=1` errors on
  that first mismatch and tells you nothing — so this plan **requires the census
  mode** (Phase 0 Step 2): the gate is **the mismatch SET with the flag on equals
  the set with the flag off**, i.e. no NEW mismatch beyond the baseline. A single
  new name = unsound.
- **Byte-identity A/B:** `TWINKLE_8G_FIXREUSE=0` vs `=1`, `cmp`. Necessary but *not
  sufficient* on its own — the reverted prototype was byte-identical; pair it with
  the FIXVERIFY-delta census before trusting.
- Boot suite (3322), `make stage2` fixed point, 3× same-session `[time:summary:reuse]` / `produce_mutable_decisions` A/B.

---

## Phase 0: Root-cause the divergence and test for a cheap characterization

**Files:** `boot/compiler/ownership.tw` (diagnostic-only), `boot/compiler/summary.tw`, `boot/compiler/codegen/variant_specialize.tw`, `boot/compiler/codegen/ownership_verdicts.tw` (re-introduce the reverted lever behind `TWINKLE_8G_FIXREUSE`, default off).

- [ ] **Step 1: Re-introduce the naive lever behind a default-off flag**

Reconstruct the reverted change (documented in compiler.md's null-result bullet and mirrored on the summary-carry threading pattern the sibling plans use): `summary.compute_cached(view, b, sem, cache_ids)` returns `SummaryCacheResult`; `SpecializeResult` carries the candidate-root `FixCache`; `compute_for_roots_reusing` takes a `carried_fix`, pre-seeds safe roots, and skips all-safe root SCCs whose fix is pre-seeded. Gate every use behind `fn fixreuse_enabled()` reading `TWINKLE_8G_FIXREUSE` (default `false`). Do **not** make it default-on at any point in this plan until Phase 2.

- [ ] **Step 2: Convert FIXVERIFY from fail-fast to a full census (diagnostic build)**

In `ownership.tw` where `fixverify_enabled()` errors on mismatch (`ownership.tw:~7233`), add a `TWINKLE_FIXVERIFY_CENSUS` mode that, instead of `error(...)`, records `label` to a process-wide counter/list and continues, printing at end:

```tw
eprintln("[fixverify:census] mismatches=${n} funcs=${names.join(",")}")
```

This yields the *complete* set of divergent functions, not just the first.

- [ ] **Step 3: Enumerate the full divergence set**

Run: `TWINKLE_FIXVERIFY=1 TWINKLE_FIXVERIFY_CENSUS=1 TWINKLE_8G_FIXREUSE=1 target/twk build boot/main.tw -o /tmp/x.wasm 2>&1 | grep '\[fixverify:census\]'`
Record the count and the function list. This is the population the reuse gate must exclude.

- [ ] **Step 4: Classify each divergent function against candidate predicates**

For each function in the census set, instrument `compute_for_roots_reusing` to emit, per divergent func: (a) is it in `unsafe`? (b) does `callee_ids(f, user_ids)` include any `unsafe`/clone-affected id? (c) does its `dependency_closure` include any function whose summary differs between the carried (8G whole-program) table and the producer's scoped table? Compare the census set against each predicate.

**This is the pivotal decision gate:**
- If **every** divergent function is caught by a *cheap* predicate (e.g. "its dependency closure contains an unsafe function" — computable from data already built in `compute_for_roots_reusing`), a sound gate exists → proceed to Phase 1.
- If the divergence set is **not** covered by any cheap predicate (i.e. some functions diverge with no call-graph explanation — the render-resolver situation of rec #1, where the divergence set has no static characterization), **stop**: write the census + the failed predicates into compiler.md as a strengthened null result and close this plan.

- [ ] **Step 5: Commit the diagnostics (flag still default off)**

```bash
git add boot/compiler
git commit -m "8G fixreuse: diagnostic census + predicate classification (default off)"
```

## Phase 1: Define and prove the fix-reuse gate (only if Phase 0 found a predicate)

**Files:** `boot/compiler/summary.tw` (tighten the pre-seed condition in `compute_for_roots_reusing`).

- [ ] **Step 1: Restrict the pre-seed to the proven-safe subset**

Add the Phase-0 predicate as the additional pre-seed condition: a candidate root's 8G `FixResult` is pre-seeded (and its SCC skipped) only when it passes the predicate (e.g. its dependency closure is disjoint from `unsafe`). All other roots are recomputed by the producer exactly as today.

- [ ] **Step 2: FIXVERIFY-delta clean over the whole self-build (census mode)**

```bash
TWINKLE_FIXVERIFY=1 TWINKLE_FIXVERIFY_CENSUS=1 TWINKLE_8G_FIXREUSE=1 target/twk build boot/main.tw -o /tmp/v.wasm 2>&1 | grep '\[fixverify:census\]'
```
Expected: the mismatch set equals the flag-off baseline (`analyze:unique_analysis_diags` only). If any NEW name appears, the predicate is insufficient — return to Phase 0 Step 4 to widen it, or conclude non-viable.

- [ ] **Step 3: Byte-identity A/B + boot suite + stage2**

`cmp` off vs on → IDENTICAL; boot suite green; `make stage2` fixed point.

- [ ] **Step 4: Confirm the win survived the tighter gate**

3× A/B on `[time:summary:reuse]` and `produce_mutable_decisions`. The reverted (unsound) version cut `summary:reuse` ~2999→1339ms; a correct predicate that excludes the divergent minority should keep most of that. **Decision gate:** if the sound subset is so small the win is within noise, revert — the safe reuse is not worth the complexity.

- [ ] **Step 5: Commit**

```bash
target/twk fmt boot/compiler/summary.tw
git add boot/compiler/summary.tw
git commit -m "8G fixreuse: gate pre-seed on the proven fix-reusable subset"
```

## Phase 2: Land it (only if Phase 1 is FIXVERIFY-clean and the win holds)

- [ ] **Step 1: Default the flag on**

Flip `TWINKLE_8G_FIXREUSE` default to on (`=0` = kill-switch), matching `TWINKLE_SUMMARY_REUSE`.

- [ ] **Step 2: Remove the census diagnostic scaffolding** (keep plain `TWINKLE_FIXVERIFY` fail-fast) and re-run all gates once more.

- [ ] **Step 3: Commit + document**

```bash
git add boot/compiler
git commit -m "Enable proven-safe 8G FixResult reuse by default (TWINKLE_8G_FIXREUSE)"
```

## Wrap-up

- [ ] Update [compiler.md](compiler.md): replace the "Null result: reuse 8G's FixResults" bullet with either a "Landed wins" bullet (the proven predicate, A/B numbers, FIXVERIFY-clean acceptance) **or** a strengthened null result (the census + why no cheap predicate covers it).
- [ ] Remove this plan's row from the [performance index](README.md) and archive the file (per [feedback_plans_readme_remove_when_done]).

## Self-review notes

- **The gate is FIXVERIFY-clean over the whole build, not byte-identity.** This is the explicit correction of what made the reverted attempt look done: it *was* byte-identical and still unsound. Byte-identity is kept only as a secondary check.
- **Phase 0 is a real off-ramp.** The most probable outcome, given the reverted attempt and rec #1, is "no cheap predicate" → null result. The plan is structured so that conclusion costs one diagnostic build, not a full implementation.
- **Never default-on before Phase 2.** Every earlier phase keeps `TWINKLE_8G_FIXREUSE` off so `main` stays byte-identical and sound while the predicate is being proven.
- **Interaction with the sibling plans:** independent of CFG/liveness reuse; orthogonal to (but complementary with) the separate `summary.compute` scoping investigation.
