# Typed-vector element families — generalize the routing, land `PVecBool`

**Date:** 2026-07-08 · **Branch:** `typed-vector-crossfn-abi` · **Status:** design

## Problem

The typed-vector machinery (`PVecI64` = raw i64 leaves instead of boxed `anyref`)
is the master lever for the read wall, but every piece of it is hardcoded to
`Int`. Adding a second element repr — `Vector<Bool>` (the dataframe `nulls` mask)
or `Vector<Float>` (Float columns) — currently means triplicating the routing,
capture, gather, and verifier machinery.

The `nulls: Vector<Bool>` column is the biggest remaining lever on the `order_by`
hot path: it is read in the null-aware sort (`sort idx + nulls` ~1344ms vs ~732ms
boxing-free) **and** gathered in every `gather`/`take`. One `Bool` family hits
multiple phases. `Float` columns are the next lever after that.

## Goal

Generalize the Int-hardcoded routing into an **element-family-parameterized**
layer (mirroring the runtime's existing `PVecFamily`), then land `PVecBool` as the
first family through it. `Float` becomes a cheap follow-on (one runtime descriptor
+ one registry entry).

This does **not** change the representation-boundary policy
([../representation-boundary-policy.md](../representation-boundary-policy.md)):
`PVecBool`/`PVecF64` are per-storage-site optimizations, never global properties
of `Vector<Bool>`/`Vector<Float>`. A typed vector that reaches a durable erased
boundary is still boxed.

## Key observation — the routing logic is already element-agnostic

The **routing analysis core** (escape analysis, alias groups, capture fixpoint,
gather fixpoint in `route_typed_vec.tw`; the param/return/capture ABI analysis in
`typed_param_abi.tw`) operates on **slot-id sets** and is completely
element-agnostic. Within that core, only three thin layers are `Int`-bound:

1. **Tags** — `is_int_vector` / `mono_key_of` → `"vec_i64"`.
2. **Builtin-id tables** — `RouteIds`' `_i64` fields.
3. **Type names** — the literal `"rt_types__PVecI64"` retype target.

The runtime is already half-generalized: `PVecFamily` (`family_i64()`,
`family_boxed()`) parameterizes the leaf-agnostic trie ops; only thin per-element
wrappers are `_i64`-specific.

So the routing *logic* is not rewritten — it is threaded with one family
descriptor. **But** the full typed-vector surface is wider than the router: the
cross-function ABI-fact maps carry no family tag, and several storage/bridge sites
are separately Int-specific — `wasm_layout.tw` (field/payload layout),
`repr_policy.tw` (candidate policy), `emit/bridge_funcs.tw` (erased variant
bridge), `runtime/core.tw` (structural equality), and `builder_push`/`box`
(element boxing). Each is enumerated below; underestimating them turns into late
verifier/runtime traps.

## Architecture

### `ElemFamily` descriptor (compiler side)

Mirrors the runtime `PVecFamily`, threaded through the tag/id/typename layers:

```
type ElemFamily = .{
  mono_elem:   MonoType,   // .Int / .Bool  — matches .Vector(elem)
  mono_key:    String,     // "vec_i64" / "vec_bool"
  pvec_type:   String,     // "rt_types__PVecI64" / "rt_types__PVecBool"
  elem_wasm:   ValType,    // .I64 / .I32
  builder_new_t, builder_push_t, builder_freeze_t, len_t, get_t, gather_t: Int,
  box_id, unbox_id: Int,
}
```

The **shared boxed ids** (`vector$len`, `builder_*`, `gather` — element-agnostic)
stay top-level in `RouteIds`; only the typed `_i64`/`_bool` ids and the type name
live in the per-family descriptor. A `families()` registry returns
`[family_i64_desc(builtins), family_bool_desc(builtins)]`; adding Float later is
one more entry.

### Per-family pass model (the key structural decision)

`route_func` (and `materialize_slot_repr`) iterate `for fam in families`, running
the **existing** `compute_eligible_v` / `rewrite` unchanged except that `ids`, the
mono-match predicate, and the retype type-name come from `fam`.

This is sound because **families are disjoint by slot mono-type**: a slot is
`Vector<Int>` XOR `Vector<Bool>`, so per-family passes never interfere — the second
pass's retypes and call-swaps hit a disjoint set of slots and call-sites. The
element-agnostic logic (escape/alias/capture/gather) stays literally untouched; it
already assumes a single family.

Rejected alternative: a single pass with `eligible_v: Dict<Int, ElemFamily>`. One
traversal, but every retype/fixpoint site must consult the per-slot family —
invasive to code that is currently correct. Cost of per-family passes is the
capture/gather fixpoints + `materialize_slot_repr` running once per family, but
families are few and each pass early-returns instantly when a function has no
vectors of that family.

### Family metadata flow (cross-function ABI facts must be family-keyed)

The escape/alias/gather logic is element-agnostic and untouched. But the
**cross-function ABI-fact maps are not** — today they record *whether* a
param/return/capture is typeable, not *which* family:

- `typeable_params: Dict<String, Vector<Int>>` (param indices)
- `typeable_return: Dict<String, Bool>`
- `capture_abi: Dict<String, Vector<Int>>` (capture indices)

Under a per-family `route_func` loop these are ambiguous: the Bool pass would read
an I64 function's `typeable_return`/`capture_abi` as a Bool typed return/capture (or
vice versa) and emit a `PVecBool`↔`PVecI64` mismatch. **The ABI facts must carry
the family.** Because a single function can mix an `Int` and a `Bool` typed vector
(a comparator capturing both an amount column and a nulls mask), the family is
**per-index for params/captures** and **per-function for the return**:

- `typeable_params: Dict<String, Dict<Int, FamilyTag>>`
- `typeable_return: Dict<String, FamilyTag>`   (absent = not typed)
- `capture_abi: Dict<String, Dict<Int, FamilyTag>>`
- typed-return **call-result** facts (`collect_typed_return_call_results`) likewise
  carry the callee's return family.

Each per-family pass filters these maps to entries matching the active `fam`. This
is the one place the generalization touches the *fact-lookup* layer (not the
analysis logic): `compute_eligible_v`'s `cap_slots` / `relaxed` / `call_result_sids`
gain a family filter. `FamilyTag` is a small index/enum into the `families()`
registry.

## Runtime storage (the Bool family)

- **Types (`runtime/types.tw`):** add `ArrayBool` (mutable `.I32` elements) and the
  `PVecBool` struct (mirroring `PVecI64`: `len`, `shift`, `root`, `tail: ref
  ArrayBool`).
- **Family descriptor (`runtime/arr.tw`):** add `family_bool()` (`elem_ty: .I32`,
  `pvec_ty: t_PVEC_BOOL`, `zero_value: .I32Const(0)`, the `*_bool` names) plus
  `empty_leaf_bool` / `empty_pvec_bool` globals. The four `PVecFamily`-generic
  funcs (`pvec_len_fn`, `pvec_get_fn`, `pvec_builder_new_fn`,
  `pvec_builder_freeze_fn`) instantiate for free.
- **Non-generic wrappers to add.** Some are mechanical clones of the `_i64`
  siblings with `.I64`→`.I32` and the Bool type names: `promote_full_tail_bool`,
  `gather_bool`, `vec_bool_roundtrip`, `builder_push_bool_raw` (accepts a raw
  `.I32` element). Where a wrapper is structurally identical modulo
  valtype/typename, fold it onto a `PVecFamily` parameter (extending the existing
  pattern) rather than copy-paste.

### `builder_push_bool` is NOT a mechanical clone

`builder_push_i64` takes its element as `.Anyref` and unboxes it with
`RefCast(BoxedInt); StructGet(0)` — the typed-vector router swaps generic
`builder_push` → `builder_push_i64` **without** rewriting the already-boxed push
argument, and an `Int` element is boxed as a `BoxedInt` struct. A `Bool` element is
boxed as `ref.i31` (`emit/anyref.tw`: Bool/Byte use i31ref), so `builder_push_bool`
must decode an **i31** anyref (`RefCast(.I31); I31GetU`) into its `.I32` leaf, not a
`BoxedInt` struct. `builder_push_bool_raw` takes the raw `.I32` directly.

### `box_bool` / `unbox_bool` — the boxed encoding

These convert typed `PVecBool` (i32 leaves) ↔ the **existing** boxed `PVec` whose
`Bool` elements are `ref.i31` (confirmed: Bool fits i31 — unlike `Int`, which needs
a `BoxedInt` struct). `box_bool` must emit exactly the `ref.i31` encoding so a boxed
round-trip is bit-identical. This and `builder_push_bool` are the two spots where
Bool genuinely diverges from an i64 mirror. The post-route verifier + a round-trip
bench catch a mismatch.

### Registration wiring

- **`builtins.tw`:** register ABI + runtime bindings for every Bool helper —
  `vector$len_bool`, `get_bool`, `builder_new_bool`, `builder_push_bool` (+ `_raw`),
  `builder_freeze_bool`, `gather_bool`, `box_bool`, `unbox_bool`,
  `vec_bool_roundtrip` — mirroring the `_i64` block (ABI valtypes use `.I32` /
  `PVecBool` / `ArrayBool`).
- **Runtime module list (`runtime/arr.tw` funcs vector + `runtime/types.tw`):**
  add the `*_bool` `FuncDef`s and the `PVecBool` / `ArrayBool` type + empty globals
  to the emitted set so they exist in the module.
- **Prelude signature hook (`prelude/signatures/vector.tw`):** only if
  `vec_bool_roundtrip` (or any Bool helper) is surfaced as a callable correctness
  probe; otherwise the family is compiler-internal and needs no prelude signature.

## Compiler routing generalization

- **`route_typed_vec.tw`:** replace `is_int_vector` / `mono_key_of` with
  `elem_family_of(mono) : ElemFamily?`. `route_func` / `materialize_slot_repr` gain
  the `for fam in families` loop; `compute_eligible_v`, `rewrite`, and every
  `collect_*` / `classify_*` / `v_group_*` helper take the active `fam` and use
  `fam.pvec_type` / `fam.*_t` ids instead of literals. Retype target becomes
  `.Ref(true, .Named(fam.pvec_type))`. Logic bodies unchanged.
- **`typed_param_abi.tw`:** same treatment — the `Vector<Int>`-only predicate
  becomes `elem_family_of`-any, so a `fn(mask: Vector<Bool>)` can carry a physical
  `PVecBool` ABI and a comparator can capture a typed `nulls` column.

## Storage-site layout & policy

The routing decides which *slots* are typed, but the physical **field/payload
layout** and the **candidate policy** are separately Int-specific and must
generalize or typed Bool fields/payloads won't become `PVecBool` consistently:

- **`wasm_layout.tw`:** `is_int_vector_field` + the literal `"rt_types__PVecI64"`
  drive both the record-field layout and the variant-payload layout. Generalize the
  predicate to `elem_family_of` and pick `fam.pvec_type` for the field/payload
  physical type.
- **`repr_policy.tw`:** the candidate-repr policy (which `Vector<T>` slots are
  eligible for a typed physical repr) is Int-only; extend it to the registered
  families.

## Verifier and emit

- **Verifier (`verify_expr.tw`, `verify_slots.tw`):** the "typed-vec ref"
  recognizers (`named_typed_vec_ref`, `is_typed_vec_i64`) generalize to any
  registered `pvec_type`. The `PVecI64`↔`PVec` mismatch check becomes "any
  typed-family ref vs boxed `PVec`, or two different typed-family refs" — the same
  backstop, now covering Bool on all four already-covered non-coercing edges (local
  stores, record-get result, record fields, closure-capture stores).
- **Emit — index/coercion/closure:** `coercions.tw` dispatches `box_i64`/`unbox_i64`
  by PVec name → per-family `box_X`/`unbox_X`; the anyref-erase (box-before-erase)
  path generalizes the same way. `arrays.tw`'s `is_pvec_i64` index fast-path
  generalizes to any typed family — `StructGet(fam.pvec_type, 0)` + `get_X`, then
  coerce the result (`.I64`/`.I32`) to the slot type. `closures.tw`'s trampoline
  downcast (`RefCast` to `PVecI64`) generalizes to the capture param's actual
  `pvec_type`.
- **Emit — erased bridge & equality (do NOT omit):** typed Bool fields/payloads
  reach two more sites that today only handle `PVecI64`:
  - **`emit/bridge_funcs.tw`** — the sum↔variant bridge helpers box/unbox only
    `PVecI64` (`box_i64`/`unbox_i64`). A typed Bool payload crossing an erased
    variant bridge needs per-family box/unbox here, or it is mishandled.
  - **`runtime/core.tw`** — structural equality boxes a typed `PVecI64`
    field/payload to `PVec` (via `rt_arr__box_i64`) before comparing by contents.
    Records/variants holding a typed Bool vector need the per-family box (`box_bool`)
    or equality falls back to reference identity on the distinct `PVecBool` struct.
- **Emit — typed-builder seeding:** `emit/runtime_abi.tw` special-cases the typed
  builder builtins **by name** (`builder_new_i64` / `builder_push_i64` /
  `builder_freeze_i64`) for builder-seed / element handling; the Bool builtin names
  must be added to those lists.

## Sequencing

1. **Generalize-as-refactor first.** Introduce `ElemFamily` + the per-family loop
   with the registry containing **only** `[i64]`. Everything stays green — a pure
   no-op refactor that proves the abstraction. Full verification loop must pass.
2. **Add the Bool family.** Runtime types/descriptor/wrappers + `box_bool`/
   `unbox_bool` + the registry entry + verifier/emit generalization for the new
   `pvec_type`.

Keeping the risky generalization and the new-family work as separate verifiable
steps means a regression is attributable to one or the other.

## Testing

- Positive probe `typed_bool_read_probe.tw`: a `Vector<Bool>` built by `collect`,
  read via index/len, asserted to hit `get_bool` with no boxing (mirror
  `typed_record_field_probe.tw`).
- Negative probe: the same `Bool` field fed by a combinator-built producer through
  a parameter stays boxed `PVec` (no `get_bool`).
- Round-trip: `box_bool`/`unbox_bool` bit-identity over a mixed `Vector<Bool>`.
- **Typed Bool record field** layout → `PVecBool`, read via index/len (covers
  `wasm_layout.tw` + `repr_policy.tw`).
- **Typed Bool variant payload** layout → `PVecBool`, plus an **erased variant
  bridge round-trip** (construct → sum-bridge → extract, contents preserved;
  covers `bridge_funcs.tw`).
- **Structural equality** of records/variants holding a typed Bool vector compares
  by contents, not `PVecBool` reference identity (covers `runtime/core.tw`).
- **`gather_bool` routing** — a typed Bool receiver gathers to a typed `PVecBool`
  result (result-eligibility + fixpoint).
- **Typed Bool capture/param** — a comparator capturing a typed `nulls` column and
  a `fn(mask: Vector<Bool>)` param carry the physical `PVecBool` ABI (matches the
  real null-aware sort path), and mixed Int+Bool captures in one closure keep their
  distinct families (exercises the family-keyed capture ABI).

### Verification loop (after each increment)

```
target/twk fmt <edited files>
cargo run --release -- build boot/main.tw -o /tmp/x.wasm     # stage0 still bootstraps
make bundle-cli 2>&1 | tail -3                               # must reach "Fixed point reached"
make boot-test 2>&1 | grep -iE "Ran [0-9]+ tests|Failed"     # expect 2980 passed
target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw   # checksums 1000000/3000000
target/twk run examples/performance/sort-bench/typed_payload_capture_guard.tw
```

No stage0 parity is required for typed-vector families (the family is boot-only;
stage0 only proves boot source bootstraps).

## Success criteria

- The generalize refactor lands green with the registry at `[i64]` (no behavior
  change; benches/tests identical).
- `PVecBool` lands: the `typed_bool_read_probe` stays boxing-free, boot-test at
  2980, `order_by_breakdown` checksums intact.
- The null-aware `sort idx + nulls` path drops from ~1344ms toward the boxing-free
  ~732ms floor, and `Bool` columns stop boxing in `gather`/`take`.

## Out of scope

- `Float` family (follow-on: one `family_f64()` runtime descriptor + one registry
  entry once this generalization lands).
- `String`/reference-payload columns (stay boxed — reference elements, no typed
  repr).
- The coercing verifier edges (call args/returns/variant payloads) and full
  `PhysPlan` materialization — deferred, non-blocking (see
  [unify-typedness-oracle-design.md](unify-typedness-oracle-design.md) §3/§6).
