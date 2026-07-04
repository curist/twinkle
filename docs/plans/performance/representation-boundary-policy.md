# Representation-Boundary Policy

**Status:** Design approved (2026-07-04), ready for implementation planning.
This doc resolves Phase 1 ("declare the representation-boundary policy") of
[backend-anyref-elimination.md](backend-anyref-elimination.md) and supersedes the
"route before vs after boundary insertion" open question carried in
[vector/typed-vector-representation.md](vector/typed-vector-representation.md).

## Problem

The boot backend never made physical representation a first-class fact. `ReprKind`
(`backend/prepared_ir.tw`) is `{ I64, F64, I32, TypedRef(MonoType),
TypedSum(MonoType), ClosureRef, ErasedSum, OpaqueAnyref, DeadValue }` — there is
no typed-vector family — and `repr_of_mono` (`backend/repr_assign.tw`) collapses
*every* `Vector<_>` to a single universal `TypedRef` container. The entire
`PVecI64` distinction lives in a separate ~1144-line post-pass,
`backend/route_typed_vec.tw`, that runs *after* boundary insertion and *after*
repr assignment, retrofitting typed slots at the wasm-type level while `ReprKind`
still reads `TypedRef`. That "erasure-mimicry tax" is the structural problem: the
pass fights a pipeline that has already erased everything, and each new boundary
(record fields → S2.2, next variants, then closures, combinators, cross-fn ABIs)
becomes another conservative special case.

The pipeline order is the root cause:

```
closure_convert → insert_boundaries → slot_assign → repr_assign → route_typed_vectors
```

Boundary insertion decides where to box/unbox *before* representation is known,
so representation can only be retrofitted, never used to place boundaries.

Stage0 already solved this correctly: `vector-backend-repr-inference.md`
(completed 2026-03-29) made physical vector repr first-class in stage0, separate
from `MonoType`, with repr-driven emitter dispatch, explicitly to "replace ad hoc
`is this a Vector<Int>?` emitter checks with a general backend policy." Boot never
adopted that model.

## The reframe

The "three policies" framing (specialize-by-representation / erase-at-boundary /
adapter-shim) is a false trichotomy. Two facts collapse it:

1. **After full monomorphization, physical representation is a pure function of
   the concrete type.** A `Vector<Int>` is `PVecI64` *everywhere*. There is no
   repr-polymorphic boundary between two `Vector<Int>` uses. So
   "specialize-by-representation" is not a per-boundary choice — it is the
   **default that falls out of monomorphization**.
2. **Pure erase-at-boundary cannot unlock the hot paths.** If a column erases to a
   boxed `PVec` when it enters an `IntCol(Vector<Int>)` variant, reads through it
   stay boxed. The variant payload *must* physically hold the typed family.

So the policy is: **representation follows the monomorphized type, uniformly;
erase only at the irreducible universal-ABI / external points.** The design work
is making that fact first-class and letting layout + boundary insertion be driven
by it.

## Measurement

`examples/performance/sort-bench/typed_variant_column_probe.tw` isolates the
dataframe read wall — a `Vector<Int>` held in a variant, read in a hot
random-index loop — against the same values as a bare typed local:

| path | time (N=1M, 10M reads) |
|------|------|
| variant-held column (boxed, current) | ~495–515ms |
| same values, bare typed local (`PVecI64`) | ~74–87ms |
| **ratio** | **~6.6–6.7×**, `match=true` |

(Absolute times were inflated by background CPU load; the ratio is load-robust —
both halves run under identical load — and matches the bare-local ~6.8× from
`typed_vec_read_probe.tw`.) The finding: **a `Vector<Int>` in a variant pays the
full boxing tax on every read, with no variant-specific overhead beyond it**, so
typing the payload recovers essentially the entire read-wall speedup.

Ceiling for dataframe `order_by` (N=1M, current full ~2327ms, sort ~1317ms, of
which ~69% is boxed key reads): typed reads at ~6.7× bring the read portion
~0.69 → ~0.10, so sort ~1317 → **~540ms** (matching the ~491ms generic-comparator
floor) and full `order_by` ~2327 → **~1500ms** — ~2.5× on the sort, ~1.5×+ on the
full path, generalizing to every primitive vector in real code.

## Design

### 1. Representation as a first-class, monomorphization-derived fact

- **Extend `ReprKind` with typed container families** — e.g. `TypedVec(ElemRepr)`,
  starting with `I64`; the shape admits `F64`/`I32`/anyref-elem. `TypedRef(mono)`
  stays the fallback for reference-payload vectors (`Vector<String>`, records),
  where boxing is not a cost — consistent with the Clojure calibration that
  reference payloads do not hit the boxed-primitive cliff.
- **`repr_of_mono` assigns the family from the element type**, `Vector<Int> →
  TypedVec(I64)`, **unconditionally** — a total function of the concrete type, no
  escape-analysis eligibility gate.
- **Aggregate layout derives field wasm-types from element repr.** `wasm_layout`
  computes record, variant, **and closure-environment** field types from the
  field's element repr, so a `Vector<Int>` in an `IntCol` variant payload, a
  record field, or a closure capture physically holds `PVecI64`. This is what
  makes the reads typed *for free* and retires the bespoke per-boundary routing.

### 2. Boundary & coercion model

Because representation is uniform per concrete type, coercions are only needed at
the irreducible erasure points:

1. **Universal runtime/helper ABIs** — an `rt` op whose signature is `anyref`
   with no typed family member yet (e.g. an un-specialized `Vector.map`). Coerce
   at the call; add typed family members over time to remove it.
2. **Universal closure calling convention** — the box-everything args path. The
   existing S2.1 typed-funcref fast path already keeps *calls* unboxed where the
   concrete signature is known.
3. **External host ABI / imports** — genuinely `anyref`.

**The coercion is O(n) one-time** (box/unbox each element, reusing the existing
`box_i64` adapter in `codegen/runtime/arr.tw`), *not* O(1) per read. Cost model:
boxing proportional to length is paid only when a typed vector is forced through a
universal ABI — never on the read path.

**Closure environments are aggregates too.** A comparator
`fn(a,b){ Int.compare(keys[a], keys[b]) }` *captures* `keys`. If the closure env
stores that capture as `anyref`, reads inside the comparator go through an erased
env slot and stay boxed even after variants are typed. The layout-derivation
principle in §1 therefore applies uniformly to records, variants, *and*
closure-env fields. Once it does, the whole hot path — column in a variant,
captured into a closure env, read via typed index in a typed comparator — stays
typed end-to-end with **zero coercions**.

**Verifier.** Generalize the existing repr check (`backend/verify_expr.tw`,
`backend/verify_slots.tw`): a typed-vec slot may only meet an erased-expecting
position through an explicit coercion node, and every coercion connects two known
reprs. This keeps the policy auditable (the anyref-elimination plan's "explicit in
backend facts, verifier catches mismatches").

## Staging

### Milestone 1 — First-class family + typed aggregate layout (Approach C core)

The structural win, keeping the coercion as a principled post-pass.

- Add the typed-vec family to `ReprKind` (`backend/prepared_ir.tw`);
  `repr_of_mono` assigns `Vector<Int> → TypedVec(I64)` unconditionally
  (`backend/repr_assign.tw`).
- `wasm_layout` derives record / variant / closure-env field wasm-types from
  element repr → those payloads physically hold `PVecI64`.
- Replace `route_typed_vec`'s conservative eligibility with a **repr-diff-driven**
  coercion inserter (reuse `box_i64`): every `Vector<Int>` is typed by default,
  and coercions appear only where a typed vector meets a universal-ABI position.
- Generalize the verifier to the coercion model.
- **Gate:** `order_by` drops toward the ~1500ms ceiling; `typed_variant_column_probe`
  ratio is realized end-to-end; add a variant-payload positive/negative probe pair
  (mirroring the record-field probes); self-host reaches fixed point and the boot
  suite is green.

### Milestone 2 — Repr-aware boundary insertion; retire the bolt-on (Approach A)

- Make `insert_boundaries` (`codegen/insert_boundaries.tw`) consult `repr_of_mono`
  and place wrap/unwrap coercions itself at repr-crossings.
- Delete `route_typed_vec.tw` and the conservative eligibility machinery.
  Representation is decided once and boundaries follow — the dual-world and the
  erasure-mimicry tax are gone.

### Milestone 3 — Broaden families and typed helper surfaces

- `Vector<Float>/<Bool>/<Byte>` families; typed `gather`/`sort`/`map`/`filter`/
  `concat`/`slice` members so hot combinators stop forcing coercions.
- Then `Dict<K,V>` families reusing the same policy (anyref-elimination
  Workstreams B/C).

### Stage0 parity

Boot-only. All of this is codegen / layout / emit inside boot's own Twinkle
source; stage0 compiles the new boot as ordinary Twinkle, and stage0 already has
the repr-inference model from the 2026-03 work. The semantics of every program are
unchanged (box/unbox roundtrips preserve values — the probe's `match=true`
validates). Self-host convergence is the gate; no stage0 changes.

## Risks and open questions

- **Coercion completeness.** Milestone 1's post-pass must insert a coercion at
  *every* typed-vs-erased crossing or the verifier will reject (or, worse, a cast
  traps). The verifier generalization is the safety net; land it alongside the
  inserter, not after.
- **Helper-family coverage vs coercion churn.** Until typed `gather`/`sort`/`map`
  land (Milestone 3), programs that route a typed vector through those combinators
  pay an O(n) coercion per call. Acceptable as a transitional cost; measure that it
  does not regress non-dataframe workloads.
- **Code size.** Typed families multiply runtime/helper surfaces. Add members on
  demand from benchmarks, not speculatively (anyref-elimination's stated risk).
- **`ReprKind` shape.** `TypedVec(ElemRepr)` vs a flat `PVecI64`/`PVecF64` set is a
  detail to settle in the implementation plan; the family-parameterized shape is
  preferred for extensibility but must stay cheap to pattern-match in hot planner
  code.

## Relationship to existing docs

- **Resolves** [backend-anyref-elimination.md](backend-anyref-elimination.md)
  Phase 1 (declare the policy) and delivers the first slice of its Workstream B
  (typed container families).
- **Supersedes** the "route before vs after boundary insertion" open question in
  [vector/typed-vector-representation.md](vector/typed-vector-representation.md)
  (answer: representation is decided from `MonoType`; Milestone 2 moves the
  decision into `insert_boundaries`).
- **Retires** `backend/route_typed_vec.tw` at Milestone 2; its landed S2.0–S2.2
  behavior becomes the special case that falls out of the general model.
- **Mirrors** the stage0 model from
  [../archive/vector-backend-repr-inference.md](../archive/vector-backend-repr-inference.md).
