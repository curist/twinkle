# Pragmatic Persistent Dict (HAMT)

## Goal

Replace the linear assoc-list dictionary with a persistent hash array mapped
trie (HAMT), using the existing `anyref` key/value storage and `rt.dict` ABI
surface. Get real near-O(1) lookups and persistent structural sharing working
now; defer typed per-key/value specialization to a future enhancement pass.

## Relationship to Other Plans

This plan **supersedes the implementation strategy** of:

- `persistent-dict.md`

That plan remains valid as a future enhancement target (typed key/value
families, `anyref` elimination), but it is no longer a prerequisite for
landing persistent dicts.

This plan is **compatible with** but does not depend on:

- `backend-anyref-elimination.md` — typed families can replace `anyref`
  storage later without changing the HAMT algorithm
- `deferred-persistence.md` — uniqueness optimization composes unchanged
- `pragmatic-persistent-vector.md` — independent but same philosophy

## Current State

- `Dict<K,V>` is backed by a flat Wasm GC array of `DictEntry` structs
  (`rt_types__Dict = array (mut (ref null $DictEntry))`).
- Every `get`, `has`, `set`, `remove` does a linear scan — O(N).
- `set` and `remove` allocate and copy the full array — O(N).
- Key comparison uses `rt.core.eq` (structural equality).
- Key types are restricted to `Int | String | Byte`.
- Iteration order is insertion order (natural property of the assoc-list).
- The uniqueness optimizer rewrites `DICT_SET` → `DICT_SET_IN_PLACE` and
  `DICT_REMOVE` → `DICT_REMOVE_IN_PLACE`.

## Target State

- `Dict<K,V>` is backed by a persistent HAMT with branching factor 32.
- `get`/`has`/`set`/`remove`: near O(1) average, O(log32 N) worst case.
- Structural sharing: updates copy only the path from root to the modified
  node; all other subtrees are shared across versions.
- Iteration order is HAMT traversal order rather than insertion order.
- Key/value storage remains `anyref` — no type-family changes.
- The `rt.dict` function set remains stable: same exported names and responsibilities, but dict-valued params/results switch from `ref $Dict` to `ref $PDict`.
- The uniqueness optimizer's in-place rewrites continue to work.

## Non-Goals

- Per-key/value-type specialization
- Expanding key types beyond `Int | String | Byte`
- Exposing hash behavior at language surface
- Changing user-visible `Dict` syntax or method names
- Preserving insertion-order iteration in v1
- Moving dict logic to `boot/lib` Twinkle source

## Hashing Strategy

Hash function must be deterministic and host-independent.

- **`Int` (i64)**: bit-mixing function → 32-bit hash. E.g. multiply by a
  large odd constant, xor-shift, truncate to i32.
- **`Byte` (i32, 0–255)**: same mix as Int after zero-extension.
- **`String`**: FNV-1a or similar over UTF-8 bytes → 32-bit hash.

The hash is computed on every lookup/update. Caching the hash inside entries
is a possible optimization but not required for v1.

The HAMT uses 5-bit slices of the hash at each trie level (32-way branching).
With a 32-bit hash, that gives 6 levels before exhaustion (plus collision
nodes).

## Type Layout

New Wasm GC types added to `rt.types`:

```wat
;; Bitmap-indexed HAMT node
;; bitmap: which of the 32 slots are occupied
;; entries: packed array of occupied children (nodes or leaf entries)
(type $HamtNode (struct
  (field $bitmap i32)
  (field $entries (ref $Array))))     ;; $Array = (array (mut anyref))

;; Single key-value entry with cached hash
(type $HamtEntry (struct
  (field $hash i32)
  (field $key anyref)
  (field $val anyref)))

;; Collision node: multiple entries sharing the same hash
(type $HamtCollision (struct
  (field $hash i32)
  (field $entries (ref $Array))))     ;; array of $HamtEntry refs

;; Persistent dictionary root
(type $PDict (struct
  (field $size i32)
  (field $root (ref null $HamtNode))))
```

The existing `$Array` type (`array (mut anyref)`) is reused for node entry
storage, keeping all existing boxing/unboxing paths working.

**Node entries**: the `entries` array in `$HamtNode` contains a packed array
of either `$HamtEntry` refs (for leaves at this level), `$HamtNode` refs (for
sub-tries), or `$HamtCollision` refs. The type is distinguished at runtime
via `ref.test` / `ref.cast`. The array length equals `popcount(bitmap)`.

## Iteration Order

Iteration order in v1 is HAMT traversal order, not insertion order.

That means:

- `keys` traverses the trie and emits keys in structural/hash order
- updates do not maintain a separate ordering side table
- `set`/`remove` retain the HAMT's near-O(1) average update behavior rather
  than paying an extra O(N) order-maintenance cost

This is a deliberate tradeoff for the pragmatic landing. If ordered iteration
is still desirable later, it can be added with a separate persistent ordering
structure or a distinct ordered-dict design.

## Core Operations

### `make() -> PDict`

- Return `PDict { size: 0, root: null }`

### `has(dict, key) -> i32`

- Compute hash of key
- Walk HAMT from root using 5-bit hash slices
- At each `HamtNode`: check bitmap, index into entries
- At `HamtEntry`: compare key via `core_eq`
- At `HamtCollision`: linear scan entries for key match
- Return 1 if found, 0 if not

### `get(dict, key) -> anyref`

- Same walk as `has`; return entry's value or null

### `get_option(dict, key) -> Variant`

- Same walk as `has`; return `Some(val)` or `None` variant

### `set(dict, key, val) -> PDict`

- Compute hash; walk HAMT
- If key exists: path-copy to the entry, replace value
- If key absent: path-copy and insert new entry, increment size

### `remove(dict, key) -> PDict`

- Compute hash; walk HAMT
- If key absent: return dict unchanged
- If key found: path-copy and remove entry, decrement size
- Compact node if it becomes empty or single-entry after removal

### `len(dict) -> i32`

- Return `dict.size` — O(1)

### `keys(dict) -> Array`

- Traverse the HAMT and collect keys into a fresh `$Array`
- Order is HAMT traversal order, not insertion order

## In-Place Variants

### `set_in_place(dict, key, val) -> PDict`

Uniqueness rewrite target. When the dict is uniquely owned:

- For existing key: mutate the HAMT path and entry value destructively
- For new key: still needs allocation for the new entry and path extension

For the initial landing, `set_in_place` can be implemented as an alias for
the persistent `set`. Follow-up: real destructive path mutation.

### `remove_in_place(dict, key) -> PDict`

Same approach: alias for persistent `remove` initially, real destructive
removal later.

## Representation Boundary

`Dict<K,V>` at the codegen level changes from `ref $Dict` (array of entries)
to `ref $PDict` (HAMT struct).

This affects:

- `src/runtime/types.rs` — new type definitions
- `src/codegen/prelude.rs` — dict valtype mapping
- `src/codegen/emit.rs` — dict literal lowering, intrinsic emission
- `src/codegen/ctx.rs` — dict ref helpers
- `boot/compiler/builtins.tw` — dict ABI metadata and builtin signatures
- boot boundary/ABI tests that currently assert `rt_types__Dict`

The `rt.dict` export signatures change their dict parameter/return types from
`ref $Dict` to `ref $PDict`, but the **function names and count stay the
same**.

## Boot Compiler Parity

The boot compiler has a mirrored runtime in
`boot/compiler/codegen/runtime/dict.tw`. It must be updated in lockstep,
along with boot builtin ABI metadata:

- `boot/compiler/codegen/runtime/types.tw` — add HAMT type definitions
- `boot/compiler/codegen/runtime/dict.tw` — rewrite all operations

Both stage0 and boot must produce the same runtime representation.

## Implementation Phases

### Phase 1: Add HAMT Types and Hash Helpers

- Add `HamtNode`, `HamtEntry`, `HamtCollision`, `PDict` to
  `src/runtime/types.rs`
- Add hash helpers to `rt.dict` or a new `rt.hash` module (Int/String/Byte)
- Mirror in `boot/compiler/codegen/runtime/types.tw`

### Phase 2+3: Rewrite `rt.dict` + Update Codegen (Atomic)

Phases 2 and 3 must land as a single atomic change. Once `rt.dict` expects
`ref $PDict` parameters, codegen must produce `ref $PDict` values.

- Rewrite `make`, `len`, `has`, `get`, `get_option`, `set`, `remove`, `keys`
  in `src/runtime/dict.rs` to operate on `PDict` with HAMT
- Implement hash helpers for Int, String, Byte keys
- Implement `set_in_place` and `remove_in_place` as persistent-op aliases
  initially
- Change dict valtype from `ref $Dict` to `ref $PDict` in codegen
- Update dict literal emission (empty dict)
- Update intrinsic emission (boundaries, casts)
- Update prelude dict ref helpers
- Update `boot/compiler/builtins.tw` so dict/runtime builtin ABI metadata
  points at `ref $PDict` rather than `ref $Dict`
- Add empty dict singleton as a Wasm global

### Phase 4: Boot Compiler Parity

This is the highest-effort phase. HAMT node manipulation (bitmap indexing,
popcount, packed array insert/remove, collision handling) is non-trivial to
express in Twinkle's Wasm IR builder.

Sub-phases:

1. **Types**: add HAMT type definitions to
   `boot/compiler/codegen/runtime/types.tw`
2. **Hash helpers**: Int/String/Byte hashing
3. **Core read ops**: `has`, `get`, `get_option`, `len` — HAMT walk, no
   mutation
4. **Core write ops**: `set`, `remove` — path-copy with bitmap manipulation
5. **Collision handling**: collision node creation and resolution
6. **Traversal-based `keys`**: HAMT walk that materializes keys in traversal
   order
7. **Remaining ops**: `set_in_place`, `remove_in_place`
8. **Builtin ABI parity**: update boot builtin ABI metadata to use `PDict`
9. **Verify**: boot-compiled programs produce correct output

### Phase 5: Validation

- All existing dict tests pass (`tests/run/dicts.tw`, `tests/run/dict_methods.tw`)
- All optimizer tests pass (`tests/opt/*dict*`)
- Boot compiler tests pass (`cargo run --release -- run boot/tests/main.tw`)
- New tests for:
  - Hash collision scenarios (craft keys with same hash)
  - Large dicts (1000+ entries): get/has/set/remove
  - Structural sharing: modify derived dict, verify original unchanged
  - Traversal-order behavior stays deterministic for a fixed hash/trie shape
  - `Dict<Byte, V>` coverage
  - In-place rewrite correctness (uniqueness optimizer)

## Risks

- **Hash collision handling**: collision nodes add complexity. Incorrect
  collision resolution can cause silent data loss. Needs targeted tests.
- **Iteration-order change**: dropping insertion-order iteration may change
  observable output for code that implicitly relied on assoc-list ordering.
- **Codegen dict ref type change**: the switch from `ref $Dict` to
  `ref $PDict` touches codegen broadly, similar to the vector change,
  including boot builtin ABI tables and boundary tests.
- **Boot parity**: the mirrored Wasm IR implementation is substantial.
- **In-place regression**: the optimizer's in-place variants become
  persistent operations initially, losing the mutation fast path.
- **Snapshot churn**: runtime type layout changes will update many test
  snapshots.
- **Node discrimination overhead**: using `ref.test`/`ref.cast` to
  distinguish entry vs node vs collision at runtime has some cost. In
  practice this is small compared to the O(N) → O(1) improvement.
- **Phase atomicity**: runtime + codegen changes must land together.
  Between the two, the compiler would be broken.

## Future Enhancements (Out of Scope)

These remain valid future work, enabled by having a working HAMT:

1. **Real in-place mutation** for uniquely owned paths
2. **Per-key/value-type specialization** (`Dict<String, Int>` with typed slots)
3. **Ordered iteration** via a separate persistent ordering structure or a
   dedicated ordered-dict design
4. **Cached hash in entries** to avoid recomputation on resize/collision
5. **Twinkle-authored library ownership** in `boot/lib`
6. **`anyref` elimination** from key/value storage
