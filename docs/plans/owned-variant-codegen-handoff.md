# Uniform-Caller Aggregate-Carrier Entry Seeding (owned-variant handoff, first cut)

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development`
> (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking. **Read the prerequisite finding first:**
> `docs/plans/aggregate-field-owned-variants.md` → "STATUS (2026-07-27)". This plan is a **first
> cut** of change **#1** ("codegen handoff") from that finding's revised design, plus the #2 body
> rewrite needed to make its first customer flip.

> **Scope, stated precisely (do not overclaim):** this plan lands the **uniform-caller entry-seed
> handoff**. It does **NOT** consume the `compute_variants` `VariantSummaryTable`, and it does NOT
> implement the full owned-variant **clone-dispatch** handoff (which is what would handle *mixed*
> callers). It reuses the pre-existing non-cloning uniform-caller path. Clone-dispatch remains a
> separate, deferred plan (see "Out of scope").

**Goal:** So that a function returning a fresh aggregate whose fields are owned carriers (e.g.
`merge_targeted`) emits an in-place dict/vector update **when every direct caller passes the
carrier Unique + last-use** — proven end-to-end by a census flip across **all** its monomorphs and
`selected_decision_count(..., "dict_set") > 0` on a fixture that *cannot* flip through any existing
path.

**Architecture:** The investigation found the compiler **already has** a non-cloning codegen
consumer for "every caller passes this param Unique" facts: `uniform_entry_seeds`
(`boot/compiler/codegen/ownership_verdicts.tw:443`) proves, per callee param, that *every* direct
caller passes it Unique (a built-in mixed-caller guard — the `failed` set), and threads the
resulting Unique seeds into the verdict pass (`analyze_with_summaries_and_entry_seeds`). The seed
*candidate* set is `seed_param_indices` (`:328`) = consumed-path targets ∪ copy-carrier sources.
**This plan adds a third source — aggregate-field carriers — to that set**, reusing the entire
existing uniform-caller proof and verdict machinery. No function cloning, no new dispatch, no
`compute_variants` consumption. Function cloning (for *mixed* callers) is out of scope.

**Tech stack:** Twinkle self-hosted compiler (`boot/`), `make bundle-cli` self-host loop,
`target/twk ir --census/--cfg` ownership probes, `TWINKLE_FIXVERIFY` guard, boot test suite.

---

## Why this is a spike-first plan (the load-bearing risk)

Three prior findings (see the referenced STATUS section) established that flipping `merge_targeted`
needs (1) a codegen consumer, (2) a body rewrite so `out` is genuinely Unique at the mutation, and
(3) aggregate-carrier detection. This plan realizes (1) via the non-cloning entry-seed path. But
**one assumption is unproven and gates the entire plan**:

> **Assumption A:** If `merge_targeted`'s carrier param `p1` is added to the entry-seed candidate
> set AND its body is rewritten to drop the post-copy `next` read, then (a) `uniform_entry_seeds`
> actually seeds `p1` Unique (i.e. all callers pass it Unique+last-use), and (b) the verdict pass
> then resolves the `out[k]=` site to `reuse(unique)` instead of `persistent(...)`.

Both halves are measurable *before* committing to the design. **Task 1 is a throwaway spike that
measures them.** If (a) fails, `merge_targeted`'s callers are non-uniform and only the
clone-dispatch fallback can flip it — stop and re-scope. If (b) fails, seeding is not enough and
the verdict transfer itself needs work — stop and re-scope. Do **not** build Tasks 2–6 until Task 1
confirms both.

**Baseline to record before starting (ground truth):**

```bash
# All three monomorphs are persistent today:
target/twk ir boot/main.tw --census --sites 2>/dev/null \
  | grep -E '^merge_targeted__' | grep dict_set
# → merge_targeted__Int ... false ... base=persistent(aliased shell) borrow-effect copy-carrier source ...
#   (also __Bool, __Vec_Int)

# The pre-existing tracked-red fixverify baseline (NOT introduced by this work):
TWINKLE_FIXVERIFY=1 target/twk build boot/main.tw -o /tmp/fv.wasm 2>&1 | tail -1
# → fixverify mismatch: analyze:unique_analysis_diags   (measure DELTA against this, not absolute)

# Boot suite tracked marker:
target/twk test 2>&1 | tail -1
# → Ran N tests: N-1 passed, 1 failed  (the merge_targeted + run_fixpoint marker)
```

---

## ⛔ SPIKE RESULT (2026-07-27): Assumption A FAILED at the caller side — this plan cannot flip merge_targeted

Task 1 was run inline. All three edits (COW detection, seed union, body rewrite) applied and
self-host reached `stage3 == stage4`. Measured outcome:

- **The body rewrite worked.** `merge_targeted__`'s generic summary moved from `p1=Published` to
  **`p1=Consumed paths{[]}`** — p1 is now a proper whole-value carrier (returned `ret_paths=.f0=from(p1)`),
  even a `target_params` seed candidate, without needing the aggregate path. So Findings 1–2 of the
  prerequisite doc are addressed by the rewrite alone.
- **The flip still did NOT happen.** All three monomorphs stayed
  `... false ... base=persistent(aliased shell) borrow-effect copy-carrier source`. p1 is a valid
  seed *candidate* but `uniform_entry_seeds` does **not** seed it.
- **Root cause (caller-side, deeper than "mixed callers"):** on the `boot/main.tw` (production)
  path, `merge_targeted`'s only call sites are `ownership.tw:6100/6111/6122` inside `run_fixpoint`,
  and each passes `next` as **`st.own` / `st.valid` / `st.prov`** — field projections of a **live
  `ForwardState` record**. A field projection of a live record is never Unique + last-use, so
  `uniform_entry_seeds`' `failed` guard correctly declines to seed p1. **No caller passes `next`
  Unique**, so the flip is impossible via the uniform-caller seed path. (Nuance: `merge_targeted` is
  *also* called directly from `boot/tests/suites/cfg_lattice_suite.tw`, but tests are not in the
  `boot/main.tw` census/emission path, so they don't affect the production seed decision.)

**Consequence — even future clone-dispatch still needs a Unique call-site argument.** There is no
codegen clone-dispatch today; but even if it existed, a variant clone is only selected at a call site
that passes the carrier Unique. There is **no such site** — all three pass a shared record field. So
the true blocker is not "which handoff" — it is that the caller (`run_fixpoint`) holds its maps as
fields of a live `ForwardState` and hands *projections* to `merge_targeted`.

**The real lever is caller-side (this reorders the roadmap):** `run_fixpoint` must pass its maps as
**owned, moved-out locals** rather than live-record fields — i.e. the E-DRY `fixpoint_iterate`
refactor (thread the maps as returned carriers in a `FixState`, consuming them each iteration)
is not a *beneficiary* of this work, it is the **prerequisite** that creates a Unique caller. Only
then does `merge_targeted` (body-rewritten, p1 a carrier) get seeded and flip.

**Disposition:** the body rewrite (Task 3) is a real, self-host-safe improvement worth keeping on its
own; the seed union (Task 4) and COW detection (Task 2b) are sound but flip nothing until a Unique
caller exists. **Tasks 2–6 are on hold.** Next step: author `docs/plans/fixpoint-edry-fixstate.md`
for the caller-side refactor (move the `ForwardState` maps to owned carriers), with the
`merge_targeted` flip as its acceptance gate — the code below is the reusable analysis half, gated on
that. All spike edits were reverted (repo clean at the plan commit).

---

## File structure

- `boot/compiler/summary.tw` — extend the in-place-site scan to COW `.Update` builtins (dict/vector),
  and make the aggregate-carrier computation reusable from the codegen layer. **New/changed:**
  `call_is_cow_update_on_derived`, `scan_inplace_op` (+`sem` param), `param_has_inplace_site`
  (+`sem`), `pub fn aggregate_carrier_params` (+`sem`, made `pub`), and their callers
  (`candidate_variants`, `candidates_by_func`, `compute_variants` call site — thread `sem`).
- `boot/compiler/codegen/ownership_verdicts.tw` — add aggregate carriers as a third seed source in
  `seed_param_indices`. **Changed:** `seed_param_indices` unions `aggregate_carrier_params`.
- `boot/compiler/ownership.tw` — behavior-preserving body rewrite of `merge_targeted` so `out` is
  Unique at the mutation. **Changed:** `merge_targeted` (`:5180`).
- `boot/compiler/opt/semantics.tw` — read-only; source of `call_info` / `CallSemantics`
  (`:234`, `:27`). No change.
- `boot/tests/fixtures/sound_uniqueness/phase8i_dict_aggregate_carrier.tw` — a new fixture proving
  the uniform-caller flip. **Must NOT use the `out := a` copy shape** (that already flips via
  copy-carrier — see `phase8d_dict_merge_helper_return.tw`); it rebinds the param directly and
  returns it in a fresh aggregate, so only the new aggregate-carrier seed can flip it.
- `boot/tests/suites/mutable_produce_suite.tw` — a `selected_decision_count` + no-copy-carrier
  attribution assertion on the fixture; later, split the tracked marker so the `merge_targeted`
  (all-monomorphs) half goes green and the `run_fixpoint` half stays tracked-red.

---

## Task 1 — SPIKE (throwaway): does seed + body-rewrite flip merge_targeted?

**Files (temporary edits, reverted at end of task):** `boot/compiler/summary.tw`,
`boot/compiler/codegen/ownership_verdicts.tw`, `boot/compiler/ownership.tw`.

This task writes the real edits from Tasks 2–4 *at once*, measures the two halves of Assumption A,
records the result in this doc, then **reverts everything**. It exists to de-risk, not to land code.

- [ ] **Step 1: Apply the COW-`.Update` in-place-site recognition** (same as Task 2). In
  `boot/compiler/summary.tw`, add the helper and an `.ACall` arm to `scan_inplace_op`, and thread
  `sem` through `scan_inplace_op` → `param_has_inplace_site` → `aggregate_carrier_params` →
  `candidate_variants` → `candidates_by_func` → the `compute_variants` call. Also add
  `use compiler.opt.semantics.{OptimizerSemantics, call_info}` (add `call_info`).

```tw
// A COW `.Update` builtin (`m[k]=v` -> Dict.set, `xs[i]=v` -> VECTOR_SET, ...) whose
// cow_base_arg is a param-derived collection is an in-place update site at the shell,
// exactly like ARecordUpdate is for a record field. Lets a dict/vector carrier param —
// invisible to the ARecordUpdate-only scan — bootstrap as a candidate.
fn call_is_cow_update_on_derived(
  sem: OptimizerSemantics,
  callee: Atom,
  args: Vector<Atom>,
  derived: Dict<Int, Bool>,
) Bool {
  case callee {
    .AGlobalFunc(fid) => case call_info(sem, fid) {
      .Some(cs) => case cs.effect {
        .Update => case cs.cow_base_arg {
          .Some(bk) => bk >= 0 and bk < args.len() and atom_is_derived(derived, args[bk]),
          _ => false,
        },
        _ => false,
      },
      .None => false,
    },
    _ => false,
  }
}
```

Add to `scan_inplace_op`'s `case op { ... }` (new signature `scan_inplace_op(sem, op, dst, st)`):

```tw
    .ACall(callee, args) => if call_is_cow_update_on_derived(sem, callee, args, st.derived) {
      st.found = true
      st.mark_derived(dst)
    } else {
      st
    },
```

- [ ] **Step 2: Add aggregate carriers to the entry-seed candidate set** (same as Task 4). Make
  `aggregate_carrier_params` `pub` in `summary.tw`, then in
  `boot/compiler/codegen/ownership_verdicts.tw` extend `seed_param_indices`:

```tw
fn seed_param_indices(
  f: cfg.CfgFunction,
  s: ownership.Summary,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  table: ownership.SummaryTable,
) Vector<Int> {
  idxs: Dict<Int, Bool> = Dict.new()
  for p in target_params(s) {
    idxs[p] = true
  }
  for p in ownership.structural_seed_params_for_borrow_effects(f, b, sem, table) {
    idxs[p] = true
  }
  for p in summary.aggregate_carrier_params(sem, f, s) {
    idxs[p] = true
  }
  collect p in idxs.keys() {
    p
  }
}
```

(`summary` is already imported in `ownership_verdicts.tw`; confirm with
`grep -n 'use compiler.summary' boot/compiler/codegen/ownership_verdicts.tw` and add the import if
absent.)

- [ ] **Step 3: Body-rewrite `merge_targeted`** (same as Task 3) at `boot/compiler/ownership.tw:5180`:

```tw
  keys := int_keys_union(old.keys(), next.keys())
  out := next
  next_locked := locked
  for k in keys {
    old_x := lat_get(old, k, default_value)
    next_x := lat_get(out, k, default_value)
    prev_x := lat_get(prev, k, default_value)
    changed := !eq(old_x, next_x)
    oscillates := has_prev and changed and eq(next_x, prev_x)
    if live_contains_int(next_locked, k) or oscillates or force_lock_changed and changed {
      next_locked = insert_sorted(next_locked, k)
      out[k] = join(old_x, next_x)
    }
  }
  MergeOut.{ map: out, locked: next_locked }
```

The only changes vs the original: `keys := ...` is hoisted **above** `out := next`, and
`next_x := lat_get(next, ...)` becomes `lat_get(out, ...)`. Behavior-preserving because union keys
are unique, so `out[k]` is untouched until its own iteration and equals the original `next[k]`.

- [ ] **Step 4: Rebuild and MEASURE the two halves of Assumption A.**

```bash
target/twk fmt boot/compiler/summary.tw boot/compiler/codegen/ownership_verdicts.tw boot/compiler/ownership.tw
make bundle-cli 2>&1 | tail -2   # must still reach "Fixed point reached: stage3 == stage4"

# (a) Did uniform_entry_seeds seed p1 (are callers uniform-Unique)?  AND
# (b) did the verdict flip?  Check ALL THREE monomorphs (baseline says all three are persistent):
target/twk ir boot/main.tw --census --sites 2>/dev/null \
  | grep -E '^merge_targeted__(Int|Bool|Vec_Int)' | grep dict_set
```

**Decision (paste the exact census lines for all three monomorphs back into this section, AND
record the full reason string — Task 5's fixture attribution reuses it):**
- **FLIPPED** — **all three** rows show `... true ... reuse(unique)` (or `dict$set_in_place ...
  MutableSelected`): Assumption A holds. Note the reason suffix (is it plain `base=reuse(unique)`,
  or does it still carry `borrow-effect copy-carrier`?) — Task 5 asserts a discriminator against
  this. Proceed to Task 2.
- **Only some monomorphs flip:** investigate the divergence before proceeding — the carrier detection
  or seeding differs per type-arg, which must be understood (do not proceed with a partial flip).
- **Still `persistent(aliased shell)`** — dump why and branch:
  ```bash
  target/twk ir boot/main.tw --cfg 2>/dev/null | grep -A1 '^fn merge_targeted__Int' | grep summary:
  ```
  - If p1 is **not** seeded (verdict pass never treats it Unique): callers are non-uniform →
    **STOP**, only clone-dispatch (fallback below) can flip merge_targeted. Re-scope.
  - If p1 **is** seeded but `out` is still Shared at the write: the verdict transfer for a
    returned aggregate carrier is the gap → **STOP**, this plan's seed approach is insufficient;
    open a verdict-transfer design task.

- [ ] **Step 5: Revert the spike.**

```bash
git checkout -- boot/compiler/summary.tw boot/compiler/codegen/ownership_verdicts.tw boot/compiler/ownership.tw
```

- [ ] **Step 6: Commit the recorded finding** (docs only — the spike code is reverted).

```bash
git add docs/plans/owned-variant-codegen-handoff.md
git commit -m "docs: record owned-variant handoff spike result (Assumption A: <held|failed>)"
```

> **Tasks 2–6 below assume the spike FLIPPED.** If it did not, they do not apply — re-scope per the
> Step 4 branch you hit.

---

## Task 2a — Expose `aggregate_carrier_params` + add the no-copy-carrier fixture (behavior unchanged)

This step is a pure visibility change plus test scaffolding, so the Task 2b red is a **behavioral**
failure (carriers wrongly `0`), not a compile error. Do NOT add the COW detection here.

**Files:**
- Modify: `boot/compiler/summary.tw` (`fn aggregate_carrier_params` → `pub fn`, **signature
  unchanged** `(f: CfgFunction, s: Summary)`).
- Create: `boot/tests/fixtures/sound_uniqueness/phase8i_dict_aggregate_carrier.tw`.
- Test: `boot/tests/suites/cfg_summary_suite.tw`.

- [ ] **Step 1: Make `aggregate_carrier_params` public** — change `fn aggregate_carrier_params` to
  `pub fn aggregate_carrier_params` (`boot/compiler/summary.tw:690`). No other change.

- [ ] **Step 2: Create the no-copy-carrier fixture.** It MUST avoid `out := a` (that shape already
  flips via the copy-carrier path — see `phase8d_dict_merge_helper_return.tw`). The carrier param is
  rebound **directly** and returned inside a fresh aggregate, so *only* the aggregate-carrier seed
  can flip it (no aliasing copy ⇒ not copy-carrier; base summary never marks `a` Consumed ⇒ not a
  `target_params` seed). One direct caller passes it fresh (Unique + last-use):

```tw
// boot/tests/fixtures/sound_uniqueness/phase8i_dict_aggregate_carrier.tw
type Pair = .{ map: Dict<Int, Int> }

fn build(a: Dict<Int, Int>) Pair {
  a[1] = 2
  Pair.{ map: a }
}

fn build_caller() Pair {
  m: Dict<Int, Int> = Dict.new()
  m[0] = 0
  build(m)
}

println(build_caller().map.len().to_string())
```

- [ ] **Step 3: Add a characterization test** pinning today's behavior (carriers **empty**), which
  also proves the API compiles from another module. In `cfg_summary_suite.tw`, reuse the
  `pipeline.compile_entry_path("${fixtures_dir()}/...")` pattern from `mutable_produce_suite.tw:21`
  (add a local `fixtures_dir()`/`compile_fixture` helper if the suite lacks one):

```tw
    .test(
      "aggregate_carrier_params: dict carrier NOT yet detected (pre-COW baseline)",
      fn() {
        art := try compile_fixture("phase8i_dict_aggregate_carrier")
        sem := semantics.make_prelude_optimizer_semantics(art.builtins)
        view := cfg.build_view(art.opt, art.builtins)
        f := try cfg.function_named(view, "build").ok_or("build not found")
        table := summary.compute(view, art.builtins, sem)
        s := try table.summary_get(f.func_id).ok_or("no summary")
        // Pre-fix: scan_inplace_op is ARecordUpdate-only, so a[1]=2 (a dict .ACall) is invisible.
        try assert.equal(summary.aggregate_carrier_params(f, s).len(), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 4: Rebuild, verify the characterization test PASSES, self-host + suite unchanged.**

```bash
target/twk fmt boot/compiler/summary.tw
make bundle-cli 2>&1 | tail -1                    # stage3 == stage4 (pure visibility change)
target/twk run boot/tests/main.tw 2>&1 | grep -i "dict carrier NOT yet detected"   # PASS
target/twk test 2>&1 | tail -1                    # same tracked marker, no new failures
```

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/summary.tw boot/tests/suites/cfg_summary_suite.tw \
  boot/tests/fixtures/sound_uniqueness/phase8i_dict_aggregate_carrier.tw
git commit -m "ownership: expose aggregate_carrier_params + pin dict-carrier detection gap"
```

## Task 2b — Add COW-`.Update` detection (behavioral red → green)

**Files:**
- Modify: `boot/compiler/summary.tw` (`scan_inplace_op`, `param_has_inplace_site`,
  `aggregate_carrier_params`, `candidate_variants`, `candidates_by_func`, `compute_variants` call,
  imports).
- Test: `boot/tests/suites/cfg_summary_suite.tw` (flip the Task 2a assertion).

- [ ] **Step 1: Turn the characterization test into the failing spec.** Change the Task 2a test to
  assert detection (and rename it), so it now FAILS. It will also need the new `(sem, f, s)`
  signature — update the call:

```tw
    .test(
      "aggregate_carrier_params detects a dict carrier via COW-Update in-place site",
      fn() {
        art := try compile_fixture("phase8i_dict_aggregate_carrier")
        sem := semantics.make_prelude_optimizer_semantics(art.builtins)
        view := cfg.build_view(art.opt, art.builtins)
        f := try cfg.function_named(view, "build").ok_or("build not found")
        table := summary.compute(view, art.builtins, sem)
        s := try table.summary_get(f.func_id).ok_or("no summary")
        try assert.equal(summary.aggregate_carrier_params(sem, f, s).len() > 0, true)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run it, verify it FAILS behaviorally** (not a compile error):

```bash
make bundle-cli 2>&1 | tail -1   # compiles (sem-threaded API lands with the impl below in the same edit set)
target/twk run boot/tests/main.tw 2>&1 | grep -i "detects a dict carrier via COW-Update"
# Expected: FAIL — carriers.len() == 0 (COW arm not added yet)
```
(If you prefer strict red-before-impl: temporarily add only the `sem` threading + `call_info` import
so the test compiles, confirm the FAIL, then add the `.ACall` arm.)

- [ ] **Step 3: Apply the detection fix** — exactly Task 1 Step 1: add the
  `call_is_cow_update_on_derived` helper and the `.ACall` arm to `scan_inplace_op`; thread `sem`
  through `scan_inplace_op` → `param_has_inplace_site` → `aggregate_carrier_params` (now
  `pub fn aggregate_carrier_params(sem, f, s)`) → `candidate_variants` → `candidates_by_func` → the
  `compute_variants` call; add `call_info` to the `use compiler.opt.semantics.{...}` import.

- [ ] **Step 4: fmt, rebuild, verify the test PASSES + self-host + suite.**

```bash
target/twk fmt boot/compiler/summary.tw
make bundle-cli 2>&1 | tail -1                    # stage3 == stage4
target/twk run boot/tests/main.tw 2>&1 | grep -i "detects a dict carrier via COW-Update"   # PASS
target/twk test 2>&1 | tail -1                    # same tracked marker, no new failures
```

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/summary.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "ownership: detect COW-Update dict/vector in-place sites for aggregate carriers"
```

---

## Task 3 — Body-rewrite `merge_targeted` (behavior-preserving)

**Files:** Modify `boot/compiler/ownership.tw` (`merge_targeted`, `:5180`).

- [ ] **Step 1: Apply the rewrite** — exactly Task 1 Step 3 (hoist `keys` above `out := next`; read
  `next_x` via `out`).

- [ ] **Step 2: fmt + rebuild + self-host (this fn is used by the compiler itself).**

```bash
target/twk fmt boot/compiler/ownership.tw
make bundle-cli 2>&1 | tail -2   # MUST reach stage3 == stage4 — a behavior change here breaks self-host
```

- [ ] **Step 3: Behavioral-equivalence gate.** The rewrite is only safe if it changes no results.

```bash
target/twk test 2>&1 | tail -1   # identical pass/fail counts to baseline (still 1 tracked red)
```
Expected: the boot suite is **unchanged** from baseline (the `merge_targeted` marker has not flipped
yet — that happens in Task 5). If any *other* test regresses, the rewrite is not equivalent — revert
and stop.

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: hoist merge_targeted next reads so out is unique at the in-place write

Behavior-preserving (union keys are unique, so out[k] equals original next[k] until its
own iteration); drops the post-copy read of next so the aliased-shell verdict can clear
once p1 is seeded Unique."
```

---

## Task 4 — Thread aggregate carriers into the entry-seed candidate set

**Files:** Modify `boot/compiler/codegen/ownership_verdicts.tw` (`seed_param_indices`).

- [ ] **Step 1: Confirm the `summary` import** exists in the file:

```bash
grep -n "use compiler.summary" boot/compiler/codegen/ownership_verdicts.tw
```
If absent, add `use compiler.summary`.

- [ ] **Step 2: Extend `seed_param_indices`** — exactly Task 1 Step 2 (the third `for p in
  summary.aggregate_carrier_params(sem, f, s)` loop). No other change; the union is idempotent (a
  param already present via `target_params`/copy-carrier is a no-op).

- [ ] **Step 3: fmt, lint, rebuild, and re-measure the census flip across ALL monomorphs (the
  primary integration gate).**

```bash
target/twk fmt boot/compiler/codegen/ownership_verdicts.tw
target/twk lint boot/main.tw
make bundle-cli 2>&1 | tail -2
target/twk ir boot/main.tw --census --sites 2>/dev/null \
  | grep -E '^merge_targeted__(Int|Bool|Vec_Int)' | grep dict_set
```
Expected (given Task 1 confirmed Assumption A): **all three** rows now read `... true ...
reuse(unique)` (or `dict$set_in_place ... MutableSelected`). If any monomorph stays persistent,
re-open Task 1's Step 4 branch — the combined edits behave differently than the spike measured,
which must be understood before landing.

- [ ] **Step 4: Differential attribution check (prove the flip is THIS seed, not a pre-existing
  path).** Temporarily comment out the new `for p in summary.aggregate_carrier_params(...)` loop,
  rebuild, and confirm the `phase8i_dict_aggregate_carrier` fixture goes back to persistent; then
  restore the loop. This proves the fixture's flip is attributable to the new seed source and not to
  copy-carrier/`target_params`.

```bash
# with the new loop removed:
make bundle-cli 2>&1 | tail -1
target/twk ir boot/tests/fixtures/sound_uniqueness/phase8i_dict_aggregate_carrier.tw --census --sites 2>/dev/null \
  | grep -E '^build\b' | grep dict_set          # expect: ... false ... persistent(...)
# restore the loop, rebuild, re-confirm it flips to reuse(unique).
```

- [ ] **Step 5: fixverify delta gate.** The pre-existing `analyze:unique_analysis_diags` mismatch is
  the tracked-red baseline; the flip must not introduce a *different* or *additional* mismatch.

```bash
TWINKLE_FIXVERIFY=1 target/twk build boot/main.tw -o /tmp/fv.wasm 2>&1 | tail -3
```
Expected: the **same** single `analyze:unique_analysis_diags` line (or fewer). A new mismatch key
means an unsound flip — stop and diagnose (the seed unioned a param whose in-place write is not
actually safe).

- [ ] **Step 6: Commit.**

```bash
git add boot/compiler/codegen/ownership_verdicts.tw
git commit -m "codegen: seed aggregate-field carriers Unique via the uniform-caller entry path

seed_param_indices now unions aggregate_carrier_params, so a returned-aggregate carrier that
every caller passes Unique+last-use is seeded Unique in the verdict pass and its dict/vector
update emits in-place. Reuses uniform_entry_seeds' existing mixed-caller guard (a non-uniform
caller keeps the base persistent) — no function cloning."
```

---

## Task 5 — End-to-end behavioral proof + retire the merge_targeted marker

**Files:** `boot/tests/suites/mutable_produce_suite.tw` (add a `selected_decision_count` assertion;
update the tracked marker).

- [ ] **Step 1: Add a fixture-level flip assertion WITH copy-carrier attribution.** Reuse the Task
  2a fixture `phase8i_dict_aggregate_carrier` (`build` rebinds `a` directly — no `out := a` — and
  returns it in a fresh `Pair`, called once with a fresh dict). Assert both that it flips AND that
  the flip's proof is **not** the copy-carrier path (the shape already excludes copy-carrier and
  `target_params`; this locks that in). Mirror the `render_candidate_rows` + `rendered_line_for_family`
  pattern used by the `phase8d_*` tests (`mutable_produce_suite.tw:362–364`). Add near the existing
  `phase8d_dict_merge_targeted_min` test (`mutable_produce_suite.tw:~348`):

```tw
    .test(
      "aggregate carrier flips dict_set in-place via the new seed (not copy-carrier)",
      fn() {
        produced := try produce_for("phase8i_dict_aggregate_carrier")
        try assert.equal(selected_decision_count(produced, "build", "dict_set") > 0, true)

        rendered := mutable_produce.render_candidate_rows(produced.rows)
        helper := rendered_line_for_family(rendered, "build", "dict_set")
        try assert.str_contains(helper, "decision produced")
        try assert.str_contains(helper, "base=reuse(unique)")
        // Discriminator: prove attribution is NOT the copy-carrier path. Confirm the exact
        // reason suffix against the Task 1 spike's recorded reason string; adjust if the
        // aggregate-carrier seed renders a different (non-copy-carrier) reason.
        try assert.is_false(helper.contains("copy-carrier"))
        .Ok({})
      },
    )
```
  (The Task 4 Step 4 differential check — removing the seed loop makes this fixture persistent — is
  the stronger, one-off attribution proof; this test is the durable guard.)

- [ ] **Step 2: Split the existing tracked marker into a green half and a red half.** The marker
  (`mutable_produce_suite.tw:380–388`, "boot ownership fixpoint maps should produce in-place dict
  decisions") already asserts **both** should flip and fails because neither does:

```tw
    .test(
      "boot ownership fixpoint maps should produce in-place dict decisions",
      fn() {
        produced := try produce_boot_main()
        try assert.is_true(selected_decision_count(produced, "merge_targeted__", "dict_set") > 0)
        try assert.is_true(selected_decision_count(produced, "run_fixpoint", "dict_set") > 0)
        .Ok({})
      },
    )
```
  `merge_targeted__` now flips but `run_fixpoint` does not, so a single test with both asserts stays
  red and hides the win. Replace it with two tests — the `merge_targeted__` half goes green now, the
  `run_fixpoint` half stays the tracked-red (awaits the E-DRY beneficiary, out of scope):

```tw
    .test(
      "boot merge_targeted produces in-place dict decisions (all monomorphs)",
      fn() {
        produced := try produce_boot_main()
        // Baseline lists exactly three monomorphs, all persistent; require EACH to flip so a
        // partial (one-monomorph) flip cannot pass as success.
        try assert.is_true(selected_decision_count(produced, "merge_targeted__Int", "dict_set") > 0)
        try assert.is_true(selected_decision_count(produced, "merge_targeted__Bool", "dict_set") > 0)
        try assert.is_true(selected_decision_count(produced, "merge_targeted__Vec_Int", "dict_set") > 0)
        .Ok({})
      },
    )
    // Tracked target marker for docs/plans/fixpoint-map-inplace.md — the run_fixpoint half.
    // Intentionally red until run_fixpoint's loop-carried maps ride the E-DRY FixState helper
    // (docs/plans/fixpoint-edry-beneficiary.md). Not a regression.
    .test(
      "boot run_fixpoint maps should produce in-place dict decisions",
      fn() {
        produced := try produce_boot_main()
        try assert.is_true(selected_decision_count(produced, "run_fixpoint", "dict_set") > 0)
        .Ok({})
      },
    )
```

- [ ] **Step 3: Keep/relocate the marker comment.** The existing comment above the old marker
  (`:375–379`) already points at `fixpoint-map-inplace.md`; move it onto the new `run_fixpoint` test
  and update it to reference the E-DRY follow-up as shown above. The `merge_targeted__` half needs no
  "intentionally red" comment — it should pass.

- [ ] **Step 4: fmt + full suite.**

```bash
target/twk fmt boot/tests/suites/mutable_produce_suite.tw
target/twk test 2>&1 | tail -3
```
Expected: the fixture flip test (Step 1) and the `merge_targeted` all-monomorphs test PASS; exactly
**one** known failure remains, and its name is the `run_fixpoint` marker (exit 1 is expected). If the
failing test is anything else, stop.

- [ ] **Step 5: Commit.**

```bash
git add boot/tests/suites/mutable_produce_suite.tw
git commit -m "test: prove aggregate-carrier in-place flip; land merge_targeted, run_fixpoint half awaits E-DRY"
```

---

## Task 6 — Update the plan docs & README (close the loop)

**Files:** `docs/plans/aggregate-field-owned-variants.md`, `docs/plans/fixpoint-map-inplace.md`,
`docs/plans/README.md`.

- [ ] **Step 1: Record the outcome.** In `aggregate-field-owned-variants.md`'s STATUS section, mark
  change **#1** DONE via the non-cloning entry-seed path (not clone-dispatch), and #2 (body rewrite)
  DONE. Note that the SCC-variant/vtable generalization (that plan's Tasks 4–6) remains **unbuilt
  and unneeded for the uniform-caller case** — it is the clone-dispatch fallback for mixed callers
  (still deferred).

- [ ] **Step 2: Update `fixpoint-map-inplace.md`** top banner: the `merge_targeted` half of the
  boundary is cleared; `run_fixpoint` remains, to be won by the E-DRY beneficiary (author that plan
  next, per the house rule).

- [ ] **Step 3: Commit.**

```bash
git add docs/plans/aggregate-field-owned-variants.md docs/plans/fixpoint-map-inplace.md docs/plans/README.md
git commit -m "docs: owned-variant codegen handoff landed for merge_targeted (uniform-caller path)"
```

---

## Out of scope (documented fallback: clone-dispatch for mixed callers)

This plan flips a carrier **only when every caller passes it Unique** (`uniform_entry_seeds`'
mixed-caller guard keeps the base persistent otherwise). A carrier called Unique from some sites and
Shared from others cannot flip via a single emitted function — it needs a **separate, guarded
variant clone** plus call-site dispatch to the clone. That is the original "codegen handoff via the
`compute_variants` vtable" idea, and it requires:

- Consuming the validated `VariantSummaryTable` (`compute_variants`) in `compute_artifacts` — today
  it is read only by diagnostic/test rendering paths (the `twk ir --cfg` command at
  `boot/commands/ir.tw:62` and the `cfg_sound_uniqueness_fixtures_suite`), never by emission.
- Emitting a variant clone per validated key (a third monomorphization axis: `(func, type-args,
  unique-key)`; interning exists in `variant_id.tw` but the emit/naming path does not).
- Call-site selection of the clone: `mutable_produce`'s `variant_key` field
  (`mutable_produce.tw:369` — currently always `.None`) would carry the selected key;
  `ownership.select_variant` (`ownership.tw:7671`) derives a key from the base summary's
  `in_place_paths` and is **insufficient as-is** (aggregate carriers have empty base
  `in_place_paths`) — selection must read the validated vtable, not the base summary.

Author this as a separate plan (`docs/plans/owned-variant-clone-dispatch.md`) **only if** a real
mixed-caller customer is found; the uniform-caller path (this plan) is the correct first cut and may
cover every real customer (`merge_targeted`, and `run_fixpoint`'s cold callers pass fresh
`Dict.new()`).

## Out of scope (separate plan)

- The `run_fixpoint` E-DRY beneficiary (extract a `FixState`-returning `fixpoint_iterate` helper so
  the 14 maps ride the same uniform-caller flip). Author `docs/plans/fixpoint-edry-beneficiary.md`
  after this plan lands, per the referenced STATUS section.

## Self-review notes

- **Spec coverage:** #1 codegen handoff → Task 4 (via the pre-existing non-cloning path) proven by
  Task 1 spike; #2 body rewrite → Task 3; #3 aggregate detection → Task 2a (expose) + 2b (COW
  detection); end-to-end gate → Task 5. Review blockers folded in: no-copy-carrier fixture (Task 2a
  Step 2 + Task 5 Step 1 attribution), API-exposure-before-behavioral-red split (2a/2b),
  all-monomorph acceptance (Tasks 1/4/5).
- **The pivotal reframe vs the prior finding:** the finding assumed the handoff meant *function
  cloning*. Task 1's research found `uniform_entry_seeds` is a ready non-cloning consumer, so the
  handoff collapses to unioning one seed source — *if* Assumption A holds, which Task 1 measures
  before any code lands.
- **Type consistency:** `aggregate_carrier_params(sem, f, s) Vector<Int>` (made `pub`, `sem`-first)
  is called identically in `summary.tw` (`candidate_variants`) and `ownership_verdicts.tw`
  (`seed_param_indices`); `call_is_cow_update_on_derived(sem, callee, args, derived) Bool` reuses
  `call_info`/`CallSemantics` (`opt/semantics.tw:234,27`) and `atom_is_derived` (existing in
  `summary.tw`).
- **Soundness rests on two existing proofs, not new ones:** `uniform_entry_seeds` proves the
  caller side (all callers Unique + last-use); the verdict pass (`shell_verdict` /
  `reusable_shell`) proves the site side (Unique + reusable at the write). This plan only widens the
  *candidate* set; both guards still gate every flip. Task 4 Step 4 is a differential attribution
  check (removing the seed makes the fixture persistent) and Step 5 checks the fixverify delta.
