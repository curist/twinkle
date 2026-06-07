# Wasm-native sort working set for dataframe `order_by` — Implementation Plan

**Goal:** Make dataframe `order_by` substantially faster by moving the hot sort working set out of persistent vectors and into dense mutable Wasm runtime arrays for the duration of the sort. The first target is sorting row indices by an `Int` key column, because the current benchmark's `amount` column is `Int` and the measured hotspot is repeated random PVec key reads inside the comparator.

**Non-goal:** This is not a replacement for persistent `Vector<T>` as the language collection. Persistent vectors remain the public value model. The native sort uses temporary runtime arrays internally, then freezes back to `Vector<Int>` row indices.

**Current conclusion:** `Vector.gather` and typed dataframe gather cleanup helped join and made the API cleaner, but `order_by` remains dominated by the sort comparator. Microbenchmarks show the expensive path is `idx.sort_by(fn(a, b) { Int.compare(keys[a], keys[b]) })`: comparison sort multiplies PVec random reads by `n log n`.

---

## Baseline metrics to track

Run from repository root after `target/twk` is fresh:

```bash
target/twk run examples/dataframe/bench/order_by_micro.tw | tee /tmp/dataframe-orderby-micro.txt
target/twk run examples/dataframe/bench/main.tw | tee /tmp/dataframe-bench.txt
rustc -O examples/dataframe/bench/order_by_rust.rs -o /tmp/order_by_rust
/tmp/order_by_rust | tee /tmp/order_by-rust.txt
```

### Twinkle order-by microbenchmarks

`examples/dataframe/bench/order_by_micro.tw` isolates three sort shapes:

```
── N = 10000 ──
sort values : 9.41ms    (checksum 10000)
sort idx id : 0.32ms    (checksum 10000)
sort idx key: 8.65ms    (checksum 10000)

── N = 100000 ──
sort values : 64.13ms   (checksum 100000)
sort idx id : 4.31ms    (checksum 100000)
sort idx key: 99.95ms   (checksum 100000)

── N = 1000000 ──
sort values : 828.89ms  (checksum 1000000)
sort idx id : 28.74ms   (checksum 1000000)
sort idx key: 1674.20ms (checksum 1000000)
```

Interpretation:

- `sort idx id` is cheap because the prelude sort detects already-ascending input and returns early.
- `sort values` measures prelude merge sort over a `Vector<Int>` value vector.
- `sort idx key` isolates the dataframe comparator shape: repeated random reads from a key PVec during index sorting. This is the primary target.

### Dataframe end-to-end benchmark

Latest dataframe benchmark after `Vector.gather` and dtype-specialized `order_by` comparator:

```
── N = 10000 ──
filter      : 2.13ms    (checksum 4912)
order_by    : 12.06ms   (checksum 10000)
group_by/agg: 4.75ms    (checksum 64)
join        : 6.40ms    (checksum 8597)

── N = 100000 ──
filter      : 17.83ms   (checksum 49735)
order_by    : 147.35ms  (checksum 100000)
group_by/agg: 27.60ms   (checksum 64)
join        : 85.41ms   (checksum 78120)

── N = 1000000 ──
filter      : 209.49ms  (checksum 498802)
order_by    : 2530.79ms (checksum 1000000)
group_by/agg: 337.20ms  (checksum 64)
join        : 1481.51ms (checksum 937500)
```

### Rust reference, same generated data

`examples/dataframe/bench/order_by_rust.rs` provides a reference ceiling, not a Twinkle ceiling:

```
N=10000    native total: 0.72ms   sort: 0.26ms   gather: 0.44ms
N=10000    merge  total: 2.08ms   sort: 1.40ms   gather: 0.68ms
N=100000   native total: 4.54ms   sort: 1.61ms   gather: 2.85ms
N=100000   merge  total: 15.57ms  sort: 13.81ms  gather: 1.73ms
N=1000000  native total: 57.63ms  sort: 8.78ms   gather: 48.35ms
N=1000000  merge  total: 133.24ms sort: 84.11ms  gather: 48.34ms
```

This shows the workload itself can be far faster when the sort operates on dense native memory. The gap is current implementation overhead: high-level prelude merge sort over persistent vectors, closure comparator calls, and repeated PVec random reads.

---

## Proposed API surface

Start with a runtime builtin hidden behind a prelude method on `Vector<Int>` or a dataframe-internal helper.

Preferred first API:

```tw
// returns row indices [0, keys.len()) sorted by keys[row]
pub fn sort_indices(keys: Vector<Int>) Vector<Int>
```

If direction is needed in the primitive:

```tw
pub fn sort_indices_by_int_key(keys: Vector<Int>, descending: Bool) Vector<Int>
```

For dataframe use, null handling is required. Two viable shapes:

```tw
pub fn sort_indices_by_int_key(keys: Vector<Int>, nulls: Vector<Bool>, descending: Bool) Vector<Int>
```

or keep null handling in dataframe by first materializing a non-null index subset. The primitive-with-null-mask is better for avoiding extra passes and for preserving current null ordering exactly.

Semantics for the nullable version:

- result is a `Vector<Int>` of row indices.
- ascending: non-null keys ascending, nulls last.
- descending: nulls first, non-null keys descending. This matches the current `order_by` behavior after applying descending order to the whole comparison result.
- equal keys may be unstable unless documented otherwise. Current `sort_by` merge sort is stable-ish by left preference; dataframe tests should not rely on tie stability unless we commit to it.

---

## Runtime design

### Data representation

For an `Int` key column:

- input `keys`: PVec of boxed `Int` values (`BoxedInt` anyrefs)
- input `nulls`: PVec of Bool values (`i31ref`)
- temporary working array: Wasm GC `Array` of records or tuple-like fields

Two representation options:

1. **Pair record array**
   ```text
   SortEntry = struct { key: i64, row: i32, is_null: i32 }
   entries: array<SortEntry>
   ```
   Pros: one array, comparator reads one object per side. Easy to extend to Float/String with different entry structs.  
   Cons: allocates one struct per row.

2. **Parallel arrays**
   ```text
   keys_buf: array<anyref or boxed i64>
   rows_buf: array<i31ref row>
   nulls_buf: array<i31ref bool>
   ```
   Pros: fewer per-row struct allocations if rows/nulls are i31; easier in-place swaps per field.  
   Cons: more array accesses and swap bookkeeping.

Start with the simpler representation that is easiest to implement correctly in the existing runtime DSL. If per-row structs are too expensive, switch to parallel arrays.

### Algorithm choice

Start with an in-place comparison sort over the dense working set:

- introsort if feasible;
- otherwise iterative quicksort with insertion-sort cutoff;
- heapsort fallback optional for v1 if recursion/stack handling is awkward.

The first win should come from eliminating repeated PVec key reads and Twinkle closure calls, not from choosing the perfect sort.

For `Int` keys, a later radix sort can be much faster and avoids comparator overhead entirely. The benchmark's `amount` key has small cardinality, so counting/radix would be a major win, but implement comparison sort first unless radix is straightforward.

### Output

After sorting the working set, build a `Vector<Int>` of sorted row indices using the existing vector builder:

```text
builder = builder_new()
for entry in sorted_entries:
  builder_push(builder, boxed_int(entry.row))
return builder_freeze(builder)
```

The dataframe then calls `take(sorted_idx)`, which routes through `Vector.gather`.

---

## Implementation phases

### Phase 1 — Add benchmarks and lock current behavior

Status: done for the standalone microbench files. Keep them as regression/perf tracking tools.

Files:

- `examples/dataframe/bench/order_by_micro.tw`
- `examples/dataframe/bench/order_by_rust.rs`
- `docs/plans/dataframe-friction-log.md`

Before runtime work, add a dataframe test that covers null order and tie behavior expectations. If tie stability is not guaranteed, avoid asserting it.

### Phase 2 — Boot runtime primitive for nullable Int-key index sort

Files:

- `boot/prelude/signatures/vector.tw`
- `boot/compiler/builtins.tw`
- `boot/compiler/codegen/runtime/arr.tw`
- `boot/tests/suites/api_vector_suite.tw`

Add a runtime builtin, tentatively:

```tw
pub fn sort_indices_by_int_key(keys: Vector<Int>, nulls: Vector<Bool>, descending: Bool) Vector<Int> {
  keys
}
```

The stub return is irrelevant because the runtime mapping replaces it. If the signature source rejects returning `keys` due to type mismatch, use a small placeholder construction accepted by the checker.

Runtime implementation outline:

1. `n = len(keys)` and validate `len(nulls) == n` if runtime helpers make this cheap; otherwise rely on caller invariant initially.
2. Allocate/fill dense working set from PVecs in one pass.
3. Sort working set by `(is_null, key)` with direction semantics.
4. Build and return sorted row-index PVec.

Test cases:

- basic reorder by int key.
- duplicates.
- already ascending.
- descending.
- nulls last ascending / first descending.
- crosses trie boundaries.

### Phase 3 — Stage0 parity

Mirror the boot runtime in Rust stage0.

Files likely touched:

- `src/runtime/arr.rs`
- `src/types/env.rs`
- `src/codegen/prelude.rs`
- `src/intrinsics/registry.rs`
- `src/intrinsics/signatures.rs`
- `src/intrinsics/contracts.rs`
- `src/ir/lower.rs`

Use the existing `Vector.gather` and `Vector.drop_last` entries as the wiring template.

### Phase 4 — Route dataframe `order_by` through the primitive for Int columns

Files:

- `examples/dataframe/frame/table.tw`
- `examples/dataframe/tests/query_suite.tw`
- `docs/plans/dataframe-friction-log.md`

In `sort_indices_by_column`, route only `.IntCol(keys)` through the primitive. Keep Float/String/Bool on the current specialized comparator until their own primitives exist.

Expected outcome: the `amount` benchmark should move substantially if the primitive avoids comparator PVec reads and Twinkle closure calls. If it does not, inspect the generated/runtime sort implementation before broadening to other types.

### Phase 5 — Extend beyond Int if justified

Possible follow-ups:

- Bool key sort: trivial rank-based sort.
- Float key sort: dense working set with `f64` compare semantics matching `Float.compare`.
- String key sort: harder; string comparisons still cost, but dense rows avoid PVec random reads.
- General `Vector.sort_by` runtime primitive: broader language benefit, but harder because arbitrary comparator closures still cross the Wasm call boundary.

---

## Success criteria

Track the same commands from the baseline section. Primary success metric:

- `examples/dataframe/bench/order_by_micro.tw`, `sort idx key` at `N = 1000000` should drop substantially from the current ~1.67s.

Secondary metric:

- `examples/dataframe/bench/main.tw`, `order_by` at `N = 1000000` should drop from the current ~2.5s range.

Guardrails:

- `filter`, `join`, and `group_by` should not regress materially.
- dataframe tests should preserve current null ordering.
- boot and Rust test suites should pass.

---

## Notes on trie-aware gather

A naive contiguous-run gather optimization was attempted and rejected because it added run-detection overhead without helping the current filter benchmark: filter keeps a monotonic but sparse subset, not long contiguous runs. A real trie-aware gather should either cache the current source leaf for monotonic sparse indices or traverse source leaves and index leaves together. That is separate from this sort plan and should be designed with its own microbenchmarks.
