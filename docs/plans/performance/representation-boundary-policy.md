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
`PVecI64` distinction lives in a separate post-pass,
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

Stage0 pointed at this direction but did **not** finish it, and is not a design
to copy. `vector-backend-repr-inference.md` (2026-03-29) built repr-metadata
*scaffolding* — `ValueRepr` (typed closures/cells), `SumRepr` (typed
`Option`/`Result` payloads), and per-local `LocalBackendInfo` — plus a dense
typed `ArrayI64` *scratch buffer* used only inside the native sort kernel (the
Level-1 "dense working set inside a kernel" approach). But stage0's persistent
`Vector<Int>` is still the universal `anyref` PVec: its leaves are
`$Array = (array (mut anyref))` and `ValueRepr` has **no** typed-vector variant.
Boot's `route_typed_vec` (`PVecI64`) is actually further along on the container
axis. So what stage0 offers is a useful *pattern* for threading repr metadata,
not a typed-vector-container implementation — this design is boot-first.

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
- **The physical-repr policy must live below both `repr_assign` and `wasm_layout`.**
  Today `repr_assign` imports `wasm_layout`; if `wasm_layout` needs the element
  repr to lay out aggregate fields, having it call `repr_of_mono` (in
  `repr_assign`) creates an import cycle. Extract `repr_of_mono` /
  `TypedVec`-classification into a lower shared module (`backend/repr_policy.tw`,
  imported by both), or move it into `wasm_layout` and have `repr_assign` consume
  it. Settle this direction before writing code — it is the first implementation
  task.
- **Aggregate layout derives field wasm-types from element repr.** `wasm_layout`
  computes record and variant field types from the field's element repr, so a
  `Vector<Int>` in an `IntCol` variant payload or a record field physically holds
  `PVecI64`. This is what makes those reads typed *for free* and retires the
  bespoke per-boundary routing.
- **Closure environments need per-capture-repr layouts (the harder case).** Today
  closure layout is keyed by `mono_to_key(.Function(params, ret))` and every
  closure shares the universal `rt_types__ClosureEnv` anyref array
  (`wasm_layout.tw`), so the function signature alone cannot describe a typed env —
  two closures with the same signature can capture different reprs. Typed captures
  therefore require a distinct env layout **keyed by the ordered capture reprs**
  (or per-closure-site), threaded from `closure_convert` where the free variables
  and their reprs are known. This is the largest piece of §1 and is on the
  dataframe critical path: the `sort_by` comparator captures the key column, so
  without a typed env its `keys[a]` reads stay boxed even after variant payloads
  are typed.

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

**The coercion is O(n) one-time** (box/unbox each element), *not* O(1) per read.
Cost model: it is paid only when a typed vector is forced through a universal ABI —
never on the read path. **Both directions are required, and only the forward one
exists today.** `box_i64` (`codegen/runtime/arr.tw`) converts `PVecI64 → PVec`;
the reverse — re-typing a boxed `PVec` result coming *back* from a universal helper
into `PVecI64` — needs a new `unbox_i64` / `from_boxed_i64` adapter that unboxes
each element (a raw `ref.cast` from `PVec` to `PVecI64` would trap, since the
leaves are physically different arrays). The design must define where results from
universal ABIs are re-typed, and the verifier (below) must reject a raw cast in
place of the adapter.

**Closure environments are aggregates too**, but not a simple field derivation —
they are the per-capture-repr-keyed layout described in §1. A comparator
`fn(a,b){ Int.compare(keys[a], keys[b]) }` *captures* `keys`; if the env stores
that capture as `anyref`, reads inside the comparator go through an erased env slot
and stay boxed even after variant payloads are typed. Once the env layout carries
the capture's `PVecI64` repr, the whole hot path — column in a variant, captured
into a closure env, read via typed index in a typed comparator — stays typed
end-to-end with **zero coercions**.

**Verifier.** Generalize the existing repr check (`backend/verify_expr.tw`,
`backend/verify_slots.tw`): a typed-vec slot may only meet an erased-expecting
position through an explicit coercion node, and every coercion connects two known
reprs. This keeps the policy auditable (the anyref-elimination plan's "explicit in
backend facts, verifier catches mismatches").

## Staging

Milestone 1 is the structural win (Approach C core), split so the lower-risk
mechanism lands and is measurable before the closure-env subsystem. Both keep the
coercion as a principled post-pass.

#### Milestone 1a — First-class family + typed variant/record payloads

The general mechanism, without touching closures. Delivers the direct-read parts
of the workload (column extraction, `gather`, `take`) and de-risks everything
`1b` depends on.

- **Break the layout/repr cycle first.** Extract `repr_of_mono` /
  `TypedVec`-classification into a shared module below both `repr_assign` and
  `wasm_layout` (see §1). Nothing else can be built cleanly until the dependency
  direction is settled.
- Add the typed-vec family to `ReprKind` (`backend/prepared_ir.tw`);
  `repr_of_mono` assigns `Vector<Int> → TypedVec(I64)` unconditionally.
- **Repr-driven helper/builtin selection.** Because every `Vector<Int>` slot is
  now `TypedVec(I64)`, emission must select the `_i64` builder/push/freeze/len/get
  helpers from the slot repr (`codegen/emit/arrays.tw`, `runtime_abi.tw`,
  `builtins.tw`) — the job `route_typed_vec` used to do by rewriting calls. Without
  this, typed slots feed boxed helpers and produce mismatches or forced
  erase/retype churn.
- `wasm_layout` derives record / variant field wasm-types from element repr → those
  payloads physically hold `PVecI64`.
- Replace `route_typed_vec`'s conservative eligibility with a **repr-diff-driven**
  coercion inserter: every `Vector<Int>` is typed by default, and coercions appear
  only where a typed vector meets a universal-ABI position. Needs **both**
  adapters — `box_i64` (`PVecI64 → PVec`) and the new `unbox_i64` (`PVec →
  PVecI64`) for results returning from universal helpers. Closure captures are
  *not* yet typed, so a typed vector entering a closure env erases here (one
  coercion) — a correctness path, not the hot path, until `1b`.
- Generalize the verifier to the coercion model (reject raw casts across reprs).
- **Gate:** `typed_variant_column_probe` is realized end-to-end (the probe extracts
  the column and reads it directly, so it does not depend on `1b`); add a
  variant-payload positive/negative probe pair; the `order_by` `gather`/`take`
  portions improve while the `sort` portion is unchanged (still gated on `1b`);
  self-host reaches fixed point and the boot suite is green.

#### Milestone 1b — Typed closure-env layouts

The comparator-capture path, and the largest/highest-risk piece.

- Add per-closure env layouts **keyed by capture reprs** (not by
  `MonoType.Function`), threaded from `closure_convert` where the free variables
  and their reprs are known (§1); type each capture read/write site accordingly.
- Prove it on a standalone closure-capture probe (positive/negative) before wiring
  through the `sort_by` comparator path.
- **Gate:** the closure-capture probe is typed; `order_by`'s `sort` drops toward
  ~540ms and the full path toward the ~1500ms ceiling; self-host + boot suite green.

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

Boot-only, and not because stage0 shares the model — it is boot-only because all
of this is codegen / layout / emit inside boot's *own* Twinkle source. Stage0
compiles boot as ordinary Twinkle regardless of boot's internal repr choices, so
the typed-vector representation is something the resulting `boot.wasm` *does*, not
something stage0 must replicate. (Stage0's persistent vectors are in fact still
`anyref`.) The semantics of every program are unchanged — box/unbox roundtrips
preserve values, which the probe's `match=true` validates. Self-host convergence
is the gate; no stage0 changes.

## Risks and open questions

- **Typed closure-env layouts (Milestone 1b) are the riskiest piece.** Keying an
  env type by capture reprs (not function signature) touches `closure_convert`, the
  env struct/type generation, and every capture read/write site. It is on the
  dataframe critical path — the headline `order_by` `sort` win is not delivered
  until `1b` lands — which is why it is split out behind the lower-risk `1a`
  mechanism and gated on a standalone closure-capture probe first.
- **Coercion completeness, both directions.** Milestone 1a's post-pass must insert a
  coercion at *every* typed-vs-erased crossing — including re-typing boxed `PVec`
  results returning from universal helpers via the new `unbox_i64` adapter — or the
  verifier will reject (or, worse, a raw cast traps). The verifier generalization is
  the safety net; land it alongside the inserter and both adapters, not after.
- **Layout/repr module layering.** The `repr_assign → wasm_layout` import direction
  means the physical-repr policy must be extracted to a shared lower module before
  `wasm_layout` can consult element reprs. Getting this wrong surfaces as an import
  cycle at build time, so it is the first task, not a cleanup.
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
- **Reuses the metadata-threading *pattern*** — not the container design — from
  stage0's [../archive/vector-backend-repr-inference.md](../archive/vector-backend-repr-inference.md),
  which built repr scaffolding (`ValueRepr`/`SumRepr`) and a sort scratch buffer
  but never a typed persistent vector container.
