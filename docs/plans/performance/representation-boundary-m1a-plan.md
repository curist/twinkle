# Milestone 1a — Typed variant/record vector representation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `Vector<Int>` a first-class typed physical representation (`TypedVec(I64)` → `rt_types__PVecI64`) that stays typed through record fields and variant payloads, with bidirectional coercions only at universal-ABI boundaries, replacing the conservative `route_typed_vec` bolt-on's eligibility with a repr-driven model.

**Architecture:** *Build-then-activate.* `repr_of_mono` cannot flip `Vector<Int> → TypedVec(I64)` incrementally and stay self-host-green, because that flip simultaneously requires typed layout, typed helper selection, and boundary coercions. So this plan **builds every component behind unit tests while nothing yet produces `TypedVec`** (self-host stays green throughout), then a single **activation task** flips the classifier + valtype and wires the coercion inserter. This mirrors how S2.0 landed (committed gated-off, then activated).

**Tech Stack:** Boot compiler (self-hosted Twinkle in `boot/`). Verification loop: `make bundle-cli` (self-host to fixed point), `make boot-test` (boot suite), and `.tw` probes built to `.wat` then grepped. Boot-only — no stage0 changes (`make bundle-cli` compiles the new boot source via stage0 as ordinary Twinkle; the self-host fixed point is the gate).

**Spec:** [representation-boundary-policy.md](representation-boundary-policy.md) (Milestone 1a).

---

## Note on granularity

The data-type and runtime-function tasks below carry complete code (they are
verifiable in isolation). The compiler-transform tasks (helper selection,
coercion insertion, verifier, activation) are specified as **exact file/function
targets + a concrete test gate** rather than fabricated line-by-line code: the
precise code is emergent and interdependent, and the *test* (a probe built to WAT
and grepped, plus self-host + boot suite) is the real contract. Every such task
names the function to change and the exact command whose output gates it.

## File structure

| File | Responsibility | Task |
|------|----------------|------|
| `boot/compiler/backend/repr_policy.tw` (new) | Pure `MonoType → ElemRepr?` / `TypedVec` classification, importable by both `repr_assign` and `wasm_layout` without a cycle | T0 |
| `boot/compiler/backend/prepared_ir.tw` | Add `ElemRepr` + `ReprKind::TypedVec(ElemRepr)` | T1 |
| `boot/compiler/codegen/runtime/arr.tw` | `unbox_i64` runtime fn (mirror of `box_i64`); register it | T2 |
| `boot/compiler/codegen/emit/anyref.tw` | Fix `emit_box_to_anyref` `.Vector_` identity trap for `PVecI64` | T3 |
| `boot/compiler/codegen/emit/{arrays,runtime_abi,calls}.tw`, `boot/compiler/builtins.tw` | Repr-driven `_i64` helper selection from slot repr | T4 |
| `boot/compiler/backend/route_typed_vec.tw` | Replace conservative eligibility with a repr-diff-driven coercion inserter | T5 |
| `boot/compiler/backend/{verify_slots,verify_expr}.tw` | Generalize the repr/coercion verifier | T6 |
| `boot/compiler/backend/repr_assign.tw`, `boot/compiler/codegen/wasm_layout.tw` | **Activation:** flip `repr_of_mono` + `val_type_of_mono` + layout for `Vector<Int>` | T7 |
| `examples/performance/sort-bench/*.tw` | Probes + regression gate | T8 |

---

## Task 0: Shared repr-policy module (break the cycle)

**Why first:** `repr_assign.tw` imports `wasm_layout.tw`; the activation needs
`wasm_layout` to consult element repr. Putting the classifier in `repr_assign`
would force `wasm_layout → repr_assign → wasm_layout`. The vector element-repr
classification is a *pure* function of `MonoType` (no `layout_of` needed), so it
extracts cleanly into a lower module both import.

**Files:**
- Create: `boot/compiler/backend/repr_policy.tw`
- Test: `boot/tests/suites/repr_policy_suite.tw` (new; register in the suite index)

- [ ] **Step 1: Write the failing test**

```tw
// boot/tests/suites/repr_policy_suite.tw
use compiler.backend.repr_policy.{elem_repr_of_vector}
use compiler.mono_type.{MonoType}

pub fn test_repr_policy() {
  // Vector<Int> classifies as the I64 element family.
  assert.eq(elem_repr_of_vector(.Vector(.Int)), .Some(.I64), "vec<int> -> I64")
  // M1a supports only I64; other primitive vectors are not yet typed.
  assert.eq(elem_repr_of_vector(.Vector(.Float)), .None, "vec<float> unsupported in M1a")
  assert.eq(elem_repr_of_vector(.Vector(.String)), .None, "vec<string> stays boxed")
  assert.eq(elem_repr_of_vector(.Int), .None, "non-vector -> None")
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run boot/tests/main.tw` (after registering the suite)
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

> **Note:** `ElemRepr` is defined here (the policy module) and re-exported/used by
> `prepared_ir.tw` in T1. Keep this the single definition; `prepared_ir` imports
> it rather than redefining.

- [ ] **Step 4: Run to verify it passes**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS.

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
- Modify: `boot/compiler/backend/prepared_ir.tw:47` (the `ReprKind` enum)
- Modify: every exhaustive `case` on `ReprKind` (find them — see Step 2)

- [ ] **Step 1: Add the variant**

In `prepared_ir.tw`, import `ElemRepr` and extend `ReprKind`:

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

- [ ] **Step 2: Find every exhaustive match and add a `TypedVec` arm**

Run: `grep -rn "case .*repr\|ReprKind" boot/compiler/ | grep -v "//"`
For each exhaustive `case` on a `ReprKind` value, add a `.TypedVec(_) => …` arm.
Until activation nothing produces `TypedVec`, so the safe placeholder arm is to
mirror the `TypedRef`/`OpaqueAnyref` behavior at that site (a `TypedVec` is a
typed ref like `TypedRef`). Where a site already has `_ =>`, no change is needed.

- [ ] **Step 3: Self-host to prove exhaustiveness**

Run: `make bundle-cli`
Expected: compiles and reaches fixed point (no non-exhaustive `case` errors).
Nothing produces `TypedVec` yet, so behavior is unchanged.

- [ ] **Step 4: Commit**

```bash
make boot-test
git add boot/compiler/backend/prepared_ir.tw boot/compiler/**/*.tw
git commit -m "backend: add ReprKind.TypedVec(ElemRepr) variant (inert)"
```

---

## Task 2: `unbox_i64` runtime adapter (`PVec → PVecI64`)

**Why:** Coercion needs both directions. `box_i64` (`PVecI64 → PVec`) exists;
the reverse re-types a boxed `PVec` returning from a universal helper. A raw
`ref.cast` would trap (different leaf array types), so it must rebuild.

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw` (add `unbox_i64_fn`, register in the fn list near `box_i64_fn()` at line ~160)

- [ ] **Step 1: Add a runtime roundtrip probe (the test)**

Create `examples/performance/sort-bench/unbox_i64_probe.tw`: build a
`Vector<Int>` via `collect`, force it boxed (append in a dead branch, per
`typed_vec_read_probe`'s `boxed_sum`), then a helper that will (post-T4) route the
value through `unbox_i64` and read it typed; assert checksum equals the boxed
read. *Until T4 wires selection this probe is a placeholder gate; its real
assertion is the checksum-match after activation (T7).* Keep it minimal now.

- [ ] **Step 2: Implement `unbox_i64_fn` mirroring `box_i64_fn`**

Mirror `box_i64_fn` (`arr.tw:1936`), swapping the builder family and element
direction: `params: [pvec_ref()]`, `results: [pvec_i64_null()]`; loop
`0..len`; read each element with the boxed `get` + `BoxedInt` unbox to raw `i64`;
push into a typed builder via `builder_new_i64` / `builder_push_i64`; finish with
`builder_freeze_i64`. Register `unbox_i64_fn()` alongside `box_i64_fn()` in the
runtime fn list.

```tw
// boot/compiler/codegen/runtime/arr.tw — sketch; mirror box_i64_fn's structure
// unbox_i64(vec: PVec?) -> PVecI64
// Reverse of box_i64: rebuild a typed PVecI64 from a boxed PVec by unboxing each
// BoxedInt element. Used when a boxed Vector<Int> result from a universal helper
// re-enters typed code.
fn unbox_i64_fn() FuncDef {
  .{
    name: "unbox_i64",
    params: [pvec_ref()],
    results: [pvec_i64_null()],
    locals: [.I32, .I32, pvec_i64_null()],  // n, i, typed_builder
    body: [
      // n = len(vec); i = 0; b = builder_new_i64()
      // loop i<n: push_i64(b, unbox(get(vec, i))); i++
      // return builder_freeze_i64(b)
      // (fill from box_i64_fn's loop skeleton, inverted)
    ],
  }
}
```

- [ ] **Step 3: Self-host (proves the fn assembles / type-checks in emitted WAT)**

Run: `make bundle-cli`
Expected: fixed point; `unbox_i64` present in the runtime. (It is dead until T5
references it — that is fine.)

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/codegen/runtime/arr.tw examples/performance/sort-bench/unbox_i64_probe.tw
git commit -m "runtime: add unbox_i64 adapter (PVec -> PVecI64), inert"
```

---

## Task 3: Fix the `emit_box_to_anyref` `PVecI64` identity trap

**Why:** `emit_box_to_anyref` (`emit/anyref.tw`) has `.Vector_(_) => buf`
(identity upcast). Once `Vector<Int>` is `PVecI64`, upcasting that distinct struct
straight to `anyref` and later `ref.cast`-ing back to `PVec` traps. Any typed
vector crossing into an `anyref` position must be `box_i64`'d first.

**Files:**
- Modify: `boot/compiler/codegen/emit/anyref.tw` (`emit_box_to_anyref`, the `.Vector_` arm)

- [ ] **Step 1: Change the `.Vector_` arm to be repr-aware**

For a `Vector<Int>` (the M1a typed family), emit `box_i64` before the identity
upcast; all other vectors keep the identity upcast. Concretely, replace
`.Vector_(_) => buf` with a match that, when the vector's element is `Int`,
appends `.Call("box_i64")` then returns `buf` (the boxed `PVec` is already an
`anyref`-compatible ref); otherwise returns `buf` unchanged.

- [ ] **Step 2: Self-host (still inert — nothing is PVecI64 yet)**

Run: `make bundle-cli && make boot-test`
Expected: fixed point + green. Behavior identical pre-activation (all
`Vector<Int>` are still `PVec`, so `box_i64` of an already-boxed value must be a
no-op path — verify the arm only fires for a `PVecI64`-typed value; if
`emit_box_to_anyref` only sees `MonoType`, gate the `box_i64` on the *source
wasm type* being `PVecI64`, not on the mono alone).

> **Landmine:** `emit_box_to_anyref` is keyed on `MonoType`, but the box-vs-noop
> decision depends on the *source physical repr* (`PVecI64` vs already-`PVec`).
> Thread the source `ValType` (or a `is_source_pvec_i64` flag) into this call
> site, or perform the `box_i64` in the coercion inserter (T5) instead and leave
> `emit_box_to_anyref` untouched. Decide during T5; whichever site owns it, the
> verifier (T6) must guarantee no raw `PVecI64 → anyref` upcast survives.

- [ ] **Step 3: Commit**

```bash
git add boot/compiler/codegen/emit/anyref.tw
git commit -m "emit: box_i64 typed vectors before anyref erase (guard the identity trap)"
```

---

## Task 4: Repr-driven `_i64` helper selection

**Why:** After activation every `Vector<Int>` slot is `TypedVec(I64)`; emission
must select `_i64` builder/push/freeze/len/get from the slot repr — the job
`route_typed_vec` did by rewriting calls. Reads already route via base wasm type
(`emit/arrays.tw` `is_pvec_i64`); this generalizes selection to builder/len/freeze
and drives it from repr, not from the bolt-on's rewrite.

**Files:**
- Modify: `boot/compiler/codegen/emit/arrays.tw` (index/len), `emit/runtime_abi.tw` (builder arg shim), `emit/calls.tw` (result adaption), `boot/compiler/builtins.tw` (the `_i64` builtin ids)

- [ ] **Step 1: Gate = the existing typed probes still pass under the bolt-on**

This task is a refactor of *how* `_i64` selection is decided (repr-driven vs
rewrite), landing before activation. Its gate is that the current typed-vector
probes are unaffected while the bolt-on is still active:

Run:
```bash
target/twk build examples/performance/sort-bench/typed_record_field_probe.tw -o /tmp/trf.wat
grep -c rt_arr__get_i64 /tmp/trf.wat   # expect >= 1
target/twk run examples/performance/sort-bench/typed_vec_read_probe.tw  # match=true, ~7x gap
```

- [ ] **Step 2: Make helper selection consult `SlotInfo.repr == TypedVec(I64)`**

Where emission currently decides the `_i64` builder/push/freeze/len variant
(today keyed off the bolt-on's slot retyping), key it off `repr == .TypedVec(.I64)`
(equivalently `wasm_type == PVecI64`). Keep reads as-is (already base-wasm-type
driven). Do not yet flip `repr_of_mono` — this only changes *how* a slot that is
already `PVecI64` selects helpers.

- [ ] **Step 3: Self-host + probes**

Run: `make bundle-cli && make boot-test` then Step 1's probe commands.
Expected: fixed point, green, probes unchanged (`match=true`, `get_i64` present).

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/codegen/emit/*.tw boot/compiler/builtins.tw
git commit -m "emit: drive _i64 helper selection from slot repr (TypedVec)"
```

---

## Task 5: Repr-diff coercion inserter (replace conservative eligibility)

**Why:** `route_typed_vec`'s eligibility is "typed only if never escapes." Replace
that with: every `Vector<Int>` is `PVecI64`, and insert a coercion (`box_i64` /
`unbox_i64`) exactly where a `PVecI64` value meets a position whose expected wasm
type is boxed `PVec` (universal helper arg, and re-typing a boxed helper *result*).

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw` (becomes the coercion inserter; keep the file, change the pass)

- [ ] **Step 1: Gate = a variant-payload probe goes typed, checksum matches**

The behavioral contract is `typed_variant_column_probe` (already in the repo):
after this task + activation, the extracted column reads typed and the checksum
matches the boxed baseline. This task builds the inserter; T7 activates. Author a
positive/negative variant probe pair now (see T8) to lock the contract.

- [ ] **Step 2: Implement the inserter**

Walk the prepared ANF. For each atom/slot whose repr is `TypedVec(I64)` used at a
position expecting boxed `PVec` (call arg to a universal helper, store into an
`anyref`/`ErasedSum` position), insert `box_i64`. For each boxed-`PVec` *result*
bound into a `TypedVec(I64)` slot (universal helper return), insert `unbox_i64`.
Where both sides are `TypedVec(I64)` (typed variant/record payload, typed local),
insert **nothing**. Remove the old "escape disqualifies" analysis.

> This is the largest transform. Reuse the existing lineage/copy-map plumbing in
> `route_typed_vec.tw`; the change is the *decision rule* (repr diff, not escape
> eligibility) and adding the `unbox_i64` direction.

- [ ] **Step 3: Gate deferred to activation**

The inserter cannot be exercised until `repr_of_mono` produces `TypedVec` (T7).
Land it self-host-green (inert: no slot is `TypedVec` yet) and commit.

Run: `make bundle-cli && make boot-test`
Expected: fixed point + green (inert).

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/backend/route_typed_vec.tw
git commit -m "backend: repr-diff coercion inserter (box_i64/unbox_i64), inert until activation"
```

---

## Task 6: Generalize the verifier

**Files:**
- Modify: `boot/compiler/backend/verify_slots.tw` (`is_typed_vec_i64`, slot checks), `boot/compiler/backend/verify_expr.tw` (`pvec_repr_mismatch`)

- [ ] **Step 1: Extend the rule**

Accept `TypedVec(I64)` ⇔ `PVecI64` wasm type as a valid slot pairing (generalize
the existing `is_typed_vec_i64`). Reject any `PVecI64`-typed value meeting a
boxed-`PVec`/`anyref` position without an intervening coercion node — the safety
net for T3/T5. Keep the existing `pvec_repr_mismatch` store check.

- [ ] **Step 2: Self-host (inert)**

Run: `make bundle-cli && make boot-test`
Expected: fixed point + green.

- [ ] **Step 3: Commit**

```bash
git add boot/compiler/backend/verify_slots.tw boot/compiler/backend/verify_expr.tw
git commit -m "verify: accept TypedVec(I64) and require coercions across reprs"
```

---

## Task 7: **Activation** — flip `Vector<Int>` to the typed family

**Why:** Turns everything on at once. This is the integration gate.

**Files:**
- Modify: `boot/compiler/backend/repr_assign.tw:231` (`repr_of_mono`), `boot/compiler/codegen/wasm_layout.tw` (`val_type_of_mono:523`, `layout_hint_of:427`, and `layout_of` vector arm)

- [ ] **Step 1: Flip the classifier and valtype**

- `repr_of_mono` (`repr_assign.tw:231`): `.Vector(elem) =>` consult
  `repr_policy.elem_repr_of_vector`; `.Some(er) => .TypedVec(er)`, else
  `.TypedRef(mono)`.
- `repr_of_named_cached` (`repr_assign.tw:260`) `.Vector_` arm: same, for named
  vector aliases.
- `val_type_of_mono` (`wasm_layout.tw:523`) and `layout_hint_of`
  (`wasm_layout.tw:427`): for `Vector<Int>` return
  `.Ref(true, .Named("rt_types__PVecI64"))` instead of `PVec`. (Variant/record
  payload types then follow automatically via `layout_of_sum_def`'s
  `val_type_of_mono(field_ty)` at `wasm_layout.tw:311`.)

- [ ] **Step 2: Self-host — the real integration test**

Run: `make bundle-cli`
Expected: reaches "Fixed point reached". If it traps or fails to converge, the
coercion inserter (T5), helper selection (T4), or the anyref trap (T3) has a gap —
debug there. **Do not proceed until the fixed point is green.**

- [ ] **Step 3: Full boot suite**

Run: `make boot-test`
Expected: all green (was ~2820+; no regressions).

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/backend/repr_assign.tw boot/compiler/codegen/wasm_layout.tw
git commit -m "activate: Vector<Int> uses TypedVec(I64)/PVecI64 through all boundaries"
```

---

## Task 8: Probes + regression gate

**Files:**
- Use: `examples/performance/sort-bench/typed_variant_column_probe.tw` (exists)
- Create: `examples/performance/sort-bench/typed_variant_column_boxed_probe.tw` (negative: a `Vector<String>` or combinator-fed variant column stays boxed)

- [ ] **Step 1: Variant payload routes typed (positive)**

Run:
```bash
target/twk build examples/performance/sort-bench/typed_variant_column_probe.tw -o /tmp/tvc.wat
grep -c rt_arr__get_i64 /tmp/tvc.wat     # expect >= 1 (was 0 before activation)
target/twk run examples/performance/sort-bench/typed_variant_column_probe.tw   # match=true; typed << variant timings converge
```
Expected: `get_i64` now present; `match=true`.

- [ ] **Step 2: Negative probe stays boxed**

Author `typed_variant_column_boxed_probe.tw` with a variant column that must stay
boxed (`StrCol(Vector<String>)`, or an `append`-fed producer). Build to WAT;
assert `grep -c rt_arr__get_i64` on that field's read is `0`.

- [ ] **Step 3: No `order_by` regression**

Run: `target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw`
Expected: `gather`/`take`/`sort` phases within noise of the pre-M1a baseline
(N=1M: sort ~1317ms, gather 3 cols ~412ms, take ~418ms, full ~2327ms). They do
**not** improve here (sort needs M1b, gather needs M3); the gate is *no material
regression* from the new boundary coercions.

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
- `route_typed_vec` is now a repr-diff coercion inserter (not escape-eligibility);
  `unbox_i64` exists; verifier accepts `TypedVec(I64)` and requires coercions.

Deferred to **M1b**: typed closure-env layouts (the `sort` win). Deferred to
**M3**: typed `gather`/`sort`/`map` (the `gather`/`take` win).
