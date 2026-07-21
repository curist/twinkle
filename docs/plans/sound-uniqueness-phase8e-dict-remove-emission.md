# Sound Uniqueness Phase 8E: Dict.remove Emission Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution order:** Execute **after Phase 8D** (`sound-uniqueness-phase8d-dict-set-emission.md`). 8D already generalized the decision producer to collect every supported call-swap family, so `DictRemove` candidates are **already produced** — they only fall back to persistent because `enabled_emit_policy` keeps `emit_dict_remove: false`. This plan is deliberately small: flip that flag and prove `Dict.remove` in-place preserves remove semantics.

**Goal:** Emit the existing `dict$remove_in_place` helper for proven-owned `Dict.remove` sites, while aliased/absent/stale sites keep the persistent `dict$remove` path, preserving insertion-order iteration, missing-key behavior, and old-version observability.

**Architecture:** `Dict.remove` reuses the same producer/selector/policy machinery as `Dict.set`, but has distinct helper semantics, so it gets its own phase. After 8D, the only suppression left is the policy flag. The analysis already proves ownership for remove sites — `twk ir --census --sites` reports `would_use=true` / `base=reuse(unique)` and mutable target `dict$remove_in_place` for owned removes and `would_use=false` / `persistent(aliased shell)` for aliased ones. The risk in this phase is **not** ownership — it is whether `dict$remove_in_place` is behavior-identical to persistent `dict$remove` (order, missing key, tombstone/compaction). This plan enables emission and verifies that behavior end-to-end.

**Tech Stack:** Twinkle boot compiler (`boot/`), `target/twk`, `compiler.codegen.mutable_select` (policy), catalog (`dict$remove` → `dict$remove_in_place`), runtime dict helper (`rt.dict`), WAT/call inspection, boot suite `target/twk run boot/tests/main.tw`, census `cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture`, self-host `make bundle-cli`, lint `target/twk lint boot/main.tw`.

## Global Constraints

- Boot compiler only; no Rust stage0 change (stage0 keeps the persistent path).
- Codegen consumes decisions; no ownership re-proof. Aliased removes stay persistent via the existing `reusable_shell=false` (verified). Do not add remove-specific ownership logic in the producer.
- **Dict-backing ownership is not value ownership.** `dict$remove_in_place` mutates only the backing; it must not touch reference-typed stored values.
- The correctness obligation for this phase is **semantic equivalence to persistent `dict$remove`**: same remaining keys, same insertion order among survivors, same behavior removing an absent key, and no corruption of the returned dict.
- Do not touch `compiler.ownership` / `compiler.cfg` / `compiler.summary`. Census must not move.
- Assert on stable text (`dict$remove`, `rt_dict__remove`, `rt_dict__remove_in_place`, `selected`, `would_use`), never on FuncIds/local values.
- After editing `.tw` files: `target/twk fmt`, `target/twk lint boot/main.tw`, lint new fixtures explicitly.
- WAT/CFG dumps under `/tmp/twinkle-phase8e/`. Run verification one command at a time.

## Dev loop note

`target/twk run boot/tests/main.tw` compiles the boot test module (incl. compiler modules) from source, so WAT-level tests see edits immediately without `make bundle-cli`. Bundle only to refresh the CLI (for `--census --sites` and `target/twk run <fixture>` reflecting the new policy) and to prove self-host, in the final task.

---

## File Structure

- Modify `boot/compiler/codegen/mutable_select.tw` — flip `emit_dict_remove` to `true` in `enabled_emit_policy()` (the cumulative build policy added in 8D). `phase8a_policy()` / `persistent_only_policy()` stay unchanged.
- Modify `boot/tests/suites/codegen_emit_suite.tw` — add owned-remove in-place (positive) and aliased-remove persistence (negative) WAT tests.
- Create `boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_fresh.tw` — positive: owned straight-line + loop remove.
- Create `boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_alias.tw` — negative: aliased remove stays persistent.
- Create `boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_roundtrip.tw` — semantic equivalence check (order/missing-key/survivors).
- Modify `docs/plans/sound-uniqueness/codegen/README.md`, `docs/plans/sound-uniqueness/README.md` — mark 8E done, advance focus to Phase 8C (builder regions) or the next chosen slice.

If 8D scoped its producer walk to VectorSet+DictSet only (rather than "all supported families"), add DictRemove to `update_call_target`'s accepted families first — see Task 1 Step 0. If 8D already generalized to all supported families (recommended, and what its plan specifies), skip that step.

---

### Task 1: Enable and verify owned `Dict.remove` in-place emission

**Files:**
- Modify: `boot/compiler/codegen/mutable_select.tw` (`enabled_emit_policy`)
- Modify: `boot/tests/suites/codegen_emit_suite.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_fresh.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_alias.tw`

**Interfaces:**
- Consumes: 8D's generalized producer (DictRemove candidates already produced), `policy_allows_call` (already handles `DictRemove`), backend selector (`is_supported_call_family` already accepts `DictRemove`), runtime `dict$remove_in_place`.
- Produces: `rt_dict__remove_in_place` for owned removes; persistent `rt_dict__remove` for aliased ones.

- [ ] **Step 0 (only if 8D did not generalize to all families): collect DictRemove candidates**

Confirm the producer already collects remove sites:

```bash
target/twk run boot/tests/main.tw 2>&1 | rg -n "dict_remove"
```

If no dict-remove candidate appears in the producer path, edit `update_call_target` in `boot/compiler/codegen/mutable_produce.tw` so `DictRemove` is an accepted family (it should already be, via `mutable_select.is_supported_call_family`, which returns `true` for `DictRemove`). Only proceed once a remove site is collected as a candidate.

- [ ] **Step 1: Create the remove fixtures**

Create `boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_fresh.tw`:

```tw
fn remove_loop(n: Int) Dict<Int, Int> {
  d: Dict<Int, Int> = Dict.new()
  i := 0
  for i < n {
    d = d.set(i, i)
    i = i + 1
  }
  j := 0
  for j < n {
    d = d.remove(j)
    j = j + 1
  }
  d
}

println(remove_loop(6).len().to_string())
```

Create `boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_alias.tw`:

```tw
fn remove_alias() Dict<Int, Int> {
  d: Dict<Int, Int> = Dict.new()
  d = d.set(1, 1)
  d = d.set(2, 2)
  keep := d
  d = d.remove(1)
  keep
}

println(remove_alias().len().to_string())
```

- [ ] **Step 2: Add failing remove WAT tests**

In `boot/tests/suites/codegen_emit_suite.tw`, add:

```tw
fn test_link_program_emits_dict_remove_in_place_for_owned_dict() Result<Void, String> {
  wat := try compile_fixture_to_linked_wat(
    "${sound_uniqueness_fixtures_dir()}/phase8e_dict_remove_fresh.tw",
  )
  try assert.is_true(wat_has_call(wat, "rt_dict__remove_in_place"))
  .Ok({})
}

fn test_link_program_keeps_dict_remove_persistent_for_aliased_dict() Result<Void, String> {
  wat := try compile_fixture_to_linked_wat(
    "${sound_uniqueness_fixtures_dir()}/phase8e_dict_remove_alias.tw",
  )
  try assert.is_true(wat_has_call(wat, "rt_dict__remove"))
  try assert.is_false(wat_has_call(wat, "rt_dict__remove_in_place"))
  .Ok({})
}
```

Register both in `suite()`:

```tw
    .test(
      "link_program emits dict$remove_in_place for an owned dict",
      test_link_program_emits_dict_remove_in_place_for_owned_dict,
    )
    .test(
      "link_program keeps dict$remove persistent for an aliased dict",
      test_link_program_keeps_dict_remove_persistent_for_aliased_dict,
    )
```

The owned fixture's monomorphized `Dict.remove` body contains `rt_dict__remove`, so the owned test asserts only presence of `rt_dict__remove_in_place`.

- [ ] **Step 3: Run and confirm the owned test fails**

Run: `target/twk run boot/tests/main.tw 2>&1 | rg -n "emits dict.remove_in_place|keeps dict.remove persistent for an aliased|FAIL|fail|passed"`
Expected: owned remove test FAILS (policy still off); aliased test passes.

- [ ] **Step 4: Flip the policy flag**

In `boot/compiler/codegen/mutable_select.tw`, in `enabled_emit_policy()`, change:

```tw
    emit_dict_remove: false,
```

to:

```tw
    emit_dict_remove: true,
```

- [ ] **Step 5: Run and confirm both pass, then the full suite**

Run: `target/twk run boot/tests/main.tw 2>&1 | rg -n "emits dict.remove_in_place|keeps dict.remove persistent|FAIL|fail|passed"`
Then: `target/twk run boot/tests/main.tw 2>&1 | rg -n "FAIL|fail|passed"`
Expected: both remove tests pass; no other failures (dict-set and vector emission tests unaffected; `phase8a_policy` selector tests unchanged).

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/codegen/mutable_select.tw boot/tests/suites/codegen_emit_suite.tw boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_fresh.tw boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_alias.tw
target/twk lint boot/main.tw
target/twk lint boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_fresh.tw
target/twk lint boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_alias.tw
git add boot/compiler/codegen/mutable_select.tw boot/tests/suites/codegen_emit_suite.tw boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_fresh.tw boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_alias.tw
git commit -m "codegen: emit in-place dict remove for owned dictionaries"
```

---

### Task 2: Self-host, semantic equivalence, census, docs

**Files:**
- Create: `boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_roundtrip.tw`
- Modify: `docs/plans/sound-uniqueness/codegen/README.md`, `docs/plans/sound-uniqueness/README.md`

**Interfaces:**
- Consumes: remove emission from Task 1.
- Produces: end-to-end proof that in-place remove is behavior-identical to persistent remove, a self-hosted CLI, updated docs.

- [ ] **Step 1: Rebuild the self-hosted CLI (correctness gate)**

```bash
make bundle-cli
```

Expected: self-host fixed point. The boot compiler removes dict keys in real code (e.g. scope/env bookkeeping); owned removes now emit `dict$remove_in_place`. If the helper mis-handles order or survivors, self-host diverges or the suite breaks — stop and fix the helper (or set `emit_dict_remove: false` and treat remove as blocked pending a helper fix) before proceeding.

- [ ] **Step 2: Add and run the semantic-equivalence fixture**

Create `boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_roundtrip.tw`:

```tw
fn build() Dict<String, Int> {
  d: Dict<String, Int> = Dict.new()
  d = d.set("a", 1)
  d = d.set("b", 2)
  d = d.set("c", 3)
  d = d.set("d", 4)
  d = d.remove("b")
  d = d.remove("z")
  d
}

fn main() {
  d := build()
  line := ""
  for k, v in d {
    line = line.concat("${k}=${v};")
  }
  println(line)
  println(d.len().to_string())
  println(d.has("b").to_string())
}

main()
```

Run and check output:

```bash
target/twk run boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_roundtrip.tw
```

Expected exactly:

```
a=1;c=3;d=4;
3
false
```

This confirms: the removed key is gone, an absent-key removal (`"z"`) is a no-op, survivors keep insertion order, and `has` reflects the removal. If output differs, `dict$remove_in_place` is not equivalent to persistent remove — stop and fix the helper.

Confirm identical output with emission off (temporarily set `emit_dict_remove: false`, `target/twk run`, do **not** commit that change): the two outputs must match.

- [ ] **Step 3: Census sites inspection**

```bash
mkdir -p /tmp/twinkle-phase8e
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_fresh.tw > /tmp/twinkle-phase8e/fresh.txt
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_alias.tw > /tmp/twinkle-phase8e/alias.txt
rg -n "dict_remove|would_use|selected|absent_fallback|dict.remove_in_place" /tmp/twinkle-phase8e/fresh.txt /tmp/twinkle-phase8e/alias.txt
```

Expected: `fresh.txt` shows `selected` / `dict$remove_in_place` for owned removes; `alias.txt` shows persistent / `would_use=false`.

- [ ] **Step 4: Census unchanged**

```bash
cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture 2>&1 | rg -n "census|baseline|total|FAIL|ok"
```

Expected: unchanged from baseline (codegen-only).

- [ ] **Step 5: Update docs**

In `docs/plans/sound-uniqueness/codegen/README.md`, check the Phase 8E boxes:

```markdown
## Codegen Phase 8E — Dict remove emission ✅ done

- [x] **Catalog and verify remove helper semantics first.** A round-trip fixture
  confirms `dict$remove_in_place` preserves insertion order among survivors,
  treats absent-key removal as a no-op, and matches persistent `dict$remove`.
- [x] **Lower proven-owned `Dict.remove` through existing in-place helpers.**
  `enabled_emit_policy` enables `emit_dict_remove`; aliased/absent/stale removes
  fall back to persistent `dict$remove`.
- [x] **Keep remove inspection distinct from set.** `twk ir --census --sites`
  reports the `dict_remove` family and `dict$remove_in_place` selection
  separately from `dict_set`.
```

Update the top status line / "Current focus" to the next slice (Phase 8C builder regions, or the next chosen phase). In `docs/plans/sound-uniqueness/README.md`, advance the "Current implementation focus" paragraph and Codegen-track summary accordingly, noting both dict families (set + remove) now emit in-place for owned dictionaries.

- [ ] **Step 6: Final verification and commit docs**

```bash
target/twk run boot/tests/main.tw
target/twk lint boot/main.tw
git status --short
find . -path './.git' -prune -o -name '*.wat' -print
git add boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_roundtrip.tw docs/plans/sound-uniqueness/codegen/README.md docs/plans/sound-uniqueness/README.md
git commit -m "docs+test: mark dict-remove in-place emission complete with equivalence guard"
```

Expected: suite green, lint clean, no WAT dumps in repo.

Final report: owned removes emit `dict$remove_in_place`; aliased removes stay persistent; round-trip preserves order/survivors/missing-key/`has`; self-host fixpoint reached; census unchanged. Note remaining families: builder regions (8C), record shell (8F), function variants (8G), record-backed field collections (8H).

---

## Self-Review

**Spec coverage** (codegen README Phase 8E, three items): "Catalog and verify remove helper semantics first" → Task 2 Step 2 round-trip equivalence fixture (order/missing-key/survivors) + self-host gate; "Lower proven-owned `Dict.remove` through existing in-place helpers" → Task 1 policy flip + WAT tests; "Keep remove inspection distinct from set" → Task 2 Step 3 census audit distinguishes `dict_remove`.

**Placeholder scan:** none — every step has exact code/commands and expected output.

**Type consistency:** the only production change is `emit_dict_remove: false → true` in `enabled_emit_policy()` (the `MutableEmitPolicy` record introduced in Phase 8D, fields per `mutable_select.tw:129-136`). `policy_allows_call` (`mutable_select.tw:163-170`) and `is_supported_call_family` (`:109`) already handle `DictRemove`; no signature changes. Fixture/WAT helpers (`sound_uniqueness_fixtures_dir`, `compile_fixture_to_linked_wat`, `wat_has_call`) are the existing codegen_emit_suite helpers.

**Dependency check:** relies on Phase 8D's `enabled_emit_policy()` and generalized `update_call_candidates`/`produce_update_call_decisions` producer. Task 1 Step 0 covers the case where 8D scoped its walk more narrowly. If 8D is not yet merged, do not start this plan — `enabled_emit_policy` will not exist.
