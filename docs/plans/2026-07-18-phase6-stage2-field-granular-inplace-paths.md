# Phase 6 Part 2, Stage 2 — Field-Granular `in_place_paths` Implementation Plan

> **⚠️ SUPERSEDED (2026-07-18) — DO NOT EXECUTE.** Review found this plan's direct-parameter syntactic scan does not survive real lowering: (1) real `add_type` is chained helper calls (`resolver.tw:491`), no direct `ARecordUpdate`; (2) `RecordUpdate` lowers to a **fresh SSA local** (`lower_anf.tw:785`), so in an update chain only the *first* update has `base = param` — the scan misses the rest, and the multi-field test used a shape real lowering never produces; (3) it checks whole-param flow, not update-*result* flow, so `tmp := (p.f=v); return p` is a false positive. Replaced by a **flow-aware dirty-path** design, split into two stages:
> - **Stage 2a** — `docs/plans/2026-07-18-phase6-stage2a-dirty-path-requirements.md` (intraprocedural prov/value-flow record-update requirements; acceptance = a `register_type_entry`-shaped mutator, NOT `add_type`).
> - **Stage 2b** — helper-call summary propagation (claims `add_type`); its own plan, written after 2a lands.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refine the generic-pass parameter summary so a parameter that is **record-field-updated in a way that flows to the return** classifies as `Consumed` and carries the exact field paths it mutates — making `add_type` render `p0=Consumed paths{[],[.f0]}` instead of Part 1's coarse shell-only output.

**Architecture:** Adopts the canonical "would-mutate ⇒ `Consumed`" model (`summary-specialization.md`): `base_role` is a syntactic/effect classification, not the generic fixpoint's move-based consume. A cheap syntactic scan (`collect_mut_paths`) finds `ARecordUpdate` sites whose base is a parameter and records the updated field; `reconcile_role` gains a `has_mut` input so such params become `Consumed` when they flow to return; `in_place_paths` is built from the collected fields (shell `[]` + each `[f]`). Still analysis-only — `twk ir --census` stays **0 in-place**.

**Tech Stack:** Twinkle (`.tw`), boot compiler only. Tests via the boot suite (`boot/tests/suites/cfg_summary_suite.tw`). Build/verify with `make boot-test`.

---

## Where this sits + the model decision

Part 1 populated `in_place_paths` only for params the generic fixpoint proved `Consumed` (via an explicit move + consume), at **shell `[]` granularity**. But in the generic pass every reference param enters `Unknown` (`ownership.tw` `consume_base` at `:884` only consumes a `Unique` base), so a param that is field-updated **without** an explicit move — the `add_type` shape `env.types = env.types.set(...)` — stays `Borrowed` and gets **no** paths under Part 1.

The canonical model (`docs/plans/sound-uniqueness/analysis/summary-specialization.md`, worked-summaries table line 218: `add_type | p0: Consumed, paths {[], [.types]}`) treats `base_role = Consumed` as **"the callee would mutate/move it"** — a syntactic/effect classification. This stage adopts that model (decision confirmed with the maintainer).

**Consequence (flagged):** this **broadens** when a param is `Consumed`. A param with a direct `param.field = …` update that flows to return flips `Borrowed → Consumed`. No current Part-1 fixture does that (they use `AInit`+`Dict.set` moves, or fresh-record construction), so **no Part-1 test is expected to re-baseline** — but `make boot-test` is the arbiter; if a role/render expectation shifts, update it to the new (correct) classification.

### Scope

**In scope (this stage):** direct-parameter **record-update** mutation sites — `ARecordUpdate(base, f, …)` where `base` is a parameter local. Covers `add_type` (acceptance **#1**: `paths{[],[.f0]}`).

**Deferred (near-term follow-ups, their own plans):**
- **Collection-consume on a param** (`set_at` wrapper `flags.set_at(k,v)` → `paths{[]}`, Case A / acceptance #2): needs the call-semantics `cow_base_arg` lookup to detect a consuming collection call whose base arg is a param. Straightforward next increment ("Stage 2b").
- **Transitive mutation sites:** base derived from a param through intermediate locals (needs provenance). A precision refinement — its absence only under-collects candidates (sound; the owned-entry pass, Stage 3, is the arbiter).
- **`ConsumedPaths` + `consume_dead` + acceptance #5/#7/#8:** these are call-site-decision concerns and move to **Stage 4** (they are not testable without the decision machinery). The roadmap grouped them with Stage 2, but they depend on Stages 3–4.

Requirement collection here is a **syntactic candidate readout**, exactly as the canonical doc states ("does not assert the generic call is in-place-capable; the owned-entry pass validates"). Rendering candidate paths in the generic summary is the intended behavior.

### File structure

- **Modify `boot/compiler/ownership.tw`:** add `collect_mut_paths` (syntactic scan), `mut_get` (Dict helper), `build_in_place_paths`; broaden `reconcile_role` with a `has_mut` parameter; wire both into `summarize_function`'s param finalization (replacing the coarse `case role { .Consumed => [[]], _ => [] }`).
- **Test `boot/tests/suites/cfg_summary_suite.tw`:** add Stage-2 fixtures (record-update param → `Consumed`+field path; update-not-flowing → `Borrowed`; two fields; render).

Verified anchors (all exist on the current branch): `atom_local_id` (`ownership.tw:80`), `param_index_of(params: Vector<LocalId>, local_id) Int?` (`:3025` — `CfgFunction.params` is `Vector<LocalId>`), `insert_sorted` (`:463`), `AnfOp.ARecordUpdate(Atom, FieldId, Atom, Bool, TypeId)` (`anf.tw:50`), `AnfOp.ARecordGet(Atom, FieldId, TypeId)` (`anf.tw:49`), `AnfOp.AAssign(LocalId, Atom)` (`anf.tw:55`). Part-1 `reconcile_role`/`build`-site live in `summarize_function`'s finalization collect (`collect rp, i in raw_params`).

---

## Task 1: Syntactic mutation-site scan + broadened `Consumed` + field paths

**Files:**
- Modify: `boot/compiler/ownership.tw` (add 3 fns, broaden `reconcile_role`, wire finalization)
- Test: `boot/tests/suites/cfg_summary_suite.tw`

- [ ] **Step 1: Write the failing test — a param record-field update flips to `Consumed` with its field path**

Add to `suite()` in `cfg_summary_suite.tw`:

```tw
    .test(
      "phase6 stage2: param record-field update flows to return -> Consumed with [.f0]",
      fn() {
        b := b_reg()
        // fn f(env) { t := env.f0; t2 := Dict.set(t,1,2); env2 := (env.f0 = t2); env2 }
        set_call: AnfOp = .ACall(
          .AGlobalFunc(b.method_id("Dict", "set")),
          [.ALocal(lid(1)), .ALitInt(1), .ALitInt(2)],
        )
        upd: AnfOp = .ARecordUpdate(
          .ALocal(lid(0)), FieldId.{ id: 0 }, .ALocal(lid(2)), false, TypeId.{ id: 0 })
        body: AnfExpr = .Let(
          lid(1), .ARecordGet(.ALocal(lid(0)), FieldId.{ id: 0 }, TypeId.{ id: 0 }),
          .Let(lid(2), set_call, .Let(lid(3), upd, .Atom(.ALocal(lid(3))))),
        )
        s := summ1("f", 1, body)
        try assert.equal(p_role(s, 0), role_tag(.Consumed))
        try assert.is_true(s.params[0].flows_to_return)
        // in_place_paths == [ [], [0] ]
        try assert.equal(s.params[0].in_place_paths.len(), 2)
        try assert.equal(s.params[0].in_place_paths[0].len(), 0) // shell []
        try assert.equal(s.params[0].in_place_paths[1][0], 0)    // [.f0]
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify it fails (param currently classifies Borrowed / no field path)**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -A4 'phase6 stage2: param record-field' | tail -8`
Expected: FAIL — under Part 1 the param is `Borrowed` (enters `Unknown`, no move) so `p_role` is `0` not `1`, and `in_place_paths` is empty.

- [ ] **Step 3: Add the mutation-site scan and helpers in `ownership.tw`**

Near the Part-1 helpers (above `summarize_function`, beside `reconcile_role`/`param_flows_to_return`), add:

```tw
fn mut_get(d: Dict<Int, Vector<Int>>, k: Int) Vector<Int> {
  case d.get(k) {
    .Some(v) => v,
    .None => [],
  }
}

// Syntactic mutation-site scan (canonical "would-mutate"): a parameter that is
// directly record-updated (`env.f = …`) is a Consumed candidate, and field `f` is
// an in-place path. Base must be the parameter local itself (direct); transitive
// param-derived bases are a later refinement (their absence only under-collects,
// which is sound — the owned-entry pass validates). Guarded against a param local
// being reassigned (AAssign) to a non-param value earlier in the body.
fn collect_mut_paths(f: CfgFunction) Dict<Int, Vector<Int>> {
  reqs: Dict<Int, Vector<Int>> = Dict.new()
  reassigned: Dict<Int, Bool> = Dict.new()
  for blk in f.blocks {
    for inst in blk.instructions {
      case inst.op {
        .ARecordUpdate(base, fld, _, _, _) => case atom_local_id(base) {
          .Some(bid) => case param_index_of(f.params, bid) {
            .Some(idx) => case reassigned.get(bid) {
              .Some(_) => {},
              .None => reqs[idx] = insert_sorted(mut_get(reqs, idx), fld.id),
            },
            .None => {},
          },
          .None => {},
        },
        .AAssign(local, _) => reassigned[local.id] = true,
        _ => {},
      }
    }
  }
  reqs
}

// Shell [] (always present, downward-closed under []) plus [f] for each collected
// record-update field. `fields` is already sorted-unique (insert_sorted), so the
// result is deterministic and its order matches same_param_paths comparison.
fn build_in_place_paths(fields: Vector<Int>) Vector<ParamPath> {
  out: Vector<ParamPath> = [[]]
  for fid in fields {
    out = out.append([fid])
  }
  out
}
```

- [ ] **Step 4: Broaden `reconcile_role` to accept `has_mut`**

Replace the Part-1 `reconcile_role` with the version that treats a syntactic mutation site as a consume signal (canonical "would mutate/move"):

```tw
// D9 precedence + canonical would-mutate model: a leaked (Retained) param is always
// Published. Otherwise a param that either has a syntactic mutation site (has_mut)
// OR was move-consumed by the fixpoint (cap), AND flows to the return, is Consumed;
// else Borrowed.
fn reconcile_role(
  esc: EscapeEffect,
  cap: ParamCapability,
  has_mut: Bool,
  flows_to_return: Bool,
) ParamRole {
  case esc {
    .Retained => .Published,
    .Borrowed => {
      consumes := has_mut or case cap {
        .Consumed => true,
        .NoCap => false,
      }
      if consumes and flows_to_return {
        .Consumed
      } else {
        .Borrowed
      }
    },
  }
}
```

- [ ] **Step 5: Wire the scan + broadened role into `summarize_function`'s finalization**

In `summarize_function`, right after the `raw_params` collect (before the return-site loop, where `f` is unshadowed), add:

```tw
  mut_paths := collect_mut_paths(f)
```

Then replace the Part-1 finalization collect (the `params: Vector<ParamSummary> = collect rp, i in raw_params { … }` block that ends `summarize_function`) with:

```tw
  params: Vector<ParamSummary> = collect rp, i in raw_params {
    flows := param_flows_to_return(i, ret, ret_paths_final)
    fields := mut_get(mut_paths, i)
    has_mut := fields.len() > 0
    role := reconcile_role(rp.esc, rp.cap, has_mut, flows)
    ipp: Vector<ParamPath> = case role {
      .Consumed => build_in_place_paths(fields),
      _ => [],
    }
    ParamSummary.{ base_role: role, in_place_paths: ipp, flows_to_return: flows }
  }
```

(This preserves Part 1's behavior for move-consumed params: they have empty `fields`, so `build_in_place_paths([])` = `[[]]` — the same coarse shell path as before.)

- [ ] **Step 6: Run the new test to verify it passes**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -A4 'phase6 stage2: param record-field' | tail -8`
Expected: PASS.

- [ ] **Step 7: Add the negative + multi-field + render tests**

```tw
    .test(
      "phase6 stage2: param field-update that does NOT flow -> Borrowed, no paths",
      fn() {
        b := b_reg()
        // fn f(env) { d := Dict.new(); env2 := (env.f0 = d); 0 }  -- updates but returns 0
        upd: AnfOp = .ARecordUpdate(
          .ALocal(lid(0)), FieldId.{ id: 0 }, .ALocal(lid(1)), false, TypeId.{ id: 0 })
        body: AnfExpr = .Let(lid(1), dict_new_call(b), .Let(lid(2), upd, .Atom(.ALitInt(0))))
        s := summ1("f", 1, body)
        try assert.equal(p_role(s, 0), role_tag(.Borrowed))
        try assert.equal(s.params[0].in_place_paths.len(), 0)
        .Ok({})
      },
    )
    .test(
      "phase6 stage2: two direct field updates -> paths{[],[.f0],[.f1]}",
      fn() {
        b := b_reg()
        // fn f(env) { a := Dict.new(); _ := (env.f0=a); c := Dict.new(); env2 := (env.f1=c); env2 }
        u0: AnfOp = .ARecordUpdate(
          .ALocal(lid(0)), FieldId.{ id: 0 }, .ALocal(lid(1)), false, TypeId.{ id: 0 })
        u1: AnfOp = .ARecordUpdate(
          .ALocal(lid(0)), FieldId.{ id: 1 }, .ALocal(lid(3)), false, TypeId.{ id: 0 })
        body: AnfExpr = .Let(
          lid(1), dict_new_call(b),
          .Let(lid(2), u0,
          .Let(lid(3), dict_new_call(b), .Let(lid(4), u1, .Atom(.ALocal(lid(4)))))),
        )
        s := summ1("f", 1, body)
        try assert.equal(p_role(s, 0), role_tag(.Consumed))
        try assert.equal(s.params[0].in_place_paths.len(), 3) // [], [0], [1]
        try assert.equal(s.params[0].in_place_paths[1][0], 0)
        try assert.equal(s.params[0].in_place_paths[2][0], 1)
        .Ok({})
      },
    )
    .test(
      "phase6 stage2: render shows field paths",
      fn() {
        s := ownership.Summary.{
          params: [
            ownership.ParamSummary.{ base_role: .Consumed, in_place_paths: [[], [0]], flows_to_return: true },
          ],
          ret: .OwnedFresh,
          ret_paths: [],
        }
        try assert.equal(summary.render_summary(s), "summary: p0=Consumed paths{[],[.f0]} ret=fresh")
        .Ok({})
      },
    )
```

- [ ] **Step 8: Run the full boot suite to verify green (and catch any Part-1 re-baseline)**

Run: `make boot-test 2>&1 | grep -E 'Ran [0-9]+ tests|Fixed point|FAIL|error' | tail -6`
Expected: `Ran N tests: N passed` and `Fixed point reached: stage3 == stage4`. If any pre-existing role/render test fails, inspect it: a param doing a direct flowing `param.field = …` update is now correctly `Consumed` — update that expectation to match (and note it in the commit). Run alone; no parallel heavy `twk`.

- [ ] **Step 9: Commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "phase6 stage2: field-granular in_place_paths for record-updated params

Adopt the canonical would-mutate Consumed model: a param directly
record-field-updated (env.f = ...) that flows to the return classifies
Consumed and carries its field paths ([], [.f]...). Syntactic candidate scan
(collect_mut_paths); reconcile_role gains has_mut. add_type-shaped params now
render paths{[],[.f0]}. Move-consumed params keep their shell [] path. Analysis
only; census stays 0. Collection-consume + transitive bases are follow-ups."
```

---

## Task 2: Stage 2 verification

**Files:** none (verification only)

- [ ] **Step 1: Census unchanged (analysis-only invariant holds)**

Run: `target/twk ir boot/main.tw --census 2>&1 | awk 'NR>1 && NF>=3 {s+=$NF} END{print "total in_place:", s+0}'`
Expected: `total in_place: 0`.

- [ ] **Step 2: Full boot suite + self-host fixed point**

Run: `make boot-test 2>&1 | grep -E 'Ran [0-9]+ tests|Fixed point|FAIL' | tail -4`
Expected: `Fixed point reached: stage3 == stage4` and `Ran N tests: N passed`. (Run alone.)

- [ ] **Step 3: Sanity-check a real entry renders field paths**

Run: `target/twk ir boot/main.tw --cfg 2>&1 | grep -m5 'paths{\[\],\[\.f'`
Expected: at least one real function whose param is directly field-updated and returned now renders `p{i}=Consumed paths{[],[.f…]}` (field-granular), not just `paths{[]}`. If none appear, that is acceptable (depends on whether boot source has a direct-param field-update-and-return shape) — the boot-suite fixtures are the authoritative coverage.

- [ ] **Step 4: Byte-identical builds (determinism)**

Run: `target/twk build boot/main.tw -o /tmp/p6s2a.wasm && target/twk build boot/main.tw -o /tmp/p6s2b.wasm && cmp /tmp/p6s2a.wasm /tmp/p6s2b.wasm && echo IDENTICAL`
Expected: `IDENTICAL`.

---

## Deferred (tracked for follow-on stages)

| Item | Home |
|---|---|
| Collection-consume on a param (`set_at` → `paths{[]}`; needs `cow_base_arg` lookup) | Stage 2b (next increment) |
| Transitive param-derived mutation bases (provenance) | Stage 2b / fold into Stage 3 |
| `ConsumedPaths` (6th `ForwardState` field) + `consume_dead` + acceptance #5/#7/#8 | Stage 4 (call-site decision) — the actual consumer |
| Owned-entry re-analysis that *validates* these candidate paths | Stage 3 |

## Self-Review

- **Spec coverage:** the record-update case of acceptance #1 (`add_type` → `paths{[],[.f0]}`) is implemented and tested; the model matches `summary-specialization.md`'s worked-summaries table for `add_type`. Collection-consume (#2/`set_at`) and the decision-dependent criteria (#5/#7/#8) are explicitly deferred with a home.
- **Type consistency:** `collect_mut_paths`/`mut_get`/`build_in_place_paths` and the broadened `reconcile_role(esc, cap, has_mut, flows)` are used consistently; `ParamPath`/`ParamSummary`/`ParamRole` match Part-1 + Stage-1 (`ParamPath` from `variant_id`). The single `reconcile_role` call site (finalization) is updated in the same task.
- **No placeholders:** every step shows exact code or an exact command + expected output. All ANF/CFG anchors (`ARecordUpdate` shape, `param_index_of`, `atom_local_id`, `insert_sorted`) were verified against the current branch.
- **Invariant preserved:** `in_place_paths ≠ [] ⇒ base_role == Consumed && flows_to_return` still holds (paths only built for `Consumed`, which requires `flows`); `Published ⇒ []` unchanged. Move-consumed params keep their `[[]]` shell path (no regression to Part-1 behavior).
- **Re-baseline watch:** Step 8 explicitly checks for and handles any Part-1 role/render test whose param now correctly classifies `Consumed`.
