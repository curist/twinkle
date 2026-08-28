# Bottom-Up HAMT Builder Plan

> **For agentic workers:** The correctness/performance spike is complete. Keep its benchmark-only machinery isolated while completing the production-adapter and cleanup work recorded below.

**Status:** Successful spike. Direct bottom-up construction is correct on the measured random-hash workload and materially faster than both incremental builders. Size-aligned crossover calibration now supports a real-dense publication crossover near `k/n = 0.12–0.15` at 1M live entries. Production interface design is **settled** in [mutdict-bottom-up-publication-adapter.md](mutdict-bottom-up-publication-adapter.md); adversarial hash-shape coverage and spike-surface cleanup remain open (the cleanup is now sequenced by the adapter doc's §9).

**Goal:** Determine whether building the persistent HAMT directly from a dense stream of cached-hash entries materially lowers MutDict publication cost and shifts the flat→persistent crossover. **Answer: yes; productionization is justified, and the random-hash workload now has a decision-grade size-aligned calibration, but no universal compiler threshold is encoded yet.**

**Architecture:** The implemented spike synthesizes the future MutDict freeze input inside the existing `Dict.compact()` seam and a benchmark-only runtime entry point. It counting-partitions entries by successive 5-bit hash fragments, allocates each final HAMT node and compressed slot array at exact size, and bulk-builds insertion order. The three-way harness prepares one dense stream, then times sequential persistent insertion, owned/editable insertion, direct bottom-up construction, and order construction independently.

**Input contract:** [mutdict-dense-freeze-input.md](mutdict-dense-freeze-input.md)

**Primary context:**
- [sound-uniqueness/storage/spike-tier0-dict.md](sound-uniqueness/storage/spike-tier0-dict.md)
- [sound-uniqueness/storage/README.md](sound-uniqueness/storage/README.md)
- `boot/compiler/codegen/runtime/dict.tw`
- `boot/compiler/codegen/runtime/types.tw`
- `boot/bench/dict_compact_builder_spike.tw` on branch `spike/mutdict-bulk-hamt`

---

## What we know

The original flat freeze inserts every entry into a fresh persistent `Dict` in insertion order. It costs about as much as building the dict normally and creates the measured crossover near k/n ≈ 1.

Source inspection corrected an assumption behind that result:

- `dict$set_in_place` mutates the outer `PDict` shell.
- Its internal `node_set` still path-copies HAMT nodes and entries arrays.
- Therefore inplace≈rebuild did not measure a true transient/editable HAMT.

An experimental `node_set_owned`, routed only through `compact()` while constructing a fresh root, measured the missing allocation lever:

| original size | live entries rebuilt | sequential builder | editable builder |
|---|---:|---:|---:|
| 65 536 | 32 768 | 10.51 ms | about 5.9–8.1 ms |
| 1 048 576 | 524 288 | 322.74 ms | about 237–254 ms |

The large case improves about 1.3×. It is useful but still performs one hash/trie traversal per entry, incrementally grows node slot arrays, looks values up in the old HAMT, and appends order entries individually.

A random-hash shape model at n=1M estimated that exact-sized bottom-up nodes would copy roughly 6.5× fewer slot references than the editable incremental builder. The naive bottom-up implementation now realizes that exact-sized-node strategy. The model did not predict wall-clock speedup: measured node construction is typically about 2.2× faster than editable construction at the large scale because partitioning, recursion, temporary arrays, and GC remain.

### Measured bottom-up result

The shared-dense harness runs one warmup and three reported samples. Representative post-warmup phase ranges are:

| benchmark label | live entries | dense prep | sequential build | editable build | bottom-up build | bulk order |
|---|---:|---:|---:|---:|---:|---:|
| 65K | 32 768 | about 1.4–1.5 ms | about 5–6 ms | about 2–4 ms | about 1.3–2.8 ms | about 0.5–0.7 ms |
| 1M | 524 288 | about 50–52 ms | about 168–173 ms | about 72–75 ms | about 30–35 ms | about 12–13 ms |

Occasional GC/timer outliers occur, especially in the editable phase. Every returned dictionary passes the full insertion-order stream, all live values, and all absent-key checks. The self-host reaches a fixed point and the boot suite passes.

The phase boundaries matter:

- dense preparation contains the sole `node_get` and is synthetic overhead absent from a real MutDict-owned dense stream;
- all three builders consume the exact same materialized `HamtEntry` array;
- a real dense publication at 524K live entries is approximately bottom-up build plus bulk order, about 42–48 ms in these runs;
- the current PDict-synthesis seam also pays roughly 50 ms of dense preparation.

WAT confirms that the harness calls the three distinct builders, that incremental builders perform no `node_get`, and that `compact()` calls `node_build_bottom_up` without `node_set`, `node_set_owned`, or per-entry `arr_push`.

## Spike question

Can a direct bottom-up builder make flat→HAMT publication cheap enough to materially expand the profitable MutDict region, while preserving the exact persistent `Dict` representation and insertion-order semantics?

**Result:** Yes on the measured workload. Bottom-up construction is already a material win in its naive form and is justified as the target MutDict-to-PDict publication adapter. It also benefits `Dict.compact()` independently.

This result does not make MutDict unconditional. Sparse live-base forks remain a structural HAMT advantage regardless of freeze speed.

## Recommended construction strategy

Use recursive most-significant-for-the-HAMT partitioning over the HAMT fragment sequence:

1. Materialize dense logical entries containing cached hash, key, value, and order index.
2. Partition the current range by bits 0–4 of the hash.
3. For each bucket containing multiple entries, recurse using bits 5–9, then 10–14, and so on.
4. Emit a leaf directly for singleton buckets.
5. At exhausted hash depth, emit a collision node and preserve key equality semantics.
6. Allocate each `HamtNode.entries` array at its final occupancy and fill it once.
7. Build `PDict.order` independently from the dense insertion-order keys with `arr_from_array`.

The implemented naive partitioner allocates fresh count, cursor, and scratch arrays per recursive node. This already captures the modeled slot-copy reduction and is fast enough to justify the architecture. A production interface should accept an internal workspace so shared ping-pong buffers can be added without changing publication semantics, but ping-pong is now a profiling-driven follow-up rather than a prerequisite. Measure allocation volume, peak live memory, and GC time before paying its complexity.

## Scope

This plan covers:

- a spike-only dense entry representation;
- bottom-up HAMT construction;
- exact collision behavior;
- insertion-order metadata;
- correctness and performance comparison with both existing builders;
- a decision on whether the builder belongs in the eventual MutDict freeze path.

It does not cover:

- implementing open-addressing MutDict;
- source-level region discovery or ownership proofs;
- clone-on-fork policy;
- flat-first public `Dict`;
- replacing HAMT for sparse-fork workloads.

## Work outline

### Task 1 — Stabilize the three-way benchmark

**Files:**
- Modify `boot/bench/dict_compact_builder_spike.tw`
- Inspect `boot/compiler/codegen/runtime/dict.tw`

- [x] Account for the prior routing through `node_set_owned`; retain explicit sequential and editable adapters.
- [x] Keep the owned/editable result as a separately selectable builder rather than overwriting the only comparison path.
- [x] Warm up before reporting and record multiple samples for timer stability.
- [x] Keep full content, absence, and insertion-order guards for every strategy.
- [x] Measure dense preparation, each builder, and bulk order separately at both benchmark scales.

**Precondition — shared dense input (hard requirement, not advisory):** All three strategies must consume the *same* pre-materialized dense entries produced in Task 2. The current `compact()` seam performs one `node_get` per entry during dense preparation to recover the value from the old HAMT; a real MutDict stream would not pay this. If the bottom-up path reads values straight from dense entries while the sequential/editable baselines still probe the old HAMT, the comparison is skewed toward bottom-up. Either all strategies read from the dense entries, or the `node_get` overhead is measured and subtracted identically from every strategy. The `compact()` seam is an input synthesizer, not the final freeze API.

### Task 2 — Define spike-only dense entries

**Files:**
- Modify `boot/compiler/codegen/runtime/types.tw`
- Modify `boot/compiler/codegen/runtime/dict.tw`

- [x] Reuse the internal `HamtEntry` shape to carry hash, key, value, and insertion-order index.
- [x] Materialize entries from live insertion order while retaining existing hash and equality functions.
- [x] Separate dense-entry preparation from each tree-construction strategy and bulk order construction.
- [ ] Add explicit empty, singleton, deep-shared-prefix, full-hash-collision, and post-publication persistent-update coverage. The large random-hash parity oracle is necessary but not sufficient for these adversarial shapes.

### Task 3 — Implement recursive partitioning and exact node construction

**Files:**
- Modify `boot/compiler/codegen/runtime/dict.tw`
- Modify `boot/compiler/codegen/runtime/types.tw` only for temporary scratch or entry types that are actually required

- [ ] Add dedicated runtime-shape tests; the spike established RED through its missing benchmark API and behavior through the full parity oracle, not targeted shape fixtures.
- [x] Implement one stable 32-bucket counting-partition step over a dense range.
- [x] Add `node_build_bottom_up(entries, lo, hi, depth)` recursion over successive 5-bit fragments with the same depth limit as `node_get` and `node_set`.
- [x] Allocate the final compressed bitmap and exact-sized entries array once per node.
- [x] Build collision nodes after all hash bits are consumed.
- [x] Construct `PDict.order` in bulk and preserve the dense stream's insertion order.
- [x] Confirm ordinary persistent `set` and `remove` retain their unchanged paths.

### Task 4 — Compare construction strategies

- [x] Run sequential, editable, and bottom-up builders from one shared dense input in the same process.
- [x] Report dense-entry preparation, each builder, and order construction separately.
- [ ] Add clustered-prefix and deliberate full-hash-collision workloads; the current key stream covers sparse pseudo-random hashes.
- [x] Inspect WAT to confirm the bottom-up path does not call `node_set`, `node_set_owned`, or repeated `arr_push` inside construction.
- [x] Produce a decision-grade crossover from size-aligned, same-session mutation and publication measurements.

#### Size-aligned crossover calibration

The calibration starts every flat, persistent, and publication measurement with
exactly `n` live entries and applies `k=n` deterministic overwrites to both
mutation strategies. Each sample verifies insertion order, every final live
value, and an equally large disjoint absent-key range for the flat table, the
persistent mutation result, and all three builder dictionaries. The persistent
loop also retains every pre-write version and checks its value, preventing an
in-place path from replacing the intended persistent baseline.

The benchmark has one warmup followed by three samples in one process. Builder
execution rotates sequential/editable/bottom-up, editable/bottom-up/sequential,
and bottom-up/sequential/editable; phase labels always name the same builder.
The `dense_prep` phase is synthetic PDict preparation (`hash_key` plus
`node_get`) and is never included in real-dense publication. Real dense-input
publication is `bottom_up + order`; the preparation-inclusive column is reported
separately for the current PDict-synthesis seam.

| live n | sample (builder order) | flat mutation | persistent mutation | dense prep | bottom-up + order | prep-inclusive publication | real `k/n` | prep-inclusive `k/n` |
|---:|---|---:|---:|---:|---:|---:|---:|---:|
| 65 536 | sample1 (S,E,B) | 0.731 ms | 22.401 ms | 2.221 ms | 6.818 ms | 9.039 ms | 0.315 | 0.417 |
| 65 536 | sample2 (E,B,S) | 0.730 ms | 22.577 ms | 2.859 ms | 6.748 ms | 9.607 ms | 0.309 | 0.440 |
| 65 536 | sample3 (B,S,E) | 0.739 ms | 23.652 ms | 2.700 ms | 5.535 ms | 8.235 ms | 0.242 | 0.359 |
| 1 048 576 | sample1 (S,E,B) | 15.179 ms | 833.218 ms | 158.858 ms | 109.886 ms | 268.744 ms | 0.134 | 0.329 |
| 1 048 576 | sample2 (E,B,S) | 14.603 ms | 862.918 ms | 192.079 ms | 105.685 ms | 297.764 ms | 0.125 | 0.351 |
| 1 048 576 | sample3 (B,S,E) | 14.654 ms | 845.541 ms | 155.880 ms | 120.274 ms | 276.155 ms | 0.145 | 0.332 |

Each crossover is derived per sample as
`publication_cost / (persistent_mutation_at_k=n - flat_mutation_at_k=n)`.
At 1M live entries, the real-dense crossover is consistently `0.125–0.145`,
while the synthetic-PDict-seam-inclusive crossover is `0.329–0.351`. The 65K
rows are broader (`0.242–0.315` real and `0.359–0.440` inclusive), including
order/build variation across rotated positions; retain the ranges rather than
collapsing them into a single average.

**Decision:** direct bottom-up publication materially moves the 1M dense
flat-to-persistent boundary to roughly 12–15% as long as the future MutDict
supplies cached hashes and values directly. The synthetic PDict seam would move
that boundary to roughly 32–34%, so it must not be silently charged to real
publication. This calibrates the production-adapter case but does not select or
encode a compiler threshold: size dispatch, adapter design, ping-pong scratch,
and adversarial hash shapes remain separate work.

### Task 5 — Production direction and spike cleanup

- [x] Decide that direct bottom-up construction survives as the intended MutDict publication adapter and as the current `Dict.compact()` rebuild strategy.
- [x] Design a compiler-private dense input and workspace interface. The input owns live unique keys, values, cached full hashes, and insertion-order indices; the builder performs no lookup or rehash. **Settled in [mutdict-bottom-up-publication-adapter.md](mutdict-bottom-up-publication-adapter.md)** (dense seam = immutable `array<HamtEntry>`; naive scratch behind a stable `freeze_dense` entry point; ping-pong is a profiling-gated follow-up).
- [ ] Make publication consume or invalidate the mutable handle and return an ordinary immutable `PDict` with no builder-owned mutable state reachable afterward. *(Design settled — §4/§5 of the adapter doc; execution is implementation-plan work.)*
- [ ] Retain the naive partitioner behind the workspace interface first; add ping-pong only if allocation/GC profiling justifies it. *(Design settled — §3 of the adapter doc.)*
- [ ] Remove spike-only public surfaces after the evidence is recorded: `Dict.bench_builders`, `Dict.bench_timings`, their builtin registrations/signatures, the timing global, and the `twinkle_runtime.now` import in `rt.dict`.
- [ ] Remove sequential/editable benchmark adapters when the comparison harness is retired; ordinary persistent operations remain.
- [ ] Update `sound-uniqueness/storage/spike-tier0-dict.md` and the storage README with the measured result, provisional crossover range, and remaining caveats.

The production adapter should conceptually separate:

```text
MutDict dense storage
  -> build_hamt_bottom_up(dense, workspace)
  -> build_order_bulk(dense)
  -> PDict
```

`Dict.compact()` may continue to synthesize the same logical dense input and reuse
the adapter, but that synthesis cost is not part of real MutDict publication.

## Correctness requirements

The resulting `PDict` must be observationally identical to sequential construction:

- every live key maps to the same value;
- size matches the number of live entries;
- insertion-order iteration is unchanged;
- same-hash unequal keys remain distinct;
- deterministic hashing is not changed;
- persistent updates after construction preserve structural sharing and do not mutate the published root;
- no builder-owned mutable handle is used after publication.

## Validation

For each retained runtime iteration:

```bash
target/twk fmt boot/compiler/codegen/runtime/dict.tw \
  boot/compiler/codegen/runtime/types.tw \
  boot/bench/dict_compact_builder_spike.tw
target/twk lint boot/main.tw
make stage2
make quick-bundle-cli
target/twk test
target/twk run boot/bench/dict_compact_builder_spike.tw
```

Inspect emitted construction calls:

```bash
target/twk wat boot/bench/dict_compact_builder_spike.tw --func bench_builders --calls
target/twk wat boot/bench/dict_compact_builder_spike.tw --func node_build_bottom_up --list
target/twk wat boot/tests/main.tw --func rt_dict__compact --calls
```

Do not run tree-sitter tests; this plan does not touch the grammar.

## Decision output

1. **Construction:** At 524K live entries, bottom-up construction is typically about 30–35 ms versus 72–75 ms editable and 168–173 ms sequential.
2. **Phase split:** Synthetic dense preparation is about 50–52 ms and bulk order about 12–13 ms at that scale. Real MutDict publication should not pay the old-HAMT `node_get` preparation seam.
3. **Crossover:** Size-aligned, same-session measurements put real dense publication at `k/n = 0.125–0.145` and synthetic-PDict-seam-inclusive publication at `0.329–0.351` for 1M live entries. The smaller 65K rows are broader; do not encode a threshold before size-dispatch and adversarial-shape work.
4. **Required inputs:** Cached hashes, direct key/value access, and bulk order construction are part of the intended production input contract. Without them, old-HAMT lookup and incremental order costs obscure the builder result.
5. **Verdict:** Preserve bottom-up construction as the intended MutDict freeze adapter and as a useful `Dict.compact()` implementation. Do not preserve the benchmark APIs as public surface.
6. **Ping-pong:** The naive builder already proves the lever. Treat shared ping-pong scratch as optional, profiling-driven optimization rather than a production gate.

No result from this plan changes the existing sparse-fork conclusion. Persistent HAMT remains the fallback for sparse-divergence forks and any region lacking the proofs or dense input required by MutDict.
