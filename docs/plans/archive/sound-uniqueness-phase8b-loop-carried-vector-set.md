# Sound Uniqueness Phase 8B: Loop-Carried Vector Set Emission Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Execution note (completed 2026-07-21):** Landed on branch
> `phase8b-loop-carried-vector-set`. One deviation surfaced during execution: the
> emittable loop-carried vector shape is **index-assignment sugar** (`flags[k] =
> false`), which lowers to an inline `vector$set_unsafe` caller candidate — **not**
> explicit `.set_at(...)` method calls, which lower to a prelude call whose update
> lives in a borrowed-param body and never surfaces as a caller-side candidate. The
> nested positive fixture and the aliased negative fixture below were therefore
> written with index sugar rather than the `.set_at` form originally drafted. (This
> does not affect the 8D/8E dict plans: `Dict.set`/`Dict.remove` are builtins and
> do produce caller-side candidates.) Self-host reached a fixed point; 3168 boot
> tests pass; ownership census unchanged.

**Goal:** Emit the existing `vector$set_in_place` helper for proven-owned *loop-carried* `Vector` indexed-update sites (single and nested loops), while every aliased, absent, stale, or unsupported loop site keeps the persistent path.

**Architecture:** Phase 8B is a codegen-only unlock, not new analysis. The ownership analysis already proves loop-carried uniqueness — the nested-loop-carried ownership work (archived `sound-uniqueness-nested-loop-ownership.md`) renders `verdict -> ...[unique:p0]` for loop and nested-loop `set_at`, and `twk ir --census --sites` already reports `would_use=true` / `base=reuse(unique)` for loop-contained sites. Phase 8A deliberately suppressed those decisions with a single `c.loop_depth == 0` gate in the decision *producer* (`mutable_produce.tw`); it did **not** distrust the verdict. This plan removes that gate so loop-contained reusable sites produce real `MutableDecision`s, adds loop-carried proof-id/reason rendering, and proves the emission with WAT/call inspection. The backend selector, emit policy (`phase8a_policy` enables the `VectorSet` family), and `vector$set_in_place` helper are unchanged and already handle these sites once a decision exists.

**Tech Stack:** Twinkle boot compiler (`boot/`), `target/twk`, ANF ownership analysis (`compiler.ownership`, `compiler.codegen.ownership_verdicts`), decision producer (`compiler.codegen.mutable_produce`), backend selector (`compiler.codegen.mutable_select`), WAT/call inspection via `target/twk build ... -o /tmp/file.wat` and `target/twk wat ... --func ... --calls`, boot test suite `target/twk run boot/tests/main.tw`, opt-in census `cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture`, self-host `make bundle-cli`, lint `target/twk lint boot/main.tw`.

## Global Constraints

- Treat the boot compiler in `boot/` as the primary implementation. No Rust stage0 change is required (this is a boot-codegen-only optimization; stage0 keeps the persistent path, which is always sound).
- Codegen consumes decisions; it does **not** re-prove uniqueness, last-use, field ownership, or escape. Phase 8B only stops the producer from *discarding* an already-proven loop-carried decision.
- Absence, ambiguity, or staleness of a decision emits the persistent operation. Aliased loop vectors stay persistent because `reusable_shell` is already `false` for them — do not add any loop-specific soundness logic in the producer.
- Phase 8B may emit only the `VectorSet` family. Dicts, builders, record shells, and function variants stay persistent (they belong to later Phase 8 slices).
- Do not change any file under `compiler.ownership` / `compiler.cfg` / `compiler.summary`. If a loop site is not proven reusable, that is an analysis concern out of scope here; the correct 8B behavior is persistent fallback.
- Never assert on rendered FuncIds, block ids, or ANF local *values* that are unstable across compiler changes. Assert stable text: function names, `set_in_place` / `rt_arr__set`, `loop-carried`, `candidate`, `selected`, and `would_use`.
- After editing `.tw` files, run `target/twk fmt <changed.tw>` and `target/twk lint boot/main.tw`. Lint any new fixture entries explicitly (`boot/main.tw` does not compile fixture files).
- Write WAT/CFG dumps under `/tmp/twinkle-phase8b/`, not inside the repository.
- Run verification commands one at a time, never concurrently or backgrounded (concurrent `twk` pegs CPU).

## Dev loop note (why `make bundle-cli` is not needed to iterate)

`target/twk run boot/tests/main.tw` compiles the boot test module — including the imported compiler modules `mutable_produce`, `mutable_select`, and the `codegen`/`pipeline` functions the WAT tests call — **from source** with the current `target/twk`, then runs it. So the boot test suite exercises your edited `mutable_produce.tw` immediately, with **no** `make bundle-cli`. `make bundle-cli` is only required to (a) refresh the standalone CLI so `target/twk ir --census --sites <fixture>` reflects the change, and (b) prove self-host in the final verification. Iterate with `target/twk run boot/tests/main.tw`; bundle once at the end.

---

## File Structure

- Modify `boot/compiler/codegen/mutable_produce.tw`
  - In `produce_phase8a_vector_set_decisions_from_validated_verdicts`, replace the `verdict.reusable_shell and c.loop_depth == 0` acceptance gate (plus its separate `loop_depth > 0` "deferred" branch) with a single `verdict.reusable_shell` gate that produces a decision regardless of loop depth.
  - Give loop-carried decisions a distinct `proof_debug_id` (`phase8b-loop:...` naming the carried base local, the update site, and the loop depth) and a distinct render `reason` (`decision produced (loop-carried, carry L<base>)`).
  - Update the function/module doc comments to describe intent (local and loop-carried owned vector-set sites) instead of the old "not nested inside a loop" wording.
- Modify `boot/tests/suites/mutable_produce_suite.tw`
  - Flip the "loop-contained vector set is deferred, not decided" test to assert a decision is produced with loop-carried proof text.
- Modify `boot/tests/suites/codegen_emit_suite.tw`
  - Flip `test_link_program_keeps_vector_set_persistent_inside_loop` to assert `rt_arr__set_in_place` is emitted for the loop fixture, and rename it accordingly.
  - Add a nested-loop positive WAT test and a loop-aliased negative WAT test, registered in `suite()`.
- Create `boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_nested.tw`
  - Positive fixture: a fresh `flags` vector carried by an outer loop and mutated through `.set_at` inside an inner loop (sieve shape).
- Create `boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_loop_alias.tw`
  - Negative fixture: a fresh vector aliased before a loop, mutated through `.set_at` in the loop, alias returned — must stay persistent.
- Modify `docs/plans/sound-uniqueness/codegen/README.md`
  - Check the Phase 8B checkboxes and advance "Current focus" to Phase 8C only after verification passes.
- Modify `docs/plans/sound-uniqueness/README.md`
  - Advance the "Current implementation focus" paragraph from Phase 8B to Phase 8C after verification passes.

The existing `boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw` (single loop, index-assignment sugar) is reused as-is; its behavior flips from persistent to in-place, which is exactly the emission this phase enables.

---

### Task 1: Produce decisions for loop-carried reusable vector-set sites

**Files:**
- Modify: `boot/compiler/codegen/mutable_produce.tw:327-401`
- Modify: `boot/tests/suites/mutable_produce_suite.tw:192-201`
- Modify: `boot/tests/suites/codegen_emit_suite.tw:1823-1830`

**Interfaces:**
- Consumes: `Candidate` (already carries `loop_depth`, `base`, `result`, `func`), `ownership_verdicts.SiteVerdict` (`reusable_shell` already `true` for proven loop sites), `mutable_select.with_decision`.
- Produces: a `MutableDecision` per proven-owned vector-set site regardless of loop depth, with loop-carried decisions distinguished in `proof_debug_id` and the render `reason`.

- [ ] **Step 1: Flip the producer unit test to expect a decision**

In `boot/tests/suites/mutable_produce_suite.tw`, replace the test at lines 192-201:

```tw
    .test(
      "loop-contained vector set is deferred, not decided",
      fn() {
        produced := try produce_for("phase8a_vector_set_loop")
        try assert.equal(produced.table.by_site.keys().len(), 0)
        rendered := mutable_produce.render_candidate_rows(produced.rows)
        try assert.str_contains(rendered, "loop-contained candidate deferred to Phase 8B")
        .Ok({})
      },
    )
```

with:

```tw
    .test(
      "loop-carried vector set produces a decision",
      fn() {
        produced := try produce_for("phase8a_vector_set_loop")
        try assert.equal(produced.table.by_site.keys().len(), 1)
        rendered := mutable_produce.render_candidate_rows(produced.rows)
        try assert.str_contains(rendered, "candidate")
        try assert.str_contains(rendered, "decision produced (loop-carried")
        proof_ids: Vector<String> = []
        for _key, ds in produced.table.by_site {
          for d in ds {
            proof_ids = proof_ids.append(d.proof_debug_id)
          }
        }
        joined := proof_ids.join(",")
        try assert.str_contains(joined, "phase8b-loop")
        try assert.str_contains(joined, "carry L")
        .Ok({})
      },
    )
```

- [ ] **Step 2: Flip the codegen WAT loop test to expect in-place emission**

In `boot/tests/suites/codegen_emit_suite.tw`, replace `test_link_program_keeps_vector_set_persistent_inside_loop` (lines 1823-1830):

```tw
fn test_link_program_keeps_vector_set_persistent_inside_loop() Result<Void, String> {
  wat := try compile_fixture_to_linked_wat(
    "${sound_uniqueness_fixtures_dir()}/phase8a_vector_set_loop.tw",
  )
  try assert.is_true(wat_has_call(wat, "rt_arr__set"))
  try assert.is_false(wat_has_call(wat, "rt_arr__set_in_place"))
  .Ok({})
}
```

with:

```tw
fn test_link_program_emits_vector_set_in_place_inside_loop() Result<Void, String> {
  wat := try compile_fixture_to_linked_wat(
    "${sound_uniqueness_fixtures_dir()}/phase8a_vector_set_loop.tw",
  )
  try assert.is_true(wat_has_call(wat, "rt_arr__set_in_place"))
  try assert.is_false(wat_has_call(wat, "rt_arr__set"))
  .Ok({})
}
```

Then update its registration in `suite()`. Find:

```tw
    .test(
      "link_program keeps vector$set persistent for a loop-contained site",
      test_link_program_keeps_vector_set_persistent_inside_loop,
    )
```

and replace with:

```tw
    .test(
      "link_program emits vector$set_in_place for a loop-carried owned vector",
      test_link_program_emits_vector_set_in_place_inside_loop,
    )
```

(If the registration label differs, match on the `test_link_program_keeps_vector_set_persistent_inside_loop` symbol and update both the label string and the symbol.)

- [ ] **Step 3: Run both tests and confirm they fail**

Run: `target/twk run boot/tests/main.tw 2>&1 | rg -n "loop-carried vector set produces|set_in_place for a loop-carried|FAIL|fail|passed"`
Expected: the two new/renamed tests FAIL — the producer still defers loop sites (0 decisions) and the loop fixture still emits `rt_arr__set`.

- [ ] **Step 4: Remove the loop-depth gate in the producer**

In `boot/compiler/codegen/mutable_produce.tw`, replace the whole `row: DecisionRenderRow = if ... { ... } else if ... { ... } else { ... }` block (lines 348-396) with a single reusable-vs-not decision:

```tw
    row: DecisionRenderRow = if verdict.reusable_shell {
      loop_carried := c.loop_depth > 0
      proof_id := if loop_carried {
        "phase8b-loop:${c.func}:carry L${c.base.id}:site L${c.result.id}:depth ${c.loop_depth}"
      } else {
        "phase8a:${c.func}:L${c.result.id}"
      }
      table = .with_decision(
        mutable_select.MutableDecision.{
          site: mutable_select.Site.{ func: c.func_id, local: c.result },
          family: mutable_select.OperationFamily.VectorSet,
          source_local: c.base,
          result_local: c.result,
          arg_count: c.arg_count,
          base_arg_index: .Some(c.base_arg_index),
          persistent_func: .Some(c.persistent_func),
          mutable_func: .Some(c.mutable_func),
          variant_key: .None,
          field_path_key: "",
          proof_debug_id: proof_id,
        },
      )
      reason := if loop_carried {
        "decision produced (loop-carried, carry L${c.base.id})"
      } else {
        "decision produced"
      }
      DecisionRenderRow.{
        func: c.func,
        local: c.result.id,
        family: c.family,
        persistent: persistent_name,
        mutable: mutable_name,
        state: "candidate",
        reason,
        proof: verdict.text,
      }
    } else {
      DecisionRenderRow.{
        func: c.func,
        local: c.result.id,
        family: c.family,
        persistent: persistent_name,
        mutable: mutable_name,
        state: "ignored",
        reason: "persistent fallback: ownership verdict did not certify reusable base",
        proof: verdict.text,
      }
    }
```

- [ ] **Step 5: Update the producer doc comments to describe intent**

In `boot/compiler/codegen/mutable_produce.tw`, update the doc comment above `phase8a_vector_set_candidates` (lines 192-197). Replace:

```tw
/// decision table (fed to the backend in a later phase) plus a render-ready
/// candidate row per site explaining why each candidate did or did not
/// produce a decision. A candidate only becomes a decision when its base is
/// certified reusable AND it is not nested inside a loop (loop-contained
/// candidates are deferred to Phase 8B).
```

with:

```tw
/// decision table (fed to the backend in a later phase) plus a render-ready
/// candidate row per site explaining why each candidate did or did not
/// produce a decision. A candidate becomes a decision when its base is
/// certified reusable by ownership analysis; this holds for straight-line
/// owned sites and for loop-carried owned sites, whose uniqueness is proven
/// across every entry and back-edge predecessor before `reusable_shell` is set.
```

Then update the comment above `produce_phase8a_vector_set_decisions_from_validated_verdicts` (lines 327-329). Replace:

```tw
// Join Phase 8A vector-set candidates with already-validated ownership verdicts.
// A candidate becomes a decision only when its base is certified reusable AND it
// is not nested inside a loop (loop-contained candidates defer to Phase 8B).
```

with:

```tw
// Join vector-set candidates with already-validated ownership verdicts. A
// candidate becomes a decision when its base is certified reusable, regardless
// of loop depth: loop-carried sites are proven owned across all predecessors by
// the ownership fixpoint, so the mutable helper is sound there too. Loop-carried
// decisions get a distinct `phase8b-loop` proof id naming the carried base local.
```

The `loop_depth` field on `Candidate` and the loop-depth counter in `walk_expr`/`walk_op` stay — they now drive the proof-id/reason distinction rather than a suppression gate. Leave the comment on `Candidate.loop_depth` (lines 43-45) mentioning Phase 8B; optionally update it to say loop depth now selects loop-carried proof rendering.

- [ ] **Step 6: Run both tests and confirm they pass**

Run: `target/twk run boot/tests/main.tw 2>&1 | rg -n "loop-carried vector set produces|set_in_place for a loop-carried|FAIL|fail|passed"`
Expected: both pass.

- [ ] **Step 7: Run the full boot suite to catch fallout**

Run: `target/twk run boot/tests/main.tw 2>&1 | rg -n "FAIL|fail|passed|assertion"`
Expected: no failures. In particular the existing positive fresh test (`test_link_program_emits_vector_set_in_place_for_owned_local`) and aliased negative test (`test_link_program_keeps_vector_set_persistent_for_aliased_local`) still pass, and the `phase8a candidate collection ... loop_depth == 0` test (mutable_produce_suite.tw:238-253) still passes — it inspects the fresh fixture, whose candidate is depth 0.

- [ ] **Step 8: Format, lint, and commit**

```bash
target/twk fmt boot/compiler/codegen/mutable_produce.tw boot/tests/suites/mutable_produce_suite.tw boot/tests/suites/codegen_emit_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/codegen/mutable_produce.tw boot/tests/suites/mutable_produce_suite.tw boot/tests/suites/codegen_emit_suite.tw
git commit -m "codegen: emit in-place vector set for loop-carried owned vectors"
```

Expected: `target/twk lint boot/main.tw` reports no new findings (pre-existing `variant_id.tw` inherent-call findings, if any, are unrelated).

---

### Task 2: Add nested-loop positive fixture and WAT regression

**Files:**
- Create: `boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_nested.tw`
- Modify: `boot/tests/suites/codegen_emit_suite.tw`

**Interfaces:**
- Consumes: Task 1's lifted producer gate plus the already-proven nested-loop ownership verdict.
- Produces: regression coverage that a `.set_at` accumulator carried through an outer loop and mutated in an inner loop (sieve shape) emits the in-place helper.

- [ ] **Step 1: Create the nested-loop positive fixture**

Create `boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_nested.tw`. The top-level `println` roots the function through linked DCE.

```tw
fn nested_set(n: Int) Vector<Bool> {
  flags: Vector<Bool> = collect _ in range(n) { true }
  i := 0
  for i < n {
    if flags[i] {
      step := i + 1
      k := i + step
      for k < n {
        flags = flags.set_at(k, false)
        k = k + step
      }
    }
    i = i + 1
  }
  flags
}

println(nested_set(10).len().to_string())
```

- [ ] **Step 2: Preflight the fixture renders reusable**

```bash
mkdir -p /tmp/twinkle-phase8b
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_nested.tw 2>&1 | rg -n "nested_set|would_use|verdict|reuse|selected|set_in_place"
```

Expected (with a bundled CLI; if the CLI is stale, this preflight instead confirms the pre-change behavior and the boot-suite test in Step 4 is the authority): the `nested_set` update site shows `would_use=true` and `base=reuse(unique)`. If it shows `persistent(aliased shell)` or `would_use=false`, stop — the analysis does not prove this nested shape yet, and this fixture must be reduced to a shape the ownership analysis already certifies (compare against the archived `nested_loop_sieve_shape` fixture) rather than changing analysis in this plan.

- [ ] **Step 3: Add the nested-loop positive WAT test**

In `boot/tests/suites/codegen_emit_suite.tw`, add next to the other `test_link_program_*` functions:

```tw
fn test_link_program_emits_vector_set_in_place_for_nested_loop() Result<Void, String> {
  wat := try compile_fixture_to_linked_wat(
    "${sound_uniqueness_fixtures_dir()}/phase8b_vector_set_nested.tw",
  )
  try assert.is_true(wat_has_call(wat, "rt_arr__set_in_place"))
  .Ok({})
}
```

Register it in `suite()` next to the loop test:

```tw
    .test(
      "link_program emits vector$set_in_place for a nested loop-carried owned vector",
      test_link_program_emits_vector_set_in_place_for_nested_loop,
    )
```

This asserts only the presence of the in-place helper; the fixture also monomorphizes `set_at` (whose runtime body legitimately contains `rt_arr__set`), so do not assert absence of `rt_arr__set` here.

- [ ] **Step 4: Run the test and confirm it passes**

Run: `target/twk run boot/tests/main.tw 2>&1 | rg -n "in_place for a nested loop|FAIL|fail|passed"`
Expected: pass.

- [ ] **Step 5: Format, lint, and commit**

```bash
target/twk fmt boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_nested.tw boot/tests/suites/codegen_emit_suite.tw
target/twk lint boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_nested.tw
target/twk lint boot/main.tw
git add boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_nested.tw boot/tests/suites/codegen_emit_suite.tw
git commit -m "test: cover nested loop-carried in-place vector set emission"
```

---

### Task 3: Add loop-aliased negative fixture and WAT regression

**Files:**
- Create: `boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_loop_alias.tw`
- Modify: `boot/tests/suites/codegen_emit_suite.tw`

**Interfaces:**
- Consumes: Task 1's lifted gate; the analysis's existing alias detection (`reusable_shell == false` for an aliased loop vector).
- Produces: a guard proving the lifted gate did not over-reach — an alias observable across the loop keeps the update persistent.

- [ ] **Step 1: Create the loop-aliased negative fixture**

Create `boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_loop_alias.tw`:

```tw
fn loop_alias(n: Int) Vector<Bool> {
  flags: Vector<Bool> = collect _ in range(n) { true }
  keep := flags
  i := 0
  for i < n {
    flags = flags.set_at(i, false)
    i = i + 1
  }
  keep
}

println(loop_alias(10).len().to_string())
```

`keep` aliases the fresh vector and is returned unmodified, so an in-place mutation of `flags` inside the loop would be observable through `keep`. The ownership analysis leaves `flags` non-unique at the update site, so `reusable_shell` is `false` and the producer must emit the persistent path.

- [ ] **Step 2: Preflight the fixture stays persistent**

```bash
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_loop_alias.tw 2>&1 | rg -n "loop_alias|would_use|verdict|persistent|reuse"
```

Expected (bundled CLI): the `loop_alias` update site shows `would_use=false` and a `persistent(...)` verdict. If it shows `would_use=true`, stop — that is an analysis soundness bug outside this plan's scope; do not paper over it in the producer.

- [ ] **Step 3: Add the loop-aliased negative WAT test**

In `boot/tests/suites/codegen_emit_suite.tw`, add:

```tw
fn test_link_program_keeps_vector_set_persistent_for_loop_alias() Result<Void, String> {
  wat := try compile_fixture_to_linked_wat(
    "${sound_uniqueness_fixtures_dir()}/phase8b_vector_set_loop_alias.tw",
  )
  try assert.is_true(wat_has_call(wat, "rt_arr__set"))
  try assert.is_false(wat_has_call(wat, "rt_arr__set_in_place"))
  .Ok({})
}
```

Register it in `suite()`:

```tw
    .test(
      "link_program keeps vector set persistent for a loop-carried aliased vector",
      test_link_program_keeps_vector_set_persistent_for_loop_alias,
    )
```

- [ ] **Step 4: Run the test and confirm it passes**

Run: `target/twk run boot/tests/main.tw 2>&1 | rg -n "persistent for a loop-carried aliased|FAIL|fail|passed"`
Expected: pass. If `rt_arr__set_in_place` appears, the analysis is not detecting the alias across the loop and this must be investigated before proceeding — it would be an unsound emission.

- [ ] **Step 5: Format, lint, and commit**

```bash
target/twk fmt boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_loop_alias.tw boot/tests/suites/codegen_emit_suite.tw
target/twk lint boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_loop_alias.tw
target/twk lint boot/main.tw
git add boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_loop_alias.tw boot/tests/suites/codegen_emit_suite.tw
git commit -m "test: guard loop-carried in-place emission against aliased vectors"
```

---

### Task 4: Docs, self-host, census, and final verification

**Files:**
- Modify: `docs/plans/sound-uniqueness/codegen/README.md`
- Modify: `docs/plans/sound-uniqueness/README.md`

**Interfaces:**
- Consumes: passing loop-carried emission tests.
- Produces: a self-hosted CLI, census confirmation, and updated phase docs advancing the active focus to Phase 8C.

- [ ] **Step 1: Rebuild the self-hosted CLI**

Because `boot/compiler/codegen/mutable_produce.tw` is part of the self-hosted compiler payload, rebuild and prove self-host:

```bash
make bundle-cli
```

Expected: the self-host loop reaches a fixed point and `target/twk` is rebuilt. If self-host diverges, stop and investigate before touching docs.

- [ ] **Step 2: Inspect the loop fixtures through the rebuilt CLI**

```bash
mkdir -p /tmp/twinkle-phase8b
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8a_vector_set_loop.tw   > /tmp/twinkle-phase8b/single.txt
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_nested.tw > /tmp/twinkle-phase8b/nested.txt
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8b_vector_set_loop_alias.tw > /tmp/twinkle-phase8b/alias.txt
rg -n "would_use|selected|policy_disabled|absent_fallback|set_in_place|persistent" /tmp/twinkle-phase8b/single.txt /tmp/twinkle-phase8b/nested.txt /tmp/twinkle-phase8b/alias.txt
```

Expected:
- `single.txt` and `nested.txt` post-prepare `mutable decisions` audit show `selected` with `vector$set_in_place` (no longer `absent_fallback`).
- `alias.txt` shows `would_use=false` / persistent and no `selected` in-place decision.

- [ ] **Step 3: Confirm the ownership census is unchanged**

```bash
cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture 2>&1 | rg -n "census|baseline|total|FAIL|ok"
```

Expected: the census total is unchanged from the committed baseline. This is a codegen-only change; it must not move ownership-analysis facts. If the count changed, an analysis file was touched inadvertently — revert that.

- [ ] **Step 4: Update the codegen README Phase 8B section**

In `docs/plans/sound-uniqueness/codegen/README.md`, check the two Phase 8B boxes:

```markdown
## Codegen Phase 8B — Loop-carried vector updates ✅ done

- [x] **Extend vector indexed-update lowering to loop-carried accumulators.**
  Target shapes like `flags = flags.set_at(k, false)` and
  `balls = balls.set_at(j, updated_ball)` where back-edge facts prove ownership.
  The producer no longer suppresses loop-contained candidates; a decision is
  produced whenever `reusable_shell` holds, and the analysis's entry/back-edge
  validation is the soundness source. Aliased loop vectors stay persistent
  because `reusable_shell` is `false` for them.
- [x] **Render loop proof ids near emitted decisions.** Loop-carried decisions
  carry a `phase8b-loop:...` proof id naming the carried base local, the update
  site, and the loop depth, and the producer render row reason reads
  `decision produced (loop-carried, carry L<base>)`.
```

Then update the status line at the top of the file (lines 3-18) so "Current focus" reads Phase 8C (existing builder-region lowering) instead of Phase 8B, and note that single- and nested-loop-carried vector indexed updates now emit `vector$set_in_place`.

- [ ] **Step 5: Update the umbrella README current-focus paragraph**

In `docs/plans/sound-uniqueness/README.md`, replace the "Current implementation focus: Codegen Phase 8B, loop-carried vector updates." paragraph (lines 42-52) so it states Phase 8B is complete — single and nested loop-carried owned `Vector.set_at` sites emit `vector$set_in_place`, aliased loop vectors stay persistent — and the active focus is now Phase 8C (existing builder-region lowering). Keep the surrounding scope-boundary paragraph about storage representation unchanged.

Also update the Codegen track summary paragraph (lines 102-111) so it says the loop-carried vector slice (Phase 8B) is done and current work moves to builder regions (Phase 8C).

- [ ] **Step 6: Final full verification, one command at a time**

```bash
target/twk run boot/tests/main.tw
target/twk lint boot/main.tw
```

Expected: boot suite fully passes; lint reports no new findings.

- [ ] **Step 7: Confirm no stray artifacts, then commit docs**

```bash
git status --short
find . -path './.git' -prune -o -name '*.wat' -print
ls /tmp/twinkle-phase8b 2>/dev/null
git add docs/plans/sound-uniqueness/codegen/README.md docs/plans/sound-uniqueness/README.md
git commit -m "docs: mark sound-uniqueness codegen Phase 8B complete"
```

Expected: no WAT dumps inside the repository; `/tmp/twinkle-phase8b` holds the temporary dumps. Docs-only commit.

Final report should state:
- whether single-loop and nested-loop owned `Vector.set_at` now emit `vector$set_in_place`;
- whether the aliased loop guard stayed persistent;
- boot suite, census (unchanged), self-host, and lint outcomes;
- that the emission policy, backend selector, and helper were unchanged (producer-only unlock);
- any residual work: dict/builder/record/variant families remain persistent (Phases 8C–8H), and this slice reuses persistent storage — typed/dense vector storage is the separate storage-representation track.

---

## Self-Review

**Spec coverage** (codegen README Phase 8B, two checklist items):
1. "Extend vector indexed-update lowering to loop-carried accumulators … where back-edge facts prove ownership." → Task 1 removes the `loop_depth == 0` suppression so proven (`reusable_shell`) loop-carried sites produce decisions; Tasks 2–3 cover the `.set_at` accumulator and nested shape positively and the aliased shape negatively; Task 4 Step 2 inspects the emitted decisions.
2. "Render loop proof ids near emitted decisions … carried local, update site, borrow sites, accepted/rejected reason." → Task 1 Steps 4/1 add `phase8b-loop:<func>:carry L<base>:site L<result>:depth <n>` and the `decision produced (loop-carried, carry L<base>)` reason, asserted in the producer test. **Scope note:** per-borrow-site enumeration (listing each interleaved read) is intentionally *not* implemented — the producer does not carry read sites, and the ownership fixpoint already validates borrows before setting `reusable_shell`; the emitted proof names the carried local, update site, depth, and accept/reject reason (via the render row's `reason`/`state` columns and `verdict.text`). Add borrow-site listing only if a later diagnostic need arises; it is a non-blocking nice-to-have, not required for the emission slice.

**Placeholder scan:** No TBD/TODO/"handle edge cases" placeholders; every code step shows exact code and every command shows expected output.

**Type consistency:** `MutableDecision` fields (`site`, `family`, `source_local`, `result_local`, `arg_count`, `base_arg_index`, `persistent_func`, `mutable_func`, `variant_key`, `field_path_key`, `proof_debug_id`) match `mutable_select.tw:25-37`. `by_site: Dict<Int, Vector<MutableDecision>>` matches `mutable_select.tw:39`, so the Task 1 test iterates `for _key, ds in produced.table.by_site { for d in ds { ... } }`. `Candidate` fields (`func`, `func_id`, `base`, `result`, `loop_depth`, `arg_count`, `base_arg_index`, `persistent_func`, `mutable_func`, `family`) match `mutable_produce.tw:50-61`. `sound_uniqueness_fixtures_dir()` and `wat_has_call`/`compile_fixture_to_linked_wat` are the existing codegen_emit_suite helpers.

**Note if `Vector.join` is unavailable:** the Task 1 proof-id assertion uses `proof_ids.join(",")`. If the boot prelude does not expose `Vector<String>.join`, replace the join+`str_contains` with a manual scan:

```tw
        found := false
        for _key, ds in produced.table.by_site {
          for d in ds {
            if d.proof_debug_id.contains("phase8b-loop") and d.proof_debug_id.contains("carry L") {
              found = true
            }
          }
        }
        try assert.is_true(found)
```
