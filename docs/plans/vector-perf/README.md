# Vector / sort / order-by performance — endeavor index

This folder gathers the plans, rejected approaches, and measurements for one
long-running effort: making **idiomatic `Vector<T>` code — indexed reads,
`sort_by`, dataframe `order_by` — fast**, without asking users to reach for
specialized APIs.

It is a dedicated subfolder because this is a major, still-open problem. Several
distinct approaches have been tried and measured; most isolated wins are small,
and the real lever is structural. Keep new plans, probes, and results here.

## Current understanding (2026-06-09)

The realistic dataframe path — `idx.sort_by(fn(a,b){ Int.compare(keys[a], keys[b]) })`
— is **~7× slower than Clojure's persistent-vector sort** (~2.37 s vs ~0.34 s at
N = 1M). Measurement (see [generic-sort-by-vector-read-perf.md](generic-sort-by-vector-read-perf.md),
"Measured decomposition") attributes the gap:

- **Reads dominate everywhere.** ~69% of the key-index path is `keys[…]` reads
  (~1624 ms of random PVec lookups). Even the no-key-read merge baseline (~720 ms)
  is ~79% reads + compares + recursion, not allocation.
- **Allocation is a minor lever.** Singleton `[xs[lo]]` vectors are negligible
  (~10 ms); append + output-vector allocation is ~150 ms (~6.5% of the path). A
  flat-buffer merge over *persistent* storage is therefore not worth shipping alone.
- **Comparator micro-opts are small.** Closure boundary (~12%) + enum/`Order`
  allocation (~9%) ≈ ~6% of the gap combined.
- **Clojure does not cache keys either** — it re-invokes the key fn per comparison
  and sorts a flat array. So the gap is constant-factor/structural, and transparent
  argsort recognition is *not* required to close it.

**Master lever:** typed flat `Vector<Int>` storage. It makes random key reads cheap
*and* enables a native-buffer merge (cheap sequential reads + no per-level
allocation) in one change. Everything else is secondary.

## Plans in this folder

| Doc | Role | Status |
|-----|------|--------|
| [generic-sort-by-vector-read-perf.md](generic-sort-by-vector-read-perf.md) | **Active lead.** Make generic callback `sort_by` + indexed reads fast; holds the current measured decomposition and reprioritized tracks | active |
| [typed-vector-representation.md](typed-vector-representation.md) | Give `Vector<Int>` (then other primitives) typed physical storage instead of boxed `anyref` leaves — now identified as the master lever | the long-term answer |
| [wasm-native-sort.md](wasm-native-sort.md) | Earlier consolidated `order_by`/native-sort track; broader context and the dense working-set framing | superseded as lead, still useful context |
| [native-typed-value-sort.md](native-typed-value-sort.md) | Lower `xs.sort()` on `Vector<Int>`/`Vector<Float>` to a native typed kernel (unbox once, raw merge, box once); the seed of typed storage | partial/landed value-sort path |
| [native-key-index-argsort.md](native-key-index-argsort.md) | Optional transparent fast path for conservatively-recognized pure key-index comparators | optional, not the baseline |
| [native-sort-by-inplace.md](native-sort-by-inplace.md) | Approach A: in-place quicksort over a uniquely-owned buffer | **rejected** |
| [native-sort-dense-merge.md](native-sort-dense-merge.md) | Approach C: dense `anyref` scratch merge sort; lost to opaque per-element scratch calls + casts | **rejected** |

## Probes (in `examples/`)

- `examples/sort-bench/sort_by_component_probe.tw` — clean component breakdown (sort, closure, reads, append).
- `examples/sort-bench/enum_alloc_probe.tw` — isolates enum/`Order` allocation (direct vs closure boundary; enums in general).
- `examples/sort-bench/merge_attribution_probe.tw` — ablates the merge (reads vs singleton vs append/alloc); validated against real `sort_by`.
- `examples/dataframe/bench/` — end-to-end `order_by` plus Clojure/Go/Rust references.

## Benchmark gate

```bash
target/twk run examples/sort-bench/sort_by_component_probe.tw
target/twk run examples/sort-bench/merge_attribution_probe.tw
target/twk run examples/dataframe/bench/order_by_breakdown.tw
clojure examples/dataframe/bench/order_by_clojure_persistent.clj
```

## Lessons banked

- Don't re-try opaque dense scratch (Approach C): per-element runtime calls + `anyref`
  casts outweigh the merge savings. Any dense/flat buffer must use **inlined** array ops.
- Don't chase comparator micro-opts for parity; they cap at ~6% combined.
- Don't ship a persistent-only flat-buffer merge; the allocation-only saving is ~6.5%.
- Do measure before prioritizing — two confident structural guesses (singleton
  allocation cost, flat-buffer merge value) were falsified by probes here.
