# Persistent Dict (HAMT) Plan

## Goal

Replace the current linear assoc-list dictionary implementation with a persistent HAMT, preserving Twinkle `Dict<K,V>` behavior.

For concrete monomorphized programs, `Dict<K,V>` should also move away from universal
`anyref` key/value storage: the long-term target is a per-instantiation container family,
so `Dict<String, Int>`, `Dict<Int, String>`, and other concrete pairs can use typed node
layouts and avoid boxing every key/value through `anyref`.

## Current State

- Runtime dict is an unsorted association list over array entries.
- `get/has/set/remove` are linear scans in `src/runtime/dict.rs`.
- Key comparison uses structural equality (`rt.core.eq`).
- Type checker restricts keys to `Int | String`.
- Key/value storage is erased through `anyref` slots, so concrete values cross boxing
  boundaries on lookup/update paths.

## Target State

Adopt a persistent hash array mapped trie (HAMT):

- `get/has/set/remove`: near O(1) average, O(log32 N) worst structural depth
- Structural sharing for persistent updates
- Stable semantics for iteration/key listing (`Dict.keys`)
- For concrete monomorphized instantiations, prefer typed dict/node layouts over
  universal `anyref` key/value fields:
  - `Dict<Int, Int>` stores direct `i64` keys and values
  - `Dict<String, Int>` stores `ref $String` keys and `i64` values
  - `Dict<String, Point>` stores `ref $String` keys and `ref $Point` values
- Keep erased fallback only where key/value layouts are genuinely unsupported or not worth
  specializing.

## Non-Goals

- Expanding key types beyond `Int | String`
- Exposing hash behavior at language surface
- Requiring host-specific hash primitives
- Forcing all key/value combinations to specialize in one step; rollout can prioritize the
  highest-value concrete pairs first.

## Data Model (Proposed)

- `Dict<K,V>` stores:
  - `size: i32`
  - `root: Node<K,V>?`
- Node families should be specialized per concrete `(K, V)` layout, not shared through
  `anyref` fields.
- Conceptually:
  - `Dict<String, Int>`:
    - `Dict_str_i64 { size: i32, root: Node_str_i64? }`
    - leaf entries store `(key: ref $String, value: i64, hash: i32|i64)`
  - `Dict<Int, String>`:
    - `Dict_i64_str { ... }`
    - leaf entries store `(key: i64, value: ref $String, hash: i32|i64)`
- Node variants:
  - Bitmap indexed node
  - Collision node (same hash, different keys)
  - Leaf entry with typed `key` / `value` / cached `hash`
- The exact node factoring can evolve, but the core policy is:
  - dict and node types are specialized per `(K, V)` instantiation
  - key/value fields use concrete Wasm types, not `anyref`

## Hashing Strategy

- `Int`: stable 64-bit mix -> 32-bit hash
- `String`: runtime UTF-8 hash (deterministic, host-independent)
- Collision handling via collision nodes and full key equality check

## Implementation Tasks

### Task A: Runtime Types for HAMT Nodes

- Update `src/runtime/types.rs`:
  - Add typed dict/node structs or generated families needed for HAMT.
  - Keep external `Dict` ref helper stable only if it can name a concrete instantiated
    dict family; otherwise migrate with coordinated codegen changes.
- Add symbol/key derivation for per-instantiation dict families.

### Task B: Reimplement `rt.dict`

- Rewrite `src/runtime/dict.rs`:
  - `make`: empty root, size 0
  - `get/has`: trie walk by hash fragments
  - `set`: path-copy insert/replace, size delta tracking
  - `remove`: path-copy delete + node compaction
  - `len`: O(1) from stored size
  - `keys`: deterministic traversal order (define and test)
  - Split helper families by concrete `(K, V)` layout where needed, or generate internal
    dispatch from a type key.
  - Avoid boxing/unboxing on specialized key/value paths.

### Task C: Hash + Equality Integration

- Add internal hash helpers in runtime (likely `rt.core` or `rt.dict` local helpers).
- Keep final key match guarded by existing equality semantics (`rt.core.eq`).
- Ensure string hashing matches UTF-8 byte representation used in runtime strings.
- For specialized families, use typed key comparisons on hot paths where possible, falling
  back to structural equality only where semantics require it.

### Task D: In-Place Rewrite Compatibility

- Keep optimizer contract in `src/opt/uniqueness.rs`:
  - `DICT_SET` -> `DICT_SET_IN_PLACE`
  - `DICT_REMOVE` -> `DICT_REMOVE_IN_PLACE`
- Redefine in-place helpers in HAMT runtime as safe destructive path mutation only when uniqueness guarantees hold.
- Verify in-place helpers target the correct specialized dict family.

### Task E: Prelude and Snapshot Alignment

- Keep existing prelude IDs and runtime symbols (`rt_dict__*`) to minimize frontend impact.
- Update snapshots and runtime dump expectations after type/function body changes.

### Task F: Specialization Policy

- Define which `(K, V)` pairs receive dedicated dict layouts first.
- Minimum target:
  - `Dict<Int, Int>`
  - `Dict<Int, String>`
  - `Dict<String, Int>`
  - `Dict<String, String>`
  - common ref-valued cases (`Dict<String, Record>`, `Dict<String, Sum>`)
- Define fallback:
  - erased/universal dict family only for unsupported/non-concrete cases
  - fallback should be the exception, not the default

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
- Add representation-focused tests:
  - `Dict<String, Int>` lookup/update paths contain no key/value boxing
  - `Dict<Int, String>` lookup/update paths contain no key/value boxing
  - erased fallback dict path still works for intentionally unsupported cases

## Staging

1. Introduce hash helpers and typed node/container families.
2. Add planner/codegen plumbing for concrete `(K, V)` dict refs.
3. Implement `get/has/len` for the first specialized key/value pairs.
4. Implement `set/remove` persistent path-copy for those pairs.
5. Implement in-place helper variants for uniqueness rewrite path.
6. Expand to additional high-value `(K, V)` families.
7. Retain erased fallback only for unsupported/non-concrete cases.
8. Finalize `keys` traversal and ordering guarantees.
9. Update snapshots and perf baseline.

## Risks

- Collision node correctness bugs can be hard to detect without targeted tests.
- Deterministic iteration order must be explicitly specified and preserved.
- In-place path mutation must remain strictly guarded by uniqueness proofs.
- Per-instantiation dict families increase runtime/codegen complexity and may require many
  helper variants.
- Specializing too many `(K, V)` pairs without profiling may increase code size more than it
  helps; staging should prioritize hot combinations.
