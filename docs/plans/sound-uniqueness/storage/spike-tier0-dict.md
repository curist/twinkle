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

They are equal within noise. Source inspection after round 2 found that this
comparison says less about allocation than originally claimed: `dict$set_in_place`
mutates only the outer `PDict`; its `node_set` still path-copies every HAMT node
and entries array. Thus inplace ≈ rebuild isolates the outer-shell allocation,
not a true editable/transient HAMT. Both paths still pay hashing, traversal,
insertion-order maintenance, and internal path copying.

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

## Round 2 (2026-08-28): reads + forks — the "unconditional" question

Round 1 (above) measured **mutate + freeze** and found the k/n ≈ 1 crossover.
The question "could a flat `MutDict` beat persistent *unconditionally* (not just
at k ≳ n)?" turns on two things round 1 never measured: **read throughput** and
**fork / structural-sharing cost**. Bench:
[`boot/bench/mutdict_reads_forks_spike.tw`](../../../../boot/bench/mutdict_reads_forks_spike.tw)
(same open-addressing proxy + sparse bijective keys; guards match across
strategies). Single pass, `target/twk run` on main.

### Reads — flat wins, and the lead grows with n

2M random probes, persistent `Dict` GET vs flat open-addressing GET:

| n | persistent | flat | flat speedup |
|---|---|---|---|
| 4 096 | 86.5 | 21.8 | **3.96×** |
| 65 536 | 91.6 | 18.5 | **4.95×** |
| 1 048 576 | 506.8 | 42.0 | **12.1×** |

Same read-wall shape as vectors: one hash + probe vs a deepening tree traversal,
so the flat advantage grows as the HAMT gets deeper.

### Forks — persistent wins until forks get very dense

`f` forked versions, each doing `d` updates, base kept **live** across all forks
(so persistent forks are true copy-on-write structural shares, not in-place):

| n | ops/fork `d` | persistent | flat (clone+write) | winner |
|---|---|---|---|---|
| 65 536 | 4 | 0.18 | 95.2 | persistent **520×** |
| 65 536 | 64 | 2.9 | 55.8 | persistent 19× |
| 65 536 | 1 024 | 16.0 | 56.7 | persistent 3.5× |
| 65 536 | 16 384 | 219.8 | 68.2 | **flat 3.2×** |
| 1 048 576 | 4 | 0.12 | 231.8 | persistent **1900×** |
| 1 048 576 | 256 | 3.2 | 230.4 | persistent 72× |
| 1 048 576 | 4 096 | 42.9 | 231.4 | persistent 5.4× |
| 1 048 576 | 65 536 | 816.2 | 270.0 | **flat 3×** |

The fork crossover sits at roughly **d ≳ n/20 to n/30 updates per fork** — a
forked version must rewrite a few percent of the whole dict before its O(n) clone
amortizes against a HAMT's O(log n) structural share. For sparse divergence (the
common case: a few updates per version) persistent wins by 20–1900×.

### What this says about "unconditional"

Flat wins the two dominant dict operations — **owned mutate (40–70×, round 1)**
and **reads (4–12×)** — but loses exactly the two things a HAMT is structurally
built for: **(1)** sparse-divergence forks of a live dict (O(log n) share vs O(n)
clone), and **(2)** the escape-to-persistent-ABI freeze (round 1's k < n loss).
So flat **cannot be unconditional** while `Dict` must offer cheap sparse forks and
a persistent escape — the conditional/gated conclusion stands, now confirmed from
the read and fork angles too.

**Customer tension made concrete:** the marquee S5 customer, `run_fixpoint`'s
transfer maps, is the *worst* case for flat — it forks per block with **few
updates per block** (sparse `d`) at **small width** (32–60, where HAMT reads are
near-constant and flat's read edge shrinks; see
[fixpoint-map-intmap.md](../../archive/fixpoint-map-intmap.md)'s round-2 rejection). So
persistent wins there on both fork and read. A good *first* flat customer is the
opposite shape: a **large, owned, build-and-query dict that never fork-shares and
never escapes** (flat build ≈ persistent build, then reads win 4–12× with no
freeze).

**The bigger lever the data hints at (flat-first default).** Today's model is
"HAMT by default, flat when dense-mutate is proven." But reads *and* mutate both
favor flat; only sparse-fork-sharing favors HAMT. The inversion — a
**flat-immutable-by-default `Dict`, falling back to HAMT only where the compiler
detects sparse-fork sharing** — is where "beat persistent everywhere" would
actually come from. It is a much larger change (it touches the persistent `Dict`
ABI itself, and the freeze/clone snapshot machinery would become the *common*
path, not the edge), and it needs a real-program census of dict fork-sharing
density before it's justified. Recorded as the S5 alternative to weigh against the
gated-`MutDict` slice.

## Round 3 (2026-08-28): owned/editable HAMT construction

A follow-up runtime spike tested the missing comparison. It added
`node_set_owned`, which mutates only a freshly constructed HAMT root, and routed
only `compact()`'s rebuild through it. Ordinary persistent `set` and
`set_in_place` retained their existing behavior. The threshold-crossing removal
in [`dict_compact_builder_spike.tw`](../../../../boot/bench/dict_compact_builder_spike.tw)
rebuilds a dict containing half the original entries:

| original n | live entries rebuilt | sequential `node_set` | editable builder |
|---|---:|---:|---:|
| 65 536 | 32 768 | 10.51 ms | about 5.9–8.1 ms |
| 1 048 576 | 524 288 | 322.74 ms | about 237–254 ms |

The large case improves only about **1.3×**. This is real but not transformative:
an editable builder removes ancestor path-copy allocation, but still hashes and
traverses once per entry, incrementally grows node slot arrays, performs a lookup
in the old HAMT, and appends the order vector entry by entry. Because compaction
also performs the old-HAMT lookup, this is not a direct flat→HAMT freeze
measurement; it is an isolation test for the allocation/path-copy lever.

A direct bottom-up builder has additional headroom. Radix-partitioning entries by
the HAMT's 5-bit hash fragments would allocate every node and exact-sized slot
array once, avoid per-entry trie traversal, and build insertion order with one
`arr_from_array`. A random-hash shape model at n=1M estimates roughly **6.5× less
slot-reference copying than the editable builder**. But implementing the full
builder requires hash partitioning, scratch storage, exact collision handling,
and stage0/boot parity. It is therefore a plausible Tier-1 optimization, not a
cheap tweak, and it remains O(n).

**Round-3 verdict:** retain the conditional-MutDict conclusion. An editable HAMT
builder can lower the freeze constant, invalidating the earlier claim that a
transient builder cannot help at all, but it does not remove the flat→persistent
crossover or the HAMT's sparse-fork advantage. Benchmark a direct bottom-up
builder only together with the real GC-array `MutDict`, where its actual dense
entry/hash arrays and order sidecar exist as inputs.

## Real Wasm-GC arena follow-up (2026-08-29)

The quick follow-up replaced the linear-memory proxy with the two candidate live
arenas feeding the landed bottom-up `freeze_dense` adapter. Across the combined
rotated 1M samples, Candidate M's unboxed in-place overwrite was about 20–24×
faster, while Candidate H's immutable-entry shallow clone was roughly 20–30×
cheaper and its dense seam was truly zero. With one full overwrite and one clone,
H won pre-publication (median 31.98 ms versus M's 73.82 ms); with four overwrites
per entry, M won (62.10 ms versus H's 107.13 ms). In the delete/reinsert workload,
M's pre-clone lead was erased or reversed by its deep clone within overlapping
ranges.

The result fortifies rather than resolves the representation gate: the winner
changes with update density and flat-clone frequency, and GC-sensitive ranges do
not justify turning the measured 1x–4x crossover into a threshold. Correctness
and emitted-shape guards passed, but construction and an end-to-end persistent
control were not timed, and String keys remain unmeasured. The reviewed verdict is
therefore **STOP WITHOUT SELECTION**. Candidate M and Candidate H remain open; the
full table, caveats, and reopening requirements are in
[the active arena gate](../../mutdict-dense-freeze-input.md#quick-spike-result-2026-08-29).

## Conclusions for S5 (revises the earlier decision)

1. **The operation-throughput lever is flat storage (`MutDict`), not a transient
   HAMT.** An editable HAMT builder improves materialization by about 1.3×, but
   keeps HAMT traversal during construction and does not approach flat mutation
   throughput. Drop transient-HAMT-as-the-region-representation; retain it only
   as a possible materialization helper.
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
- The owned/editable builder result comes from the existing `compact()` seam,
  not directly from flat storage. It includes old-HAMT lookups and incremental
  order-vector appends that a real `MutDict` freeze can avoid.
- A radix-partitioned bottom-up HAMT build remains unmeasured. It can cut more
  construction work than the editable builder, but stays O(n), must preserve
  insertion order and exact collision semantics, and does not address sparse
  forks.
- Flat-proxy uses linear memory + open addressing; a real GC-array `MutDict` would
  be at least as fast on mutate.
