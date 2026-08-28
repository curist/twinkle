# MutDict Dense Freeze Input Plan

> **For agentic workers:** Use `subagent-driven-development` or `executing-plans` to execute this plan with review checkpoints. Treat the representation choice as a measured design gate, not permission to implement all of S5.

**Status:** Active S5 representation investigation. The immutable publication adapter is landed; the real Wasm-GC `MutDict` runtime and its storage layout have not started.

**Goal:** Design and measure the real Wasm-GC `MutDict` backing so it retains key hashes and can produce the landed dense `array<HamtEntry>` publication seam in insertion order, with at most one conversion/compaction pass and no rehashing.

**Architecture:** `MutDict` remains a compiler-private flat open-addressing hashmap for hot owned regions. Its mutable storage layout is deliberately separate from the settled immutable publication seam: `freeze_dense(dense: array<HamtEntry>, len)` now builds the persistent HAMT and order vector. This plan chooses and validates the mutable backing, then measures whether it can hand a hole-free `HamtEntry` prefix directly to that adapter or must perform one conversion/compaction pass first.

**Primary context:**
- [sound-uniqueness/storage/README.md](sound-uniqueness/storage/README.md)
- [sound-uniqueness/storage/spike-tier0-dict.md](sound-uniqueness/storage/spike-tier0-dict.md)
- [archived publication-adapter design](archive/mutdict-bottom-up-publication-adapter.md)
- [archived adapter extraction plan](archive/2026-08-28-mutdict-publication-adapter-extraction.md)
- [bottom-up builder spike](dict-bottom-up-hamt-builder.md)
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

The consumer side is no longer open. `freeze_dense` accepts an initialized live prefix of immutable `HamtEntry { hash, key, val, order_index }` records, builds the HAMT bottom-up, bulk-builds order, and returns an ordinary persistent `PDict`. `Dict.compact()` is its first production customer, and engineered-hash tests cover empty, singleton, deep-prefix, and full-hash-collision shapes.

The storage investigation must therefore preserve hashes already computed for open-addressing lookup and produce that exact seam. Rehashing every key at publication would waste work and make string-key freezing especially expensive.

## Required storage-to-publication contract

Regardless of physical layout, a proven-owned `MutDict<K,V>` must be able to enumerate every live entry exactly once in insertion order with the logical fields:

```text
DenseFreezeEntry {
  cached_hash: i64,
  key: K,
  value: V,
  order_index: i32,
}
```

Parallel typed GC arrays or mutable arena records remain valid candidates for the hot storage representation. They are not the publication ABI. Before calling the landed adapter, storage must expose or materialize an immutable dense `array<HamtEntry>` whose initialized prefix contains those fields, with `order_index == output_position`. A hole-free canonical `HamtEntry` arena may pass its live prefix directly; any other representation gets one conversion/compaction pass.

Required invariants:

- `cached_hash` is the same deterministic hash used by persistent `Dict`.
- Updating an existing key preserves its order position.
- Removing a key removes it from logical iteration.
- Removing and reinserting a key appends it at the end.
- Enumeration contains no empty slots or deletion tombstones.
- The published prefix assigns `order_index = output_position`, including after interior dead entries are dropped.
- Publication performs at most one conversion/compaction pass and never rehashes or performs a per-entry persistent `node_get`.
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

Table slots point to stable entry IDs in insertion order. Two concrete variants now matter:

1. **Canonical immutable `HamtEntry` records plus external liveness.** Updates replace whole records and remap the index; a hole-free live prefix can be passed directly to `freeze_dense`, while deletions require compaction.
2. **Mutable arena records `{hash,key,value,live}`.** Updates can replace values in place, but publication must copy live entries into fresh immutable `HamtEntry` records and rewrite `order_index` during the one allowed conversion/compaction pass.

Both variants keep resizing confined to the index and preserve stable insertion identity. They trade hot update cost against publication copying, dead-entry compaction, and clone accounting.

**Initial recommendation:** evaluate C first, with the mutable-arena variant as the baseline and the canonical-entry variant as the zero-copy comparison. Do not settle it without measuring update, remove/reinsert, resize, clone, dead-entry compaction, seam conversion, and direct publication costs separately.

## Scope

This plan covers:

- the private Wasm-GC runtime representation;
- cached deterministic hashes;
- flat get/set/remove behavior;
- insertion-order preservation;
- clone/snapshot behavior;
- producing the landed immutable `array<HamtEntry>` freeze input directly or through one conversion/compaction pass;
- enough compiler-private ABI to benchmark the runtime target and its publication boundary.

It does not cover:

- selecting source regions for MutDict;
- ownership or freshness proofs;
- interprocedural mutable ABI routing;
- redesigning the landed bottom-up HAMT builder or `freeze_dense` adapter;
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
- [ ] Specify whether each candidate exposes canonical immutable `HamtEntry` records directly or requires the one allowed conversion pass.
- [ ] Choose the smallest layout whose costs can be measured with a minimal runtime target.

**Deliverable:** a settled mutable physical layout and an exact adapter mapping to `array<HamtEntry>` added to this document before runtime implementation begins.

### Task 2 — Build a minimal Wasm-GC MutDict runtime target

**Likely files:**
- Modify `boot/compiler/codegen/runtime/types.tw`
- Create or modify a focused runtime module under `boot/compiler/codegen/runtime/`
- Modify `boot/compiler/codegen/codegen.tw` to register the runtime module if a new module is created
- Mirror in `src/runtime/` only when moving from spike to a retained implementation

- [ ] Add behavior tests first for insert, replace, remove, remove/reinsert, resize, collision, and insertion-order enumeration.
- [ ] Implement only the Int→Int family needed for the first performance measurement, while keeping the representation contract generic.
- [ ] Add clone and dense-enumeration operations plus the candidate's seam-conversion operation.
- [ ] Inspect emitted WAT to confirm flat GC arrays, cached hashes, no persistent HAMT operations on the hot path, and no rehash or `node_get` in seam conversion.

### Task 3 — Measure the representation

**Benchmark:** extend or add a feature-named benchmark under `boot/bench/`; do not name it after an S5 sequence number.

- [ ] Compare the real GC-array target with the existing buffer proxy for build, overwrite, get, remove/reinsert, resize, clone, dense enumeration, seam conversion, and `freeze_dense` publication.
- [ ] Sweep small and large maps and include sparse keys and deliberate full-hash collisions where practical.
- [ ] Keep guards that compare observable contents and insertion order.
- [ ] Report mutation, conversion/compaction, HAMT build, order build, and total publication separately.

### Task 4 — Integrate with the landed publication adapter

- [ ] Produce the exact immutable `array<HamtEntry>` prefix consumed by `freeze_dense`.
- [ ] Call the landed adapter through a compiler-private, source-invisible lowering route; do not expose a public mutable Dict API.
- [ ] Prove direct handoff or the single conversion/compaction pass preserves cached hashes, assigns `order_index = output_position`, and shares no subsequently-mutable arena state with the returned `PDict`.
- [ ] Retain and re-check the published version after ordinary persistent `set` and `remove`; reject or trap post-freeze mutable-handle use according to the S5 lifecycle contract.
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
4. Can the chosen storage expose canonical `HamtEntry` records directly, or is one O(n) conversion/compaction pass required before the landed adapter?
5. Does direct handoff or conversion preserve cached hashes, rewrite `order_index`, and isolate the published `PDict` from mutable storage?
6. Does the measured representation justify proceeding to MutDict region selection, or should S5 stop?
