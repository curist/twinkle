# Loop-Carried / Threaded Field Ownership Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. This plan is **investigation-led**: Task 1 pins the exact proof gap before any fix is written. Do not skip it.

**Goal:** Prove a loop-carried (and simple threaded) record's reference-typed field backing uniquely owned, so a record-backed collection update on that field lowers in place across loop iterations — not just on a straight-line first use.

**Architecture:** A single field-backed update on a freshly-built record already lowers in place (the full-tier variant `[unique:p0,p0.f0]` is proven at the caller and routed). The gap is loop-carried state: for `for i { s = s.insert(i) }`, the caller proves only the **shell** tier `[unique:p0]`, so `insert`/`remove` route to the shell clone and stay persistent. The ownership fixpoint's loop machinery seeds and joins **whole-value** (shell) uniqueness across back-edges but not reference-typed **field paths**: `join_entry_field_own` keeps a joined field map only when the shell is Unique, and `collect_loop_seed_candidates` seeds shell ownership only. This plan extends loop-carried ownership to preserve depth-one field paths across the back-edge, soundly (never over a shared or escaping field backing).

**Tech Stack:** Twinkle boot compiler (`boot/`), self-hosted. Build via `make quick-bundle-cli`; boot tests via `target/twk run boot/tests/main.tw`; inspect with `target/twk ir <fixture> --census --sites` and `target/twk wat <fixture> --func <fn> --calls`. No Rust stage0 changes.

**Design source:** `docs/plans/sound-uniqueness/codegen/README.md` §"Codegen Phase 8H"; `docs/plans/sound-uniqueness/analysis/records-fields.md`; the as-built ownership fixpoint in `boot/compiler/ownership.tw`.

---

## Global Constraints

- **Soundness first.** A loop-carried field may be proven owned **only** when every back-edge predecessor exits with that field genuinely owned and the shell is Unique across the loop. A shared, aliased, or escaping field backing must stay persistent. The existing negative fixtures (`field_dict_alias_old`, `field_vector_alias_old`, `red_*`, `visit_aliased`) must remain persistent and keep their old-handle observability.
- Field-path claims stay **depth-one** (`[.f]`), matching `call_arg_paths` and `VariantId`. No `Elem`/`Val`/payload paths.
- No new operation families, no runtime-helper ABI changes, no source-semantics changes.
- Byte-identical output for every fixture with no loop-carried/threaded reference-typed field update (verify against the `sound_uniqueness` WAT set).
- After editing `.tw` files: `target/twk fmt <files>` then `target/twk lint boot/main.tw`.
- Do not run tree-sitter tests. Every task that changes analysis must pass `make stage2` (self-host fixed point) before it is considered done.

## Orientation — evidence and mechanism

Probe (`for i in range(5) { s = s.insert(i) }; s = s.remove(2)`), census today:

```text
f298 -> f301 "insert__Int$v301" [f298|0:] sites=1 rec=0 routed     # SHELL tier only
f299 -> f302 "remove__Int$v302" [f299|0:] sites=1 rec=0 routed     # SHELL tier only
insert__Int$v301 ... field=persistent(insufficient deep ownership) # not in place
```

Contrast: `field_set_wrapper.tw` (single `seen = seen.insert(1)` on a fresh Set) proves the **full** tier `[unique:p0,p0.f0]` and its clone emits `rt_dict__set_in_place`. So `insert`'s return **does** propagate `.entries` field ownership on a straight-line use; the loss happens specifically across the **loop back-edge**.

Relevant code (`boot/compiler/ownership.tw`):

- `call_arg_paths` (`ownership.tw:6763`) — derives a call's per-argument `PathSet` from `pre.atom_field_own(arg)`. If the caller's field ownership of `s` includes `[.entries]` at the call, the full tier is selected. This is the consumer; it does not need to change.
- `join_entry_field_own` (`ownership.tw:5478`) — at a block entry, joins predecessors' exit field maps for each live local, but **only keeps the map when the joined shell is Unique** ("downward-closed"). At a loop header the shell may be Unknown/optimistically-seeded, and the field map is dropped.
- `collect_loop_seed_candidates` (`ownership.tw:5127`) + `loop_seed_active` (`5073`) — optimistically seed **shell** ownership for loop-header locals; consulted in the `.Unknown` ownership case (`5187`). There is no field-path analogue.
- `run_fixpoint_validated` (`ownership.tw:6400`) drives the seeded fixpoint; `stabilize_seeds` / `validate_loop_seed` (`5893`) / `retain_valid_loop_seeds` (`5917`) grow and validate the optimistic shell seeds.

**Working hypothesis (to be confirmed in Task 1):** the loop header joins `s` with shell Unique (via the shell loop-seed) but `join_entry_field_own` still drops `[.entries]` because either (a) the back-edge exit field map for `s` is empty, or (b) the field map is present but discarded by a shell/field ordering issue in the join. The fix likely extends the loop-seed/join to carry a **validated** field path across the back-edge, gated by the same downward-closed shell-Unique rule.

## File Structure

- **Create** fixtures under `boot/tests/fixtures/cfg/sound_uniqueness/`:
  - `loop_field_dict_update.tw` — loop-carried record-field dict update (the general case, no Set).
  - `loop_field_dict_alias.tw` — same shape but the field backing is aliased before the loop (negative; must stay persistent).
  - `loop_set_insert.tw` — loop-carried `Set.insert` (the Set wrapper case).
- **Modify** `boot/compiler/ownership.tw` — extend loop-carried ownership to preserve depth-one field paths across back-edges (exact functions determined by Task 1).
- **Create** `boot/tests/suites/loop_field_ownership_suite.tw` — positive/negative census + WAT tests.
- **Modify** `boot/tests/main.tw` — register the suite.

---

## Task 1: Investigation — pin the exact point field ownership is lost

**Files:** Create `boot/tests/fixtures/cfg/sound_uniqueness/loop_field_dict_update.tw` (used as the probe). No production changes.

- [ ] **Step 1: Create the general (non-Set) loop fixture.** `loop_field_dict_update.tw`:

```twinkle
pub type Env = .{ types: Dict<Int, Int>, spare: Dict<Int, Int> }

pub fn go() Int {
  env := Env.{ types: Dict.new(), spare: Dict.new() }
  for i in range(5) {
    env.types[i] = i * 10
  }
  case env.types.get(3) {
    .Some(v) => v,
    .None => 0,
  }
}

println(go().to_string())
```

Runtime output is `30`. This is the pure lever — a record field, no prelude wrapper.

- [ ] **Step 2: Confirm the gap reproduces without Set.**

```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/loop_field_dict_update.tw --census --sites | grep -E "record_backed|field=|in_place"
target/twk wat boot/tests/fixtures/cfg/sound_uniqueness/loop_field_dict_update.tw --func go --calls | grep -i in_place
```

Expected: the quartet is recognized (`record_backed_dict`) but the decision is `ignored ... field=persistent(insufficient deep ownership)` and `go` emits the persistent `rt_dict__set`, not in place. (If it already emits in place, the Set-only probe was a wrapper artifact — record that and narrow the plan to the Set path.)

- [ ] **Step 3: Instrument the join.** Add a temporary `eprintln` in `join_entry_field_own` (`ownership.tw:5478`) that, for the loop-header block and the loop-carried local, prints: the joined `shell_unique` bool, whether each back-edge predecessor `is_processed`, and the `src` field map (`src.is_empty()`). Rebuild and read:

```bash
make quick-bundle-cli
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/loop_field_dict_update.tw --cfg 2>&1 | grep -i "joindbg"
```

- [ ] **Step 4: Classify the cause and record the decision.**
  - **Cause A — back-edge exit field map is empty:** the loop body's exit does not carry `[.types]` owned (the in-place update result's field ownership is not surviving to the block exit / back-edge). Fix target: the transfer/materialization that produces the back-edge exit field_own.
  - **Cause B — shell not Unique at the header when the field map is present:** the shell loop-seed isn't marking `s`/`env` Unique at the header at the moment the field map would be kept. Fix target: order/interaction of the shell loop-seed with `join_entry_field_own`.
  - **Cause C — a field-specific loop seed is required:** the optimistic seed set (`collect_loop_seed_candidates`) must gain a field-path analogue that `validate_loop_seed`/`retain_valid_loop_seeds` grow and prune exactly like the shell seed. Fix target: the loop-seed machinery.

- [ ] **Step 5: Remove the instrumentation and write the finding into Task 2.** Commit only the fixture:

```bash
git add boot/tests/fixtures/cfg/sound_uniqueness/loop_field_dict_update.tw
git commit -m "test(loop-field): add loop-carried record-field dict fixture and record proof-gap finding

Cause <A|B|C>: <one-line summary of where field ownership is lost across the back-edge>."
```

---

## Task 2: Failing tests — loop-carried field update should lower in place; aliased must not

**Files:** Create `boot/tests/fixtures/cfg/sound_uniqueness/loop_field_dict_alias.tw`, `boot/tests/fixtures/cfg/sound_uniqueness/loop_set_insert.tw`; Create `boot/tests/suites/loop_field_ownership_suite.tw`; Modify `boot/tests/main.tw`.

- [ ] **Step 1: Create the negative (aliased) fixture.** `loop_field_dict_alias.tw`:

```twinkle
pub type Env = .{ types: Dict<Int, Int>, spare: Dict<Int, Int> }

pub fn go() Bool {
  shared := Dict.new().set(0, 0)
  env := Env.{ types: shared, spare: shared }
  for i in range(3) {
    env.types[i] = i * 10
  }
  shared.has(0)
}

println(go().to_string())
```

`types` and `spare` alias `shared`, so the loop must NOT mutate `types` in place. Runtime output is `true`.

- [ ] **Step 2: Create the Set loop fixture.** `loop_set_insert.tw`:

```twinkle
pub fn go() Int {
  s: Set<Int> = Set.new()
  for i in range(5) {
    s = s.insert(i)
  }
  s.len()
}

println(go().to_string())
```

Runtime output is `5`.

- [ ] **Step 3: Write the suite (positive in-place, negative persistent).** Create `loop_field_ownership_suite.tw` (mirror helpers from `field_backed_collection_suite.tw`: `compile_fixture_wat`, `wat_func_body_result`, `wat_has_instr`, `ir_sites_text`):

```twinkle
.test(
  "loop-carried owned record-field dict update lowers in place",
  fn() Result<Void, String> {
    body := try wat_func_body_result(try compile_fixture_wat("loop_field_dict_update"), "go")
    try assert.ok(wat_has_instr(body, "rt_dict__set_in_place"), "owned loop field update must set in place")
    .Ok({})
  },
)
.test(
  "aliased loop-carried field update stays persistent",
  fn() Result<Void, String> {
    body := try wat_func_body_result(try compile_fixture_wat("loop_field_dict_alias"), "go")
    try assert.ok(wat_has_instr(body, "rt_dict__set"), "aliased loop field must use persistent set")
    try assert.ok(!wat_has_instr(body, "rt_dict__set_in_place"), "aliased loop field must not mutate")
    .Ok({})
  },
)
.test(
  "loop-carried Set.insert reaches the full-tier clone and sets in place",
  fn() Result<Void, String> {
    sites := ir_sites_text("loop_set_insert")
    try assert.str_contains(sites, "|0:;0:0")  // full tier published/routed
    .Ok({})
  },
)
```

- [ ] **Step 4: Register and run red.** Add the suite to `boot/tests/main.tw`, then:

```bash
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: the positive dict test and the Set full-tier test FAIL (persistent today); the aliased-negative test PASSES already (correctly persistent).

- [ ] **Step 5: Commit the red tests.**

```bash
git add boot/tests/fixtures/cfg/sound_uniqueness/loop_field_dict_alias.tw boot/tests/fixtures/cfg/sound_uniqueness/loop_set_insert.tw boot/tests/suites/loop_field_ownership_suite.tw boot/tests/main.tw
git commit -m "test(loop-field): red tests for loop-carried field in-place + aliased persistence"
```

---

## Task 3: Extend loop-carried ownership to preserve depth-one field paths

**Files:** Modify `boot/compiler/ownership.tw`. Exact functions per Task 1's Cause A/B/C.

The implementation is written after Task 1 pins the cause; the following are the concrete shapes for each cause. Implement only the one Task 1 identified (or the minimal combination it shows).

- [ ] **Step 1 (Cause A — back-edge exit field map empty): carry the in-place update result's field ownership to the block exit.** In the forward transfer for the field-backed `ARecordUpdate` result, ensure the rebuilt record's `[.f]` field_own is set Unique when the update was proven owned, so it survives to `blk.exit` and thus to `join_entry_field_own` on the back-edge. Verify via the Task 1 instrumentation that the back-edge `src` field map is now non-empty.

- [ ] **Step 2 (Cause B — join ordering): keep the joined field map when the loop-seeded shell is Unique.** In `join_entry_field_own` (`ownership.tw:5478`), make the `shell_unique` check consult the same optimistic loop-seed the shell fixpoint uses (`loop_seed_active`) for the header block, so a field map validated on the back-edge is retained under the identical soundness gate as the shell.

- [ ] **Step 3 (Cause C — field loop seed): add a validated field-path loop seed.** Give the loop-seed machinery a depth-one field-path analogue: seed `[.f]` optimistically at a loop header for a field whose back-edge predecessor exits it owned, and extend `validate_loop_seed`/`retain_valid_loop_seeds` to **retract** the field seed whenever the shell seed is retracted or the field is not genuinely owned on the back-edge. The field seed must never outlive its shell seed (downward-closed).

- [ ] **Step 2/3 shared invariant.** Whatever the cause, the retained field path must be justified by the back-edge, not assumed: an aliased field backing (`loop_field_dict_alias`) exits the loop body with `[.types]` **not** owned, so the join/seed must drop it and the update stays persistent.

- [ ] **Step 4: Format, lint, build, run the Task 2 tests green.**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: the positive dict + Set tests pass; the aliased-negative test still passes; all other suites green.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/ownership.tw
git commit -m "analysis(loop-field): preserve depth-one field ownership across loop back-edges

Cause <A|B|C> fix: <one line>. Aliased/shared field backings stay persistent."
```

---

## Task 4: Soundness sweep, byte-identical fallback, and self-host gate

**Files:** Modify `boot/tests/suites/loop_field_ownership_suite.tw` (runtime-parity assertions).

- [ ] **Step 1: Runtime correctness for positives and negatives.**

```bash
target/twk run boot/tests/fixtures/cfg/sound_uniqueness/loop_field_dict_update.tw   # expect 30
target/twk run boot/tests/fixtures/cfg/sound_uniqueness/loop_field_dict_alias.tw    # expect true
target/twk run boot/tests/fixtures/cfg/sound_uniqueness/loop_set_insert.tw          # expect 5
```

Specialize on/off parity for each:

```bash
for f in loop_field_dict_update loop_field_dict_alias loop_set_insert; do
  a=$(target/twk run boot/tests/fixtures/cfg/sound_uniqueness/$f.tw)
  b=$(TWINKLE_VARIANT_SPECIALIZE=0 target/twk run boot/tests/fixtures/cfg/sound_uniqueness/$f.tw)
  echo "$f: on=$a off=$b"; test "$a" = "$b" || echo "  PARITY FAIL"
done
```

Expected: outputs `30` / `true` / `5`; on == off for all three.

- [ ] **Step 2: Existing negative fixtures still persistent.** Confirm no aliasing regression: re-run the scoped helper inspection from the 8I gate over `field_dict_alias_old`, `field_vector_alias_old`, `red_delegate_read_after`, `red_transport_read_after`, `visit_aliased` and confirm each user function still emits the persistent helper (no new `set_in_place`).

- [ ] **Step 3: Byte-identical fallback.** Baseline the `sound_uniqueness` fixture WAT set before Task 1; after Task 3, only fixtures with a loop-carried/threaded owned field update (`loop_field_dict_update`, `loop_set_insert`) may change. Every other fixture's WAT must be byte-identical.

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
git add boot/tests/suites/loop_field_ownership_suite.tw
git commit -m "test(loop-field): runtime parity, negative-aliasing sweep, byte-identical fallback"
```

Then in `docs/plans/sound-uniqueness/codegen/README.md`, correct the Set entry in the 8H "Deferred" list (the fresh-Set case already lowers in place; loop-carried Set now does too) and note loop-carried field ownership landed. Remove this plan's row from `docs/plans/README.md` and move this file to `docs/plans/archive/`.

---

## Prerequisite doc correction (do first, independently)

Before Task 1, correct two statements that current evidence falsifies (they were written from inspecting the caller function instead of the routed clone):

- `docs/plans/sound-uniqueness/codegen/README.md` — the 8H "Deferred" note says `Set` "in-place emission is not yet achieved." Reality: a fresh Set's `insert`/`remove` already routes to a full-tier clone that emits `rt_dict__set_in_place`. Reword to: recognized *and lowered in place for straight-line owned use*; only loop-carried/threaded Sets stay persistent (this plan).
- `docs/plans/sound-uniqueness/README.md` and the committed 8I note ("Set … still emit persistently") — same correction.

Commit this doc fix separately (`docs(8H/8I): correct Set in-place status`) so the plan's history starts from an accurate baseline.

## Scope Boundary

Delivers loop-carried and simple threaded depth-one field ownership. It does **not** deliver:

- Path-level consume-dead with post-call sibling reads (the `field_transport_ctx` transport shape) — that remains a separate deferral.
- `Elem`/`Val` nested-value ownership (dict/vector element values).
- Cross-function field ownership beyond the depth-one return-path already proven by summaries.
- Recursive-clone self-routing (that is the field-tier recursive self-routing plan).

## Self-Review Checklist

1. **Spec coverage:** investigation (Task 1), red positive+negative tests (Task 2), the cause-specific fix (Task 3), soundness sweep + byte-identical + self-host (Task 4), plus the prerequisite doc correction. ✓
2. **Investigation-led, no guessing:** Task 3's fix is selected by Task 1's observed cause (A/B/C), not assumed. ✓
3. **Soundness anchor:** every retained field path is justified by a back-edge that exits the field owned under the downward-closed shell-Unique rule; `loop_field_dict_alias` is the guard that this holds. ✓
4. **Type consistency:** depth-one `[.f]` paths, `join_entry_field_own`, `loop_seed_active`, `call_arg_paths`, and full-tier `|0:;0:0` keys are used consistently across tasks. ✓
5. **Fallback safety:** byte-identical for all non-loop-field fixtures; existing negative-aliasing fixtures re-swept in Task 4 Step 2. ✓
