# Phase 6 Part 2, Stage 3 — Owned-Entry Re-Analysis (`summarize_variant`) Implementation Plan

> **Status: archived.** Phase 6 analysis work landed; remaining variant generation, routing, and deeper path/variant machinery are tracked by the codegen/migration tracks.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `summarize_variant(f, key)` — a re-run of the existing ownership analysis with the key's parameters seeded **`Unique`** at function entry — so a parameter threaded through a transport helper (which the generic pass publishes) is **recovered** in its owned variant: the Phase-5 return-path recovery gate fires because the argument is now proven unique. This closes the Phase-5 param-threaded deferral for the ret-path (transport-wrapper) case.

**Architecture:** The owned-entry summary is the *same* transfer re-run with keyed reference parameters entering `Unique` instead of `Unknown` (D13 — no second proof engine). Seeding the parameter `Unique` at block 0 makes `arg_unique[k]` true at any call that passes it, so the existing `OwnedFromParam(k)` recovery gate (`ownership.tw:1174`) recovers the region instead of publishing it — the parameter classifies `Borrowed`/`Consumed` (not `Published`) and its return-path/`in_place_paths` facts survive. Analysis-only: `twk ir --census` stays **0 in-place**; no variant is emitted, no call site selects one yet (that is Stage 4).

**Tech Stack:** Twinkle (`.tw`), boot compiler only. Tests via the boot suite. Build/verify with `make boot-test`.

---

## Why this design (context)

Stage 2b established that a parameter threaded through a helper that hands it back is **published** in the generic pass, so `reconcile_role` classifies it `Published` and discards the candidate `in_place_paths`. The design closes this by *ownership monomorphization* (D10/D13): re-analyze the callee with the keyed `(param, path)` slots entering `Unique`. Under that entry the parameter is provably owned at its uses, so the recovery machinery that already exists fires.

Concretely, the `case_w_param_fixture` (`cfg_return_paths_suite.tw:183`):

```tw
fn helper(x) { Wrapper.{ f0: x } }              // fresh record; ret_path .f0 = OwnedFromParam(0)
fn caller(ctx) { out := helper(ctx); out.f0 }   // extracts the wrapped param back out
```

The generic guard at `cfg_return_paths_suite.tw:592` asserts `caller`'s `ctx` is **published** (`caller_own == own_shared()`) — its comment says verbatim *"param-threaded transport is not recovered until Phase 6 parameter-ownership."* At the `helper(ctx)` call, the gate snapshots `arg_unique[0] = own_is_unique(pre_own, ctx) and is_last_use(last, ctx)` (`ownership.tw:1137`); `ctx` enters `Unknown`, so `own_is_unique` is false ⇒ the `OwnedFromParam(0)` recovery fails ⇒ publish-on-fail publishes `ctx` (`ownership.tw:1191`).

Stage 3 seeds `ctx` **`Unique`** at entry. Now `own_is_unique(ctx)` is true and `ctx` is last-use at the call, so `arg_unique[0]` is true ⇒ the recovery fires ⇒ `ctx` is **not** published. Its escape classification is `Borrowed` (not `Retained`), so `reconcile_role` no longer forces `Published`.

### Why seeding alone suffices (no transfer changes — D13)

- The recovery gate (`ownership.tw:1174`) already keys on `arg_unique[k]`. Seeding the parameter `Unique` is the *only* input it lacks. The transfer, the gate, and the finalization are unchanged.
- `collect_field_reqs` (Stage 2a/2b) is a **separate** dataflow independent of ownership, so it computes the *same* candidate `dirty` set regardless of seeding. What changes is only the finalization: `reconcile_role(esc, …)` sees `esc = Borrowed` under seeding instead of `Retained`, so a `Consumed`-flowing param's `in_place_paths` survive instead of being discarded.
- The generic pass is `summarize_variant` with an **empty** seed, so `summarize_function`'s output is byte-for-byte unchanged (verified in Task 2).

### What is NOT recovered here (honest scope boundary → Stage 4)

- **Whole-value `MayAliasParams(k)` returns (the `add_type` caller).** A helper that returns its *whole* parameter (e.g. `add_type` returning `env`) classifies `ret = MayAliasParams(k)`, and `transfer_summarized_call` publishes `args[k]` **unconditionally** for that case (`ownership.tw:1160`), *regardless* of `arg_unique`. **Seeding does not change that publish** — so `summarize_variant(add_type_caller, {(0,[])})` still classifies the caller's param `Published`. This is a **scope correction to Stage 2b**: the Stage 2b plan/memory said Stage 3 would reclassify the whole-value `add_type`-caller `Consumed`; that is wrong — Stage 3 recovers only the **ret-path transport** case (`case_w_param`, a fresh `Wrapper.{ f0: ctx }` with a `ret_path OwnedFromParam(0)`). The whole-value `add_type` case is **Stage 4a**: `MayAliasParams(k)` publishes unconditionally, so 4a makes the caller **move** it (result shell `Unique`, arg consumed via `consume_base`) when the arg is proven `Unique + last-use` (`arg_unique`) — no owned-variant *selection* and no new return representation needed for the direct case (this note originally overstated the obligation; see [2026-07-19-phase6-stage4a-whole-return-move.md](2026-07-19-phase6-stage4a-whole-return-move.md)). The Stage 2b docs are corrected as part of this plan's review.
- **Field-granular ownership** (which field *backings* are unique — D6). **Stage 3 is shell-only.** It seeds a parameter's **shell** ownership `Unique` (`own = Unique`) for each `[]` requirement and seeds **no** `field_own`. A unique record shell does **not** imply unique field backings (Phase 4: a unique shell can hold shared reference fields — shell reuse and field in-place are independent), so shell seeding is sufficient and correct only for the **shell-level / ret-path transport** recovery, which is all Stage 3 claims. Any `[.f]` requirement in the key is **ignored** for seeding (`summarize_variant` seeds only `req.path.is_shell()` reqs; a downward-closed key always carries the param's `[]` req, so its shell is still seeded). Field-granular `field_own` seeding — needed to distinguish a unique `.types` backing from a shared `.values` backing (D6 mixed ownership) — is **Stage 4**. This is not a soundness shortcut: the summary's `in_place_paths` are candidates (from the seeding-independent `collect_field_reqs`) that Stage 4/owned-entry validates; Stage 3 only proves the shell-level escape recovery.
- **Demand-driven variant memo + SCC variant fixpoint** (D12) → Stage 5. Stage 3 provides the `summarize_variant` *primitive*, called directly with an explicit key; there is no memo, no per-call-site demand, no recursion-tie-the-knot yet.
- **Rendering the variant/decision** → Stage 6.

### Scope

**In scope (Stage 3):** `summarize_variant(f, key)` = the existing analysis re-run with the key's parameters seeded shell-`Unique`; the `Borrowed`/`Consumed` (not `Published`) recovery it produces for a ret-path transport parameter. The generic `summarize_function` becomes a thin wrapper (empty seed) — provably unchanged.

**Explicitly out of scope:** whole-value `MayAliasParams` recovery (Stage 4), field-granular partial-ownership seeding (Stage 4), the variant memo/SCC fixpoint (Stage 5), call-site variant selection (Stage 4), rendering (Stage 6). All named in the deferral table.

### File structure

- **Modify `boot/compiler/ownership.tw`:** add a `unique_seed: Dict<Int, Bool>` parameter to `run_fixpoint` (local ids to seed `Unique` at block 0) + `seed_param_own` helper; add the block-0 seed application; refactor `summarize_function`'s body into a private `summarize_seeded(…, unique_seed)`; add the `pub fn summarize_function` thin wrapper (empty seed) and `pub fn summarize_variant(f, key, …)`.
- **Test `boot/tests/suites/cfg_return_paths_suite.tw`:** add `use compiler.variant_id`; an `is_published` helper; the owned-entry recovery test on `case_w_param_fixture`. The existing generic guard (`:592`) stays green (regression check).

Verified anchors (current branch): `run_fixpoint` (`ownership.tw:2592`), its block-0 prov seed `entry_prov = seed_param_prov(entry_prov, params)` (`:2642`), its two callers — `analyze_function` (`:2930`) and `summarize_function` (`:3577`); `summarize_function` is `pub`, 5-arg (`:3563`), called from `summary.tw:395`; `seed_param_prov` (`:2304`); `own_tag(.Unique) == 0` (`:23`); the recovery gate `arg_unique[k]` / publish-on-fail (`:1137`, `:1174`, `:1191`); `case_w_param_fixture` + generic guard (`cfg_return_paths_suite.tw:183`, `:592`); return-paths suite helpers `b_reg`/`sem`/`module_of`/`fdef`/`wrapper_record` present; `vid` is the ownership.tw alias for `variant_id` (`UniqueKey`/`UniqueReq`/`shell`).

---

## Task 1: Owned-entry seeding + `summarize_variant`

**Files:**
- Modify: `boot/compiler/ownership.tw` (`seed_param_own`, `run_fixpoint` seed param, `summarize_seeded`/`summarize_function`/`summarize_variant`)
- Test: `boot/tests/suites/cfg_return_paths_suite.tw` (import + helper + recovery test)

- [ ] **Step 1: Write the failing owned-entry recovery test**

In `cfg_return_paths_suite.tw`, add `use compiler.variant_id` to the imports (near the
other `use compiler.*` lines), then add this helper next to `caller_own`
(`:80`):

```tw
fn is_published(s: ownership.Summary, i: Int) Bool {
  case s.params[i].base_role {
    .Published => true,
    _ => false,
  }
}
```

Then add the test (place it right after the existing generic guard test
`"param-threaded state is NOT recovered: …"`, `:592`, so the contrast is co-located):

```tw
    .test(
      "phase6 stage3: owned-entry re-analysis recovers a param-threaded transport",
      fn() {
        // helper(x) = Wrapper.{ f0: x }  (ret_path .f0 = OwnedFromParam(0));
        // caller(ctx) = { out := helper(ctx); out.f0 }  -- extracts the wrapped param.
        funcs := case_w_param_fixture()
        b := b_reg()
        v := cfg.build_view(module_of(funcs), b)
        t := summary.compute(v, b, sem())

        // Generic caller summary: ctx (param0) is published by the failed recovery gate.
        gen := case ownership.summary_get(t, 2) {
          .Some(s) => s,
          .None => error("no caller summary"),
        }
        try assert.is_true(is_published(gen, 0))

        // Owned variant: seed ctx (param0) shell-Unique. arg_unique[0] is now true at the
        // helper call, so the OwnedFromParam(0) recovery fires -> ctx is NOT published and
        // its region flows to the return.
        caller := case cfg.function_named(v, "caller") {
          .Some(f) => f,
          .None => error("no caller cfg"),
        }
        key: variant_id.UniqueKey = [variant_id.UniqueReq.{ param: 0, path: variant_id.shell() }]
        owned := ownership.summarize_variant(caller, key, t, b, sem(), Dict.new())
        try assert.is_true(!is_published(owned, 0))          // recovered, not leaked
        try assert.is_true(owned.params[0].flows_to_return)  // ctx's region reaches the return
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify failure (function undefined)**

Run: `set -o pipefail; target/twk build boot/tests/main.tw -o /tmp/s3_t.wasm 2>&1 | tail -6`
Expected: an unresolved-name error for `ownership.summarize_variant`.

- [ ] **Step 3: Add `seed_param_own` and thread `unique_seed` through `run_fixpoint`**

In `ownership.tw`, add `seed_param_own` immediately after `seed_param_prov`
(`:2304`–`:2312`):

```tw
// Seed block-0 entry ownership for an owned-entry re-analysis (Stage 3): each param
// whose local id is marked in `unique_seed` enters Unique instead of the default
// Unknown. This is the ONLY input the return-path recovery gate lacks to fire for a
// param argument (own_is_unique). Empty seed => identity (the generic pass).
fn seed_param_own(
  entry: Dict<Int, Int>,
  unique_seed: Dict<Int, Bool>,
  params: Vector<LocalId>,
) Dict<Int, Int> {
  for p in params {
    case unique_seed.get(p.id) {
      .Some(u) => if u {
        entry[p.id] = own_tag(.Unique)
      },
      .None => {},
    }
  }
  entry
}
```

Add the `unique_seed` parameter to `run_fixpoint`. Change its header (`:2592`):

```tw
fn run_fixpoint(
  blocks: Vector<CfgBlock>,
  params: Vector<LocalId>,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
) FixResult {
```

to:

```tw
fn run_fixpoint(
  blocks: Vector<CfgBlock>,
  params: Vector<LocalId>,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
  unique_seed: Dict<Int, Bool>,
) FixResult {
```

and, at the block-0 seed site inside `run_fixpoint` (`:2641`–`:2643`), apply the own
seed right after the prov seed:

```tw
      if blk.id.id == 0 {
        entry_prov = seed_param_prov(entry_prov, params)
      }
```

becomes:

```tw
      if blk.id.id == 0 {
        entry_prov = seed_param_prov(entry_prov, params)
        entry_own = seed_param_own(entry_own, unique_seed, params)
      }
```

Update the **other** `run_fixpoint` caller — `analyze_function` (`:2930`) — to pass an
empty seed (its block-fact path is not owned-entry in Stage 3):

```tw
  fx := run_fixpoint(blocks, params, table, b, sem, no_suppress)
```

becomes:

```tw
  fx := run_fixpoint(blocks, params, table, b, sem, no_suppress, Dict.new())
```

- [ ] **Step 4: Refactor `summarize_function` into `summarize_seeded` + add `summarize_variant`**

Rename the current `pub fn summarize_function` (`:3563`) to a private
`summarize_seeded` that takes the seed, and pass the seed to its `run_fixpoint` call
(`:3577`). Change the header:

```tw
pub fn summarize_function(
  f: CfgFunction,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
) Summary {
```

to:

```tw
fn summarize_seeded(
  f: CfgFunction,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
  unique_seed: Dict<Int, Bool>,
) Summary {
```

and its `run_fixpoint` call (`:3577`):

```tw
  fx := run_fixpoint(blocks, f.params, table, b, sem, suppress)
```

to:

```tw
  fx := run_fixpoint(blocks, f.params, table, b, sem, suppress, unique_seed)
```

**Also seed the return-site replay (load-bearing).** `summarize_seeded` re-forwards each
return block to classify `ret`/`ret_paths` (and thus `flows_to_return`) with its **own**
join, independent of `run_fixpoint`. It seeds prov but not own at block 0 (`:3607`–`:3609`):

```tw
        if blk.id.id == 0 {
          entry_prov = seed_param_prov(entry_prov, f.params)
        }
        entry_field := join_entry_field_own(blk, fx.exit_field_own, entry_own, done)
```

becomes:

```tw
        if blk.id.id == 0 {
          entry_prov = seed_param_prov(entry_prov, f.params)
          entry_own = seed_param_own(entry_own, unique_seed, f.params)
        }
        entry_field := join_entry_field_own(blk, fx.exit_field_own, entry_own, done)
```

Without this, a block-0 return (as in `case_w_param`'s single-block caller) reclassifies
the return with the param entering `Unknown`, so the recovery fails in the replay and
`flows_to_return` comes back false — breaking the Task 1 acceptance even though the main
pass (`fx`) was seeded. `join_entry_field_own` is passed the now-seeded `entry_own`, so
its shell-gated field join reflects the owned entry.

Then add the two public entry points immediately **above** `summarize_seeded` (so
`summarize_function` keeps its original name and 5-arg signature for `summary.tw:395`):

```tw
// Generic summary: every reference parameter enters Unknown (empty seed). This is the
// caller-visible/conservative summary the SCC driver fixes.
pub fn summarize_function(
  f: CfgFunction,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
) Summary {
  summarize_seeded(f, table, b, sem, suppress, Dict.new())
}

// Owned-entry (ownership-specialized) summary for a VariantId key (Stage 3): re-run the
// same analysis with each keyed parameter's SHELL seeded Unique at entry, so the return-
// path recovery gate fires for a param-threaded transport arg. STAGE 3 IS SHELL-ONLY: it
// seeds own=Unique for `[]` reqs and seeds NO field_own -- a unique record shell does not
// imply unique field backings (Phase 4). A `[.f]` req is ignored here; field-granular
// seeding (distinguishing a unique .types backing from a shared .values sibling, D6) is
// Stage 4. Downward-closed keys always carry the param's `[]` req, so its shell is seeded.
// This does NOT canonicalize/downward-close the key -- the Stage 4 caller must pass a
// canonical, downward-closed key (variant_id.canonicalize_key/downward_close). Stage 3 tests
// pass keys directly; a `[[], [.f]]` key seeds the same shell as `[[]]` (the `[]` req).
pub fn summarize_variant(
  f: CfgFunction,
  key: vid.UniqueKey,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
) Summary {
  unique_seed: Dict<Int, Bool> = Dict.new()
  for req in key {
    if req.path.is_shell() and req.param >= 0 and req.param < f.params.len() {
      unique_seed[f.params[req.param].id] = true
    }
  }
  summarize_seeded(f, table, b, sem, suppress, unique_seed)
}
```

- [ ] **Step 5: Run the suite to verify the recovery test passes**

Run (pipefail-safe): `set -o pipefail; make boot-test 2>&1 | grep -aE 'Ran [0-9]+ tests|FAIL|owned-entry re-analysis recovers' | tail -5`
Expected: `Ran N tests: N passed` (the runner prints a `x <name>` line only on failure, so a
green run shows just the count; a failure prints the `stage3: owned-entry re-analysis recovers`
name). The recovery test asserts `gen` param0 `Published`, `owned` param0 not `Published`,
and `flows_to_return` true.

If the recovery test fails with `owned` param0 still `Published`: confirm `arg_unique[0]` is
firing — temporarily
`println` inside the test `summary.render_summary(owned)`; it should show `p0=Borrowed`
(or `Consumed`) with `ret` naming p0, **not** `Published`. Verify **both** seed sites are
patched: `run_fixpoint`'s block-0 guard (the main pass, drives `esc`) **and** the
return-site replay's block-0 guard (drives `ret`/`ret_paths`/`flows_to_return`). If
`p0` is not `Published` but `flows_to_return` is false, the **return-replay seed** is
missing. Remove any print before committing.

- [ ] **Step 6: Add the empty-key parity + shell-only guard tests**

```tw
    .test(
      "phase6 stage3: summarize_variant with an empty key equals summarize_function",
      fn() {
        funcs := case_w_param_fixture()
        b := b_reg()
        v := cfg.build_view(module_of(funcs), b)
        t := summary.compute(v, b, sem())
        caller := case cfg.function_named(v, "caller") {
          .Some(f) => f,
          .None => error("no caller cfg"),
        }
        empty_key: variant_id.UniqueKey = []
        variant := ownership.summarize_variant(caller, empty_key, t, b, sem(), Dict.new())
        generic := ownership.summarize_function(caller, t, b, sem(), Dict.new())
        try assert.is_true(summary.same_summary(variant, generic)) // empty seed == generic
        .Ok({})
      },
    )
    .test(
      "phase6 stage3: a field-only ([.f]) key seeds nothing (Stage 3 is shell-only)",
      fn() {
        // A non-shell req is ignored by summarize_variant, so a field-only key recovers
        // nothing -- ctx stays Published, same as the generic pass. Field-granular seeding
        // is Stage 4. (Real downward-closed keys always carry the [] req too.)
        funcs := case_w_param_fixture()
        b := b_reg()
        v := cfg.build_view(module_of(funcs), b)
        t := summary.compute(v, b, sem())
        caller := case cfg.function_named(v, "caller") {
          .Some(f) => f,
          .None => error("no caller cfg"),
        }
        field_key: variant_id.UniqueKey = [variant_id.UniqueReq.{ param: 0, path: variant_id.field(0) }]
        owned := ownership.summarize_variant(caller, field_key, t, b, sem(), Dict.new())
        try assert.is_true(is_published(owned, 0)) // no shell seed -> not recovered

        // A downward-closed key { [], [.f] } seeds the shell (from the [] req) and behaves
        // exactly like { [] } -- the field req adds nothing in the shell-only Stage 3.
        dc_key: variant_id.UniqueKey = [
          variant_id.UniqueReq.{ param: 0, path: variant_id.shell() },
          variant_id.UniqueReq.{ param: 0, path: variant_id.field(0) },
        ]
        dc := ownership.summarize_variant(caller, dc_key, t, b, sem(), Dict.new())
        try assert.is_true(!is_published(dc, 0)) // recovered via the shell req
        .Ok({})
      },
    )
```

- [ ] **Step 7: Run the full suite (generic guard must stay green)**

Run (pipefail-safe): `set -o pipefail; make boot-test 2>&1 | grep -aE 'Ran [0-9]+ tests|FAIL' | tail -3`
Expected: `Ran N tests: N passed`. In particular the existing generic guard
(`"param-threaded state is NOT recovered …"`, `cfg_return_paths_suite.tw:592`) still
passes — Stage 3 does **not** change the generic pass. If any pre-existing summary test
changed, the generic pass was accidentally altered (the seed must be empty for
`summarize_function`); do not "update" those expectations — fix the seeding.

- [ ] **Step 8: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw
target/twk lint boot/main.tw    # expect only the pre-existing findings; fix any NEW finding
git add boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw
git commit -m "phase6 stage3: owned-entry re-analysis (summarize_variant)

Re-run the ownership analysis with a VariantId key's parameters seeded Unique at
block-0 entry (seed_param_own threaded through run_fixpoint). A param-threaded
transport arg is then recovered: arg_unique fires, the OwnedFromParam recovery
gate no longer publishes it, and the param classifies Borrowed/Consumed instead
of Published. summarize_function is now a thin empty-seed wrapper, so the generic
pass is unchanged. No transfer changes (same re-run). Whole-value MayAliasParams
recovery + field-granular partial seeding are Stage 4; the variant memo/SCC
fixpoint is Stage 5. Analysis only: census stays 0."
```

---

## Task 2: Stage 3 verification

**Files:** none (verification only). All commands use `set -o pipefail` so a `make` failure is not masked by a trailing `grep`/`tail`.

- [ ] **Step 1: Census unchanged (analysis-only invariant)**

Run: `set -o pipefail; target/twk ir boot/main.tw --census 2>&1 | awk 'NR>1 && NF>=3 {s+=$NF} END{print "total in_place:", s+0}'`
Expected: `total in_place: 0`.

- [ ] **Step 2: Generic output unchanged (empty-seed wrapper is a no-op)**

The authoritative check is the **untouched generic guard** (`cfg_return_paths_suite.tw:592`,
asserted green in Task 1 Step 7) plus **byte-identical builds** (Step 4) and the
**self-host fixed point** (Step 3): `compute` only ever calls `summarize_function`
(empty seed) on `boot/main.tw`, and `seed_param_own` with an empty seed is identity, so
generic summaries and emitted code cannot change. As a cheap smoke test, the field-path
render count is also stable:

Run: `set -o pipefail; target/twk ir boot/main.tw --cfg 2>&1 | grep -c 'Consumed paths{\[\],\[\.f'`
Expected: the **same** count as before Stage 3 (capture on `HEAD~` if unsure). A changed
count means the empty-seed wrapper is not actually identity — investigate rather than
re-baseline.

- [ ] **Step 3: Full boot suite + self-host fixed point (unmasked)**

Run: `make boot-test`
Expected (read the tail): `Ran N tests: N passed`. Then, run alone (no parallel heavy `twk`):

Run: `set -o pipefail; touch boot/compiler/ownership.tw && make stage2 2>&1 | tail -4`
Expected: `Fixed point reached: stage3 == stage4`.

- [ ] **Step 4: Byte-identical builds (determinism)**

Run: `set -o pipefail; target/twk build boot/main.tw -o /tmp/p6s3_x.wasm && target/twk build boot/main.tw -o /tmp/p6s3_y.wasm && cmp /tmp/p6s3_x.wasm /tmp/p6s3_y.wasm && echo IDENTICAL`
Expected: `IDENTICAL`.

- [ ] **Step 5: Lint clean**

Run: `target/twk lint boot/main.tw`
Expected: no new findings beyond the pre-existing ones (`--explain` for rationale).

---

## Deferred (tracked)

| Item | Home |
|---|---|
| Whole-value `MayAliasParams(k)` recovery (the `add_type` caller). **Resolved by Stage 4a** via option (b) below — no representation needed for the direct case. | **Stage 4a** ([2026-07-19-phase6-stage4a-whole-return-move.md](2026-07-19-phase6-stage4a-whole-return-move.md)) |
| Field-granular partial-ownership seeding (`field_own` Unique for `[.f]` with a shared sibling — D6 mixed ownership); this is where a `[.f]` req in a key gains its distinct effect | **Stage 4** |
| Per-call-site key selection + `consume_dead` + `ConsumedPaths` | **Stage 4** |
| Demand-driven variant memo + SCC variant fixpoint (D12, ascending iterated cell, recursion tie-the-knot) | **Stage 5** |
| Rendering the specialized summary + per-site variant decision in `twk ir --cfg` | **Stage 6** |
| Reference-vs-scalar field filter | **Stage 2c** (still open) |

## Self-Review

- **Spec coverage (scoped — the shell/ret-path slice of D10):** this plan implements the **shell-owned-entry / ret-path transport** slice of D10/D11/D13 — `summarize_variant` re-runs the analysis with the key's **shell** slots seeded `Unique` (D13's "same transfer, different entry"), pinned by the `case_w_param` recovery test (exactly the fixture the design names: "the owned-variant caller recovers … the generic still publishes"). It does **not** implement full D10 per-path seeding: `phase6-design.md` D10 says the keyed `(param, path)` slots enter `Unique` *including field/path-level ownership*, but Stage 3 seeds only the shell (`own`) and ignores `[.f]` reqs. **Field/path-level owned-entry seeding is the Stage 4 portion of D10.** So this is the D10 *shell* increment, not full D10. The generic guard is preserved (not relaxed); the owned recovery is asserted alongside it.
- **No transfer changes (D13):** the only code added to the analysis is a block-0 seed applied at **both** forwarding sites — the `run_fixpoint` main pass (drives `esc`) and the `summarize_seeded` return-site replay (drives `ret`/`ret_paths`/`flows_to_return`). The recovery gate, transfer, and finalization are untouched. Missing the return-replay seed would silently break `flows_to_return` (review finding — now Task 1 Step 4).
- **Shell-only, no unsound field claim:** Stage 3 seeds `own = Unique` for `[]` reqs and **no** `field_own`; it does **not** assume a unique shell implies unique field backings (Phase 4: it does not). `summarize_variant` seeds only `req.path.is_shell()` reqs — a `[.f]`-only key recovers nothing (guard test, Step 6). Field-granular seeding is Stage 4.
- **Honest scope boundary (Stage 2b reconciled):** the whole-value `MayAliasParams` publish (`ownership.tw:1160`) is **not** recovered by seeding — so the `add_type` caller stays `Published` here; that recovery is **Stage 4a** (the caller-side `arg_unique` move; no representation needed for the direct case — deferral table). The Stage 2b docs (which said Stage 3 closes it) are corrected as part of this review. The acceptance uses the ret-path transport case (`case_w_param`) that seeding *does* handle, so the test is meaningful, not a fake pass.
- **Generic pass provably unchanged:** `summarize_function` = `summarize_seeded(…, Dict.new())`, and `seed_param_own` with an empty seed is identity — verified by census 0, unchanged `--cfg` count, byte-identical builds, self-host fixed point, and the untouched generic guard. This is the load-bearing safety property (Stage 3 must not perturb the generic summaries the SCC driver depends on).
- **Certain assertions:** the acceptance asserts only what is certain — `gen` param0 `Published`, `owned` param0 **not** `Published` and `flows_to_return` — capturing the recovery without over-committing to `Borrowed`-vs-`Consumed` (which depends on whether the transport move invalidates the binding; Step 5 reveals it via a temporary render if needed).
- **Type consistency:** `run_fixpoint(…, unique_seed: Dict<Int, Bool>)`, `seed_param_own(entry, unique_seed, params)`, `summarize_seeded(…, unique_seed)`, `summarize_variant(f, key: vid.UniqueKey, …)` are used consistently; both `run_fixpoint` call sites (`analyze_function`, `summarize_seeded`) pass the new argument; `summary.tw:395` keeps calling the unchanged 5-arg `summarize_function`. `key`'s `UniqueReq.param` is a param **index** mapped to a local id via `f.params[req.param].id` before seeding (the seed dict is keyed by local id, matching `entry_own`).
- **Determinism/termination:** the seed is applied at block-0 entry every iteration (like a fresh-unique param); it only lowers the entry lattice point, so the fixpoint still converges. Empty-seed generic output is byte-identical (Task 2).
- **Verification not masked:** every `make boot-test`/`make stage2` check uses `set -o pipefail` or is run unpiped; `twk lint` is run after edits.
