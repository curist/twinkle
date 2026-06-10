# Vector / sort / order-by performance — endeavor index

This folder gathers the plans, rejected approaches, and measurements for one
long-running effort: making **idiomatic `Vector<T>` code — indexed reads,
`sort_by`, dataframe `order_by` — fast**, without asking users to reach for
specialized APIs.

It is a dedicated subfolder because this is a major, still-open problem. Several
distinct approaches have been tried and measured; most isolated wins are small,
and the real lever is structural. Keep new plans, probes, and results here.

> **Picking up / reviewing the typed-vector work?** Start at
> **[HANDOFF.md](HANDOFF.md)** — current state (S1 + S2.0 landed and working on
> branch `native-typed-value-sort`), commit trail, verify commands, and what's next.

## Current understanding (2026-06-10)

The realistic dataframe path — `idx.sort_by(fn(a,b){ Int.compare(keys[a], keys[b]) })`
— is **~7× slower than Clojure's persistent-vector sort** (~2.27 s vs ~0.34 s at
N = 1M). Measurement (see [generic-sort-by-vector-read-perf.md](generic-sort-by-vector-read-perf.md),
"Measured decomposition" and the 2026-06-10 re-measure) attributes the gap:

- **The gap is structural, not warm-up.** In-process repeats and forced TurboFan
  (`--no-liftoff,--no-wasm-lazy-compilation`) leave the generic (~704–746 ms) and
  key-index (~2240–2280 ms) numbers unchanged. Only the native value-sort kernel
  tier-warms: ~102 ms cold first run → **~58–63 ms** warm (the recorded ~106–115 ms
  figures are cold; warmed-vs-warmed it beats Clojure's ~192 ms by ~3×).
- **Random key reads dominate the key-index path.** ~69% is `keys[…]` reads
  (~1.6 s of random PVec lookups in the cache-hostile ~16 ns/read regime).
- **The merge's own reads were ~half of the mechanics half, now largely removed.**
  The prelude `merge_sorted` caches cursor values and hoists lens (landed 2026-06-10),
  cutting ~3 reads + 2 `len` calls per step to ~1 read: mechanics ~739 → ~647 ms
  (~12%). Merge-context reads cost only ~4–5 ns (small sub-vectors hit the tail
  fast path); the remaining ~650 ms floor is closure calls, `Order` allocation,
  recursion, and append mechanics.
- **Allocation is a minor lever.** Singleton `[xs[lo]]` vectors are negligible
  (~10 ms); append + output-vector allocation is ~150 ms (~6.5% of the path). A
  flat-buffer merge over *persistent* storage is therefore not worth shipping alone.
- **Comparator micro-opts are small vs the key-index gap but a large share of
  the mechanics half.** Closure boundary (~122 ms) + enum/`Order` allocation
  (~68 ms) ≈ ~6% of the 7× gap, but ~30% of the post-cache mechanics floor.
  Both halves are now **landed** (2026-06-10). Enum allocation: payload-free
  variant literals hoisted to shared immutable globals (`Order.Lt`, `.None`
  become one `global.get`, not a per-use `struct.new`); generic `sort_by`
  ~645 → ~610 ms. Closure boundary (T3.2): non-tail closure calls now use the
  typed funcref (unboxed args, no args array, direct result) via a runtime
  `ref.test`, instead of the universal box-everything path; generic `sort_by`
  ~610 → ~495 ms. Together with the merge-cursor cache, generic `sort_by`
  mechanics dropped ~743 → ~495 ms (~33%) on this branch. Comparator mechanics
  are now essentially exhausted — the remaining gap is the read wall.
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
- `examples/sort-bench/sort_repeat_probe.tw` — runs native sort / generic `sort_by` / key-index three times in-process to expose V8 tier-up (Liftoff → TurboFan) effects.
- `examples/sort-bench/ref_vector_read_clojure.clj` — Clojure read calibration for dense `long[]`, boxed persistent `Vector<Long>`, and reference payload vectors (`String`, row objects, map rows); summarized in [typed-vector-representation.md](typed-vector-representation.md).
- `examples/sort-bench/long_array_sort_clojure.clj` — Clojure sort calibration for `long[]` clone + `Arrays/sort` vs persistent-vector `sort`; summarized in [typed-vector-representation.md](typed-vector-representation.md).
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
- Don't chase comparator micro-opts for parity; they cap at ~6% of the key-index gap
  combined (though they are ~30% of the post-cache mechanics floor).
- Don't ship a persistent-only flat-buffer merge; the allocation-only saving is ~6.5%.
- Do measure before prioritizing — three confident structural guesses (singleton
  allocation cost, flat-buffer merge value, "the merge floor is mostly reads") were
  falsified or halved by probes here.
- Do control for V8 tiering and background load: repeat phases in-process
  (`sort_repeat_probe.tw`) and check system load — a background game invalidated one
  whole benchmarking session, and the native kernel is ~1.7× faster warm than cold.
