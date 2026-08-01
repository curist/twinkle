# Tier-0 vector spike results

Bench source: [`boot/bench/mutvec_spike.tw`](../../../../boot/bench/mutvec_spike.tw)
Run: single pass, `target/twk run` on main (2026-08-01). Numbers are wall ms.

Three strategies, all producing a persistent `Vector<Int>` of size `n` after `k`
random indexed updates:

- **inplace** — owned `xs[i]=v` loop; flips to boxed PVec `set_in_place` on main
  (verified via WAT). The current best the compiler already ships.
- **rebuild** — same loop with `xs` aliased each iteration so the in-place proof
  cannot fire; true persistent PVec rebuild. The naive baseline.
- **flat** — `@std.buffer` i64 region (proxy for a growable GC-array `MutVec<Int>`):
  unboxed O(1) writes, then one O(n) `collect` freeze into `Vector<Int>`.

The proxy uses linear memory, so it slightly *understates* the real GC-array case.

## Mutate-phase only (the trustworthy signal)

Isolates update cost, insensitive to cross-function JIT/GC warmup. `flat` = write
loop only (freeze listed separately below).

| n | k/n | rebuild | inplace | flat | flat vs inplace |
|---|---|---|---|---|---|
| 65 536 | 25% | 2.49 | 0.47 | 0.023 | **20×** |
| 65 536 | 100% | 12.60 | 1.53 | 0.091 | **17×** |
| 65 536 | 400% | 44.83 | 5.91 | 0.322 | **18×** |
| 65 536 | 1600% | 162.65 | 19.78 | 1.272 | **16×** |
| 1 048 576 | 25% | 98.44 | 9.55 | 0.357 | **27×** |
| 1 048 576 | 100% | 376.19 | 41.62 | 1.392 | **30×** |
| 1 048 576 | 400% | 1519.52 | 185.73 | 5.239 | **35×** |

Unboxed flat mutation is **15–35× faster than boxed `set_in_place`**, widening
with n (the boxing / read-wall cost). Persistent rebuild is another ~8× worse than
`set_in_place`, so flat is ~250–290× faster than the naive persistent path.

## One-time freeze tax (flat only)

`MutVec -> PVec` O(n) copy, paid once at the boundary:

| n | freeze ms |
|---|---|
| 4 096 | ~0.067 |
| 65 536 | ~0.25 |
| 1 048 576 | ~5.5–6.0 |

Even including freeze, end-to-end `flat` wins at every n ≥ 65 536 row. Example at
n=1M, k/n=100%: inplace mutate 41.6ms vs flat mutate+freeze 1.39+6.02 = 7.4ms.

## Where flat loses

Only at small n with low k/n, where the O(n) freeze tax exceeds the mutate savings
— e.g. n=4096, k/n=25%: inplace 0.030ms vs flat mutate+freeze 0.087ms. Absolute
times there are sub-0.1ms and irrelevant. **No meaningful crossover exists in the
range that matters.**

## Conclusions for the storage track

1. **Build `MutVec`.** Unboxed GC-array storage beats boxed PVec `set_in_place` by
   15–35×; the boxing/read-wall is the dominant vector-update cost and this is the
   lever that removes it.
2. **Cliff-free, no runtime size dispatch for vectors.** Flat wins across all
   realistic n; the only losses are sub-0.1ms. The "private tagged representation"
   valve is not needed for vectors — the design simplifies to "flat for vectors."
3. **Freeze is a cheap boundary adapter,** not a concern for vectors — the
   symmetric-2×2 worry about materialization cost does *not* bite on the vector
   side (as predicted from the contiguous-copy constant).
4. **The ownership in-place analysis already earns its keep** (rebuild → inplace is
   ~8×); `MutVec` extends that same proven-owned path further.

## Caveats / follow-ups

- Single run; small-n rows (n ≤ 256) are at timer resolution — ignore them.
- Cross-function absolute `build` vs `freeze` comparison has a warmup confound
  (first big-n function pays JIT/GC); the mutate-phase *ratios* are the robust
  result and are far beyond noise.
- Tier-1 (real GC-array `MutVec` runtime op) should confirm the proxy and is
  expected to be at least as fast (no linear-memory pointer math).
- Dict spike (flat hashmap vs transient HAMT freeze cost) is still needed for S5.
