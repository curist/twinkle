# Persistent Dict (HAMT) Plan

## Goal

Replace the current linear assoc-list dictionary implementation with a persistent HAMT, preserving Twinkle `Dict<K,V>` behavior.

## Current State

- Runtime dict is an unsorted association list over array entries.
- `get/has/set/remove` are linear scans in `src/runtime/dict.rs`.
- Key comparison uses structural equality (`rt.core.eq`).
- Type checker restricts keys to `Int | String`.

## Target State

Adopt a persistent hash array mapped trie (HAMT):

- `get/has/set/remove`: near O(1) average, O(log32 N) worst structural depth
- Structural sharing for persistent updates
- Stable semantics for iteration/key listing (`Dict.keys`)

## Non-Goals

- Expanding key types beyond `Int | String`
- Exposing hash behavior at language surface
- Requiring host-specific hash primitives

## Data Model (Proposed)

- `Dict` stores:
  - `size: i32`
  - `root: Node?`
- Node variants:
  - Bitmap indexed node
  - Collision node (same hash, different keys)
  - Leaf entry (`key`, `value`, cached `hash`)

## Hashing Strategy

- `Int`: stable 64-bit mix -> 32-bit hash
- `String`: runtime UTF-8 hash (deterministic, host-independent)
- Collision handling via collision nodes and full key equality check

## Implementation Tasks

### Task A: Runtime Types for HAMT Nodes

- Update `src/runtime/types.rs`:
  - Add dict node structs/variants needed for HAMT.
  - Keep external `Dict` ref helper stable (or migrate with coordinated codegen changes).

### Task B: Reimplement `rt.dict`

- Rewrite `src/runtime/dict.rs`:
  - `make`: empty root, size 0
  - `get/has`: trie walk by hash fragments
  - `set`: path-copy insert/replace, size delta tracking
  - `remove`: path-copy delete + node compaction
  - `len`: O(1) from stored size
  - `keys`: deterministic traversal order (define and test)

### Task C: Hash + Equality Integration

- Add internal hash helpers in runtime (likely `rt.core` or `rt.dict` local helpers).
- Keep final key match guarded by existing equality semantics (`rt.core.eq`).
- Ensure string hashing matches UTF-8 byte representation used in runtime strings.

### Task D: In-Place Rewrite Compatibility

- Keep optimizer contract in `src/opt/uniqueness.rs`:
  - `DICT_SET` -> `DICT_SET_IN_PLACE`
  - `DICT_REMOVE` -> `DICT_REMOVE_IN_PLACE`
- Redefine in-place helpers in HAMT runtime as safe destructive path mutation only when uniqueness guarantees hold.

### Task E: Prelude and Snapshot Alignment

- Keep existing prelude IDs and runtime symbols (`rt_dict__*`) to minimize frontend impact.
- Update snapshots and runtime dump expectations after type/function body changes.

## Validation

- Existing dict behavior tests pass:
  - `tests/run/dicts.tw`
  - `tests/run/dict_methods.tw`
  - `tests/opt/*dict*`
- Add HAMT-specific tests:
  - Hash collision scenarios
  - Deep trie path updates/removes
  - Structural sharing sanity (older versions remain intact)
  - Deterministic `Dict.keys` ordering checks

## Staging

1. Introduce hash helpers and node types.
2. Implement `get/has/len` on HAMT.
3. Implement `set/remove` persistent path-copy.
4. Implement in-place helper variants for uniqueness rewrite path.
5. Finalize `keys` traversal and ordering guarantees.
6. Update snapshots and perf baseline.

## Risks

- Collision node correctness bugs can be hard to detect without targeted tests.
- Deterministic iteration order must be explicitly specified and preserved.
- In-place path mutation must remain strictly guarded by uniqueness proofs.
