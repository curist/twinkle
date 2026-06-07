# Make idiomatic Twinkle sorting fast with native dense working sets — Implementation Plan

**Goal:** Idiomatic Twinkle collection code should be fast enough without dataframe/user code reaching for bespoke runtime escape hatches. The motivating hotspot is dataframe `order_by`, written naturally as “sort row indices by a key column, then gather rows”. Today that idiomatic shape stays as high-level prelude merge sort over persistent vectors and pays closure calls plus repeated PVec random reads inside the comparator. The real fix is to make the compiler/runtime route hot sort patterns through native dense working sets while preserving the public `Vector<T>` value model.

**Principle:** User code should remain idiomatic:

```tw
idx.sort_by(fn(a, b) { Int.compare(keys[a], keys[b]) })
```

or dataframe-level:

```tw
t.order_by("amount", Dir.Asc)
```

The optimizer/runtime should make those shapes fast. A narrow `sort_indices_by_int_key(...)` helper is acceptable as an internal lowering target, but not as the main user-facing solution.

**Non-goal:** This is not a replacement for persistent `Vector<T>` as the language collection. Persistent vectors remain the public value model. Sort kernels may use temporary mutable Wasm arrays internally, then freeze back to persistent vectors.

**Current conclusion:** `Vector.gather` and typed dataframe gather cleanup helped join and made the API cleaner, but `order_by` remains dominated by sorting. Microbenchmarks show the expensive path is `idx.sort_by(fn(a, b) { Int.compare(keys[a], keys[b]) })`: comparison sort multiplies PVec random reads by `n log n`. Native-language references show the workload itself is far cheaper when sort works over dense memory, so the current ~2.5s dataframe `order_by` is implementation overhead, not a Twinkle ceiling.

**Companion representation plan:** [typed-vector-representation.md](typed-vector-representation.md) is the broader monomorphization/physical-representation fix. This sort plan can materialize dense typed working sets inside kernels; the typed-vector plan makes `Vector<Int>` access itself avoid boxed `anyref` reads where possible.

---

## Baseline metrics to track

Run from repository root after `target/twk` is fresh:

```bash
target/twk run examples/dataframe/bench/order_by_micro.tw | tee /tmp/dataframe-orderby-micro.txt
target/twk run examples/dataframe/bench/main.tw | tee /tmp/dataframe-bench.txt
rustc -O examples/dataframe/bench/order_by_rust.rs -o /tmp/order_by_rust
/tmp/order_by_rust | tee /tmp/order_by-rust.txt
go run examples/dataframe/bench/order_by_go.go | tee /tmp/order_by-go.txt
clojure examples/dataframe/bench/order_by_clojure.clj | tee /tmp/order_by-clojure.txt
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

### Dataframe `order_by` breakdown

`examples/dataframe/bench/order_by_breakdown.tw` breaks the current `order_by("amount", Asc)` path into generation, index construction, key-index sorting, null-aware key-index sorting, gather, and full `order_by`.

Latest run:

```
── N = 10000 ──
generate table      : 6.71ms
build idx           : 0.28ms     (checksum 10000)
sort idx by amount  : 9.35ms     (checksum 10000)
sort idx + nulls    : 8.78ms     (checksum 10000)
gather amount vector: 0.32ms     (checksum 10000)
gather 3 columns    : 1.77ms     (checksum 30000)
table.take(sorted)  : 2.01ms     (checksum 10000)
full order_by       : 10.72ms    (checksum 10000)

── N = 100000 ──
generate table      : 45.86ms
build idx           : 2.11ms     (checksum 100000)
sort idx by amount  : 92.74ms    (checksum 100000)
sort idx + nulls    : 119.03ms   (checksum 100000)
gather amount vector: 4.57ms     (checksum 100000)
gather 3 columns    : 30.36ms    (checksum 300000)
table.take(sorted)  : 28.60ms    (checksum 100000)
full order_by       : 152.73ms   (checksum 100000)

── N = 1000000 ──
generate table      : 462.04ms
build idx           : 27.22ms    (checksum 1000000)
sort idx by amount  : 1620.13ms  (checksum 1000000)
sort idx + nulls    : 2155.01ms  (checksum 1000000)
gather amount vector: 71.69ms    (checksum 1000000)
gather 3 columns    : 414.06ms   (checksum 3000000)
table.take(sorted)  : 416.34ms   (checksum 1000000)
full order_by       : 2652.56ms  (checksum 1000000)
```

At `N = 1000000`, the full path is dominated by null-aware index sorting (~2.16s) plus final table gather (~0.42s). The difference between `sort idx by amount` and `sort idx + nulls` shows that even an all-non-null column pays heavily for per-comparison null-mask checks. The next useful primitive should therefore avoid both repeated PVec key reads and repeated PVec null-mask reads inside the comparator.

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

### Native-language references, same generated data

`examples/dataframe/bench/order_by_rust.rs`, `order_by_go.go`, and `order_by_clojure.clj`
provide reference points, not a Twinkle ceiling.

Rust:

```
N=10000    native total: 0.72ms   sort: 0.26ms   gather: 0.44ms
N=10000    merge  total: 2.08ms   sort: 1.40ms   gather: 0.68ms
N=100000   native total: 4.54ms   sort: 1.61ms   gather: 2.85ms
N=100000   merge  total: 15.57ms  sort: 13.81ms  gather: 1.73ms
N=1000000  native total: 57.63ms  sort: 8.78ms   gather: 48.35ms
N=1000000  merge  total: 133.24ms sort: 84.11ms  gather: 48.34ms
```

Go (`sort.Slice` over row indices):

```
N=10000    go total: 1.48ms   sort: 1.33ms   gather: 0.13ms
N=100000   go total: 15.37ms  sort: 12.75ms  gather: 2.46ms
N=1000000  go total: 77.21ms  sort: 59.33ms  gather: 17.27ms
```

Clojure/JVM (`Arrays.sort` over boxed row indices; includes cold JVM/script overhead):

```
N=10000    clj total: 7.84ms    sort: 4.25ms    gather: 3.15ms
N=100000   clj total: 42.76ms   sort: 14.51ms   gather: 25.66ms
N=1000000  clj total: 244.66ms  sort: 119.34ms  gather: 120.57ms
```

These show the workload itself can be far faster when the sort operates on dense native memory. The gap is current implementation overhead: high-level prelude merge sort over persistent vectors, closure comparator calls, and repeated PVec random reads.

---

## Desired end-state

### User-facing surface

Prefer keeping the source surface ordinary and broad:

```tw
xs.sort()
xs.sort_by(fn(a, b) { ... })
idx.sort_by(fn(a, b) { Int.compare(keys[a], keys[b]) })
t.order_by("amount", Dir.Asc)
```

The implementation may introduce internal runtime intrinsics, but users should not need to pick a special “fast” API for common cases. If a public API is added, it should be broadly useful and semantic, such as an `argsort`/`sort_by_key` convenience, not a dataframe-specific escape hatch.

Potential public additions, after the internals prove out:

```tw
keys.argsort()                         // Vector<Int> of row indices
keys.argsort_by(dir: SortDir)          // direction-aware
keys.argsort_nulls(nulls, dir, nulls)  // explicit null policy
```

These are optional conveniences. They should lower to the same runtime kernels as idiomatic `sort_by` shapes where possible.

### Compiler/runtime strategy

Layer the fix so generic idioms improve over time:

1. **Make `Vector.sort` / common `Vector.sort_by` cases runtime-native.**
   The current prelude merge sort is valuable as a simple spec, but hot execution should use a runtime kernel with a dense mutable working set. This complements [typed-vector-representation.md](typed-vector-representation.md): native sort reduces hot algorithm overhead now; typed vectors reduce boxed read overhead at the representation level.
2. **Recognize common comparator shapes.**
   In particular, `idx.sort_by(fn(a, b) { Int.compare(keys[a], keys[b]) })` should lower to an internal typed argsort kernel instead of calling a closure for every comparison.
3. **Keep ordered/reverse detection outside the heavy kernel.**
   The existing pre-scan for already-ordered input is useful and simple in Twinkle. The native kernel can assume it is sorting a genuinely unsorted working set.
4. **Use internal intrinsics as lowering targets, not as required user APIs.**
   An internal `vector$argsort_int_key(keys, nulls, descending)` is fine if produced by dataframe/table lowering or optimizer recognition. It is not the desired programming model.

---

## Runtime design

### Dense working-set representation

For the first typed key path (`Int` keys):

- input `keys`: PVec of boxed `Int` values (`BoxedInt` anyrefs)
- optional input `nulls`: PVec of Bool values (`i31ref`)
- temporary working set: Wasm GC arrays holding dense keys, row ids, and null flags

Two representation options:

1. **Pair/entry record array**
   ```text
   SortEntry = struct { key: i64, row: i32, is_null: i32 }
   entries: array<SortEntry>
   ```
   Pros: one logical item to swap; straightforward comparator.
   Cons: one struct allocation per row.

2. **Parallel arrays**
   ```text
   keys_buf: array<i64-like boxed values or typed i64 storage when available>
   rows_buf: array<i31ref row>
   nulls_buf: array<i31ref bool>
   ```
   Pros: fewer per-row object allocations; closer to native array-sort layout.
   Cons: swaps touch multiple arrays; more bookkeeping.

Pick the representation that is easiest to implement correctly in the current runtime DSL, then measure. If entry records allocate too much, switch to parallel arrays.

### Algorithm choice

Start with an in-place comparison sort over the dense working set:

- iterative quicksort/introsort if feasible;
- insertion sort cutoff for small partitions;
- heapsort fallback optional if worst-case behavior matters.

A branchless or branch-reduced partition can be explored after the dense working set is in place, but it should not be the first success criterion. The first win should come from eliminating closure calls and repeated PVec key/null-mask reads.

For `Int` keys, a later radix/counting strategy may beat comparison sort. The benchmark's `amount` key has small cardinality, so counting sort would be especially strong, but implement the general dense comparison path first unless radix is straightforward.

### Output

After sorting the working set, build a `Vector<Int>` of sorted row indices using the existing vector builder:

```text
builder = builder_new()
for row in sorted_rows:
  builder_push(builder, boxed_int(row))
return builder_freeze(builder)
```

Dataframe `take(sorted_idx)` then uses the normal gather path.

## Implementation phases

### Phase 1 — Benchmarks and behavior locks

Status: started. Keep these as regression/perf tracking tools and extend them as new kernels land.

Files:

- `examples/dataframe/bench/order_by_micro.tw`
- `examples/dataframe/bench/order_by_breakdown.tw`
- `examples/dataframe/bench/order_by_rust.rs`
- `examples/dataframe/bench/order_by_go.go`
- `examples/dataframe/bench/order_by_clojure.clj`
- `docs/plans/dataframe-friction-log.md`

Before runtime work, add/keep dataframe tests that cover null order and duplicate-key behavior. Avoid relying on tie stability unless the native sort commits to stable output.

### Phase 2 — Runtime-native `Vector.sort` / typed `Int` sort kernel

First improve an idiomatic language primitive rather than dataframe code. Add a runtime implementation for sorting `Vector<Int>` values over a dense temporary working set, then route `Vector.sort<Int>` or the relevant intrinsic path to it.

Why first:

- It attacks `sort values`, currently ~829ms at `N = 1000000`.
- It validates dense working-set sort mechanics without closure callbacks or key-index pattern recognition.
- It is a general language improvement, not a dataframe-only workaround.

Files likely touched:

- `boot/prelude/signatures/vector.tw`
- `boot/compiler/builtins.tw`
- `boot/compiler/codegen/runtime/arr.tw`
- `boot/tests/suites/api_vector_suite.tw`
- stage0 parity files under `src/`

Tests:

- basic sort, duplicates, negative/positive ints.
- already ascending / descending behavior.
- crosses trie boundaries.
- preserves existing `Ord` semantics for `Int`.

### Phase 3 — Native generic `Vector.sort_by` working set, if callback overhead is acceptable

Implement a runtime-backed sort that flattens the input vector once, sorts a dense array, and freezes back to PVec, while still calling the comparator callback.

This will not eliminate callback cost, but it can remove high-level Twinkle recursive merge-sort overhead and repeated PVec reads for sorting `xs` itself. Measure before and after; if callback crossing dominates, keep this as an internal stepping stone and focus on comparator-shape specialization.

### Phase 4 — Optimizer/lowering recognition for key-index comparators

Recognize the idiomatic key-index shape:

```tw
idx.sort_by(fn(a, b) { Int.compare(keys[a], keys[b]) })
```

and lower it to an internal typed argsort kernel over dense key/null buffers. This is the real fix for dataframe `order_by` while preserving idiomatic source.

Recognition can start narrow:

- receiver is a `Vector<Int>` index vector;
- comparator parameters are used only as indices into the same `Vector<Int>` key vector;
- comparison is `Int.compare(keys[a], keys[b])` or the descending reversal equivalent;
- optional null-mask checks match the dataframe pattern.

Internal lowering target, not necessarily public API:

```tw
vector$argsort_int_key(keys: Vector<Int>, nulls: Vector<Bool>, descending: Bool) Vector<Int>
```

The native kernel should assume the caller has already handled cheap ordered/reverse detection if desired.

### Phase 5 — Route dataframe `order_by` without changing user ergonomics

Keep dataframe source high-level. Either:

1. write `order_by` in an idiomatic shape that the optimizer recognizes; or
2. use a private/internal helper only inside the dataframe implementation while preserving the public `t.order_by(...)` API.

Do not require dataframe users to call a special fast primitive. The performance win should be visible through normal `order_by`.

### Phase 6 — Extend typed key paths

After `Int` is proven:

- Bool key sort: rank-based dense sort.
- Float key sort: dense `f64` compare semantics matching `Float.compare`.
- String key sort: dense row/key storage still avoids PVec random reads, though string comparison remains costly.
- Optional public `argsort`/`sort_by_key` convenience APIs if they feel generally useful.

## Success criteria

Track the same commands from the baseline section.

Primary language-level success metrics:

- `examples/dataframe/bench/order_by_micro.tw`, `sort values` at `N = 1000000` should drop substantially from the current ~829ms once `Vector.sort<Int>` is runtime-native.
- `sort idx key` at `N = 1000000` should drop substantially from the current ~1.67s once key-index comparator recognition or an equivalent internal lowering lands.

Dataframe success metric:

- `examples/dataframe/bench/main.tw`, `order_by` at `N = 1000000` should drop from the current ~2.5s range without changing the public dataframe API or asking users to call a special fast path.

Guardrails:

- Idiomatic `Vector.sort`, `Vector.sort_by`, and dataframe `order_by` remain the user-facing APIs.
- `filter`, `join`, and `group_by` should not regress materially.
- dataframe tests should preserve current null ordering.
- boot and Rust test suites should pass.

---

## Notes on trie-aware gather

A naive contiguous-run gather optimization was attempted and rejected because it added run-detection overhead without helping the current filter benchmark: filter keeps a monotonic but sparse subset, not long contiguous runs. A real trie-aware gather should either cache the current source leaf for monotonic sparse indices or traverse source leaves and index leaves together. That is separate from this sort plan and should be designed with its own microbenchmarks.
