# MutDict Dense Freeze Input Plan

> **For agentic workers:** Use `subagent-driven-development` or `executing-plans` to execute this plan with review checkpoints. Treat the representation choice as a measured design gate, not permission to implement all of S5.

**Status:** Draft investigation/implementation plan. S5 has not started.

**Goal:** Design the real Wasm-GC `MutDict` backing so it retains key hashes and can expose live entries as a dense insertion-order stream suitable for cloning, querying, and bottom-up HAMT materialization.

**Architecture:** `MutDict` remains a compiler-private flat open-addressing hashmap for hot owned regions. Its physical layout must also support the logical freeze contract `cached_hash + key + value + order_index` for every live entry without rehashing keys or reconstructing insertion order. This plan chooses and validates that layout; the bottom-up HAMT consumer is planned separately in [dict-bottom-up-hamt-builder.md](dict-bottom-up-hamt-builder.md).

**Primary context:**
- [sound-uniqueness/storage/README.md](sound-uniqueness/storage/README.md)
- [sound-uniqueness/storage/spike-tier0-dict.md](sound-uniqueness/storage/spike-tier0-dict.md)
- [`boot/bench/dict_spike.tw`](../../boot/bench/dict_spike.tw)
- [`boot/bench/mutdict_reads_forks_spike.tw`](../../boot/bench/mutdict_reads_forks_spike.tw)

---

## What we know

The Tier-0 buffer proxy established the representation tradeoff:

- Flat open addressing wins owned writes by roughly 40–70× and large-map reads by roughly 4–12× over the current boxed HAMT.
- Converting flat storage to the persistent HAMT with sequential `Dict.set` is an expensive O(n) build.
- Flat snapshots are much cheaper to clone than to convert to a HAMT.
- Persistent HAMT storage remains decisively better for sparse divergence from a live base.
- `run_fixpoint`'s small, sparse-forked transfer maps are therefore not a MutDict customer.

The current buffer proxy stores open-addressing keys and values plus an insertion-order key sidecar. A real runtime representation has not been selected. In particular, we have not decided whether order tracks keys, table slots, stable entry IDs, or a dense entry arena.

The bottom-up builder investigation adds one requirement: do not discard hashes already computed for open-addressing lookup. Rehashing every key at publication would waste work and make string-key freezing especially expensive.

## Required logical contract

Regardless of physical layout, a proven-owned `MutDict<K,V>` must be able to enumerate every live entry exactly once in insertion order as:

```text
DenseFreezeEntry {
  cached_hash: i64,
  key: K,
  value: V,
  order_index: i32,
}
```

This is a logical contract, not a commitment to allocating one record per entry. Parallel typed GC arrays are acceptable and likely preferable.

Required invariants:

- `cached_hash` is the same deterministic hash used by persistent `Dict`.
- Updating an existing key preserves its order position.
- Removing a key removes it from logical iteration.
- Removing and reinserting a key appends it at the end.
- Enumeration contains no empty slots or deletion tombstones.
- Ownership of the backing does not imply ownership of reference-typed keys or values.
- A clone used for an observed old version preserves the complete logical stream.
- Consuming the stream for persistent materialization invalidates the mutable handle only at the selected publication boundary.

## Candidate physical layouts to evaluate

### A. Table arrays plus order-of-key sidecar

Keep keys, values, and cached hashes in open-addressing slot arrays; keep insertion-order keys separately. Freeze walks order and probes the table for each value.

- Simplest continuation of the Tier-0 proxy.
- Resizing is straightforward.
- Dense publication still performs n table probes.

### B. Table arrays plus order-of-slot sidecar

Order stores table slot indices.

- Cheap ordered value access until resize.
- Every resize must rewrite the order sidecar because slots move.
- This coupling may make growth and cloning more expensive.

### C. Stable dense entry arena plus open-addressing index

Insertion appends `{hash,key,value,live}` to a dense arena; table slots point to stable entry IDs. Updates replace the value at the existing entry ID, removes mark the entry dead, and reinsertion appends a new entry.

- Naturally provides the desired publication stream and stable insertion order.
- Resizing rebuilds only the index.
- Requires a compaction policy for dead arena entries and careful clone accounting.

**Initial recommendation:** evaluate C first. It aligns the hot flat index with the publication contract without making table slots part of observable order. Do not settle it without measuring update, remove/reinsert, resize, clone, and dense-stream costs.

## Scope

This plan covers:

- the private Wasm-GC runtime representation;
- cached deterministic hashes;
- flat get/set/remove behavior;
- insertion-order preservation;
- clone/snapshot behavior;
- producing the dense freeze input;
- enough compiler-private ABI to benchmark the runtime target.

It does not cover:

- selecting source regions for MutDict;
- ownership or freshness proofs;
- interprocedural mutable ABI routing;
- implementing the bottom-up HAMT builder;
- changing the public `Dict<K,V>` representation;
- making flat Dict the language default.

## Work outline

### Task 1 — Confirm the runtime contract and choose a layout

**Inspect:**
- `boot/compiler/codegen/runtime/dict.tw`
- `boot/compiler/codegen/runtime/types.tw`
- `src/runtime/dict.rs`
- `src/runtime/types.rs`
- `boot/bench/dict_spike.tw`

- [ ] Record the exact persistent hash and equality behavior that MutDict must reuse.
- [ ] Specify growth, empty-slot, deletion, and compaction rules for each candidate layout.
- [ ] Specify how clone and dense enumeration operate without violating insertion order.
- [ ] Choose the smallest layout whose costs can be measured with a minimal runtime target.

**Deliverable:** a settled physical layout and logical dense-stream contract added to this document before runtime implementation begins.

### Task 2 — Build a minimal Wasm-GC MutDict runtime target

**Likely files:**
- Modify `boot/compiler/codegen/runtime/types.tw`
- Create or modify a focused runtime module under `boot/compiler/codegen/runtime/`
- Modify `boot/compiler/codegen/codegen.tw` to register the runtime module if a new module is created
- Mirror in `src/runtime/` only when moving from spike to a retained implementation

- [ ] Add behavior tests first for insert, replace, remove, remove/reinsert, resize, collision, and insertion-order enumeration.
- [ ] Implement only the Int→Int family needed for the first performance measurement, while keeping the representation contract generic.
- [ ] Add clone and dense-enumeration operations.
- [ ] Inspect emitted WAT to confirm flat GC arrays, cached hashes, and no persistent HAMT operations on the hot path.

### Task 3 — Measure the representation

**Benchmark:** extend or add a feature-named benchmark under `boot/bench/`; do not name it after an S5 sequence number.

- [ ] Compare the real GC-array target with the existing buffer proxy for build, overwrite, get, remove/reinsert, resize, clone, and dense enumeration.
- [ ] Sweep small and large maps and include sparse keys and deliberate full-hash collisions where practical.
- [ ] Keep guards that compare observable contents and insertion order.
- [ ] Report phases separately; do not hide publication cost inside mutation time.

### Task 4 — Hand the stream to publication consumers

- [ ] Freeze the exact logical contract consumed by the bottom-up-builder plan.
- [ ] Record whether the chosen layout can hand arrays directly to the builder or must first compact them.
- [ ] Record clone cost and any dead-entry threshold that triggers arena compaction.
- [ ] Update the storage README and spike results with measured conclusions.

## Validation

For retained Twinkle runtime changes:

```bash
target/twk fmt <changed-.tw-files>
target/twk lint boot/main.tw
make stage2
make quick-bundle-cli
target/twk test
```

Also inspect the relevant runtime calls with `target/twk wat ... --calls`. Preserve the known baseline when `twk lint boot/main.tw` reports unrelated existing findings; the acceptance condition is no new finding from this work.

## Decision output

This plan should end with answers to:

1. Which physical layout provides flat hot operations and a genuinely dense publication stream?
2. Does retaining hashes materially reduce publication cost, especially for strings?
3. What are clone and resize costs for the chosen layout?
4. Can bottom-up HAMT construction consume the representation without another O(n) reshaping pass?
5. Does the measured representation justify proceeding to MutDict region selection, or should S5 stop?
