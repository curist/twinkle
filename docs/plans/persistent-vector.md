# Persistent Vector Plan

## Goal

Replace the current flat copy-on-write vector backing with a persistent vector data structure, while preserving Twinkle `Vector<T>` semantics and API.

For concrete monomorphized programs, `Vector<T>` should also move away from generic
`anyref` element storage: the long-term target is a per-instantiation container family,
so `Vector<Int>`, `Vector<String>`, and `Vector<Record...>` can use distinct runtime
container/node types with typed element slots instead of boxing every element through
`anyref`.

## Current State

- `Vector<T>` is backed by `rt.arr` using a flat Wasm GC array (`rt_types__Array`).
- `set`, `concat`, and `slice` allocate and copy.
- `push` is currently lowered via builder intrinsics to avoid worst-case O(N^2) patterns, but core representation is still flat.
- Runtime implementation lives in `src/runtime/arr.rs`.
- Element storage is erased through `anyref`, so scalar payloads are boxed at container
  boundaries and recovered on reads.

## Target State

Use a persistent tree-backed vector (bit-partitioned trie; optional RRB extension later):

- `get/set`: near O(log32 N)
- `push`: near O(1) amortized persistent update
- Structural sharing across versions
- Keep surface methods and indexing behavior unchanged
- For concrete monomorphized instantiations, prefer typed container layouts over
  `Array<anyref>` payload storage:
  - `Vector<Int>` should store `i64` elements directly
  - `Vector<String>` should store `ref $String` elements directly
  - `Vector<Point>` should store `ref $Point` elements directly
- Keep an erased fallback path only where the element type is genuinely not concrete or
  not worth specializing.

## Non-Goals

- Changing user-visible `Vector` syntax or method names
- Removing uniqueness optimization pass
- Introducing mutable-only vector semantics
- Committing to full specialization for every possible type in one step; rollout can be
  staged by high-value element families first.

## Data Model (Proposed)

- Add dedicated vector runtime types (in `rt.types`) separate from generic `Array`
  payload buffers.
- Base representation should be per concrete element layout, not a single universal
  `Vector` with `anyref` payload slots.
- Conceptually:
  - `Vector<T>` lowers to a specialized runtime family derived from `T`
  - `Vector<Int>`:
    - `Vector_i64 { len: i32, shift: i32, root: Node_i64?, tail: Tail_i64 }`
    - `Node_i64` stores fixed-arity `i64` children or child-node refs
    - `Tail_i64` stores direct `i64` elements
  - `Vector<String>`:
    - `Vector_str { ... tail: Tail_str }`
    - `Tail_str` stores `ref $String`
- The exact node split can still evolve, but the important policy is:
  - container and node types are specialized per element instantiation
  - element slots use the element's concrete Wasm type, not `anyref`
- Keep `rt_types__Array` only for genuinely erased payload arrays and compatibility
  boundaries that still require universal storage.

## Implementation Tasks

### Task A: Runtime Type Additions

- Update `src/runtime/types.rs`:
  - Add specialized `Vector_*`, tail, and node-related type definitions.
  - Add `ref_vector()` helpers.
- Add symbol/key derivation for per-instantiation vector families.
- Keep existing `Array` type for non-vector uses and erased fallback paths.

### Task B: Rewrite `rt.arr` Operations for Tree Representation

- Update `src/runtime/arr.rs`:
  - `make`, `get`, `set`, `concat`, `slice`, `len` to operate on persistent vector structure.
  - Preserve exported names (`rt_arr__*`) initially to avoid broad call-site churn.
  - Implement path-copy update for `set`.
  - Implement `push` path via tail buffering and root promotion logic.
  - Split helper families by concrete vector layout where needed (`*_i64`, `*_str`, etc.),
    or equivalent internal dispatch generated from the type key.
  - Avoid boxing/unboxing on element read/write paths for specialized families.

### Task C: Builder Intrinsics Alignment

- Rework `builder_new`, `builder_from`, `builder_push`, `builder_freeze` to build the new persistent vector efficiently.
- Preserve current optimizer/lowerer contract:
  - `VECTOR_BUILDER_NEW/FROM/PUSH/FREEZE` in IR remains valid.
- Ensure builder paths also specialize by element layout so `push` into `Vector<Int>`
  does not route through `anyref`.

### Task D: Codegen Type Plumbing

- Update vector valtype mapping in:
  - `src/codegen/ctx.rs`
  - `src/codegen/prelude.rs`
  - `src/codegen/emit.rs`
- Ensure runtime imports for vector ops use per-instantiation vector refs once
  representation changes.
- Add layout/planner support so monomorphized `Vector<T>` picks the specialized
  container family from the concrete element type.
- Keep explicit boundary conversions only for erased fallback cases, not for normal
  element reads/writes in specialized vectors.

### Task E: Optimizer Compatibility

- Verify `src/opt/uniqueness.rs` rewrite assumptions still hold.
- Keep in-place rewrite legality tied to uniqueness proofs, but adjust runtime implementation of in-place helpers as needed.
- Verify uniqueness rewrites still target the correct specialized helper family.

### Task F: Specialization Policy

- Define which `Vector<T>` instantiations must get dedicated layouts.
- Minimum target:
  - scalar element types (`Int`, `Float`, `Bool`, `Byte`)
  - `String`
  - concrete record/sum refs
  - typed closure refs where those are already available in the backend
- Define fallback:
  - erased/universal vector family only for genuinely non-concrete or unsupported `T`
  - fallback should be the exception, not the default

## Validation

- Existing vector tests continue to pass:
  - `tests/run/vectors.tw`
  - `tests/opt/*vector*`
- Add performance-focused correctness tests:
  - Deep append chains
  - Repeated branching versions (structural sharing safety)
  - Large index updates and slices
- Add representation-focused tests:
  - `Vector<Int>` read/write path contains no element boxing
  - `Vector<String>` read/write path contains no element boxing
  - fallback vector path still works for intentionally erased cases
- Update runtime/build snapshots for changed runtime type layout.

## Staging

1. Add new runtime types and keep old flat operations compiling.
2. Introduce dedicated outer vector refs and planner/codegen plumbing.
3. Specialize scalar element families first (`Int`, `Float`, `Bool`, `Byte`).
4. Move `get/set/len` onto the specialized persistent representation.
5. Move `push` + builder path onto the specialized representation.
6. Add `String` and common ref-element families.
7. Move `concat/slice`.
8. Retain erased fallback only for unsupported/non-concrete cases.
9. Update snapshots and perf baseline.

## Risks

- Type-layout churn impacts many snapshots.
- Incorrect node/tail boundary logic can cause subtle indexing bugs.
- Builder and uniqueness rewrites can regress if assumptions diverge from runtime semantics.
- Per-instantiation container families increase codegen/runtime surface area and may require
  helper-family generation or internal dispatch.
- Specializing too broadly without profiling could increase code size more than it helps;
  staging should prioritize hot element families.
