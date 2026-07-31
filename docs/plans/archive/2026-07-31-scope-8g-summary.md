# Scope 8G's Whole-Program `summary.compute` — Soundness Investigation Plan

> **❌ NULL RESULT (Phase 1, 2026-07-31) — not viable.** Instrumented the required
> `variant_scope` (downward closure of the mutation candidates ∪ the upward caller
> closure of published callees ∪ those callers' callee closure):
> `total=4106 updatable=523 down_wanted=3235 (79%) up_callers=873 union=3857 (93.9%)`.
> The scope is **93.9% of the program** — the `collect_groups` caller scan needs
> accurate summaries for an 873-func *upward* caller closure that sits outside the
> 79% downward `wanted` floor. Scoping to 94% saves <7% of the ~5.9s summary phase
> (~350ms) while carrying real codegen-soundness risk (a dropped variant) and a
> producer-reuse interaction (8G's summary is the producer's reuse base, imposing
> the 79% floor by itself). Below the plan's own 0.85 stop-gate — **stopped at
> Phase 1; Phase 2/3 not attempted.** Investigation-only instrumentation reverted.
> Recorded in [compiler.md](compiler.md). Plan retained below for the methodology.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax. **This is investigation-first: no perf change ships until the soundness gate in Phase 2 passes.**

**Goal:** Determine whether Phase 8G's `variant_specialize` can compute its ownership summary over a *scoped* subset of functions (a "variant-relevant closure") instead of all ~4123 functions, without dropping any published owned variant — and if so, land it. The `table` sub-phase (`summary.compute`) is ~5806ms, the single largest sound-uniqueness cost.

**Architecture:** 8G needs summaries only to (a) decide which callees publish an owned variant (`compute_variants`) and (b) evaluate callers' argument uniqueness at published callees (`collect_groups`). If the set of functions that can affect either decision is provably closed and smaller than the whole program, scoping the summary to it is byte-identical. The **danger** (which killed the FixResult-reuse lever, see [compiler.md](compiler.md)) is subtler here: a scoped summary gives out-of-scope functions *conservative* summaries, and a conservative callee summary can suppress a variant that the real summary would publish → a clone not made → the persistent path instead of in-place → **a codegen change**. So the whole plan is a soundness proof, with byte-identity as the machine check, before any timing claim.

**Tech Stack:** Twinkle self-hosted boot compiler (`boot/compiler/summary.tw`, `variant_specialize.tw`); `make stage2`; census/`--sites` route audit; no stage0/Rust changes expected.

---

## Why this needs its own plan

The sound-and-cheap CFG/liveness reuse levers live in
[2026-07-31-ownership-cfg-liveness-reuse.md](2026-07-31-ownership-cfg-liveness-reuse.md).
This one is separated because it is **not obviously sound**: it can change which
clones are built. It must be de-risked as a *soundness investigation* first, and
only becomes an implementation plan if Phase 2 proves no variant is dropped. If
Phase 2 fails, the deliverable is a documented null result, not code.

**Prior soundness lesson to carry in:** conservative-summary substitution is only
"can only lose precision, never fabricate" *for the mutable producer's decisions*
(compiler.md, `compute_for_roots`). For **variant publishing** the direction is
reversed — losing precision can lose a variant, which changes output. Do not
assume the producer's scoping argument transfers.

---

## Verification harness

Same gates as the sibling plan, plus a variant-route audit:

- **Byte-identity A/B** on `TWINKLE_8G_SCOPE=0` vs `=1` (the authoritative gate — a scoped summary that drops a variant *will* diff).
- **Variant-route audit:** the census `--sites` view renders `render_routes` (`variant_specialize.tw:540`). Diff the route table (generic→clone, sites, verdict) between scoped and unscoped: it must be identical, and gives a *localized* signal (which variant vanished) when byte-identity fails.
- **FIXVERIFY / SUMMARY_REUSE_VERIFY**, boot suite (3321), `make stage2` fixed point.
- Same-session 3× A/B timings under `TWINKLE_TIMINGS=1` on `[time:8g] table=...`.

---

## Phase 0: Characterize what the summary is actually used for

**Files (read-only):** `boot/compiler/summary.tw` (`compute`, `compute_variants`, `run_scc`, `with_dedupe_helpers`), `boot/compiler/codegen/variant_specialize.tw` (`collect_groups`, `published_callee_set`).

- [ ] **Step 1: Enumerate every consumer of the 8G summary table**

Read and write down, in this plan's Phase-0 notes, exactly which functions' summaries each consumer reads:
- `compute_variants(view, b, sem, table)` — does it iterate all functions, or only updatable callees? What summaries does variant eligibility read (the callee's own, and its callees' via the resolver)?
- `collect_groups` — `call_uniques_sited_with_field_seed` over callers of published callees; which summaries does the caller-side scan consult?
- `with_dedupe_helpers(table, view, b, sem)` — does it need whole-program summaries?

- [ ] **Step 2: State the candidate scope precisely**

Define `variant_scope` as the smallest set provably sufficient: `updatable_funcs` (candidate callees) ∪ their transitive user-callee closure (needed to summarize them correctly) ∪ all callers that syntactically call a published callee (`cfg_calls_published`) ∪ those callers' callee-closure. Write the closure definition down; it drives Phase 1.

- [ ] **Step 3: Decision gate**

If any consumer provably needs a genuinely whole-program summary with no smaller sufficient set (e.g. `compute_variants` inspects every function's summary for eligibility), **stop**: scoping cannot be byte-identical. Record the finding in compiler.md and close this plan as a null result.

## Phase 1: Measure the closure size (is the juice worth it?)

**Files:** `boot/compiler/codegen/variant_specialize.tw` (instrument only).

- [ ] **Step 1: Instrument `variant_scope` size without changing behavior**

In `specialize_module_with_cap_sem`, after `vt`/`published` are available, compute the Phase-0 closure (reusing `summary.dependency_closure`, `summary.tw:1838`) and print under `spec_timings_enabled()`:

```tw
if timed {
  eprintln("[time:8g:scope] total=${view.functions.len()} scope=${variant_scope.keys().len()}")
}
```

- [ ] **Step 2: Read the ratio**

Run: `TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/x.wasm 2>&1 | grep '\[time:8g:scope\]'`
**Decision gate:** the mutable producer's analogous closure (`wanted`) is already ~3254/4123 (~79%). If `variant_scope / total > ~0.85`, scoping saves <15% of the 5806ms (~<870ms) and adds soundness risk for little — record and stop, or downgrade to "revisit only if bigger levers land." Only proceed to Phase 2 if the ratio is meaningfully small.

- [ ] **Step 3: Commit the instrumentation**

```bash
git add boot/compiler/codegen/variant_specialize.tw
git commit -m "8G: instrument variant-relevant closure size (no behavior change)"
```

## Phase 2: Prove no variant is dropped (the soundness gate)

**Files:** `boot/compiler/summary.tw` (add `compute_scoped` behind a flag), `boot/compiler/codegen/variant_specialize.tw`.

- [ ] **Step 1: Add `TWINKLE_8G_SCOPE` and a scoped compute**

Add `summary.compute_scoped(view, b, sem, scope)`: identical to `compute` but seeds out-of-scope functions with `conservative_summary` and skips their SCCs (reuse the SCC-skip structure from `compute_for_roots_cached`, `summary.tw:1898`). Gate its use in `specialize_module_with_cap_sem` behind `TWINKLE_8G_SCOPE` (default **off** during investigation).

- [ ] **Step 2: Route-audit A/B (localized soundness signal first)**

```bash
TWINKLE_8G_SCOPE=0 target/twk ir boot/main.tw --sites > /tmp/routes_off.txt   # confirm the right census flag renders render_routes
TWINKLE_8G_SCOPE=1 target/twk ir boot/main.tw --sites > /tmp/routes_on.txt
diff /tmp/routes_off.txt /tmp/routes_on.txt && echo ROUTES-IDENTICAL
```
Expected: `ROUTES-IDENTICAL`. A diff names exactly which generic→clone route vanished under scoping — the concrete soundness counterexample. If it diffs, the scope in Phase 0 Step 2 is too small (missing a variant-affecting function); either widen it with a proven justification and retry, or conclude scoping is not byte-identical and stop.

- [ ] **Step 3: Full byte-identity A/B**

```bash
TWINKLE_8G_SCOPE=0 target/twk build boot/main.tw -o /tmp/off.wasm
TWINKLE_8G_SCOPE=1 target/twk build boot/main.tw -o /tmp/on.wasm
cmp /tmp/off.wasm /tmp/on.wasm && echo IDENTICAL
```
Expected: `IDENTICAL`. **This is the hard gate.** If routes match but the binary differs, a summary difference leaked into the mutable producer's decisions (the producer consumes 8G's carried summary — a scoped 8G summary would carry conservative entries for out-of-scope funcs). That interaction must be resolved (e.g. the producer must still receive real summaries for its own roots) before proceeding.

- [ ] **Step 4: FIXVERIFY + SUMMARY_REUSE_VERIFY + boot suite**

```bash
TWINKLE_FIXVERIFY=1 TWINKLE_SUMMARY_REUSE_VERIFY=1 TWINKLE_8G_SCOPE=1 target/twk build boot/main.tw -o /tmp/v.wasm 2>&1 | grep -iE "mismatch|MISMATCH"
```
Expected: no output. Then boot suite green.

- [ ] **Step 5: Decision checkpoint**

Only if Steps 2–4 are all clean does scoping ship. Otherwise write the counterexample and disposition into compiler.md as a null result and stop here.

## Phase 3: Land it (only if Phase 2 is clean)

- [ ] **Step 1: Make scoping the default**

Flip `TWINKLE_8G_SCOPE` default on (`=0` becomes the kill-switch), matching the `TWINKLE_SUMMARY_REUSE` convention.

- [ ] **Step 2: A/B timing + stage2**

3× medians of `[time:8g] table=...`; confirm the drop. `make stage2` → fixed point.

- [ ] **Step 3: Commit**

```bash
target/twk fmt boot/compiler/summary.tw boot/compiler/codegen/variant_specialize.tw
git add boot/compiler/summary.tw boot/compiler/codegen/variant_specialize.tw
git commit -m "Scope 8G summary to the variant-relevant closure (TWINKLE_8G_SCOPE)"
```

## Wrap-up

- [ ] Update [compiler.md](compiler.md): either a "Landed wins" bullet with A/B numbers + the byte-identity/route-audit proof, **or** a "Null result" bullet with the concrete dropped-variant counterexample.
- [ ] Remove this plan's row from [README.md](../README.md) / this dir's index and archive the file (per [feedback_plans_readme_remove_when_done]).

## Self-review notes

- **The soundness direction is the whole point:** unlike the mutable producer's scoping (conservative = safe), scoping the *variant-publishing* summary can drop a clone. Phase 2's route-audit + byte-identity are the machine proof; the plan explicitly refuses to ship on a timing win alone.
- **Interaction risk flagged:** 8G's carried summary also feeds the mutable producer's reuse — Phase 2 Step 3 calls out that a scoped carried summary must not degrade the producer's own decisions.
- **Off-by-default during investigation** so main stays byte-identical until the gate passes.
