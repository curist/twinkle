# Phase 6 Part 2, Stage 2b — Helper-Call Requirement Propagation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend Stage 2a's dirty-path dataflow with a **call-result transfer** so that `collect_field_reqs` propagates a parameter's origin + a consuming callee's in-place field paths through helper calls (`env = env.add_type(...); env = env.bind_type(...)`), producing the transitive **candidate** requirement set that Stage 3 consumes.

**Architecture:** Stage 2a's forward dirty-path dataflow tracks, per value, its origin parameter and dirtied field paths, but breaks the chain at every `ACall` (call results get origin *none*). Stage 2b adds one transfer rule: a call to a summarized user function whose **single returned parameter** carries in-place field requirements hands that parameter's **origin** and its **non-shell field paths** back to the call result. Still analysis-only — `twk ir --census` stays **0 in-place**.

> **Scope correction (discovered during execution — load-bearing):** Stage 2b wires the propagation into `collect_field_reqs`, but it does **not** make a param-threaded caller classify `Consumed` at the summary level. A helper that returns its parameter classifies `ret = MayAliasParams(k)`, so `transfer_summarized_call` **publishes** that argument at the call site (`ownership.tw:1160`); the caller's threaded param therefore ends `Retained` and `reconcile_role` forces `Published` + empty `in_place_paths`, discarding the candidate. This is the **Phase 5 param-threaded deferral**, and closing it (param enters `Unique` → the recovery gate fires → not published → the candidate survives) is **Stage 3's owned-entry re-analysis**, not this stage. `collect_field_reqs` is a *separate* dataflow that computes the candidate regardless of publication — so Stage 2b is verified by (a) a **direct** `collect_field_reqs` unit test (the candidate is computed) and (b) a **deferral-boundary guard** (the generic summary stays `Published`, i.e. Stage 2b does not unsoundly surface `Consumed`). The Stage 2a plan's note that "`add_type` becomes an acceptance test in 2b" was imprecise: the *caller*'s `Consumed` manifestation is a later-stage acceptance.
>
> **Boundary refinement (from Stage 3 planning — supersedes "Stage 3" below):** recovery splits by the helper's return shape. A **ret-path transport** helper (returns a fresh `Wrapper.{ f0: ctx }` via `ret_path OwnedFromParam`) is recovered in **Stage 3** by seeding the param `Unique` (the `arg_unique` gate un-publishes). But this plan's fixtures — and `add_type` — return the **whole** param, so `ret = MayAliasParams(k)`, which `transfer_summarized_call` publishes **unconditionally** (`ownership.tw:1160`), *regardless* of seeding. That whole-value case is **Stage 4** (owned-variant selection **plus** a whole-return move representation), **not Stage 3**. Where prose below says "Stage 3" for the `g`/`add_type` whole-value manifestation, read **Stage 4**. See `docs/plans/2026-07-19-phase6-stage3-owned-entry-reanalysis.md`.

**Tech Stack:** Twinkle (`.tw`), boot compiler only. Tests via the boot suite. Build/verify with `make boot-test`.

---

## Why this design (context)

Stage 2a's `collect_field_reqs` (`ownership.tw`) tracks only `ARecordUpdate` (record→record, add `[f]`) and `AAssign` (rebind); every other op — including `ACall` — leaves the result with origin *none*, so the chain breaks. That is correct for Stage 2a's scope (a directly-mutating function like `add_type`, whose own `env.types = env.types.set(...)` **is** an `ARecordUpdate` and already classifies `Consumed paths{[],[.types]}`).

The gap Stage 2b closes is the **caller** that never mutates directly but **threads a parameter through consuming helpers**:

```tw
fn add_type(env, name, id) { env.types = env.types.set(name, id); env }   // 2a: p0 Consumed {[],[.types]}

fn build_step(env) {              // 2b: collect_field_reqs computes candidate {[.types]} for p0
  env = add_type(env, "A", 1)     // rebind-to a CALL result — origin none in 2a
  env = add_type(env, "B", 2)
  env
}
```

In Stage 2a, `add_type(env, …)`'s result has origin *none*, so `env`'s chain breaks after the first call and `collect_field_reqs` attributes nothing to `build_step`'s parameter. Stage 2b makes the call result inherit the threaded parameter's origin plus `add_type`'s consumed field paths, so `collect_field_reqs` accumulates the candidate `{[.types]}` for `build_step`'s parameter.

**But the candidate does not surface as `Consumed` on `build_step`'s summary in this stage** — see the Scope correction above: `add_type` returns `alias(p0)`, so each call publishes `env`, and `reconcile_role`'s `Published` precedence discards the candidate. Stage 3's owned-entry re-analysis (param enters `Unique` → not published) is what surfaces it. Stage 2b's job is to make `collect_field_reqs` *produce* the candidate; Stage 3 makes it *survive*.

### The propagation rule (exact)

At `let L = call f(a0, a1, …)` where `f` is a **summarized user function** (`AGlobalFunc(fid)`, `summary_get(table, fid)` is `.Some(s)`):

- Find every parameter position `i` such that `i < args.len()`, `s.params[i].flows_to_return` is true, and the argument `args[i]` has a dirty-path fact with `origin >= 0`.
- If **exactly one** such position `j` exists, the result `L` takes:
  - `origin` = `args[j]`'s origin (the parameter whose region the callee hands back), and
  - `dirty` = `args[j]`'s dirty set **∪** the **non-shell** paths of `s.params[j].in_place_paths` (the fields the callee mutates in place, which — same record type — are the same field ids on the caller's argument).
- Otherwise (zero carriers, or an **ambiguous** return aliasing multiple parameters), the result is origin *none* (the chain breaks, exactly as Stage 2a).

Using `flows_to_return` (not `base_role == Consumed`) as the origin-carry condition also threads a **pure transport** helper (`fn wrap(env){env}`: `Borrowed`, flows, empty `in_place_paths`) — the origin propagates and *no* dirty path is added, which is exactly right. A `Consumed` carrier additionally contributes its field paths.

### Soundness posture (why this is safe now)

The dirty-path set is a **candidate requirements readout**, not a proof — the design (`phase6-design.md`, "Requirement collection") is explicit: *"a requirements readout, not proof … the candidate set the owned-entry pass validates."* No codegen consumes it (`--census` stays 0). An over-optimistic candidate is filtered by Stage 3's owned-entry re-analysis and Stage 4's call-site `consume_dead` gate. So Stage 2b may propagate freely without a soundness gate here.

### SCC / recursion

`collect_field_reqs` now reads `table`. It is called from `summarize_function`, which the SCC driver (`summary.tw` `compute`/`run_scc`) re-runs with the current callee summaries (callee-first order; in-SCC members re-summarized on change). A callee in an **earlier** SCC (the `add_type` case) is fully summarized before its caller, so the caller sees the final summary. An in-SCC (mutually recursive) callee is re-read each iteration; its `in_place_paths` grows monotonically and `same_summary` already compares `in_place_paths` (via `PathSet.same`), so the fixpoint terminates. No driver change is needed — requirement collection ascends alongside the existing generic fixpoint.

### Scope

**In scope (2b):** propagate origin + non-shell field paths through a call to a summarized user function whose single returned parameter carries requirements. Worked example: a caller threading a parameter through `add_type`-shaped consuming helpers → `p0=Consumed paths{[],[.f…]}`.

**Explicitly out of scope (honest, not faked):**
- **Field-projection arguments** (`helper(env.sub, …)`): Stage 2a's `FlowFact` tracks only whole-parameter origin (an `ARecordGet` result has origin *none*), so passing a *field of* a parameter to a helper does not propagate. This matches Blocker 1's field-only, shallow depth cap. Deeper argument paths are a later refinement.
- **Ambiguous multi-parameter returns** (`if c a else b`): two flowing parameters ⇒ origin *none* (conservative). No attempt to split.
- **Reference-vs-scalar field filter** → still Stage 2c. 2b reports candidate field paths exactly as 2a does.
- **Owned-entry validation / call-site decision** → Stages 3–4. 2b only grows the candidate readout.

### File structure

- **Modify `boot/compiler/ownership.tw`:** thread `table: SummaryTable` into `transfer_flow` and `collect_field_reqs`; add the `ACall` transfer case + a `call_result_fact` helper; update the one `summarize_function` call site to pass `table`.
- **Test `boot/tests/suites/cfg_summary_suite.tw`:** update the three Stage-2a `collect_field_reqs` unit call sites to pass `ownership.empty_summary_table()`; add a localized `collect_field_reqs` unit test (hand-built table) + its negative; add summary-level acceptance tests via `compute_of` (real two-function module).

Verified anchors (current branch): `Atom.AGlobalFunc(FuncId)` (`anf.tw:19`), `summary_get(t: SummaryTable, func_id: Int) Summary?` used inherent as `table.summary_get(fid)` (`ownership.tw:75`, call site `:994`), positional arg↔param mapping `for ps, i in s.params { … args[i] }` (`transfer_summarized_call`, `ownership.tw:1144`), `transfer_flow` (`ownership.tw:3347`), `collect_field_reqs` (`ownership.tw:3447`), finalization `field_reqs := collect_field_reqs(f)` (`ownership.tw:3674`), Stage-2a unit call sites (`cfg_summary_suite.tw:1401,1420,1460`), `compute_of`/`summ_of` helpers (`cfg_summary_suite.tw:173,179`), `SummaryTable = .{ by_func: Dict<Int, Summary> }` (`ownership.tw`), `p_role`/`role_tag`/`dirty_fields`/`cfg_func_of`/`same_ints` test helpers already present.

---

## Task 1: Call-result propagation in the dirty-path dataflow

**Files:**
- Modify: `boot/compiler/ownership.tw` (`transfer_flow` + new `call_result_fact` + `collect_field_reqs` signature + finalization call site)
- Test: `boot/tests/suites/cfg_summary_suite.tw` (fix 3 call sites; add 2 localized unit tests)

- [ ] **Step 1: Update the three Stage-2a unit call sites to the new arity (they must still compile after Step 3)**

In `cfg_summary_suite.tw`, the three existing calls (`:1401`, `:1420`, `:1460`) currently read:

```tw
        reqs := ownership.collect_field_reqs(cfg_func_of("f", 1, body))
```
```tw
        reqs := ownership.collect_field_reqs(cfg_func_of("f", 1, body))
```
```tw
        reqs := ownership.collect_field_reqs(cfg_func_of("f", 2, body))
```

Change each to pass an empty table (no user callees in those fixtures):

```tw
        reqs := ownership.collect_field_reqs(cfg_func_of("f", 1, body), ownership.empty_summary_table())
```
```tw
        reqs := ownership.collect_field_reqs(cfg_func_of("f", 1, body), ownership.empty_summary_table())
```
```tw
        reqs := ownership.collect_field_reqs(cfg_func_of("f", 2, body), ownership.empty_summary_table())
```

- [ ] **Step 2: Write the failing localized unit tests (propagation + negative)**

Add these two tests in `cfg_summary_suite.tw` immediately after the existing
`"collect_field_reqs: field updates across a branch join both reach return"` test
(the last `collect_field_reqs:` unit test, ending near `:1461`):

```tw
    .test(
      "collect_field_reqs: param threaded through a consuming helper propagates its fields",
      fn() {
        // helper (func 7): p0 Consumed paths{[],[.f0]}, returns p0 (flows).
        helper := ownership.Summary.{
          params: [
            ownership.ParamSummary.{
              base_role: .Consumed,
              in_place_paths: variant_id.PathSet.{ paths: [variant_id.shell(), variant_id.field(0)] },
              flows_to_return: true,
            },
          ],
          ret: .MayAliasParams([0]),
          ret_paths: [],
        }
        tbl: Dict<Int, ownership.Summary> = Dict.new()
        tbl[7] = helper
        table := ownership.SummaryTable.{ by_func: tbl }
        // fn g(env) { r1 := helper(env); r2 := helper(r1); r2 }  -- thread env through twice.
        body: AnfExpr = .Let(
          lid(1),
          .ACall(.AGlobalFunc(FuncId.{ id: 7 }), [.ALocal(lid(0))]),
          .Let(
            lid(2),
            .ACall(.AGlobalFunc(FuncId.{ id: 7 }), [.ALocal(lid(1))]),
            .Atom(.ALocal(lid(2))),
          ),
        )
        reqs := ownership.collect_field_reqs(cfg_func_of("g", 1, body), table)
        try assert.is_true(same_ints(dirty_fields(reqs, 0), [0])) // env's .f0 propagated
        .Ok({})
      },
    )
    .test(
      "collect_field_reqs: a fresh-returning helper breaks the chain",
      fn() {
        // helper (func 7): returns a FRESH value (no param flows) -> no propagation.
        helper := ownership.Summary.{
          params: [
            ownership.ParamSummary.{
              base_role: .Borrowed,
              in_place_paths: variant_id.empty_set(),
              flows_to_return: false,
            },
          ],
          ret: .OwnedFresh,
          ret_paths: [],
        }
        tbl: Dict<Int, ownership.Summary> = Dict.new()
        tbl[7] = helper
        table := ownership.SummaryTable.{ by_func: tbl }
        // fn g(env) { r1 := helper(env); r1 }  -- r1 is fresh, origin none.
        body: AnfExpr = .Let(
          lid(1),
          .ACall(.AGlobalFunc(FuncId.{ id: 7 }), [.ALocal(lid(0))]),
          .Atom(.ALocal(lid(1))),
        )
        reqs := ownership.collect_field_reqs(cfg_func_of("g", 1, body), table)
        try assert.equal(dirty_fields(reqs, 0).len(), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 3: Run to verify failure (arity mismatch / undefined behavior)**

Run: `set -o pipefail; target/twk build boot/tests/main.tw -o /tmp/s2b_t.wasm 2>&1 | tail -8`
Expected: a compile error — `collect_field_reqs` takes 1 argument but the new tests (and, after this step is applied, the fixed Stage-2a sites) pass 2. This confirms the tests exercise the new signature before it exists.

- [ ] **Step 4: Thread `table` into the dataflow and add `call_result_fact`**

In `ownership.tw`, add `call_result_fact` immediately **above** `transfer_flow`
(`:3347`):

```tw
// Transitive requirement propagation (Stage 2b): a call to a summarized user
// function whose returned parameter carries in-place field requirements hands those
// requirements (and that parameter's origin) back to the call result, so a parameter
// threaded through consuming helpers (env = env.add_type(...)) accumulates their
// field paths. Fires only when EXACTLY ONE parameter position both flows to the
// callee's return and receives an origin-bearing argument (else the origin is
// ambiguous -> none). Builtins and unknown callees break the chain (origin none).
// A requirements readout, not a proof: Stage 3's owned-entry pass validates it.
fn call_result_fact(
  st: Dict<Int, FlowFact>,
  table: SummaryTable,
  callee: Atom,
  args: Vector<Atom>,
) FlowFact {
  fid := case callee {
    .AGlobalFunc(f) => f.id,
    _ => return ff_none(),
  }
  s := case table.summary_get(fid) {
    .Some(x) => x,
    .None => return ff_none(),
  }
  carrier := 0 - 1
  count := 0
  for ps, i in s.params {
    if i < args.len() and ps.flows_to_return and flow_get(st, args[i]).origin >= 0 {
      carrier = i
      count = count + 1
    }
  }
  if count != 1 {
    return ff_none()
  }
  base := flow_get(st, args[carrier])
  dirty := base.dirty
  for p in s.params[carrier].in_place_paths.paths {
    if !p.is_shell() {
      dirty = dirty.add(p)
    }
  }
  FlowFact.{ origin: base.origin, dirty }
}
```

Then change `transfer_flow`'s signature and add the `ACall` case. Replace:

```tw
fn transfer_flow(st: Dict<Int, FlowFact>, op: AnfOp, result: Int) Dict<Int, FlowFact> {
  case op {
    .ARecordUpdate(base, fld, _, _, _) => {
      bf := flow_get(st, base)
      st[result] = if bf.origin >= 0 {
        FlowFact.{ origin: bf.origin, dirty: bf.dirty.add(vid.field(fld.id)) }
      } else {
        ff_none()
      }
      st
    },
    .AAssign(local, a) => {
      st[local.id] = flow_get(st, a)
      st
    },
    _ => st,
  }
}
```

with:

```tw
fn transfer_flow(
  st: Dict<Int, FlowFact>,
  table: SummaryTable,
  op: AnfOp,
  result: Int,
) Dict<Int, FlowFact> {
  case op {
    .ARecordUpdate(base, fld, _, _, _) => {
      bf := flow_get(st, base)
      st[result] = if bf.origin >= 0 {
        FlowFact.{ origin: bf.origin, dirty: bf.dirty.add(vid.field(fld.id)) }
      } else {
        ff_none()
      }
      st
    },
    .AAssign(local, a) => {
      st[local.id] = flow_get(st, a)
      st
    },
    .ACall(callee, args) => {
      st[result] = call_result_fact(st, table, callee, args)
      st
    },
    _ => st,
  }
}
```

- [ ] **Step 5: Thread `table` through `collect_field_reqs` and update the finalization call site**

In `collect_field_reqs` (`:3447`), change the signature and the single
`transfer_flow` call inside the instruction loop. The header:

```tw
pub fn collect_field_reqs(f: CfgFunction) Dict<Int, vid.PathSet> {
```

becomes:

```tw
pub fn collect_field_reqs(f: CfgFunction, table: SummaryTable) Dict<Int, vid.PathSet> {
```

and inside the block loop:

```tw
      for inst in blk.instructions {
        st = transfer_flow(st, inst.op, inst.anf_local.id)
      }
```

becomes:

```tw
      for inst in blk.instructions {
        st = transfer_flow(st, table, inst.op, inst.anf_local.id)
      }
```

Finally, in `summarize_function` (`:3674`) update the caller — `table` is already a
parameter of `summarize_function`:

```tw
  field_reqs := collect_field_reqs(f, table)
```

- [ ] **Step 6: Run the unit tests to verify they pass**

Run (pipefail-safe): `set -o pipefail; make boot-test 2>&1 | grep -aE 'Ran [0-9]+ tests|FAIL' | tail -3`
Expected: `Ran N tests: N passed` (the propagation test yields `[0]`; the fresh-returning negative yields `[]`).

- [ ] **Step 7: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw    # expect only the 4 pre-existing inherent-calls findings; fix any NEW finding
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "phase6 stage2b: propagate in-place field requirements through helper calls

Extend the dirty-path dataflow with a call-result transfer: a call to a
summarized user function whose single returned parameter carries in-place field
paths hands that parameter's origin and non-shell field paths back to the call
result. A parameter threaded through consuming helpers now accumulates their
paths instead of breaking the chain at the call. Reads callee summaries via the
threaded SummaryTable; ambiguous multi-param returns and builtins break the
chain. Analysis only (census stays 0); candidates validated in Stage 3."
```

---

## Task 2: Deferral-boundary guard (summary-level, via `compute`)

The Scope correction above is the reason this is a **guard**, not a `Consumed`
acceptance: a param threaded through a helper that returns it is **published** at
the call site in the generic pass, so the caller stays `Published` with empty
`in_place_paths`. `collect_field_reqs` still computes the candidate (proven by the
Task 1 unit test); this task pins that Stage 2b must **not** unsoundly surface
`Consumed` on such a param — the manifestation is a Stage 3 acceptance.

**Files:**
- Test: `boot/tests/suites/cfg_summary_suite.tw` (whole-program `compute_of` fixture)

- [ ] **Step 1: Write the boundary-guard test**

Add after the Task 1 unit tests. `compute_of`/`summ_of`/`p_role`/`role_tag` are
existing helpers; `fdef(id, name, nparams, body)` builds a function with that id.

```tw
    .test(
      "phase6 stage2b: param-threaded caller stays Published in the generic pass (Stage 3 boundary)",
      fn() {
        // fn h(env) { env.f0 = 0 ; env2 }  -- 2a gives h: p0 Consumed {[],[.f0]}, ret alias(p0).
        h_body: AnfExpr = .Let(
          lid(1),
          .ARecordUpdate(.ALocal(lid(0)), FieldId.{ id: 0 }, .ALitInt(0), false, TypeId.{ id: 0 }),
          .Atom(.ALocal(lid(1))),
        )
        // fn g(env) { env = h(env); env = h(env); env }  -- thread env through h twice.
        g_body: AnfExpr = .Let(
          lid(1),
          .ACall(.AGlobalFunc(FuncId.{ id: 1 }), [.ALocal(lid(0))]),
          .Let(
            lid(2),
            .ACall(.AGlobalFunc(FuncId.{ id: 1 }), [.ALocal(lid(1))]),
            .Atom(.ALocal(lid(2))),
          ),
        )
        t := compute_of([fdef(1, "h", 1, h_body), fdef(2, "g", 1, g_body)])
        h := summ_of(t, 1)
        g := summ_of(t, 2)
        // h itself (Stage 2a direct update): Consumed with shell + field.
        try assert.equal(p_role(h, 0), role_tag(.Consumed))
        try assert.equal(h.params[0].in_place_paths.paths.len(), 2)
        // g threads env through h, which returns alias(p0), so the call PUBLISHES env in
        // the generic pass (the Phase-5 param-threaded deferral). collect_field_reqs still
        // computes the candidate {[.f0]} (see the collect_field_reqs propagation unit test),
        // but reconcile_role's Published precedence discards it here. Stage 3's owned-entry
        // re-analysis (env enters Unique -> recovery gate fires -> not published) is what
        // reclassifies g Consumed. This guard pins the boundary: Stage 2b must NOT surface
        // Consumed on a published param.
        try assert.equal(p_role(g, 0), role_tag(.Published))
        try assert.equal(g.params[0].in_place_paths.paths.len(), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify it passes**

Run: `set -o pipefail; make boot-test 2>&1 | grep -aE 'Ran [0-9]+ tests|FAIL' | tail -3`
Expected: `Ran N tests: N passed`. If `g` comes back `Consumed` here, Stage 2b is
unsoundly leaking the candidate past the `Published` gate — investigate before
proceeding (the whole point of the guard). If `h` is not `Consumed`, the Stage 2a
direct-update classification regressed.

> **Confirm the diagnosis if surprised:** temporarily add
> `println("g: ${summary.render_summary(g)}")` — it should print
> `g: p0=Published ret=alias(p0)`. Remove the print before committing.

- [ ] **Step 3: Format, lint, commit**

```bash
target/twk fmt boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/tests/suites/cfg_summary_suite.tw
git commit -m "phase6 stage2b: guard the param-threaded deferral boundary

A caller threading its param through a helper that returns an alias of it is
published at the call site in the generic pass, so reconcile_role classifies it
Published with empty in_place_paths -- even though collect_field_reqs computes
the propagated candidate. Pins the Phase-5 param-threaded deferral: Stage 2b must
not surface Consumed on a published param; Stage 3 owned-entry re-analysis
reclassifies it."
```

---

## Task 3: Stage 2b verification

**Files:** none (verification only). All commands use `set -o pipefail` so a `make` failure is not masked by a trailing `grep`/`tail`.

- [ ] **Step 1: Census unchanged (analysis-only invariant)**

Run: `set -o pipefail; target/twk ir boot/main.tw --census 2>&1 | awk 'NR>1 && NF>=3 {s+=$NF} END{print "total in_place:", s+0}'`
Expected: `total in_place: 0`.

- [ ] **Step 2: Real-code sanity — generic summaries are UNCHANGED**

Run: `set -o pipefail; target/twk ir boot/main.tw --cfg 2>&1 | grep -c 'Consumed paths{\[\],\[\.f'`
Expected: the **same** count as before Stage 2b (capture it on `HEAD~` if unsure). This is the point of the Scope correction: because param-threaded callers are published in the generic pass, the propagated candidate never surfaces on a summary, so Stage 2b renders **no new** field-path classifications. The change is a pure no-op on generic output (a stronger statement than "≥ before"), which — together with the byte-identical build (Step 5) and self-host fixed point (Step 3) — is the analysis-only guarantee. The Task 1 unit test is the authoritative proof the candidate is actually computed.

- [ ] **Step 3: Full boot suite + self-host fixed point (unmasked)**

Run: `make boot-test`
Expected (read the tail): `Ran N tests: N passed`. Then, run alone (no parallel heavy `twk`):

Run: `set -o pipefail; touch boot/compiler/ownership.tw && make stage2 2>&1 | tail -4`
Expected: `Fixed point reached: stage3 == stage4`.

- [ ] **Step 4: Lint clean**

Run: `target/twk lint boot/main.tw`
Expected: no new findings beyond the 4 pre-existing `inherent-calls` findings in `variant_id.tw` (`--explain` for rationale).

- [ ] **Step 5: Byte-identical builds (determinism)**

Run: `set -o pipefail; target/twk build boot/main.tw -o /tmp/p6s2b_x.wasm && target/twk build boot/main.tw -o /tmp/p6s2b_y.wasm && cmp /tmp/p6s2b_x.wasm /tmp/p6s2b_y.wasm && echo IDENTICAL`
Expected: `IDENTICAL` (the propagated paths ride the same canonical-sorted `PathSet`, so no ordering nondeterminism).

---

## Deferred (tracked)

| Item | Home |
|---|---|
| Field-projection arguments (`helper(env.sub, …)`) propagating | later refinement (needs a deeper `FlowFact`, beyond Blocker 1's field-only shallow cap) |
| Reference-vs-scalar field filter (needs per-field type metadata not on `CfgFunction`) | **Stage 2c** |
| Owned-entry re-analysis validating these candidate paths | **Stage 3** |
| Call-site `consume_dead` decision + `ConsumedPaths` | **Stage 4** |
| SCC variant fixpoint + variant cap | **Stage 5** |

## Self-Review

- **Spec coverage:** The Stage 2a plan's deferral to 2b — *"helper-call propagation"* — is implemented by the `ACall` transfer (Task 1). Its **directly-observable** effect (the propagated candidate) is pinned by the Task 1 `collect_field_reqs` unit test; its **non-effect** on the generic summary (the `Published` gate) is pinned by the Task 2 boundary guard. The `add_type` *caller*'s `Consumed` manifestation is **not** claimed here — it is a Stage 3 acceptance (see the Scope correction).
- **Real-lowering fidelity:** the fixtures thread `env` through `h` via chained `ACall` results (`r1 := h(env); r2 := h(r1); r2`), the shape source `env = env.h(); env = env.h()` lowers to. `h`'s own summary comes from Stage 2a's direct `ARecordUpdate`, so the two stages compose exactly as in production.
- **Honest scope, corrected during execution:** the original Task 2 asserted the threaded caller classifies `Consumed`. Execution proved that false — the helper returns `MayAliasParams(k)`, so the call publishes the arg (`ownership.tw:1160`) → `Published` → the candidate is discarded by `reconcile_role`. The plan now asserts the true generic-pass outcome (`Published`) and documents the Stage 3 boundary rather than pretending the manifestation lands in 2b.
- **Meaningful, not a fake pass:** the Task 1 negative (fresh-returning helper → origin none) confirms propagation fires **only** through a flowing carrier; the Task 2 guard confirms Stage 2b stays sound-conservative (does not leak `Consumed` past the `Published` gate). Together they show the propagation is computed but correctly not yet surfaced.
- **No faked filter:** scalar-vs-reference filtering stays deferred to Stage 2c; 2b reports candidate field paths and the deferral table says so.
- **Soundness posture stated:** the propagation is a candidate readout (no census change), validated by Stage 3 — so no in-place gate is needed in 2b.
- **Type consistency:** `call_result_fact(st, table, callee, args) FlowFact`, `transfer_flow(st, table, op, result)`, and `collect_field_reqs(f, table)` are used consistently; `s.params[j].in_place_paths` is a `PathSet` (`.paths` iterated, `.is_shell()` per `ParamPath`), matching the nominal refactor. The single production `collect_field_reqs` call site and the three Stage-2a unit call sites are all updated to the 2-arg form.
- **Determinism/termination:** the transfer is monotone (dirty grows via `PathSet.add`; origin only degrades), reads callee summaries the SCC driver already fixes callee-first, and adds no new driver state — so the generic fixpoint's termination and byte-identical output are preserved (Task 3 Steps 3, 5).
- **Verification not masked:** every `make boot-test`/`make stage2` check uses `set -o pipefail` or is run unpiped; `twk lint` is run after edits.
