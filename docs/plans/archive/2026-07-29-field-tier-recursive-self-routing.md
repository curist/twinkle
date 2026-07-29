# Field-Tier Recursive Self-Routing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route a field-tier variant clone's in-SCC recursive call back to itself, so an owned recursive record-field collection update lowers in place at every recursion depth, not just the first.

**Architecture:** The caller→clone route already works (a full-tier clone `[unique:p0,p0.f0]` is built and its block-0 field-backed update emits in place). The gap is the clone's *own* recursive call: `variant_specialize.recursive_routes_for` re-analyzes the generic callee under a **shell-only** seed (`seed_for_variant`), so the self-call proves only `p0` and routes to the shell tier (or stays generic). This plan threads the clone's **field-path seed** (`summary.field_seed_for_variant`, already built for clone emission in 8H) into the recursive-route ownership analysis so the self-call proves `p0.f0` and `select_variant_for_arg_paths` picks the full-tier clone.

**Tech Stack:** Twinkle boot compiler (`boot/`), self-hosted. Build via `make quick-bundle-cli` while iterating; boot tests via `target/twk run boot/tests/main.tw`; inspect with `target/twk ir <fixture> --census --sites` and `target/twk wat <fixture> --func <fn> --calls`. No Rust stage0 changes.

**Design source:** `docs/plans/sound-uniqueness/codegen/README.md` §"Codegen Phase 8H" ("Field-tier recursive self-routing" deferral), and the as-built 8G/8H seams in `variant_specialize.tw`, `ownership.tw`, `summary.tw`.

---

## Global Constraints

- Emit path must stay sound: a clone's self-call routes to the field clone **only** when the recursive-route analysis proves the field path, exactly as the caller route already requires. Absence of proof falls back to the shell tier or the generic function (the current behavior).
- No new operation families, no runtime-helper ABI changes, no source-semantics changes.
- Field-path seeds come only from the exact canonical `VariantId` (`field_seed_for_variant`); a shell-unique receiver never implies field-backed ownership.
- Byte-identical output for every fixture that does **not** contain a field-tier recursive self-call (verify against the `sound_uniqueness` fixture WAT set).
- After editing `.tw` files: `target/twk fmt <files>` then `target/twk lint boot/main.tw`.
- Do not run tree-sitter tests.

## Orientation — the exact current state

`field_visit_rec.tw` is the driving fixture. Its census today:

```text
f295 -> f299 "visit$v299" [f295|0:;0:0] sites=1 rec=0 routed proof=8g:f295|0:;0:0
visit$v299 ... record_backed_vector_set ... vector$set_in_place ... selected  (block-0 update, in place)
```

- `sites=1` — the caller `go` routes to the full-tier clone `visit$v299`.
- `rec=0` — the clone's own `visit(cur, n-1)` self-call is **not** routed to `visit$v299`; deeper iterations run the generic `visit` (persistent).
- Target after this plan: `rec=1`.

Key code:

- `boot/compiler/codegen/variant_specialize.tw:409` `recursive_routes_for` — computes `seed := summary.seed_for_variant(ccfg, g.variant)` (shell-only `Dict<Int, Bool>`) then `sited := ownership.call_uniques_sited(ccfg, table, b, sem, Dict.new(), seed)`. The field paths of `g.variant` are dropped here.
- `boot/compiler/summary.tw:1273` `field_seed_for_variant(f, v) Dict<Int, ff.FieldMap>` — already exists; returns the depth-one field seed for a variant's non-shell requirements. This is exactly what clone emission uses.
- `boot/compiler/ownership.tw:6802` `call_uniques_sited(f, table, b, sem, suppress, unique_seed)` — runs `run_fixpoint_validated(..., unique_seed, ...)` (no field seed) then materializes per-block states; at block 0 it applies `seed_param_own(entry_own, unique_seed, params)` but **no** `seed_param_field_own`.
- `boot/compiler/ownership.tw:7116` `ownership_stage` (the analyze path) is the field-aware reference: it seeds `entry_field = seed_param_field_own(entry_field, field_seed, params)` at block-0 materialization (line 7191). Note it does **not** thread `field_seed` into `run_fixpoint_validated`. Whether block-0-only field seeding reaches a self-call in a *later* block is the open question resolved by Task 2.

## File Structure

- **Modify** `boot/compiler/ownership.tw` — give `call_uniques_sited` a `field_seed: Dict<Int, ff.FieldMap>` parameter (keep the current 6-arg signature as a wrapper passing `Dict.new()`); apply it at block-0 materialization, and — if Task 2 requires — thread it into `run_fixpoint_validated` so it propagates across blocks.
- **Modify** `boot/compiler/codegen/variant_specialize.tw` — `recursive_routes_for` passes `summary.field_seed_for_variant(ccfg, g.variant)` into `call_uniques_sited`.
- **Modify** `boot/tests/suites/field_backed_collection_suite.tw` — add the recursive-route assertion.

---

## Task 1: Failing test — the field clone's self-call must route to itself

**Files:** Modify `boot/tests/suites/field_backed_collection_suite.tw`.

**Interface:** reuse the existing `specialize_module`/route inspection. A `VariantRoute` (see `variant_route.tw`) carries `clone_func`, `variant`, and `recursive_routes: Vector<Int>`; `recursive_routes.len() > 0` is the "rec routed" signal that renders as `rec=N` in the census.

- [ ] **Step 1: Add the failing assertion.** In `field_backed_collection_suite.tw`, add:

```twinkle
.test(
  "field-tier recursive clone routes its own recursive call to itself",
  fn() Result<Void, String> {
    art := try compile_fixture("field_visit_rec")
    spec := variant_specialize.specialize_module(art.opt, art.builtins)
    // The full-tier clone is the route whose variant requires a non-shell path.
    full := case spec.routes.find(fn(r) {
      r.clone_func >= 0 and r.variant.unique.any(fn(req) { !req.path.is_shell() })
    }) {
      .Some(r) => r,
      .None => return .Err("no full-tier clone route"),
    }
    try assert.ok(
      full.recursive_routes.len() > 0,
      "the field clone's in-SCC recursive call must route back to the clone (rec>0)",
    )
    .Ok({})
  },
)
```

- [ ] **Step 2: Run and verify the red state.**

```bash
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: the new test FAILS with "rec>0" — today `recursive_routes.len() == 0` for the full-tier clone.

- [ ] **Step 3: Commit the red test.**

```bash
git add boot/tests/suites/field_backed_collection_suite.tw
git commit -m "test(recursive-field): assert field clone routes its own recursion"
```

---

## Task 2: Spike — determine how far a block-0 field seed propagates

**Files:** none (investigation only). Output: a decision recorded in Task 3's implementation.

The open question: does seeding `field_own` at block-0 materialization (as `ownership_stage` does) make the field path visible at a self-call that lives in a **later** block (`field_visit_rec`'s self-call is in the `else` arm), or must the seed thread through `run_fixpoint_validated` so it flows across the block boundary?

- [ ] **Step 1: Add a temporary debug seed to the recursive-route path.** In `recursive_routes_for` (`variant_specialize.tw:422`), temporarily replace the `call_uniques_sited` call with a field-seeded variant that seeds only at block-0 materialization (Task 3 Step 1 builds the real API; here, prototype it inline or via a throwaway `call_uniques_sited_dbg`). Print each sited call's `arg_paths` for the `visit` self-call:

```bash
# after wiring a debug print of s.callee + s.arg_paths in recursive_routes_for
TWINKLE_TIMINGS=0 target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/field_visit_rec.tw --census --sites 2>&1 | grep -i "recdbg"
```

- [ ] **Step 2: Read the result and decide.**
  - If the self-call's `arg_paths[0]` already contains the field path with block-0-only seeding → **Decision A:** block-0 materialization seed is sufficient. Task 3 only touches the post-fixpoint block-0 seeding in `call_uniques_sited`.
  - If it does not → **Decision B:** the field seed must propagate through the fixpoint. Task 3 additionally threads `field_seed` into `run_fixpoint_validated` and seeds `entry_field` at the fixpoint's block-0 (mirroring how `unique_seed` already flows). `ownership_stage` should be updated to pass its `field_seed` there too, so the analyze path gains the same cross-block propagation (verify no `sound_uniqueness` WAT changes result — a pure precision *addition* on seeded clones only).

- [ ] **Step 3: Remove the debug print.** Record the decision (A or B) in the Task 3 commit message.

> Rationale for the spike: the emit path proves the clone's block-0 update field-owned via `EntrySeedFacts.field_own` through `ownership_stage`, but the *recursive-route* analysis is a separate `call_uniques_sited` pass. Which seeding depth is required is a fact about the fixpoint we must observe, not guess.

---

## Task 3: Thread the field seed into the recursive-route analysis

**Files:** Modify `boot/compiler/ownership.tw`; Modify `boot/compiler/codegen/variant_specialize.tw`.

**Interfaces:**
- `ownership.call_uniques_sited_with_field_seed(f, table, b, sem, suppress, unique_seed, field_seed) Vector<SitedCallUniq>`, with the existing `call_uniques_sited(...)` kept as a wrapper passing `ff`-empty `Dict.new()`.
- (Decision B only) `run_fixpoint_validated(..., field_seed, ...)` gains a `field_seed: Dict<Int, ff.FieldMap>` parameter; all existing callers pass `Dict.new()`.

- [ ] **Step 1: Add the field-seeded entry point.** In `ownership.tw`, rename the current `call_uniques_sited` body to `call_uniques_sited_with_field_seed` with the extra `field_seed: Dict<Int, ff.FieldMap>` parameter, and re-add the public wrapper:

```twinkle
pub fn call_uniques_sited(
  f: CfgFunction,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
  unique_seed: Dict<Int, Bool>,
) Vector<SitedCallUniq> {
  call_uniques_sited_with_field_seed(f, table, b, sem, suppress, unique_seed, Dict.new())
}
```

- [ ] **Step 2: Seed block-0 field ownership in the materialization.** In `call_uniques_sited_with_field_seed`, at the block-0 branch that currently does `entry_own = seed_param_own(entry_own, unique_seed, f.params)` (around `ownership.tw:6843`), add immediately after the `entry_field := join_entry_field_own(...)` line:

```twinkle
if blk.id.id == 0 {
  entry_field = seed_param_field_own(entry_field, field_seed, f.params)
}
```

- [ ] **Step 3 (Decision B only): thread the seed through the fixpoint.** Add `field_seed: Dict<Int, ff.FieldMap>` to `run_fixpoint_validated` and seed the fixpoint's block-0 `entry_field` the same way `unique_seed` is seeded, then have `call_uniques_sited_with_field_seed` and `ownership_stage` pass their `field_seed` through. Update every other `run_fixpoint_validated` caller to pass `Dict.new()`. Under Decision A, skip this step.

- [ ] **Step 4: Pass the variant's field seed from the recursive-route path.** In `variant_specialize.recursive_routes_for` (`variant_specialize.tw:422`), change:

```twinkle
seed := summary.seed_for_variant(ccfg, g.variant)
sited := ownership.call_uniques_sited(ccfg, table, b, sem, Dict.new(), seed)
```

to:

```twinkle
seed := summary.seed_for_variant(ccfg, g.variant)
field_seed := summary.field_seed_for_variant(ccfg, g.variant)
sited := ownership.call_uniques_sited_with_field_seed(
  ccfg, table, b, sem, Dict.new(), seed, field_seed,
)
```

- [ ] **Step 5: Format, lint, build, and run the Task 1 test green.**

```bash
target/twk fmt boot/compiler/ownership.tw boot/compiler/codegen/variant_specialize.tw
target/twk lint boot/main.tw
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: the Task 1 test passes (`recursive_routes.len() > 0`); all other suites stay green.

- [ ] **Step 6: Commit.**

```bash
git add boot/compiler/ownership.tw boot/compiler/codegen/variant_specialize.tw
git commit -m "codegen(recursive-field): seed variant field paths into recursive-route analysis

Decision <A|B> from the propagation spike: <one line>."
```

---

## Task 4: Verify emission, runtime parity, and byte-identical fallback

**Files:** Modify `boot/tests/suites/field_backed_collection_suite.tw`.

- [ ] **Step 1: Assert the clone's recursive path emits in place and the census shows rec routed.** Extend the suite:

```twinkle
.test(
  "recursive field clone stays in-place across depth (rec routed, in-place emit)",
  fn() Result<Void, String> {
    wat := try compile_fixture_wat("field_visit_rec")
    body := try wat_func_body_result(wat, "visit_v")
    try assert.ok(
      wat_has_instr(body, "rt_arr__set_in_place"),
      "the recursive field clone must set xs in place",
    )
    sites := ir_sites_text("field_visit_rec")
    try assert.str_contains(sites, "rec=1")
    .Ok({})
  },
)
```

- [ ] **Step 2: Runtime parity (specialize on/off) and manual census.**

```bash
target/twk run boot/tests/fixtures/cfg/sound_uniqueness/field_visit_rec.tw
TWINKLE_VARIANT_SPECIALIZE=0 target/twk run boot/tests/fixtures/cfg/sound_uniqueness/field_visit_rec.tw
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/field_visit_rec.tw --census --sites | grep -E "rec=|-> f"
```

Expected: both runs print `0`; census shows `... rec=1 routed`.

- [ ] **Step 3: Byte-identical fallback check.** Compile every `sound_uniqueness` fixture that has no field-tier recursion and confirm the WAT is unchanged from before this plan (capture a baseline before Task 1). A convenient set: all fixtures except `field_visit_rec` / `visit_rec` / `visit_rec_run`.

```bash
# baseline (before Task 1): target/twk build <each>.tw -o base_<name>.wat; shasum
# after:                    target/twk build <each>.tw -o new_<name>.wat;  shasum; diff
```

Expected: identical for every non-recursive-field fixture; only the recursive-field fixtures change (gaining the routed in-place recursion).

- [ ] **Step 4: Self-host gate.**

```bash
target/twk lint boot/main.tw
make bundle-cli
target/twk run boot/tests/main.tw
make stage2
```

Expected: lint clean; bundle succeeds; boot suite green; self-host reaches a fixed point (stage3 == stage4).

- [ ] **Step 5: Commit and update docs.**

```bash
git add boot/tests/suites/field_backed_collection_suite.tw
git commit -m "test(recursive-field): lock in in-place recursion and byte-identical fallback"
```

Then in `docs/plans/sound-uniqueness/codegen/README.md`, remove the "Field-tier recursive self-routing" item from the 8H "Deferred to a follow-up slice" list and note it is done (self-call routes to the field clone; `rec=1`). Remove this plan's row from `docs/plans/README.md` and move this file to `docs/plans/archive/`.

---

## Scope Boundary

Delivers self-routing for a **single** recursive function's field-tier clone (the `visit`-shape). It does **not** deliver:

- Full mutual-recursion SCC clone closure (a peer demanded only from inside another clone is still not created on demand — the 8G boundary is unchanged).
- Caller-side loop-carried/threaded field ownership (that is the separate loop/threaded-field-ownership plan; this plan only seeds the *recursive-route* re-analysis of an already-cloned function).
- Any new operation family or Elem/Val nested-value paths.

## Self-Review Checklist

1. **Spec coverage:** red route test (Task 1), propagation spike (Task 2), field-seed threading (Task 3), emission + parity + byte-identical + self-host (Task 4). ✓
2. **No text parsing / soundness:** routing still goes through `select_variant_for_arg_paths` on proven `arg_paths`; the only change is that the recursive-route analysis is now allowed to prove field paths via an explicit `VariantId`-derived seed. ✓
3. **Type consistency:** `call_uniques_sited_with_field_seed`, `field_seed: Dict<Int, ff.FieldMap>`, `field_seed_for_variant`, and `recursive_routes` are used consistently across tasks. ✓
4. **Fallback safety:** the 6-arg `call_uniques_sited` wrapper and empty `field_seed` keep every non-recursive path byte-identical; Decision-B fixpoint threading defaults every other caller to `Dict.new()`. ✓
