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

Implemented for generic `Vector.sort_by`: copy the input into a flat mutable Wasm-GC scratch array, perform a stable bottom-up merge over dense buffers, and freeze back to `Vector`. It is correct and stable.

Result: **measured neutral-to-negative; did not pass the gate.** In a controlled same-machine A/B against the pre-C recursive merge sort, the dense path *regressed* the pure `Vector<Int>` sort (~808 ms → ~935 ms at N = 1M) and was flat on both index-key sort and dataframe `order_by`. The double copy in/out and per-element un-inlined `scratch_get`/`scratch_set` runtime calls cost more than the old PVec-builder merge saved, and on `order_by` the opaque comparator's persistent-vector key/null reads still dominate — so changing the merge mechanics is neutral. This confirms (like Approach A) that generic-sort mechanics are not the lever.

The reusable part is the `Scratch<T>` dense-buffer infrastructure (an opaque mutable Wasm-GC array with `scratch_new`/`get`/`set` in both compilers), which is a building block for the dense key-index argsort kernel below — not the `sort_by` rewrite itself.

See: [native-sort-dense-merge.md](native-sort-dense-merge.md).

### Native typed value-sort kernel — first dense kernel that won

`xs.sort()` on `Vector<Int>` and `Vector<Float>` now lowers to a native typed-array
merge kernel: the input is materialized once into a dense `i64`/`f64` buffer, sorted by
a stable merge whose comparisons and moves are pure unboxed `i64`/`f64` inside a single
runtime function, and frozen back to a persistent `Vector`. Implemented in both the boot
compiler and the Rust stage0 reference.

This is the first dense-typed sort kernel to actually pass the gate, and it does so
precisely where the two earlier dense attempts failed. Approach A (in-place quicksort
over a uniquely-owned vector) lost because the in-place writes degraded to persistent
copy-on-write. Approach C (a generic stable merge over an *opaque* `anyref` `Scratch<T>`)
lost because every element touch paid an `anyref` cast plus an un-inlined
`scratch_get`/`scratch_set` call, and the comparator stayed boxed. The difference here is
*typed and inlined*: a monomorphic `i64`/`f64` dense buffer with inlined element access
and no per-comparison closure, so the merge is straight-line numeric work.

Measured same-machine (Apple silicon, `date.now()` microbench in
`examples/sort-bench/value_sort_micro.tw`; Twinkle COLD single-run via `target/twk run`;
Clojure persistent-vector via `examples/sort-bench/value_sort_clojure.clj`, WARMED):

| N | Twinkle `Vector<Int>.sort()` | Twinkle `Vector<Float>.sort()` | Clojure pvec `(vec (sort v))` |
|---:|---:|---:|---:|
| 10,000 | ~1.4 ms | ~1.5 ms | ~0.7 ms |
| 100,000 | ~9.0 ms | ~10.2 ms | ~11 ms |
| 1,000,000 | ~115 ms | ~118 ms | ~172 ms |

The headline `Vector<Int>` value sort at N = 1M dropped from the documented ~808 ms
generic-merge baseline to ~115 ms — about a 7× improvement — and now **beats** the
Clojure persistent-vector reference (~172 ms warmed on this machine), comfortably under
the ~200 ms target. The ~115 ms is the **cold first-run** number: repeating the sort
in-process (`examples/sort-bench/sort_repeat_probe.tw`, 2026-06-10) shows it settle at
**~58–63 ms** once V8 tiers the kernel up from Liftoff to TurboFan. This is the only
sort path with a material tiering effect — the generic and key-index `sort_by` numbers
are stable across in-process runs and under forced
`--v8-flags=--no-liftoff,--no-wasm-lazy-compilation`. `Vector<Float>` tracks the same curve. (Inputs are identical across
implementations: both report first `0` / last `999998` at N = 1M.) This validates the
dense-typed-kernel direction: the win comes from typed, inlined element access, not from
the merge mechanics that A and C also used.

Note this is the *plain value sort* — keys are sorted directly. The remaining lever for
the dataframe `order_by` path is the key-index argsort (below), where the comparator
reads persistent-vector keys/null masks by index; that has not yet been moved to a dense
typed working set.

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

The immediate implementation plan is [generic-sort-by-vector-read-perf.md](generic-sort-by-vector-read-perf.md): improve generic `sort_by` callback execution and indexed vector reads first, so idiomatic callbacks remain competitive even when they have observable side effects. [native-key-index-argsort.md](native-key-index-argsort.md) remains an optional transparent fast path for recognized pure key-index comparators, not the baseline performance story.

See also: [typed-vector-representation.md](typed-vector-representation.md).

## Bench commands

Keep the benchmark set small and focused:

```bash
target/twk run examples/sort-bench/value_sort_micro.tw
clojure examples/sort-bench/value_sort_clojure.clj

target/twk run examples/dataframe/bench/order_by_micro.tw
target/twk run examples/dataframe/bench/order_by_breakdown.tw
target/twk run examples/dataframe/bench/main.tw

clojure examples/dataframe/bench/order_by_clojure_persistent.clj
clojure examples/dataframe/bench/order_by_clojure.clj
go run examples/dataframe/bench/order_by_go.go
rustc -O examples/dataframe/bench/order_by_rust.rs -o /tmp/order_by_rust && /tmp/order_by_rust
```

Use `order_by_breakdown.tw` to understand component scale, not as an additive profiler. Use the Clojure persistent-vector benchmark as the most relevant external sanity check for Twinkle's persistent collection model.
