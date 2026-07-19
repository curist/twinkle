# Phase 6 Part 2, Stage 4a — Whole-Return Move Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a summarized call's return **aliases exactly one parameter** (`ret = MayAliasParams(k)`) and that argument is proven **`Unique` + last-use** at the call, **move** it (the result takes the argument's unique region; the argument is consumed) instead of publishing it — closing the whole-value recovery that Stages 2b/3 deferred, and improving generic precision for fresh-unique arguments.

**Architecture:** `transfer_summarized_call` currently publishes every `MayAliasParams(k)` argument unconditionally, forcing the result `Shared`. Stage 4a gates that branch on the existing `arg_unique[k]` fact (`own_is_unique(pre_own) and is_last_use`): when a single aliased argument is provably unique-and-dead, apply the existing `consume_base` move (result `Unique`, argument `valid=false`) with the argument's provenance — the destructive-update condition. This is a **caller-side transfer refinement**: no new return representation, no variant selection, no field seeding. It composes with Stage 3's owned-entry seeding to finally classify the `add_type`-caller `Consumed`. Analysis-only: `twk ir --census` stays **0 in-place**.

**Tech Stack:** Twinkle (`.tw`), boot compiler only. Tests via the boot suite. Build/verify with `make boot-test`.

---

## Where Stage 4a sits (the Stage 4 split)

Stage 4 as designed (D4/D6/D7 + the whole-return-move obligation) bundles several coupled subsystems. This plan is the **first, independent slice**:

- **Stage 4a (this plan):** whole-return **move** — the `MayAliasParams(k)` → move-when-`arg_unique` transfer refinement. Self-contained; needs nothing from the rest.
- **Stage 4b (later):** field-granular owned-entry seeding — extend `summarize_variant` to seed `field_own` `Unique` for `[.f]` reqs (D6 mixed ownership: unique `.types`, shared `.values`).
- **Stage 4c (later):** the per-call-site **VariantId decision records** + `consume_dead` for mixed/partial keys + `ConsumedPaths` + `SpecializationFacts` (D4/D6/Blocker-3/5). This is the codegen-facing *decision*; the analysis *recoveries* it depends on land in 4a/4b.
- **Stage 5:** SCC variant fixpoint (D12). **Stage 6:** rendering.

**Correction to the Stage 3 deferral table.** It said Stage 4 "must add a whole-return owned-handoff *representation*." For the **direct** caller (a function recovering its own threaded param, or a fresh-unique local), the caller-side `arg_unique` move (4a) suffices — no summary-encoded representation is needed. A representation is only needed for **multi-level transitive** propagation (a function that recovers a param via the move and then *returns it* so its own caller can recover), which is a later refinement, not Stage 4a.

---

## Why this design (context)

Stages 2b and 3 established the gap: a helper that returns its whole parameter classifies `ret = MayAliasParams(k)`, and the caller publishes that argument unconditionally (`ownership.tw:1160`), *regardless* of whether the argument is owned. So:

- **`add_type` caller under owned entry (Stage 3):** `summarize_variant(g, {(0,[])})` seeds `env` `Unique`, but the `add_type(env)` call still publishes `env` (unconditional `MayAliasParams`), so `g`'s owned variant stays `Published` — the recovery Stage 3 could **not** reach.
- **`build_env` Case B (generic, fresh local):** `env := Env.{…}; env = add_type(env, …)` — `env` is fresh `Unique` + last-use at the call, yet it is published, so `env` goes `Shared` even though it is provably owned.

The fix is exactly the move the compiler already performs for COW builtins. `consume_base` (`ownership.tw:880`):

```tw
fn consume_base(st: ForwardState, result: Int, base: Atom, last: Vector<Int>) ForwardState {
  case atom_local_id(base) {
    .Some(bid) => {
      base_own := fact_of(st.own, base)
      case base_own {
        .Unique => if is_last_use(last, bid) {
          st = .set_own_st(result, .Unique)
          st.set_valid(bid, false)          // <- argument consumed
        } else { st.set_own_st(result, .Unknown) },
        _ => st.set_own_st(result, .Unknown),
      }
    },
    .None => st.set_own_st(result, .Unknown),
  }
}
```

Applying it in the `MayAliasParams` branch, gated on `arg_unique[k]`, makes a proven-unique whole-return a move. **Soundness (destructive-update theorem):** `arg_unique[k] = own_is_unique(pre_own, k) and is_last_use(last, k)` — the pre-update logical version of the region has no observable continuation (last-use), so handing it to the result and consuming the argument is sound. A non-unique or non-last-use argument (any `MayAliasParams` param argument in the generic pass) keeps the current publish, so the classification only sharpens where uniqueness is proven.

### What changes, and what doesn't

- **Owned variant of the `add_type` caller** (`summarize_variant(g, {(0,[])})`): `env` is seeded `Unique` (Stage 3) → `arg_unique` at the call → **move** → `env` consumed and not published → `esc=Borrowed`, `cap=Consumed`, and (with the Stage 2b dirty candidate surviving) classifies **`Consumed`** with field paths. **This closes the Stage 2b/3 deferral.**
- **Generic caller with a fresh-unique local** (`build_env` shape): the local **moves** (result `Unique`, local consumed) instead of publishing — a generic precision improvement.
- **Generic caller passing a param** (the Stage 2b guard's `g`): the param enters `Unknown` in the generic pass → `arg_unique` false → **still publishes** → `g`'s generic summary stays `Published`. **The Stage 2b guard stays green** (4a does not touch the generic param case).

### Determinism / monotonicity

The move only sharpens a result from `Shared` to `Unique` and a param role from `Published` toward `Consumed`/`Borrowed` — **downward** in the role lattice (`Published → Consumed → Borrowed`), the direction the descending generic fixpoint already moves, so `same_summary` still converges. Codegen is unaffected (census 0), so `boot.wasm` stays byte-identical and self-host converges; only the **summary** (`--cfg`) output sharpens.

### Scope

**In scope (4a):** the single-aliased-param whole-return move gated on `arg_unique`, in `transfer_summarized_call`'s `MayAliasParams` branch.

**Explicitly out of scope:** multi-param `MayAliasParams` moves (2+ aliased params → keep publishing); field-granular seeding (Stage 4b); VariantId call-site decision records + `consume_dead` for mixed/partial keys + `ConsumedPaths` (Stage 4c); summary-encoded transitive move representation (later); SCC variant fixpoint (Stage 5); rendering (Stage 6).

### File structure

- **Modify `boot/compiler/ownership.tw`:** refine the `MayAliasParams` branch of `transfer_summarized_call` (single-param `arg_unique` → `consume_base` move; else the existing publish).
- **Test `boot/tests/suites/cfg_return_paths_suite.tw`:** a fresh-unique local moved through a whole-value (`MayAliasParams`) helper.
- **Test `boot/tests/suites/cfg_summary_suite.tw`:** the `add_type`-caller **owned** variant now classifies `Consumed` (alongside the untouched Stage 2b generic guard).

Verified anchors (current branch): `transfer_summarized_call` (`ownership.tw:1122`), its pre-call snapshot `pre_own`/`pre_prov`/`arg_unique` (`:1133`–`:1140`), the `MayAliasParams` publish branch (`:1156`–`:1166`), `consume_base` (`:880`, inherent-callable), `prov_of` used with `pre_prov`, `set_prov_st`; return-paths suite helpers `b_reg`/`sem`/`fdef`/`module_of`/`dict_new_call`/`analyzed_caller`/`caller_own`/`own_unique`/`own_shared`; summary suite helpers `compute_of`/`summ_of`/`p_role`/`role_tag`/`fdef`/`cfg`/`variant_id` present.

---

## Task 1: The whole-return move

**Files:**
- Modify: `boot/compiler/ownership.tw` (`MayAliasParams` branch)
- Test: `boot/tests/suites/cfg_return_paths_suite.tw`, `boot/tests/suites/cfg_summary_suite.tw`

- [ ] **Step 1: Write the failing fresh-local move test**

In `cfg_return_paths_suite.tw`, add (next to the other `case_w_*` tests):

```tw
    .test(
      "phase6 stage4a: fresh-unique local moves through a whole-value (MayAliasParams) helper",
      fn() {
        b := b_reg()
        // h(x) = x  -- returns its whole param, so ret = MayAliasParams([0]).
        idhelper := fdef(1, "h", 1, .Atom(.ALocal(lid(0))))
        // caller() { ctx := Dict.new(); out := h(ctx); out }
        caller_body: AnfExpr = .Let(
          lid(0),
          dict_new_call(b),
          .Let(
            lid(1),
            .ACall(.AGlobalFunc(FuncId.{ id: 1 }), [.ALocal(lid(0))]),
            .Atom(.ALocal(lid(1))),
          ),
        )
        funcs := [idhelper, fdef(2, "caller", 0, caller_body)]
        f := analyzed_caller(funcs, "caller")
        // ctx (lid0) is fresh Unique + last-use at the call, so the whole-return moves:
        // out (lid1) is Unique, not Shared (which is what the unconditional publish gave).
        try assert.equal(caller_own(f, 1), own_unique())
        .Ok({})
      },
    )
```

- [ ] **Step 2: Write the failing owned-variant `add_type`-caller test**

In `cfg_summary_suite.tw`, add immediately after the existing
`"phase6 stage2b: param-threaded caller stays Published in the generic pass …"` guard
(so the generic/owned contrast is co-located):

```tw
    .test(
      "phase6 stage4a: add_type-caller owned variant recovers Consumed (whole-return move)",
      fn() {
        // fn h(env) { env.f0 = 0 ; env2 }  -- h: p0 Consumed {[],[.f0]}, ret alias(p0).
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
        b := b_reg()
        v := cfg.build_view(module_of([fdef(1, "h", 1, h_body), fdef(2, "g", 1, g_body)]), b)
        t := summary.compute(v, b, sem())
        // Generic g still publishes its param (Unknown at entry) -- the Stage 2b guard.
        gen := summ_of(t, 2)
        try assert.equal(p_role(gen, 0), role_tag(.Published))
        // Owned variant: seed env Unique -> each h(env) whole-return MOVES env (arg_unique)
        // instead of publishing -> env recovered + consumed, and the Stage 2b dirty
        // candidate survives -> Consumed with field paths.
        g_cfg := case cfg.function_named(v, "g") {
          .Some(fn_) => fn_,
          .None => error("no g"),
        }
        key: variant_id.UniqueKey = [variant_id.UniqueReq.{ param: 0, path: variant_id.shell() }]
        owned := ownership.summarize_variant(g_cfg, key, t, b, sem(), Dict.new())
        try assert.equal(p_role(owned, 0), role_tag(.Consumed))
        try assert.is_true(!owned.params[0].in_place_paths.is_empty()) // carries field paths
        .Ok({})
      },
    )
```

- [ ] **Step 3: Run to verify both fail**

Run: `set -o pipefail; make boot-test 2>&1 | grep -aE 'Ran [0-9]+ tests|FAIL|stage4a' | tail -6`
Expected: FAIL on both `stage4a` tests — the fresh local is `Shared` (published) and the owned
`g` is `Published` (whole-return publishes even when seeded), because the move is not yet
implemented.

- [ ] **Step 4: Implement the whole-return move**

In `ownership.tw`, replace the `MayAliasParams` branch of `transfer_summarized_call`
(`:1156`–`:1166`):

```tw
    .MayAliasParams(idxs) => {
      origins: Vector<Int> = []
      for k in idxs {
        if k < args.len() {
          st = .publish_atom(args[k])
          origins = union_sorted(origins, prov_of(st.prov, args[k]))
        }
      }
      st = .set_own_st(result, .Shared)
      st = .set_prov_st(result, origins)
    },
```

with:

```tw
    .MayAliasParams(idxs) => {
      // Whole-return move (Stage 4a): if the return aliases EXACTLY ONE param and that
      // argument is proven Unique + last-use here (arg_unique -- the destructive-update
      // condition), MOVE it: the result takes the argument's unique region and the
      // argument is consumed (consume_base), instead of publishing. Multi-param aliasing
      // or a non-unique/non-last-use argument keeps the conservative publish, so a param
      // argument in the generic pass (Unknown) still publishes.
      if idxs.len() == 1 and idxs[0] < args.len() and arg_unique[idxs[0]] {
        k := idxs[0]
        st = st.consume_base(result, args[k], last)          // result Unique, args[k] valid=false
        st = st.set_prov_st(result, prov_of(pre_prov, args[k]))
      } else {
        origins: Vector<Int> = []
        for k in idxs {
          if k < args.len() {
            st = .publish_atom(args[k])
            origins = union_sorted(origins, prov_of(st.prov, args[k]))
          }
        }
        st = .set_own_st(result, .Shared)
        st = .set_prov_st(result, origins)
      }
    },
```

- [ ] **Step 5: Run the two new tests**

Run: `set -o pipefail; make boot-test 2>&1 | grep -aE 'Ran [0-9]+ tests|FAIL' | tail -4`
Expected: the two `stage4a` tests pass. If other suites now FAIL, they are almost certainly
pre-existing summary/ownership tests that asserted the **old** publish behavior for a
fresh-unique argument through a whole-value helper — those are **precision improvements**;
go to Task 2 Step 1 to re-baseline them deliberately (do not revert the move).

- [ ] **Step 6: Format, lint**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw    # expect only the pre-existing findings; fix any NEW finding
```

---

## Task 2: Re-baseline + verification

**Files:** possibly pre-existing test suites (re-baselining only); otherwise verification.

- [ ] **Step 1: Identify and re-baseline any tests that asserted the old publish behavior**

Run the full suite and inspect every failure that is **not** a new `stage4a` test:

Run: `set -o pipefail; make boot-test 2>&1 | grep -aE 'FAIL|^\s*x ' | tail -40`
For each failure, confirm it is a fresh-unique argument now **moving** (result `Unique`/param
recovered) where it previously published (`Shared`/`Published`) — i.e. a **sharper, sound**
result. Update the expectation and add a one-line comment noting Stage 4a changed it. If any
failure is *not* explainable as this precision gain, **stop** — the move may be firing where
`arg_unique` should be false (investigate the gate), or a genuinely-shared arg is being moved
(unsound). Do not blanket-update.

- [ ] **Step 2: Census unchanged (analysis-only invariant)**

Run: `set -o pipefail; target/twk ir boot/main.tw --census 2>&1 | awk 'NR>1 && NF>=3 {s+=$NF} END{print "total in_place:", s+0}'`
Expected: `total in_place: 0`.

- [ ] **Step 3: Full boot suite + self-host fixed point (unmasked)**

Run: `make boot-test`
Expected (read the tail): `Ran N tests: N passed`. Then, run alone (no parallel heavy `twk`):

Run: `set -o pipefail; touch boot/compiler/ownership.tw && make stage2 2>&1 | tail -4`
Expected: `Fixed point reached: stage3 == stage4`. (Summaries sharpen but codegen is unchanged
— census 0 — so the self-host payload still converges. If it diverges, the transfer became
non-monotone; investigate.)

- [ ] **Step 4: Byte-identical builds (determinism)**

Run: `set -o pipefail; target/twk build boot/main.tw -o /tmp/p6s4a_x.wasm && target/twk build boot/main.tw -o /tmp/p6s4a_y.wasm && cmp /tmp/p6s4a_x.wasm /tmp/p6s4a_y.wasm && echo IDENTICAL`
Expected: `IDENTICAL`.

- [ ] **Step 5: Generic precision may increase (expected, not a regression)**

Run: `set -o pipefail; target/twk ir boot/main.tw --cfg 2>&1 | grep -c 'Consumed paths{\[\],\[\.f'`
Expected: **greater than or equal to** the pre-4a count (62) — a fresh-unique argument moved
through a whole-value helper can now let a caller's param recover. Unlike earlier stages this
is **not** required to be unchanged; 4a is a deliberate precision change. Byte-identical builds
(Step 4) remain the codegen-no-op guarantee.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "phase6 stage4a: whole-return move for a proven-unique MayAliasParams arg

When a summarized call's return aliases exactly one param and that arg is Unique +
last-use at the call (arg_unique), move it (result Unique, arg consumed via
consume_base) instead of publishing. Closes the whole-value recovery Stages 2b/3
deferred: the add_type-caller owned variant (env seeded Unique) now moves env at
each call and classifies Consumed with field paths, while the generic pass (param
Unknown) still publishes -- the Stage 2b guard stays green. Also recovers a fresh-
unique local threaded through a whole-value helper. No new return representation,
no variant selection (Stage 4c). Analysis only: census stays 0, self-host holds."
```

---

## Deferred (tracked)

| Item | Home |
|---|---|
| Multi-param `MayAliasParams` moves (2+ aliased params) | later refinement (keep publishing; rare) |
| Field-granular owned-entry seeding (`field_own` Unique for `[.f]`, D6 mixed ownership) | **Stage 4b** |
| Per-call-site VariantId decision records + `consume_dead` (mixed/partial keys) + `ConsumedPaths` + `SpecializationFacts` | **Stage 4c** |
| Summary-encoded transitive whole-return move representation (multi-level recovery chains) | later refinement |
| Demand-driven variant memo + SCC variant fixpoint (D12) | **Stage 5** |
| Rendering the specialized summary + per-site variant decision | **Stage 6** |

## Self-Review

- **Spec coverage:** closes the whole-value recovery obligation the Stage 3 deferral named — via the caller-side move, which I verified needs no new ret representation for the direct case (correcting the Stage 3 deferral wording). Pinned by two acceptances: the fresh-unique-local move (generic precision) and the `add_type`-caller owned-variant `Consumed` (the Stage 2b/3 deferral).
- **Reuses the existing move primitive (no new mechanism):** the move is `consume_base` (`ownership.tw:880`), the same one COW builtins use — result `Unique`, argument `valid=false`. No parallel proof path.
- **Sound gate:** `arg_unique[k] = own_is_unique(pre_own) and is_last_use` is the destructive-update condition; a non-unique/non-last-use argument (every generic param arg) keeps the publish, so the Stage 2b generic guard stays green and no shared argument is ever moved.
- **Honest scope:** multi-param moves, field seeding (4b), the VariantId call-site decision (4c), and the transitive representation are deferred with reasons; 4a is only the single-param caller-side move.
- **Determinism/termination:** the move sharpens results downward in the role lattice (`Published→Consumed→Borrowed`), the direction the generic fixpoint already descends, so `same_summary` converges; codegen is untouched (census 0) so builds stay byte-identical and self-host holds.
- **Re-baselining is deliberate, not blanket:** Task 2 Step 1 requires each non-`stage4a` failure to be explained as a sound precision gain (fresh-unique arg moving) before updating it, and stops on anything else.
- **Type consistency:** `consume_base(st, result, base, last)`, `arg_unique[k]`, `prov_of(pre_prov, args[k])`, `set_prov_st` are all in scope in `transfer_summarized_call`; the single-param guard (`idxs.len() == 1`) keeps the multi-param path on the existing publish.
- **Verification not masked:** every `make boot-test`/`make stage2` check uses `set -o pipefail` or is run unpiped; `twk lint` is run after edits.
