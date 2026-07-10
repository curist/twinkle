# Vector / sort / order-by performance — endeavor index

This folder is the vector-focused subtrack of the broader
[compiled-program performance plan](../compiled-programs.md): making **idiomatic
`Vector<T>` code — indexed reads, `sort_by`, dataframe `order_by` — fast** without
asking users to reach for specialized APIs.

Most of the effort has landed or been measured-and-parked; this README is the map.
For live per-boundary status start with **[boundary-tracklist.md](boundary-tracklist.md)**;
for the per-phase representation story see the umbrella
**[typed-vector-representation.md](typed-vector-representation.md)**.

## Current status (2026-07-10)

- **Landed (`main`):** typed `PVecI64` storage (unboxed i64 leaves, ~8× faster
  reads than boxed) at conservative closed sites — non-escaping locals (S2.0) and
  typed record fields (S2.2). Model: `PVecI64` is a **per-storage-site**
  optimization, never a global property of `Vector<Int>`
  ([../representation-boundary-policy.md](../representation-boundary-policy.md)); a
  uniform-typing attempt was built and reverted (archived
  `m1a-anyref-readback-investigation.md`).
- **Landed (this branch `typed-vector-crossfn-abi`, merging to `main`):** typed
  variant payloads (A3), cross-function return/copy ABI (B1–B5, B7), typed
  `gather`, closure-capture typing incl. the dataframe key column (C1–C2, via the
  oracle unification), element-family generalization + `PVecBool`, typed
  `Vector.make`, and typed-field call-result retyping. Net dataframe effect: the
  captured key column stays typed into the comparator.
- **B6 (the sort read-wall) — investigated, not shipped.** Re-measuring corrected a
  stale figure (full `order_by` is ~1.2s, not ~1.84s) and showed the merge floor is
  ~490ms of *boxed idx reads* out of a ~510ms floor (mechanics are ~17ms). A
  buffer-backed sort kernel worked (~35%) but was **reverted** as a
  compiler-special-cased point solution. The real blocker: a typed **parameter**
  ABI for named functions **does not exist** (only typed returns/captures do), so
  the merge's parameter reads can't stay typed via the analysis.

## The forward lever

The general unlock is **typed cross-function parameter ABI** ("Extend"):
specialize a function by representation + coerce at call sites, so typed vectors
flow through `map`/`filter`/`gather`/`sort`/user helpers uniformly. Everything B6
needed is downstream of it. Secondary: **B8** (typed `take`/gather for the non-Int
dataframe columns).

> **Scope note:** typed-vector representation pays off for **numeric/columnar**
> workloads (dataframe, big `Vector<Int>` reads/sorts). It does **not** help the
> boot compiler itself, whose hot vectors are *references* (`Vector<Instr>`,
> `MonoType`, `CoreExpr`, `String`) and whose costs are construction + Dict/HAMT.
> Pick the lever to match the workload.

## Living docs

| Doc | Role |
|-----|------|
| [boundary-tracklist.md](boundary-tracklist.md) | "Where are we" map — every boundary a typed vector must cross (A/B/C), per-item ✅/🟡/⬜, and the `order_by` critical path |
| [typed-vector-representation.md](typed-vector-representation.md) | The umbrella: per-phase status + the long-term representation answer |
| [generic-sort-by-vector-read-perf.md](generic-sort-by-vector-read-perf.md) | The read-wall measurement / decomposition — the reference for `order_by` cost |

## Archived / record (`archive/`)

Landed sub-work (design + plan pairs), a reverted approach, and superseded
spikes/handoffs. Kept for the record, not active.

- **Landed:** `storage-site-typed-vectors*` (A3 payloads), `expected-vt-coercion*`
  (B1), `tail-match-result-typing*` (B2), `crossfn-typed-vector-abi*`,
  `unify-typedness-oracle-design` (C2), `typed-vector-elem-families*` (+`PVecBool`),
  `typed-vector-make-and-bool-parity-design`,
  `2026-07-09-typed-field-call-result-retyping`, `native-typed-value-sort`
  (`sort_i64`, the no-comparator kernel on `main`).
- **Reverted:** `b6-representation-preserving-sort-design`,
  `b6-buffer-argsort-kernel-plan` (buffer sort kernel — compiler-special-cased
  point solution; findings folded into the tracklist).
- **Superseded / spikes / scoping:** `typed-vector-spike`,
  `typed-vector-continue-here`, `crossfn-abi-instrumentation`,
  `top-level-typed-vector-routing-scope`, `m1a-anyref-readback-investigation`
  (the reverted uniform-typing lesson), `native-key-index-argsort`,
  `wasm-native-sort`.

## Probes (in `examples/performance/`)

- `sort-bench/sort_by_component_probe.tw` — component breakdown (sort, closure, reads, append).
- `sort-bench/merge_attribution_probe.tw` — ablates the merge (reads vs singleton vs append/alloc).
- `sort-bench/typed_vec_read_probe.tw` — typed vs boxed `PVecI64` read routing (~6.8×).
- `sort-bench/typed_{record_field,variant_payload,gather,bool_*}_probe.tw` — per-boundary routing guards.
- `dataframe/bench/` — end-to-end `order_by` plus Clojure/Go/Rust references.

## Benchmark gate

```bash
target/twk run examples/performance/sort-bench/sort_by_component_probe.tw
target/twk run examples/performance/sort-bench/merge_attribution_probe.tw
target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw
```

## Lessons banked

- **`PVecI64` is per-site, never global.** A typed vector reaching a durable erased
  boundary (anyref, universal `ClosureEnv`, generic container, untyped variant)
  must be boxed — the danger is a coercion *per read* (the reverted uniform-typing
  bug), not a coercion at a crossing.
- **The merge floor is reads, not mechanics** (~490ms boxed reads of a ~510ms
  floor). Comparator micro-opts and allocation savings cap at a few percent.
- **Typed *parameter* ABI for named functions doesn't exist** — the real blocker
  for keeping vectors typed across calls; building it is the "Extend" project.
- **Don't special-case sorts.** A buffer/typed kernel that the compiler recognizes
  by name banks a number but is a point solution; the honest general form of the
  index-sort win is an explicit `argsort` primitive, and the principled read-wall
  fix is typed representation, not a bespoke sort.
- **Measure before prioritizing.** Confident structural guesses (allocation cost,
  flat-buffer merge value, "the floor is mechanics", "linear memory loses on
  sorts") were repeatedly falsified by probes here.
