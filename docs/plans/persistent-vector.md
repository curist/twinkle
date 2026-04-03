# Persistent Vector Plan

## Goal

Replace the current flat copy-on-write vector backing with a persistent vector
data structure, while preserving Twinkle `Vector<T>` semantics and API.

For concrete monomorphized programs, `Vector<T>` should also move away from
generic `anyref` element storage: the long-term target is a
per-instantiation container family, so `Vector<Int>`, `Vector<String>`, and
`Vector<Record...>` can use distinct runtime container/node types with typed
element slots instead of boxing every element through `anyref`.

This plan is subordinate to
[`backend-anyref-elimination.md`](backend-anyref-elimination.md). If the two
documents disagree, the backend `anyref` elimination plan wins. Transitional
erased fallbacks are allowed during migration, but they are not part of the
intended end state for supported concrete vector code paths.

## Current State

- `Vector<T>` is backed by `rt.arr` using a flat Wasm GC array (`rt_types__Array`).
- `set`, `concat`, and `slice` allocate and copy.
- The boot compiler's current collection contract is split:
  - user-facing `Vector.append/get/set/make` are compiler intrinsics
  - raw runtime helpers are `len`, `set` (`vector_set_unsafe`), `concat`, `slice`
  - `collect` and loop-push rewrites depend on `builder_new`, `builder_from`,
    `builder_push`, and `builder_freeze`
- Indexed assignment `xs[i] = v` currently lowers to the raw persistent update helper
  (`vector_set_unsafe`), while safe `Vector.set` remains an intrinsic that wraps
  bounds checks and returns `Option<Vector<T>>`.
- Runtime implementation lives in `boot/compiler/codegen/runtime/arr.tw` and mirrors
  stage0 `src/runtime/arr.rs`.
- Element storage is erased through `anyref`, so scalar payloads are boxed at container
  boundaries and recovered on reads.

## Target State

Use a persistent tree-backed vector (bit-partitioned trie; optional RRB extension later):

- `get/set`: near O(log32 N)
- `push`: near O(1) amortized persistent update
- Structural sharing across versions
- Keep surface methods and indexing behavior unchanged
- Preserve the current split between:
  - safe user-facing operations (`Vector.get`, `Vector.set`)
  - raw backend helpers used by lowering/optimization (`vector_set_unsafe`,
    builders, optional in-place fast paths)
- For concrete monomorphized instantiations, prefer typed container layouts over
  `Array<anyref>` payload storage:
  - `Vector<Int>` should store `i64` elements directly
  - `Vector<String>` should store `ref $String` elements directly
  - `Vector<Point>` should store `ref $Point` elements directly
- For supported concrete `T`, the steady-state backend target is fully typed
  vector/container/helper families with no backend-internal `anyref` element storage.
- Transitional erased fallback may exist only during migration for unsupported or
  not-yet-specialized `T`; per `backend-anyref-elimination.md`, it is not the
  intended long-term model for ordinary concrete code.

The long-term ownership split is:

- semantic vector behavior in ordinary Twinkle library code (`boot/lib`)
- low-level typed storage/runtime helpers in compiler-owned substrate modules

Those substrate modules may be authored in Rust or in Twinkle through the Wasm
IR layer, but they should remain raw helper families rather than the semantic
home of the persistent vector algorithm.

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

- Update `boot/compiler/codegen/runtime/arr.tw` and stage0 `src/runtime/arr.rs`:
  - `make`, `get`, `set`, `concat`, `slice`, `len` to operate on persistent vector structure.
  - Preserve the current boot compiler runtime surface:
    - required raw helpers: `len`, `set`, `concat`, `slice`
    - required builder helpers: `builder_new`, `builder_from`, `builder_push`,
      `builder_freeze`
    - parity helpers kept if still used by stage0 or future lowering: `make`, `get`
  - Implement path-copy update for `set`.
  - Keep `push` support compatible with the current intrinsic/builder split:
    direct `vector_push` plus the builder fast path used by `collect` and loop rewrites.
  - Split helper families by concrete vector layout where needed (`*_i64`, `*_str`, etc.),
    or equivalent internal dispatch generated from the type key.
  - Avoid boxing/unboxing on element read/write paths for specialized families.

### Task C: Builder Intrinsics Alignment

- Rework `builder_new`, `builder_from`, `builder_push`, `builder_freeze` to build the new persistent vector efficiently.
- Preserve current optimizer/lowerer contract:
  - `VECTOR_BUILDER_NEW/FROM/PUSH/FREEZE` in IR remains valid.
- Treat builder ops as part of the current required backend surface, not an optional
  optimization layer:
  - `collect` lowering emits them directly
  - loop-region uniqueness rewrites target them directly
- Ensure builder paths also specialize by element layout so `push` into `Vector<Int>`
  does not route through `anyref`.
- Preserve current alias-safety semantics of `builder_from`: seeding a builder from an
  existing persistent vector must not mutate shared structure unless uniqueness has
  already been proved elsewhere. If the builder needs writable transient state, it must
  detach first or maintain separate transient-only nodes.

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
- Keep `vector_set_in_place` available as the optimizer-only fast path, even if it is
  implemented as a thin wrapper over the persistent representation at first.
- Verify uniqueness rewrites still target the correct specialized helper family.

### Task F: Specialization Policy

- Define which `Vector<T>` instantiations must get dedicated layouts.
- Minimum target:
  - scalar element types (`Int`, `Float`, `Bool`, `Byte`)
  - `String`
  - concrete record/sum refs
  - typed closure refs where those are already available in the backend
- Define fallback:
  - temporary erased/universal vector family only for genuinely non-concrete or
    unsupported `T` during migration
  - fallback should be the exception, not the default
  - fallback must be removed for supported concrete families once their typed layouts
    land; it is not part of the target architecture

## Validation

- Existing vector tests continue to pass:
  - `tests/run/vectors.tw`
  - `tests/opt/*vector*`
- Keep coverage for compiler hooks, not just user-visible methods:
  - `collect` over vectors still lowers through builder ops correctly
  - indexed assignment still preserves persistent semantics
  - uniqueness rewrites still preserve semantics when switching to in-place helpers
- Add performance-focused correctness tests:
  - Deep append chains
  - Repeated branching versions (structural sharing safety)
  - Large index updates and slices
- Add representation-focused tests:
  - `Vector<Int>` read/write path contains no element boxing
  - `Vector<String>` read/write path contains no element boxing
  - `builder_from` seeded from an existing vector does not mutate aliases
  - temporary fallback vector path, if still present during migration, works only for
    intentionally erased cases
- Update runtime/build snapshots for changed runtime type layout.

## Staging

1. Add new runtime types and keep old flat operations compiling.
2. Introduce dedicated outer vector refs and planner/codegen plumbing.
3. Specialize scalar element families first (`Int`, `Float`, `Bool`, `Byte`).
4. Move `get/set/len` onto the specialized persistent representation.
5. Move `push` + builder path onto the specialized representation.
6. Add `String` and common ref-element families.
7. Move `concat/slice`.
8. Restrict transitional erased fallback to unsupported/non-concrete cases only, then
   remove it for supported concrete families.
9. Update snapshots and perf baseline.

## Risks

- Type-layout churn impacts many snapshots.
- Incorrect node/tail boundary logic can cause subtle indexing bugs.
- Builder and uniqueness rewrites can regress if assumptions diverge from runtime semantics.
- Per-instantiation container families increase codegen/runtime surface area and may require
  helper-family generation or internal dispatch.
- Specializing too broadly without profiling could increase code size more than it helps;
  staging should prioritize hot element families.
