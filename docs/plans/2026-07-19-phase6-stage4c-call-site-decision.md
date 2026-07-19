# Phase 6 Part 2, Stage 4c — Call-Site Variant Selection (decision core) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A pure function `select_variant(func_id, callee_summary, arg_unique)` that computes the **VariantId a call site selects** — the D4 shell-level key selection: each callee parameter the summary says it *consumes* (non-empty `in_place_paths`) whose argument is proven `Unique + last-use` contributes its paths to the owned `UniqueKey`; an empty key means the generic variant. This is the analysis core of the per-call-site specialization decision, unit-testable in isolation from the recording/memo machinery.

**Architecture:** The caller-side ownership *recovery* is already complete (Stages 2/3/4a: a fresh-unique or seeded-unique argument moves through a consuming callee). Stage 4c produces the **decision record** that codegen will route on — *which* clone (owned key vs generic) each call site is eligible for. This first slice lands the **decision logic** as a pure function over `(callee Summary, per-argument uniqueness)` → `VariantId`, reusing Stage 1's `variant_id` (`UniqueReq`/`UniqueKey`/`VariantId`/`canonicalize_variant`). It records nothing yet and is not wired into the pass — that (the `SpecializationFacts` table + `ConsumedPaths`) is deferred because it couples with Stage 5's variant memo/fixpoint. Analysis-only: `twk ir --census` stays **0 in-place**.

**Tech Stack:** Twinkle (`.tw`), boot compiler only. Tests via the boot suite. Build/verify with `make boot-test`.

---

## Why this scope (the Stage 4c split)

The full Stage 4c (design D4/D6/Blocker-3/5) is: form the candidate key from pre-call per-path facts, reduce it by the `consume_dead` fixed point, **select** the `VariantId`, **record** a `CallDecision` per site, and **invalidate** the consumed paths (`ConsumedPaths`) so a later disjoint-sibling read stays legal. Two spikes during Stage 4b planning established the boundaries that make a first slice clean:

- **The decision is codegen-routing, not further analysis.** `add_type`'s owned variant summary is *identical* to its generic summary (both `p0=Consumed paths{[],[.types]} ret=alias(0)`); the only difference is which clone codegen emits (in-place vs persistent). So the analysis payoff of 4c is the **decision record itself**, and the decision *logic* can be computed and tested as a pure function without changing any summary.
- **`consume_dead` for the whole-value/shell case is exactly `arg_unique`.** `arg_unique[k] = own_is_unique(pre_own, k) and is_last_use(last, k)` is the destructive-update condition — unique + no continuation — which is precisely `consume_dead` at the shell. The richer `consume_dead` read-rules (a disjoint `.values` read after a `.types`-only consume) are the **field-granular / `ConsumedPaths`** part, deferred.
- **The recording pass couples with Stage 5.** Emitting a `CallDecision` per site, and *demanding* the owned variant's summary when one is selected, is the variant-demand → memo → (for recursive callees) SCC-fixpoint loop — that is Stage 5's machinery. Recording is deferred to sit with it.

So this slice is the **pure decision function** the recording pass (later 4c) and Stage 5 will call. It is analogous to how Stage 2a landed `collect_field_reqs` (a pure analysis function) before it was wired.

### What "decide" means here (Cases B vs C)

For a callee like `add_type` (`p0=Consumed paths{[],[.types]}`):

- **Case B** (`build_env`: a fresh-unique `env` threaded, consume-dead at the call) → `arg_unique[0] = true` → `select_variant` returns `add_type[unique:0,.types]` (key `{(0,[]),(0,[.types])}`).
- **Case C** (`branch_env`: `env` aliased/read later) → `arg_unique[0] = false` → `select_variant` returns the **generic** variant (empty key).
- **Transport / read-only callee** (`p0=Borrowed`, empty `in_place_paths`) → even a unique argument yields the **generic** variant: there is nothing to specialize (a `Published`/`Borrowed` param never enters a key, D5).

### Scope

**In scope (this slice):** the pure `select_variant(func_id, s, arg_unique) VariantId` — shell-level key selection from a callee summary + per-argument uniqueness, canonicalized/downward-closed via Stage 1. Empty key ⇒ generic.

**Explicitly out of scope:** the per-call-site recording pass + `SpecializationFacts` table (couples with **Stage 5**); `ConsumedPaths` + the `consume_dead` read-rules for the mixed-ownership disjoint-sibling read (later 4c, needs field facts); field-granular partial keys where only *some* fields of the argument are unique (needs the field-ownership facts that Stage 4b found are **not** summary-observable — likely a codegen-track concern); the variant cap (D7); the variant memo + SCC recursion (**Stage 5**); rendering the decision in `twk ir --cfg` (**Stage 6**).

### File structure

- **Modify `boot/compiler/ownership.tw`:** add `pub fn select_variant(func_id, s, arg_unique) vid.VariantId` near the summary helpers.
- **Test `boot/tests/suites/cfg_summary_suite.tw`:** unit-test `select_variant` with hand-built `Summary` values (Cases B/C, transport-generic, multi-param).

Verified anchors (current branch): `Summary`/`ParamSummary` (`ownership.tw:45`–`51`, `in_place_paths: vid.PathSet`), `vid` alias (`ownership.tw:14`), `variant_id.VariantId`/`UniqueReq`/`UniqueKey` (`variant_id.tw:104`–`108`), `variant_id.canonicalize_variant` (does `downward_close` + canonical sort/dedup, `variant_id.tw:186`), `variant_id.field(f)`/`shell()` constructors, `PathSet.paths`/`is_empty`; suite has `role_tag`/`variant_id` imported and constructs `ownership.Summary`/`ParamSummary` in existing tests.

---

## Task 1: The `select_variant` decision function

**Files:**
- Modify: `boot/compiler/ownership.tw` (add `select_variant`)
- Test: `boot/tests/suites/cfg_summary_suite.tw` (unit tests)

- [ ] **Step 1: Write the failing unit tests**

In `cfg_summary_suite.tw`, add a small helper next to the other summary helpers:

```tw
// A one-param callee summary with the given role + in_place_paths, for select_variant tests.
fn one_param_summary(role: ownership.ParamRole, ipp: variant_id.PathSet) ownership.Summary {
  ownership.Summary.{
    params: [ownership.ParamSummary.{ base_role: role, in_place_paths: ipp, flows_to_return: true }],
    ret: .MayAliasParams([0]),
    ret_paths: [],
  }
}

// The field ids of a VariantId's [.f] reqs on param k, sorted (shell reqs excluded).
fn variant_fields(v: variant_id.VariantId, k: Int) Vector<Int> {
  out: Vector<Int> = []
  for r in v.unique {
    if r.param == k and r.path.segs.len() == 1 {
      out = .append(r.path.segs[0])
    }
  }
  out
}
```

Then the tests:

```tw
    .test(
      "phase6 stage4c: a consumed param with a unique arg selects the owned key (Case B)",
      fn() {
        // add_type-shape callee: p0 Consumed paths{[],[.f0]}.
        ipp := variant_id.PathSet.{ paths: [variant_id.shell(), variant_id.field(0)] }
        s := one_param_summary(.Consumed, ipp)
        v := ownership.select_variant(7, s, [true]) // arg0 Unique + last-use
        try assert.equal(v.func, 7)
        // owned key: downward-closed {(0,[]),(0,[.f0])} -> shell + one field req.
        try assert.equal(v.unique.len(), 2)
        try assert.is_true(same_ints(variant_fields(v, 0), [0]))
        .Ok({})
      },
    )
    .test(
      "phase6 stage4c: a consumed param with a NON-unique arg selects generic (Case C)",
      fn() {
        ipp := variant_id.PathSet.{ paths: [variant_id.shell(), variant_id.field(0)] }
        s := one_param_summary(.Consumed, ipp)
        v := ownership.select_variant(7, s, [false]) // arg0 not unique
        try assert.equal(v.unique.len(), 0) // empty key => generic
        .Ok({})
      },
    )
    .test(
      "phase6 stage4c: a transport/read-only param never keys (Borrowed/empty -> generic)",
      fn() {
        s := one_param_summary(.Borrowed, variant_id.empty_set())
        v := ownership.select_variant(7, s, [true]) // unique arg, but nothing to consume
        try assert.equal(v.unique.len(), 0)
        .Ok({})
      },
    )
    .test(
      "phase6 stage4c: only the consuming+unique param contributes to the key",
      fn() {
        // p0 Consumed {[],[.f0]} + unique; p1 Borrowed + unique -> key from p0 only.
        ipp0 := variant_id.PathSet.{ paths: [variant_id.shell(), variant_id.field(0)] }
        s := ownership.Summary.{
          params: [
            ownership.ParamSummary.{ base_role: .Consumed, in_place_paths: ipp0, flows_to_return: true },
            ownership.ParamSummary.{ base_role: .Borrowed, in_place_paths: variant_id.empty_set(), flows_to_return: false },
          ],
          ret: .MayAliasParams([0]),
          ret_paths: [],
        }
        v := ownership.select_variant(7, s, [true, true])
        try assert.equal(v.unique.len(), 2) // (0,[]) + (0,[.f0]); p1 contributes nothing
        try assert.is_true(same_ints(variant_fields(v, 1), [])) // no reqs on p1
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify failure (function undefined)**

Run: `set -o pipefail; target/twk build boot/tests/main.tw -o /tmp/s4c_t.wasm 2>&1 | tail -6`
Expected: an unresolved-name / arity error for `ownership.select_variant`.

- [ ] **Step 3: Implement `select_variant`**

In `ownership.tw`, add near the other summary helpers (e.g. just below `summarize_variant`):

```tw
// Call-site variant selection (Stage 4c, D4 shell-level): the VariantId a caller
// selects for callee `func_id` given the callee's summary and, per argument
// position, whether that argument is proven Unique + last-use at the call
// (arg_unique -- the whole-value consume_dead condition). Each parameter the callee
// CONSUMES (non-empty in_place_paths; a Borrowed/Published param never has any) whose
// argument is arg_unique contributes its in_place_paths to the owned key. The result
// is canonicalized + downward-closed (Stage 1); an empty key means the generic
// variant. This is a pure decision -- it records nothing and demands no summary
// (the recording pass + ConsumedPaths + variant memo are later 4c / Stage 5).
pub fn select_variant(func_id: Int, s: Summary, arg_unique: Vector<Bool>) vid.VariantId {
  reqs: vid.UniqueKey = []
  for ps, i in s.params {
    if i < arg_unique.len() and arg_unique[i] and !ps.in_place_paths.is_empty() {
      for p in ps.in_place_paths.paths {
        reqs = .append(vid.UniqueReq.{ param: i, path: p })
      }
    }
  }
  vid.canonicalize_variant(vid.VariantId.{ func: func_id, unique: reqs })
}
```

- [ ] **Step 4: Run the unit tests to verify they pass**

Run (pipefail-safe): `set -o pipefail; make boot-test 2>&1 | grep -aE 'Ran [0-9]+ tests|FAIL' | tail -3`
Expected: `Ran N tests: N passed` (Case B → key len 2 with field `[0]`; Case C → empty; transport → empty; multi-param → key from p0 only).

- [ ] **Step 5: Add a determinism test (canonical key is order-independent)**

```tw
    .test(
      "phase6 stage4c: the selected key is canonical (order-independent, deduped)",
      fn() {
        // Two fields in either PathSet order canonicalize to the same VariantId string.
        a := variant_id.PathSet.{ paths: [variant_id.shell(), variant_id.field(0), variant_id.field(3)] }
        sa := one_param_summary(.Consumed, a)
        va := ownership.select_variant(9, sa, [true])
        vb := ownership.select_variant(9, sa, [true])
        try assert.equal(
          variant_id.variant_canonical_string(va),
          variant_id.variant_canonical_string(vb),
        )
        try assert.equal(va.unique.len(), 3) // shell + two fields, deduped/sorted
        .Ok({})
      },
    )
```

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw    # expect only the pre-existing findings; fix any NEW finding
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "phase6 stage4c: call-site variant selection (decision core)

Pure select_variant(func_id, callee_summary, arg_unique): each parameter the
callee consumes (non-empty in_place_paths) whose argument is proven Unique +
last-use contributes its paths to the owned UniqueKey; canonicalized +
downward-closed via variant_id (Stage 1); empty key => generic. Cases B/C
covered. Not yet wired into a pass -- the SpecializationFacts recording table +
ConsumedPaths couple with Stage 5's variant memo, and field-granular partial keys
are deferred. Analysis only: census stays 0."
```

---

## Task 2: Verification

**Files:** none (verification only). All commands use `set -o pipefail` so a `make` failure is not masked by a trailing `grep`/`tail`.

- [ ] **Step 1: Census unchanged (analysis-only invariant)**

Run: `set -o pipefail; target/twk ir boot/main.tw --census 2>&1 | awk 'NR>1 && NF>=3 {s+=$NF} END{print "total in_place:", s+0}'`
Expected: `total in_place: 0`.

- [ ] **Step 2: Generic output unchanged (`select_variant` is not called by `compute`)**

Run: `set -o pipefail; target/twk ir boot/main.tw --cfg 2>&1 | grep -c 'Consumed paths{\[\],\[\.f'`
Expected: the same count as before (62). `select_variant` is exercised only by the unit
tests; `compute`/`summarize_function` do not call it, so no generic summary changes.

- [ ] **Step 3: Full boot suite + self-host fixed point (unmasked)**

Run: `make boot-test`
Expected (read the tail): `Ran N tests: N passed`. Then, run alone (no parallel heavy `twk`):

Run: `set -o pipefail; touch boot/compiler/ownership.tw && make stage2 2>&1 | tail -4`
Expected: `Fixed point reached: stage3 == stage4`.

- [ ] **Step 4: Byte-identical builds (determinism)**

Run: `set -o pipefail; target/twk build boot/main.tw -o /tmp/p6s4c_x.wasm && target/twk build boot/main.tw -o /tmp/p6s4c_y.wasm && cmp /tmp/p6s4c_x.wasm /tmp/p6s4c_y.wasm && echo IDENTICAL`
Expected: `IDENTICAL`.

- [ ] **Step 5: Lint clean**

Run: `target/twk lint boot/main.tw`
Expected: no new findings beyond the pre-existing ones.

---

## Deferred (tracked)

| Item | Home |
|---|---|
| Per-call-site recording pass + `SpecializationFacts` (variants + `CallDecision` table), keyed by `site_key` (Stage 1) | later 4c, with **Stage 5** (variant demand → memo) |
| `ConsumedPaths` + the `consume_dead` read-rules (disjoint-sibling read after a partial consume, D6) | later 4c |
| Field-granular partial keys (only some argument fields unique) — needs field-ownership facts that are **not** summary-observable (Stage 4b finding) | codegen track / later |
| Variant cap (D7) | later 4c |
| Variant memo + SCC recursion (D12) | **Stage 5** |
| Rendering the per-site decision in `twk ir --cfg` | **Stage 6** |

## Self-Review

- **Spec coverage:** implements D4's shell-level key selection — the core of the Stage 4c call-site decision — as a pure function, pinned by Cases B (owned key) and C (generic), plus the transport/read-only → generic (D5) and determinism guards. The recording/`ConsumedPaths`/memo/rendering are deferred with reasons (they couple with Stage 5 / need field facts).
- **Grounded by the spikes:** the two Stage-4b spikes established *why* this is the right slice — the decision is codegen-routing (owned and generic summaries are identical), and `consume_dead` at the shell is exactly `arg_unique`. So `select_variant` takes `arg_unique` as input rather than re-deriving anything.
- **Reuses Stage 1, no new identity code:** the key is built from `in_place_paths` and canonicalized via `variant_id.canonicalize_variant` (downward-close + sort + dedup), so determinism and the shell-closure invariant come for free.
- **Sound by construction:** a param only enters the key when it is *consumed* (non-empty `in_place_paths`, which a `Borrowed`/`Published` param never has — D5) **and** its argument is `arg_unique`; there is no path by which a shared or read-only argument produces an owned key. The function decides nothing about *emission* (census stays 0).
- **No generic perturbation:** `select_variant` has no caller in `compute`, so generic summaries, the `--cfg` count, byte-identical builds, and the self-host fixed point are all unchanged (a true no-op on compilation, exercised only by unit tests).
- **Type consistency:** `select_variant(func_id: Int, s: Summary, arg_unique: Vector<Bool>) vid.VariantId`; `ps.in_place_paths` is a `PathSet` (`.paths` iterated, `.is_empty()` checked); `vid.UniqueReq{param, path}` and `vid.canonicalize_variant` match Stage 1. The `variant_fields`/`one_param_summary` test helpers use the same shapes.
- **Verification not masked:** every `make boot-test`/`make stage2` check uses `set -o pipefail` or is run unpiped; `twk lint` is run after edits.
