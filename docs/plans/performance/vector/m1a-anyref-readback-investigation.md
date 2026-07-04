# M1a activation — anyref read-back pathology (investigation handoff)

**Status:** activation **reverted** on branch `typed-vector-repr-m1a` (never
reached main). It self-hosted and passed all boot tests, but was not viable:
captured `Vector<Int>` reads are O(n) per access, which hangs `order_by`. Root
cause confirmed and recorded here; the corrected direction is in
[../representation-boundary-policy.md](../representation-boundary-policy.md). This
note is retained as the post-mortem.

## What landed (branch `typed-vector-repr-m1a`)

| commit | what |
|---|---|
| `84d9ad84` | T0: `backend/repr_policy.tw` — pure `elem_repr_of_vector` (Vector<Int>→I64) |
| `fd19e3f8` | T1: `ReprKind.TypedVec(ElemRepr)` + exhaustiveness arms (inert) |
| `46dd30aa` | T2: `unbox_i64` runtime adapter (PVec→PVecI64), inert |
| `325755bd` | T3: `emit_coerce_stack` PVec↔PVecI64 both ways + anyref erase, inert |
| `f26cd7a3` | **Activation:** `repr_of_mono`/`val_type_of_mono` map `Vector<Int>`→`TypedVec(I64)`/`PVecI64` unconditionally; `route_typed_vectors` removed from the pipeline; Option-B boundary coercions |

Design docs: `representation-boundary-policy.md` (spec),
`representation-boundary-m1a-plan.md` (plan). Both assume "M1a shippable alone,
M1b (typed closure envs) later" — **that assumption is falsified by this bug.**

## The failing repro (self-contained)

```tw
// /tmp/cap.tw  — captured Vector<Int> read in a loop
use @std.date
fn run() Int {
  keys := collect i in range(5000) { i }
  read := fn(a: Int) Int { keys[a] }   // closure captures keys
  acc := 0
  for k in range(200000) {
    acc = acc + read(k % 5000)
  }
  acc
}
t := date.now()
r := run()
println("captured-read 200k over vec[5000]: ${date.now() - t}ms (${r})")
```

- On the activated branch: **~8116 ms** (should be ~20 ms).
- `target/twk build /tmp/cap.tw -o /tmp/cap.wat` then `grep -c rt_arr__unbox_i64 /tmp/cap.wat` → nonzero: the **closure body calls `unbox_i64`** (rebuilds the whole vector) per access.
- Real-world trigger: `idx.sort_by(fn(a,b){ Int.compare(keys[a], keys[b]) })` —
  the comparator captures the key column; O(n log n) comparisons × O(n) rebuild =
  O(n² log n). `examples/performance/dataframe/bench/order_by_breakdown.tw` at
  N=1M ran >28 min CPU (was ~2.3 s pre-activation) before being killed.

## Root cause (confirmed)

Uniform typing makes `Vector<Int>` = `PVecI64` **everywhere**, but closure
environments store `anyref`. So a captured `Vector<Int>`:

1. is boxed to `PVec` at capture (`box_i64`, once — fine), and
2. must be **unboxed to `PVecI64` on every access** to satisfy its static type —
   via `emit_unbox_from_anyref` (`boot/compiler/codegen/emit/anyref.tw`), whose
   `.Vector_` arm now does `RefCast anyref→PVec` **then `unbox_i64`**, and
   `unbox_i64` **rebuilds the entire vector**.

Read once → fine. Read in a loop → O(n) per read.

**The eager `unbox_i64` is required for type-correctness under uniform typing —
it is not a stray call that can simply be deleted.** This is the exact pathology
the original conservative `route_typed_vec` avoided by only typing vectors that
*never escape*. Uniform typing removed that guard.

## Culprit code (pinpoint — the closure env boundary)

The per-access rebuild is the `ClosureEnv` boundary, not the lambda body (the WAT
shows the lambda already receives `PVecI64`):

- `boot/compiler/codegen/emit/closures.tw`
  - `emit_make_closure` — boxes captured `Vector<Int>` into the anyref env
    (`emit_box_to_anyref` → `rt_arr__box_i64`).
  - `emit_typed_trampoline` — reads captures from the anyref env and unboxes on
    *every* closure invocation (`emit_unbox_from_anyref` → `rt_arr__unbox_i64`).
    (Its "No boxing/unboxing" comment is now false.)
- `boot/compiler/codegen/runtime/types.tw` — `ClosureEnv` is `Array anyref`, so it
  cannot store typed capture slots directly (this is what M1b would change).
- Separate implementation inconsistency (not the cause): `cached_repr_of_mono`
  in `repr_assign.tw` kept `.Vector(_) => TypedRef` while the public
  `repr_of_mono` was flipped to `TypedVec` — resolved by the revert (all three
  Vector arms are `TypedRef` again); the redesign should unify them behind one
  classifier.

## Culprit code (activation)

- `boot/compiler/codegen/emit/anyref.tw` — `emit_unbox_from_anyref` `.Vector_` arm
  (the per-access rebuild) and `emit_box_to_anyref` `.Vector_` arm (box on store).
- `boot/compiler/backend/repr_assign.tw` — `repr_of_mono` / `repr_of_named_cached`
  flip to `TypedVec`.
- `boot/compiler/codegen/wasm_layout.tw` — `val_type_of_mono` `Vector<Int>`→PVecI64.
- `boot/compiler/backend/prepare.tw` — `route_typed_vectors` removed from pipeline.
- `boot/compiler/codegen/emit.tw` — `emit_atom_as_boxed_pvec` + append/get/set/make
  intrinsic base-box/result-unbox.
- `boot/compiler/codegen/emit/{arrays,coercions}.tw`, `backend/verify_expr.tw`.

## Options to weigh (fresh session)

1. **Revert activation; keep the conservative `route_typed_vec`.** The
   uniform-typing thesis has a real hole at anyref *read* boundaries; it needs
   typed boundaries everywhere (closures + erased sums + generic anyref) before
   it's sound — much bigger than "M1a". *Currently recommended.*
2. **Hybrid:** keep uniform typed *layout* for typed positions (variants/records
   worked), but **do not type vectors that escape to `anyref`** (reinstate the
   escape guard for captures/erased positions). Closer to original design.
3. **M1b (typed closure envs) now** — necessary but *not sufficient*: other
   anyref-held `Vector<Int>` (erased sums, generic anyref) still unbox-per-access.

## Key open question for root-cause work

Is the per-access unbox avoidable by **hoisting** (unbox a captured vector once on
closure entry into a typed local, reuse) rather than per-use — or is
per-representation typed storage at every anyref boundary the only sound fix?
That distinction decides whether this is a codegen-CSE fix or an architectural one.
