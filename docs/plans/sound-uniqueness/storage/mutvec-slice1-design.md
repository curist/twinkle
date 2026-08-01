# MutVec slice 1 — design spec

**Status:** Design approved 2026-08-01; ready for an implementation plan.
**Track:** Storage representation (S1/S2/S3) — see [README.md](README.md).
**Backed by:** [spike-tier0-vector.md](spike-tier0-vector.md) (unboxed flat mutation
15–35× faster than boxed PVec `set_in_place`; cheap one-time freeze; no meaningful
crossover → `MutVec` is an unconditional vector win).

## Goal and scope

Deliver the first end-to-end `MutVec` slice: a proven-owned local `Vector<Int>`
is born in a flat, unboxed, mutable Wasm GC representation, mutated in place
across a region, and materialized back to a persistent `PVecI64` only at the
region's boundary — reproducing the spike's speedup on a compiled program.

**In scope (slice 1):**

- Element type **`Int`** only (`MutVecI64`, backed by the existing `ArrayI64` GC
  array).
- Region shape: an owned local vector born from a `collect` / `Vector.make` /
  `[]`-seed producer, carrying **at least one indexed-update** (`xs[i] = v`),
  optionally with **append** (`xs = xs.append(v)`), then **frozen to `PVecI64` at a
  single boundary**, after
  which the persistent value — never the mutable handle — flows to its consumer (a
  return, or a use/call expecting a persistent `Vector`).
- A dedicated ANF→ANF region pass that performs the representation change and
  relocates the materialization freeze to the boundary.

**Out of scope (later slices / tracks):**

- `Bool` / `Float` / boxed element families (S3 follow-ups).
- Thaw-from-`PVec` entry for param-sourced owned vectors (S4).
- Owned-specialized mutable ABI across calls (S4).
- Standalone append-only accumulator loops (`acc = []; for { acc = .append(v) }`)
  that carry **no** indexed-update — these stay on the existing boxed builder in
  slice 1 (see Non-goals / regression).
- Dict storage (S5).

## Success criteria

- The `boot/bench/mutvec_spike.tw` indexed-update shape, when written as ordinary
  owned Twinkle (`collect` + `xs[i]=v` loop + return), compiles to `mutvec_*` ops
  and reproduces the spike's order-of-magnitude mutate speedup on a compiled
  program (~30× target at large n).
- Self-host reaches a fixed point (`make stage2` / boot test suite green).
- `twk ir --census --sites` shows new `mutvec` region rows for claimed sites;
  rows for all **unclaimed** sites (existing builder regions, `set_in_place`,
  persistent) are unchanged. The current census is update-site oriented and
  builder-region facts surface separately, so this needs a defined region-audit
  row (at minimum: region/proof id, producer/begin site, backing family, each
  in-region op site, the materialization exit, `would_use`/`consumed` state). The
  **exact row shape and the surfacing point for the region decision records are
  specified by the implementation plan**, not this design; the acceptance
  requirement is that claimed regions are auditable and unclaimed rows are a clean
  diff.
- No `PVecI64 → PVec → PVecI64` (or boxed) round-trip is introduced on the hot
  path.

## Architecture

Three interlocking pieces land together, because `MutVec` is a *representation
change*, not a call swap: the value must physically be a `MutVec` across its whole
region, so the substrate (S3), the region rewrite (S2), and the repr-aware guard
(S1) cannot be separated in a first slice.

### 1. Why this is a natural extension of existing lowering

`collect i in range(n) { i }` already lowers to
`builder_new_i64 → builder_push_i64 (loop) → builder_freeze_i64`. The freeze sits
immediately after the producer. When the collect result then feeds an owned
indexed-update / append region, the rewrite is:

1. retype the producer's builder as a `MutVecI64`;
2. **slide the freeze down** past the owned region to its true boundary;
3. route in-region `xs[i]=v` → `mutvec_set_i64` and `xs=.append(v)` →
   `mutvec_push_i64`.

The freeze that used to sit after the producer becomes the materialization
boundary. "Stay low, materialize at the boundary" falls directly out of relocating
that one freeze.

### 2. S3 — runtime substrate (`boot/compiler/codegen/runtime/arr.tw`)

A new GC struct type, `Int` family only for slice 1:

```
rt_types__MutVecI64 = struct {
  data: ref (array (mut i64)),   // ArrayI64 backing (existing GC array type)
  len:  i32 (mut),               // logical length; capacity == array.len(data)
}
```

Runtime ops (each mutates the struct in place via `struct.set` / `array.set` and
returns the same ref, for ANF SSA threading):

| op | signature | notes |
|---|---|---|
| `mutvec_new_i64` | `(cap: i32) -> MutVecI64` | allocate `ArrayI64(max(cap, MIN_CAP))`, `len = 0`; `cap = 0` (the `[]`-seed) is allowed and rounds up to `MIN_CAP`. `cap` is a capacity hint, **not** a length |
| `mutvec_make_i64` | `(n: i32, fill: i64) -> MutVecI64` | `actual_len = max(0, n)`; allocate `ArrayI64(max(actual_len, MIN_CAP))`, fill `data[0..actual_len) = fill`, `len = actual_len`. The producer for `Vector.make(n, v)`, mirroring `pvec_make_fn`. **Negative `n` must yield an empty vector**, matching the current signed-`Int` `pvec_make` loop (its `i >= n` guard breaks immediately for `n <= 0`) — storing a negative `len` would corrupt `mutvec_len_i64` and every bounds check. Without this op, rewriting `Vector.make` to `mutvec_new_i64(n)` (len 0) would make a following `xs[i]=v` trap |
| `mutvec_push_i64` | `(mv, x: i64) -> mv` | if `len == capacity` grow first (see growth), then `data[len] = x; len += 1` |
| `mutvec_set_i64` | `(mv, i: i32, x: i64) -> mv` | **explicit logical-length check**: trap if `i < 0 || i >= len`, else `data[i] = x`. Not a bare `array.set` — `capacity >= len`, so relying on Wasm array bounds would wrongly permit writes in `[len, capacity)` |
| `mutvec_get_i64` | `(mv, i: i32) -> i64` | **explicit logical-length check**: trap if `i < 0 || i >= len`, else `data[i]` |
| `mutvec_len_i64` | `(mv) -> i32` | `len` |
| `mutvec_freeze_i64` | `(mv) -> PVecI64` | O(n) build via the existing typed builder (`builder_push_i64_raw`), reading `data[0..len]` |

**Bounds:** `capacity == array.len(data)` and is always `>= len`. `set`/`get`
trap at the *logical* length via an explicit compare so semantics match `Vector`
OOB exactly; the Wasm array bound is a weaker backstop only.

**Growth:** `new_capacity = max(MIN_CAP, capacity * 2)` — the `max` is required
because `capacity * 2 == 0` when `capacity == 0`, so a `[]`-seed (`mutvec_new_i64(0)`
→ `MIN_CAP`) and doubling both stay well-defined. `MIN_CAP` is a small constant
(e.g. 4). Growth copies via `array.copy` into the larger backing and `struct.set`s
`data`. The freeze reuses the typed builder machinery, so no new persistent-build
code is needed.

**Producer → op mapping** (how each region seed reaches its starting `len`):

- `collect i in range(n) { … }` / `[]`-seed → `mutvec_new_i64(hint)` then a push
  loop; `len` grows from 0 via `mutvec_push_i64` (capacity hint = the range size
  when statically known, else `MIN_CAP`). Reaches `len = n` through pushes.
- `Vector.make(n, v)` → `mutvec_make_i64(n, v)`; `len = n` immediately, so a
  following `xs[i] = v` is in bounds.

### 3. How `MutVecI64` is represented in the compiler (physical repr, not a MonoType)

`MonoType` stays `Vector<Int>` for these values — the source type does not change.
`MutVecI64` is a compiler-private **physical representation**, carried the same way
`PVecI64` already is: today `repr_assign.tw` assigns backend slots a
`ReprKind.TypedVec(ElemRepr)` (`prepared_ir.tw`, `ElemRepr = {I64, F64, I32}` in
`repr_policy.tw`) that is distinct from the boxed `PVec` ABI and from `MonoType`.
Slice 1 adds a sibling `ReprKind` variant (e.g. `MutVec(ElemRepr)`) assigned by the
same machinery:

- the region's producer/handle slot is physically `MutVec(I64)` from the begin
  site up to the freeze;
- the post-freeze slot is `TypedVec(I64)` (`PVecI64`) — the existing typed repr;
- **no slot in the region is `Anyref`**, honoring the S1 representation-aware
  invariant end to end (typed producer → typed mutable region → typed persistent
  result, no boxing detour).

**Stage handoff (two stages, not one).** `mutvec_region.rewrite_module` runs early,
on ANF, and does **not** assign `ReprKind` — `repr_assign` runs later, during backend
preparation (`assign_repr_for_module`, "called after `assign_slots_for_module` in
`prepare_backend()`"). So "retype the producer's builder as `MutVecI64`" is a
two-stage handoff:

1. the ANF pass emits the `mutvec_*` runtime ABI calls and attaches the region
   decision record (naming the handle slots);
2. backend prepare / `repr_assign` consumes those — the `mutvec_*` op ABIs and the
   record — and marks the named slots `MutVec(I64)` (and the post-freeze slot
   `TypedVec(I64)`).

No new `MonoType`, no `Anyref` erasure. The exact record fields `repr_assign` keys
off, and how it threads through `prepare`, are pinned down by the implementation
plan.

### 4. S2/S1 — the dedicated region pass (`boot/compiler/codegen/mutvec_region*.tw`)

A **new** ANF→ANF pass (`mutvec_region.rewrite_module`) that runs **immediately
before** `builder_region.rewrite_module` (see Pipeline placement below), **sharing
detection helpers** with `builder_region_detect` but not modifying it.

- **Detection.** Reusing shared owned/liveness/non-overlap/deadness helpers, find a
  region: an owned local vector born from a supported producer (`collect` /
  `Vector.make` / `[]`-seed), whose in-region ops are all in the supported set and
  all owned, materialized at a freezable boundary (see Exit handling). The region
  must be the **genuinely-new shape**: it must include at least one
  **indexed-update**, which the builder-region pass never claimed.
- **Supported in-region ops.** Mutating: `vector$set_unsafe` (indexed update) and
  append. Non-publishing reads: index read (`mutvec_get_i64`) and length
  (`mutvec_len_i64`) — reads do not escape the handle and do not force
  materialization, so a region may freely contain them. **Any other op on the
  handle** (concat, slice, passing it to a call, storing it in an aggregate, etc.)
  is unsupported and **rejects the region** (fall back to persistent) rather than
  being silently mis-lowered.
- **S1 repr-aware guard.** The region takes the MutVec path only if its element
  type proves to `Int`. Otherwise it is not claimed and the existing path applies.
  This is the guard that prevents representation downgrades — a typed site either
  gets `MutVecI64` or stays persistent, never an `anyref` detour.
- **Exit handling (soundness-critical).** The freeze is inserted at the boundary,
  and only the resulting `PVecI64` flows onward — so the distinction is **the
  frozen value vs the still-mutable handle**. A terminal consumer that receives the
  *frozen persistent value* is a valid boundary: a return, or a call/expression
  expecting a persistent `Vector` (the freeze is inserted immediately before it, so
  the callee gets `PVecI64`, its normal ABI). What is **rejected** is the *mutable
  handle* leaving the region — passed to a call while still mutable, captured by a
  closure, stored into an escaping aggregate (record/variant/dict/vector), or
  reachable at more than one exit. "Single publication boundary" therefore means
  the region is claimed **only if every control-flow exit carrying the live handle
  reduces to one freezable point**, not "find one boundary and ignore the rest."
  Slice 1 is conservative: it requires **exactly one** such freeze point and
  rejects (persistent fallback) multiple exits, early `return`, `break value`, a
  `try`/early-return arm, closure capture, escaping-aggregate storage of the
  handle, or a host/import boundary carrying the handle. A rejected region is
  emitted unchanged by the existing passes. Multi-exit freezing is deferred to a
  later slice / S4.
- **Emit.** Producer → `mutvec_new_i64` (collect / `[]`-seed builder) or
  `mutvec_make_i64` (`Vector.make`, see Producer → op mapping); in-region push →
  `mutvec_push_i64`; in-region set → `mutvec_set_i64`; in-region read →
  `mutvec_get_i64`; in-region length → `mutvec_len_i64`; boundary → relocated
  `mutvec_freeze_i64`.
- **Region decision record (full lifecycle schema).** This record is the interface
  the backend/`repr_assign` consumes *without re-proving anything*, so it must carry
  every field the README "Region decision-record lifecycle" requires, not a subset.
  For slice 1 each claimed region names:
  - **stable region id + proof/debug id** (deterministic, for census + audit);
  - **begin site**, **source value**, and **source physical repr** (the producer
    slot's repr before the change — e.g. a boxed builder or `TypedVec(I64)`);
  - **selected private storage family**: `MutVec(I64)`;
  - **every in-region op site** with its storage-compatible operand/result shape
    (`mutvec_new`/`make`/`push`/`set`/`get`/`len`), so no op is emitted against an
    incompatible repr;
  - **the materialization exit** (slice 1: exactly one freezable point) and the
    **post-materialization value** (`TypedVec(I64)` / `PVecI64`);
  - **the handle-invalidation point** — the program point after the freeze beyond
    which the `MutVec` handle is dead and must not be read;
  - **fallback behavior** if the region is later found stale/ambiguous/missing its
    exit: emit the ordinary persistent path.
  - **Validations** the selector runs before accepting the record: no path from
    `begin` reaches a publication sink with an unfrozen handle; no use of the handle
    after materialization; no double-begin for the same live storage and no
    double-freeze of the same handle; all op sites use a compatible family/repr.

  Backend emitters consume the record; they do not re-prove ownership, region exits,
  or handle validity. **The concrete record type (field names/encoding) and exactly
  which fields `repr_assign` keys off are pinned down by the implementation plan**,
  but the schema above is the required content.

### 5. Pipeline placement and non-overlap ordering

The current codegen order is `builder_region.rewrite_module(m, b)` (→ ANF′) →
`variant_specialize` (→ ANF″) → `closure_convert`, with the call-swap /
mutable-decision producer consuming the rewritten ANF (its stale-artifact guard
keys off the ANF′ fingerprint). `builder_region.rewrite_module` takes no exclusion
input and computes its decisions internally.

**Non-overlap contract (chosen: "MutVec erases builder-visible shapes"; the other
two options are rejected for slice 1).** `mutvec_region.rewrite_module` runs
**first**, producing ANF^m, and **fully rewrites every region it claims** — the
producer becomes `mutvec_new_i64`/`mutvec_make_i64`, appends become
`mutvec_push_i64`, indexed updates become `mutvec_set_i64`. Because a claimed
region is fully lowered, **no builder-visible shape (`acc=[]`-seed + append, or a
collect builder chain) survives at those sites**, so `builder_region.rewrite_module`
runs on ANF^m **genuinely unchanged and needs no exclusion parameter** — it simply
finds nothing to claim there. This is why the builder pass stays byte-identical
without modification. (Rejected alternatives: adding an exclusion-set parameter to
the builder API; or a shared up-front decision producer arbitrating both — the
latter is the eventual Approach-A convergence, not slice 1.)

The domains are also disjoint by construction: builder-regions only claim
append-only accumulators (no indexed-update), while MutVec *requires* an
indexed-update. So the erasure argument and the disjointness argument both hold.

Downstream: `variant_specialize` and the call-swap producer consume the
doubly-rewritten ANF. Because a claimed region's `vector$set_unsafe` is already
`mutvec_set_i64`, the call-swap producer sees nothing to claim there and does
**not** emit `set_in_place` for it — no double claim; unclaimed `xs[i]=v` sites
still get `set_in_place` exactly as today. The ordering is deterministic, each
region is claimed by at most one pass, and the stale-artifact guard keys off the
final rewritten ANF fingerprint.

## Non-goals and regression management

Slice 1 is deliberately isolated (Approach B):

- `builder_region_detect` and the `set_in_place` decision path are **not
  modified**; regions they currently claim stay **byte-identical**.
- **Non-overlap contract.** The MutVec pass claims a region only if it can *fully*
  lower it (Int element, all in-region ops supported, boundary identified). A
  region it does not fully own is left entirely to the existing passes. A region is
  claimed by at most one pass.
- Standalone append-only Int loops and String builders keep the boxed builder in
  slice 1. Append gets `MutVec` **only inside a claimed MutVec region** (one that
  also carries indexed-update), matching the slice scope without touching the
  standalone-append path.
- Persistent fallback remains the default for anything unclaimed, stale,
  ambiguous, aliased, escaping, or of an unsupported element type.

## End goal: convergence to a unified pass (Approach A)

**This isolated pass is the first step, not the destination.** The end state is a
**single unified typed-vector-region pass** that subsumes both `mutvec_region` and
`builder_region_detect`: it owns append, indexed-update, and typed flat backing
uniformly across all typed vector regions (including today's standalone append
accumulators), and retires the boxed-builder-for-typed-vectors path. Slice 1 keeps
the two passes separate for regression isolation; a later consolidation slice
merges them once the MutVec path is proven, migrating the standalone append loops
and removing the duplicated detection helpers. The shared-helper structure in
slice 1 is chosen specifically to make that merge cheap.

## Testing

- **Runtime substrate:** targeted codegen/execution coverage for each
  `mutvec_*_i64` op, including grow-by-doubling across the capacity boundary, the
  OOB trap on **both** `mutvec_set_i64` and `mutvec_get_i64` (each `i >= len`, and
  the in-`[len, capacity)` case that a bare `array` bound would miss), and
  `mutvec_make_i64(n, v)` producing `len = n` prefilled (including `n < MIN_CAP`
  and `n = 0`).
- **Region lowering:** positive fixtures — collect-born + indexed-update;
  `Vector.make`-born + indexed-update; +append; in-region `get`/`len`;
  materialize-at-return — assert `mutvec_*` emission and a single boundary freeze.
  **Negative fixtures, one per rejected class**, each asserting the region is **not**
  claimed and the existing path is emitted unchanged:
  - not-owned / aliased handle;
  - element type not `Int`;
  - no indexed-update (append-only — stays on the boxed builder);
  - unsupported in-region op on the handle (concat, slice);
  - multiple exits carrying the live handle;
  - early `return` of the handle;
  - `break value` carrying the handle;
  - `try` / early-return arm carrying the handle;
  - closure capture of the handle;
  - handle stored into an escaping aggregate (record / variant / dict / vector);
  - handle passed to a call (still mutable);
  - handle reaching a host/import boundary.
- **Regression:** `--census --sites` diff shows only additive `mutvec` rows;
  existing builder/`set_in_place`/persistent rows unchanged. Self-host fixed point.
- **Performance (end-of-slice gate):** compiled microbench reproduces the spike's
  mutate speedup; run per the storage-track "performance is an end-of-track gate"
  invariant, not per intermediate step.
