# Persistent vector read-path follow-up

## Current state

Persistent vectors stay in place for the stage0 Wasm backend.

We also keep the runtime-side read-path cleanup in `src/runtime/arr.rs`:
- `rt_arr__get` is monolithic again
- small vectors (`len <= 32`) use a direct tail-only fast path
- tail reads are handled directly in `get`
- the read path no longer depends on `get -> get_leaf -> final array.get`

An experimental stage0 codegen optimization pass was tried and then reverted.
That experiment included:
- chunked lowering for `for x in xs`
- inline stage0 lowering for `xs[i]` and `Vector.get`
- extra Wasm-local machinery to support those paths

The experiment made the backend more complex but did not materially improve the
observed read-heavy regressions, so it is not the direction to keep pursuing.

## What we learned

The persistent vector algorithm itself is still the right family of data structure.
The remaining regression appears more likely to come from the **Wasm GC access
shape** than from the bit-partitioned trie design itself.

The current evidence points more toward:
- dynamic `ref.cast` overhead during trie descent
- nullable/non-null transitions in hot reads
- GC object-graph traversal cost through `VecInternal` / `VecLeaf`
- Wasmtime optimization limits around current Wasm GC patterns

and less toward:
- helper call layering alone
- generic loop lowering alone
- the persistent vector algorithm being fundamentally wrong for the use case

## Current type hierarchy and where casts occur

### Type definitions (`src/runtime/types.rs`)

```wat
(type $VecNode     (sub (struct)))                                    ;; abstract base, non-final
(type $VecLeaf     (sub $VecNode (struct (field $data (ref $Array))))) ;; leaf: holds element array
(type $VecChildren (array (mut (ref null $VecNode))))                  ;; child slots (typed as base)
(type $VecInternal (sub $VecNode (struct (field $children (ref $VecChildren)))))
(type $PVec        (struct
                     (field $len i32)
                     (field $shift i32)
                     (field $root (ref null $VecInternal))
                     (field $tail (ref $VecLeaf))))
```

The root cause of all runtime casts: **`$VecChildren` stores `ref null $VecNode`**.
Every `array.get` from a children array returns the abstract base type, forcing a
`ref.cast` to recover the concrete `$VecInternal` or `$VecLeaf` type.

### Cast inventory in `src/runtime/arr.rs`

**Hot read path — `get` (27 total `RefCast` in file, breakdown below):**

| Location | Operation | Count per call |
|---|---|---|
| `get` loop body | `ref.cast (ref null $VecInternal)` after `array.get $VecChildren` | 1 per trie level |
| `get` loop body | `ref.as_non_null` on nullable `$VecInternal` local | 1 per trie level |
| `get` final step | `ref.cast (ref $VecLeaf)` after `array.get $VecChildren` | 1 |

For a vector with 1025–32768 elements (shift=10, depth=2 internal levels):
- **2× `ref.cast` to `VecInternal`** (loop iterations)
- **2× `ref.as_non_null`** (loop iterations)
- **1× `ref.cast` to `VecLeaf`** (final step)
- **Total: 5 dynamic type checks per trie read**

**Write paths — `push_tail`, `do_set`, `push`, `new_path`, `set`:**

| Function | Cast | Why |
|---|---|---|
| `push_tail` | `VecNode → VecInternal` (nullable) | Downcast child for recursion |
| `push_tail` | `VecInternal → VecNode` | Upcast result to store in `$VecChildren` |
| `do_set` (leaf branch) | `VecNode → VecLeaf` | Downcast to access `data` field |
| `do_set` (leaf branch) | `VecLeaf → VecNode` | Upcast result to return as `VecNode` |
| `do_set` (internal branch) | `VecNode → VecInternal` (×2) | Downcast for children copy + child access |
| `do_set` (internal branch) | `VecInternal → VecNode` | Upcast result to return |
| `push` (overflow) | `VecInternal → VecNode` | Upcast old root for `$VecChildren` storage |
| `push` (overflow) | `VecLeaf → VecNode` | Upcast old tail for `new_path` |
| `push` (no overflow) | `VecLeaf → VecNode` | Upcast old tail for `push_tail` |
| `push` (result) | `VecNode → VecInternal` (nullable) | Downcast `push_tail`/`new_path` result for `$PVec.root` |
| `new_path` | `VecInternal → VecNode` | Upcast newly created node to store in `$VecChildren` |
| `set` | `VecInternal → VecNode` | Upcast root for `do_set` call |
| `set` | `VecNode → VecInternal` | Downcast `do_set` result back for `$PVec.root` |

### Why existing optimization passes can't help

The monomorphization pass (`src/ir/monomorphize.rs`) and peephole optimizers
(`src/opt/pipeline.rs`, `src/opt/passes.rs`) operate on Core IR / ANF IR — the
user-level intermediate representations. The `ref.cast` instructions are emitted
directly in the hand-written Wasm IR for the runtime (`src/runtime/arr.rs`) and
in the codegen backend (`src/codegen/emit.rs`). These passes have no visibility
into or control over the runtime's Wasm instructions.

The uniqueness pass (`src/opt/uniqueness.rs`) can rewrite COW update operations
(e.g. `VECTOR_APPEND → VECTOR_APPEND_IN_PLACE`) but doesn't affect the internal
trie traversal shape — the in-place variant still uses the same `$VecNode` type
hierarchy and the same cast-heavy descent.

## Path forward

### Phase 1: Measure the remaining read cost more directly

Add focused microbenchmarks or targeted WAT inspection around:
- tiny vectors (`len <= 32`)
- larger vectors hitting the tail path
- trie reads with `shift == 5`
- trie reads with `shift == 10`

Goal:
- separate small-vector behavior from true trie-descent behavior
- identify whether the cost is mostly in casts, nullability, or depth

### Phase 2: Unified node layout (eliminates all trie-descent casts)

Replace the `VecNode` / `VecLeaf` / `VecInternal` subtype hierarchy with a
single non-abstract struct. This eliminates every `ref.cast` in the trie
descent loop because `$VecChildren` array elements are already the concrete
type — no downcast needed.

#### 2a. New type definitions (`src/runtime/types.rs`)

Remove `$VecNode`, `$VecLeaf`, `$VecInternal`. Replace with:

```wat
;; Single unified node — no subtype hierarchy, no abstract base
(type $VecUNode (struct
  (field $children (ref null $VecUChildren))  ;; non-null for internal nodes, null for leaves
  (field $data     (ref null $Array))))        ;; non-null for leaves, null for internal nodes

(type $VecUChildren (array (mut (ref null $VecUNode))))

(type $PVec (struct
  (field $len   i32)
  (field $shift i32)
  (field $root  (ref null $VecUNode))   ;; was: ref null $VecInternal
  (field $tail  (ref $VecUNode))))      ;; was: ref $VecLeaf — always a leaf-shaped node
```

Trade-off: every node carries one wasted null field (internal nodes have
`data = null`, leaves have `children = null`). This is 1 reference-sized
slot per node — negligible compared to the 32-slot children/data arrays
they point to.

#### 2b. Read path changes (`get` / `get_leaf` in `src/runtime/arr.rs`)

**Before (current):**
```
loop:
  node = ref.as_non_null(node_local)        ;; nullable → non-null
  children = struct.get $VecInternal 0      ;; get children array
  child = array.get $VecChildren (idx)      ;; returns ref null $VecNode
  node_local = ref.cast (ref null $VecInternal) child  ;; DOWNCAST
  ...
final:
  child = array.get $VecChildren (idx)      ;; returns ref null $VecNode
  leaf = ref.cast (ref $VecLeaf) child      ;; DOWNCAST
  data = struct.get $VecLeaf 0
```

**After (unified):**
```
loop:
  node = ref.as_non_null(node_local)        ;; still needed (root is nullable)
  children = struct.get $VecUNode 0         ;; get children — statically typed
  children_nn = ref.as_non_null(children)   ;; children is ref null, assert non-null
  child = array.get $VecUChildren (idx)     ;; returns ref null $VecUNode — SAME type
  node_local = child                        ;; NO CAST — already the right type
  ...
final:
  child = array.get $VecUChildren (idx)     ;; returns ref null $VecUNode
  child_nn = ref.as_non_null(child)         ;; assert non-null
  data = struct.get $VecUNode 1             ;; get data field — statically typed
  data_nn = ref.as_non_null(data)           ;; data is ref null $Array, assert non-null
```

Net effect on a depth-2 read:
- **Before:** 2× `ref.cast VecInternal` + 2× `ref.as_non_null` + 1× `ref.cast VecLeaf` = **5 dynamic checks**
- **After:** 3× `ref.as_non_null` (root, children field, leaf data field) = **3 null checks, 0 casts**

`ref.as_non_null` is cheaper than `ref.cast` — it's a null-pointer check, not a
full runtime type test against the GC type hierarchy.

**Debuggability note:** With the unified layout, a bug causing trie traversal
into a leaf node (accessing its null `children` field) would trap with a null
deref rather than a `ref.cast` failure. Cast failures carry type information in
the error; null traps don't. This is acceptable for a correct implementation but
worth knowing when debugging trie invariant violations.

#### 2c. Write path changes (`push_tail`, `do_set`, `push`, `new_path`)

All upcast/downcast pairs disappear because there's only one node type.

**`push_tail`:** Currently casts `VecNode → VecInternal` for recursion and
`VecInternal → VecNode` when returning. Both become identity — the child
from `array.get $VecUChildren` is already `ref null $VecUNode`, and the
`struct.new $VecUNode` result is already the right type to store back.

**`do_set`:** Currently branches on `level == 0` and casts to `VecLeaf` or
`VecInternal`. With unified nodes, the branch remains (to decide whether to
copy `data` or `children`) but the casts become `struct.get $VecUNode 0/1`
with null checks — no `ref.cast`.

**`new_path`:** Currently casts `VecInternal → VecNode` after wrapping.
Becomes a direct `struct.new $VecUNode` — already the storage type.

**`push`:** Currently casts `VecLeaf → VecNode` (old tail for promotion) and
`VecNode → VecInternal` (result). Both become identity on the unified type.

#### 2d. Node construction helpers

Leaf construction (replaces `StructNew(T_VEC_LEAF)`):
```wat
ref.null $VecUChildren   ;; children = null (it's a leaf)
<data ref>               ;; data = the element array
struct.new $VecUNode
```

Internal construction (replaces `StructNew(T_VEC_INTERNAL)`):
```wat
<children ref>           ;; children = the children array
ref.null $Array          ;; data = null (it's an internal node)
struct.new $VecUNode
```

#### 2e. Files to change

| File | What changes |
|---|---|
| `src/runtime/types.rs` | Remove `VecNode`, `VecLeaf`, `VecInternal`, `VecChildren`. Add `VecUNode`, `VecUChildren`. Update `PVec` fields. Update type constant names and ref helpers. |
| `src/runtime/arr.rs` | Rewrite all functions to use unified node. Remove all `RefCast` to/from `VecNode`/`VecLeaf`/`VecInternal`. Replace with `struct.get` + `ref.as_non_null`. |
| `src/codegen/emit.rs` | Update any references to `T_VEC_LEAF`, `T_VEC_INTERNAL`, `T_VEC_NODE`, `T_VEC_CHILDREN` to use new type names. Update vector literal emission (currently does `ArrayNewFixed → StructNew VecLeaf`). |
| `src/codegen/ctx.rs` | References `T_PVEC` (lines 14, 619, 2494). Changes likely minimal since `PVec` keeps its name — only needed if ref helpers for the old `VecLeaf`/`VecInternal`/`VecNode` types are used here. |

#### 2f. Validation

- `cargo test` — all existing tests pass
- `cargo run --release -- run boot/tests/main.tw` — boot compiler tests pass
- Manual WAT inspection of `get` to confirm zero `ref.cast` in trie descent
- Compare benchmark against `795d1c8` for read-heavy workloads

### Phase 3: Further optimizations (if unified layout isn't sufficient)

These are independent follow-ups, only worth pursuing if Phase 2 doesn't
close the gap. "Close the gap" means read-heavy benchmarks are within ~10%
of the `795d1c8` baseline; if they're still >10% slower after Phase 2,
investigate Phase 3 options with profiling data.

**3a. Non-null children field for internal nodes.**
The unified node uses `ref null $VecUChildren` for the children field, requiring
`ref.as_non_null` during descent. An alternative: split into two struct types
again but without a subtype relationship — use `anyref` in children array and
a single `ref.cast` only at the leaf step. This is worse than unified if
Wasmtime treats `ref.as_non_null` as nearly free (which it should).

**3b. Inline `get` at codegen time for constant-shift vectors.**
If profiling shows the loop overhead (branch on `level > B`) matters for
shallow tries, emit unrolled depth-specific `get` variants. Only justified
if Phase 2 measurements show the loop branch itself is significant vs. the
cast overhead we've already removed.

## Explicit non-goals for now

Do **not** rush into changing the representation family to something unrelated,
such as:
- RRB trees
- finger trees
- ropes
- HAMT hybrids for vectors

Those solve different problems and are not justified by the current evidence.

Also avoid reintroducing complex stage0-only codegen special cases unless a new
measurement clearly shows that runtime representation costs are no longer the
main bottleneck.

## Recommended next implementation target

1. Implement Phase 2 (unified node layout) in one pass:
   - Update types in `src/runtime/types.rs`
   - Rewrite `src/runtime/arr.rs` against the new types
   - Update `src/codegen/emit.rs` references
2. Validate with `cargo test` + boot compiler tests
3. Inspect emitted WAT to confirm cast elimination
4. Benchmark against `795d1c8`
