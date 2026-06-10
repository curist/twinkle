# Typed Vector Record Fields Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep a `Vector<Int>` in its typed `PVecI64` representation when stored in a record field, so reads through the field (`r.col[i]`, `r.col.len()`) hit raw `i64` leaves instead of the boxed `PVec` pointer-chase.

**Architecture:** A whole-program analysis decides, per `(TypeId, FieldId)`, whether a `Vector<Int>` field is physically `PVecI64` (fully typed) or `PVec` (boxed) — never mixed, no boundary coercions. The decision rides in `ResolvedEnv` to a single override point in `layout_of_named`, and drives the existing typed-vector routing (`route_typed_vec.tw`) to retype producer/consumer slots. Conservative single-direction inference (Approach B): the field is the only cross-function carrier, so no fixpoint.

**Tech Stack:** Self-hosted Twinkle boot compiler (`boot/`), Wasm GC backend. Validation via integration WAT probes + `make boot-test` + `make stage2` self-host fixed point.

**Design doc:** [typed-record-fields.md](typed-record-fields.md)

---

## Background for the implementer

You are working inside the **boot compiler** (`boot/compiler/`), itself written in Twinkle. The build/verify loop:

- `make boot-test` runs `target/twk run boot/tests/main.tw`. This **compiles your edited `boot/compiler/*.tw` source with the current `target/twk` and runs it**, so unit/integration suites exercise your source changes immediately — no rebuild of `target/twk` needed for the suites to see edits.
- `make stage2` rebuilds `target/boot.wasm` by having the compiler compile **itself**, and checks the self-host **fixed point** (stage2 == stage4). This is the real correctness gate for compiler changes. Run it after each task that changes compiler behavior.
- WAT probes: `target/twk build path/to/probe.tw -o /tmp/out.wat` emits human-readable WAT. Inspect with `grep`, never by reading the whole file (large).

Key types you will touch (already defined):

- `PreparedOp` variants (`boot/compiler/backend/prepared_ir.tw:97`):
  - `ARecord(TypeId, Vector<PreparedFieldAtom>)` — record construction.
  - `ARecordGet(PreparedAtom, FieldId, TypeId)` — field read.
  - `ARecordUpdate(PreparedAtom, FieldId, PreparedAtom, Bool, TypeId)` — field update.
  - `AIndex(PreparedAtom base, PreparedAtom index, IndexKind, MonoType)` — `xs[i]`; `IndexKind.Array` is the vector read.
- `PreparedFieldAtom = .{ field: FieldId, value: PreparedAtom }` (`prepared_ir.tw:118`).
- `FieldId = .{ id: Int }`, `TypeId = .{ id: Int }`.
- `SlotInfo` carries `.mono: MonoType` (semantic type) and `.slot.id: Int`.
- `ResolvedEnv` (`boot/compiler/resolver.tw:110`) — threaded to every `layout_of` call site.

The decision-set key format is the string `"${tid.id}:${fid.id}"` throughout.

---

## Task 1: Plumbing — env field + layout override (inert no-op)

Add the carrier and the single representation override point, wired so that with an empty decision set nothing changes. Proven by a layout unit test that toggles the set by hand.

**Files:**
- Modify: `boot/compiler/resolver.tw:110-126` (add field), `:128-146` (`empty_env`)
- Modify: `boot/compiler/codegen/wasm_layout.tw:212-220` (field loop override)
- Test: `boot/tests/suites/wasm_layout_suite.tw`

- [ ] **Step 1: Add the field to `ResolvedEnv`**

In `boot/compiler/resolver.tw`, add to the `ResolvedEnv` record (after `value_origins`):

```tw
pub type ResolvedEnv = .{
  types: Vector<TypeEntry>,
  type_names: Vector<String>,
  type_bindings: Dict<String, TypeId>,
  type_origins: Dict<Int, String>,
  functions: Vector<FunctionSig>,
  function_bindings: Dict<String, String>,
  function_origins: Dict<String, FunctionOrigin>,
  methods: Dict<String, Vector<MethodEntry>>,
  methods_by_type: Dict<Int, Vector<MethodEntry>>,
  type_index: Dict<String, Int>,
  type_id_index: Dict<Int, Int>,
  func_index: Dict<String, Int>,
  extern_namespaces: Dict<String, Bool>,
  value_bindings: Dict<String, MonoType>,
  value_origins: Dict<String, FunctionOrigin>,
  typed_vector_fields: Dict<String, Bool>,
}
```

And in `empty_env()`, add the default (after `value_origins: Dict.new(),`):

```tw
    value_origins: Dict.new(),
    typed_vector_fields: Dict.new(),
```

All other env constructions derive from `empty_env()` via field rebinding, so no other site needs updating.

- [ ] **Step 2: Add the override in `layout_of_named`**

In `boot/compiler/codegen/wasm_layout.tw`, replace the field loop (currently lines 212-220) inside the `.Record(...)` arm:

```tw
      sym := "$${name}_${mono_to_key(.Named(tid, args))}"
      field_layouts := collect f, i in fields {
        // Substitute type params with concrete args
        field_ty := subst_type_params(f.ty, type_params, args)
        vt := if env.typed_vector_fields.has("${tid.id}:${i}") and is_int_vector_field(field_ty) {
          .Ref(true, .Named("rt_types__PVecI64"))
        } else {
          val_type_of_mono(field_ty, env)
        }
        WasmFieldLayout.{
          field_id: FieldId.{ id: i },
          name: f.name,
          val_type: vt,
        }
      }
      .Record(WasmRecordLayout.{ type_id: tid, sym, fields: field_layouts })
```

Add this helper near the top of `wasm_layout.tw` (after the imports, before `mono_to_key`):

```tw
fn is_int_vector_field(ty: MonoType) Bool {
  case ty {
    .Vector(.Int) => true,
    _ => false,
  }
}
```

- [ ] **Step 3: Write the failing test**

In `boot/tests/suites/wasm_layout_suite.tw`, add a test that builds a record-with-`Vector<Int>`-field env, populates `typed_vector_fields`, and asserts the field val_type is the `PVecI64` ref. Use the existing `env_with_record` helper (line 19) and `ResolvedField` shape used elsewhere in this suite.

```tw
test "record Vector<Int> field becomes PVecI64 when flagged typed" {
  fields := [ResolvedField.{ name: "col", ty: .Vector(.Int) }]
  base := env_with_record("Col", 30, fields)
  // flag (tid=30, fid=0) typed
  base.typed_vector_fields["30:0"] = true

  l := layout_of(.Named(TypeId.{ id: 30 }, []), base)
  case l {
    .Record(r) => {
      assert.eq(r.fields.len(), 1)
      case r.fields[0].val_type {
        .Ref(_, .Named(n)) => assert.eq(n, "rt_types__PVecI64"),
        _ => assert.fail("expected PVecI64 ref field"),
      }
    },
    _ => assert.fail("expected record layout"),
  }
}

test "record Vector<Int> field stays boxed when not flagged" {
  fields := [ResolvedField.{ name: "col", ty: .Vector(.Int) }]
  base := env_with_record("Col", 31, fields)

  l := layout_of(.Named(TypeId.{ id: 31 }, []), base)
  case l {
    .Record(r) => case r.fields[0].val_type {
      .Ref(_, .Named(n)) => assert.ne(n, "rt_types__PVecI64"),
      _ => {},
    },
    _ => assert.fail("expected record layout"),
  }
}
```

Confirm `ResolvedField` constructor and the `assert` API (`assert.eq`/`assert.ne`/`assert.fail`) match the conventions already used in `wasm_layout_suite.tw` — adapt names to whatever the suite uses (check the top of the file for the `assert` import and existing test macro/`test "..."` form). If the suite uses a different test-declaration form, mirror it exactly.

- [ ] **Step 4: Run the test to verify it fails**

Run: `make boot-test 2>&1 | tail -20`
Expected: the first new test FAILS before Step 2 is applied (if you wrote the test before the override). If you applied Step 2 already, instead temporarily confirm by asserting the override fires. Net expectation after Steps 1-2: both tests PASS.

- [ ] **Step 5: Run the full suite**

Run: `make boot-test 2>&1 | tail -5`
Expected: `Ran N tests: N passed` (N = prior count + 2).

- [ ] **Step 6: Verify self-host fixed point**

Run: `make stage2 2>&1 | tail -5`
Expected: no error; fixed point holds (`make stage2` completes; if `target/boot.wasm` was fresh it may say "Nothing to be done", in which case touch a source file or trust the boot-test compile path).

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/resolver.tw boot/compiler/codegen/wasm_layout.tw boot/tests/suites/wasm_layout_suite.tw
git commit -m "typed-vector: env-carried typed record-field layout override (inert)

Add ResolvedEnv.typed_vector_fields and a single override point in
layout_of_named so a (TypeId, FieldId) flagged typed materializes a
PVecI64 struct field instead of the boxed PVec ref. Inert until the
analysis populates the set; covered by a layout unit test."
```

---

## Task 2: Surface the decision set on `PreparedModule` and enrich `env` in codegen (still empty)

Thread an (empty) decision set from `prepare_backend` through to the env consumed by verify/plan/emit, so later tasks only have to populate it.

**Files:**
- Modify: `boot/compiler/backend/prepare.tw:33-37` (`PreparedModule`), `:46-65` (`prepare_backend`)
- Modify: `boot/compiler/codegen/codegen.tw:87-115` (build `env2`)

- [ ] **Step 1: Add the field to `PreparedModule`**

In `boot/compiler/backend/prepare.tw`:

```tw
pub type PreparedModule = .{
  anf: AnfModule,
  closure_captures: Dict<Int, Vector<CaptureParam>>,
  funcs: Vector<PreparedFunc>,
  typed_vector_fields: Dict<String, Bool>,
}
```

- [ ] **Step 2: Populate it (empty for now) in `prepare_backend`**

In `prepare_backend`, change the return (currently lines 63-64):

```tw
  funcs3 := route_typed_vectors(funcs2, builtins)
  .{ anf: anf2, closure_captures, funcs: funcs3, typed_vector_fields: Dict.new() }
```

- [ ] **Step 3: Build `env2` in codegen and thread it to verify/plan/emit**

In `boot/compiler/codegen/codegen.tw`, after `prepared := prepare_backend(...)` (line 87) and before the verify call, add:

```tw
  prepared := prepare_backend(closure_conversion.anf, env, builtins, closure_conversion.captures)
  t2 := date.now()

  env2 := env
  env2.typed_vector_fields = prepared.typed_vector_fields
```

Then change the three downstream calls to pass `env2` instead of `env`:

- `verify_prepared_module_with_level(prepared, env2, builtins, verify_level_from_env())` (line 95)
- `registry := plan_wasm_types(prepared, env2, builtins)` (line 107)
- `user_module := emit_module(prepared, registry, builtins, env2)` (line 115)

Leave any other use of `env` in `codegen.tw` (e.g. earlier phases) untouched.

- [ ] **Step 4: Run the suite + self-host (pure no-op, must be green)**

Run: `make boot-test 2>&1 | tail -5`
Expected: `Ran N tests: N passed` (unchanged from Task 1).

Run: `make stage2 2>&1 | tail -5`
Expected: fixed point holds, no error.

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/backend/prepare.tw boot/compiler/codegen/codegen.tw
git commit -m "typed-vector: thread typed-field decision set into verify/plan/emit env

PreparedModule now carries typed_vector_fields (empty for now);
codegen builds env2 with it and passes env2 to verify, plan, and emit so
the struct type definition, struct.new, struct.get, and verification all
read one source of truth. No behavior change yet."
```

---

## Task 3: Refactor `route_typed_vec` escape analysis to distinguish field stores (behavior-preserving)

Generalize the escape analysis so a v-group's record-field stores can be reported separately from genuine escapes. With the existing call path, field stores still count as escapes (current behavior), so nothing changes yet. This is the shared-predicate factoring the design requires.

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw` (escape analysis section, lines ~303-413)

- [ ] **Step 1: Add a classification result type and a classifier**

In `route_typed_vec.tw`, add near the escape-analysis section:

```tw
// Result of classifying how a typed-vector v-group is used.
// - escapes: true if used by anything other than index/len/return/break and
//   record-field stores.
// - field_keys: the distinct "${tid}:${fid}" keys this group is stored into via
//   ARecord / ARecordUpdate (Vector<Int> fields only).
type VGroupUse = .{ escapes: Bool, field_keys: Vector<String> }
```

Add a classifier that walks the body once, mirroring `v_group_escapes` but recording field-store keys instead of treating them as escapes. Keep `v_group_escapes` as a thin wrapper so existing callers are unchanged:

```tw
fn classify_v_group(
  expr: PreparedExpr,
  vs: Dict<Int, Bool>,
  len_id: Int,
  builtins: BuiltinRegistry,
  slots: Dict<Int, SlotInfo>,
) VGroupUse {
  acc := VGroupUse.{ escapes: false, field_keys: [] }
  classify_expr(expr, vs, len_id, builtins, slots, acc)
}
```

Implement `classify_expr` / `classify_op` as copies of `v_group_escapes` / `op_group_escapes` that:
- on `.ARecord(tid, fields)`: for each field atom whose `value` slot is in `vs`, if that field's slot mono is `Vector<Int>`, append `"${tid.id}:${f.field.id}"` to `field_keys` (do **not** set `escapes`); for any in-`vs` value in a non-`Vector<Int>` field, set `escapes = true`.
- on `.ARecordUpdate(base, fid, val, _, tid)`: if `val` slot is in `vs`, treat as a field store the same way (append `"${tid.id}:${fid.id}"`); if `base` slot is in `vs`, that is a read of the typed vector as a record — set `escapes = true` (a typed vector is not a record).
- everything else: identical to `op_group_escapes`, accumulating `escapes = escapes or <existing condition>`.

Because Twinkle records are immutable, thread `acc` through the recursion and return the merged result (merge = `escapes` OR'd, `field_keys` concatenated), following the rebinding style used elsewhere in this file.

To get a value slot's mono, look it up in `slots` (`SlotInfo.mono`) and reuse the existing `mono_key_of`/`is_int_vector` helpers (lines 153, 265).

- [ ] **Step 2: Re-express `v_group_escapes` in terms of the classifier (preserve behavior)**

Replace the body of `v_group_escapes` so field stores still count as escapes for current callers:

```tw
fn v_group_escapes(
  expr: PreparedExpr,
  vs: Dict<Int, Bool>,
  len_id: Int,
  builtins: BuiltinRegistry,
) Bool {
  // Existing callers don't have slots handy and treat field stores as escapes.
  // Keep that exact behavior by delegating to the structural walk below.
  ... // keep the original implementation unchanged
}
```

Simplest safe approach: **leave `v_group_escapes`/`op_group_escapes` exactly as they are** and add `classify_v_group`/`classify_expr`/`classify_op` as new functions alongside them. Duplication here is acceptable and lower-risk than rewriting the proven path; a later cleanup can merge them once the new path is trusted. Prefer this unless you are confident in the merge.

- [ ] **Step 3: Run suite + self-host (no behavior change)**

Run: `make boot-test 2>&1 | tail -5`
Expected: `Ran N tests: N passed` (unchanged).

Run: `make stage2 2>&1 | tail -5`
Expected: fixed point holds.

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/backend/route_typed_vec.tw
git commit -m "typed-vector: add v-group classifier that records record-field stores

classify_v_group reports a typed vector's field-store keys separately
from genuine escapes, without changing the existing escape path. Shared
shape logic for the upcoming typed-field analysis. No behavior change."
```

---

## Task 4: Implement `analyze_typed_fields` (pure analysis, not yet wired)

Compute the per-`(TypeId, FieldId)` typed decision over all functions. Build and unit-test it in isolation; do not wire it into `prepare_backend` yet (the real set stays empty until Task 6).

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw` (new analysis function + helpers)
- Test: `boot/tests/suites/typed_record_fields_suite.tw` (new) and register it in `boot/tests/main.tw`

- [ ] **Step 1: Implement the analysis**

Add to `route_typed_vec.tw`:

```tw
// Per-field accumulator while scanning all functions.
type FieldStat = .{ has_ok_producer: Bool, bad_producer: Bool, bad_consumer: Bool }

// Whole-program decision: which (TypeId, FieldId) Vector<Int> fields can be
// physically PVecI64. Conservative single-direction (Approach B): a field is
// typed iff it has >=1 typed-routable producer and every producer and consumer
// is typed-friendly.
pub fn analyze_typed_fields(
  funcs: Vector<PreparedFunc>,
  builtins: BuiltinRegistry,
) Dict<String, Bool> {
  ids := route_ids(builtins) // factor the RouteIds construction out of route_typed_vectors
  stats: Dict<String, FieldStat> = Dict.new()

  for pf in funcs {
    stats = scan_func_fields(pf, ids, builtins, stats)
  }

  out: Dict<String, Bool> = Dict.new()

  for key in stats.keys() {
    case stats[key] {
      .Some(s) => if s.has_ok_producer and !s.bad_producer and !s.bad_consumer {
        out[key] = true
      },
      .None => {},
    }
  }

  out
}
```

`route_ids(builtins)` is the `RouteIds.{ ... }` literal currently built inline at the top of `route_typed_vectors` (lines 51-60) — extract it into a `fn route_ids(builtins: BuiltinRegistry) RouteIds` and call it from both places (DRY).

`scan_func_fields` does, for one function:

1. `copy_map := build_copy_map(pf.body, Dict.new())`.
2. `cands := collect_candidates(pf.body, pf.slots, ids, [])` — typed-vector v-groups.
3. For each candidate `c`: `aliases := aliases_for(c.v, copy_map)`; `use := classify_v_group(pf.body, aliases, ids.len, builtins, pf.slots)`.
   - If `use.escapes` is false and `use.field_keys` has exactly one distinct key `k`: this producer is OK for `k` → mark `stats[k].has_ok_producer = true`.
   - If `use.field_keys` has a distinct-key count != 1 (zero is just a normal local; >=2 is multi-field), and there is at least one field key: every key in `use.field_keys` gets `bad_producer = true` (a value that also escapes or feeds multiple fields can't be cleanly typed).
   - If `use.escapes` is true **and** there are field keys: every key gets `bad_producer = true`.
4. Separately, walk the body for **all** producer field-store sites whose stored value is **not** one of the OK typed candidates: any `ARecord`/`ARecordUpdate` storing into a `Vector<Int>` field a value that does not trace to an OK candidate v-group ⇒ `stats[key].bad_producer = true`. (This catches boxed producers: a field fed by a parameter or a combinator result.)
5. Walk the body for consumers: every `ARecordGet(_, fid, tid)` whose **result slot** mono is `Vector<Int>` with key `k`. If the result slot's uses are all `AIndex .Array` / `len` locally ⇒ contributes nothing bad; otherwise `stats[k].bad_consumer = true`. Reuse a consumer-use check shaped like the `v_group_escapes` index/len test, applied to the single result slot.

Implement helpers:
- `distinct_keys(keys: Vector<String>) Vector<String>` — dedupe.
- `result_consumed_typed_only(expr, result_sid, len_id, builtins) Bool` — true if `result_sid` is used only by `AIndex .Array` base and `len(result_sid)`; mirror the relevant arms of `op_group_escapes` for a singleton set `{result_sid}`.
- A `field_store_sites(expr, slots) Vector<.{ key: String, value: PreparedAtom }>` collector for step 4, gating on `Vector<Int>` field slot mono.

Keep all of these in `route_typed_vec.tw`; they share the candidate/alias/escape primitives already there.

- [ ] **Step 2: Write failing unit tests**

Create `boot/tests/suites/typed_record_fields_suite.tw`. Build small `PreparedFunc` values by hand (mirror how `wasm_plan_suite.tw` / `backend_verify_suite.tw` construct prepared funcs — copy their helper for assembling a `PreparedFunc` with a slot map and a `PreparedExpr` body). Cover:

- **Positive:** one function constructs `Col.{ data: <builder_freeze v> }` where `v` is a `Vector<Int>` candidate whose only other uses are `len`/index; another function reads `c.data` and indexes it. Expect `analyze_typed_fields(funcs, builtins)` to contain key `"${col_tid}:0"` → true.
- **Negative (boxed producer):** the `data` field is fed from a function parameter (not a candidate). Expect the key absent.
- **Negative (bad consumer):** `c.data` is passed to a user function (escape). Expect the key absent.
- **Negative (multi-field):** the same `v` stored into two different field keys. Expect both keys absent.

Each test asserts `out.has(key)` true/false via the suite's `assert` API.

Register the suite in `boot/tests/main.tw` next to the other `*_suite` registrations (follow the existing `use`/run pattern there).

- [ ] **Step 3: Run the new suite to verify it fails, then passes**

Run: `make boot-test 2>&1 | tail -20`
Before implementing Step 1: the new suite FAILS (function undefined). After Step 1: all four tests PASS.

- [ ] **Step 4: Full suite + self-host**

Run: `make boot-test 2>&1 | tail -5`
Expected: `Ran N tests: N passed` (prior + 4).

Run: `make stage2 2>&1 | tail -5`
Expected: fixed point holds (analysis is not yet called in the real pipeline, so behavior is unchanged).

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/backend/route_typed_vec.tw boot/tests/suites/typed_record_fields_suite.tw boot/tests/main.tw
git commit -m "typed-vector: whole-program analyze_typed_fields (Approach B), unit-tested

Decide per (TypeId, FieldId) whether a Vector<Int> field can be PVecI64:
typed iff it has a typed-routable producer and every producer/consumer is
typed-friendly; any boxed producer, escaping consumer, or multi-field
producer demotes it. Pure analysis, not yet wired into compilation."
```

---

## Task 5: Extend `route_typed_vectors` to type producer/consumer slots for typed fields

Make the routing pass accept the decision set and (a) stop treating stores into typed fields as escapes, (b) treat reads from typed fields as typed sources, retyping those slots to `PVecI64`. Unit-test against a hand-supplied set. Still not activated in the real pipeline (Task 6 does the wire-up).

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw` (`route_typed_vectors`, `route_func`, candidate collection, escape check)
- Test: `boot/tests/suites/typed_record_fields_suite.tw`

- [ ] **Step 1: Add the set parameter**

Change the signature:

```tw
pub fn route_typed_vectors(
  funcs: Vector<PreparedFunc>,
  builtins: BuiltinRegistry,
  typed_fields: Dict<String, Bool>,
) Vector<PreparedFunc> {
  ids := route_ids(builtins)

  collect pf in funcs {
    route_func(pf, ids, builtins, typed_fields)
  }
}
```

Thread `typed_fields` into `route_func`.

- [ ] **Step 2: Stores into typed fields are not escapes**

In the escape check used by `route_func` (`v_group_escapes`/`op_group_escapes`), make `ARecord`/`ARecordUpdate` storing a v-group value into a field whose key is in `typed_fields` return `false` (not an escape). Storing into a non-typed field stays an escape. Pass `typed_fields` (and `slots` for the field mono check) down to the escape predicate, or branch in `route_func` using the `classify_v_group` result: a candidate is eligible if `use.escapes` is false **and** every key in `use.field_keys` is in `typed_fields`.

Concretely, in `route_func` replace the eligibility test (current line 82 `if !v_group_escapes(...)`) with:

```tw
    use := classify_v_group(pf.body, aliases, ids.len, builtins, pf.slots)
    all_field_keys_typed := use.field_keys.len() == 0 or all_in(use.field_keys, typed_fields)

    if !use.escapes and all_field_keys_typed {
      // eligible: mark v + builder lineage typed (existing body)
    }
```

with `all_in(keys, set) Bool` = every key present in `set`.

- [ ] **Step 3: Reads from typed fields are typed sources**

Extend `collect_candidates` so an `ARecordGet(_, fid, tid)` whose key `"${tid.id}:${fid.id}"` is in `typed_fields` registers its result slot as a typed vector (append a `Cand` with `v: result_sid` and a sentinel builder id, or add a parallel candidate list for field-reads that need no builder rewrite). Then in `route_func`, ensure these result slots are added to `eligible_v` (so the slot retypes to `PVecI64` and downstream `xs[i]`/`len` route to `_i64`). A field-read candidate has **no builder lineage**, so skip the `eligible_b` loop for it.

The cleanest shape: give `collect_candidates` access to `typed_fields`, and emit field-read candidates into a separate `Vector<Int>` (result sids). In `route_func`, union those into `eligible_v` after the escape gate (a typed-field read is unconditionally a typed source — the field is already decided typed).

- [ ] **Step 4: Update all `route_typed_vectors` callers**

`prepare_backend` (`prepare.tw:63`) currently calls `route_typed_vectors(funcs2, builtins)`. Update to pass a set; for now pass `Dict.new()` (Task 6 swaps in the real set):

```tw
  funcs3 := route_typed_vectors(funcs2, builtins, Dict.new())
```

Check `boot/tests/suites/` for any direct callers of `route_typed_vectors` and update them similarly (search: `grep -rn 'route_typed_vectors(' boot/`).

- [ ] **Step 5: Unit-test the routing with a hand-supplied set**

In `typed_record_fields_suite.tw`, add tests that call `route_typed_vectors(funcs, builtins, typed_fields)` with a set flagging the column field, then assert:
- the producer builder_freeze slot / field-read result slot is retyped to a `PVecI64` ref (`SlotInfo.wasm_type` is `.Ref(_, .Named("rt_types__PVecI64"))`);
- an `xs[i]` over a typed-field-read result will route (inspect that the relevant `len`/builder ops were swapped to `_i64`, mirroring how you'd verify the existing S2.0 routing).

If hand-building these funcs is impractical, replace this step with an **integration** assertion deferred to Task 6 (WAT probe) and keep only the producer/consumer slot-typing assertions you can build cheaply. Do not leave the step empty — either unit-assert slot retyping or explicitly move the assertion to Task 6's probe and note it here.

- [ ] **Step 6: Run suite + self-host**

Run: `make boot-test 2>&1 | tail -5`
Expected: all green (real pipeline still passes `Dict.new()`, so compilation behavior is unchanged).

Run: `make stage2 2>&1 | tail -5`
Expected: fixed point holds.

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/backend/route_typed_vec.tw boot/compiler/backend/prepare.tw boot/tests/suites/typed_record_fields_suite.tw
git commit -m "typed-vector: route producer/consumer slots for typed record fields

route_typed_vectors takes the typed-field set: stores into a typed field
no longer escape, and reads from a typed field become typed sources whose
result slots retype to PVecI64. Real pipeline still passes an empty set;
covered by routing unit tests."
```

---

## Task 6: Activate end-to-end + integration probes

Wire the analysis into `prepare_backend`, feed the real set to routing and to `env2`, and prove the whole path with WAT probes. This is the activation task — run the full verification gate.

**Files:**
- Modify: `boot/compiler/backend/prepare.tw:46-65`
- Create: `examples/sort-bench/typed_record_field_probe.tw` (positive), `examples/sort-bench/typed_record_field_boxed_probe.tw` (negative)
- Modify: `docs/plans/vector-perf/README.md` (index the probes)

- [ ] **Step 1: Write the positive probe**

Create `examples/sort-bench/typed_record_field_probe.tw`: a record holding a `Vector<Int>` column built by `collect` in a constructor function, read+indexed in a separate function, summed in a hot loop so the read is real. Example shape:

```tw
type Col = .{ data: Vector<Int>, n: Int }

fn make_col(n: Int) Col {
  xs := collect i in range(n) { i * 3 }
  Col.{ data: xs, n }
}

fn sum_col(c: Col) Int {
  total := 0
  for k in range(c.n) {
    total = total + c.data[k]
  }
  total
}

c := make_col(1000)
println(sum_col(c).to_string())
```

- [ ] **Step 2: Run it to confirm current (boxed) codegen**

Run: `target/twk build examples/sort-bench/typed_record_field_probe.tw -o /tmp/trf.wat && grep -c 'rt_arr__get_i64' /tmp/trf.wat`
Expected (before Step 4): `0` — the field read uses the boxed `rt_arr__get`, not `_i64`.

Also confirm it runs correctly:
Run: `target/twk run examples/sort-bench/typed_record_field_probe.tw`
Expected: prints the correct sum (for n=1000, `sum of i*3 for i in 0..999` = `3 * 999*1000/2` = `1498500`).

- [ ] **Step 3: Write the negative probe**

Create `examples/sort-bench/typed_record_field_boxed_probe.tw`: same shape but the column is fed from a parameter or via `.append` (a non-routable boxed producer), so the field must stay boxed.

```tw
type Col = .{ data: Vector<Int>, n: Int }

fn wrap(xs: Vector<Int>) Col {
  Col.{ data: xs, n: xs.len() }
}

fn sum_col(c: Col) Int {
  total := 0
  for k in range(c.n) {
    total = total + c.data[k]
  }
  total
}

base := [1, 2, 3, 4]
c := wrap(base.append(5))
println(sum_col(c).to_string())
```

- [ ] **Step 4: Activate the analysis in `prepare_backend`**

In `boot/compiler/backend/prepare.tw`:

```tw
  funcs2 := assign_repr_for_module(funcs, env, builtins)

  typed_fields := route_typed_vec.analyze_typed_fields(funcs2, builtins)
  funcs3 := route_typed_vectors(funcs2, builtins, typed_fields)
  .{ anf: anf2, closure_captures, funcs: funcs3, typed_vector_fields: typed_fields }
```

Ensure `analyze_typed_fields` is exported (`pub`) and imported in `prepare.tw`'s `use compiler.backend.route_typed_vec.{...}` line.

- [ ] **Step 5: Rebuild and verify the positive probe is now typed**

Run: `make stage2 2>&1 | tail -5`
Expected: self-host fixed point holds (the compiler still compiles itself; `analyze_typed_fields` may type fields inside the compiler too — the fixed-point check guarantees correctness).

Run: `target/twk build examples/sort-bench/typed_record_field_probe.tw -o /tmp/trf.wat`
Then:
- `grep -c 'rt_arr__get_i64' /tmp/trf.wat` → Expected: `>= 1` (field read is typed).
- `grep 'rt_types__PVecI64' /tmp/trf.wat | head` → Expected: the `Col` struct field type references `PVecI64`.
- `grep -c 'box_i64' /tmp/trf.wat` → Expected: `0` at the field store/load (boxing only appears at genuine S2.1 boundaries, which this probe avoids).

Run: `target/twk run examples/sort-bench/typed_record_field_probe.tw`
Expected: still prints `1498500` (correctness preserved through the representation change).

- [ ] **Step 6: Verify the negative probe stays boxed**

Run: `target/twk build examples/sort-bench/typed_record_field_boxed_probe.tw -o /tmp/trfb.wat && grep -c 'rt_arr__get_i64' /tmp/trfb.wat`
Expected: `0` — the field has a boxed producer, so it stays `PVec`.

Run: `target/twk run examples/sort-bench/typed_record_field_boxed_probe.tw`
Expected: prints `15` (1+2+3+4+5).

- [ ] **Step 7: Full suite**

Run: `make boot-test 2>&1 | tail -5`
Expected: `Ran N tests: N passed`, no regressions.

- [ ] **Step 8: Dataframe guardrail — no regression**

Run the existing dataframe example/benches to confirm `filter`/`join`/`group_by`/`order_by` still produce correct results:
Run: `target/twk run examples/dataframe/bench/sort_by_costs.tw 2>&1 | tail -20` (and any other dataframe smoke test present)
Expected: completes without error; results unchanged from before this branch's activation.

- [ ] **Step 9: Index the probes**

Add the two new probes to `docs/plans/vector-perf/README.md` under the probe index, with one-line descriptions (positive: typed record-field column read; negative: boxed-producer field stays `PVec`).

- [ ] **Step 10: Commit**

```bash
git add boot/compiler/backend/prepare.tw examples/sort-bench/typed_record_field_probe.tw examples/sort-bench/typed_record_field_boxed_probe.tw docs/plans/vector-perf/README.md
git commit -m "typed-vector: activate typed record-field routing end-to-end (S2.2)

prepare_backend now runs analyze_typed_fields and feeds the decision to
routing and to the emit env, so a Vector<Int> column built by collect and
read through a record field keeps PVecI64 storage with rt_arr__get_i64
reads. Positive/negative WAT probes added; self-host + boot suite green;
dataframe results unchanged."
```

---

## Task 7: Verifier coverage for typed record-field slots

Confirm the prepared-IR verifier accepts a typed value stored into a typed field and rejects a representation mismatch. The activation in Task 6 already runs verification under `env2`; this task hardens/clarifies it if needed.

**Files:**
- Inspect: `boot/compiler/backend/verify_slots.tw` (typed-vector handling around line 184), `verify_expr.tw`
- Modify only if a gap is found.

- [ ] **Step 1: Check whether verification already passes for the positive probe**

Run: `TWINKLE_VERIFY=strict target/twk build examples/sort-bench/typed_record_field_probe.tw -o /tmp/trf.wat 2>&1 | tail -20`
(Use whatever env var/flag selects the strict verify level — check `verify_level_from_env()` in `codegen.tw` for the exact name.)
Expected: builds cleanly. If it errors with a representation mismatch on the `Col.data` store or read, there is a verifier gap.

- [ ] **Step 2: If a gap exists, extend the verifier**

In `verify_slots.tw` / `verify_expr.tw`, where `ARecord` field stores and `ARecordGet` results are checked against expected field val_types, consult the field layout under the enriched env (the verifier already receives `env2`). Make the expected field type for a typed `(tid, fid)` be `PVecI64`, matching the value/result slot wasm_type. Mirror the existing slot-level typed-vector accommodation noted at `verify_slots.tw:184`.

If Step 1 already passes, write a one-line note in the commit that no verifier change was needed and skip the edit.

- [ ] **Step 3: Full suite + self-host**

Run: `make boot-test 2>&1 | tail -5` → Expected: all green.
Run: `make stage2 2>&1 | tail -5` → Expected: fixed point holds.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "typed-vector: verifier accepts typed record-field slots

<Either: extend prepared-IR verification so a PVecI64 value stored into a
typed (TypeId, FieldId) field type-checks, mirroring the slot-level typed
handling. Or: confirm strict verification already passes for typed record
fields; no change needed.>"
```

---

## Self-review notes (for the implementer)

- **Cache-key risk (from the spec):** `cached_layout_of` keys on `mono_to_key(mono)`, but a record's field repr now depends on `env`. This is sound because `env2` is constant across plan+emit and each compilation builds a fresh registry layout cache. If you ever see a record's field type flip between boxed/typed within one build, suspect a cache shared across the un-enriched (prepare) and enriched (emit) phases — the fix is to ensure the emit-phase registry/layout caches are not seeded from prepare-phase caches.
- **`ARecordUpdate` copy semantics:** updating a *different* field of a record whose typed field is copied through must carry the `PVecI64` slot intact. The positive probe does not exercise this; if you add an update-based probe and it traps or mismatches, the update lowering is re-deriving the field type instead of copying by struct type — handle in Task 7's verifier/emit review.
- **Stage0 parity:** not required for this boot-codegen optimization (per the no-stage0-parity rule for backend-only typed-vector opts).
- **Honest payoff expectation:** combinator-built dataframe columns stay boxed until typed combinators (parent-plan Phase 5). The positive probe uses `collect`, which *is* typed; that is the immediately-covered case. Do not expect the negative/boxed probe or combinator-built columns to become typed in this plan.
