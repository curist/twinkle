# Phase 6 Completion — Per-Call-Site Decision Verdicts Implementation Plan

> **Status: archived.** Phase 6 analysis work landed; remaining variant generation, routing, and deeper path/variant machinery are tracked by the codegen/migration tracks.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render the per-call-site ownership-specialization decision in `twk ir --cfg` — the last analysis step of Phase 6. At each user call whose argument is proven owned, emit a verdict naming the owned variant the caller may select (`build_env#… -> f{add_type}[unique:…]`), so the specialization story is **verifiable before any codegen**. This closes the Phase 6 exit criterion (Case B∩C) and completes the analysis track.

**Architecture:** `block_verdicts` (`ownership.tw:1849`) already walks each block's instructions carrying the pre-instruction forward state `pre` and the per-instruction `last`-use set, and records a rendered verdict string per instruction (for transport projections and record updates) that `cfg.tw` prints in `--cfg`. Stage 4c-core already provides the pure decision `select_variant`. This change adds one `.ACall` case to `block_verdicts`: compute `arg_unique` from `pre.own` + `last` (exactly as `transfer_summarized_call` does), call `select_variant(callee, summary, arg_unique)`, and record an **owned-variant** verdict (generic decisions are skipped to keep `--cfg` readable). No new pass, no `ForwardState` change, no driver change. The verdict is a debug/render string that never reaches codegen, so `twk ir --census` stays **0 in-place**, the emitted `wasm` is byte-identical, and the self-host fixed point holds.

**Tech Stack:** Twinkle (`.tw`), boot compiler only. Tests via the boot suite. Build/verify with `make boot-test`.

## Global Constraints

- Boot compiler only: put the implementation in `boot/`; do not change Rust stage0 unless bootstrapping requires it.
- Analysis/render-only: no codegen behavior change; `target/twk ir boot/main.tw --census` must stay `total in_place: 0`.
- Preserve self-hosting: `make stage2` must still report `Fixed point reached: stage3 == stage4`.
- Format and lint Twinkle edits: run `target/twk fmt` on edited `.tw` files and `target/twk lint boot/main.tw`; fix any new linter finding.
- Builtins are not specialization candidates: call-site verdict recording must follow `transfer_call` dispatch and skip callees handled by `call_info(sem, fid)`.
- Record only owned decisions in `--cfg`: generic decisions render as no verdict for this slice.

---

## Why this completes the analysis track

The canonical exit criterion (`analysis/README.md`) is: *the B∩C "one callee, two caller shapes" example prints a verifiable per-call-site specialization decision (owned-specialized vs generic) in `twk ir --cfg`.* Every ingredient now exists:

- **The recovery** is done (2a/2b/3/4a): `add_type`-shaped callees classify `Consumed` with `in_place_paths`, and a fresh-unique argument is proven owned at the call.
- **The decision logic** is done (4c-core): `select_variant(func_id, callee_summary, arg_unique)`.
- **The render hook** exists: `block_verdicts` → `blk.exit.verdicts` → `cfg.tw` render.

What is missing is only the *wiring*: computing `arg_unique` at each call site and recording the decision verdict. That is this change.

**Scope is the flagship (direct-mutator) case**, per the re-scoping recorded in `phase6-design.md`: the decision reads the callee's **generic** summary, which is exact for a direct-mutator callee like `add_type` (`Consumed` generically). Param-threaded/recursive-callee decisions (which need the owned summary / SCC fixpoint) and the machine-readable `SpecializationFacts` table are the **codegen** track's to consume; here the recording is sound (a callee whose generic summary does not advertise a consumed param simply yields no owned verdict — an under-approximation, never a wrong owned claim).

### Scope

**In scope:** an `.ACall` case in `block_verdicts` that records an **owned-variant** verdict (via `select_variant` off the callee's generic summary + per-call `arg_unique`), rendered in `twk ir --cfg`.

**Explicitly out of scope:** rendering the `generic` decision (skipped for `--cfg` readability — a refinement); a machine-readable `SpecializationFacts`/`CallDecision` table (codegen consumes ANF-keyed decisions then; the verdict is the human-verifiable render); param-threaded/recursive-callee decisions via demanded owned summaries + the SCC variant fixpoint (codegen track); `ConsumedPaths` + the D6 disjoint-sibling read-rule + field-granular seeding (codegen track); the variant cap (D7).

### File structure

- **Modify `boot/compiler/ownership.tw`:** add `render_call_decision(callee_id, VariantId) String`; add the `.ACall` case to `block_verdicts`.
- **Test `boot/tests/suites/cfg_return_paths_suite.tw`:** `build_env` (owned) / `branch_env` (generic) fixtures; assert the per-call verdicts.

Verified anchors (current branch): `block_verdicts` walks instructions with `pre := st` and `last := last_use_at(...)` and writes `verdicts[inst.anf_local.id]` (`ownership.tw:1849`–`1924`); `callee_func_id(a) FuncId?` (`:703`), `own_is_unique(own, id)` (`:1258`), `is_last_use(last, id)` (`:721`), `table.summary_get(id)`, `select_variant(func_id, s, arg_unique) vid.VariantId` (Stage 4c-core), `atom_local_id`; `blk.exit.verdicts: Dict<Int, String>` rendered by `cfg.tw:1028`; suite helpers `analyzed_caller`/`fdef`/`module_of`/`dict_new_call`/`b_reg` present.

---

## Task 1: The per-call-site decision verdict

**Files:**
- Modify: `boot/compiler/ownership.tw`
- Test: `boot/tests/suites/cfg_return_paths_suite.tw`

- [ ] **Step 1: Write the failing verdict tests**

In `cfg_return_paths_suite.tw`, add a helper next to `caller_own`:

```tw
// The rendered decision verdict recorded at a call's result local (or "" if none).
fn verdict_of(f: cfg.CfgFunction, local_id: Int) String {
  for blk in f.blocks {
    case blk.exit.verdicts.get(local_id) {
      .Some(s) => return s,
      .None => {},
    }
  }
  ""
}

// substring test
fn contains_sub(hay: String, needle: String) Bool {
  hay.contains(needle)
}
```

Then the tests:

```tw
    .test(
      "phase6 completion: build_env-shape call selects the owned variant (verdict)",
      fn() {
        b := b_reg()
        // at(env) { env.f0 = 0 ; env2 }  -- direct mutator: Consumed {[],[.f0]}.
        at := fdef(
          1,
          "at",
          1,
          .Let(
            lid(1),
            .ARecordUpdate(.ALocal(lid(0)), FieldId.{ id: 0 }, .ALitInt(0), false, TypeId.{ id: 0 }),
            .Atom(.ALocal(lid(1))),
          ),
        )
        // be() { e := Dict.new(); r := at(e); r }  -- e is fresh Unique + last-use at the call.
        be_body: AnfExpr = .Let(
          lid(0),
          dict_new_call(b),
          .Let(lid(1), .ACall(.AGlobalFunc(FuncId.{ id: 1 }), [.ALocal(lid(0))]), .Atom(.ALocal(lid(1)))),
        )
        f := analyzed_caller([at, fdef(2, "be", 0, be_body)], "be")
        // the at(e) call (result lid1) records an OWNED verdict naming a unique key.
        try assert.is_true(contains_sub(verdict_of(f, 1), "unique"))
        .Ok({})
      },
    )
    .test(
      "phase6 completion: branch_env-shape call (arg read later) selects generic (no owned verdict)",
      fn() {
        b := b_reg()
        at := fdef(
          1,
          "at",
          1,
          .Let(
            lid(1),
            .ARecordUpdate(.ALocal(lid(0)), FieldId.{ id: 0 }, .ALitInt(0), false, TypeId.{ id: 0 }),
            .Atom(.ALocal(lid(1))),
          ),
        )
        // br() { e := Dict.new(); r := at(e); x := e.f0; r }  -- e read AFTER the call,
        // so it is not last-use at the call -> arg not unique -> generic -> no owned verdict.
        br_body: AnfExpr = .Let(
          lid(0),
          dict_new_call(b),
          .Let(
            lid(1),
            .ACall(.AGlobalFunc(FuncId.{ id: 1 }), [.ALocal(lid(0))]),
            .Let(lid(2), .ARecordGet(.ALocal(lid(0)), FieldId.{ id: 0 }, TypeId.{ id: 0 }), .Atom(.ALocal(lid(1)))),
          ),
        )
        f := analyzed_caller([at, fdef(2, "br", 0, br_body)], "br")
        try assert.equal(verdict_of(f, 1), "") // generic decisions are not rendered
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify failure**

Run: `set -o pipefail; make boot-test 2>&1 | grep -aE 'Ran [0-9]+ tests|  x .*completion' | tail -4`
Expected: FAIL on the `build_env-shape` test — this is the red test because no verdict is recorded at the call yet (`verdict_of` returns `""`, so `contains_sub(..., "unique")` is false). The `branch_env` test already passes vacuously (`""`), but keep it — it becomes meaningful once the case is added.

- [ ] **Step 3: Add `render_call_decision` + the `.ACall` verdict case**

In `ownership.tw`, add `render_call_decision` just below `select_variant`:

```tw
// Render a call-site decision verdict (Stage 4c completion): the owned variant a
// caller may select. Generic decisions (empty key) render as "" -- callers skip them
// to keep `twk ir --cfg` readable (a generic-render refinement is deferred).
fn render_call_decision(callee_id: Int, v: vid.VariantId) String {
  if v.unique.len() == 0 {
    ""
  } else {
    reqs: Vector<String> = collect r in v.unique {
      if r.path.segs.len() == 0 {
        "p${r.param}"
      } else {
        "p${r.param}.f${r.path.segs[0]}"
      }
    }
    "-> f${callee_id}[unique:${reqs.join(",")}]"
  }
}
```

Then add the `.ACall` case to `block_verdicts`'s **pre-transfer** `case inst.op`
(`ownership.tw:1866`, alongside `.ARecordGet`). This must mirror `transfer_call`'s
callee dispatch: builtins handled by `call_info(sem, fid)` are skipped before user
summaries are consulted, so verdicts are recorded only for direct user calls whose
transfer also uses `table.summary_get`.

```tw
      .ACall(callee, args) => {
        case callee_func_id(callee) {
          .Some(fid) => case call_info(sem, fid) {
            .Some(_) => {}, // builtin semantics -> no user-call specialization verdict
            .None => case table.summary_get(fid.id) {
              .Some(s) => {
                au: Vector<Bool> = collect a in args {
                  case atom_local_id(a) {
                    .Some(id) => own_is_unique(pre.own, id) and is_last_use(last, id),
                    .None => false,
                  }
                }
                dec := render_call_decision(fid.id, select_variant(fid.id, s, au))
                if dec.len() > 0 {
                  verdicts[inst.anf_local.id] = dec
                }
              },
              .None => {}, // non-summarized direct user callee -> no decision
            },
          },
          _ => {}, // indirect/closure call
        }
      },
```

- [ ] **Step 4: Run the verdict tests**

Run: `set -o pipefail; make boot-test 2>&1 | grep -aE 'Ran [0-9]+ tests|  x |FAIL' | tail -4`
Expected: both completion tests pass (`be` records `-> f1[unique:p0,p0.f0]` at lid1; `br`
records nothing there). If other suites fail, they are `--cfg` snapshot tests picking up the
new call verdicts — re-baseline them in Task 2 Step 1 (they are new, sound render output).

- [ ] **Step 5: Format, lint**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw
target/twk lint boot/main.tw    # expect only the pre-existing findings; fix any NEW finding
```

---

## Task 2: Re-baseline + verification

- [ ] **Step 1: Re-baseline any `--cfg` snapshot tests that now show call verdicts**

Run the full suite and inspect every non-`completion` failure:

Run: `set -o pipefail; make boot-test 2>&1 | grep -aE 'FAIL|^\s*x ' | tail -40`
For each, confirm it is a `--cfg`/`render_view` snapshot now including a new owned-call
verdict line (sound, new render output) and update the expectation. If a failure is not
explainable as an added owned verdict, **stop** and investigate (the decision may be firing
where `arg_unique` should be false).

- [ ] **Step 2: Census unchanged (analysis-only invariant)**

Run: `set -o pipefail; target/twk ir boot/main.tw --census 2>&1 | awk 'NR>1 && NF>=3 {s+=$NF} END{print "total in_place:", s+0}'`
Expected: `total in_place: 0`.

- [ ] **Step 3: Full boot suite + self-host fixed point (unmasked)**

Run: `make boot-test`
Expected: `Ran N tests: N passed`. Then, run alone:

Run: `set -o pipefail; touch boot/compiler/ownership.tw && make stage2 2>&1 | tail -4`
Expected: `Fixed point reached: stage3 == stage4` (verdicts are render-only; the emitted
`wasm` is unchanged).

- [ ] **Step 4: Byte-identical builds (determinism)**

Run: `set -o pipefail; target/twk build boot/main.tw -o /tmp/p6done_x.wasm && target/twk build boot/main.tw -o /tmp/p6done_y.wasm && cmp /tmp/p6done_x.wasm /tmp/p6done_y.wasm && echo IDENTICAL`
Expected: `IDENTICAL`.

- [ ] **Step 5: Real-code sanity — owned decisions render on `boot/main.tw`**

Run: `set -o pipefail; target/twk ir boot/main.tw --cfg 2>&1 | grep -c '\[unique:'`
Expected: informational only. A positive count means real call sites where a proven-owned argument is threaded into a consuming callee now render their owned-variant decision — the Phase 6 specialization story on real code. Zero is acceptable (the flagship is proven by the boot-suite fixtures); if positive, spot-check one for plausibility.

- [ ] **Step 6: Lint clean + commit**

Run: `target/twk lint boot/main.tw` → no new findings.

```bash
git add boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw
git commit -m "phase6 completion: render per-call-site ownership-specialization decisions

Wire select_variant into block_verdicts: at each user call, compute arg_unique
from the pre-call ownership + last-use, and record an owned-variant verdict
(-> f{id}[unique:...]) when the caller may select a specialized callee; generic
decisions are skipped for --cfg readability. Completes the Phase 6 exit criterion
(Case B owned / Case C generic, verifiable in twk ir --cfg) -- the analysis track
is done. Render-only: census stays 0, wasm byte-identical, self-host fixed point.
Param-threaded/recursive decisions + the machine-readable SpecializationFacts +
field-granular seeding ride to the codegen track (per phase6-design notes)."
```

---

## Deferred (tracked → codegen track)

| Item | Home |
|---|---|
| Rendering the `generic` decision (every fallback prints its reason) | render refinement (skipped for noise) |
| Machine-readable `SpecializationFacts` / `CallDecision` table (ANF-keyed) | codegen handoff |
| Param-threaded/recursive-callee decisions via demanded owned summaries + SCC variant fixpoint (D12) | codegen track |
| `ConsumedPaths` + D6 disjoint-sibling read-rule + field-granular seeding | codegen track |
| Variant cap (D7) | codegen track |

## Self-Review

- **Completes the stated exit:** the README's Phase 6 exit is a verifiable per-call-site decision in `twk ir --cfg`; this renders exactly that for Case B (owned) / Case C (generic), reusing the existing verdict pipeline.
- **Minimal, low-risk:** one `.ACall` case in `block_verdicts` + one render helper; `arg_unique` is derived from the pre-instruction `pre.own` + `last` the loop already carries, identical to `transfer_summarized_call`. No pass, `ForwardState`, or driver change.
- **Render-only, provably no codegen effect:** verdicts are strings in `blk.exit.verdicts`, consumed only by `cfg.tw` rendering; census stays 0, the `wasm` is byte-identical, and the self-host fixed point holds — the change cannot alter emitted code.
- **Sound under-approximation:** an owned verdict is recorded only when the callee's generic summary advertises a consumed param **and** the argument is `arg_unique`; a param-threaded callee (generic `Published`/empty) yields no owned verdict rather than a wrong one — the deferred owned-summary/SCC precision only *adds* verdicts later, never corrects a wrong one.
- **Honest scope:** generic-render, the machine-readable table, and the deeper variant machinery are deferred to codegen with reasons, matching the re-scoping now recorded in `phase6-design.md`.
- **Type consistency:** `render_call_decision(callee_id: Int, v: vid.VariantId) String`; `select_variant(fid.id, s, au)`; `au: Vector<Bool>` from `own_is_unique(pre.own, id) and is_last_use(last, id)`; `callee_func_id`/`atom_local_id`/`table.summary_get` all in `block_verdicts` scope.
- **Verification not masked:** every `make boot-test`/`make stage2` check uses `set -o pipefail` or is run unpiped; `--cfg` snapshot re-baselines are per-failure-justified; `twk lint` after edits.
