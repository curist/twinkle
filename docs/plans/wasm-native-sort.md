# Order-by and native sort performance — consolidated plan

**Status:** active performance track. Keep user-facing dataframe and collection code idiomatic, but lower hot sort/order-by shapes to dense runtime work where needed.

## Goal

Twinkle users should be able to write normal code:

```tw
t.order_by("amount", Dir.Asc)
idx.sort_by(fn(a, b) { Int.compare(keys[a], keys[b]) })
xs.sort_by(cmp)
```

and get performance in the same broad class as other FP/native systems. The public `Vector<T>` remains persistent; the compiler/runtime may use temporary dense mutable buffers inside hot kernels and then freeze back to persistent values.

The main benchmark is the dataframe `order_by("amount", Asc)` path over generated columns. This is intentionally a dataframe workload: sort row ids by a column, then gather all columns by those row ids.

## Current performance signal

The relevant baseline is not merely "better than Twinkle's old implementation". A Clojure version using ordinary persistent vectors is much faster than Twinkle today, so the realistic target is to close that implementation gap.

Representative N = 1,000,000 numbers:

| implementation | total | sort | gather | note |
|---|---:|---:|---:|---|
| Twinkle dataframe `order_by` | ~2.5s | ~2.1s null-aware index sort | ~0.4s table gather | current order of magnitude |
| Clojure persistent-vector version | ~0.38s | ~0.33s | ~0.05s | `sort-by` over row ids, `nth` into persistent vectors |
| Clojure/JVM array version | ~0.25s | ~0.12s | ~0.12s | dense primitive/object arrays |
| Go slice version | ~0.08–0.10s | ~0.06–0.08s | ~0.02s | dense slices |
| Rust dense version | ~0.06–0.16s | ~0.01–0.09s | ~0.05–0.06s | dense vectors; varies by native/merge variant |

Use these as direction-setting references, not exact pass/fail thresholds. The Clojure persistent-vector result is the most important comparator for Twinkle's public collection model: persistent vectors alone do not explain a multi-second result.

## What the existing breakdown really says

`examples/dataframe/bench/order_by_breakdown.tw` is a set of component probes, not an additive profiler for a single `order_by` invocation. The useful mental model for current `order_by` is:

```text
order_by ≈ build idx + null-aware sort of idx + table.take(sorted)
         ≈ tens of ms + ~2s sort + ~0.4s gather, at N = 1M
```

The sort dominates. The hot comparator repeatedly reads:

```tw
col.nulls[a]
col.nulls[b]
keys[a]
keys[b]
```

inside an O(n log n) comparison sort. Each read goes through Twinkle's current `Vector` representation and each comparison also pays closure/comparator overhead.

## Attempts so far

### Approach A: in-place sort over a uniquely-owned Vector

Tried replacing prelude merge sort with an in-place quicksort-style algorithm over a fresh vector buffer.

Result: rejected. The expected in-place writes did not materialize across helper/recursive call boundaries; the generated code fell back to persistent copy-on-write writes. That made `order_by` dramatically slower instead of faster. This is not worth digging further unless the uniqueness model changes substantially.

See: [native-sort-by-inplace.md](native-sort-by-inplace.md).

### Approach C: dense scratch-buffer stable merge sort

Current direction for generic `Vector.sort_by`: copy the input into a flat mutable Wasm-GC scratch array, perform a stable bottom-up merge over dense buffers, and freeze back to `Vector`.

This is architecturally sound because it removes two real costs from generic `sort_by`:

- persistent-vector allocation at every merge level;
- persistent-vector writes during the sort body.

But expectations should be modest for dataframe `order_by`, because Approach C keeps the opaque comparator closure. It does not remove the repeated `keys[a]` / `nulls[a]` persistent-vector reads inside every comparison. Treat it as a foundation and correctness-preserving generic sort improvement, not the final dataframe performance lever.

See: [native-sort-dense-merge.md](native-sort-dense-merge.md).

## Main vector to attack

The highest-value vector is **key-index sorting**:

```tw
idx.sort_by(fn(a, b) { Int.compare(keys[a], keys[b]) })
```

and the nullable dataframe variant:

```tw
idx.sort_by(fn(a, b) {
  // compare null rank first, then key
})
```

This shape is common, idiomatic, and much easier to optimize than arbitrary comparator code. The user-facing API should stay the same; the implementation should recognize or internally route this shape to a dense key-sort/argsort kernel.

## End-game direction

### 1. Keep idiomatic APIs

Do not ask users to call a dataframe-specific escape hatch for normal order-by. Keep:

```tw
t.order_by("amount", Dir.Asc)
idx.sort_by(fn(a, b) { Int.compare(keys[a], keys[b]) })
```

A semantic API such as `sort_by_key` / `argsort` may be useful later, but it should be ergonomic and broadly useful, not a performance-only workaround.

### 2. Lower key-index sorts to dense working sets

For primitive dataframe columns, internally materialize once:

```text
row ids     -> dense Int row-id buffer
keys        -> dense typed key buffer
null ranks  -> dense Bool/Int null-order buffer
```

Then sort row ids against those dense buffers. This avoids repeated persistent-vector traversals and null-mask reads inside every comparison.

The first practical target should be Int keys with null ordering, because it covers the current `amount` benchmark and validates the architecture. Extend to Float, Bool, and String after the path is proven.

### 3. Consider non-comparison sorts for suitable keys

The benchmark's `amount` column has low cardinality. A dense Int argsort can later choose radix/counting strategies when the key range makes that profitable. Start with a general dense comparison path unless counting/radix is straightforward.

### 4. Continue broader representation work

Typed vector representation remains the grand-picture fix:

- primitive `Vector<Int>` / `Vector<Float>` reads should avoid unnecessary boxed `anyref` traffic;
- dense working-set sort kernels are a near-term proof point;
- typed vectors help far beyond sorting: map/filter/fold, dataframe columns, group-by, joins, and numeric workloads.

See: [typed-vector-representation.md](typed-vector-representation.md).

## Bench commands

Keep the benchmark set small and focused:

```bash
target/twk run examples/dataframe/bench/order_by_micro.tw
target/twk run examples/dataframe/bench/order_by_breakdown.tw
target/twk run examples/dataframe/bench/main.tw

clojure examples/dataframe/bench/order_by_clojure_persistent.clj
clojure examples/dataframe/bench/order_by_clojure.clj
go run examples/dataframe/bench/order_by_go.go
rustc -O examples/dataframe/bench/order_by_rust.rs -o /tmp/order_by_rust && /tmp/order_by_rust
```

Use `order_by_breakdown.tw` to understand component scale, not as an additive profiler. Use the Clojure persistent-vector benchmark as the most relevant external sanity check for Twinkle's persistent collection model.
