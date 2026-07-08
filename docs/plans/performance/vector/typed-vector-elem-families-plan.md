# Typed-vector element families + `PVecBool` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generalize the Int-hardcoded typed-vector routing into an element-family-parameterized layer, then land `PVecBool` (raw i32 leaves) as the first family, so the dataframe `nulls: Vector<Bool>` mask stays typed through reads, gathers, captures, and comparators.

**Architecture:** An `ElemFamily` descriptor (mirroring the runtime's existing `PVecFamily`) is threaded through the three Int-bound layers of the routing core — tags, builtin-id tables, type names — and `route_func` iterates `for fam in families`. Families are disjoint by slot mono-type, so per-family passes never interfere and the escape/alias/capture/gather logic is unchanged. Cross-function ABI facts (params/returns/captures) become family-keyed. The non-route Int-specific sites (field/payload layout, candidate policy, erased variant bridge, structural equality, typed-builder seeding, element boxing) are generalized in Stage 1, then Stage 2 supplies the Bool family data.

**Tech Stack:** Twinkle self-hosted compiler (`boot/`), Wasm GC codegen, `target/twk` CLI. Design spec: [typed-vector-elem-families.md](typed-vector-elem-families.md). Prior family (S1): [typed-vector-spike.md](typed-vector-spike.md).

---

## Conventions for this plan

- **FamilyTag** = the family's `mono_key` String (`"vec_i64"`, `"vec_bool"`). It is self-describing and already the unique key the router uses; `families()` is indexed by it. No new enum type.
- **Clone tasks:** when a step says "clone `foo_i64` → `foo_bool`", the source function is named with its file+line; apply the stated instruction-level delta. Do NOT invent a new algorithm — the i64 version is the reference.
- **Verification loop** (referenced as *[VL]* below, run from repo root):
  ```
  target/twk fmt <edited .tw files>
  cargo run --release -- build boot/main.tw -o /tmp/x.wasm     # stage0 bootstraps
  make bundle-cli 2>&1 | tail -3                               # must reach "Fixed point reached"
  make boot-test 2>&1 | grep -iE "Ran [0-9]+ tests|Failed"     # expect 2980 passed, 0 Failed
  ```
- **Bench guard** *[BG]*: `target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw` — checksums must stay `1000000` / `3000000`.
- Stage 1 tasks are behavior-preserving. Their pass condition is *[VL]* green **and** *[BG]* checksums + timings materially unchanged (the emitted module should be byte-identical; a timing shift signals an accidental behavior change).
- Commit after every task. Match repo commit style (imperative subject, what/why body). Add `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.

## File map

**Stage 1 (generalize, registry = `[i64]`):**
- `boot/compiler/backend/route_typed_vec.tw` — `ElemFamily`, `families()`, `elem_family_of`, per-family loop, threaded helpers.
- `boot/compiler/backend/typed_param_abi.tw` — family-keyed ABI facts.
- `boot/compiler/backend/verify_expr.tw`, `verify_slots.tw` — any-`pvec_type` recognizers.
- `boot/compiler/codegen/emit/coercions.tw`, `arrays.tw`, `closures.tw`, `bridge_funcs.tw` — per-family dispatch.
- `boot/compiler/codegen/emit/runtime_abi.tw` — typed-builder name lists (data, extended in Stage 2).
- `boot/compiler/codegen/runtime/core.tw` — structural equality per-family box.
- `boot/compiler/codegen/wasm_layout.tw`, `boot/compiler/backend/repr_policy.tw` — layout/policy per-family.

**Stage 2 (add Bool family — data + runtime):**
- `boot/compiler/codegen/runtime/types.tw` — `ArrayBool`, `PVecBool`.
- `boot/compiler/codegen/runtime/arr.tw` — `family_bool()`, `*_bool` funcs, empty globals.
- `boot/compiler/builtins.tw` — Bool helper ABI + runtime bindings.
- `examples/performance/sort-bench/` — new probes.

---

# Stage 1 — Generalize the routing (no behavior change)

### Task 1: `ElemFamily` descriptor + `families()` registry + `elem_family_of`

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw` (near `RouteIds`, ~lines 29-57)

- [ ] **Step 1: Add the `ElemFamily` type and registry.** Insert after the `RouteIds` definition. `RouteIds` keeps the shared *boxed* ids (`builder_new`, `builder_push`, `builder_freeze`, `len`, `gather`); the typed ids + names move into `ElemFamily`.

```
pub type ElemFamily = .{
  mono_key: String,          // "vec_i64" — FamilyTag
  pvec_type: String,         // "rt_types__PVecI64"
  elem_wasm: ValType,        // .I64
  builder_new: Int,
  builder_push: Int,
  builder_freeze: Int,
  len: Int,
  get: Int,
  gather: Int,
  box_call: String,          // runtime fn name: "rt_arr__box_i64"
  unbox_call: String,        // "rt_arr__unbox_i64"
}

pub fn families(builtins: BuiltinRegistry) Vector<ElemFamily> {
  [family_i64_desc(builtins)]
}

fn family_i64_desc(builtins: BuiltinRegistry) ElemFamily {
  .{
    mono_key: "vec_i64",
    pvec_type: "rt_types__PVecI64",
    elem_wasm: .I64,
    builder_new: builtins.id("vector$builder_new_i64").id,
    builder_push: builtins.id("vector$builder_push_i64").id,
    builder_freeze: builtins.id("vector$builder_freeze_i64").id,
    len: builtins.id("vector$len_i64").id,
    get: builtins.id("vector$get_i64").id,
    gather: builtins.id("vector$gather_i64").id,
    box_call: "rt_arr__box_i64",
    unbox_call: "rt_arr__unbox_i64",
  }
}
```

- [ ] **Step 2: Add `elem_family_of`.** This replaces `mono_key_of` + `is_int_vector` as the family selector. Insert near `mono_key_of` (~line 928).

```
fn elem_family_of(mono: MonoType, fams: Vector<ElemFamily>) ElemFamily? {
  key := case mono {
    .Vector(.Int) => "vec_i64",
    _ => "other",
  }

  for f in fams {
    if f.mono_key == key {
      return .Some(f)
    }
  }

  .None
}
```

- [ ] **Step 3: Verify it compiles unused.** Nothing calls the new code yet; `make stage2 2>&1 | tail -3` must reach "Fixed point reached" (a compile check that the new declarations parse/typecheck).

Run: `make stage2 2>&1 | tail -3`
Expected: `Fixed point reached`

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/backend/route_typed_vec.tw
git commit -m "backend: add ElemFamily descriptor + families() registry (i64 only)"
```

### Task 2: Thread `ElemFamily` through the routing pass

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw` (`route_func`, `compute_eligible_v`, `materialize_slot_repr`, and the `collect_*`/`classify_*`/`v_group_*` helpers)

- [ ] **Step 1: Convert `route_func` to loop over families.** Replace the single-shot body so it folds each family's rewrite over `pf`. Because families are disjoint by slot mono-type, applying them in sequence is correct.

```
fn route_func(
  pf: PreparedFunc,
  ids: RouteIds,
  builtins: BuiltinRegistry,
  typed_fields: Dict<String, Bool>,
  typed_payloads: Dict<String, Bool>,
  capture_abi: Dict<String, Dict<Int, String>>,
  typeable_return: Dict<String, String>,
) PreparedFunc {
  cur := pf

  for fam in families(builtins) {
    cur = route_func_family(cur, fam, ids, builtins, typed_fields, typed_payloads, capture_abi, typeable_return)
  }

  cur
}
```

  Rename the existing `route_func` body to `route_func_family(pf, fam, ...)` taking `fam: ElemFamily` as the second parameter. (Note the ABI-fact map types changed — `capture_abi: Dict<String, Dict<Int, String>>`, `typeable_return: Dict<String, String>`; these are wired in Task 3. Until then, keep them as the current types and revisit — OR implement Task 3's map-type change first if the compiler rejects the mismatch. Recommended: land Tasks 2+3 as one commit if the type change forces it.)

- [ ] **Step 2: In `route_func_family` + `compute_eligible_v`, replace every Int-literal with `fam`.** Concretely:
  - `is_int_vector(mono_key_of(info))` → `case elem_family_of(info.mono, [fam]) { .Some(_) => true, .None => false }` (single-family check for the active pass). Do this in `collect_candidate_from_op`, `slot_is_int_vector`, `atom_is_int_vector_slot`, `free_var_typed_local`.
  - `.Ref(true, .Named("rt_types__PVecI64"))` (the retype target in `route_func_family`, ~line 439) → `.Ref(true, .Named(fam.pvec_type))`.
  - `pf.phys_return = .Some(.Ref(true, .Named("rt_types__PVecI64")))` (~line 457) → `fam.pvec_type`.
  - `ids.gather` / `ids.gather_i64` usage in `collect_gather_results` / `op_group_escapes`: the *boxed* gather id (`ids.gather`) is the source op that gets swapped; the typed target is `fam.gather`. Keep `ids.gather` as the match key (that is the pre-swap builtin), and route to `fam.gather` in `rewrite`.

- [ ] **Step 3: Thread `fam` into the collect/classify/v_group helpers.** Add a `fam: ElemFamily` parameter to `collect_candidates`*, `collect_gather_results`*, `classify_expr`/`classify_op`, `v_group_escapes`/`op_group_escapes`, `v_group_typeable`, and pass it down. Replace their internal `is_int_vector`/`ids.*_i64` references with `fam.*`. The escape/alias logic is otherwise unchanged.

- [ ] **Step 4: Update `rewrite`** (the builder/len/gather call-swapper) to swap the boxed builtin id → the active `fam`'s typed id (`fam.builder_new`/`builder_push`/`builder_freeze`/`len`/`gather`) for slots retyped in this pass. (Find `rewrite` in the second half of the file, lines 1499+.)

- [ ] **Step 5: Update `materialize_slot_repr`** to loop families the same way, keying output `"${func_id}:${slot}"` unchanged (a slot belongs to exactly one family).

- [ ] **Step 6: Verify no-op.** *[VL]* green (2980 tests), *[BG]* checksums intact.

Run: *[VL]* then *[BG]*
Expected: `Fixed point reached`; `Ran 2980 tests` / `0 Failed`; checksums `1000000` / `3000000`; `order_by` timings within noise of baseline.

- [ ] **Step 7: Commit.**

```bash
git add boot/compiler/backend/route_typed_vec.tw
git commit -m "backend: route typed vectors per ElemFamily (i64 pass unchanged)"
```

### Task 3: Family-keyed cross-function ABI facts

**Files:**
- Modify: `boot/compiler/backend/typed_param_abi.tw`
- Modify: `boot/compiler/backend/route_typed_vec.tw` (consumers: `collect_typed_return_call_results`, `typeable_capture_slots`, `collect_relaxed_captures`)
- Modify: the call sites that build/pass these maps (grep `typeable_return`, `capture_abi`, `typeable_params` across `boot/compiler/`)

- [ ] **Step 1: Change the ABI-fact map types** to carry the family (its `mono_key`):
  - `typeable_params: Dict<String, Vector<Int>>` → `Dict<String, Dict<Int, String>>` (param index → family mono_key).
  - `typeable_return: Dict<String, Bool>` → `Dict<String, String>` (func id → return family mono_key; absent = not typed).
  - `capture_abi: Dict<String, Vector<Int>>` → `Dict<String, Dict<Int, String>>` (capture index → family mono_key).

- [ ] **Step 2: In `typed_param_abi.tw`, record the family** when a param/return/capture is judged typeable. Where it currently establishes `Vector<Int>`-ness via `is_int_vector`, call `elem_family_of(mono, families(builtins))` and store the resulting `fam.mono_key` at that index. A param whose element has no family is simply not recorded (unchanged behavior). Generalize the receiver predicate from `Vector<Int>`-only to `elem_family_of`-any.

- [ ] **Step 3: In the `route_typed_vec.tw` consumers, filter by the active `fam`.**
  - `collect_typed_return_call_results`: the callee's return family (`typeable_return["${fid.id}"]`) must equal `fam.mono_key` for the call result to be a source in this pass.
  - `typeable_capture_slots` + `collect_relaxed_captures`: only indices whose recorded family == `fam.mono_key` participate in this pass.
  Signatures gain `fam: ElemFamily`.

- [ ] **Step 4: Update `slot_typed_after_route` / `free_var_typed_local`** and any other callers to pass the new map types (empty `Dict.new()` where they passed empty before still type-checks).

- [ ] **Step 5: Verify no-op.** With `[i64]` only, every recorded family is `"vec_i64"`, so all filters pass exactly as before. *[VL]* green + *[BG]* intact.

Run: *[VL]* then *[BG]*
Expected: `Ran 2980 tests` / `0 Failed`; checksums intact.

- [ ] **Step 6: Commit.**

```bash
git add boot/compiler/backend/typed_param_abi.tw boot/compiler/backend/route_typed_vec.tw <other call sites>
git commit -m "backend: family-key cross-function typed-vector ABI facts"
```

### Task 4: Generalize the verifier recognizers

**Files:**
- Modify: `boot/compiler/backend/verify_slots.tw` (`is_typed_vec_i64`, ~lines 190-199)
- Modify: `boot/compiler/backend/verify_expr.tw` (`named_typed_vec_ref` + the mismatch check, ~lines 758-778, 1123)

- [ ] **Step 1: Generalize `is_typed_vec_i64`** (rename to `is_typed_vec` and take the family set). It currently checks `name == "rt_types__PVecI64"` and `mono` is `Vector<Int>`. Replace with: `name` matches **any** `fam.pvec_type` and `mono` matches that family via `elem_family_of`.

```
fn is_typed_vec(wasm_type: ValType, mono: MonoType, fams: Vector<ElemFamily>) Bool {
  named := case wasm_type {
    .Ref(_, .Named(name)) => name,
    _ => "",
  }

  for f in fams {
    if named == f.pvec_type {
      case elem_family_of(mono, [f]) {
        .Some(_) => return true,
        .None => {},
      }
    }
  }

  false
}
```

- [ ] **Step 2: Generalize `named_typed_vec_ref`** in `verify_expr.tw` to accept any registered `pvec_type` (loop `families`), and the mismatch predicate (~line 777) to "two typed refs naming different `pvec_type`s, OR a typed ref vs boxed `rt_types__PVec`". The four covered non-coercing edges (local store, record-get result, record field, closure-capture store) are unchanged in structure — only the recognizer broadens.

- [ ] **Step 3: Thread `families(builtins)`** to these functions (the verifier already has `builtins` in scope at its entry; pass the family vector down, or recompute once at the verify entry and thread it).

- [ ] **Step 4: Verify no-op.** Only `PVecI64` is registered, so the recognizer set is identical. *[VL]* green.

Run: *[VL]*
Expected: `Ran 2980 tests` / `0 Failed`.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/backend/verify_slots.tw boot/compiler/backend/verify_expr.tw
git commit -m "backend: verifier recognizes any registered typed-vector repr"
```

### Task 5: Generalize emit dispatch (coercions, index, closures, bridge, equality, builder-seed)

**Files:**
- Modify: `boot/compiler/codegen/emit/coercions.tw` (~lines 25-48)
- Modify: `boot/compiler/codegen/emit/arrays.tw` (`is_pvec_i64` + fast path, ~lines 23-120)
- Modify: `boot/compiler/codegen/emit/closures.tw` (trampoline downcast, ~lines 620-629)
- Modify: `boot/compiler/codegen/emit/bridge_funcs.tw` (~lines 21, 41, 82)
- Modify: `boot/compiler/codegen/runtime/core.tw` (structural equality, ~lines 477-501)

- [ ] **Step 1: Coercions.** Replace the two hardcoded pairs (`box_i64`/`unbox_i64` at PVecI64↔PVec) with a family-dispatched form: given `from_name`/`to_name`, look up the family whose `pvec_type` participates and emit `fam.box_call` / `fam.unbox_call`. The anyref-erase box-before-erase branch (~line 47) generalizes the same way (`fname == fam.pvec_type` → `.Call(fam.box_call)`).

- [ ] **Step 2: Index fast path (`arrays.tw`).** Generalize `is_pvec_i64` to `pvec_family_of(vt, fams) ElemFamily?` (returns the family whose `pvec_type` names `vt`). In the fast path, use `StructGet(fam.pvec_type, 0)` and `.Call(fam.get)`'s runtime name, then coerce the result from `fam.elem_wasm` to the slot type (identity for i64→Int; i32→Bool is the Bool case handled by the existing scalar coercion).

- [ ] **Step 3: Closure trampoline downcast (`closures.tw`).** The `RefCast(false, .Named("rt_types__PVecI64"))` (~line 629) generalizes: when the capture param's physical type names a family `pvec_type`, cast to that. Loop families to match the param's `pvec_type`.

- [ ] **Step 4: Erased variant bridge (`bridge_funcs.tw`).** The recognizer (`name == "rt_types__PVecI64"`, line 21) → any `fam.pvec_type`; the box/unbox calls (lines 41, 82) → the matched `fam.box_call`/`fam.unbox_call`.

- [ ] **Step 5: Structural equality (`runtime/core.tw`).** The typed-ref recognizer (line 482) → any `fam.pvec_type`; both `rt_arr__box_i64` calls (lines 498, 501) → the matched `fam.box_call`. This boxes both operands to the universal `PVec` before content comparison, regardless of family.

- [ ] **Step 6: Verify no-op.** Only i64 registered. *[VL]* green + *[BG]* intact.

Run: *[VL]* then *[BG]*
Expected: `Ran 2980 tests` / `0 Failed`; checksums intact.

- [ ] **Step 7: Commit.**

```bash
git add boot/compiler/codegen/emit/coercions.tw boot/compiler/codegen/emit/arrays.tw boot/compiler/codegen/emit/closures.tw boot/compiler/codegen/emit/bridge_funcs.tw boot/compiler/codegen/runtime/core.tw
git commit -m "codegen: dispatch typed-vector box/unbox/index/equality per family"
```

### Task 6: Generalize storage-site layout + candidate policy

**Files:**
- Modify: `boot/compiler/codegen/wasm_layout.tw` (`is_int_vector_field` + literals, ~lines 59-69, 265-266, 312-313)
- Modify: `boot/compiler/backend/repr_policy.tw` (~lines 174, 364)

- [ ] **Step 1: `wasm_layout.tw`.** Replace `is_int_vector_field(field_ty)` guarding the field layout (line 265) and the variant-payload layout (line 312) with `elem_family_of(field_ty, families(builtins))`, and use the returned `fam.pvec_type` for the physical `.Ref(true, .Named(...))` type instead of the literal `"rt_types__PVecI64"`. (Thread `builtins`/`families` to these layout functions if not already present.)

- [ ] **Step 2: `repr_policy.tw`.** Extend the candidate-repr policy that currently admits only `Vector<Int>` to admit any element with a registered family (`elem_family_of` non-`None`). Keep the "inert until `val_type_of_mono` yields the typed repr" gating (line 364 comment) intact.

- [ ] **Step 3: Verify no-op.** Only i64 registered → identical layout decisions. *[VL]* green + *[BG]* intact.

Run: *[VL]* then *[BG]*
Expected: `Ran 2980 tests` / `0 Failed`; checksums intact.

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/codegen/wasm_layout.tw boot/compiler/backend/repr_policy.tw
git commit -m "codegen: field/payload layout + candidate policy per family"
```

**Stage 1 gate:** the module emitted for `boot/main.tw` and every bench should be byte-identical to the pre-Stage-1 build (registry unchanged in content). Confirm with:
```
git stash; make stage2 2>&1 | tail -1; cp target/boot.wasm /tmp/base.wasm; git stash pop
make stage2 2>&1 | tail -1; cmp target/boot.wasm /tmp/base.wasm && echo "BYTE-IDENTICAL"
```
Expected: `BYTE-IDENTICAL`. (If not identical, a Stage 1 change altered behavior — bisect the tasks.)

---

# Stage 2 — Add the Bool family

### Task 7: `ArrayBool` + `PVecBool` runtime types

**Files:**
- Modify: `boot/compiler/codegen/runtime/types.tw` (~lines 26, 38-54)
- Modify: `boot/compiler/codegen/runtime/arr.tw` (type-name consts + empty globals, ~lines 26, 89, 200-214)

- [ ] **Step 1: Declare the array + struct types** in `types.tw`, mirroring `ArrayI64`/`PVecI64` with `.I32` elements:

```
.Array("ArrayBool", .{ name: .None, mutable: true, ty: .I32 }),
```
  and the `PVecBool` struct with the same field shape as `PVecI64` (line 49) but `tail: .Ref(false, .Named("ArrayBool"))`.

- [ ] **Step 2: Add type-name consts + null/ref helpers** in `arr.tw`, cloning the `t_ARRAY_I64`/`t_PVEC_I64` block (lines 26, 89-100): `t_ARRAY_BOOL := "rt_types__ArrayBool"`, `t_PVEC_BOOL := "rt_types__PVecBool"`, `arr_bool_null()`, `pvec_bool_null()`, `pvec_bool_ref()`, `arr_bool_ref()`.

- [ ] **Step 3: Add empty globals** cloning `empty_leaf_i64`/`empty_pvec_i64` (lines 200-214) → `empty_leaf_bool` (`ArrayNewFixed(t_ARRAY_BOOL, 0)`) and `empty_pvec_bool`.

- [ ] **Step 4: Verify it compiles.** These types are unreferenced by routing yet, so behavior is unchanged; the module just gains type defs.

Run: `make stage2 2>&1 | tail -3`
Expected: `Fixed point reached`.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/codegen/runtime/types.tw boot/compiler/codegen/runtime/arr.tw
git commit -m "runtime: declare ArrayBool + PVecBool GC types"
```

### Task 8: `family_bool()` + PVecFamily-generic + mechanical `*_bool` clones

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw`

- [ ] **Step 1: Add `family_bool()`** cloning `family_i64()` (lines 1340-1355) with `.I32` elements:

```
fn family_bool() PVecFamily {
  .{
    len_name: "len_bool",
    get_name: "get_bool",
    builder_new_name: "builder_new_bool",
    builder_freeze_name: "builder_freeze_bool",
    pvec_ty: t_PVEC_BOOL,
    pvec_null: pvec_bool_null(),
    pvec_ref: pvec_bool_ref(),
    arr_ty: t_ARRAY_BOOL,
    arr_null: arr_bool_null(),
    elem_ty: .I32,
    empty_pvec_global: "empty_pvec_bool",
    zero_value: .I32Const(0),
  }
}
```

- [ ] **Step 2: Register the generic PVecFamily funcs for Bool.** In the emitted func list (`arr.tw` ~lines 152-164, where `family_i64().pvec_len_fn()` etc. appear), add `family_bool().pvec_len_fn()`, `.pvec_get_fn()`, `.pvec_builder_new_fn()`, `.pvec_builder_freeze_fn()`. These reuse the leaf-agnostic trie unchanged.

- [ ] **Step 3: Clone the non-generic wrappers** into the same list, each a copy of its `_i64` sibling with `t_ARRAY_I64→t_ARRAY_BOOL`, `t_PVEC_I64→t_PVEC_BOOL`, `.I64→.I32`, `.I64Const→.I32Const`, and the `_i64`→`_bool` name/callee. Source references:
  - `promote_full_tail_i64_fn` (line 1733) → `promote_full_tail_bool_fn`
  - `gather_i64_fn` (line 3717) → `gather_bool_fn` (its internal `get`/`push`/leaf ops become the `_bool` variants)
  - `vec_i64_roundtrip_fn` (line 4372) → `vec_bool_roundtrip_fn`
  - `builder_push_i64_raw_fn` (line 1936) → `builder_push_bool_raw_fn` (param `.I64`→`.I32`, `ArraySet(t_ARRAY_I64)`→`ArraySet(t_ARRAY_BOOL)`)

- [ ] **Step 4: Verify it compiles + still no-op.** Nothing routes to these yet. *[VL]* green.

Run: *[VL]*
Expected: `Ran 2980 tests` / `0 Failed`.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/codegen/runtime/arr.tw
git commit -m "runtime: PVecBool family descriptor + mechanical _bool clones"
```

### Task 9: `builder_push_bool` (i31 decode) + `box_bool`/`unbox_bool` (i31 encoding)

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw`

- [ ] **Step 1: `builder_push_bool_fn`.** Clone `builder_push_i64_fn` (line 1840) with the array/pvec/elem-type deltas from Task 8 Step 3, **except** the element-decode prologue. `builder_push_i64` does:
  ```
  .LocalGet(1), .RefCast(false, .Named(t_BOXED_INT)), .StructGet(t_BOXED_INT, 0), .LocalSet(5),
  ```
  Replace with the i31 decode (Bool is boxed as `ref.i31`), producing an `.I32` in the leaf slot:
  ```
  .LocalGet(1), .RefCast(false, .I31), .I31GetU, .LocalSet(5),
  ```
  and make local 5 `.I32` (not `.I64`), and the tail `ArraySet(t_ARRAY_BOOL)` writes the `.I32`. Everything else (tail-length i31 counter, `promote_full_tail_bool` at the 32-boundary) mirrors i64.

- [ ] **Step 2: `box_bool_fn` / `unbox_bool_fn`.** Clone `box_i64_fn` (line 2030) / `unbox_i64_fn` (line 2086). These walk the typed `PVecBool` and produce/consume a boxed `PVec` whose elements are `ref.i31`:
  - `box_bool`: where `box_i64` reads an `.I64` leaf and wraps it in a `BoxedInt` struct (`StructNew(t_BOXED_INT)`), instead read the `.I32` leaf and wrap with `.RefI31`.
  - `unbox_bool`: where `unbox_i64` casts an element to `BoxedInt` and `StructGet`s the `.I64`, instead `.RefCast(false, .I31)` + `.I31GetU` to an `.I32` leaf.

- [ ] **Step 3: Register both** in the emitted func list next to the Task 8 clones.

- [ ] **Step 4: Round-trip probe (TDD).** Create `examples/performance/sort-bench/bool_family_roundtrip_probe.tw` that builds a mixed `Vector<Bool>`, and (once Task 11 wires `families()`) asserts `unbox_bool(box_bool(v))` preserves contents. For now this task's check is that the funcs compile and boot-test stays green.

Run: *[VL]*
Expected: `Ran 2980 tests` / `0 Failed`.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/codegen/runtime/arr.tw examples/performance/sort-bench/bool_family_roundtrip_probe.tw
git commit -m "runtime: builder_push_bool (i31 decode) + box_bool/unbox_bool"
```

### Task 10: Register Bool helpers in `builtins.tw`

**Files:**
- Modify: `boot/compiler/builtins.tw` (ABI block ~lines 137-146; runtime-binding block ~lines 558-588)

- [ ] **Step 1: Add ABI entries** mirroring the `_i64` block (lines 138-146). Add helpers `pvec_bool_n()` / `pvec_bool_()` (clone `pvec_i64_n`/`pvec_i64_` at lines 81-86, naming `t_PVEC_BOOL`). Then:

```
"vector$len_bool" => abi([pvec_bool_n()], [.I32]),
"vector$get_bool" => abi([pvec_bool_n(), .I32], [.I32]),
"vector$builder_new_bool" => abi([], [arr_()]),
"vector$builder_push_bool" => abi([arr_n(), .Anyref], []),
"vector$builder_freeze_bool" => abi([arr_n()], [pvec_bool_()]),
"vector$gather_bool" => abi([pvec_bool_n(), pvec_n()], [pvec_bool_()]),
```

- [ ] **Step 2: Add runtime bindings** mirroring lines 562-588:

```
rt("vector$len_bool", "rt.arr", "len_bool", .None),
rt("vector$get_bool", "rt.arr", "get_bool", .None),
rt("vector$builder_new_bool", "rt.arr", "builder_new_bool", .None),
rt("vector$builder_push_bool", "rt.arr", "builder_push_bool", .None),
rt("vector$builder_freeze_bool", "rt.arr", "builder_freeze_bool", .None),
rt("vector$gather_bool", "rt.arr", "gather_bool", .None),
```
  (`builder_new_bool` reuses the generic `pvec_builder_new_fn` output named `builder_new_bool` from Task 8 Step 2.)

- [ ] **Step 3: Verify.** Builtins resolve; nothing routes to them yet (families still returns `[i64]`). *[VL]* green.

Run: *[VL]*
Expected: `Ran 2980 tests` / `0 Failed`.

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/builtins.tw
git commit -m "builtins: register Bool typed-vector helper ABI + bindings"
```

### Task 11: Activate the Bool family (registry + `elem_family_of` + runtime_abi + read probes)

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw` (`families()`, `elem_family_of`)
- Modify: `boot/compiler/codegen/emit/runtime_abi.tw` (typed-builder name lists, ~lines 29-50)
- Create: `examples/performance/sort-bench/typed_bool_read_probe.tw`
- Create: `examples/performance/sort-bench/typed_bool_boxed_probe.tw`

- [ ] **Step 1: Write the positive read probe (TDD, fails first).** Mirror `typed_record_field_probe.tw`. Build a `Vector<Bool>` with `collect`, read via index/len, print a checksum.

```
// typed_bool_read_probe.tw
mask := collect i in range(1000) { i % 3 == 0 }
count := 0

for i in range(mask.len()) {
  if mask[i] {
    count = count + 1
  }
}

println("bool_count=${count}")
```

- [ ] **Step 2: Confirm it is boxed BEFORE activation.** The codegen must NOT contain `get_bool` yet.

Run: `target/twk wat examples/performance/sort-bench/typed_bool_read_probe.tw --func '$main' --calls | grep -c get_bool`
Expected: `0` (routing not active).

- [ ] **Step 3: Add the Bool family to the registry.**

```
pub fn families(builtins: BuiltinRegistry) Vector<ElemFamily> {
  [family_i64_desc(builtins), family_bool_desc(builtins)]
}
```
  Add `family_bool_desc(builtins)` cloning `family_i64_desc` with `mono_key: "vec_bool"`, `pvec_type: "rt_types__PVecBool"`, `elem_wasm: .I32`, the `*_bool` builtin ids, `box_call: "rt_arr__box_bool"`, `unbox_call: "rt_arr__unbox_bool"`.

- [ ] **Step 4: Map `.Vector(.Bool)` in `elem_family_of`.**

```
key := case mono {
  .Vector(.Int) => "vec_i64",
  .Vector(.Bool) => "vec_bool",
  _ => "other",
}
```

- [ ] **Step 5: Extend the typed-builder name lists** in `runtime_abi.tw` (lines 29-50) to include `"vector$builder_push_bool"`, `"vector$builder_new_bool"`, `"vector$builder_freeze_bool"` alongside the `_i64` names (builder-seed + element handling).

- [ ] **Step 6: Rebuild and confirm the probe is now typed.**

Run: `make bundle-cli 2>&1 | tail -1 && target/twk wat examples/performance/sort-bench/typed_bool_read_probe.tw --func '$main' --calls | grep -c get_bool`
Expected: `Fixed point reached`; `get_bool` count `>= 1`.

- [ ] **Step 7: Write the negative probe** `typed_bool_boxed_probe.tw`: the same `Vector<Bool>` produced by a combinator (`append` in a loop) passed through a parameter — must stay boxed (no `get_bool`).

```
// typed_bool_boxed_probe.tw
fn build(n: Int) Vector<Bool> {
  acc: Vector<Bool> = []

  for i in range(n) {
    acc = acc.append(i % 3 == 0)
  }

  acc
}

fn count_true(xs: Vector<Bool>) Int {
  c := 0
  for i in range(xs.len()) { if xs[i] { c = c + 1 } }
  c
}

println("bool_count=${count_true(build(1000))}")
```

Run: `target/twk wat examples/performance/sort-bench/typed_bool_boxed_probe.tw --func count_true --calls | grep -c get_bool`
Expected: `0` (boxed producer through a param stays boxed).

- [ ] **Step 8: Run both probes for correctness + full suite.**

Run: `target/twk run examples/performance/sort-bench/typed_bool_read_probe.tw && target/twk run examples/performance/sort-bench/typed_bool_boxed_probe.tw && make boot-test 2>&1 | grep -iE "Ran [0-9]+ tests|Failed"`
Expected: both print `bool_count=334`; `Ran 2980 tests` / `0 Failed`.

- [ ] **Step 9: Commit.**

```bash
git add boot/compiler/backend/route_typed_vec.tw boot/compiler/codegen/emit/runtime_abi.tw examples/performance/sort-bench/typed_bool_read_probe.tw examples/performance/sort-bench/typed_bool_boxed_probe.tw
git commit -m "backend: activate PVecBool family + typed Bool read probes"
```

### Task 12: Verify typed Bool captures/params (the nulls-comparator path)

**Files:**
- Create: `examples/performance/sort-bench/typed_bool_capture_probe.tw`
- Create: `examples/performance/sort-bench/typed_mixed_capture_probe.tw`

- [ ] **Step 1: Capture probe.** A comparator captures a typed `Vector<Bool>` mask and reads it, mirroring the null-aware sort. Assert the closure body reads via `get_bool` (typed env), not boxed.

```
// typed_bool_capture_probe.tw
use @std.sort

mask := collect i in range(1000) { i % 2 == 0 }
idx := collect i in range(1000) { i }
sorted := idx.sort_by(fn(a, b) {
  ma := if mask[a] { 1 } else { 0 }
  mb := if mask[b] { 1 } else { 0 }
  Int.compare(ma, mb)
})
println("first=${sorted[0]} last=${sorted[999]}")
```

- [ ] **Step 2: Confirm the comparator reads typed.** Find the comparator's mangled name, then check its calls.

Run: `target/twk wat examples/performance/sort-bench/typed_bool_capture_probe.tw --func sort_by --list` then `... --func <comparator> --calls | grep -c get_bool`
Expected: `>= 1` (captured mask is `PVecBool` in the env; typed read).

- [ ] **Step 3: Mixed Int+Bool capture probe** — one comparator capturing both an Int key column and a Bool mask, exercising the family-keyed capture ABI (each capture keeps its own family).

```
// typed_mixed_capture_probe.tw
keys := collect i in range(1000) { (i * 7) % 1000 }
mask := collect i in range(1000) { i % 2 == 0 }
idx := collect i in range(1000) { i }
sorted := idx.sort_by(fn(a, b) {
  if mask[a] != mask[b] {
    if mask[a] { -1 } else { 1 }
  } else {
    Int.compare(keys[a], keys[b])
  }
})
println("first=${sorted[0]}")
```

Run: `target/twk wat examples/performance/sort-bench/typed_mixed_capture_probe.tw --func <comparator> --calls | grep -cE "get_i64|get_bool"`
Expected: both `get_i64` and `get_bool` present (Int capture typed as PVecI64, Bool capture typed as PVecBool — no cross-family confusion).

- [ ] **Step 4: Correctness + suite.**

Run: `target/twk run examples/performance/sort-bench/typed_bool_capture_probe.tw && target/twk run examples/performance/sort-bench/typed_mixed_capture_probe.tw && make boot-test 2>&1 | grep -iE "Ran [0-9]+ tests|Failed"`
Expected: probes run without trap; `Ran 2980 tests` / `0 Failed`.

- [ ] **Step 5: Commit.**

```bash
git add examples/performance/sort-bench/typed_bool_capture_probe.tw examples/performance/sort-bench/typed_mixed_capture_probe.tw
git commit -m "test: typed Bool capture + mixed Int/Bool capture probes"
```

### Task 13: Verify field/payload layout, erased bridge, structural equality, gather

**Files:**
- Create: `examples/performance/sort-bench/typed_bool_field_payload_probe.tw`

- [ ] **Step 1: Field + payload + equality + gather probe.** One program covering all four non-route sites:

```
// typed_bool_field_payload_probe.tw
type Col = .{ nulls: Vector<Bool> }
type Tagged = { Present(Vector<Bool>), Absent }

fn mk() Col {
  .{ nulls: collect i in range(100) { i % 4 == 0 } }
}

c := mk()
c2 := mk()

// field read (typed PVecBool field)
n := 0
for i in range(c.nulls.len()) { if c.nulls[i] { n = n + 1 } }

// structural equality over records holding a typed Bool vector
eq := c == c2

// variant payload + erased bridge round-trip
t: Tagged = .Present(c.nulls)
extracted := case t {
  .Present(v) => v,
  .Absent => [],
}
m := 0
for i in range(extracted.len()) { if extracted[i] { m = m + 1 } }

// gather over a typed Bool vector
idx := collect i in range(50) { i * 2 }
g := c.nulls.gather(idx)

println("n=${n} eq=${eq} m=${m} glen=${g.len()}")
```

- [ ] **Step 2: Confirm the field layout is PVecBool.**

Run: `target/twk build examples/performance/sort-bench/typed_bool_field_payload_probe.tw -o /tmp/bf.wat && grep -c "PVecBool" /tmp/bf.wat`
Expected: `>= 1` (the `Col.nulls` field and/or `Tagged.Present` payload typed as `PVecBool`).

- [ ] **Step 3: Confirm gather routes typed.**

Run: `target/twk wat examples/performance/sort-bench/typed_bool_field_payload_probe.tw --func '$main' --calls | grep -c gather_bool`
Expected: `>= 1`.

- [ ] **Step 4: Correctness (equality + bridge + gather all sound).**

Run: `target/twk run examples/performance/sort-bench/typed_bool_field_payload_probe.tw`
Expected: `n=25 eq=true m=25 glen=50`.

- [ ] **Step 5: Full suite.**

Run: `make boot-test 2>&1 | grep -iE "Ran [0-9]+ tests|Failed"`
Expected: `Ran 2980 tests` / `0 Failed`.

- [ ] **Step 6: Commit.**

```bash
git add examples/performance/sort-bench/typed_bool_field_payload_probe.tw
git commit -m "test: typed Bool field/payload layout, bridge, equality, gather"
```

### Task 14: End-to-end — dataframe null-aware sort win

**Files:** none (measurement + final gate)

- [ ] **Step 1: Confirm the dataframe `nulls` column now types.** Inspect the null-aware sort path in the dataframe bench.

Run: `target/twk wat examples/performance/dataframe/bench/order_by_breakdown.tw --func <null_aware_sort_fn> --calls | grep -c get_bool`
Expected: `>= 1` (the mask read is typed). (Discover the fn name with `--list`.)

- [ ] **Step 2: Run the breakdown bench.** Confirm checksums intact and the null-aware `sort idx + nulls` phase dropped from ~1344ms toward the boxing-free ~732ms floor.

Run: `target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw`
Expected: checksums `1000000` / `3000000`; the `sort idx + nulls` line materially lower than the recorded ~1344ms baseline; `full order_by` improved.

- [ ] **Step 3: Full verification loop + capture guard.**

Run: *[VL]* then `target/twk run examples/performance/sort-bench/typed_payload_capture_guard.tw`
Expected: `Fixed point reached`; `Ran 2980 tests` / `0 Failed`; guard runs fast without a per-read-unbox miscompile.

- [ ] **Step 4: Update the tracklist + folder docs.** In `boundary-tracklist.md`, mark B8's Bool portion done and note `PVecBool` landed; in this folder's `README.md`, flip the plan row status from `design` to reflect the landed state; add a `project_typed_vector_repr` MEMORY note update for the Bool family. Commit.

```bash
git add docs/plans/performance/vector/boundary-tracklist.md docs/plans/performance/vector/README.md docs/plans/performance/vector/typed-vector-elem-families-plan.md
git commit -m "docs: PVecBool family landed — null-aware sort typed"
```

---

## Self-review checklist (run before execution)

- **Spec coverage:** Family metadata flow → Task 3; `builder_push_bool`/`box_bool` i31 → Task 9; storage-site layout → Task 6; bridge/equality → Task 5; registration wiring → Tasks 7-10; per-family pass → Tasks 1-2; all six test gaps → Tasks 11-13. ✔
- **Byte-identity gate** after Stage 1 catches any accidental behavior change from the refactor.
- **Ordering:** the ABI-fact map-type change (Task 3) may force Tasks 2+3 into one commit if the compiler rejects the intermediate type mismatch — Task 2 Step 1 flags this. Land them together if so.
- **`box`/`unbox` are runtime fn names** (`rt_arr__box_bool`), not builtin ids — reflected in `ElemFamily.box_call`/`unbox_call` (Task 1) and used by coercions/bridge/equality (Task 5).
