# Bottom-Up HAMT Builder Plan

> **For agentic workers:** Use `subagent-driven-development` or `executing-plans` to execute this spike with review checkpoints. Keep benchmark-only machinery isolated until the performance decision is made.

**Status:** Draft spike plan. Direct bottom-up construction is unmeasured.

**Goal:** Determine whether building the persistent HAMT directly from a dense stream of cached-hash entries materially lowers MutDict publication cost and shifts the flat→persistent crossover.

**Architecture:** The spike first synthesizes the future MutDict freeze input inside the existing `Dict.compact()` seam, then radix-partitions entries by the HAMT's 5-bit hash fragments and allocates each final node once with an exact-sized slot array. It compares the current sequential persistent builder, the experimental owned/editable incremental builder, and direct bottom-up construction without requiring real MutDict or compiler region selection.

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

A random-hash shape model at n=1M estimates that exact-sized bottom-up nodes copy roughly 6.5× fewer slot references than the editable incremental builder. That model is not a runtime result. The direct route must be implemented and measured before using it to revise the S5 crossover.

## Spike question

Can a direct bottom-up builder make flat→HAMT publication cheap enough to materially expand the profitable MutDict region, while preserving the exact persistent `Dict` representation and insertion-order semantics?

This spike does not attempt to make MutDict unconditional. Sparse live-base forks remain a structural HAMT advantage regardless of freeze speed.

## Recommended construction strategy

Use recursive most-significant-for-the-HAMT partitioning over the HAMT fragment sequence:

1. Materialize dense logical entries containing cached hash, key, value, and order index.
2. Partition the current range by bits 0–4 of the hash.
3. For each bucket containing multiple entries, recurse using bits 5–9, then 10–14, and so on.
4. Emit a leaf directly for singleton buckets.
5. At exhausted hash depth, emit a collision node and preserve key equality semantics.
6. Allocate each `HamtNode.entries` array at its final occupancy and fill it once.
7. Build `PDict.order` independently from the dense insertion-order keys with `arr_from_array`.

A scratch-array ping-pong partitioner is the leading implementation option. Use `node_build_bottom_up(entries, scratch, lo, hi, depth)` as the intended internal helper name so the benchmark and WAT inspection have a stable target. A full fixed-pass radix sort followed by tree construction is a valid fallback if it substantially simplifies Wasm IR, but it touches every entry at every hash fragment and must be measured rather than assumed cheaper.

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

- [ ] Note the current routing: `compact()` already calls `node_set_owned` (the owned/editable builder), so the committed bench measures the editable path, not sequential. Restore or retain a separately selectable `node_set` path to reproduce the "sequential builder" column — do not assume sequential is the live baseline.
- [ ] Keep the owned/editable result as a separately selectable builder rather than overwriting the only comparison path.
- [ ] Warm up before reporting and record multiple samples for timer stability.
- [ ] Keep content and insertion-order guards for every strategy.
- [ ] Measure the builder seam separately at 65K and 1M-scale inputs.

**Precondition — shared dense input (hard requirement, not advisory):** All three strategies must consume the *same* pre-materialized dense entries produced in Task 2. The current `compact()` seam performs one `node_get` per entry (`dict.tw:2132`) to recover the value from the old HAMT; a real MutDict stream would not pay this. If the bottom-up path reads values straight from dense entries while the sequential/editable baselines still probe the old HAMT, the comparison is skewed toward bottom-up. Either all strategies read from the dense entries, or the `node_get` overhead is measured and subtracted identically from every strategy. The `compact()` seam is an input synthesizer, not the final freeze API.

### Task 2 — Define spike-only dense entries

**Files:**
- Modify `boot/compiler/codegen/runtime/types.tw`
- Modify `boot/compiler/codegen/runtime/dict.tw`

- [ ] Add an internal entry shape or parallel arrays carrying hash, key, value, and insertion-order index.
- [ ] Materialize those entries from `compact()`'s live order sequence while retaining the existing hash and equality functions.
- [ ] Separate dense-entry preparation time from tree construction time where the available timing seam permits.
- [ ] Test empty, singleton, replacement-free unique entries, deep shared prefixes, and full-hash collisions before optimizing.

### Task 3 — Implement recursive partitioning and exact node construction

**Files:**
- Modify `boot/compiler/codegen/runtime/dict.tw`
- Modify `boot/compiler/codegen/runtime/types.tw` only for temporary scratch or entry types that are actually required

- [ ] Write runtime-shape and behavior tests that fail before the bottom-up helper exists.
- [ ] Implement one 32-bucket partition step over a dense range.
- [ ] Add `node_build_bottom_up(entries, scratch, lo, hi, depth)` recursion over successive 5-bit fragments with the same depth limit as `node_get` and `node_set`.
- [ ] Allocate the final compressed bitmap and exact-sized entries array once per node.
- [ ] Build collision nodes for entries with identical complete hashes.
- [ ] Construct `PDict.order` in bulk and preserve update/remove/reinsert semantics represented by the input stream.
- [ ] Confirm ordinary persistent `set` and `remove` still use their unchanged paths.

### Task 4 — Compare construction strategies

- [ ] Run sequential, editable, and bottom-up builders in the same environment.
- [ ] Report dense-entry preparation, partition/build, order construction, and total publication separately where possible.
- [ ] Include sparse random hashes, clustered hash prefixes, and full-hash collisions.
- [ ] Inspect WAT to confirm the bottom-up path does not call `node_set`, `node_set_owned`, or repeated `arr_push` inside construction.
- [ ] Translate the measured publication cost back into the flat-versus-HAMT crossover using the existing mutation data.

### Task 5 — Decide what survives the spike

- [ ] If bottom-up construction materially shifts the end-to-end crossover, preserve the logical builder interface and make the real MutDict plan produce it directly.
- [ ] If the improvement is marginal, retain the simpler editable builder only if `Dict.compact()` benefits independently and stage0 parity is worthwhile.
- [ ] Remove temporary builder variants and mutable type changes that are not part of the selected result.
- [ ] Update `sound-uniqueness/storage/spike-tier0-dict.md` and the storage README with measured evidence and limitations.

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
target/twk wat boot/bench/dict_compact_builder_spike.tw --func compact --calls
target/twk wat boot/bench/dict_compact_builder_spike.tw --func node_build_bottom_up --list
```

Do not run tree-sitter tests; this plan does not touch the grammar.

## Decision output

A fresh session completing this plan must report:

1. Direct bottom-up construction time versus sequential and editable construction.
2. How much time belongs to dense-entry preparation versus HAMT node construction.
3. The resulting estimated k/n crossover for a flat region that must publish once.
4. Whether cached hashes and bulk order construction are required to obtain the win.
5. Whether the implementation is justified as a MutDict freeze adapter, only as a `Dict.compact()` optimization, or not at all.

No result from this plan changes the existing sparse-fork conclusion without a separate fork benchmark.
