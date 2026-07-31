# Ownership CFG + Liveness Reuse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cut the sound-uniqueness codegen phases (~13.5s of a ~20s `boot/main.tw` build) by eliminating two *structurally-safe* redundancies between Phase 8G (`variant_specialize`) and the mutable-decision producer: the post-clone CFG rebuild (~987ms) and cross-pass liveness recomputation (~0.3–0.5s).

**Architecture:** Both levers thread a fact 8G already computed into the producer, which currently recomputes it from scratch. Unlike the reverted FixResult-reuse attempt, these facts have **no summary-table dependency** — a `CfgFunction` and a per-function liveness result are purely structural over an unchanged function body, so reuse is byte-identical *by construction* for every function 8G and the producer share. Each lever is proven viable behind an env flag (A/B on the same input) with `TWINKLE_FIXVERIFY` + output byte-identity + the `make stage2` fixed point as gates **before** it is trusted, then the flag stays as a kill-switch.

**Tech Stack:** Twinkle self-hosted boot compiler (`boot/compiler/*.tw`); Deno-bundled CLI (`target/twk`); `make stage2` self-host loop; `cargo`/stage0 unaffected (no new language features).

---

## Background

The probe (see [compiler.md](compiler.md), "Current baseline 2026-07-31") found the two dominant phases:

```text
variant_specialize        ~7.7s   (8G: summary.compute ~5806ms, groups ~1616ms, variants ~671ms)
produce_mutable_decisions  ~5.9s   (cfg ~987ms, summary reuse ~4786ms, ownership ~813ms)
```

The producer's `summary` **reuse** cost is not safely reducible by carrying 8G's FixResults: the reverted null result (compiler.md, "Null result: reuse 8G's FixResults") proved 8G's whole-program-table FixResults diverge from the producer's scoped-table fixes. Scoping 8G's *own* `summary.compute` (the 5806ms phase) is a separate, higher-risk investigation tracked in [2026-07-31-scope-8g-summary.md](2026-07-31-scope-8g-summary.md). This plan targets only the two facts that are scope-independent:

1. **CFG (`cfg ~987ms`)** — the producer runs `cfg.build_view(opt) |> prune_dead_merge` over the post-clone module. Both `build_view` (`boot/compiler/cfg.tw:827`) and `prune_dead_merge` (`boot/compiler/ownership.tw`) are `collect f in view.functions { per_function(f) }` — purely per-function. 8G already built + pruned a view over the pre-clone module (`variant_specialize.tw:302`). Every function unchanged by specialization has a byte-identical `CfgFunction`.

2. **Liveness (~0.3–0.5s)** — `[time:own:live]` shows liveness computed in five places on the same blocks (`summary:`, `field_reqs:`, `call_uniques:`, `analyze:`, `prune:`). compiler.md's "reuse summary-pass liveness in field_reqs" already shared `summary:`→`field_reqs:`; the `call_uniques:`/`analyze:` recomputations remain.

**Discipline (the lesson from the reverted lever):** structural byte-identity on one input is necessary but not sufficient — every task's acceptance runs `TWINKLE_FIXVERIFY=1` (recompute-and-compare guard, `boot/compiler/ownership.tw:5735`) and the `make stage2` fixed point, not just an output diff.

---

## Verification harness (used by every task)

These are the standing gates. A task is "green" only when all of its listed gates pass.

- **Boot compiles (fast):** `target/twk build boot/main.tw -o /tmp/check.wasm` → exits 0.
- **Boot suite:** `target/twk run boot/tests/main.tw 2>&1 | grep -a "Ran .* tests"` → `NNNN passed`, 0 failed (current count 3321).
- **Byte-identity A/B (the primary correctness gate):** with a flag OFF vs ON on the same compiler binary and input:
  ```bash
  <FLAG>=0 target/twk build boot/main.tw -o /tmp/off.wasm
  <FLAG>=1 target/twk build boot/main.tw -o /tmp/on.wasm
  cmp /tmp/off.wasm /tmp/on.wasm && echo IDENTICAL
  ```
- **FIXVERIFY delta:** `analyze:unique_analysis_diags` is a **pre-existing tracked-red baseline** (mismatches on `main`, flag off). The gate is therefore **no NEW mismatch beyond it**, not "prints nothing": confirm the flag on produces the *same* mismatch as the flag off (plain FIXVERIFY errors on the first, so a byte-identical A/B is the practical proxy; use the census mode from the fixcache-reuse plan if a set comparison is needed).
- **Self-host fixed point:** `make stage2` → prints `Fixed point reached: stage3 == stage4`.
- **Same-session A/B timing:** run each variant 3× sequentially (never concurrently — see [feedback_sequential_heavy_verification]), compare medians of the relevant `[time]`/`[time:mutable:artifacts]` lines under `TWINKLE_TIMINGS=1`.

After any `.tw` edit: `target/twk fmt <file>` then `target/twk lint boot/main.tw` (expect "No findings").

---

## Lever A: Incremental CFG reuse (8G → mutable producer) — ✅ LANDED (2026-07-31)

**Status:** shipped behind `TWINKLE_CFG_REUSE` (default on). Headroom measured at
4057/4127 functions reusable; producer `cfg` phase ~845 → ~49ms; output
byte-identical (flag on vs off, and vs the no-reuse reference); FIXVERIFY delta
zero (only the pre-existing `analyze:unique_analysis_diags` baseline); `make
stage2` fixed point; 3322 boot tests pass (incl. the new equivalence test). The
task breakdown below is retained for reference.

**Files:**
- Modify: `boot/compiler/codegen/variant_specialize.tw` (carry the pruned view + changed-func set in `SpecializeResult`)
- Modify: `boot/compiler/codegen/codegen.tw` (disabled-8G constructor; thread the view into the producer call)
- Modify: `boot/compiler/codegen/mutable_produce.tw` (`produce_mutable_decisions_seeded_with_sem` signature + passthrough)
- Modify: `boot/compiler/codegen/ownership_verdicts.tw` (`compute_candidate_artifacts_seeded`: reuse the carried view)
- Modify: `boot/compiler/cfg.tw` (add `build_view_reusing` helper)
- Test: `boot/tests/suites/cfg_summary_suite.tw` (reuse-equivalence unit test)

### Task A0: Measure the reuse headroom and lock the changed-func computation

- [ ] **Step 1: Instrument the changed-func split in `specialize_module_with_cap_sem`**

In `boot/compiler/codegen/variant_specialize.tw`, just before the final `SpecializeResult.{ ... }` return (currently `variant_specialize.tw:472`), add a timing-gated diagnostic. The changed set is **clones ∪ callers whose body `rewrite_expr` actually rewrote**. Compute it from the routing data already in scope (`survivors` gives clone ids; a rewritten caller is any `f` for which `rewrite_expr(f.body, ...)` differs). Add a change-tracking rewrite (Step 2) then:

```tw
if timed {
  eprintln(
    "[time:8g:cfgreuse] total_funcs=${routed_funcs.len()} clones=${survivors.len()} rewritten_callers=${changed_callers.keys().len()} reusable=${routed_funcs.len() - survivors.len() - changed_callers.keys().len()}",
  )
}
```

- [ ] **Step 2: Add a change-tracking rewrite to compute `changed_callers`**

Add alongside `rewrite_expr` (`variant_specialize.tw:558`) a variant that reports whether it changed anything, so the caller set is exact (do not decode `vid.site_key` — it is a Szudzik pairing, `variant_id.tw:241`):

```tw
type RewriteResult = .{ expr: AnfExpr, changed: Bool }

fn rewrite_expr_tracked(e: AnfExpr, caller: Int, site_to_clone: Dict<Int, Int>) RewriteResult {
  case e {
    .Let(local, op, body) => {
      or := rewrite_op_tracked(op, local, caller, site_to_clone)
      br := rewrite_expr_tracked(body, caller, site_to_clone)
      RewriteResult.{ expr: .Let(local, or.expr, br.expr), changed: or.changed or br.changed }
    },
    _ => RewriteResult.{ expr: e, changed: false },
  }
}
```

with a matching `rewrite_op_tracked` mirroring `rewrite_op` (`variant_specialize.tw:569`): the `.ACall` arm sets `changed: true` exactly when `site_to_clone.get(vid.site_key(caller, result.id))` is `.Some`, and the `.AIf`/`.AMatch`/`.ALoop`/`.ADefer` arms OR their children's `changed`. Replace the existing `routed_funcs := collect f in funcs { ... rewrite_expr(...) }` with a loop that calls `rewrite_expr_tracked` and records `changed_callers[f.func_id.id] = true` when `changed`.

- [ ] **Step 3: Build, run, read the split**

Run: `TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/x.wasm 2>&1 | grep '\[time:8g:cfgreuse\]'`
Expected: one line; `reusable` should be ~4000+ of ~4100 funcs (only ~28 clones + a small number of rewritten callers change). **Decision gate:** if `reusable / total_funcs < 0.8`, CFG reuse saves little — stop and record the finding in compiler.md instead of proceeding.

- [ ] **Step 4: Verify the tracked rewrite is behavior-preserving**

Gates: boot compiles, boot suite green, `cmp` the output against a pre-Step-2 build (the tracked rewrite must produce identical ANF, so the wasm is byte-identical).

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/codegen/variant_specialize.tw
git commit -m "8G: track rewritten callers + instrument CFG-reuse headroom"
```

### Task A1: Carry 8G's pruned view + changed set in `SpecializeResult`

- [ ] **Step 1: Extend the `SpecializeResult` type**

In `variant_specialize.tw` (type at `:49`), add two fields after `fix_cache` is *not* present (that was reverted) — add after `summary`:

```tw
  // 8G's per-function pruned CFG view over the PRE-clone module. Every function
  // absent from `changed_funcs` has a byte-identical CfgFunction post-clone, so
  // the producer reuses it instead of rebuilding. Empty when 8G is disabled.
  pruned_view: cfg.CfgView,
  // Func ids whose CfgFunction changed under specialization: the clones (new) and
  // the callers whose call sites were rewritten to a clone. The producer rebuilds
  // only these; all others are reused from `pruned_view`.
  changed_funcs: Dict<Int, Bool>,
```

- [ ] **Step 2: Populate them in every `SpecializeResult` return**

`view` is already in scope (`variant_specialize.tw:302`). For the early returns (no variants / no updatable / func-def-missing / cfg-missing) `changed_funcs` is empty (`Dict.new()`) and `pruned_view` is `view`. For the final return, `changed_funcs` = the set built in Task A0 unioned with the clone ids (`for g in survivors { changed[g.clone_id] = true }`), and `pruned_view` is `view` (the PRE-clone pruned view — clones are absent from it, which is correct: they are in `changed_funcs` and always rebuilt).

- [ ] **Step 3: Add a `no_reuse` helper + wire the disabled-8G path**

In `variant_specialize.tw` next to `no_reuse_summary()`:

```tw
pub fn no_reuse_view() cfg.CfgView {
  cfg.CfgView.{ functions: [] }
}
```

In `codegen.tw` (disabled-8G constructor at `:146`), add `pruned_view: variant_specialize.no_reuse_view(), changed_funcs: Dict.new(),`.

- [ ] **Step 4: Build + boot suite green (no behavior change yet — fields are unused)**

Gates: boot compiles, boot suite green.

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/codegen/variant_specialize.tw boot/compiler/codegen/codegen.tw
git commit -m "8G: carry pruned CFG view + changed-func set in SpecializeResult"
```

### Task A2: Add `cfg.build_view_reusing` and consume it in the producer

- [ ] **Step 1: Add the reuse helper in `cfg.tw`**

After `build_view` (`cfg.tw:827`):

```tw
// Build a view for `m`, reusing already-built CfgFunctions from `prior` for every
// function whose id is NOT in `changed`. `prior` must be a view over a module that
// agrees with `m` on all unchanged functions (same body => same CfgFunction), which
// holds for 8G's pre-clone view vs the post-clone module. Functions in `changed`
// (and any absent from `prior`, e.g. clones) are built fresh.
pub fn build_view_reusing(
  m: AnfModule,
  prior: CfgView,
  changed: Dict<Int, Bool>,
) CfgView {
  by_id: Dict<Int, CfgFunction> = Dict.new()
  for f in prior.functions {
    by_id[f.func_id] = f
  }
  functions := collect f in m.functions {
    is_changed := case changed.get(f.func_id.id) {
      .Some(v) => v,
      .None => false,
    }
    if is_changed {
      build_function(f)
    } else {
      case by_id.get(f.func_id.id) {
        .Some(cf) => cf,
        .None => build_function(f),
      }
    }
  }
  CfgView.{ functions }
}
```

Note: `prior` is 8G's **pruned** view, so a reused `CfgFunction` is already pruned. Fresh-built ones must still go through `prune_function` — handle that in Step 3, not here.

- [ ] **Step 2: Add the flag + thread the view through the producer signatures**

Env flag in `ownership_verdicts.tw` (next to `summary_reuse_enabled`, `:26`):

```tw
fn cfg_reuse_enabled() Bool {
  case proc.env("TWINKLE_CFG_REUSE") {
    .Some(v) => v != "0",
    .None => true,
  }
}
```

Thread `prior_view: cfg.CfgView` and `changed_funcs: Dict<Int, Bool>` (defaulting to `cfg.CfgView.{ functions: [] }` / `Dict.new()`) through, mirroring the existing `carried` summary parameter: `codegen.tw` call site (`:177`) passes `spec.pruned_view, spec.changed_funcs`; `mutable_produce.produce_mutable_decisions_seeded_with_sem` (`:512`) and its `produce_mutable_decisions_seeded` default caller (`:495`) add the params; `ownership_verdicts.compute_candidate_artifacts_seeded` (`:580`) and its `compute_candidate_artifacts` default caller (`:558`) add them.

- [ ] **Step 3: Use the reuse path in `compute_candidate_artifacts_seeded`**

Replace the CFG build (`ownership_verdicts.tw:591-592`):

```tw
  view := if cfg_reuse_enabled() and prior_view.functions.len() > 0 {
    ownership.prune_dead_merge_selective(
      cfg.build_view_reusing(opt, prior_view, changed_funcs),
      changed_funcs,
    )
  } else {
    ownership.prune_dead_merge(cfg.build_view(opt, b))
  }
```

Add `prune_dead_merge_selective` in `ownership.tw` next to `prune_dead_merge`: it prunes only functions in `changed` (or absent from a "already pruned" marker) and passes the rest through untouched, because reused functions came from 8G's already-pruned view:

```tw
pub fn prune_dead_merge_selective(view: CfgView, changed: Dict<Int, Bool>) CfgView {
  functions := collect f in view.functions {
    is_changed := case changed.get(f.func_id) {
      .Some(v) => v,
      .None => false,
    }
    if is_changed {
      prune_function(f)
    } else {
      f
    }
  }
  CfgView.{ functions }
}
```

Rationale: a reused function is already the output of `prune_function` (from 8G's pruned view), and `prune_function` is idempotent over an already-pruned function, so skipping it is byte-identical; a freshly-built clone/rewritten-caller is pruned normally.

- [ ] **Step 4: Byte-identity A/B (the gate that would have caught the last lever)**

```bash
TWINKLE_CFG_REUSE=0 target/twk build boot/main.tw -o /tmp/off.wasm
TWINKLE_CFG_REUSE=1 target/twk build boot/main.tw -o /tmp/on.wasm
cmp /tmp/off.wasm /tmp/on.wasm && echo IDENTICAL
```
Expected: `IDENTICAL`. If not, `prune_function` is **not** idempotent (or an "unchanged" function's CFG actually differs post-clone) — in that case drop `prune_dead_merge_selective` and prune the whole reused view with `prune_dead_merge` (still saves the `build_function` cost), re-run, and record which assumption failed.

- [ ] **Step 5: FIXVERIFY + boot suite**

```bash
TWINKLE_FIXVERIFY=1 TWINKLE_CFG_REUSE=1 target/twk build boot/main.tw -o /tmp/v.wasm 2>&1 | grep -i mismatch
```
Expected: no output. Then boot suite green.

- [ ] **Step 6: A/B timing**

Run 3× each; compare median `[time:mutable:artifacts] cfg=...` and the `produce_mutable_decisions` total:
```bash
for i in 1 2 3; do TWINKLE_TIMINGS=1 TWINKLE_CFG_REUSE=0 target/twk build boot/main.tw -o /tmp/x.wasm 2>&1 | grep -E 'mutable:artifacts|produce_mutable_decisions:'; done
for i in 1 2 3; do TWINKLE_TIMINGS=1 TWINKLE_CFG_REUSE=1 target/twk build boot/main.tw -o /tmp/x.wasm 2>&1 | grep -E 'mutable:artifacts|produce_mutable_decisions:'; done
```
Expected: `cfg=` drops from ~987ms toward the fresh-build cost of just the changed funcs (tens of ms). **Decision gate:** if the phase drop is within noise, revert (keep the flag OFF by default or remove).

- [ ] **Step 7: `make stage2` fixed point**

Run: `make stage2` → `Fixed point reached: stage3 == stage4`.

- [ ] **Step 8: Commit**

```bash
target/twk fmt boot/compiler/cfg.tw boot/compiler/ownership.tw boot/compiler/codegen/ownership_verdicts.tw boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/codegen.tw
git add -A boot/compiler
git commit -m "Reuse 8G's pruned CFG in the mutable producer (TWINKLE_CFG_REUSE)"
```

### Task A3: Reuse-equivalence unit test

- [ ] **Step 1: Add a test to `cfg_summary_suite.tw`**

Mirror the existing `compute_for_roots_reusing` equivalence test (`cfg_summary_suite.tw:~2695`): build a view fresh vs via `build_view_reusing` with a `changed` set covering a mutated function, and assert `prune_dead_merge_selective(reused, changed)` equals `prune_dead_merge(fresh)` function-by-function (compare `render` or block/terminator counts per func via the suite's existing helpers).

```tw
// build_view_reusing over an unchanged function reproduces build_view exactly;
// a "changed" function is rebuilt identically to a from-scratch build.
reused := cfg.build_view_reusing(m, prior_pruned, changed)
for f in reused.functions {
  fresh_f := try cfg.function_named(fresh, f.name).ok_or("missing")
  try assert.equal(cfg.count_blocks(f), cfg.count_blocks(fresh_f))
}
```

- [ ] **Step 2: Run the suite**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -a "Ran .* tests"` → all pass.

- [ ] **Step 3: Commit**

```bash
git add boot/tests/suites/cfg_summary_suite.tw
git commit -m "Test: build_view_reusing equivalence to build_view"
```

---

## Lever B: Cross-pass liveness sharing (call_uniques / analyze) — ⏸️ DEPRIORITIZED (B0 done 2026-07-31)

**Status: measured, deprioritized — not built.** B0 investigation completed after
Levers A + fix-reuse landed; the finding is that Lever B is sound but low-ROI now:

- **Liveness is not skippable on the fix-reuse path.** `analyze_copy_carriers`
  (`ownership.tw:7231`) runs unconditionally before the fix-cache reuse check and
  consumes the live-stamped blocks, so a fix-cache hit still needs liveness.
- **The summary→analyze overlap eroded.** Fix-reuse now skips the ~390 pre-seeded
  safe-root SCCs in the summary phase (`run_sccs` 591→201), so there is no
  summary-side liveness to share for most of analyze's scope. Full coverage would
  require *also* carrying 8G's liveness (sound — liveness is invariant under
  specialization's call-target rewrites, unlike FixResults — but more plumbing).
- **`call_uniques` liveness is fragmented** across 3 contexts/views (summary
  variant computation `summary.tw:1602/1625`, 8G `collect_groups`, verdicts
  dry-run `ownership_verdicts.tw:488`) — no single clean boundary.
- **Plumbing is disproportionate:** a sound `LiveCache` must replicate the entire
  fix_cache + 8G-carry surface (`SeededResult`, `CachedSummary`, `run_scc`/
  `SccResult`, `SummaryCacheResult`, `SpecializeResult`, `analyze_selected`,
  `analyze_function`) on the hottest ownership code, for a **~0.3s** reachable win
  (vs fix-reuse 1.5s, CFG 0.8s).

**Recommendation:** leave unbuilt unless a later change re-enlarges the overlap or
the ~0.3s becomes material. The task breakdown below is retained for reference.

Only pursue after Lever A lands (or is decisively rejected). This is smaller (~0.3–0.5s) and its viability is already argued in compiler.md ("Remaining liveness redundancy").

**Files:**
- Modify: `boot/compiler/ownership.tw` (`call_uniques*`, `analyze_selected_with_summaries_and_entry_seeds`, and the `[time:own:live]` producers)

### Task B0: Confirm the redundant liveness sites and their block-identity

- [ ] **Step 1: Quantify**

Run: `TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/x.wasm 2>&1 | grep '\[time:own:live\]' | sed -E 's/.*func=([^:]+):.*/\1/' | sort | uniq -c`
Expected: the same funcs appear under multiple pass labels (`summary`, `field_reqs`, `call_uniques`, `analyze`, `prune`). **Decision gate:** if `call_uniques:`/`analyze:` liveness is <5% of the phase, skip Lever B.

- [ ] **Step 2: Confirm block-identity across the boundary**

Read `analyze_selected_with_summaries_and_entry_seeds` and `call_uniques_sited*` in `ownership.tw`; confirm they operate on the **same** `f.blocks` the summary pass already computed liveness for (they do when the CFG is shared — Lever A guarantees this for unchanged funcs). Record which pass boundaries share identical blocks (only those can share a cache).

### Task B1: Thread a per-view liveness cache

- [ ] **Step 1: Define the cache + flag**

Add `pub type LiveCache = .{ by_func: Dict<Int, <liveness-result-type>> }` and `TWINKLE_LIVE_REUSE` (default on) in `ownership.tw`, matching the `FixCache` shape (`ownership.tw:5709`). Populate it where the summary pass computes liveness; consult it (keyed by func id) in the `call_uniques`/`analyze` liveness call, falling back to fresh compute on a miss.

- [ ] **Step 2: Byte-identity + FIXVERIFY + boot suite**

Same gates as A2 Steps 4–5 with `TWINKLE_LIVE_REUSE`. Liveness over identical blocks is identical, so output must be byte-identical.

- [ ] **Step 3: A/B timing + stage2**

Confirm `[time:own:live]` for `call_uniques:`/`analyze:` drops; `make stage2` fixed point holds.

- [ ] **Step 4: Commit**

```bash
target/twk fmt boot/compiler/ownership.tw
git add boot/compiler/ownership.tw
git commit -m "Reuse summary-pass liveness in call_uniques/analyze (TWINKLE_LIVE_REUSE)"
```

---

## Wrap-up

- [ ] Update [compiler.md](compiler.md): move the CFG-reuse (and liveness, if landed) from a lever to a "Landed wins" bullet with the measured A/B numbers and the byte-identity/FIXVERIFY/stage2 acceptance; refresh the "Current baseline" phase split.
- [ ] Remove this plan's row from [README.md](README.md) and move this file to `docs/plans/archive/` (per [feedback_plans_readme_remove_when_done]).

## Self-review notes

- **Spec coverage:** Lever A (CFG, ~987ms) + Lever B (liveness, ~0.4s) cover both scope-independent redundancies the probe identified; the scope-dependent `summary` cost is pursued separately in [2026-07-31-scope-8g-summary.md](2026-07-31-scope-8g-summary.md) because it can change codegen and needs its own soundness gate.
- **Soundness discipline:** every implementing task gates on byte-identity + FIXVERIFY + stage2, not just an output diff — the explicit fix for what made the FixResult lever look viable until FIXVERIFY.
- **Kill-switches:** `TWINKLE_CFG_REUSE`, `TWINKLE_LIVE_REUSE` (both default on, `=0` restores the from-scratch path), matching the existing `TWINKLE_SUMMARY_REUSE` / `TWINKLE_COLD_WORKLIST` convention.
- **Type consistency:** `pruned_view: cfg.CfgView` and `changed_funcs: Dict<Int, Bool>` are used identically in `SpecializeResult`, the producer signatures, and `build_view_reusing`/`prune_dead_merge_selective`.
