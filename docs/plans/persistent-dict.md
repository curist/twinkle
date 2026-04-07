# Persistent Dict (HAMT) Plan

## Goal

Replace the current linear assoc-list dictionary implementation with a persistent
hash array mapped trie (HAMT), preserving the existing Twinkle `Dict<K,V>`
surface and the current `rt.dict` function set.

This plan intentionally follows the same pragmatic strategy used for persistent
vectors:

- land the representation/algorithmic win first
- keep the current erased `anyref` key/value storage for the first version
- preserve optimizer and lowering contracts
- defer typed per-`(K, V)` specialization to follow-up work

The long-term target still aligns with
[`backend-anyref-elimination.md`](backend-anyref-elimination.md): supported
concrete dict instantiations should eventually move away from backend-internal
`anyref` payload storage. But that is a follow-up, not a prerequisite for the
first HAMT landing.

## Current State

- Runtime dict is an unsorted association list over array entries.
- Stage0 implementation lives in `src/runtime/dict.rs`.
- Boot mirror lives in `boot/compiler/codegen/runtime/dict.tw`.
- Shared runtime types live in:
  - `src/runtime/types.rs`
  - `boot/compiler/codegen/runtime/types.tw`
- Prelude / builtin ABI wiring currently treats dict values as `ref $Dict`:
  - `src/codegen/prelude.rs`
  - `boot/compiler/builtins.tw`
- Current representation:
  - `DictEntry = struct { key: anyref, val: anyref }`
  - `Dict = array (mut (ref null DictEntry))`
- `get/has/set/remove` are linear scans.
- `set` and `remove` are copy-on-write over the whole backing array.
- Key comparison uses structural equality via `rt.core.eq`.
- Key types are restricted to `Int | String | Byte`.
- The optimizer rewrites `DICT_SET` / `DICT_REMOVE` to in-place variants when
  uniqueness permits, including loop rewrites already covered by current tests.
- Dict iteration currently lowers through `Dict.keys` and therefore observes the
  order produced by `keys`.

## Key Findings From Current Implementation

Before changing the plan, inspect the current runtime/codegen surface:

### Runtime representation and ABI

- `src/runtime/types.rs` defines:
  - `T_DICT_ENTRY = rt_types__DictEntry`
  - `T_DICT = rt_types__Dict`
- `src/runtime/dict.rs` exports exactly these helpers:
  - `make`
  - `len`
  - `keys`
  - `has`
  - `get`
  - `get_option`
  - `set`
  - `remove`
  - `set_in_place`
  - `remove_in_place`
- Boot mirrors the same surface in `boot/compiler/codegen/runtime/dict.tw`.

### Codegen/builtin assumptions

- `src/codegen/prelude.rs` maps dict runtime calls to `ref_dict()` /
  `ref_dict_null()` and `anyref` key/value parameters.
- `boot/compiler/builtins.tw` encodes the same ABI, still using
  `rt_types__Dict` and `anyref` payloads.
- Dict refs are also hardcoded in direct lowering/codegen emission paths:
  - `src/codegen/emit.rs` emits `rt_dict__get` / `rt_dict__get_option`
  - `src/codegen/emit.rs` emits `dict_get_unsafe` via `rt_dict__get`
  - `boot/compiler/codegen/emit.tw` directly emits `rt_dict__get` /
    `rt_dict__get_option`
- Several tests and ABI guardrails currently assert that dict values are backed
  by `rt_types__Dict`.

### Order-sensitive tests already exist

Even if user code does not intentionally rely on dict iteration order, the test
suite does today:

- `tests/run/dicts.tw` expects insertion-order output for `for k, v in dict`
  and `for k in dict`
- runtime shape tests in `boot/tests/suites/runtime_suite.tw` assert the
  current array/entry implementation details
- layout/ABI tests in boot suites assert `rt_types__Dict` by name

So a HAMT migration is not just an internal optimization. It changes visible
representation and, unless we choose otherwise, potentially visible iteration
behavior.

## Target State (Phase 1)

Adopt a persistent HAMT with branching factor 32:

- `get/has/set/remove`: near O(1) average, O(log32 N) worst structural depth
- structural sharing for persistent updates
- key/value storage remains `anyref` in v1
- `rt.dict` keeps the same exported function names and responsibilities
- optimizer-only in-place helpers remain part of the ABI surface
- dict runtime values move from flat `ref $Dict` arrays to a root struct
  representing the HAMT

## Non-Goals (Phase 1)

- per-`(K, V)` typed dict/node families
- expanding key types beyond `Int | String | Byte`
- exposing hash behavior at the language surface
- moving dict ownership to `boot/lib`
- eliminating all backend-internal `anyref` usage in the first landing

## Iteration Ordering Contract

The HAMT migration must preserve today's observable dict ordering semantics.

That means the following continue to observe insertion order:

- `Dict.keys`
- `for k in d`
- `for k, v in d`
- `collect` over dicts

Required behavior:

- first insertion of a new key appends that key at the end of iteration order
- updating an existing key preserves that key's position
- removing a key preserves the relative order of remaining keys
- removing and later reinserting the same key treats the reinsertion as a new
  insertion at the end

This is a deliberate semantic constraint, not an implementation suggestion.
The HAMT's internal traversal order is not user-visible unless it happens to
match the insertion-order contract above.

## Ordering Metadata Strategy

Because plain HAMT traversal does not preserve insertion order, the first HAMT
landing must carry explicit ordering metadata.

Recommended shape for v1:

- `PDict` stores:
  - `size`
  - `root`
  - `order`
- `order` is a persistent sequence of keys representing observable iteration
  order
- HAMT remains the lookup/update index
- `order` remains the iteration source of truth

Important dependency note:

- `PVec` is only an obvious fit on stage0 because the persistent vector runtime
  types already exist there
- boot does not currently have matching persistent-vector runtime type coverage
  in `boot/compiler/codegen/runtime/types.tw`
- therefore `PVec`-backed ordering is not a free reuse on boot; it requires an
  explicit dependency decision

Acceptable ways to satisfy that dependency:

1. add/mirror the persistent vector runtime types and any needed ABI support in
   boot first, then use `PVec` for dict order in both runtimes
2. choose a different ordering representation that already exists in both
   runtimes for the first HAMT landing

Until that decision is made, `PVec` should be treated as the default
recommendation, not as already-available shared infrastructure.

Operationally:

- on insert of a new key:
  - insert into HAMT
  - append key to `order`
- on update of existing key:
  - update HAMT value only
  - leave `order` unchanged
- on remove:
  - remove from HAMT
  - remove key from `order`
- on remove+reinsert:
  - key is appended again as a fresh insertion

This makes the first HAMT landing somewhat larger than a pure traversal-order
HAMT, but it keeps language semantics stable and makes the migration easier to
reason about.

## Ordering Structure Tradeoff

Preserving order means one operation remains less than ideal in the first
landing:

- HAMT keeps lookup/update path-copy near O(1) average
- order maintenance may still impose O(N) work on removal, depending on the
  chosen persistent sequence representation

That tradeoff is acceptable for the first landing because:

- it preserves current language behavior
- it still removes the current O(N) lookup/update bottleneck
- it keeps future optimization options open

If later profiling shows ordered removal is a major cost center, we can optimize
that separately without changing user-visible semantics.

## Data Model (Phase 1)

Introduce HAMT-specific runtime types while keeping erased payloads:

```wat
(type $HamtNode (struct
  (field $bitmap i32)
  (field $entries (ref $Array))))

(type $HamtEntry (struct
  (field $hash i32)
  (field $key anyref)
  (field $val anyref)))

(type $HamtCollision (struct
  (field $hash i32)
  (field $entries (ref $Array))))

(type $PDict (struct
  (field $size i32)
  (field $root (ref null $HamtNode))
  (field $order (ref $PVec))))
```

Notes:

- `$Array` remains the shared `(array (mut anyref))`
- `HamtNode.entries` stores packed child refs:
  - `HamtEntry`
  - `HamtNode`
  - `HamtCollision`
- runtime node discrimination uses `ref.test` / `ref.cast`
- `PDict.order` is the observable iteration source
- dict-valued runtime params/results change from `ref $Dict` to `ref $PDict`

## Hashing Strategy

Hashing must be deterministic and host-independent.

- `Int`: stable bit-mix to 32-bit hash
- `Byte`: same stable mix after zero-extension
- `String`: deterministic UTF-8 byte hash

Collision handling uses collision nodes plus final key equality via `rt.core.eq`.

## Runtime Surface to Preserve

The following exported names stay stable:

- `rt_dict__make`
- `rt_dict__len`
- `rt_dict__keys`
- `rt_dict__has`
- `rt_dict__get`
- `rt_dict__get_option`
- `rt_dict__set`
- `rt_dict__remove`
- `rt_dict__set_in_place`
- `rt_dict__remove_in_place`

Semantics:

- `get_option` remains the safe user-facing lookup path
- raw `get` remains available for parity/internal lowering
- `set_in_place` / `remove_in_place` remain optimizer-only ABI hooks

For the first landing:

- `set_in_place` may alias persistent `set`
- `remove_in_place` may alias persistent `remove`

Real destructive path mutation is a follow-up optimization.

## Representation Boundary Changes

The dict representation boundary changes atomically:

- stage0 runtime types and `rt.dict`
- boot runtime types and `rt.dict`
- stage0 prelude/runtime ABI wiring
- boot builtin ABI wiring
- tests that assert old `rt_types__Dict`/`DictEntry` details

Current assumptions that must change together:

- `src/runtime/types.rs`
- `src/runtime/dict.rs`
- `src/codegen/prelude.rs`
- `src/codegen/emit.rs`
- `boot/compiler/codegen/runtime/types.tw`
- `boot/compiler/codegen/runtime/dict.tw`
- `boot/compiler/codegen/emit.tw`
- `boot/compiler/builtins.tw`
- runtime/layout/ABI tests in `boot/tests/suites/*`

In particular, the atomic rollout must include all direct dict import/call sites
that still hardcode `ref $Dict` assumptions, including:

- `rt_dict__get`
- `rt_dict__get_option`
- the internal `dict_get_unsafe` lowering path that routes through `rt_dict__get`

## Implementation Tasks

### Task A: Add HAMT Runtime Types

Update stage0 and boot runtime type definitions to add:

- `HamtNode`
- `HamtEntry`
- `HamtCollision`
- `PDict`

Keep old flat dict types only as long as needed for atomic migration. The final
HAMT landing should remove ordinary runtime dependence on the old assoc-list
representation.

### Task B: Reimplement `rt.dict`

Rewrite stage0 and boot dict runtimes to use HAMT operations:

- `make`: empty `PDict`
- `len`: O(1) from stored size
- `has`: trie walk by hash fragments
- `get`: trie walk returning value-or-null
- `get_option`: Option variant wrapper over lookup
- `set`: persistent path-copy insert/replace
- `remove`: persistent path-copy delete + compaction
- `keys`: materialize keys from `order`, not HAMT traversal
- `set_in_place` / `remove_in_place`: preserve the same ordering semantics as
  their persistent counterparts; initially aliasing the persistent versions is
  acceptable if needed for landing speed

### Task C: Ordered Iteration Support

Add ordering metadata support in both stage0 and boot runtimes.

Minimum requirements:

- `PDict` stores an `order` sequence
- `make` initializes empty order metadata
- `set` appends only when inserting a previously absent key
- `remove` removes the key from order while preserving relative order
- `keys` reads from order rather than walking the HAMT directly
- `for` / `collect` over dicts therefore continue to see stable insertion order

Dependency decision required before implementation:

- if `order` is backed by `PVec`, boot must first gain the necessary persistent
  vector runtime type support
- otherwise the first HAMT landing must choose an ordering representation that
  is already available in both runtimes

Implementation note:

- `PVec` is the default recommendation for stage0
- for boot, treat `PVec` as a dependency to be introduced explicitly, not as
  already-present shared runtime machinery
- remove-from-order may remain O(N) in the first landing; that is acceptable
  for v1 if lookup/update still benefit from HAMT

### Task D: Hash + Equality Integration

Add deterministic runtime hash helpers for:

- `Int`
- `String`
- `Byte`

Preserve final key comparison through existing equality semantics.

### Task E: Prelude / Builtin ABI Update

Change dict runtime valtypes from `ref $Dict` to `ref $PDict` in:

- `src/codegen/prelude.rs`
- `src/codegen/emit.rs`
- `boot/compiler/codegen/emit.tw`
- `boot/compiler/builtins.tw`

This includes the direct lowering/codegen call sites for:

- `rt_dict__get`
- `rt_dict__get_option`
- `dict_get_unsafe`

Keep exported function names and arities stable where possible.

### Task F: Test and Snapshot Realignment

Update tests that currently bake in assoc-list details:

- runtime shape tests that assert `DictEntry`/`Dict` array behavior
- boot ABI/layout tests that assert `rt_types__Dict`

Keep and expand order-semantics tests rather than weakening them.

Required coverage:

- insertion appends to iteration order
- updating an existing key preserves position
- removal preserves the relative order of remaining keys
- remove+reinsert appends at the end
- `for k in d` and `for k, v in d` agree with `Dict.keys`

### Task G: Keep Optimizer Compatibility

Preserve the optimizer contract for:

- `DICT_SET -> DICT_SET_IN_PLACE`
- `DICT_REMOVE -> DICT_REMOVE_IN_PLACE`

The loop-rewrite work already covered by the dict in-place optimization plan
must continue to apply after the HAMT migration.

## Validation

- Existing dict API behavior still passes with insertion-order semantics preserved:
  - `tests/run/dict_methods.tw`
  - `tests/run/dicts.tw`
  - `tests/opt/*dict*`
- Boot compiler tests pass:
  - `cargo run --release -- run boot/tests/main.tw`
- Add HAMT-specific tests for:
  - hash collisions
  - deep trie updates/removes
  - structural sharing
  - large dicts
  - `Dict<Byte, _>` behavior
  - preserved insertion-order semantics for a fixed update/remove history
- Add ABI/layout tests for the new `PDict`/HAMT types.

## Staging

1. Add HAMT type definitions in stage0 and boot runtimes, including `PDict.order`.
2. Decide the first cross-runtime ordering representation:
   - either introduce the missing boot-side support needed for `PVec` order
   - or choose an ordering container already available in both runtimes.
3. Add deterministic hash helpers.
4. Rewrite stage0 `rt.dict` to `PDict` + HAMT + ordered iteration metadata.
5. Rewrite boot `rt.dict` to match.
6. Update all dict ABI/import sites from `ref $Dict` to `ref $PDict`, including prelude and direct emitter call sites.
7. Update runtime/layout/ABI tests and snapshots.
8. Add explicit order-contract tests for insert/update/remove/reinsert behavior.
9. Re-run optimizer tests to ensure in-place rewrites still target dict ops.

## Risks

- Collision-node correctness bugs can silently lose entries.
- Boot parity is substantial because HAMT manipulation is much more complex than
  the current flat array implementation.
- Changing dict runtime type names/layout touches many ABI guardrails and
  snapshots.
- Preserving insertion order makes the first HAMT landing more complex,
  especially around ordered removal.
- In-place variants may temporarily lose their destructive-path advantage if
  implemented as aliases first.

## Future Enhancements

Once the base HAMT lands and stabilizes:

1. real destructive path mutation for unique dicts
2. typed per-`(K, V)` dict/node families
3. backend-internal `anyref` elimination for common concrete dicts
4. cached hashes / further node-layout optimizations
5. possible more efficient ordered-removal metadata if profiling justifies it
6. possible `boot/lib` ownership later
