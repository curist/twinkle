# Pragmatic Persistent Vector

## Goal

Replace the flat copy-on-write vector backing with a persistent bit-partitioned
trie (+ tail), using the existing `anyref` element storage and `rt.arr` ABI
surface. Get real O(log32 N) structural sharing working now; defer typed
per-element specialization to a future enhancement pass.

## Relationship to Other Plans

This plan **supersedes the implementation strategy** of:

- `persistent-vector.md`
- `persistent-vector-i64-poc.md`
- `twinkle-vector-kickoff.md`
- `boot-lib-vector-consumption.md`
- `twinkle-runtime-import-boundary.md`

Those plans remain valid as future enhancement targets, but they are no longer
prerequisites for landing persistent vectors. The dependency chain they defined
(runtime import boundary → artifact consumption → typed family substrate →
algorithm) was blocking progress.

This plan is **compatible with** but does not depend on:

- `backend-anyref-elimination.md` — typed families can replace `anyref` storage
  later without changing the trie algorithm
- `deferred-persistence.md` — uniqueness optimization composes unchanged
- `static-uniqueness-plan.md` — future precision improvements still apply

## Current State

- `Vector<T>` is backed by a flat Wasm GC array (`rt_types__Array`).
- Every `set`, `push`, `concat`, and `slice` allocates and copies the full
  array — O(N) per operation.
- Element storage is erased through `anyref`.
- The builder system (`builder_new/from/push/freeze`) uses a doubling buffer
  with capacity tracking via `BoxedInt` in a 3-slot `Array`.
- The uniqueness optimizer rewrites `VECTOR_SET_UNSAFE` →
  `VECTOR_SET_IN_PLACE` which lowers to `array.set`.

## Target State

- `Vector<T>` is backed by a persistent bit-partitioned trie with branching
  factor 32 and an unshared tail buffer.
- `get`/`set`: O(log32 N) — at most 7 levels for 34 billion elements.
- `push`: O(1) amortized (tail append) / O(log32 N) when tail overflows.
- Structural sharing: updates copy only the path from root to the modified
  node; all other subtrees are shared across versions.
- Element storage remains `anyref` — no type-family changes.
- The `rt.arr` export surface remains identical.
- The builder system is simplified to wrap persistent `push`.
- The uniqueness optimizer's `VECTOR_SET_IN_PLACE` continues to work.

## Non-Goals

- Per-element-type specialization (`Vector<Int>` with `i64` slots, etc.)
- Moving vector logic to `boot/lib` Twinkle source
- Changing the runtime import boundary or adding new builtin categories
- Changing user-visible `Vector` syntax or method names
- RRB tree concatenation (v1 concat iterates and pushes)

## Type Layout

New Wasm GC types added to `rt.types`:

```wat
;; Abstract trie node base (non-final, no fields)
(type $VecNode (sub (struct)))

;; Leaf: wraps a fixed-size anyref array of elements
(type $VecLeaf (sub $VecNode (struct
  (field $data (ref $Array)))))       ;; $Array = (array (mut anyref))

;; Internal node: wraps a fixed-size array of child node refs
(type $VecChildren (array (mut (ref null $VecNode))))
(type $VecInternal (sub $VecNode (struct
  (field $children (ref $VecChildren)))))

;; Persistent vector root
(type $PVec (struct
  (field $len i32)
  (field $shift i32)
  (field $root (ref null $VecInternal))
  (field $tail (ref $VecLeaf))))
```

The existing `$Array` type (`array (mut anyref)`) is reused for leaf element
storage, keeping all existing boxing/unboxing paths working.

### Empty Vector Singleton

A shared empty vector is allocated once as a global:

- `EMPTY_LEAF`: a `$VecLeaf` wrapping a zero-length `$Array`
- `EMPTY_VEC`: a `$PVec` with `len=0, shift=0, root=null, tail=EMPTY_LEAF`

All operations that produce empty vectors (`make(0, _)`, slicing to zero
length, etc.) return this singleton to avoid repeated allocation.

### Key Invariants

- `0 <= tail.data.len <= 32`
- `len = trie_element_count + tail.data.len`
- `root = null` iff `len <= 32` (tail-only vector)
- `shift = 0` when `root = null`; otherwise `shift = 5 * tree_depth`
- Internal nodes always have 32-slot child arrays; unused slots are null.
  Only the rightmost spine may have null children.
- Trie leaves always contain exactly 32 elements
- `tailoff(len) = if len <= 32 { 0 } else { ((len - 1) >> 5) << 5 }`
- `tailoff(0) = 0` — the empty vector should never reach index-based
  operations; runtime should trap on out-of-bounds before `tailoff` matters

## Representation Boundary

`Vector<T>` at the codegen level changes from `ref $Array` to `ref $PVec`.

This affects:

- `src/runtime/types.rs` — new type definitions
- `src/codegen/prelude.rs` — vector valtype mapping
- `src/codegen/emit.rs` — vector literal lowering, intrinsic emission
- `src/codegen/ctx.rs` — vector ref helpers

The `rt.arr` export signatures change their vector parameter/return types from
`ref $Array` to `ref $PVec`, but the **function names and count stay the
same**.

## Core Operations

### `make(len, fill) -> PVec`

- `len == 0`: return shared empty vector singleton
- `len <= 32`: tail-only vector with `len` copies of `fill`
- `len > 32`: repeated `push` from empty (O(N log32 N))

Optimization note: since `fill` is uniform, `make` could build full 32-element
leaves directly and assemble the trie bottom-up in O(N). Deferred for v1 but
straightforward to add later.

### `get(vec, idx) -> anyref`

- If `idx >= tailoff(vec.len)`: read from `tail.data[idx - tailoff]`
- Otherwise: descend from `root` using 5-bit slices of `idx` at each level,
  cast nodes appropriately, read from leaf

### `set(vec, idx, val) -> PVec`

- If index is in the tail: copy tail data, update one slot, return new PVec
  with same root
- Otherwise: path-copy from root to target leaf, update one element in the
  copied leaf, reuse all unaffected siblings

### `len(vec) -> i32`

- Return `vec.len` field directly — O(1)

### `push` (internal, used by builder and concat)

- If tail has room (`tail.data.len < 32`): copy tail, append element
- If tail is full: promote old tail into trie as a new leaf, create fresh
  single-element tail
  - No root yet: create root with old tail as slot 0, `shift = 5`
  - Room in current depth: path-copy right spine, insert old tail
  - Depth full: new root with old root in slot 0, fresh path in slot 1,
    `shift += 5`

### `concat(a, b) -> PVec`

v1: iterate `b` elements and push each onto `a`. This is O(M log32 N)
where M = len(b). A future enhancement can use RRB concatenation for O(log N).

### `slice(vec, start, end) -> PVec`

v1: push elements `[start, end)` into a new empty vector. O(K log32 K) where
K = end - start.

**Known performance cliff**: patterns like `slice(vec, 1, vec.len)` (drop
first element) are O(N log N) with this approach. A trie-native slice that
reuses shared subtrees is possible but significantly more complex. Flagged as
a future enhancement.

### `pop` / drop-last

Not a separate `rt.arr` export today — users write `slice(v, 0, len - 1)`.
With the naive slice this becomes O(N log N). A trie-native `pop` (reverse
of `push` — shrink tail, or demote last trie leaf to tail) is O(log32 N)
and worth adding as a follow-up once the base trie is working.

## Builder Design

The builder currently maintains a 3-slot `Array` with `[buf, len, cap]` and
uses a doubling buffer strategy with amortized O(1) in-place push.

A naive builder that wraps persistent `push` would allocate a new `PVec`
struct and copy the tail array on every push — even when the builder is the
sole owner. This is a real regression for the build-then-freeze pattern
(e.g., `collect` comprehensions), which is the hottest vector construction
path.

### Transient builder with mutable tail

The builder keeps a mutable tail buffer internally and only constructs the
final `PVec` on `freeze`:

- **State**: `[pvec_so_far, tail_buf, tail_len]`
  - `pvec_so_far`: the trie built up so far (tail-only empty vec initially)
  - `tail_buf`: a mutable `$Array` used as the write buffer
  - `tail_len`: `BoxedInt` tracking how many elements are in `tail_buf`
- **`builder_new()`**: `[empty_pvec, Array(cap=32), BoxedInt(0)]`
- **`builder_from(vec)`**: freeze the existing vec's structure into
  `pvec_so_far`, start with a fresh empty tail buffer. This avoids mutating
  any shared trie nodes from the source vector.
- **`builder_push(builder, elem)`**:
  - If `tail_len < 32`: `tail_buf[tail_len] = elem; tail_len += 1`
    (true in-place mutation, amortized O(1))
  - If `tail_len == 32`: promote `tail_buf` as a full leaf into
    `pvec_so_far`'s trie, allocate a fresh 32-slot `tail_buf`,
    write `elem` at slot 0, set `tail_len = 1`
- **`builder_extend(builder, vec)`**: iterate vec, push each element
- **`builder_freeze(builder)`**: construct final `PVec` from
  `pvec_so_far`'s trie + `tail_buf[0..tail_len]` as the tail leaf

This preserves the current amortized O(1) push performance for `collect`
and loop-rewritten append patterns, while producing a proper persistent
vector on freeze.

The 3-slot `$Array` ABI shape is preserved for the builder object itself,
keeping the lowering contract (`VECTOR_BUILDER_NEW/FROM/PUSH/FREEZE`) and
the optimizer's builder region rewrite stable.

**Alias safety**: `builder_from` does not retain mutable references to the
source vector's nodes. The source vector's trie is incorporated into
`pvec_so_far` by reference (safe because trie nodes are never mutated by
the builder — only the transient `tail_buf` is mutated). The fresh
`tail_buf` is builder-private.

## Uniqueness Optimizer Compatibility

### `VECTOR_SET_IN_PLACE`

Currently emits `array.set` on the flat array. With the trie, true in-place
leaf mutation is only safe when the entire path from root to the target leaf
is uniquely owned.

For the initial landing, `VECTOR_SET_IN_PLACE` can be implemented as an alias
for the persistent `set` (path-copy). This preserves correctness while losing
the O(1) fast path.

Follow-up: implement real in-place path mutation for the unique case, which
requires walking the path and mutating nodes directly. This is still O(log32 N)
but avoids allocation.

### Builder rewrites

Continue to work unchanged — the builder ABI is preserved and `push` is the
same logical operation.

### Dict in-place rewrites

Unaffected — dict is a separate data structure.

## Vector Literal Lowering

Currently: `ArrayNewFixed(T_ARRAY, n)` creates a flat array directly.

With the trie:

- Small literals (`<= 32` elements): create a `$Array` via `ArrayNewFixed`,
  wrap in a `VecLeaf`, wrap in a `PVec` with null root
- Large literals (`> 32` elements): use repeated push or a builder

## Boot Compiler Parity

The boot compiler has a mirrored runtime in
`boot/compiler/codegen/runtime/arr.tw`. It must be updated in lockstep:

- `boot/compiler/codegen/runtime/types.tw` — add `PVec`, `VecNode`,
  `VecLeaf`, `VecInternal`, `VecChildren` type definitions
- `boot/compiler/codegen/runtime/arr.tw` — rewrite all operations to use trie

Both stage0 and boot must produce the same runtime representation so programs
compiled by either can use the same runtime module.

## Implementation Phases

### Phase 1: Add Trie Types

- Add `VecNode`, `VecLeaf`, `VecChildren`, `VecInternal`, `PVec` to
  `src/runtime/types.rs`
- Add corresponding ref helpers
- Mirror in `boot/compiler/codegen/runtime/types.tw`
- No functional changes yet — old code still compiles

### Phase 2+3: Rewrite `rt.arr` + Update Codegen (Atomic)

Phases 2 and 3 must land as a single atomic change. Once `rt.arr` expects
`ref $PVec` parameters, codegen must produce `ref $PVec` values — there is
no intermediate state where the runtime expects the new type but codegen
emits the old one.

- Rewrite `make`, `get`, `set`, `len`, `concat`, `slice` in
  `src/runtime/arr.rs` to operate on `PVec`
- Implement transient builder with mutable tail
- Keep `builder_extend` working via iterative push into transient tail
- Implement `VECTOR_SET_IN_PLACE` as persistent set initially
- Change vector valtype from `ref $Array` to `ref $PVec` in codegen
- Update vector literal emission
- Update intrinsic emission (boundaries, casts)
- Update prelude vector ref helpers
- Add empty vector singleton as a Wasm global

### Phase 4: Boot Compiler Parity

This is the highest-effort phase. The trie logic (especially `push` with
tail promotion, depth expansion, and the transient builder) is non-trivial
to express in Twinkle's Wasm IR builder.

Sub-phases:

1. **Types**: add `PVec`, `VecNode`, `VecLeaf`, `VecInternal`, `VecChildren`
   to `boot/compiler/codegen/runtime/types.tw`
2. **Core read ops**: `get`, `len` — simplest to port, good smoke test
3. **Core write ops**: `set`, `make` — path-copy logic
4. **Push + tail promotion**: the most complex single operation
5. **Builder**: transient builder with mutable tail
6. **Remaining ops**: `concat`, `slice`, `builder_extend`
7. **Verify**: boot-compiled programs produce correct output

### Phase 5: Validation

- All existing vector tests pass (`tests/run/vectors.tw`)
- All collect tests pass
- All optimizer tests pass (`tests/opt/*vector*`)
- Boot compiler tests pass (`cargo run --release -- run boot/tests/main.tw`)
- New tests for:
  - Boundary sizes: 32, 33, 1024, 1025
  - Deep append chains (10000+ elements)
  - Structural sharing: modify derived vector, verify original unchanged
  - Builder seeded from existing vector preserves original
  - Large literal lowering

## Risks

- **Tail promotion bugs**: off-by-one in shift/index calculations are the most
  likely failure mode. Careful testing at boundary sizes (32, 33, 1024, 1025)
  is essential.
- **Codegen vector ref type change**: many places assume `Vector<T>` is
  `ref $Array`. The switch to `ref $PVec` will touch codegen broadly.
- **Boot parity**: the mirrored Wasm IR in `boot/compiler/codegen/runtime/`
  must implement the same algorithm, which is substantial.
- **In-place set regression**: the optimizer's `SET_IN_PLACE` becomes a
  persistent set initially, losing the O(1) fast path. This is acceptable
  for correctness but should be followed up.
- **Snapshot churn**: runtime type layout changes will update many test
  snapshots.

## Future Enhancements (Out of Scope)

These remain valid future work, enabled by having a working trie:

1. **Real in-place mutation** for uniquely owned paths
2. **Per-type specialization** (`Vector<Int>` with `i64` leaf slots)
3. **RRB concatenation** for O(log N) concat
4. **Twinkle-authored library ownership** in `boot/lib`
5. **`anyref` elimination** from element storage
