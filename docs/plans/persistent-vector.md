# Persistent Vector Plan

## Goal

Replace the current flat copy-on-write vector backing with a persistent vector data structure, while preserving Twinkle `Vector<T>` semantics and API.

## Current State

- `Vector<T>` is backed by `rt.arr` using a flat Wasm GC array (`rt_types__Array`).
- `set`, `concat`, and `slice` allocate and copy.
- `push` is currently lowered via builder intrinsics to avoid worst-case O(N^2) patterns, but core representation is still flat.
- Runtime implementation lives in `src/runtime/arr.rs`.

## Target State

Use a persistent tree-backed vector (bit-partitioned trie; optional RRB extension later):

- `get/set`: near O(log32 N)
- `push`: near O(1) amortized persistent update
- Structural sharing across versions
- Keep surface methods and indexing behavior unchanged

## Non-Goals

- Changing user-visible `Vector` syntax or method names
- Removing uniqueness optimization pass
- Introducing mutable-only vector semantics

## Data Model (Proposed)

- Add dedicated vector runtime types (in `rt.types`) separate from generic `Array` payload buffers.
- Base representation:
  - `Vector { len: i32, shift: i32, root: Node?, tail: Array<anyref> }`
  - `Node` as fixed-arity (32-way) branching container
- Keep `rt_types__Array` for generic payload arrays and option payloads.

## Implementation Tasks

### Task A: Runtime Type Additions

- Update `src/runtime/types.rs`:
  - Add `Vector` and node-related type definitions.
  - Add `ref_vector()` helpers.
- Keep existing `Array` type for non-vector uses.

### Task B: Rewrite `rt.arr` Operations for Tree Representation

- Update `src/runtime/arr.rs`:
  - `make`, `get`, `set`, `concat`, `slice`, `len` to operate on persistent vector structure.
  - Preserve exported names (`rt_arr__*`) initially to avoid broad call-site churn.
  - Implement path-copy update for `set`.
  - Implement `push` path via tail buffering and root promotion logic.

### Task C: Builder Intrinsics Alignment

- Rework `builder_new`, `builder_from`, `builder_push`, `builder_freeze` to build the new persistent vector efficiently.
- Preserve current optimizer/lowerer contract:
  - `VECTOR_BUILDER_NEW/FROM/PUSH/FREEZE` in IR remains valid.

### Task D: Codegen Type Plumbing

- Update vector valtype mapping in:
  - `src/codegen/ctx.rs`
  - `src/codegen/prelude.rs`
  - `src/codegen/emit.rs`
- Ensure runtime imports for vector ops use `ref_vector` once representation changes.

### Task E: Optimizer Compatibility

- Verify `src/opt/uniqueness.rs` rewrite assumptions still hold.
- Keep in-place rewrite legality tied to uniqueness proofs, but adjust runtime implementation of in-place helpers as needed.

## Validation

- Existing vector tests continue to pass:
  - `tests/run/vectors.tw`
  - `tests/opt/*vector*`
- Add performance-focused correctness tests:
  - Deep append chains
  - Repeated branching versions (structural sharing safety)
  - Large index updates and slices
- Update runtime/build snapshots for changed runtime type layout.

## Staging

1. Add new runtime types and keep old flat operations (compile passes).
2. Move `get/set/len` first (lowest risk).
3. Move `push` + builder path.
4. Move `concat/slice`.
5. Flip codegen valtype to dedicated vector ref type.
6. Update snapshots and perf baseline.

## Risks

- Type-layout churn impacts many snapshots.
- Incorrect node/tail boundary logic can cause subtle indexing bugs.
- Builder and uniqueness rewrites can regress if assumptions diverge from runtime semantics.
