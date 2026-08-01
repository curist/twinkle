# Tier-0 dict spike results

Bench source: [`boot/bench/dict_spike.tw`](../../../../boot/bench/dict_spike.tw)
Run: single pass, `target/twk run` on main (2026-08-01). Wall ms. Guards match
across all three strategies at every row → proxy validated.

Three strategies, `Dict<Int,Int>` of `n` sparse keys, `k` random updates:

- **inplace** — owned `m[k]=v`; flips to boxed HAMT `dict$set_in_place` (8D). The
  current best shipped.
- **rebuild** — same loop, `m` aliased each iteration so the in-place proof cannot
  fire; persistent HAMT rebuild. Naive baseline.
- **flat** — `@std.buffer` open-addressing table (proxy for a flat mutable
  `MutDict`): O(1) probed writes + insertion-order sidecar, then a freeze that
  inserts every entry into a fresh persistent `Dict` (the flat→HAMT rebuild).

## Two results that reframe S5

### 1. `dict$set_in_place` barely beats persistent rebuild

| n | k/n | rebuild mutate | inplace mutate |
|---|---|---|---|
| 65 536 | 400% | 95.2 | 96.8 |
| 1 048 576 | 25% | 239.3 | 266.8 |
| 1 048 576 | 100% | 898.2 | 853.7 |

They are equal within noise. For dicts the per-op cost is **HAMT traversal +
hashing + insertion-order maintenance**, and the persistent path's extra node
*allocation* is negligible on top of that. (Contrast vectors, where in-place was
~8× rebuild because PVec spine allocation is the dominant cost.) So the existing
8D in-place dict optimization delivers little, and — critically — **a transient
HAMT cannot help either**: it keeps the same HAMT traversal, so its mutate cost
≈ inplace. Its only advantage (~O(1) freeze) is moot because the in-place path
has no freeze to begin with.

### 2. Flat storage wins big on mutate, but the freeze is a full dict build

Mutate-phase, flat vs inplace:

| n | k/n | inplace | flat | flat vs inplace |
|---|---|---|---|---|
| 65 536 | 25% | 6.86 | 0.16 | **43×** |
| 65 536 | 100% | 24.09 | 0.62 | **39×** |
| 65 536 | 400% | 96.80 | 2.28 | **42×** |
| 1 048 576 | 25% | 266.76 | 3.72 | **72×** |
| 1 048 576 | 100% | 853.73 | 14.98 | **57×** |

Flat unboxed open-addressing is **40–70× faster** than boxed HAMT set_in_place —
the same read-wall story as vectors, larger here because hashing + tree traversal
is dearer than a trie index.

But the freeze (flat → persistent HAMT) is expensive — it *is* a full dict build:

| n | flat freeze | inplace build |
|---|---|---|
| 65 536 | ~17–20 | ~18–21 |
| 1 048 576 | ~596–606 | ~560–584 |

## The crossover is real and sits at k/n ≈ 1

End-to-end region cost (flat mutate + freeze) vs the current inplace mutate:

| n | k/n | inplace | flat (mut+freeze) | winner |
|---|---|---|---|---|
| 65 536 | 25% | 6.86 | 0.16+19.8 = 20.0 | inplace |
| 65 536 | 100% | 24.09 | 0.62+17.2 = 17.8 | flat (marginal) |
| 65 536 | 400% | 96.80 | 2.28+17.4 = 19.7 | **flat 5×** |
| 1 048 576 | 25% | 266.76 | 3.72+606 = 610 | inplace |
| 1 048 576 | 100% | 853.73 | 14.98+596 = 611 | flat |

Flat wins only when **mutations exceed distinct keys (k ≳ n)**; below that the
O(n) freeze dominates and flat loses. The user's "conversion cost might kill it"
concern is **confirmed** for the build-once / read-heavy regime.

## Boundary cost: clone-flat vs freeze-to-HAMT

A snapshot at a publication point can be produced two ways. Freeze rebuilds a
persistent HAMT (changes representation). Clone copies the flat backing (stays
flat). The expensive part of freeze is the *representation change* (tree build),
not the O(n) itself — so a flat clone is far cheaper:

| n | freeze → HAMT | clone (stay flat) | clone advantage |
|---|---|---|---|
| 65 536 | 19.9 | 1.4 | **14×** |
| 1 048 576 | 608 | 15 | **40×** |

The clone here is a naïve per-element i64 loop over the ~5n backing slots — an
*upper bound*; a real GC-array `MutDict` clones with one bulk `array.copy` and is
faster still. `gf==gc` confirms the clone is a correct snapshot.

This adds a middle tier to dict publication and reframes when `MutDict` wins:

1. **Old version provably dead → in-place, no copy** (the current set_in_place
   case).
2. **Old version observed but consumers stay in the private flat representation →
   clone** (14–40× cheaper than freeze). This is the key new option: internal
   forks (`next = m; next[k] = v` where both stay in owned-flat code) cost a cheap
   bulk copy, not a HAMT rebuild.
3. **Old version escapes to the persistent `Dict` ABI → freeze to HAMT**
   (expensive, unavoidable — but deferrable to the *true* edge, paid once).

Consequence: the k/n ≈ 1 crossover was driven by a per-boundary ~600ms freeze. If
internal forks clone (~15ms) and the HAMT build happens only at the final
published result, flat `MutDict` stays competitive far below k/n = 1 in
fork-heavy chains. This is precisely the `run_fixpoint` transfer-map shape: fork
per block, thread through owned-flat transfer code, build a HAMT only at the end.

Honest counterpoint: freeze is not the only alternative to clone. A persistent
HAMT forks in ~O(log n) via structural sharing, so for **sparse-divergence**
forks (each version does few ops) keeping a persistent HAMT can beat an O(n) flat
clone. Flat-clone wins when a forked version does enough ops to amortize the copy
(rough order at n=1M: ≳ ~10⁴ ops/fork, from clone_cost ÷ per-op saving). So the
real selector is *ops-per-fork* and whether the snapshot stays flat — not k/n
alone.

## Conclusions for S5 (revises the earlier decision)

1. **The dict lever is flat storage (`MutDict`), not a transient HAMT.** Transient
   HAMT keeps the traversal cost that actually dominates, so it does not beat the
   shipped in-place path. **Drop transient-HAMT-as-default.**
2. **`MutDict` is conditional, unlike `MutVec`.** It wins for update-dense / wide
   regions (k ≳ n) and *loses* for build-once/lookup-heavy dicts (k < n) because
   of the O(n) HAMT freeze. So it must be gated on a proven update-density / wide
   region with persistent fallback — it is not an unconditional win.
3. **This is exactly the S4 "stay low across a helper chain" customer.** The freeze
   only pays off when amortized over many ops before one materialization — i.e. a
   `MutDict` kept low across a long owned chain (the `run_fixpoint` transfer-map
   case), not a dict frozen every iteration.
4. **Size/k-dependence bites here (it did not for vectors).** The crossover sits at
   k ≈ n, both unknown at compile time, so `MutDict` adoption needs either a static
   update-density signal (loop trip-count vs distinct-key estimate) or a
   conservatively wide proven region — otherwise persistent fallback.

## Caveats / follow-ups

- Single run; n=4096 rows are near timer resolution.
- Transient HAMT was **not** built; its mutate ≈ inplace is inferred from "both
  traverse the HAMT" plus the measured inplace≈rebuild result, not measured
  directly. If S5 still wants to keep it as an option, a transient proxy would
  confirm, but the inference is strong.
- A bulk bottom-up HAMT build might cut the freeze constant below n sequential
  inserts, but it stays O(n) and must preserve insertion order; it does not move
  the k≈n crossover much.
- Flat-proxy uses linear memory + open addressing; a real GC-array `MutDict` would
  be at least as fast on mutate.
