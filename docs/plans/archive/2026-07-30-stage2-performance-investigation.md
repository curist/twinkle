# Stage2 Performance Investigation Implementation Plan

> **Status: COMPLETE.** Landed `summary.compute_for_roots_reusing` (branch
> `stage2-perf-summary-reuse`): the mutable-decision producer seeds its scoped
> summary from Phase 8G's carried whole-program table and recomputes only the
> clone-affected closure. `produce_mutable_decisions` ~10–13s → ~7.5–9.4s per
> heavy self-host build; fixed point stage3 == stage4 intact; reuse machine-verified
> equal to from-scratch on every build. See Optimization Results and Final Handoff.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce `make stage2` wall-clock time after the sound-uniqueness hang fix without weakening sound-uniqueness codegen or fixed-point verification.

**Architecture:** Start with a measured full self-host loop baseline, then isolate repeated expensive phases across stage1, stage2, stage3, and stage4 compiler builds. Optimize only the largest measured repeated cost, with preference for safe artifact reuse or scope reduction before algorithmic rewrites. Preserve diagnostics that help future performance work while keeping ordinary logs readable.

**Tech Stack:** Twinkle boot compiler (`boot/`), Rust stage0 (`target/release/twk`), Deno JS runtime harness (`tools/js_runtime/deno_main.mjs`), `make stage2`, `TWINKLE_TIMINGS=1`, sound-uniqueness phases 8G and mutable-decision production.

## Global Constraints

- Root cause before optimization: every code change must be tied to measured phase evidence.
- Do not weaken soundness checks, disable Phase 8G, or skip mutable decision production in normal builds.
- Keep `make stage2` fixed-point verification intact: stage3 and stage4 wasm must compare equal.
- After editing `.tw` files, run `target/twk fmt` on the exact files changed and `target/twk lint boot/main.tw`.
- Never run `tree-sitter test` from the agent.
- For timeout commands piped through `tee`, use `set -o pipefail` when the exit status matters.
- Treat `target/twk` as possibly stale after boot compiler changes; use explicit `BOOT_WASM=/tmp/...` Deno runtime runs when verifying newly built boot wasm behavior before rebundling.

---

## Current Evidence

The previous hang investigation is archived at `docs/plans/archive/2026-07-30-sound-uniqueness-stage2-hang-investigation.md`.

Confirmed facts from that investigation:

- The apparent hang was caused by non-converging dirty-path flow equality in `collect_field_reqs`.
- Commit `d7d59eea` fixed the convergence bug by comparing missing facts as `ff_none()` and treating two originless flow facts as equal.
- A direct default boot build with the fixed stage1 wasm completed and wrote `/tmp/stage2-fixed2.wasm`.
- The fixed direct build still showed expensive repeated phases:

```text
[time] variant_specialize: 14270.945708ms
[time] produce_mutable_decisions: 13508.953749999997ms
[time:mutable:artifacts] cfg=789.5232079999987ms summary=11688.846584000003ms ownership=837.1659999999974ms scope_roots=541
```

This means `make stage2` no longer hangs, but each self-hosted compiler build may spend substantial time recomputing sound-uniqueness summaries and call-site uniqueness data. Because `make stage2` runs several compiler builds, repeated phase costs multiply.

---

## Probe: Path A viability and the real summary-reuse target

Before running the full baseline, a targeted probe traced how the two dominant
phases compute their summaries. Measured on one real `target/twk build boot/main.tw`
build with `TWINKLE_TIMINGS=1`:

```text
[time:8g] table=8960ms variants=1072ms groups=2410ms filter=16ms groups0=65 updatable=27
[time] variant_specialize: 12500ms
[time:mutable:artifacts] cfg=1093ms summary=10733ms ownership=798ms scope_roots=544
[time] produce_mutable_decisions: 12772ms
```

Pipeline facts (`boot/compiler/codegen/codegen.tw:114-177`):

- `builder_region.rewrite_module` → `anf_prime`.
- Phase 8G (`variant_specialize.specialize_module_with_sem(anf_prime, ...)`) →
  `anf_spec`. 8G builds a **whole-program** summary (`summary.compute`,
  `variant_specialize.tw:288`) over the **pre-clone** `anf_prime`, then
  physically inserts clones with **fresh func ids** (`max_func_id(anf)+1`,
  `variant_specialize.tw:328`) and rewrites caller sites.
- The mutable producer (`produce_mutable_decisions_seeded_with_sem(anf_spec, ...)`)
  runs over the **post-clone** `anf_spec` and builds a **scoped** summary
  (`summary.compute_for_roots_cached`, `ownership_verdicts.tw:584`, scoped to
  544 candidate roots + clone ids). `scope_roots=544` in the log confirms it
  uses the scoped `compute_candidate_artifacts_seeded` path, not the
  whole-program `compute_artifacts` (which prints `scope_roots=-1`).

**Verdict on Path A (reuse one summary table between 8G and mutable production):
NOT VIABLE as written.** Its precondition — "both phases compute equivalent
whole-program or compatible scoped summaries over the same post-8G module" —
fails on both counts:

1. Different module version. In the real boot build 8G creates **27 clones**
   (`updatable=27`), so `anf_spec ≠ anf_prime`. 8G's table has zero entries for
   the 27 clones — the exact functions the mutable producer must reason about.
   Handing that table forward is unsound. (Only when 8G creates no clones and
   returns `anf` unchanged would the modules match.)
2. Different computation. The mutable side is a scoped-for-roots summary, not a
   re-run of 8G's whole-program table. There is no single table to hand over.

**Real target (Path C flavored): incremental per-function summary reuse with an
explicit invalidation set.** The mutable producer's "scoped" summary is barely
scoped — 544 roots pull in a near-total dependency closure (~3435 of ~4100
functions per the Path D closure example), so it re-summarizes ~84% of the
program that 8G already summarized. Yet 8G structurally touched only ~27 clones
plus the caller functions whose call sites it rewrote; every other function has a
byte-identical body pre/post-clone and an identical, reusable summary entry. The
sound optimization is to carry 8G's per-function summary entries into the mutable
producer and recompute only the invalidation set:

```text
invalidate = { 27 clone func ids } ∪ { host funcs of the rewritten call sites }
```

8G already tracks all of these (clone ids, `site_to_clone`, `variant_to_clone`,
`rewrite_calls` sites), so the invalidation set is available without new analysis.

Open proof obligations for Task 3 (resolve before relying on reuse):

- Confirm a per-function summary entry is a pure function of that function's body
  plus its callees' summaries, so a cross-module entry is substitutable. The
  `_cached` in `compute_for_roots_cached` suggests a memo to piggyback on.
- Confirm 8G can hand the mutable producer the exact set of rewritten-site host
  functions (it can — `site_to_clone` keys are the sites).
- A summary entry that depends on a callee whose summary changed must itself be
  invalidated (transitive closure of the invalidation set over the call graph),
  or reuse must be limited to entries provably independent of changed callees.

---

## Baseline Results

Full `TWINKLE_TIMINGS=1 make stage2` on branch `stage2-perf-summary-reuse`
(clean tree, fixed point reached: stage3 == stage4). Log at
`/tmp/twinkle-stage2-baseline.log`.

The loop performs one tiny bridge build (29 modules), a project check (no
codegen), and **three heavy boot/main codegen builds** (256 modules each). The
three heavy builds have an identical sound-uniqueness workload every time:
`groups0=65 updatable=27 scope_roots=544`.

| Build step | Compiler wasm | variant_specialize (8G) | 8G whole-prog summary `table` | produce_mutable_decisions | mutable scoped `summary` |
|---|---|---|---|---|---|
| bridge build | stage1 | 0.30s | 0.23s | 0.19s | 0.15s |
| project check | stage1 | n/a (check, no codegen) | — | — | — |
| stage1 → stage2 | `boot-stage1.wasm` | 11.38s | 8.26s | 10.53s | 9.14s |
| stage2 → stage3 | `boot.wasm` | 12.43s | 9.16s | 12.17s | 10.40s |
| stage3 → stage4 | `boot.wasm` | 12.77s | 9.41s | 12.61s | 10.74s |

Dominant repeated cost (per heavy build): **two large summary passes** —
8G's whole-program `summary.compute` (~8.3–9.4s) and the mutable producer's
scoped `summary.compute_for_roots_cached` (~9.1–10.7s). Together ~20s of each
~24s codegen, repeated across all three heavy builds (~60s of summary work
total). Everything else (monomorphize ~0.09s, optimize ~0.4s, emit ~1s) is
negligible by comparison. This matches and confirms the single-build probe.

## Recompute Localization Results

- **Dominant repeated cost:** two whole-module-scale summary passes per heavy
  build — 8G `summary.compute` (whole-program, pre-clone) and the mutable
  producer `summary.compute_for_roots_cached` (scoped to 544 roots, post-clone).
- **Why it repeats:** the mutable producer's "scope" (544 roots) closes over
  ~84% of the module, so it re-summarizes almost every function 8G already
  summarized, on a module that differs only by 27 added clones plus rewritten
  caller sites.
- **Candidate artifact to reuse/scope:** 8G's per-function summary entries.
  Carry them into the mutable producer; recompute only the invalidation set
  `{27 clone ids} ∪ {rewritten-site host funcs}` and its transitive callers.
- **Safety constraints:** a per-function entry may only be reused if its body is
  unchanged AND none of its (transitive) callees' summaries changed; otherwise
  recompute. 8G must expose the exact set of functions it structurally modified.
- **Rejected hypotheses:** Path A (hand 8G's whole-program table directly to the
  mutable producer) — unsound because the 27 clones are absent from 8G's table
  and the mutable pass is a different, scoped computation. See the Probe section.

## Optimization Results

Implemented the incremental-reuse target (a sound refinement of Task 3 Path A,
Path-C-flavored): 8G's whole-program summary is carried into the mutable producer
via `SpecializeResult.summary` and used to seed `compute_for_roots_reusing`, which
recomputes only the clone-affected closure.

- **Root cause of remaining slowness:** the mutable producer re-summarized ~3400
  functions (its 544-root "scope" closes over ~84% of the module) that 8G had
  already summarized on a module differing only by 27 clones + redirected sites.
- **Optimization implemented:** `summary.compute_for_roots_reusing` — seed the
  provably-reusable set (body unchanged AND all callees reusable) from the carried
  table; propagate the changed frontier (clones + funcs calling a clone) backward
  to transitive callers; run the fixpoint only over that closure. All root SCCs
  still run, so the root-keyed `fix_cache` handed to the ownership pass is
  unchanged. Threaded through `variant_specialize` → `codegen`/`ir` →
  `mutable_produce` → `ownership_verdicts`. `TWINKLE_SUMMARY_REUSE=0` disables it;
  `TWINKLE_SUMMARY_REUSE_VERIFY=1` also computes the from-scratch table and asserts
  per-function equality.
- **Representative build before/after** (stage1→stage2, `TWINKLE_TIMINGS=1`):
  `produce_mutable_decisions` 10.53s → 7.51s; its summary substep 9.14s → 3.92s
  (the residual ~2.2s in the "summary" bucket is `uniform_entry_seeds`, outside
  this optimization). `variant_specialize` unchanged (8G still computes the table
  we now reuse).
- **Full `make stage2` before/after** (per heavy build, `produce_mutable_decisions`):
  stage1→2 10.53s→7.51s, stage2→3 12.17s→9.10s, stage3→4 12.61s→9.37s (~3s each,
  ~9s across the loop). Reuse stats every heavy build: `funcs=4111 reused=3282
  clones=27 unsafe=229 skipped_sccs=3232`.
- **Fixed-point result:** `Fixed point reached: stage3 == stage4`, loop completes.
- **Tests/checks:** `TWINKLE_SUMMARY_REUSE_VERIFY=1 make stage2` — all four builds
  report `[summary:reuse:verify] ok` with zero mismatches; `target/twk run
  boot/tests/main.tw` — 3321 passed (incl. a new `cfg_summary_suite` test asserting
  reuse == from-scratch for a full carried table and for one with a simulated
  clone-absent entry); `twk lint boot/main.tw` — no findings.
- **Regressions or deferrals:** none observed. `variant_specialize`'s own
  whole-program summary is inherent (it decides clones) and unchanged. The reused
  path still runs all 544 root SCCs to keep the fix_cache identical; skipping those
  too would need the ownership pass to tolerate an incomplete cache (deferred).

## File Structure

- Modify: `docs/plans/2026-07-30-stage2-performance-investigation.md`
  - Owns measurements, decisions, final root cause, and verification evidence for this follow-up investigation.
- Possibly modify: `boot/compiler/codegen/variant_specialize.tw`
  - Owns Phase 8G timing, updatable prefiltering, variant publication, and clone routing.
- Possibly modify: `boot/compiler/codegen/mutable_produce.tw`
  - Owns mutable candidate collection, candidate/root counts, and mutable producer phase markers.
- Possibly modify: `boot/compiler/codegen/ownership_verdicts.tw`
  - Owns scoped summary/ownership artifact production for mutable decisions.
- Possibly modify: `boot/compiler/summary.tw`
  - Owns whole-program and scoped summary computation, `compute_variants`, `compute_for_roots_cached`, and summary timing output.
- Possibly modify: `boot/compiler/ownership.tw`
  - Owns `call_uniques_sited`, summarization cost, and ownership helper behavior used by summary and variant collection.
- Test: add or update focused tests under `boot/tests/suites/`
  - Add regression coverage only for the confirmed optimization semantics.
- No planned changes to `tree-sitter-twinkle/`.

---

## Task 1: Capture a full `make stage2` timing baseline

**Files:**
- Modify: `docs/plans/2026-07-30-stage2-performance-investigation.md`

**Interfaces:**
- Consumes: existing `make stage2`, `TWINKLE_TIMINGS=1`, and current boot compiler.
- Produces: one baseline log and a stage-by-stage timing table.

- [ ] **Step 1: Ensure the working tree starts clean.**

Run:

```bash
git status --short
```

Expected: no output. If there is output, stop and record the dirty files before running long measurements.

- [ ] **Step 2: Run a full timed self-host loop.**

Run:

```bash
set -o pipefail
TWINKLE_TIMINGS=1 make stage2 2>&1 | tee /tmp/twinkle-stage2-baseline.log
```

Expected: the command completes with `Self-host loop completed successfully.` and `Fixed point reached: stage3 == stage4`.

- [ ] **Step 3: Extract high-level stage boundaries.**

Run:

```bash
rg "^==>|Self-host loop completed|Fixed point reached|\[time\] (compile_modules|core_link|monomorphize|lower_anf|optimize|builder_region_rewrite|variant_specialize|closure_convert|produce_mutable_decisions|prepare_backend|verify|plan_wasm_types|emit_module|link|wasm_dce|emit_wasm_binary)" /tmp/twinkle-stage2-baseline.log > /tmp/twinkle-stage2-baseline-phases.log
```

Expected: `/tmp/twinkle-stage2-baseline-phases.log` contains phase lines for each compiler build performed by `make stage2`.

- [ ] **Step 4: Extract sound-uniqueness phase details.**

Run:

```bash
rg "time:8g|time:mutable|time:summary:roots|time:own:selected|time:mutable:fixcache" /tmp/twinkle-stage2-baseline.log > /tmp/twinkle-stage2-baseline-su.log
```

Expected: `/tmp/twinkle-stage2-baseline-su.log` shows whether 8G summary/variants/groups/filter or mutable scoped summary dominates each stage.

- [ ] **Step 5: Record the baseline table.**

Add a `## Baseline Results` section to this plan with this table filled from `/tmp/twinkle-stage2-baseline-phases.log` and `/tmp/twinkle-stage2-baseline-su.log`:

```markdown
## Baseline Results

| Build step | Compiler wasm | Output | Total observation | Dominant phases | Sound-uniqueness detail |
|---|---|---|---|---|---|
| stage0 -> stage1 | Rust stage0 | `target/boot-stage1.wasm` | measured | measured | not applicable or measured |
| stage1 bridge/check | `target/boot-stage1.wasm` | bridge + project check | measured | measured | measured if present |
| stage1 -> stage2 | `target/boot-stage1.wasm` | `target/boot.wasm` | measured | measured | measured |
| stage2 -> stage3 | `target/boot.wasm` | `/tmp/twinkle-selfhost/stage3.wasm` | measured | measured | measured |
| stage3 -> stage4 | `target/boot.wasm` after stage3 adoption | `/tmp/twinkle-selfhost/stage4.wasm` | measured | measured | measured |
```

Expected: the table identifies the repeated dominant phase before any optimization is proposed.

- [ ] **Step 6: Commit the baseline-only plan update.**

Run:

```bash
git add docs/plans/2026-07-30-stage2-performance-investigation.md
git commit -m "Measure stage2 performance baseline"
```

Expected: commit contains only the plan update with measured baseline data.

---

## Task 2: Localize repeated sound-uniqueness recomputation

**Files:**
- Possibly modify: `boot/compiler/codegen/variant_specialize.tw`
- Possibly modify: `boot/compiler/codegen/mutable_produce.tw`
- Possibly modify: `boot/compiler/codegen/ownership_verdicts.tw`
- Possibly modify: `boot/compiler/summary.tw`
- Possibly modify: `boot/compiler/ownership.tw`
- Modify: `docs/plans/2026-07-30-stage2-performance-investigation.md`

**Interfaces:**
- Consumes: baseline from Task 1.
- Produces: a precise recomputation map naming the repeated expensive functions or artifacts.

- [ ] **Step 1: Decide whether existing timings are sufficient.**

Read `/tmp/twinkle-stage2-baseline-su.log` and answer in the plan:

```markdown
### Recompute Localization

- Is Phase 8G dominated by `summary.compute`, `summary.compute_variants`, `collect_groups`, or `updatable_filter`?
- Is mutable production dominated by scoped CFG, scoped summary, selected ownership, decision join, or fix-cache misses?
- Which expensive subphase repeats with similar cost across stage1->stage2, stage2->stage3, and stage3->stage4?
```

Expected: if the log already identifies the dominant repeated subphase, skip to Step 4. If not, add instrumentation in Step 2.

- [ ] **Step 2: Add temporary timing markers only where baseline is ambiguous.**

If needed, add `TWINKLE_TIMINGS`-guarded markers around specific candidate calls. Use existing timing style and keep normal builds unchanged when `TWINKLE_TIMINGS` is unset.

Candidate marker locations:

```twinkle
// boot/compiler/codegen/variant_specialize.tw
view := ownership.prune_dead_merge(cfg.build_view(anf, b))
table := summary.compute(view, b, sem)
vt := summary.compute_variants(view, b, sem, table)
groups0 := collect_groups(view, table, b, sem, vt, published)
updatable := updatable_funcs(anf, b)

// boot/compiler/codegen/ownership_verdicts.tw
summarized := summary.compute_for_roots_cached(view, b, sem, scope)
artifacts := ownership.compute_selected_artifacts(...)

// boot/compiler/summary.tw
call sites of call_uniques_sited, run_scc_variants, and with_dedupe_helpers
```

Expected: markers compile and produce useful subphase boundaries without verbose per-function logs.

- [ ] **Step 3: Rebuild and rerun one representative compiler build.**

Run:

```bash
target/twk fmt boot/compiler/codegen/variant_specialize.tw boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/ownership_verdicts.tw boot/compiler/summary.tw boot/compiler/ownership.tw
./target/release/twk check boot/main.tw
./target/release/twk build boot/main.tw -o /tmp/boot-stage1-perf.wasm
set -o pipefail
TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/boot-stage1-perf.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build -o /tmp/stage2-perf-localize.wasm 2>&1 | tee /tmp/stage2-perf-localize.log
```

Expected: `/tmp/stage2-perf-localize.log` reaches `WASM output: /tmp/stage2-perf-localize.wasm` and identifies the dominant repeated subphase.

- [ ] **Step 4: Record the recomputation hypothesis.**

Add a `## Recompute Localization Results` section:

```markdown
## Recompute Localization Results

- Dominant repeated cost:
- Why it repeats:
- Candidate artifact that could be reused or scoped:
- Safety constraints for changing it:
- Rejected hypotheses:
```

Expected: the next task has one target and a falsifiable safety argument.

- [ ] **Step 5: Commit localization evidence and intentional retained markers.**

If source markers were added and are worth retaining, commit them with the plan update. If markers were temporary, remove them before committing.

Run:

```bash
git add docs/plans/2026-07-30-stage2-performance-investigation.md boot/compiler/codegen/variant_specialize.tw boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/ownership_verdicts.tw boot/compiler/summary.tw boot/compiler/ownership.tw
git commit -m "Localize stage2 sound-uniqueness cost"
```

Expected: commit contains only useful diagnostics or the plan evidence.

---

## Task 3: Choose and test one optimization path

**Files:**
- Possibly modify: `boot/compiler/codegen/variant_specialize.tw`
- Possibly modify: `boot/compiler/codegen/ownership_verdicts.tw`
- Possibly modify: `boot/compiler/summary.tw`
- Possibly modify: `boot/compiler/ownership.tw`
- Test: add or update focused tests under `boot/tests/suites/`
- Modify: `docs/plans/2026-07-30-stage2-performance-investigation.md`

**Interfaces:**
- Consumes: recomputation target from Task 2.
- Produces: one measured optimization with regression coverage.

Choose exactly one path based on Task 2 evidence.

### Optimization Path A: reuse one summary table between Phase 8G and mutable production

Use this path only if Task 2 proves both phases compute equivalent whole-program or compatible scoped summaries over the same post-8G module.

- [ ] **Step A1: Prove module identity and semantic compatibility.**

Record whether the mutable producer runs before or after 8G clone insertion and whether the summary table from 8G is still valid for mutable candidates.

Expected: if function ids or bodies differ after 8G, do not reuse the 8G table directly; switch to Path B or C.

- [ ] **Step A2: Introduce an explicit artifact carrier only if safe.**

Use a named record rather than hidden globals. Example shape if proven safe:

```twinkle
type SoundUniquenessArtifacts = .{
  view: cfg.CfgView,
  summary: summary.SummaryTable,
  variants: summary.VariantSummaryTable,
}
```

Expected: callers can see which module version the artifacts describe.

- [ ] **Step A3: Add equivalence coverage.**

Add a focused test proving reused artifacts produce the same mutable decisions as recomputed artifacts for a module with 8G clones and field-backed candidates.

Expected: test fails if stale pre-clone summaries are accidentally reused after clone insertion.

### Optimization Path B: scope Phase 8G expensive publication earlier

Use this path if Task 2 proves Phase 8G spends most time publishing variants or scanning call groups for functions that `updatable_funcs` later discards.

- [ ] **Step B1: Move or duplicate the cheap updatable prefilter before expensive publication.**

Compute `updatable := updatable_funcs(anf, b)` before the expensive operation identified in Task 2. Do not skip summaries for callees that can influence updatable roots unless the dependency proof is explicit.

Expected: non-updatable functions no longer drive expensive 8G publication or group scans where safe.

- [ ] **Step B2: Preserve recursive routing semantics.**

Add or run coverage for recursive field-tier routing and self/mutual-recursive variants.

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: existing variant routing and field-backed collection tests still pass when run with a fresh `target/twk` after bundling, or run focused suites via a fresh `BOOT_WASM` during development.

### Optimization Path C: reduce repeated `call_uniques_sited` work

Use this path if Task 2 shows `collect_groups`, variant publication, or mutable scoped summary repeatedly reruns `call_uniques_sited` for the same functions and seeds.

- [ ] **Step C1: Identify cache key inputs.**

Document the exact inputs that determine `call_uniques_sited` output: function id, summary table, builtin registry, optimizer semantics, variant resolver, field seed, and any suppress/seed maps.

Expected: if a stable key cannot be defined without unsafe aliasing, do not cache.

- [ ] **Step C2: Add a local cache with explicit lifetime.**

Keep the cache local to one compiler build and one CFG/module version. Do not use process-global mutable state.

Expected: repeated calls in one phase reuse results; separate module versions recompute.

- [ ] **Step C3: Add hit/miss timing output.**

When `TWINKLE_TIMINGS=1`, print cache hits and misses in the same style as existing `time:mutable:fixcache` output.

Expected: optimized run shows meaningful hits and lower repeated cost.

### Optimization Path D: improve scoped summary closure or SCC scheduling

Use this path if Task 2 shows scoped summary remains dominant and recomputes many functions irrelevant to mutable roots.

- [ ] **Step D1: Measure closure size and skipped SCCs before changing logic.**

Use existing `time:summary:roots` output:

```text
funcs=4100 roots=541 wanted=3435 wanted_blocks=48435 wanted_insts=109570 wanted_edges=59419 sccs=3808 run_sccs=3148 skipped_sccs=660
```

Expected: the plan records whether closure size, not per-function cost, dominates.

- [ ] **Step D2: Add a regression test for dependency closure semantics if changing closure logic.**

The test must compare reachable function membership, not timing. Use a small call graph where several roots share callees and one callee reaches a leaf.

Expected: reachable set remains unchanged after optimization.

- [ ] **Step D3: Optimize only the proven closure or scheduling hotspot.**

Examples that require proof before implementation:

```twinkle
// Mark queued callees before enqueueing to avoid duplicate worklist entries.
// Keep deterministic sorted processing.
queued: Dict<Int, Bool> = Dict.new()
```

Expected: same reachable set, fewer queued duplicates or fewer unnecessary SCC runs.

---

## Task 4: Verify the optimization on the self-host loop

**Files:**
- Modify: implementation files selected in Task 3.
- Modify: `docs/plans/2026-07-30-stage2-performance-investigation.md`

**Interfaces:**
- Consumes: optimization from Task 3.
- Produces: measured `make stage2` improvement with fixed-point verification intact.

- [ ] **Step 1: Run compiler checks and focused tests.**

Run the checks relevant to changed files. At minimum:

```bash
./target/release/twk check boot/main.tw
target/twk lint boot/main.tw
```

If tests were added or changed, run their focused suite through the freshest available compiler. If `target/twk` is stale, use a newly built `BOOT_WASM` with the Deno runtime.

Expected: checks pass and focused tests pass.

- [ ] **Step 2: Run a timed representative build.**

Run:

```bash
./target/release/twk build boot/main.tw -o /tmp/boot-stage1-optimized.wasm
set -o pipefail
TWINKLE_TIMINGS=1 BOOT_WASM=/tmp/boot-stage1-optimized.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build -o /tmp/stage2-optimized.wasm 2>&1 | tee /tmp/stage2-optimized.log
```

Expected: build reaches `WASM output: /tmp/stage2-optimized.wasm`; relevant dominant phase is lower than baseline.

- [ ] **Step 3: Run full `make stage2` with timings.**

Run:

```bash
set -o pipefail
TWINKLE_TIMINGS=1 make stage2 2>&1 | tee /tmp/twinkle-stage2-optimized.log
```

Expected: command completes with `Self-host loop completed successfully.` and `Fixed point reached: stage3 == stage4`.

- [ ] **Step 4: Compare baseline and optimized logs.**

Run:

```bash
rg "\[time\] variant_specialize|\[time\] produce_mutable_decisions|time:mutable:artifacts|time:8g|time:summary:roots" /tmp/twinkle-stage2-baseline.log /tmp/twinkle-stage2-optimized.log > /tmp/twinkle-stage2-comparison.log
```

Expected: comparison log shows which phase improved and whether any phase regressed.

- [ ] **Step 5: Record verification results.**

Add a `## Optimization Results` section:

```markdown
## Optimization Results

- Root cause of remaining slowness:
- Optimization implemented:
- Representative build before/after:
- Full `make stage2` before/after:
- Fixed-point result:
- Tests/checks:
- Regressions or deferrals:
```

Expected: results are evidence-based and do not claim full test success unless full tests were actually run and passed.

- [ ] **Step 6: Commit implementation and results.**

Run:

```bash
git add docs/plans/2026-07-30-stage2-performance-investigation.md boot/compiler/codegen/variant_specialize.tw boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/ownership_verdicts.tw boot/compiler/summary.tw boot/compiler/ownership.tw boot/tests/suites
git commit -m "Optimize stage2 sound-uniqueness build cost"
```

Expected: commit contains the optimization, coverage, and measured results.

---

## Task 5: Cleanup diagnostics and document retained performance tools

**Files:**
- Possibly modify: `boot/compiler/codegen/variant_specialize.tw`
- Possibly modify: `boot/compiler/codegen/mutable_produce.tw`
- Possibly modify: `boot/compiler/codegen/ownership_verdicts.tw`
- Possibly modify: `boot/compiler/summary.tw`
- Possibly modify: `boot/compiler/ownership.tw`
- Possibly modify: `docs/plans/sound-uniqueness/codegen/README.md`
- Modify: `docs/plans/2026-07-30-stage2-performance-investigation.md`

**Interfaces:**
- Consumes: final optimization and verification from Task 4.
- Produces: clean diagnostics and handoff notes.

- [ ] **Step 1: Remove noisy temporary markers.**

Search for temporary trace labels added during this plan:

```bash
rg "hot-function|perf-localize|temporary|trace:own:field_reqs|trace:summary:func" boot/compiler
```

Expected: no noisy temporary markers remain unless explicitly documented.

- [ ] **Step 2: Keep useful timing lines intentionally.**

Retain concise high-level timing lines that are generally useful under `TWINKLE_TIMINGS=1`, such as phase totals, cache hit/miss summaries, and scoped summary aggregate counts.

Expected: ordinary `TWINKLE_TIMINGS=1` output remains readable.

- [ ] **Step 3: Document retained flags or diagnostics.**

If new timing output or env flags remain, update `docs/plans/sound-uniqueness/codegen/README.md` with:

```markdown
### Performance diagnostics

- `TWINKLE_TIMINGS=1`: prints phase totals for 8G, mutable production, scoped summary, and retained cache summaries.
- Use this when comparing `make stage2` before/after compiler optimization work.
```

Expected: future agents know which diagnostics are intentional.

- [ ] **Step 4: Final checks.**

Run:

```bash
target/twk fmt boot/compiler/codegen/variant_specialize.tw boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/ownership_verdicts.tw boot/compiler/summary.tw boot/compiler/ownership.tw
./target/release/twk check boot/main.tw
target/twk lint boot/main.tw
```

Expected: formatting is stable, type checking succeeds, and lint reports no findings.

- [ ] **Step 5: Final plan handoff.**

Add a final handoff section:

```markdown
## Final Handoff

- Baseline log:
- Optimized log:
- Optimization commit:
- Remaining performance deferrals:
- Commands verified:
```

Expected: the plan is useful as an archived record after work completes.

- [ ] **Step 6: Commit cleanup.**

Run:

```bash
git add docs/plans/2026-07-30-stage2-performance-investigation.md docs/plans/sound-uniqueness/codegen/README.md boot/compiler/codegen/variant_specialize.tw boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/ownership_verdicts.tw boot/compiler/summary.tw boot/compiler/ownership.tw
git commit -m "Document stage2 performance diagnostics"
```

Expected: final commit contains cleanup and documentation only.

---

## Final Handoff

- **Baseline log:** `/tmp/twinkle-stage2-baseline.log` (see Baseline Results table).
- **Optimized log:** `/tmp/twinkle-stage2-optimized.log` / `/tmp/stage2-time.txt`
  (per-heavy-build `produce_mutable_decisions` 7.51s / 9.10s / 9.37s).
- **Verify log:** `/tmp/twinkle-stage2-verify.log` — `TWINKLE_SUMMARY_REUSE_VERIFY=1
  make stage2`, four `[summary:reuse:verify] ok`, fixed point reached.
- **Implementation commit:** "Reuse Phase 8G summary to seed mutable-decision
  producer" on branch `stage2-perf-summary-reuse` (`summary.tw`,
  `variant_specialize.tw`, `codegen.tw`, `mutable_produce.tw`,
  `ownership_verdicts.tw`, `ir.tw`, `cfg_summary_suite.tw`).
- **Retained diagnostics:** `[time:summary:reuse]` under `TWINKLE_TIMINGS=1`;
  `TWINKLE_SUMMARY_REUSE` / `TWINKLE_SUMMARY_REUSE_VERIFY` env flags. Documented in
  `docs/plans/sound-uniqueness/codegen/README.md` → Performance diagnostics.
- **Remaining performance deferrals:** 8G's own whole-program summary (~8–9s/build)
  is inherent to clone selection and untouched. The reuse path still runs all 544
  root SCCs to keep the ownership `fix_cache` byte-identical; skipping those would
  need the ownership pass to tolerate an incomplete cache. `uniform_entry_seeds`
  (~2s) inside the mutable "summary" bucket is now the next-largest sub-cost.
- **Commands verified:** `TWINKLE_SUMMARY_REUSE_VERIFY=1 make stage2` (0 mismatches,
  fixed point); `make stage2` (fixed point, timings); `target/twk run
  boot/tests/main.tw` (3321 passed); `twk lint boot/main.tw` (no findings).
- **Not done:** `make bundle-cli` — `target/twk` still embeds the pre-optimization
  compiler; rebundle to ship the CLI with reuse enabled.
