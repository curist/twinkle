# Representation-Boundary Policy

**Status:** Revised 2026-07-04 after the uniform-typing approach was built,
measured, and **reverted**. This doc previously proposed making
`Vector<Int>`'s physical representation a pure function of its semantic type
(`Vector<Int> = PVecI64` everywhere). That premise is unsound-for-performance and
has been rolled back. The corrected direction — **typed vectors are a
storage-site representation, not the canonical representation of `Vector<Int>`
everywhere** — is described below.

Related: [backend-anyref-elimination.md](backend-anyref-elimination.md) (Phase 1
this partially answers), [vector/typed-vector-representation.md](vector/typed-vector-representation.md),
and the failure post-mortem
[vector/m1a-anyref-readback-investigation.md](vector/m1a-anyref-readback-investigation.md).

## What was tried and why it failed

The "M1a activation" (branch `typed-vector-repr-m1a`, commit `f26cd7a3`, now
reverted) flipped `repr_of_mono`/`val_type_of_mono` so every `Vector<Int>` slot,
param, result, field, and variant payload became `PVecI64` unconditionally, and
removed the conservative `route_typed_vectors` pass in favor of uniform typing
plus `emit_coerce_stack` boundary coercions.

It self-hosted and passed all tests, but was **catastrophically slow** on any
workload that reads a captured `Vector<Int>` in a loop:

- Closure environments store `anyref` (`ClosureEnv = array anyref`). A captured
  `Vector<Int>` is boxed to `PVec` on capture, then **must be `unbox_i64`'d (a
  full O(n) rebuild) on every read-back** to satisfy its `PVecI64` static type.
- A 200k-iteration loop reading a captured 5000-element vector took **~8100 ms**
  (should be ~5 ms). `sort_by(fn(a,b){ Int.compare(keys[a], keys[b]) })` captures
  the key column, so `order_by` at N=1M went from ~2.3 s to a multi-minute hang.

The eager unbox is **required** for type-correctness once the type says
`PVecI64` — the env genuinely holds a boxed `PVec`. So it is not a bug to patch;
it is the direct consequence of the uniform-typing premise.

**Lesson:** uniform `Vector<Int> = PVecI64` is unsound-for-performance while any
durable storage boundary is still universal `anyref`. The original conservative
`route_typed_vec` was conservative *for a reason* — it only typed vectors that
never escape, precisely to avoid anyref read-back cost.

## Corrected thesis

`PVec` (boxed, `anyref` leaves) is the **canonical** representation of
`Vector<T>`. `TypedVec`/`PVecI64` is an **optimization representation** permitted
only where the storage/call path preserves typed representation, or where escape
analysis proves read-back through `anyref` is not hot/repeated.

Representation is therefore **storage-site dependent, not globally
type-dependent**:

```text
semantic type: Vector<Int>          (always)
physical repr: PVec  or  PVecI64    (depends on the slot/storage site)
```

`SlotInfo.repr` / `SlotInfo.wasm_type` already exist to carry exactly this
distinction. The redesign leans into that mechanism instead of bypassing it with
an unconditional `val_type_of_mono(Vector<Int>) = PVecI64`.

## What is kept (reusable infrastructure, currently inert or conservative)

The activation revert preserves everything except the unconditional flip:

- `PVecI64` runtime type and the `_i64` vector helpers (builder/freeze/len/get).
- `rt_arr__box_i64` / `rt_arr__unbox_i64` as explicit boundary adapters.
- `ReprKind.TypedVec(ElemRepr)` and verifier support for typed vector slots.
- Typed emission paths for literals, `collect`/builders, and indexed reads.
- `emit_coerce_stack` PVec↔PVecI64 arms as a correctness tool.
- The conservative `route_typed_vectors` pass, restored as the shippable baseline
  (types only non-escaping locals + the S2.1/S2.2 boundary-adapter cases).

## Hybrid typed-storage policy (the redesign)

Use the typed `PVecI64` representation **only where the destination/storage is
itself typed**:

- local non-escaping vectors (already done — `route_typed_vec` S2.0);
- typed record fields (already done — S2.2);
- typed sum payloads *iff the sum layout physically stores `PVecI64`*;
- direct typed function params/results (S2.1);
- typed closure environments (later — see M1b).

Force the boxed `PVec` representation when crossing a **durable erased
boundary**, because typed storage cannot be preserved there and read-back would
rebuild:

- universal `anyref`;
- universal closure env (`ClosureEnv = array anyref`);
- generic container payloads (`Vector<anyref>`, etc.);
- erased `Variant`;
- unknown ABI / export / import paths.

The compiler already distinguishes semantic type from physical repr in
`SlotInfo`; the policy is to compute the physical repr per storage site (typed
when provably preserved, boxed otherwise) rather than from the mono type alone.

## Staging

### Short term — conservative baseline (done)

`route_typed_vectors` is restored as the shippable baseline: existing typed-local
and typed-record-field wins, no captured-vector catastrophe, with the typed
runtime/codegen pieces retained for reuse.

### Medium term — storage-site typed policy

Make representation a per-storage-site decision:

- teach the layout/repr layer to emit `PVecI64` for a `Vector<Int>` slot/field/
  payload only when that site is a typed container that stores `PVecI64`, and
  `PVec` otherwise;
- keep `box_i64`/`unbox_i64` at the *few* real crossings (once per crossing,
  never per read);
- **unify the three repr paths** (`repr_of_mono`, `repr_of_named_cached`,
  `cached_repr_of_mono` in `backend/repr_assign.tw`) behind one classifier so
  public and cached logic cannot diverge (the activation exposed a divergence
  where the cached path was not flipped).

### Longer term — typed closure environments (M1b)

For hot closures, store captured `Vector<Int>` as `PVecI64` in the closure object
so the typed trampoline reads `PVecI64` directly (no `anyref` round-trip):

```text
closure object stores captured keys as PVecI64
typed trampoline reads PVecI64 directly → lambda receives PVecI64 → get_i64
```

This fixes `sort_by(fn(a,b){ keys[a] … })`. The universal fallback still stores/
reads boxed `PVec`. **This does not solve every `anyref` case** (erased sums,
generic anyref), so it is not the full uniform-typing fix — it is one typed
boundary among several.

## Boundary between "optimization" and "canonical"

> `TypedVec` is an optimization representation. The canonical representation of
> `Vector<Int>` remains `PVec`. Typed representation is allowed only where the
> compiler can prove or encode typed storage.

Everything the compiler cannot prove typed stays boxed `PVec` — which is correct,
and cheap on read-back, by construction.

## Measurement context (still valid)

`examples/performance/sort-bench/typed_variant_column_probe.tw` and
`typed_vec_read_probe.tw` show the read-wall win is real where storage is typed
(~7× on non-escaping typed reads). The dataframe `order_by` ceiling analysis
(sort dominated by boxed key reads) still motivates typed *columns* — but only
via typed storage sites (typed variant payloads + typed closure envs), not via
uniform typing. `m1a-anyref-readback-investigation.md` holds the failure repro.
