# Sound Uniqueness Phase 8D: Dict.set Emission Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution order:** Execute **after Phase 8B** (`sound-uniqueness-phase8b-loop-carried-vector-set.md`). This plan generalizes the vector-set decision producer that 8B modifies. Phase 8E (`Dict.remove`) executes after this plan and depends on the producer generalization landed here.

**Goal:** Emit the existing `dict$set_in_place` helper for proven-owned `Dict.set` sites (straight-line and loop-carried), while every aliased, absent, stale, or unsupported dict-set site keeps the persistent `dict$set` path.

**Architecture:** Like 8A/8B, this is a producer-and-policy unlock, not new analysis. The ownership analysis already proves dict-backing uniqueness — `twk ir --census --sites` already reports `would_use=true` / `base=reuse(unique)` and mutable target `dict$set_in_place` for owned `Dict.set` sites, and `would_use=false` / `persistent(aliased shell)` for aliased ones. Two things currently suppress emission: (1) the decision **producer** (`mutable_produce.tw`) only collects `VectorSet` candidate sites, so dict sites never get a `MutableDecision`; (2) the build **policy** (`phase8a_policy`) enables only `emit_vector_set`. This plan generalizes the producer to collect every supported call-swap family (`VectorSet`/`DictSet`/`DictRemove`) and introduces a cumulative build policy that enables `emit_dict_set`. The catalog (`dict$set` → `dict$set_in_place`, base arg 0), the backend selector (`is_supported_call_family` already accepts `DictSet`), `policy_allows_call`, and the runtime `dict$set_in_place` helper are all already in place. `DictRemove` candidates are collected here but stay persistent (policy off) until Phase 8E.

**Tech Stack:** Twinkle boot compiler (`boot/`), `target/twk`, decision producer (`compiler.codegen.mutable_produce`), backend selector/policy (`compiler.codegen.mutable_select`), catalog (`compiler.codegen.mutable_catalog`), backend wiring (`compiler.codegen.codegen`, `boot/commands/ir.tw`), WAT/call inspection via `target/twk build ... -o /tmp/file.wat` and `target/twk wat ... --func ... --calls`, boot suite `target/twk run boot/tests/main.tw`, census `cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture`, self-host `make bundle-cli`, lint `target/twk lint boot/main.tw`.

## Global Constraints

- Boot compiler is the primary implementation; no Rust stage0 change (stage0 keeps the always-sound persistent path).
- Codegen consumes decisions; it does **not** re-prove ownership. Soundness is delegated entirely to the existing `reusable_shell` verdict — aliased dicts stay persistent because that flag is already `false` for them (verified). Do **not** add dict-specific ownership logic in the producer.
- **Dict-backing ownership is not value ownership.** In-place `dict$set_in_place` mutates only the HAMT backing (spine/leaves), never the reference-typed values stored inside. Storing a shared value into an owned backing is fine; do not extend this to mutating stored values.
- Do not change any `compiler.ownership` / `compiler.cfg` / `compiler.summary` file. Census must not move (codegen-only change).
- Assert on stable text (function names, `dict$set` / `rt_dict__set` / `rt_dict__set_in_place`, `selected`, `policy_disabled`, `would_use`), never on rendered FuncIds/block ids/local values.
- After editing `.tw` files, run `target/twk fmt <changed.tw>` and `target/twk lint boot/main.tw`; lint new fixtures explicitly.
- Write WAT/CFG dumps under `/tmp/twinkle-phase8d/`, not in the repo.
- Run verification commands one at a time, never concurrently or backgrounded.

## Dev loop note

`target/twk run boot/tests/main.tw` compiles the boot test module — including `mutable_produce`, `mutable_select`, and the `codegen`/`pipeline` functions the WAT tests call — from source with the current `target/twk`. So the suite exercises your edits immediately, with **no** `make bundle-cli`. `make bundle-cli` is required only to (a) refresh the CLI so `target/twk ir --census --sites` and `target/twk run <fixture>` reflect the new build policy, and (b) prove self-host. Iterate with the suite; bundle in the final task.

---

## File Structure

- Modify `boot/compiler/codegen/mutable_select.tw`
  - Make `is_supported_call_family` `pub` (the producer reuses it).
  - Add `enabled_emit_policy()` — the single cumulative build policy (vector-set + dict-set enabled; dict-remove/record off). Keep `phase8a_policy()` and `persistent_only_policy()` unchanged as selector-test fixtures.
- Modify `boot/compiler/codegen/mutable_produce.tw`
  - Generalize the candidate walk from `VectorSet`-only to any supported call-swap family. Rename `VectorSetTarget` → `UpdateCallTarget` and `vector_set_target` → `update_call_target`, carrying the catalog's `decision_family`. Add `decision_family` to `Candidate`. In decision production, use `c.decision_family` instead of the hardcoded `OperationFamily.VectorSet`.
  - Rename the public producer API from `*_phase8a_vector_set_*` to family-neutral `*_update_call_*` names (the "vector_set" name is now wrong).
- Modify `boot/compiler/codegen/codegen.tw`
  - Call the renamed producer and pass `enabled_emit_policy()` instead of `phase8a_policy()`.
- Modify `boot/commands/ir.tw`
  - Call the renamed producer-with-artifacts and pass `enabled_emit_policy()` for `--census --sites` audit rendering.
- Modify `boot/tests/suites/mutable_produce_suite.tw`
  - Update producer call sites to the renamed API; add dict-set positive/negative producer tests.
- Modify `boot/tests/suites/codegen_emit_suite.tw`
  - Add dict-set in-place emission (positive) and aliased-dict persistence (negative) WAT tests.
- Create `boot/tests/fixtures/sound_uniqueness/phase8d_dict_set_fresh.tw` — positive: owned straight-line + loop dict set.
- Create `boot/tests/fixtures/sound_uniqueness/phase8d_dict_set_alias.tw` — negative: aliased dict set stays persistent.
- Modify `docs/plans/sound-uniqueness/codegen/README.md` and `docs/plans/sound-uniqueness/README.md` — mark Phase 8D done, advance focus.

The `backend_prepare_suite.tw:757-758` assertions (`phase8a_policy` has `emit_vector_set=true`, `emit_dict_set=false`) and the `codegen_emit_suite.tw` selector tests (`phase8a_policy` emits vector, keeps dict persistent) stay **unchanged** — they document the selector mechanism with the vector-only fixture policy, which this plan does not alter.

---

### Task 1: Generalize the decision producer to all supported call-swap families

**Files:**
- Modify: `boot/compiler/codegen/mutable_select.tw:109-116` (make `is_supported_call_family` pub)
- Modify: `boot/compiler/codegen/mutable_produce.tw`
- Modify: `boot/tests/suites/mutable_produce_suite.tw`

**Interfaces:**
- Consumes: `mutable_catalog.UpdateEntry.decision_family` (already set for `DictSet`/`DictRemove`), `mutable_select.is_supported_call_family`, `ownership_verdicts.SiteVerdict.reusable_shell` (already `true` for owned dict sites).
- Produces: a `MutableDecision` per proven-owned supported-family update site, tagged with the correct `OperationFamily`.

- [ ] **Step 1: Make the supported-family predicate public**

In `boot/compiler/codegen/mutable_select.tw`, change the signature at line 109:

```tw
fn is_supported_call_family(family: OperationFamily) Bool {
```

to:

```tw
pub fn is_supported_call_family(family: OperationFamily) Bool {
```

- [ ] **Step 2: Add a failing dict-set producer test**

In `boot/tests/suites/mutable_produce_suite.tw`, first update the existing `produce_for` helper and all references from the old names to the new ones you will introduce in this task. Do the rename mechanically:
- `mutable_produce.phase8a_vector_set_candidates` → `mutable_produce.update_call_candidates`
- `mutable_produce.produce_phase8a_vector_set_decisions` → `mutable_produce.produce_update_call_decisions`
- `mutable_produce.produce_phase8a_vector_set_decisions_full` → `mutable_produce.produce_update_call_decisions_full`
- `mutable_produce.produce_phase8a_vector_set_decisions_with_artifacts` → `mutable_produce.produce_update_call_decisions_with_artifacts`

Then add this test after the aliased-vector test:

```tw
    .test(
      "owned dict set produces a dict_set decision",
      fn() {
        produced := try produce_for("phase8d_dict_set_fresh")
        rendered := mutable_produce.render_candidate_rows(produced.rows)
        try assert.str_contains(rendered, "dict_set")
        try assert.str_contains(rendered, "candidate")
        try assert.str_contains(rendered, "decision produced")
        families: Vector<String> = []
        for _key, ds in produced.table.by_site {
          for d in ds {
            fam := case d.family {
              .DictSet => "DictSet",
              .VectorSet => "VectorSet",
              _ => "other",
            }
            families = families.append(fam)
          }
        }
        try assert.is_true(families.contains("DictSet"))
        .Ok({})
      },
    )
    .test(
      "aliased dict set is ignored, falls back to persistent",
      fn() {
        produced := try produce_for("phase8d_dict_set_alias")
        rendered := mutable_produce.render_candidate_rows(produced.rows)
        try assert.str_contains(rendered, "dict_set")
        try assert.str_contains(rendered, "ignored")
        .Ok({})
      },
    )
```

- [ ] **Step 3: Create the dict-set fixtures**

Create `boot/tests/fixtures/sound_uniqueness/phase8d_dict_set_fresh.tw`:

```tw
fn dict_fresh() Dict<Int, Int> {
  d: Dict<Int, Int> = Dict.new()
  d = d.set(1, 10)
  d = d.set(2, 20)
  d
}

fn dict_loop(n: Int) Dict<Int, Int> {
  d: Dict<Int, Int> = Dict.new()
  i := 0
  for i < n {
    d = d.set(i, i)
    i = i + 1
  }
  d
}

println(dict_fresh().len().to_string())
println(dict_loop(5).len().to_string())
```

Create `boot/tests/fixtures/sound_uniqueness/phase8d_dict_set_alias.tw`:

```tw
fn dict_alias() Dict<Int, Int> {
  d: Dict<Int, Int> = Dict.new()
  d = d.set(1, 1)
  keep := d
  d = d.set(2, 2)
  keep
}

println(dict_alias().len().to_string())
```

- [ ] **Step 4: Run the new tests and confirm they fail**

Run: `target/twk run boot/tests/main.tw 2>&1 | rg -n "owned dict set produces|aliased dict set is ignored|FAIL|fail|passed"`
Expected: FAIL — the producer still collects only `VectorSet`, so no dict decision (and no dict candidate row) is produced. (The rename in Step 2 also means the suite won't compile until Task 1 Step 6 renames the producer functions; if the suite fails to compile, that is the expected red state — proceed to implement.)

- [ ] **Step 5: Generalize the producer candidate extraction**

In `boot/compiler/codegen/mutable_produce.tw`, replace the `VectorSetTarget` type and `vector_set_target` function (lines 65 and 74-105) with a family-neutral version. Replace:

```tw
type VectorSetTarget = .{ base: LocalId, mutable_func: FuncId, family: String, base_arg_index: Int }
```

with:

```tw
type UpdateCallTarget = .{
  base: LocalId,
  mutable_func: FuncId,
  family: String,
  base_arg_index: Int,
  decision_family: mutable_select.OperationFamily,
}
```

and replace the whole `vector_set_target` function with:

```tw
// A catalog entry names an update-call candidate only when it carries a
// supported call-swap decision family (VectorSet/DictSet/DictRemove), a live
// mutable target, a base-argument position, and the actual argument at that
// position is a plain local (not a literal/global the ownership analysis
// cannot key on). The decision family rides along so the produced decision is
// tagged correctly; the emit policy — not this producer — decides which
// families actually emit their mutable target.
fn update_call_target(entry: mutable_catalog.UpdateEntry, args: Vector<Atom>) UpdateCallTarget? {
  fam := case entry.decision_family {
    .Some(f) => f,
    .None => return .None,
  }
  if !mutable_select.is_supported_call_family(fam) {
    return .None
  }
  mutable_fid := case entry.mutable_func {
    .Some(m) => m,
    .None => return .None,
  }
  idx := case entry.base_arg_index {
    .Some(i) => i,
    .None => return .None,
  }
  if idx < 0 or idx >= args.len() {
    return .None
  }
  base := case atom_local(args[idx]) {
    .Some(l) => l,
    .None => return .None,
  }
  .Some(
    UpdateCallTarget.{
      base,
      mutable_func: mutable_fid,
      family: entry.family,
      base_arg_index: idx,
      decision_family: fam,
    },
  )
}
```

- [ ] **Step 6: Carry the decision family through `Candidate` and rename the API**

In `boot/compiler/codegen/mutable_produce.tw`:

Add `decision_family` to the `Candidate` type (after `base_arg_index`, line 57):

```tw
  base_arg_index: Int,
  decision_family: mutable_select.OperationFamily,
```

Update the `walk_op` candidate construction (the `.Some(t) => candidates.append(Candidate.{ ... })` block, lines 142-153) to call the renamed helper and set the family:

```tw
        .Some(entry) => case update_call_target(entry, args) {
          .Some(t) => candidates.append(Candidate.{
            func_id,
            func,
            result,
            base: t.base,
            arg_count: args.len(),
            family: t.family,
            base_arg_index: t.base_arg_index,
            decision_family: t.decision_family,
            persistent_func: fid,
            mutable_func: t.mutable_func,
            loop_depth,
          }),
          .None => candidates,
        },
```

Rename the four public producer functions and their internal callers (keep bodies otherwise as left by Phase 8B):
- `phase8a_vector_set_candidates` → `update_call_candidates`
- `produce_phase8a_vector_set_decisions` → `produce_update_call_decisions`
- `produce_phase8a_vector_set_decisions_full` → `produce_update_call_decisions_full`
- `produce_phase8a_vector_set_decisions_with_artifacts` → `produce_update_call_decisions_with_artifacts`
- keep `produce_..._from_validated_verdicts` internal name or rename to `produce_update_call_decisions_from_validated_verdicts` (update its single caller).

In `produce_update_call_decisions_from_validated_verdicts`, change the decision's family from the hardcoded VectorSet to the candidate's family. Find (in the Phase 8B `if verdict.reusable_shell { ... }` block):

```tw
          family: mutable_select.OperationFamily.VectorSet,
```

and replace with:

```tw
          family: c.decision_family,
```

Also update the module doc header and the timing label string `[time:mutable]` may keep its name. Update the `proof_debug_id` prefixes if they embed "phase8a"/"phase8b" — keep the loop-carried `phase8b-loop:` prefix from Phase 8B (it is family-neutral) and the straight-line `phase8a:` prefix, or generalize both to `update:`/`update-loop:`; if you generalize, update the Phase 8B producer test's `str_contains("phase8b-loop")` accordingly. (Simplest: leave the prefixes as-is; they are opaque debug ids.)

- [ ] **Step 7: Update the two build-path callers to the renamed producer**

In `boot/compiler/codegen/codegen.tw:92`, change:

```tw
  produced_decisions := mutable_produce.produce_phase8a_vector_set_decisions(anf, builtins)
```

to:

```tw
  produced_decisions := mutable_produce.produce_update_call_decisions(anf, builtins)
```

In `boot/commands/ir.tw:82`, change `mutable_produce.produce_phase8a_vector_set_decisions_with_artifacts` to `mutable_produce.produce_update_call_decisions_with_artifacts`.

- [ ] **Step 8: Run the producer tests and confirm they pass**

Run: `target/twk run boot/tests/main.tw 2>&1 | rg -n "owned dict set produces|aliased dict set is ignored|owned vector|loop-carried vector set produces|FAIL|fail|passed"`
Expected: the dict producer tests pass; the vector producer tests (fresh/alias/loop) still pass — the generalized walk is a superset that still tags vector sites `VectorSet`.

- [ ] **Step 9: Format, lint, commit**

```bash
target/twk fmt boot/compiler/codegen/mutable_select.tw boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/codegen.tw boot/commands/ir.tw boot/tests/suites/mutable_produce_suite.tw boot/tests/fixtures/sound_uniqueness/phase8d_dict_set_fresh.tw boot/tests/fixtures/sound_uniqueness/phase8d_dict_set_alias.tw
target/twk lint boot/main.tw
target/twk lint boot/tests/fixtures/sound_uniqueness/phase8d_dict_set_fresh.tw
target/twk lint boot/tests/fixtures/sound_uniqueness/phase8d_dict_set_alias.tw
git add boot/compiler/codegen/mutable_select.tw boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/codegen.tw boot/commands/ir.tw boot/tests/suites/mutable_produce_suite.tw boot/tests/fixtures/sound_uniqueness/phase8d_dict_set_fresh.tw boot/tests/fixtures/sound_uniqueness/phase8d_dict_set_alias.tw
git commit -m "codegen: produce mutable decisions for all supported update-call families"
```

At this point dict-set decisions are **produced** but still emit persistently — the build policy has not enabled `emit_dict_set` yet. Task 2 enables emission.

---

### Task 2: Enable dict-set in-place emission on the build path

**Files:**
- Modify: `boot/compiler/codegen/mutable_select.tw` (add `enabled_emit_policy()`)
- Modify: `boot/compiler/codegen/codegen.tw:106`, `boot/commands/ir.tw:93`
- Modify: `boot/tests/suites/codegen_emit_suite.tw`

**Interfaces:**
- Consumes: `policy_allows_call` (already handles `DictSet`), backend selector (already supports `DictSet`), runtime `dict$set_in_place`.
- Produces: `rt_dict__set_in_place` calls for owned dict sites; persistent `rt_dict__set` for aliased ones.

- [ ] **Step 1: Add failing dict-set WAT emission tests**

In `boot/tests/suites/codegen_emit_suite.tw`, add:

```tw
fn test_link_program_emits_dict_set_in_place_for_owned_dict() Result<Void, String> {
  wat := try compile_fixture_to_linked_wat(
    "${sound_uniqueness_fixtures_dir()}/phase8d_dict_set_fresh.tw",
  )
  try assert.is_true(wat_has_call(wat, "rt_dict__set_in_place"))
  .Ok({})
}

fn test_link_program_keeps_dict_set_persistent_for_aliased_dict() Result<Void, String> {
  wat := try compile_fixture_to_linked_wat(
    "${sound_uniqueness_fixtures_dir()}/phase8d_dict_set_alias.tw",
  )
  try assert.is_true(wat_has_call(wat, "rt_dict__set"))
  try assert.is_false(wat_has_call(wat, "rt_dict__set_in_place"))
  .Ok({})
}
```

Register both in `suite()` near the vector emission tests:

```tw
    .test(
      "link_program emits dict$set_in_place for an owned dict",
      test_link_program_emits_dict_set_in_place_for_owned_dict,
    )
    .test(
      "link_program keeps dict$set persistent for an aliased dict",
      test_link_program_keeps_dict_set_persistent_for_aliased_dict,
    )
```

Note the owned fixture defines `dict$set` in its monomorphized runtime body (which contains `rt_dict__set`), so the owned test asserts only the *presence* of `rt_dict__set_in_place`; the aliased test asserts absence of the in-place call.

- [ ] **Step 2: Run the tests and confirm the owned one fails**

Run: `target/twk run boot/tests/main.tw 2>&1 | rg -n "emits dict.set_in_place|keeps dict.set persistent for an aliased|FAIL|fail|passed"`
Expected: the owned dict test FAILS (build policy still keeps dict persistent); the aliased test passes.

- [ ] **Step 3: Add the cumulative build policy**

In `boot/compiler/codegen/mutable_select.tw`, after `phase8a_policy()` (line 158), add:

```tw
// The families the compiler actually emits mutably on the real build path.
// This is the single cumulative policy; it grows as each family's phase lands.
// Per-phase selector tests use the fixed fixtures (persistent_only_policy /
// phase8a_policy), not this cumulative one, so enabling a family here does not
// perturb those mechanism tests.
pub fn enabled_emit_policy() MutableEmitPolicy {
  MutableEmitPolicy.{
    emit_vector_set: true,
    emit_dict_set: true,
    emit_dict_remove: false,
    emit_record_shell_update: false,
  }
}
```

- [ ] **Step 4: Switch the build path to the cumulative policy**

In `boot/compiler/codegen/codegen.tw:106`, change `mutable_select.phase8a_policy()` to `mutable_select.enabled_emit_policy()`.

In `boot/commands/ir.tw:93`, change `mutable_select.phase8a_policy()` to `mutable_select.enabled_emit_policy()`.

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `target/twk run boot/tests/main.tw 2>&1 | rg -n "emits dict.set_in_place|keeps dict.set persistent for an aliased|FAIL|fail|passed"`
Expected: both pass. Then run the full suite: `target/twk run boot/tests/main.tw 2>&1 | rg -n "FAIL|fail|passed"` — no failures. In particular the vector emission tests (fresh/loop/nested/alias) and the `phase8a_policy` selector tests (`emits vector`, `keeps Dict.set persistent`) still pass, because `phase8a_policy()` is unchanged.

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/codegen/mutable_select.tw boot/compiler/codegen/codegen.tw boot/commands/ir.tw boot/tests/suites/codegen_emit_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/codegen/mutable_select.tw boot/compiler/codegen/codegen.tw boot/commands/ir.tw boot/tests/suites/codegen_emit_suite.tw
git commit -m "codegen: emit in-place dict set for owned dictionaries"
```

---

### Task 3: Self-host, runtime correctness, census, docs

**Files:**
- Create: `boot/tests/fixtures/sound_uniqueness/phase8d_dict_roundtrip.tw`
- Modify: `docs/plans/sound-uniqueness/codegen/README.md`, `docs/plans/sound-uniqueness/README.md`

**Interfaces:**
- Consumes: dict-set emission from Tasks 1-2.
- Produces: end-to-end proof that owned in-place dict set is behavior-preserving (contents + insertion order), a self-hosted CLI, and updated docs.

- [ ] **Step 1: Rebuild the self-hosted CLI (correctness gate)**

```bash
make bundle-cli
```

Expected: self-host reaches a fixed point. This is the strongest correctness signal: the boot compiler uses dicts pervasively, so owned dict sites inside the compiler now emit `dict$set_in_place`; if that helper were wrong, the compiler would miscompile itself and self-host would diverge or the suite would break. If self-host diverges, stop and investigate `dict$set_in_place` before continuing.

- [ ] **Step 2: Add and run an owned-dict round-trip correctness fixture**

Create `boot/tests/fixtures/sound_uniqueness/phase8d_dict_roundtrip.tw`:

```tw
fn build() Dict<String, Int> {
  d: Dict<String, Int> = Dict.new()
  d = d.set("a", 1)
  d = d.set("b", 2)
  d = d.set("c", 3)
  d = d.set("b", 20)
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
}

main()
```

Run it with the rebuilt CLI and check output:

```bash
target/twk run boot/tests/fixtures/sound_uniqueness/phase8d_dict_roundtrip.tw
```

Expected exactly:

```
a=1;b=20;c=3;
3
```

This confirms in-place dict set preserves values, key update semantics, and insertion order (b keeps its original position after the value update). If the order or values differ, `dict$set_in_place` is not order/semantics preserving — stop and fix the helper (or, if the helper cannot be trusted, disable `emit_dict_set` in `enabled_emit_policy` and treat dict-set as blocked pending a helper fix).

Verify it against the persistent baseline by confirming the same program produces the same output when compiled with dict emission off (temporarily set `emit_dict_set: false`, `target/twk run` — but do **not** commit that change): output must be identical.

- [ ] **Step 3: Inspect the census sites through the rebuilt CLI**

```bash
mkdir -p /tmp/twinkle-phase8d
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8d_dict_set_fresh.tw > /tmp/twinkle-phase8d/fresh.txt
target/twk ir --census --sites boot/tests/fixtures/sound_uniqueness/phase8d_dict_set_alias.tw > /tmp/twinkle-phase8d/alias.txt
rg -n "dict_set|would_use|selected|policy_disabled|absent_fallback|dict.set_in_place" /tmp/twinkle-phase8d/fresh.txt /tmp/twinkle-phase8d/alias.txt
```

Expected: `fresh.txt` audit shows `selected` with `dict$set_in_place` for the owned sites; `alias.txt` shows persistent/`would_use=false` and no `selected` in-place decision. Any `dict_remove` sites present show `policy_disabled` (Phase 8E territory).

- [ ] **Step 4: Confirm the ownership census is unchanged**

```bash
cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture 2>&1 | rg -n "census|baseline|total|FAIL|ok"
```

Expected: census total unchanged from baseline (codegen-only change). If it moved, an analysis file was touched — revert that.

- [ ] **Step 5: Update the docs**

In `docs/plans/sound-uniqueness/codegen/README.md`, check the Phase 8D boxes:

```markdown
## Codegen Phase 8D — Dict set emission ✅ done

- [x] **Lower proven-owned `Dict.set` through existing in-place helpers.** The
  generalized decision producer collects dict-set sites, and `enabled_emit_policy`
  turns on `dict$set_in_place` emission for owned sites; key lookup and
  old-version observability are preserved (aliased dicts stay persistent).
- [x] **Keep nested value ownership conservative.** In-place set mutates only the
  HAMT backing, never reference-typed values stored inside the dict.
- [x] **Inspect dict helper selection.** `twk ir --census --sites` shows
  `selected` / `dict$set_in_place` for owned sites and persistent for aliased
  ones; a round-trip fixture confirms value/order preservation.
```

Update the file's top status line and "Current focus" to Phase 8E (Dict remove). Note that the decision producer is now family-neutral (`produce_update_call_decisions`), covering vector-set and dict-set, and that `enabled_emit_policy` is the cumulative build policy.

In `docs/plans/sound-uniqueness/README.md`, advance the "Current implementation focus" paragraph and the Codegen-track summary from Phase 8D to Phase 8E.

- [ ] **Step 6: Final verification and commit docs**

```bash
target/twk run boot/tests/main.tw
target/twk lint boot/main.tw
git status --short
find . -path './.git' -prune -o -name '*.wat' -print
git add boot/tests/fixtures/sound_uniqueness/phase8d_dict_roundtrip.tw docs/plans/sound-uniqueness/codegen/README.md docs/plans/sound-uniqueness/README.md
git commit -m "docs+test: mark dict-set in-place emission complete with round-trip guard"
```

Expected: suite fully green, lint clean, no WAT dumps in the repo.

Final report should state: owned straight-line and loop dict sets emit `dict$set_in_place`; aliased dicts stay persistent; round-trip output preserves values/order; self-host fixpoint reached; census unchanged; `DictRemove` remains produced-but-policy-disabled pending Phase 8E.

---

## Self-Review

**Spec coverage** (codegen README Phase 8D, three items): "Lower proven-owned `Dict.set` through existing in-place helpers" → Tasks 1-2; "Keep nested value ownership conservative" → constraint + the backing-only nature of `dict$set_in_place`, guarded by the round-trip fixture in Task 3 Step 2; "Inspect dict helper selection" → Task 3 Step 3 census audit + WAT tests in Task 2.

**Placeholder scan:** No TBD/TODO/"handle edge cases"; every code step has exact code, every command has expected output.

**Type consistency:** `Candidate` gains `decision_family: mutable_select.OperationFamily`, set from `UpdateCallTarget.decision_family`, consumed as `c.decision_family` in the `MutableDecision.family` field (`mutable_select.tw:27`). `UpdateEntry.decision_family` is `OperationFamily?` (`mutable_catalog.tw:16`), matched with `case ... { .Some(f) => ..., .None => ... }`. `is_supported_call_family` becomes `pub` and is called as `mutable_select.is_supported_call_family(fam)`. `enabled_emit_policy()` returns `MutableEmitPolicy` with the four existing fields (`mutable_select.tw:129-136`). Producer rename is applied consistently across `mutable_produce.tw`, `codegen.tw`, `commands/ir.tw`, and `mutable_produce_suite.tw`.

**Cross-plan consistency:** This plan modifies the producer *after* Phase 8B lands, keeping 8B's loop-carried gate lift and `phase8b-loop:` proof id (family-neutral). The `by_site: Dict<Int, Vector<MutableDecision>>` iteration (`for _key, ds in ... { for d in ds { ... } }`) matches the 8B plan's helper shape. If `Vector<String>.contains` is unavailable for the `families.contains("DictSet")` check, replace with a manual `found` scan as in the 8B plan's fallback note.
