# Emit coercion by destination physical type (`Expected{vt, mono}`)

**Status:** design approved 2026-07-06, not yet built
**Branch:** `typed-vector-crossfn-abi`
**Related:** [typed-vector-representation.md](./typed-vector-representation.md) (see "Control-flow-result-typing attempt" and "typed-return bridge")

## Problem

Emit coerces a produced value to a physical Wasm type derived from the value's
**mono** (`val_type_of_mono(mono)`), even when the destination slot has been
retyped to a different physical representation by the storage-site typed-vector
router. For a `Vector<Int>` whose slot is physically `PVecI64`, the coercion
target is recomputed as the boxed `PVec`, so the already-typed value is boxed —
and stored into a `PVecI64` local, which is a Wasm type mismatch (traps / invalid
module).

This is one root cause with four instances:

| Site | Real destination physical type | Status today |
|---|---|---|
| Function return | `PreparedFunc.phys_return` | one-off patch in `emit_return_coerce` (works) |
| Runtime-call result (gather) | the call's result vt | name-exemption `is_typed_vec_result` in `calls.tw` (works, ad-hoc) |
| `if` arm store | `result_vt` param of `emit_if_op` | **dropped** — threads `result_mono` instead |
| `match` arm store | `result_vt` param of `emit_match_op` | **dropped** — `MatchCtx` built without it |

In every case the destination's physical `ValType` is **already available one
frame up** at the store/return/call site; the terminal coercion re-derives it
from mono and gets the boxed answer.

### The concrete target

Dataframe accessors of the shape:

```tw
pub fn as_ints(c: Column) Vector<Int> {
  case c.data { .IntCol(v) => v, _ => error("...") }
}
```

`v` is a typed payload read (`PVecI64`). If the `case`-result slot is also
`PVecI64`, `as_ints` carries a `PVecI64` return ABI (the landed `phys_return`
bridge), so `keys := as_ints(col)` is typed, so a sort comparator capturing
`keys` reads `get_i64` (the landed M1b typed-closure-capture). This is the last
mile to drop the ~1343ms dataframe `order_by` sort. Routing already makes the
result slot `PVecI64` correctly; emit then boxes the arm value and produces
invalid Wasm, which is why the routing was reverted.

### Why store-time / post-hoc fixups are ruled out

The arm boxes **at emission** (`emit_expr(arm, result_mono)` →
`emit_atom_for_expected` → `emit_coerce_stack(from, val_type_of_mono(mono), …)`).
Any store-time or post-emit fixup would therefore see a boxed `PVec` and need
`PVec → PVecI64` = `unbox_i64` = an **O(n) rebuild** — the exact pathology the
storage-site model exists to avoid. The value must be coerced to the physical
destination type *at the arm's terminal emit*, not afterward.

## Chosen approach: `Expected{vt, mono}`, staged

Replace the bare `expected_ty: MonoType` on the emit coercion path with:

```tw
type Expected = .{ vt: ValType, mono: MonoType }
```

`vt` is the **true destination physical type**. `mono` is demoted to a hint used
only for (a) the `Void`/`Never` short-circuits and (b) `emit_coerce_stack`
instruction selection. Terminal coercion targets `exp.vt`, never
`val_type_of_mono(mono)`.

This makes the physical destination type a first-class threaded value instead of
something re-derived at leaves. It **unifies all three existing overrides**
(`phys_return`, the gather name-exemption, and the missing arm store) and
establishes the invariant: *emit coerces to the destination's physical type,
carried as data*. Future typed representations (PVecF64, Buffer views, typed
dicts) then plug in for free — routing sets a slot's `wasm_type`, and
stores/returns/calls target it automatically.

### Rejected alternatives

- **ctx transient `result_phys: ValType?`** — least apparent churn, but there is
  no `with_*` helper on `EmitCtx`; its 3 construction sites build full ~14-field
  struct literals, so a per-arm transient needs a full-literal rebuild per arm
  plus save/restore discipline. It also "fixes" a mono-vs-physical confusion by
  adding hidden mutable physical state — the exact bug class being eliminated.
- **Elide the single-real-arm result slot at ANF** — smallest diff, ships the
  dataframe win, but only handles the one-non-diverging-arm shape, adds an ANF
  special case, and never collapses the four sites (gather exemption and
  `phys_return` stay separate forever). Kept in mind only as a fallback if the
  dataframe number is needed before the refactor lands.

## Staging

### Stage 1 — behavior-preserving `Expected` wrapper

Introduce `Expected` and a constructor
`expected(mono, env) = .{ vt: val_type_of_mono(mono, env), mono }`. Swap the
parameter type on the coercion-path functions and wrap all call sites:

- `emit_expr`, `emit_tail_expr`, `emit_atom_for_expected`, `emit_return_coerce`
- the `emit_expr` function-type field on `ControlEmitFns` and `MatchEmitFns`
- the ~15 call sites (mostly in `control_flow.tw` and `match.tw`) that currently
  pass `result_mono` / `expected_ty`

Terminal coercion uses `exp.vt`. Because `vt == val_type_of_mono(mono)`
everywhere in this stage, there is **zero behavior change**. Self-host to a fixed
point and run the full boot test suite in isolation. This de-risks the wide
mechanical churn before any semantics move.

### Stage 2 — inject the real physical targets

- Thread `result_vt` into `MatchCtx` (it is dropped at `match.tw:74` today; the
  `emit_match_op` wrapper already receives it).
- At the arm/spine store sites build `Expected{ vt: result_vt, mono }` so a typed
  arm coerces to `PVecI64` (a no-op).
- Fold `phys_return` and the gather exemption into the same general path: the
  return coercion targets `Expected{ vt: phys_return, mono }`, and
  `adapt_runtime_result` targets the call's real result vt — **delete** the
  `is_typed_vec_result` box-skip.
- Re-apply the reverted `route_func` result-slot typing pass (result slot + typed
  arm-atom slots retyped to `PVecI64`; result-position bare `Atom(a)` in a
  `relaxed` set is not an escape).
- `append_result_store` stays a pure `LocalSet` — the arm already produced
  `exp.vt`, so there is no store-time coercion and no box→unbox rebuild.

### Soundness argument (closes the box→unbox tension)

Routing's `arm_verdict` already **disqualifies** the result slot if any
non-diverging arm is boxed (verdict 2). Therefore `result_vt = PVecI64` is set
only when **every** non-diverging arm is a typed source → every arm emits
natively to `PVecI64` → the uniform physical store is consistent and
coercion-free. The routing invariant is exactly what makes Stage 2's uniform
physical store sound; mixed typed/boxed arms keep the boxed `PVec` result slot as
before.

## Verification

- **Stage 1:** `make bundle-cli` reaches a self-host fixed point; `make boot-test`
  fully green. No WAT diff in behavior expected (target vt is unchanged).
- **Stage 2:** self-host fixed point + `make boot-test`; dataframe repro
  `target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw`
  must not trap and should drop `sort idx by amount` (~1342ms). Inspect
  `as_ints` WAT (`target/twk build … -o /tmp/x.wat`) to confirm the `IntCol` arm
  no longer emits `box_i64` before the store.

## Pointers

- `boot/compiler/codegen/emit.tw` — `emit_expr`, `emit_tail_expr`,
  `emit_atom_for_expected`, `emit_return_coerce`, `current_return_mono`,
  `emit_op` (carries `result_vt`), `emit_if_op`/`emit_match_op` wrappers.
- `boot/compiler/codegen/emit/control_flow.tw` — `emit_if_op`,
  `emit_if_else_spine`, `append_result_store`, `ControlEmitFns`.
- `boot/compiler/codegen/emit/match.tw` — `MatchCtx`/`TailMatchCtx`,
  `emit_match_op`, `emit_arm_chain`, `emit_match_arm_body`, `MatchEmitFns`.
- `boot/compiler/codegen/emit/coercions.tw` — `emit_coerce_stack`,
  `adapt_runtime_result`.
- `boot/compiler/codegen/emit/runtime_abi.tw` / `calls.tw` —
  `is_typed_vec_result` and its use at `calls.tw:357`.
- `boot/compiler/backend/route_typed_vec.tw` — `route_func`,
  `slot_typed_after_route`, `v_group_escapes`, `arm_verdict`, `phys_return`.
