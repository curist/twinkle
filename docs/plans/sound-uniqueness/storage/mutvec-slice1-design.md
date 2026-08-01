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
  `[]`-seed producer, undergoing **indexed-update** (`xs[i] = v`) and/or **append**
  (`xs = xs.append(v)`), then materialized at a single publication boundary
  (return, or last use feeding a persistent-typed consumer).
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
  persistent) are unchanged.
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
| `mutvec_new_i64` | `(cap: i32) -> MutVecI64` | allocate `ArrayI64(cap)`, `len = 0` |
| `mutvec_push_i64` | `(mv, x: i64) -> mv` | grow-by-doubling (`array.copy` into 2× backing) when `len == cap`, then `data[len]=x; len++` |
| `mutvec_set_i64` | `(mv, i: i32, x: i64) -> mv` | `data[i] = x` (traps on `i >= len`, matching `Vector` OOB) |
| `mutvec_get_i64` | `(mv, i: i32) -> i64` | `data[i]` |
| `mutvec_len_i64` | `(mv) -> i32` | `len` |
| `mutvec_freeze_i64` | `(mv) -> PVecI64` | O(n) build via the existing typed builder (`builder_push_i64_raw`), reading `data[0..len]` |

Growth policy: doubling, small initial capacity (or a producer-supplied size hint
when the region is born from `collect range(n)` / `Vector.make(n, …)`). The freeze
reuses the typed builder machinery, so no new persistent-build code is needed.

### 3. S2/S1 — the dedicated region pass (`boot/compiler/codegen/mutvec_region*.tw`)

A **new** ANF→ANF pass, run at the same stage as the builder-region rewrite,
**sharing detection helpers** with `builder_region_detect` but not modifying it.

- **Detection.** Reusing shared owned/liveness/non-overlap/deadness helpers, find a
  region: an owned local vector born from a supported producer (`collect` /
  `Vector.make` / `[]`-seed), whose in-region ops are all supported
  (`vector$set_unsafe`, append) and all owned, with a single materialization
  boundary. The region must be the **genuinely-new shape**: it must include at
  least one **indexed-update**, which the builder-region pass never claimed.
- **S1 repr-aware guard.** The region takes the MutVec path only if its element
  type proves to `Int`. Otherwise it is not claimed and the existing path applies.
  This is the guard that prevents representation downgrades — a typed site either
  gets `MutVecI64` or stays persistent, never an `anyref` detour.
- **Emit.** Producer builder → `mutvec_new_i64`; in-region push → `mutvec_push_i64`;
  in-region set → `mutvec_set_i64`; reads → `mutvec_get_i64`; boundary → relocated
  `mutvec_freeze_i64`.
- **Region decision record.** Each claimed region carries a record naming the
  producer/begin site, the backing family (`MutVecI64`), every in-region op site,
  the single materialization exit, and the post-freeze `PVecI64` value — satisfying
  the storage-track lifecycle contract (README "Region decision-record
  lifecycle"). Backend emitters consume the record; they do not re-prove ownership.

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
  `mutvec_*_i64` op, including grow-by-doubling across the capacity boundary and
  the OOB trap on `mutvec_set_i64`.
- **Region lowering:** positive fixtures (collect-born + indexed-update; +append;
  materialize-at-return) assert `mutvec_*` emission and a single boundary freeze;
  negative fixtures (aliased / escaping / non-Int / no-indexed-update) assert the
  region is **not** claimed and the existing path is emitted unchanged.
- **Regression:** `--census --sites` diff shows only additive `mutvec` rows;
  existing builder/`set_in_place`/persistent rows unchanged. Self-host fixed point.
- **Performance (end-of-slice gate):** compiled microbench reproduces the spike's
  mutate speedup; run per the storage-track "performance is an end-of-track gate"
  invariant, not per intermediate step.
