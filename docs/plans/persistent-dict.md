# Persistent Dict (HAMT) Plan

## Goal

Replace the current linear assoc-list dictionary implementation with a persistent HAMT, preserving Twinkle `Dict<K,V>` behavior.

For concrete monomorphized programs, `Dict<K,V>` should also move away from universal
`anyref` key/value storage: the long-term target is a per-instantiation container family,
so `Dict<String, Int>`, `Dict<Int, String>`, and other concrete pairs can use typed node
layouts and avoid boxing every key/value through `anyref`.

This plan is subordinate to
[`backend-anyref-elimination.md`](backend-anyref-elimination.md). If the two
documents disagree, the backend `anyref` elimination plan wins. Transitional
erased fallbacks are allowed during migration, but they are not part of the
intended end state for supported concrete dict code paths.

## Current State

- Runtime dict is an unsorted association list over array entries.
- `get/has/set/remove` are linear scans in `src/runtime/dict.rs`.
- Key comparison uses structural equality (`rt.core.eq`).
- The current compiler/runtime contract is larger than the user-visible `Dict` method set:
  - user-facing `Dict.get` is the safe `Option`-returning operation
  - indexed assignment `m[k] = v` lowers directly to `dict_set`
  - `for`/`collect` over dicts depend on `Dict.keys` and therefore observe its order
  - the optimizer rewrites `dict_set` / `dict_remove` to `dict_set_in_place` /
    `dict_remove_in_place` when uniqueness permits
- Current implementations restrict dict keys to a small builtin family
  (`Int | String | Byte`); the persistent
  runtime should preserve those key semantics rather than broaden them implicitly.
- Runtime implementation lives in `boot/compiler/codegen/runtime/dict.tw` and mirrors
  stage0 `src/runtime/dict.rs`.
- Key/value storage is erased through `anyref` slots, so concrete values cross boxing
  boundaries on lookup/update paths.

## Target State

Adopt a persistent hash array mapped trie (HAMT):

- `get/has/set/remove`: near O(1) average, O(log32 N) worst structural depth
- Structural sharing for persistent updates
- Stable semantics for iteration/key listing (`Dict.keys`)
- Preserve the current split between:
  - safe `Option`-returning lookup exposed to user code
  - raw helpers used by lowering or optimizer fast paths
- For concrete monomorphized instantiations, prefer typed dict/node layouts over
  universal `anyref` key/value fields:
  - `Dict<Int, Int>` stores direct `i64` keys and values
  - `Dict<String, Int>` stores `ref $String` keys and `i64` values
  - `Dict<String, Point>` stores `ref $String` keys and `ref $Point` values
- For supported concrete `(K, V)` pairs, the steady-state backend target is fully typed
  dict/container/helper families with no backend-internal `anyref` payload storage.
- Transitional erased fallback may exist only during migration for unsupported or
  not-yet-specialized pairs; per `backend-anyref-elimination.md`, it is not the
  intended long-term model for ordinary concrete code.

## Non-Goals

- Expanding key types beyond the current builtin family (`Int | String | Byte`)
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
- `Byte`: stable byte-to-int mix with the same deterministic hash contract as other
  builtin keys
- `String`: runtime UTF-8 hash (deterministic, host-independent)
- Collision handling via collision nodes and full key equality check

## Iteration Ordering Contract

The HAMT implementation must preserve today's observable dict ordering semantics,
not merely produce a deterministic order:

- `Dict.keys`, `for k in d`, `for k, v in d`, `collect` over dicts, and helpers such
  as `Dict.values` all observe dict order.
- First insertion of a new key appends that key at the end of the observable order.
- Updating an existing key preserves that key's position.
- Removing a key preserves the relative order of the remaining keys.
- Removing and later reinserting the same key treats the reinsertion as a new
  insertion at the end.

If the HAMT's internal node traversal does not naturally preserve this contract,
the runtime must maintain separate ordering metadata or an equivalent mechanism.

## Implementation Tasks

### Task A: Runtime Types for HAMT Nodes

- Update `src/runtime/types.rs`:
  - Add typed dict/node structs or generated families needed for HAMT.
  - Keep external `Dict` ref helper stable only if it can name a concrete instantiated
    dict family; otherwise migrate with coordinated codegen changes.
- Add symbol/key derivation for per-instantiation dict families.

### Task B: Reimplement `rt.dict`

- Rewrite `boot/compiler/codegen/runtime/dict.tw` and stage0 `src/runtime/dict.rs`:
  - `make`: empty root, size 0
  - `get/has`: trie walk by hash fragments
  - `set`: path-copy insert/replace, size delta tracking
  - `remove`: path-copy delete + node compaction
  - `len`: O(1) from stored size
  - `keys`: preserve the insertion-order contract above, not just deterministic output
  - Preserve the current boot compiler runtime surface:
    - semantic helpers: `make`, `len`, `keys`, `get_option`, `has`, `set`, `remove`
    - optimizer helpers: `set_in_place`, `remove_in_place`
    - optional raw lookup helper kept for parity/internal lowering: `get`
  - Split helper families by concrete `(K, V)` layout where needed, or generate internal
    dispatch from a type key.
  - Avoid boxing/unboxing on specialized key/value paths.

### Task C: Hash + Equality Integration

- Add internal hash helpers in runtime (likely `rt.core` or `rt.dict` local helpers).
- Keep final key match guarded by existing equality semantics (`rt.core.eq`).
- Ensure string hashing matches UTF-8 byte representation used in runtime strings.
- Keep `Byte` key behavior first-class alongside `Int` and `String`; do not regress the
  existing `Dict<Byte, V>` surface while changing representation.
- `Dict.keys` order is part of today's observable behavior because dict iteration lowers
  through `keys`; preserve the insertion-order contract above.
- For specialized families, use typed key comparisons on hot paths where possible, falling
  back to structural equality only where semantics require it.

### Task D: In-Place Rewrite Compatibility

- Keep optimizer contract in `src/opt/uniqueness.rs`:
  - `DICT_SET` -> `DICT_SET_IN_PLACE`
  - `DICT_REMOVE` -> `DICT_REMOVE_IN_PLACE`
- Redefine in-place helpers in HAMT runtime as safe destructive path mutation only when uniqueness guarantees hold.
- Verify in-place helpers target the correct specialized dict family.
- Treat those helpers as optimizer-only ABI, not user-visible surface API.

### Task E: Prelude and Snapshot Alignment

- Keep existing prelude IDs and runtime symbols (`rt_dict__*`) to minimize frontend impact.
- Update snapshots and runtime dump expectations after type/function body changes.

### Task F: Specialization Policy

- Define which `(K, V)` pairs receive dedicated dict layouts first.
- Minimum target:
  - `Dict<Int, Int>`
  - `Dict<Byte, Int>`
  - `Dict<Int, String>`
  - `Dict<String, Int>`
  - `Dict<String, String>`
  - `Dict<Byte, String>`
  - common ref-valued cases (`Dict<String, Record>`, `Dict<String, Sum>`)
- Define fallback:
  - temporary erased/universal dict family only for unsupported/non-concrete cases
    during migration
  - fallback should be the exception, not the default
  - fallback must be removed for supported concrete families once their typed layouts
    land; it is not part of the target architecture

## Validation

- Existing dict behavior tests pass:
  - `tests/run/dicts.tw`
  - `tests/run/dict_methods.tw`
  - `tests/opt/*dict*`
- Keep coverage for compiler hooks:
  - dict iteration order preserves insertion/update/remove semantics via `Dict.keys`
  - `m[k] = v` still maps to the persistent update path
  - uniqueness rewrites to in-place helpers preserve semantics
- Add HAMT-specific tests:
  - Hash collision scenarios
  - Deep trie path updates/removes
  - Structural sharing sanity (older versions remain intact)
  - `Dict<Byte, _>` coverage for get/has/set/remove/iteration
  - `Dict.keys` ordering checks for insert, overwrite, remove, and remove+reinsert
- Add representation-focused tests:
  - `Dict<String, Int>` lookup/update paths contain no key/value boxing
  - `Dict<Int, String>` lookup/update paths contain no key/value boxing
  - `Dict<Byte, Int>` lookup/update path contains no key boxing once that family is
    supported
  - temporary erased fallback dict path, if still present during migration, works only
    for intentionally unsupported cases

## Staging

1. Introduce hash helpers and typed node/container families.
2. Add planner/codegen plumbing for concrete `(K, V)` dict refs.
3. Implement `get/has/len` for the first specialized key/value pairs.
4. Implement `set/remove` persistent path-copy for those pairs.
5. Implement in-place helper variants for uniqueness rewrite path.
6. Expand to additional high-value `(K, V)` families, including `Byte`-key cases.
7. Restrict transitional erased fallback to unsupported/non-concrete cases only.
8. Remove transitional fallback for supported concrete families and finalize `keys`
   ordering guarantees.
9. Update snapshots and perf baseline.

## Risks

- Collision node correctness bugs can be hard to detect without targeted tests.
- Deterministic iteration order must be explicitly specified and preserved.
- In-place path mutation must remain strictly guarded by uniqueness proofs.
- Per-instantiation dict families increase runtime/codegen complexity and may require many
  helper variants.
- Specializing too many `(K, V)` pairs without profiling may increase code size more than it
  helps; staging should prioritize hot combinations.
