# Milestone 1a — Typed variant/record vector representation — Implementation Plan

> **SUPERSEDED (2026-07-04).** This plan executed the uniform-typing activation
> (`Vector<Int> = PVecI64` everywhere), which was built, measured, and reverted —
> it makes captured-vector reads O(n) per access (see
> [vector/m1a-anyref-readback-investigation.md](vector/archive/m1a-anyref-readback-investigation.md)).
> The corrected direction (typed vectors as a *storage-site* representation) is in
> [representation-boundary-policy.md](representation-boundary-policy.md). Kept for
> the record; do not execute as written. T0–T3 (the reusable infrastructure) still
> stand; only Task 6/7 (the activation) were reverted.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `Vector<Int>` a first-class typed physical representation (`TypedVec(I64)` → `rt_types__PVecI64`) that stays typed through record fields and variant payloads, with bidirectional coercions at universal-ABI boundaries handled by the existing `emit_coerce_stack` layer, replacing the conservative `route_typed_vec` bolt-on.

**Architecture:** *Build-then-activate.* `repr_of_mono` cannot flip `Vector<Int> → TypedVec(I64)` incrementally and stay self-host-green, because that flip simultaneously requires typed layout, typed helper selection, and boundary coercions. So this plan **builds every component while the legacy `route_typed_vec` pass keeps running unchanged** (self-host + existing S2 typed-vector behavior stay green throughout), then a single **activation task** flips the classifier + valtype, teaches literals, and *disables* the now-redundant legacy pass. This mirrors how S2.0 landed (built, then activated).

**Tech Stack:** Boot compiler (self-hosted Twinkle in `boot/`). Verification loop: `make bundle-cli` (self-host to fixed point), `make boot-test` (boot suite), and `.tw` probes built to `.wat` then grepped. Boot-only — no stage0 changes (`make bundle-cli` compiles the new boot via stage0 as ordinary Twinkle; the self-host fixed point is the gate).

**Spec:** [representation-boundary-policy.md](representation-boundary-policy.md) (Milestone 1a).

---

## Key facts discovered during planning (do not re-derive)

- **Coercion already lives in `emit_coerce_stack`** (`codegen/emit/coercions.tw`,
  signature `(from: ValType, to: ValType, mono, ctx, buf)`, 17 call sites across
  records/variants/calls). It **already** emits `box_i64` for `PVecI64 → PVec`
  (`is_vector_int(mono)` guarded) and routes `to: .Anyref` through
  `emit_box_to_anyref`. The reverse (`PVec → PVecI64`) and the
  `PVecI64 → .Anyref` (box-then-erase) cases are what's missing.
- **`route_typed_vec` today sets `wasm_type = PVecI64` but leaves `repr` as
  `TypedRef`** (`with_repr_wasm(info, info.repr, PVecI64)`), so pre-activation the
  typed-vector marker is the *wasm type*, not the repr. Any pre-activation code
  must treat "typed vec" as `repr == TypedVec(I64) OR wasm_type == PVecI64`.
- **`builder_push_i64` is `(Array?, anyref)`** and unboxes `BoxedInt` internally;
  the builder handle is a boxed `Array?`, not a `PVecI64`. Do not pass raw `i64`.
- **`emit_array_literal` (`emit/arrays.tw`) always emits `StructNew("rt_types__PVec")`**
  and ignores the result valtype — `Vector<Int>` literals need explicit handling at
  activation.
- **Variant/record payload valtypes already flow through `val_type_of_mono`**
  (`wasm_layout.tw:311` for sum defs; records similarly), so making
  `val_type_of_mono(Vector<Int>) = PVecI64` makes typed payloads fall out.

## Note on granularity

Data-type/runtime tasks carry complete code. The compiler-transform tasks
(coercion cases, helper selection, activation) are specified as **exact
file/function target + concrete test gate** rather than fabricated line-by-line
code — the code is emergent and interdependent, and the *test* (a probe built to
WAT and grepped, plus self-host + boot suite) is the real contract.

## File structure

| File | Responsibility | Task |
|------|----------------|------|
| `boot/compiler/backend/repr_policy.tw` (new) | Pure `MonoType → ElemRepr?` classification, importable by `repr_assign` and `wasm_layout` without a cycle | T0 |
| `boot/compiler/backend/prepared_ir.tw` | Add `ElemRepr` + `ReprKind::TypedVec(ElemRepr)` | T1 |
| `boot/compiler/codegen/runtime/arr.tw` | `unbox_i64` runtime fn (`PVec → PVecI64`); register it | T2 |
| `boot/compiler/codegen/emit/coercions.tw` | Extend `emit_coerce_stack`: reverse `unbox_i64` + `PVecI64 → .Anyref` box-then-erase | T3 |
| `boot/compiler/codegen/emit/{arrays,runtime_abi,calls}.tw`, `builtins.tw` | Repr/wasm-driven `_i64` helper selection (transitional OR rule) | T4 |
| `boot/compiler/backend/{verify_slots,verify_expr}.tw` | Accept `TypedVec(I64) ⇔ PVecI64`; require coercions across reprs | T5 |
| `boot/compiler/backend/repr_assign.tw`, `codegen/wasm_layout.tw`, `codegen/emit/arrays.tw`, `backend/prepare.tw` | **Activation:** flip classifier + valtype, teach literals, disable legacy pass | T6 |
| `examples/performance/sort-bench/*.tw` | Probes + regression gate | T7 |

---

## Task 0: Shared repr-policy module (break the cycle)

**Why first:** `repr_assign.tw` imports `wasm_layout.tw`; activation needs
`wasm_layout` to consult element repr. Putting the classifier in `repr_assign`
would force `wasm_layout → repr_assign → wasm_layout`. The vector element-repr
classification is a *pure* function of `MonoType` (no `layout_of`), so it extracts
cleanly into a lower module both import.

**Files:**
- Create: `boot/compiler/backend/repr_policy.tw`
- Test: `boot/tests/suites/repr_policy_suite.tw` (new; register in the suite index / `boot/tests/main.tw`)

- [ ] **Step 1: Write the failing test**

```tw
// boot/tests/suites/repr_policy_suite.tw
use compiler.backend.repr_policy.{elem_repr_of_vector}
use compiler.mono_type.{MonoType}

pub fn test_repr_policy() {
  assert.eq(elem_repr_of_vector(.Vector(.Int)), .Some(.I64), "vec<int> -> I64")
  assert.eq(elem_repr_of_vector(.Vector(.Float)), .None, "vec<float> unsupported in M1a")
  assert.eq(elem_repr_of_vector(.Vector(.String)), .None, "vec<string> stays boxed")
  assert.eq(elem_repr_of_vector(.Int), .None, "non-vector -> None")
}
```

(Match the boot suite's actual `assert` API when registering; adjust the
`assert.eq` call shape if the suite uses a different helper.)

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — `repr_policy` / `elem_repr_of_vector` undefined.

- [ ] **Step 3: Implement the module**

```tw
// boot/compiler/backend/repr_policy.tw
//! Physical-representation policy shared by repr_assign and wasm_layout.
//!
//! Pure functions of MonoType only — deliberately no dependency on wasm_layout
//! (layout_of), so both repr_assign and wasm_layout can import this without a
//! cycle. Named/aggregate repr still lives in repr_assign (it needs layout_of);
//! only the primitive vector element-family classification lives here.
use compiler.mono_type.{MonoType}

// The primitive storage class of a typed vector's elements. M1a implements only
// I64; F64/I32 are reserved so ReprKind.TypedVec(ElemRepr) keeps its final shape.
pub type ElemRepr = { I64, F64, I32 }

// elem_repr_of_vector returns the typed element family for a Vector<T> whose T is
// a currently-supported primitive, else .None (the vector stays boxed TypedRef).
// M1a: only Vector<Int> -> .Some(.I64).
pub fn elem_repr_of_vector(mono: MonoType) ElemRepr? {
  case mono {
    .Vector(.Int) => .Some(.I64),
    _ => .None,
  }
}
```

`ElemRepr` is defined here (single definition); `prepared_ir.tw` imports it in T1.

- [ ] **Step 4: Run to verify it passes**

Run: `target/twk run boot/tests/main.tw` → PASS.

- [ ] **Step 5: Self-host + commit**

```bash
make bundle-cli && make boot-test
git add boot/compiler/backend/repr_policy.tw boot/tests/suites/repr_policy_suite.tw boot/tests/main.tw
git commit -m "backend: add pure repr_policy module (ElemRepr + vector classification)"
```

Expected: `make bundle-cli` prints "Fixed point reached"; boot suite green.

---

## Task 1: Add `TypedVec(ElemRepr)` to `ReprKind`

**Files:**
- Modify: `boot/compiler/backend/prepared_ir.tw:47` (the `ReprKind` enum) + import `ElemRepr`
- Modify: every exhaustive `case` on `ReprKind` (find them — Step 2)

- [ ] **Step 1: Add the variant**

```tw
use compiler.backend.repr_policy.{ElemRepr}

pub type ReprKind = {
  I64,
  F64,
  I32,
  TypedRef(MonoType),
  TypedSum(MonoType),
  TypedVec(ElemRepr),   // typed primitive vector family (M1a: only .I64)
  ClosureRef,
  ErasedSum,
  OpaqueAnyref,
  DeadValue,
}
```

- [ ] **Step 2: Add a `TypedVec` arm to every exhaustive match**

Run: `grep -rn "ReprKind\|\.TypedRef(\|\.OpaqueAnyref" boot/compiler/ | grep -v "//"`
For each exhaustive `case` on a `ReprKind`, add `.TypedVec(_) => …` mirroring the
`TypedRef` arm (a `TypedVec` is a typed ref like `TypedRef`). Sites with `_ =>`
need no change. Nothing produces `TypedVec` yet, so behavior is unchanged.

- [ ] **Step 3: Self-host to prove exhaustiveness**

Run: `make bundle-cli` → reaches fixed point (no non-exhaustive `case` errors).

- [ ] **Step 4: Commit**

```bash
make boot-test
git add boot/compiler/backend/prepared_ir.tw boot/compiler/**/*.tw
git commit -m "backend: add ReprKind.TypedVec(ElemRepr) variant (inert)"
```

---

## Task 2: `unbox_i64` runtime adapter (`PVec → PVecI64`)

**Why:** `emit_coerce_stack` already boxes `PVecI64 → PVec`; the reverse re-types a
boxed `PVec` returning from a universal helper. A raw `ref.cast` traps (different
leaf array types), so it rebuilds.

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw` (add `unbox_i64_fn`; register near `box_i64_fn()` at line ~160)

- [ ] **Step 1: Implement `unbox_i64_fn` (mirror of `box_i64_fn`, inverted)**

Correct shape (verified against `box_i64_fn` and `builder_push_i64`): the builder
handle is a boxed `Array?` from `builder_new_i64`; push the **boxed** element
straight in — `builder_push_i64(builder, get(vec, i))` — since `builder_push_i64`
is `(Array?, anyref)` and unboxes `BoxedInt` itself; finish with
`builder_freeze_i64(builder)` which yields the `PVecI64`. Read the source with the
boxed `len`/`get` runtime ops (the ones `box_i64` reads *from* on the typed side,
here used on the boxed side).

```tw
// boot/compiler/codegen/runtime/arr.tw
// unbox_i64(vec: PVec?) -> PVecI64
// Reverse of box_i64: rebuild a typed PVecI64 from a boxed PVec. Reads the boxed
// vector element-by-element and pushes through the typed builder (which unboxes
// BoxedInt internally), so a boxed Vector<Int> result from a universal helper can
// re-enter typed code.
fn unbox_i64_fn() FuncDef {
  .{
    name: "unbox_i64",
    params: [pvec_ref()],        // boxed PVec in
    results: [pvec_i64_null()],  // typed PVecI64 out
    locals: [.I32, .I32, arr_null()],  // n, i, builder (boxed Array handle)
    body: [
      // n   = <boxed len>(vec)
      // i   = 0
      // b   = builder_new_i64()            // returns Array
      // loop i<n: builder_push_i64(b, <boxed get>(vec, i)); i = i+1
      // return builder_freeze_i64(b)       // yields PVecI64
      // (fill from box_i64_fn's loop skeleton, inverted; reuse its len/get calls)
    ],
  }
}
```

- [ ] **Step 2: Register + self-host**

Add `unbox_i64_fn()` to the runtime fn list beside `box_i64_fn()`.
Run: `make bundle-cli` → fixed point. (`unbox_i64` is dead until T3 references it.)

- [ ] **Step 3: Commit**

```bash
git add boot/compiler/codegen/runtime/arr.tw
git commit -m "runtime: add unbox_i64 adapter (PVec -> PVecI64), inert"
```

---

## Task 3: Extend `emit_coerce_stack` (reverse + anyref-erase)

**Why:** `emit_coerce_stack` is the established stack-coercion layer and already
has the source/target ValTypes — the correct place for both missing cases. This
replaces the earlier (wrong) idea of inserting coercion ANF nodes in
`route_typed_vec`, and resolves the "`emit_box_to_anyref` by mono is insufficient"
trap: the rule keys off the *source valtype*, which `emit_coerce_stack` has.

**Files:**
- Modify: `boot/compiler/codegen/emit/coercions.tw` (`emit_coerce_stack`)

- [ ] **Step 1: Add the reverse case `PVec → PVecI64`**

Alongside the existing `from PVecI64 → to PVec ⇒ box_i64` case, add: `from` is
`rt_types__PVec`, `to` is `rt_types__PVecI64`, `is_vector_int(mono)` ⇒
`buf.append(.Call("rt_arr__unbox_i64"))`.

- [ ] **Step 2: Add the anyref-erase case `PVecI64 → .Anyref`**

Before the generic `to: .Anyref => emit_box_to_anyref(...)` arm, special-case a
`from` of `rt_types__PVecI64` going `to: .Anyref`: emit `box_i64` first (yielding a
`PVec`, which is an `anyref`-compatible ref), *then* the normal erase. This is the
guard for the identity trap — do **not** upcast a `PVecI64` straight to `anyref`.
Leave `emit_box_to_anyref` itself untouched (its `.Vector_ => buf` identity is
correct for an already-boxed `PVec`).

- [ ] **Step 3: Self-host (inert pre-activation)**

Run: `make bundle-cli && make boot-test`
Expected: fixed point + green. Pre-activation, legacy-typed vectors are
non-escaping (they never reach a record/variant/anyref boundary), so these new
cases don't fire — behavior is unchanged.

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/codegen/emit/coercions.tw
git commit -m "emit: emit_coerce_stack handles PVec<->PVecI64 both ways + anyref erase"
```

---

## Task 4: Repr/wasm-driven `_i64` helper selection (transitional OR rule)

**Why:** After activation, `_i64` builder/push/freeze/len/get selection must key
off the typed marker. Because the legacy pass marks typed vectors by *wasm type*
(`PVecI64`) not repr, the transitional rule must accept **either** signal so the
existing probes keep passing before activation.

**Files:**
- Modify: `boot/compiler/codegen/emit/{arrays,runtime_abi,calls}.tw`, `boot/compiler/builtins.tw`

- [ ] **Step 1: Gate = existing typed probes still pass**

```bash
target/twk build examples/performance/sort-bench/typed_record_field_probe.tw -o /tmp/trf.wat
grep -c rt_arr__get_i64 /tmp/trf.wat   # expect >= 1
target/twk run examples/performance/sort-bench/typed_vec_read_probe.tw  # match=true, ~7x gap
```

- [ ] **Step 2: Make selection use `is_typed_vec(slot)` = `repr == TypedVec(I64) OR wasm_type == PVecI64`**

Introduce a small predicate `is_typed_vec_slot(info)` returning true when
`info.repr` is `.TypedVec(.I64)` **or** `info.wasm_type` is
`.Ref(_, .Named("rt_types__PVecI64"))`. Route builder/push/freeze/len/get `_i64`
selection through it. Reads already dispatch on the base wasm type
(`emit/arrays.tw` `is_pvec_i64`) — leave those, they already satisfy the OR.

- [ ] **Step 3: Self-host + probes**

Run: `make bundle-cli && make boot-test` then Step 1's commands.
Expected: fixed point, green, probes unchanged (`match=true`, `get_i64` present).

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/codegen/emit/*.tw boot/compiler/builtins.tw
git commit -m "emit: _i64 helper selection via repr OR PVecI64 wasm type (transitional)"
```

---

## Task 5: Generalize the verifier

**Files:**
- Modify: `boot/compiler/backend/verify_slots.tw` (`is_typed_vec_i64`), `boot/compiler/backend/verify_expr.tw` (`pvec_repr_mismatch`)

- [ ] **Step 1: Extend the rule**

Accept `TypedVec(I64)` repr paired with a `PVecI64` wasm type (generalize the
existing `is_typed_vec_i64`, which today keys off wasm type only). Keep the
`pvec_repr_mismatch` store check. Coercions are emit-time (T3), so the verifier's
job here is slot-level consistency (`repr` and `wasm_type` agree), not coercion
node presence.

- [ ] **Step 2: Self-host (inert)**

Run: `make bundle-cli && make boot-test` → fixed point + green.

- [ ] **Step 3: Commit**

```bash
git add boot/compiler/backend/verify_slots.tw boot/compiler/backend/verify_expr.tw
git commit -m "verify: accept TypedVec(I64) repr paired with PVecI64"
```

---

## Task 6: **Activation** — flip `Vector<Int>` to the typed family

**Why:** Turns everything on at once and disables the now-redundant legacy pass.
This is the integration gate. (This also *pulls the spec's M2 "delete
`route_typed_vec`" forward* to a *disable* here, since `repr_of_mono` +
`emit_coerce_stack` now supply what the legacy pass used to.)

**Files:**
- Modify: `boot/compiler/backend/repr_assign.tw:231,260` (`repr_of_mono`, `repr_of_named_cached`)
- Modify: `boot/compiler/codegen/wasm_layout.tw:523,427` (`val_type_of_mono`, `layout_hint_of`) + the `layout_of` vector arm
- Modify: `boot/compiler/codegen/emit/arrays.tw` (`emit_array_literal`)
- Modify: `boot/compiler/backend/prepare.tw:88` (skip the legacy `route_typed_vectors` call)

- [ ] **Step 1: Flip the classifier and valtype**

- `repr_of_mono` (`repr_assign.tw:231`) `.Vector(elem)`: consult
  `repr_policy.elem_repr_of_vector`; `.Some(er) => .TypedVec(er)` else
  `.TypedRef(mono)`. Same for `repr_of_named_cached`'s `.Vector_` arm (`:260`).
- `val_type_of_mono` (`wasm_layout.tw:523`) and `layout_hint_of` (`:427`): for
  `Vector<Int>` return `.Ref(true, .Named("rt_types__PVecI64"))`. Variant/record
  payloads then follow automatically via `val_type_of_mono(field_ty)`
  (`wasm_layout.tw:311`).

- [ ] **Step 2: Teach `Vector<Int>` literals (the `AArrayLit` gap)**

`emit_array_literal` always builds a boxed `PVec`. For a `Vector<Int>` result,
either (a) build through the typed builder (`builder_new_i64` → per-element
`builder_push_i64` → `builder_freeze_i64`) so the literal is born `PVecI64`, or
(b) build the boxed `PVec` as today and append `unbox_i64`. Prefer (a). Gate: a
`[1,2,3]`-into-typed-slot probe validates (Step 4).

- [ ] **Step 3: Disable the legacy pass**

In `prepare.tw`, stop calling `route_typed_vectors` (line ~88). `repr_of_mono` now
supplies `TypedVec` repr + `PVecI64` wasm type directly, and `emit_coerce_stack`
supplies boundary coercions, so the escape-based pass is redundant. (Keep the file
for M2's reference; just remove it from the pipeline.)

- [ ] **Step 4: Self-host — the real integration test**

Run: `make bundle-cli`
Expected: "Fixed point reached". If it traps or fails to converge, the gap is in
coercion (T3), helper selection (T4), literals (Step 2), or a missed boundary —
debug there. **Do not proceed until the fixed point is green.**

- [ ] **Step 5: Full boot suite**

Run: `make boot-test` → all green (was ~2820+; no regressions).

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/backend/repr_assign.tw boot/compiler/codegen/wasm_layout.tw boot/compiler/codegen/emit/arrays.tw boot/compiler/backend/prepare.tw
git commit -m "activate: Vector<Int> is TypedVec(I64)/PVecI64 everywhere; disable legacy route"
```

---

## Task 7: Probes + regression gate

**Files:**
- Use: `examples/performance/sort-bench/typed_variant_column_probe.tw` (exists)
- Create: `examples/performance/sort-bench/typed_variant_column_boxed_probe.tw` (negative)

- [ ] **Step 1: Variant payload routes typed (positive)**

```bash
target/twk build examples/performance/sort-bench/typed_variant_column_probe.tw -o /tmp/tvc.wat
grep -c rt_arr__get_i64 /tmp/tvc.wat     # expect >= 1 (was 0 before activation)
target/twk run examples/performance/sort-bench/typed_variant_column_probe.tw   # match=true
```

- [ ] **Step 2: Negative probe stays boxed**

Author `typed_variant_column_boxed_probe.tw` with a variant column that must stay
boxed (`StrCol(Vector<String>)`). Build to WAT; assert `grep -c rt_arr__get_i64`
on that field's read is `0`.

- [ ] **Step 3: No `order_by` regression**

Run: `target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw`
Expected: `gather`/`take`/`sort` within noise of the pre-M1a baseline (N=1M: sort
~1317ms, gather 3 cols ~412ms, take ~418ms, full ~2327ms). They do **not** improve
here (sort → M1b, gather → M3); the gate is *no material regression* from the new
boundary coercions.

- [ ] **Step 4: Commit**

```bash
git add examples/performance/sort-bench/typed_variant_column_boxed_probe.tw
git commit -m "probes: variant-payload typed positive/negative + order_by regression gate"
```

---

## Definition of done (M1a)

- `make bundle-cli` reaches fixed point; `make boot-test` fully green.
- `typed_variant_column_probe`: `get_i64` present, `match=true`.
- Negative probe: boxed field stays `PVec` (no `get_i64`).
- `order_by_breakdown` shows no material regression.
- Coercion lives in `emit_coerce_stack` (both directions + anyref-erase);
  `unbox_i64` exists; verifier accepts `TypedVec(I64)`; the legacy
  `route_typed_vectors` pass is out of the pipeline.

Deferred to **M1b**: typed closure-env layouts (the `sort` win). Deferred to
**M3**: typed `gather`/`sort`/`map` (the `gather`/`take` win). **Spec
reconciliation:** M1a disables `route_typed_vec` (M2's deletion is now just tidy-up
+ making `insert_boundaries` repr-aware).
