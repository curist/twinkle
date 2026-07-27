# Copy-Carrier Dict Acceptance Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Verify that the general borrow/effect checker from `docs/plans/2026-07-24-ownership-borrow-effect-checker-plan.md` solves the motivating `merge_targeted` copy-carrier dict-set pattern without rewriting boot compiler source.

**Architecture:** This plan is a dependent acceptance slice, not an implementation plan for a second recognizer. The checker plan owns checker-level fixtures, rejection reason vocabulary, suite helpers, and mechanism tests. This slice owns only the `merge_targeted_min` source-shape preservation, focused acceptance commands, boot-main measurement, and `fixpoint-map-inplace.md` outcome note.

**Tech Stack:** Twinkle boot compiler (`boot/`), ownership analysis (`boot/compiler/ownership.tw`), mutable decision reports, boot fixtures/suites, rebuilt CLI verification.

## Global Constraints

- This plan depends on successful implementation of `docs/plans/2026-07-24-ownership-borrow-effect-checker-plan.md`.
- Do not create duplicate checker fixtures or duplicate suite helpers here; consume the checker plan's fixtures and assertions.
- **Do not rewrite the boot compiler's `merge_targeted` source to get the win.** In particular, do not change `next.keys()` to `out.keys()` or `lat_get(next, ...)` to `lat_get(out, ...)` in `boot/compiler/ownership.tw` as the implementation strategy.
- Do not globally mark `Dict.keys` as `Allocate/fresh` while runtime `keys()` may return `pd_ORDER` by reference.
- Every selected `merge_targeted_min` dict-set decision must come from the borrow/effect checker and render proof text such as `borrow-effect copy-carrier`.
- After every `.tw` edit batch, run `target/twk fmt <changed .tw files>` and `target/twk lint boot/main.tw` before committing.
- Heavy verification commands run one at a time.

---

## File Structure

- Modify `boot/tests/fixtures/sound_uniqueness/phase8d_dict_merge_targeted_min.tw` only if necessary to preserve the source-alias shape while making the key stream unique.
- Do not create or edit the checker-level negative fixtures in this plan. They are owned by `2026-07-24-ownership-borrow-effect-checker-plan.md`.
- Do not add duplicate test helpers to `mutable_produce_suite.tw` or `codegen_emit_suite.tw` in this plan. If those helpers/assertions are missing, return to the checker plan.
- Modify `docs/plans/fixpoint-map-inplace.md` only to record measured boot-main outcome.

---

## Imported Requirements From the Borrow/Effect Checker

Before this slice can pass, the checker plan must provide:

- accepted proof rendering for the positive copy-carrier pattern;
- active rejection reasons for alias escape, get-after-write, keys-after-write, non-unique-key-stream, cross-local key aliasing, unknown source use, and keys/remove conflicts;
- compositional unique-key-stream certification at call sites;
- conservative key equality: two distinct key locals are possibly equal unless proven distinct;
- default-deny conflict handling;
- publication suppression that preserves a genuinely reusable carrier instead of overriding `persistent(aliased shell)`.

---

### Task 1: Preserve the motivating fixture shape

**Files:**
- Modify: `boot/tests/fixtures/sound_uniqueness/phase8d_dict_merge_targeted_min.tw`

**Interfaces:**
- Consumes: existing positive fixture.
- Produces: a fixture that still exercises source reads after `out := next`.

- [ ] **Step 1: Confirm source reads remain source reads**

Ensure the helper still contains this shape:

```tw
  out := next
  next_locked := locked
  keys := int_keys_union(old.keys(), next.keys())
  for k in keys {
    old_x := lat_get(old, k, 0)
    next_x := lat_get(next, k, 0)
    prev_x := lat_get(prev, k, 0)
```

Do not replace `next.keys()` or `lat_get(next, ...)` with `out` reads.

- [ ] **Step 2: Make only the key stream unique if needed**

If the fixture's `int_keys_union` can emit duplicate keys, update only that helper to:

```tw
fn int_keys_union(a: Vector<Int>, b: Vector<Int>) Vector<Int> {
  out := a
  for k in b {
    if !out.contains(k) {
      out = .append(k)
    }
  }
  out
}
```

This preserves the source-alias pattern while satisfying the borrow/effect checker's compositional uniqueness requirement.

- [ ] **Step 3: Format and lint if the fixture changed**

Run:

```bash
target/twk fmt boot/tests/fixtures/sound_uniqueness/phase8d_dict_merge_targeted_min.tw
target/twk lint boot/main.tw
```

Expected: lint prints `No findings.`

---

### Task 2: Verify checker-owned fixture and suite coverage exists

**Files:**
- Read-only check: `boot/tests/fixtures/sound_uniqueness/`
- Read-only check: `boot/tests/suites/mutable_produce_suite.tw`
- Read-only check: `boot/tests/suites/codegen_emit_suite.tw`

**Interfaces:**
- Consumes: completed checker plan fixtures and assertions.
- Produces: confirmation that this slice is not duplicating checker-owned tests.

- [ ] **Step 1: Check checker fixture ownership**

Run:

```bash
for f in \
  phase8d_dict_copy_carrier_positive.tw \
  phase8d_dict_copy_carrier_helper_mediated_positive.tw \
  phase8d_dict_copy_carrier_get_after_set_negative.tw \
  phase8d_dict_copy_carrier_keys_after_set_negative.tw \
  phase8d_dict_copy_carrier_distinct_locals_negative.tw \
  phase8d_dict_copy_carrier_duplicate_helper_arg_negative.tw \
  phase8d_dict_copy_carrier_unknown_call_negative.tw \
  phase8d_dict_copy_carrier_retains_source_negative.tw \
  phase8e_dict_remove_keys_borrow_negative.tw; do
  test -f "boot/tests/fixtures/sound_uniqueness/$f" || exit 1
done
```

Expected: command exits successfully. If any fixture is missing, return to the checker plan instead of creating it here.

- [ ] **Step 2: Check reason vocabulary assertions**

Run:

```bash
rg 'get-after-write|keys-after-write|non-unique-key-stream|source-escape|unknown-source-use|copy-carrier' boot/tests/suites/mutable_produce_suite.tw
rg 'remove_in_place' boot/tests/suites/codegen_emit_suite.tw
```

Expected: the checker-owned suite assertions are present. If missing, update the checker plan implementation, not this slice.

---

### Task 3: Verify focused merge-targeted acceptance after checker implementation

**Files:**
- No source edits expected.

**Interfaces:**
- Consumes: completed checker implementation and Task 1 fixture shape.
- Produces: strict evidence that the motivating unchanged source-alias pattern selects in-place.

- [ ] **Step 1: Run source-level tests**

Run:

```bash
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

Expected: copy-carrier positive and negative checker tests pass. The boot-main fixpoint probe may still fail if broader Phase 0 routes remain.

- [ ] **Step 2: Rebuild CLI**

Run:

```bash
make bundle-cli
```

Expected: command succeeds.

- [ ] **Step 3: Strictly verify focused copy-carrier rows**

Run:

```bash
target/twk ir boot/tests/fixtures/sound_uniqueness/phase8d_dict_merge_targeted_min.tw --census --sites \
  > /tmp/merge-targeted-min-sites.txt
rg '^merge_targeted_min\t[^\t]+\tdict_set\t[^\t]+\tdict\$set_in_place\tselected\t' /tmp/merge-targeted-min-sites.txt
rg '^caller\t[^\t]+\tdict_set\t[^\t]+\tdict\$set_in_place\tselected\t' /tmp/merge-targeted-min-sites.txt
rg 'borrow-effect|copy-carrier' /tmp/merge-targeted-min-sites.txt
! rg '^(merge_targeted_min|caller)\t[^\t]+\tdict_set\t[^\t]+\t[^\t]+\t(absent_fallback|[^\t]*persistent)' /tmp/merge-targeted-min-sites.txt
```

Expected: helper and caller are selected, and proof text contains the checker proof token.

- [ ] **Step 4: Spot-check checker-owned safety negatives**

Run:

```bash
target/twk ir boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_distinct_locals_negative.tw --census --sites \
  > /tmp/distinct-locals-sites.txt
rg 'get-after-write' /tmp/distinct-locals-sites.txt
! rg '^distinct_locals_negative\t[^\t]+\tdict_set\t[^\t]+\tdict\$set_in_place\tselected\t' /tmp/distinct-locals-sites.txt

target/twk build boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_keys_borrow_negative.tw -o /tmp/remove-keys-borrow.wat
! rg 'rt_dict__remove_in_place' /tmp/remove-keys-borrow.wat
```

Expected: cross-local key-alias negative is actively rejected, and keys/remove does not emit remove-in-place.

---

### Task 4: Measure boot-main outcome and record boundary

**Files:**
- Modify: `docs/plans/fixpoint-map-inplace.md` only to record measured outcome.

**Interfaces:**
- Produces: measured result for original boot-main target.

- [ ] **Step 1: Measure boot-main census**

Run:

```bash
target/twk ir boot/main.tw --census --sites > /tmp/twinkle-sites.txt
rg -n "^run_fixpoint\t|^merge_targeted|^join_entry_ownership_assumed|^join_entry_ownership\t" /tmp/twinkle-sites.txt
```

- [ ] **Step 2: Record measured result**

If `run_fixpoint` remains persistent, append to `docs/plans/fixpoint-map-inplace.md`:

```md
### Follow-up probe: borrow/effect copy-carrier checker

The borrow/effect checker flipped the merge-targeted copy-carrier dict-set shape without rewriting boot compiler source. It did not flip `run_fixpoint`; remaining persistent rows point back to the broader publication routes already pinned here: `.get`-return read-helper family, `join_entry_*` helper summaries, and the FixState/FixResult double-embed. Do not expand the copy-carrier slice; plan the broader sound-uniqueness work separately.
```

If `run_fixpoint` flips, append:

```md
### Follow-up probe: borrow/effect copy-carrier checker

The borrow/effect checker flipped the merge-targeted copy-carrier dict-set shape without rewriting boot compiler source, and the rebuilt boot-main census also shows selected `run_fixpoint` dict-set rows. Keep the mutable-produce boot-main test and rebuilt CLI census as regression gates for this success.
```

- [ ] **Step 3: Commit acceptance slice result**

Run:

```bash
git add boot/tests/fixtures/sound_uniqueness/phase8d_dict_merge_targeted_min.tw docs/plans/fixpoint-map-inplace.md
git commit -m "tests: gate borrow-effect copy-carrier dict decisions"
```

Omit `docs/plans/fixpoint-map-inplace.md` if no measurement note was added. If `phase8d_dict_merge_targeted_min.tw` did not change, omit it too.

---

## Self-Review

- **Spec coverage:** This plan no longer duplicates checker-owned fixtures or suite helpers. It preserves the no-source-rewrite requirement and verifies the motivating copy-carrier acceptance after the general checker lands.
- **Placeholder scan:** No unresolved placeholders remain.
- **Type consistency:** Fixture names and proof tokens match the checker plan.
- **Acceptance:** Passing this slice proves the general checker handles the motivating `merge_targeted_min` source-alias pattern and records whether boot-main `run_fixpoint` also benefits.
