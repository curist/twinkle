# Typed Field Call-Result Retyping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Retype typed-return call results when they are stored into already-typed vector record fields, avoiding PVec/PVecBool verifier mismatches.

**Architecture:** Keep typed field analysis dependent on `typeable_return`, then make the routing escape classifier recognize typed record-field stores as non-escaping. Preserve stricter consumer checks by passing an empty typed-field set where only index/len consumers should be accepted.

**Tech Stack:** Twinkle boot compiler (`boot/compiler/backend/*.tw`), boot test suites (`boot/tests/suites/*.tw`), `target/twk` for format/lint/test/build.

## Global Constraints

- Treat the boot compiler in `boot/` as the primary implementation.
- After editing `.tw` files, run `target/twk fmt` and `target/twk lint`.
- Do not claim dataframe `nulls` is solved unless verified; this task only fixes producer retyping into typed fields.
- Record-field stores are non-coercing; a producer stored into a typed field must be physically typed before the store.

---

### Task 1: Add regression fixture and update analysis API call sites

**Files:**
- Modify: `boot/tests/suites/typed_record_fields_suite.tw`
- Modify: `boot/tests/suites/route_typed_vec_suite.tw`

**Interfaces:**
- Consumes: `analyze_typed_fields(funcs, builtins, typeable_return)`.
- Produces: a failing regression test proving a typed-return `Vector<Bool>` call result stored into a typed field must verify.

- [x] **Step 1: Update helper and suite calls**

Replace existing two-argument calls to `analyze_typed_fields` with `analyze_typed_fields(..., Dict.new())` where the tests intentionally exercise builder-only behavior.

- [x] **Step 2: Add the Bool call-result field fixture**

Add a fixture equivalent to:

```tw
fn bool_call_result_field_src() String {
  "type BoolCol = .{ mask: Vector<Bool> }\n"
    .concat("fn no_nulls(n: Int) Vector<Bool> { Vector.make(n, false) }\n")
    .concat("fn mk(n: Int) BoolCol { BoolCol.{ mask: no_nulls(n) } }\n")
    .concat("fn read(c: BoolCol, n: Int) Int {\n")
    .concat("  s := 0\n")
    .concat("  for i in range(n) { if c.mask[i] { s = s + 1 } }\n")
    .concat("  s\n")
    .concat("}\n")
    .concat("read(mk(8), 8)\n")
}
```

- [x] **Step 3: Add verifier regression test**

Add a test that compiles the fixture through `verify_inputs`, copies `prepared.typed_vector_fields` into the env, and expects `verify_prepared_module_with_level(..., .Full)` to return `.Ok(_)`.

- [x] **Step 4: Verify RED**

Run: `target/twk run boot/tests/main.tw`
Expected before routing fix: tests typecheck after signature updates, then the new regression fails with a record-field representation mismatch.

---

### Task 2: Make call-result escape classification typed-field aware

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw`

**Interfaces:**
- Consumes: `typed_fields: Dict<String, Bool>` and `slots: Dict<Int, SlotInfo>` from `compute_eligible_v`.
- Produces: `v_group_escapes`/`op_group_escapes` that treat in-family stores to typed fields as non-escapes.

- [x] **Step 1: Extend escape classifier signatures**

Add parameters to `v_group_escapes` and `op_group_escapes`:

```tw
typed_fields: Dict<String, Bool>,
slots: Dict<Int, SlotInfo>,
fam: ElemFamily,
```

Thread these through all recursive calls.

- [x] **Step 2: Update `ARecord` classification**

In `op_group_escapes`, replace unconditional field escape with typed-field-aware logic:

```tw
.ARecord(tid, fields) => {
  for f in fields {
    if slot_in(f.value, vs) {
      key := "${tid.id}:${f.field.id}"
      if !atom_in_family(f.value, slots, fam) or !typed_fields.has(key) {
        return true
      }
    }
  }

  false
}
```

- [x] **Step 3: Update `ARecordUpdate` classification**

Treat the base as escaping if it is in the group, and the value as non-escaping only when it is active-family and the target field key is typed.

- [x] **Step 4: Update call sites**

Pass real `typed_fields`, `pf.slots`, and `fi.fam` from payload-read and typed-return call-result gates in `compute_eligible_v`. Pass `Dict.new()`, `pf.slots`, and `fam` from strict consumer checks so field consumers remain demoting unless they are only index/len.

- [x] **Step 5: Verify GREEN**

Run: `target/twk run boot/tests/main.tw`
Expected: regression passes and no suite failures.

---

### Task 3: Format, lint, isolated repro, and dataframe probe

**Files:**
- Verify only; no expected source modifications beyond formatter output.

**Interfaces:**
- Consumes: fixed boot compiler.
- Produces: verification evidence for the isolated repro and dataframe WAT counts.

- [x] **Step 1: Format touched Twinkle files**

Run:

```bash
target/twk fmt boot/compiler/backend/route_typed_vec.tw boot/compiler/backend/typed_param_abi.tw boot/tests/suites/typed_record_fields_suite.tw boot/tests/suites/route_typed_vec_suite.tw
```

- [x] **Step 2: Lint relevant entrypoint**

Run:

```bash
target/twk lint boot/tests/main.tw
```

- [x] **Step 3: Build isolated repro**

Run the `/tmp/simple_field.tw` build from the report and expect success.

- [x] **Step 4: Probe dataframe counts**

Run the dataframe WAT build/count commands. Report counts as diagnostic evidence only, not as a success claim for `nulls` unless the field is actually typed.

---

### Follow-up Task: Type field-read gather results stored back into typed fields

**Goal:** Let a field with a direct clean producer remain typed when another producer is `typed_field.gather(idx)` stored back into the same field.

**Approach:** Seed field analysis from direct producers before the final boxed-store demotion pass, then iterate. In each round, collect typed field reads from the previous field set, propagate through `gather` results, and classify those derived slots as clean field producers if they feed exactly one typed field and do not otherwise escape. Recompute field-store demotions from scratch each round.

**Regression:** Add a `BoolCol.{ mask: c.mask.gather(idx) }` fixture and assert the final prepared module marks the mask field typed and verifies.

- [x] Implemented field-analysis fixpoint for typed field reads feeding `gather` results.
- [x] Added the gather-result field regression.
- [x] Verified `/tmp/gather_field.tw` emits `BoolCol.mask: PVecBool`, uses `gather_bool`, and emits no `box_bool` or generic `gather` calls.
- [x] Verified dataframe `Column.nulls` emits `PVecBool` with no `box_bool` or generic `gather` calls in `order_by_breakdown.wat`.
