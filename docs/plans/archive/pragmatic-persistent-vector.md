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

### Stage0 / Rust runtime side

The pragmatic persistent-vector migration is already substantially landed on the
stage0 side.

- `src/runtime/types.rs` defines persistent vector runtime types including:
  - `rt_types__VecChildren`
  - `rt_types__VecInternal`
  - `rt_types__PVec`
- `src/runtime/arr.rs` is already a trie-backed persistent vector runtime,
  including internal helpers such as:
  - `tailoff`
  - `get_leaf`
  - `new_path`
  - `push_tail`
  - `do_set`
  - `push`
- stage0 runtime snapshots already characterize the trie-based `PVec`
  representation rather than the old flat array representation.

### Boot compiler mirror side

The boot mirror is still behind and remains the main unfinished part.

- `boot/compiler/codegen/runtime/types.tw` still lacks the persistent vector
  trie types.
- `boot/compiler/codegen/runtime/arr.tw` still implements the old flat-array
  runtime.
- `boot/compiler/builtins.tw` still describes vector ABI in terms of
  `rt_types__Array` rather than `rt_types__PVec`.
- `boot/tests/suites/runtime_suite.tw` still characterizes the old flat-array
  `rt.arr` runtime shape.

### Net status

So the practical remaining work is no longer “land persistent vectors” in the
abstract. It is:

- finish boot runtime parity with the already-landed stage0/runtime shape
- update boot ABI/codegen assumptions from `Array` to `PVec`
- realign boot/runtime tests and snapshots with the trie representation

## Target State

- `Vector<T>` is backed by a persistent bit-partitioned trie with branching
  factor 32 and an unshared tail buffer.
- `get`/`set`: O(log32 N) — at most 7 levels for 34 billion elements.
- `push`: O(1) amortized (tail append) / O(log32 N) when tail overflows.
- Structural sharing: updates copy only the path from root to the modified
  node; all other subtrees are shared across versions.
- Element storage remains `anyref` — no type-family changes.
- The `rt.arr` function set remains stable: same exported names and responsibilities, but vector-valued params/results switch from `ref $Array` to `ref $PVec`.
- The builder system switches to a transient mutable-tail design that preserves amortized O(1) append during construction.
- The uniqueness optimizer's `VECTOR_SET_IN_PLACE` continues to work, but initially lowers to persistent `set` rather than raw in-place leaf mutation.

## Non-Goals

- Per-element-type specialization (`Vector<Int>` with `i64` slots, etc.)
- Moving vector logic to `boot/lib` Twinkle source
- Changing the runtime import boundary or adding new builtin categories
- Changing user-visible `Vector` syntax or method names
- RRB tree concatenation (v1 concat iterates and pushes)

## Type Layout

The current stage0 source of truth in `src/runtime/types.rs` uses this shape:

```wat
;; Internal children array stores eqref, which is either:
;; - a nested VecInternal
;; - a leaf Array directly
(type $VecChildren (array (mut (ref null eq))))

(type $VecInternal (struct
  (field $children (ref $VecChildren))))

(type $PVec (struct
  (field $len i32)
  (field $shift i32)
  (field $root (ref null $VecInternal))
  (field $tail (ref $Array))))
```

Important note: the implementation has already evolved away from the older
`VecNode` / `VecLeaf` wrapper sketch.

Current layout policy:

- `VecChildren` stores `eqref`
- internal children are `VecInternal`
- leaf payloads are bare `Array` refs stored directly in `VecChildren`
- `PVec.tail` is a bare `Array` ref, not a wrapped `VecLeaf`

The existing `$Array` type (`array (mut anyref)`) is reused for leaf element
storage, keeping all existing boxing/unboxing paths working.

### Empty Vector Singleton

A shared empty vector is allocated once as a global:

- `EMPTY_TAIL`: a zero-length `$Array`
- `EMPTY_VEC`: a `$PVec` with `len=0, shift=0, root=null, tail=EMPTY_TAIL`

All operations that produce empty vectors (`make(0, _)`, slicing to zero
length, etc.) return this singleton to avoid repeated allocation.

### Key Invariants

- `0 <= tail.len <= 32`
- `len = trie_element_count + tail.len`
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

This boundary has already changed on the stage0/runtime side, but not yet on the
boot mirror.

Relevant touchpoints:

- stage0/runtime side:
  - `src/runtime/types.rs`
  - `src/runtime/arr.rs`
  - `src/codegen/prelude.rs`
  - `src/codegen/emit.rs`
  - `src/codegen/ctx.rs`
- boot mirror side still needing parity:
  - `boot/compiler/codegen/runtime/types.tw`
  - `boot/compiler/codegen/runtime/arr.tw`
  - `boot/compiler/builtins.tw`
  - `boot/compiler/codegen/emit.tw`
  - boot boundary/ABI/runtime tests that still assert `rt_types__Array`

The `rt.arr` export signatures change their vector parameter/return types from
`ref $Array` to `ref $PVec`, but the **function names and count stay the
same**.

## Core Operations

### `make(len, fill) -> PVec`

- `len == 0`: return shared empty vector singleton
- `len <= 32`: tail-only vector with `len` copies of `fill`
- `len > 32`: repeated `push` from empty (amortized O(N) overall; most pushes only copy the tail, with trie promotion every 32 elements)

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
final `PVec` on `freeze`.

Current stage0 implementation note:

- the stage0 helper set already includes `builder_extend`
- stage0 also includes vector/array bridge helpers such as `from_array`,
  `to_array`, and `from_read_file_result`
- boot parity should be checked against the current stage0 helper surface,
  not only the smaller original sketch

- **State**: `[pvec_so_far, tail_buf, tail_len]`
  - `pvec_so_far`: the trie built up so far (tail-only empty vec initially)
  - `tail_buf`: a mutable `$Array` used as the write buffer
  - `tail_len`: `BoxedInt` tracking how many elements are in `tail_buf`
- **`builder_new()`**: `[empty_pvec, Array(cap=32), BoxedInt(0)]`
- **`builder_from(vec)`**: split the existing vector at its tail boundary.
  - `pvec_so_far` receives only the source vector's trie/full-leaf prefix
    (that is, all elements before `tailoff(vec.len)`)
  - the source tail elements are copied into the fresh mutable `tail_buf`
  - `tail_len` is initialized to the copied tail length

  This preserves the normal persistent-vector invariant that `pvec_so_far`
  contains only full 32-element leaves plus an empty tail, while the builder's
  mutable right edge lives entirely in `tail_buf`. It also avoids mutating any
  shared trie nodes from the source vector.
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

**Builder invariant**: `pvec_so_far` is always a valid `PVec` whose tail is
empty; all builder-private, not-yet-frozen right-edge elements live in
`tail_buf[0..tail_len]`. When `tail_buf` fills, it is promoted as a full leaf
into `pvec_so_far` and replaced with a fresh empty 32-slot buffer.

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

Today this intrinsic lowers directly to raw `array.set` in codegen. With the
trie representation, true in-place leaf mutation is only safe when the entire
path from root to the target leaf is uniquely owned.

For the initial landing, `VECTOR_SET_IN_PLACE` should lower to the persistent
`set` implementation (path-copy) rather than emitting raw mutation. This
preserves correctness while losing the old O(1) flat-array fast path.

This requires updates in both stage0 and boot intrinsic lowering, not just in
`rt.arr`, because the current implementation bypasses the runtime helper layer.

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

This is now the primary remaining scope of the plan.

The boot compiler has a mirrored runtime in
`boot/compiler/codegen/runtime/arr.tw`, but it still reflects the old flat
array design. It must be brought into parity with the already-landed
stage0/runtime `PVec` representation.

Required parity areas:

- `boot/compiler/codegen/runtime/types.tw`
  - add the persistent vector runtime types needed by the current stage0 shape
  - match stage0 naming/layout closely enough for shared runtime expectations
  - use the current `VecChildren + VecInternal + PVec` design, not the older
    `VecNode` / `VecLeaf` sketch
- `boot/compiler/codegen/runtime/arr.tw`
  - rewrite all vector operations to use the trie representation
  - add the internal helper functions needed by the current stage0 runtime
    design
  - include the full current stage0 helper surface where required, including
    `builder_extend` and array/vector bridge helpers if boot/runtime parity
    depends on them
- `boot/compiler/builtins.tw`
  - switch vector ABI metadata from `rt_types__Array` to `rt_types__PVec`
- `boot/compiler/codegen/emit.tw`
  - update direct emitter assumptions and runtime calls so vector-typed values
    are treated as `PVec`
- `boot/tests/suites/runtime_suite.tw` and related boot tests
  - stop characterizing the old flat-array runtime shape
  - start characterizing the trie-based runtime shape that stage0 already uses

Both stage0 and boot must produce the same runtime representation so programs
compiled by either can use the same runtime module.

## Implementation Phases

## Implementation Status

### Completed on stage0/runtime side

These parts are already effectively done and should be treated as the current
source of truth:

- persistent vector runtime types in `src/runtime/types.rs`
- trie-backed `rt.arr` implementation in `src/runtime/arr.rs`
- stage0/runtime snapshots that already describe `PVec`
- stage0-side runtime representation and helper set used by current builds

### Remaining: boot parity work

The remaining implementation work is to port that stage0/runtime design into the
boot mirror and align boot ABI/codegen/tests.

Recommended execution order:

1. **Boot runtime types**
   - update `boot/compiler/codegen/runtime/types.tw`
   - add the persistent vector trie types needed by the current stage0 shape
   - mirror the actual current layout (`VecChildren`, `VecInternal`, `PVec`)
2. **Boot read-path parity**
   - update `boot/compiler/codegen/runtime/arr.tw`
   - port `len` and `get`
   - port helper functions required to navigate the trie (for example `tailoff`
     and `get_leaf` equivalents)
3. **Boot write-path parity**
   - port `make` and `set`
   - port the internal path-copy helpers used by stage0 (for example `do_set`)
4. **Push + tail promotion**
   - port the persistent `push` logic
   - port helper functions such as `new_path` / `push_tail` equivalents
5. **Builder parity**
   - port `builder_new`
   - port `builder_from`
   - port `builder_push`
   - port `builder_extend`
   - port `builder_freeze`
   - preserve alias-safety and the current collect/loop-builder contract
6. **Bridge/helper parity as needed**
   - audit whether boot also needs the current stage0 bridge helpers:
     `from_array`, `to_array`, `from_read_file_result`
   - if they are part of the shared runtime contract, port them too
7. **Boot ABI/codegen parity**
   - update `boot/compiler/builtins.tw`
   - update `boot/compiler/codegen/emit.tw`
   - ensure boot-side `VECTOR_SET_IN_PLACE` follows the persistent-vector
     representation rather than flat-array assumptions
7. **Boot tests/snapshots parity**
   - update runtime suite expectations
   - update ABI/layout tests
   - re-run the full boot test driver

## Validation

### Running boot tests during development

The preferred way to run boot compiler tests during development is the two-step
Node.js approach, which is ~5× faster than `twk run` (~3s vs ~16s):

```bash
# Full compile + run (~3s)
tools/boot-test-fast.sh

# Run-only, reuse last compiled .wasm (~1s)
tools/boot-test-fast.sh --run-only
```

This compiles to Wasm via `twk build`, then executes via Node.js (whose V8 Wasm
GC implementation is significantly faster than wasmtime for this workload).
The `--run-only` flag is useful when iterating on test expectations without
changing boot compiler source.

The canonical full-fidelity command remains:

```bash
cargo run --release --bin twk -- run boot/tests/main.tw
```

### Stage0/runtime status

Stage0/runtime validation already includes trie-based runtime snapshots and the
existing vector behavior/perf work built on top of that representation.

### Boot parity exit criteria

The plan should be considered complete when all of the following hold on the
boot side:

- boot runtime types define the persistent vector shape expected by stage0
- boot `rt.arr` uses trie-based `PVec`, not flat arrays
- boot builtin ABI metadata uses `rt_types__PVec`
- boot emitter/runtime call sites no longer assume vector = `rt_types__Array`
- boot runtime suite no longer characterizes the flat-array `rt.arr`
- boot runtime suite characterizes the actual current stage0 trie layout rather
  than an outdated `VecNode` / `VecLeaf` sketch
- existing vector tests pass (`tests/run/vectors.tw`)
- collect tests pass
- optimizer tests pass (`tests/opt/*vector*`)
- boot compiler tests pass (`cargo run --release -- run boot/tests/main.tw`)

Important focused coverage:

- boundary sizes: 32, 33, 1024, 1025
- deep append chains
- structural sharing
- builder seeded from existing vector preserves original
- builder/collect parity
- large literal lowering

## Risks

- **Boot parity is the main risk now**: the mirrored Wasm IR in
  `boot/compiler/codegen/runtime/` still needs a substantial port of the
  trie-based runtime logic.
- **Tail promotion bugs**: off-by-one in shift/index calculations are the most
  likely failure mode. Careful testing at boundary sizes (32, 33, 1024, 1025)
  is essential.
- **Direct boot emitter assumptions**: it is not enough to update runtime types
  and builtin ABI metadata; direct emitter/runtime call sites also need to stop
  assuming vector = `rt_types__Array`.
- **In-place set regression**: the optimizer's `SET_IN_PLACE` becomes a
  persistent set initially, losing the O(1) fast path. This is acceptable
  for correctness but should be followed up.
- **Snapshot churn**: boot runtime/layout snapshots will change as the mirror is
  brought into parity with stage0.

## Future Enhancements (Out of Scope)

These remain valid future work, enabled by having a working trie:

1. **Real in-place mutation** for uniquely owned paths
2. **Per-type specialization** (`Vector<Int>` with `i64` leaf slots)
3. **RRB concatenation** for O(log N) concat
4. **Twinkle-authored library ownership** in `boot/lib`
5. **`anyref` elimination** from element storage
