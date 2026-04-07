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

## Phase 1 results: where the cost actually is

### Benchmark matrix

Criterion benchmarks in `benches/tw/` measure indexed reads (`xs[i]`) and
iterator traversal (`for x in xs`) across vector sizes, trie depths, and
access patterns. All times are median wall-clock from criterion with
`sample_size(10)`.

```
Benchmark                Depth  Total (ms)        Reads   ns/read  Notes
------------------------------------------------------------------------------------------
get_tiny                     0       119.5   32,000,000       3.7  tail-only, 32 elem
get_shallow                  1       267.9   50,000,000       5.4  1000 elem, 50M reads
get_shallow_matched          1        25.8    2,500,000      10.3  1000 elem, 2.5M reads
get_1024                     1        26.2    2,560,000      10.2  depth boundary low
get_1025                     2        26.8    2,562,500      10.5  depth boundary high
get_deep                     2      1200.3    2,500,000     480.1  50k elem
tail_48                      1        62.8   16,000,000       3.9  tail-only, 48-elem vec
tail_1040                    2        75.4   16,000,000       4.7  tail-only, 1040-elem vec
tail_50000                   2      1244.4   16,000,000      77.8  tail-only, 50k-elem vec
iter_tiny                    0       112.0   32,000,000       3.5  iterator, 32 elem
iter_sum                     2      1203.7    2,500,000     481.5  iterator, 50k elem
```

### Key findings

**1. Trie depth / `ref.cast` overhead is negligible.**

`get_1024` (depth 1) vs `get_1025` (depth 2): 10.2 vs 10.5 ns/read. Adding
an entire trie level — with its `ref.cast` to `VecInternal` — costs ~0.3 ns.
The unified node layout (old Phase 2) would save this amount, which is
immaterial compared to the real bottleneck.

**2. GC object graph size is the dominant cost.**

Tail-only reads (no trie descent, no casts) prove this conclusively:

| Vector size | GC objects | Tail ns/read | Ratio vs 48 |
|---|---|---|---|
| 48 elements | ~7 | 3.9 | 1× |
| 1,040 elements | ~69 | 4.7 | 1.2× |
| 50,000 elements | ~3,231 | 77.8 | **20×** |

Going from 1,040 → 50,000 elements (same trie depth!) causes a 17× slowdown
on reads that never touch the trie. The only difference is the number of GC
objects reachable from the `$PVec` reference.

**3. The 50k-vector penalty is a flat tax on every access.**

- `get_deep` (trie reads on 50k): 480 ns/read
- `tail_50000` (tail reads on 50k): 78 ns/read
- Trie descent adds ~400 ns on top, but the 78 ns GC baseline is already 20×
  worse than small vectors.

**4. Cache warming matters but doesn't explain the gap.**

`get_shallow` at 50M total reads amortizes to 5.4 ns/read; `get_shallow_matched`
at 2.5M reads shows 10.3 ns/read (~2×). But `tail_50000` at 16M reads (plenty
of warming) still shows 78 ns — the GC graph cost persists regardless of cache
temperature.

**5. Iterator overhead is negligible.**

`iter_tiny` ≈ `get_tiny` (3.5 vs 3.7 ns), `iter_sum` ≈ `get_deep` (481 vs 480 ns).
The `for x in xs` lowering adds no measurable cost beyond the underlying reads.

### What this rules out

The original plan focused on `ref.cast` elimination via a unified node layout.
The data shows this would save ~0.3 ns/read — a rounding error against the
~475 ns/read regression on large vectors.

The bottleneck is not:
- `ref.cast` vs `ref.as_non_null` (negligible difference)
- trie depth (depth 1 vs 2 is identical)
- iterator lowering (matches indexed reads)
- helper call layering (already eliminated in previous cleanup)

The bottleneck **is**:
- the sheer number of GC objects reachable from a large `$PVec`
- likely Wasmtime GC tracing / write-barrier / object-graph bookkeeping
  scaling with the number of live reference-typed fields

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

### GC object count at branching factor 32

| Vector size | Leaf nodes | Internal nodes | Total GC objects* |
|---|---|---|---|
| 32 | 0 | 0 | 3 (PVec + tail leaf + tail array) |
| 1,024 | 31 | 1 | ~67 |
| 1,040 | 32 | 1 | ~69 |
| 10,000 | 312 | 11 | ~649 |
| 50,000 | 1,562 | 52 | ~3,231 |

*Each leaf = 1 VecLeaf struct + 1 Array; each internal = 1 VecInternal struct + 1 VecChildren array; plus PVec + tail.

### Cast inventory (preserved for reference)

See git history for the full cast inventory. The Phase 1 measurements show
cast cost is ~0.3 ns per trie level, making it a non-issue for optimization.

## Revised path forward

The goal is to reduce the number of GC objects reachable from a `$PVec`,
since that is the dominant cost driver.

### Phase 2: Reduce GC object count

There are three complementary approaches, ordered by expected impact and
implementation complexity.

#### 2a. Eliminate leaf wrapper structs

**Current:** Each trie leaf is a `$VecLeaf` struct wrapping a `$Array`.
Two GC objects per leaf.

**Proposed:** Store `$Array` references directly in the children array.
Use `(array (mut (ref null $VecNode)))` where `$VecNode` is `anyref` or
an `eqref`, and cast to `$Array` at the leaf level.

This halves the leaf-level GC objects (the largest contributor). For a
50k vector: ~1,562 fewer GC objects.

**Trade-off:** Requires one `ref.cast` at the leaf step to recover the
`$Array` type. Phase 1 showed a single cast costs ~0.3 ns — negligible.

**Complexity:** Moderate. Changes `$VecChildren` element type and the
leaf construction/access patterns in `arr.rs`. Does not change the trie
algorithm.

#### 2b. Wider branching factor

**Current:** Branching factor 32 (B=5). A 50k vector has ~1,562 leaves
and ~52 internal nodes.

**Proposed:** Branching factor 64 (B=6) or 128 (B=7).

| BF | Leaves (50k) | Internals | Total GC objects | Reduction |
|---|---|---|---|---|
| 32 | 1,562 | 52 | ~3,231 | baseline |
| 64 | 781 | 13 | ~1,590 | ~51% |
| 128 | 391 | 4 | ~793 | ~75% |

**Trade-off:** Wider nodes waste more space on partially-filled children
arrays. A BF=64 children array is 64 reference slots (~512 bytes) even if
only a few are populated. For persistent updates, each `set` copies the
affected children array, so wider = more copying per update.

Write-path benchmarks (`vector_set_chain`, `vector_append_chain`) should
be checked to ensure the write-cost increase doesn't outweigh the read
improvement.

**Complexity:** Low. Change `B` constant, update `MASK`/`BF`, adjust
`tailoff`. The trie algorithm is parameterized on `B` already.

#### 2c. Flatten small-to-medium vectors (not planned)

This would use a flat `$Array` for vectors below some threshold, avoiding
the trie entirely for small vectors. However, the complexity is high
(representation tag, conditional dispatch in every operation, promotion
logic) and the benefit is uncertain given that 2a+2b should already
substantially reduce GC object count. If further optimization is needed
after 2a+2b, Phase 3 (Wasmtime GC investigation) is a better direction.

### Phase 2 implementation order

**2b (wider branching factor) was tried first and rejected.** Changing the
branching factor from 32 to 64 made read performance materially worse:

| Benchmark | BF=32 ns/read | BF=64 ns/read | Change |
|---|---|---|---|
| `get_deep` | 480.1 | 705.7 | +47% |
| `tail_50000` | 77.8 | 112.8 | +45% |
| `iter_sum` | 480.2 | 705.6 | +47% |

This indicates Wasmtime's GC cost is sensitive not just to object count, but
also to the number of reference slots per object. Wider `VecChildren` arrays
scan worse than narrower ones, even though there are fewer total nodes.

**2a (eliminate leaf wrappers) is the active Phase 2 direction.** This keeps
branching factor 32 but removes `VecLeaf` wrapper structs, storing leaf
`Array` refs directly in `VecChildren` and in `PVec.tail`.

Implementation status:
- `VecNode` and `VecLeaf` removed from `src/runtime/types.rs`
- `VecChildren` now stores `(ref null eq)`
- `PVec.tail` is now `(ref $Array)`
- `src/runtime/arr.rs` rewritten so leaf arrays are stored directly
- `src/codegen/emit.rs` updated so vector literals construct `PVec` with a
  bare array tail
- `src/runtime/dict.rs` updated for the new `PVec` layout

Measured result after 2a:

| Benchmark | Before ns/read | After ns/read | Change |
|---|---|---|---|
| `get_deep` | 480.1 | 392.0 | **-18.4%** |
| `tail_50000` | 77.8 | 63.4 | **-18.5%** |
| `iter_sum` | 481.5 | 392.9 | **-18.4%** |
| `get_1025` | 10.5 | 9.5 | -9.2% |
| `get_tiny` | 3.7 | 3.2 | -15.4% |

This confirms the earlier hypothesis: reducing GC object count helps, and it
helps even when the access path never descends the trie (`tail_50000`).

**2c (flat small vectors) is not planned.** The complexity of maintaining
two representation paths (flat + trie) with conditional dispatch and
promotion logic is high. If 2a is still insufficient, Phase 3 investigation
into Wasmtime GC behavior is a better next step than adding representation
complexity.

### Phase 2 validation

- `cargo build --release`
- Run full `cargo bench --bench wasm_exec -- vector_read_depth` suite
- Compare against current baseline (this document's Phase 1 numbers)
- Run `cargo test`
- Run `cargo run --release -- run boot/tests/main.tw`
- Check write-path benchmarks (`vector_set_chain`, `vector_append_chain`)
  to ensure no regression

### Phase 3: Further investigation (if Phase 2 isn't sufficient)

**3a. Wasmtime GC behavior profiling.**
The tail-only read scaling (3.9 → 77.8 ns as vector size grows) suggests
Wasmtime's GC is doing work proportional to the reachable object graph
on every access. This could be:
- write barriers on `struct.get` of reference fields
- GC safepoint checks scaling with heap pressure
- object layout / memory fragmentation from many small GC allocations

Profiling Wasmtime itself (e.g. `perf record` on the host) during the
`tail_50000` benchmark could identify the exact source.

**3b. Compact read-only snapshots.**
If the boot compiler's hot loops are build-then-read (collect + iterate),
the builder could freeze into a flat array representation that bypasses
the trie entirely for reads. The `builder_freeze` path already exists —
it could produce a compact `$Array`-backed `$PVec` variant.

**3c. Investigate Wasmtime GC improvements.**
The scaling behavior may improve in newer Wasmtime releases as Wasm GC
support matures. Track Wasmtime's GC-related changelogs.

## Explicit non-goals

Do **not** change the data structure family (RRB trees, finger trees,
ropes, HAMT hybrids). The persistent bit-partitioned trie is correct for
this use case — the problem is GC object count, not algorithmic complexity.

Do **not** reintroduce complex stage0 codegen special cases. The cost is
in the runtime representation, not in how user code lowers to Wasm.

The unified node layout (old Phase 2) is **deprioritized**. It eliminates
`ref.cast` but saves only ~0.3 ns/read. It can be revisited as a cleanup
after the GC object count reduction is done, but it is not an optimization
priority.

## Benchmark files

All benchmarks live in `benches/tw/` and are wired into criterion via
`benches/wasm_exec.rs` under the `vector_read_depth` group.

| File | Size | Depth | Access | Total reads | Purpose |
|---|---|---|---|---|---|
| `vector_get_tiny.tw` | 32 | 0 | indexed | 32M | Tail-only baseline |
| `vector_get_shallow.tw` | 1,000 | 1 | indexed | 50M | Depth 1, hot cache |
| `vector_get_shallow_matched.tw` | 1,000 | 1 | indexed | 2.5M | Depth 1, matched reads |
| `vector_get_1024.tw` | 1,024 | 1 | indexed | 2.56M | Depth boundary low |
| `vector_get_1025.tw` | 1,025 | 2 | indexed | 2.56M | Depth boundary high |
| `vector_get_deep.tw` | 50,000 | 2 | indexed | 2.5M | Depth 2, large working set |
| `vector_get_tail_48.tw` | 48 | 1 | tail-only | 16M | GC graph baseline (small) |
| `vector_get_tail_1040.tw` | 1,040 | 2 | tail-only | 16M | GC graph baseline (medium) |
| `vector_get_deep_tail_only.tw` | 50,000 | 2 | tail-only | 16M | GC graph cost (large) |
| `vector_iter_tiny.tw` | 32 | 0 | iterator | 32M | Iterator vs indexed comparison |
| `vector_iter_sum.tw` | 50,000 | 2 | iterator | 2.5M | Iterator at scale |

Existing write-path benchmarks (not yet in criterion):
- `vector_append_chain.tw` — repeated persistent append
- `vector_append_indirect.tw` — append through helper function
- `vector_collect_sum.tw` — builder path construction
- `vector_set_chain.tw` — persistent point updates
