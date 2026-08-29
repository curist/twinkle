# MutDict Dense Freeze Input Plan

> **For agentic workers:** Use `subagent-driven-development` or
> `executing-plans` to execute this plan with review checkpoints. The first
> executable slice is the isolated representation benchmark in Task 2, not a
> complete MutDict runtime. Do not implement probing, region selection, lowering,
> or lifecycle machinery until Task 3 records and reviews the representation
> decision.

**Status:** S5 representation gate completed and stopped without selection after
the 2026-08-29 evidence-closure run. The immutable publication adapter is landed,
but the real Wasm-GC `MutDict` runtime and its retained storage layout have not
started. Candidate M wins update-dense rows and decisively beats the aliased
persistent control, but the optimized boot-compiler census is dominated by
build-once/low-overwrite maps, where Candidate H wins without justifying a boxed
storage reopening. Task 4 remains closed pending an evidenced update-dense
customer.

**Goal:** Measure the smallest real Wasm-GC storage slice that distinguishes an
unboxed mutable arena followed by one conversion pass from a canonical immutable
`HamtEntry` arena with external liveness. The selected representation must retain
key hashes and produce the landed dense `HamtEntry` publication seam in insertion
order, with at most one conversion/compaction pass and no rehashing.

**Architecture:** `MutDict` remains a compiler-private flat open-addressing
hashmap candidate for hot owned regions. Its mutable storage layout is deliberately
separate from the settled immutable publication seam:
`freeze_dense(dense: Array<anyref>, len)` consumes a hole-free prefix of exact
immutable `HamtEntry` records and builds the persistent HAMT and order vector.
Before building the full hashmap, this plan measures only the arena behavior that
differs between the two layout-C variants. Both candidates use the same stable
entry-ID trace, so the first slice intentionally omits the common open-addressing
index.

**Primary context:**
- [sound-uniqueness/storage/README.md](sound-uniqueness/storage/README.md)
- [sound-uniqueness/storage/spike-tier0-dict.md](sound-uniqueness/storage/spike-tier0-dict.md)
- [archived publication-adapter design](archive/mutdict-bottom-up-publication-adapter.md)
- [archived adapter extraction plan](archive/2026-08-28-mutdict-publication-adapter-extraction.md)
- [bottom-up builder spike](dict-bottom-up-hamt-builder.md)
- [`boot/bench/dict_spike.tw`](../../boot/bench/dict_spike.tw)
- [`boot/bench/mutdict_reads_forks_spike.tw`](../../boot/bench/mutdict_reads_forks_spike.tw)

---

## What is already settled

The Tier-0 buffer proxy established the broad representation tradeoff:

- Flat open addressing wins owned writes by roughly 40–70× and large-map reads
  by roughly 4–12× over the current boxed HAMT.
- Bottom-up construction materially reduces dense flat-to-HAMT publication cost,
  but publication remains O(n).
- Flat snapshots are much cheaper than HAMT conversion when entries can be copied
  safely in bulk.
- Persistent HAMT storage remains decisively better for sparse divergence from a
  live base.
- `run_fixpoint`'s small, sparse-forked transfer maps are not a MutDict customer.

The consumer side is also settled. `freeze_dense` accepts an initialized live
prefix of immutable
`HamtEntry { hash: i64, key: anyref, val: anyref, order_index: i32 }`, builds the
HAMT bottom-up, bulk-builds order, and returns an ordinary persistent `PDict`.
`Dict.compact()` is its first production customer, and engineered-hash tests cover
empty, singleton, deep-prefix, and full-hash-collision shapes.

The remaining question is narrower: which live arena should feed that seam?
Existing linear-memory buffer spikes cannot answer it because they do not measure
Wasm-GC struct mutation, immutable-record replacement, boxing, external liveness,
or safe snapshot cloning.

## Authoritative runtime contract

The first measurement is **boot-runtime-only**. The authoritative files are
`boot/compiler/codegen/runtime/dict.tw` and
`boot/compiler/codegen/runtime/types.tw`. Stage0 `src/runtime/` currently has an
older Dict layout and no compatible `freeze_dense`; it is not part of this spike
and must not be mirrored merely for benchmark parity.

For the Int→Int measurement family:

- Hashing is deterministic wyhash v3 with seed zero through `rt.dict.hash_i64`.
  The full `i64` key is hashed once when the logical entry is created, and the
  cached hash is copied unchanged through publication.
- The isolated arena target compares Int keys with `i64.eq`. This is equivalent to
  `core_eq` for Int. A later generic MutDict must first compare the cached hash and
  then use `rt.core.eq` for key equality, matching persistent Dict semantics.
- The publication seam is physically the existing mutable
  `rt.types.Array<anyref>` whose initialized prefix contains references that cast
  to the exact nominal `rt.types.HamtEntry` type. The notation
  `array<HamtEntry>` in design text is logical shorthand, not a new typed-array
  ABI.
- `freeze_dense` trusts its producer. It does not validate uniqueness, hashes,
  liveness, or order indices and performs no `hash_key`, `node_get`, or
  duplicate-key lookup.

Every producer must guarantee:

- every live key appears exactly once;
- entries are in logical insertion order;
- `order_index == output_position`;
- no empty slots or deletion tombstones occur in the published prefix;
- overwriting an existing key preserves its order position;
- removing and reinserting a key appends it at the end;
- ownership of the backing does not imply ownership of reference-typed keys or
  values;
- no subsequently mutable arena state is reachable from the returned `PDict`.

## Candidate layouts

### Layouts A and B — not part of the first slice

The earlier alternatives remain documented but are rejected for the first
representation measurement:

- **Table slots plus an order-of-key sidecar** requires n extra table probes at
  publication and does not isolate the dense-seam question.
- **Table slots plus an order-of-slot sidecar** couples every index resize to an
  order-sidecar rewrite.

A later session may revisit them only if both layout-C candidates fail. The first
slice does not implement or benchmark either layout.

### Candidate M — unboxed mutable arena plus one conversion pass

The provisional retained baseline follows the storage track's unboxed hot-path
commitment:

```text
MutDictArenaEntryI64 {
  hash: i64       // immutable after insertion
  key: i64        // immutable after insertion
  val: i64        // mutable
  live: i32       // mutable, 0 = dead and 1 = live
}

arena: Array<anyref>  // entries cast to MutDictArenaEntryI64
len: i32              // physical insertion history, including dead entries
live_len: i32
```

Rules:

- The physical arena position is the stable insertion ID.
- Insertion appends one entry with a precomputed hash.
- Overwrite uses `struct.set` on `val` at the existing stable ID. It neither
  appends nor changes insertion order.
- Remove sets `live = 0` and decrements `live_len`.
- Reinsertion appends a fresh live entry at `len` and therefore moves the key to
  the end of logical iteration.
- Publication scans `arena[0..len)` once. For every live entry it boxes the key and
  value, allocates a fresh immutable `HamtEntry`, copies the cached hash unchanged,
  and assigns `order_index = output_position`.
- Publication always requires that one conversion pass, even when there are no
  holes, because the mutable arena entry is a different nominal type and stores
  unboxed fields.
- A snapshot clone must deep-copy every physical mutable arena record. A shallow
  copy would let later `val` or `live` mutation change the observed old version.

### Candidate H — canonical immutable `HamtEntry` arena plus external liveness

This candidate is the boxed zero-copy control:

```text
arena: Array<anyref>  // each initialized entry is an immutable HamtEntry
live: I32Array        // 0 = dead and 1 = live
len: i32              // physical insertion history, including dead entries
live_len: i32
```

Rules:

- The physical arena position is the stable insertion ID.
- Insertion boxes key and value and appends an immutable `HamtEntry` containing
  the cached hash and its physical insertion position.
- Overwrite replaces the array slot **at the same stable ID** with a new immutable
  `HamtEntry`. It reuses the cached hash and boxed key, boxes the new value, and
  preserves the old `order_index`. It must not append-and-remap, because that
  would incorrectly move an overwrite to the end.
- Remove sets the external liveness element to zero.
- Reinsertion appends a new immutable entry and a live marker.
- When all entries in `arena[0..len)` are live, the arena is already the exact
  dense seam and may be passed directly to `freeze_dense`.
- When interior entries are dead, publication performs one fresh dense-copy pass.
  Merely moving references is insufficient because `HamtEntry.order_index` is
  immutable; copied live entries must be recreated with
  `order_index = output_position`.
- A snapshot clone bulk-copies the arena references and liveness array. Immutable
  entries may be shared because later overwrites replace array slots rather than
  mutate entry fields.

Candidate H is not automatically eligible as retained S5 storage. Its key and
value fields are boxed `anyref`, which conflicts with the storage README's settled
unboxed throughput target. If it wins materially, the result is a request to
reopen that commitment and update the storage design explicitly—not permission to
silently adopt boxed hot storage.

## Smallest measurement-first slice

The first executable slice is a throwaway real Wasm-GC arena benchmark, not a
MutDict runtime.

Both candidates consume identical deterministic traces of stable entry IDs,
unboxed Int keys and values, and precomputed `hash_i64` results. Hash preparation
and trace construction are outside timed regions. Arena capacity is preallocated
for each trace so this slice measures entry representation rather than a shared
growth policy.

The slice intentionally excludes:

- open-addressing slots, probing, table tombstones, and index resize;
- generic key equality and String keys;
- source-level region discovery or ownership proofs;
- compiler lowering and private builtin registration;
- the consumed-handle shell and runtime poison;
- public mutable Dict APIs;
- stage0 runtime parity.

These exclusions are deliberate. They are common or later concerns and would
turn the representation gate into the full S5 runtime.

### Workloads

Use sparse deterministic Int keys and run at 65K and 1M live-entry scales. If a
phase is below useful timer resolution, repeat it over fresh arenas rather than
relying on the smaller row.

1. **Construction**
   - Fill a preallocated arena from identical key/value/hash streams.
   - Candidate M allocates unboxed mutable records.
   - Candidate H boxes keys and values and allocates immutable `HamtEntry`
     records.

2. **Dense overwrite**
   - No deletions.
   - Measure `k/n` in `{1/8, 1, 4}` with the same deterministic stable-ID stream.
   - Candidate M mutates `val` in place.
   - Candidate H boxes the replacement value and replaces the immutable record at
     the same array position.

3. **Interior holes plus remove/reinsert**
   - Remove every fourth original entry, then reinsert those keys in removal
     order with new values.
   - Final live size remains n; physical history becomes 1.25n.
   - Expected iteration order is surviving original keys followed by reinserted
     keys in removal order.
   - Both candidates compact at publication and rewrite final order indices.

4. **Observed-old-version clone**
   - Clone the same dense and churned physical states before further writes.
   - Candidate M deep-copies mutable records.
   - Candidate H bulk-copies entry references and external liveness.
   - Mutate the clone and prove the retained base remains unchanged.

5. **Persistent control**
   - In the same process, apply equivalent logical traces to an aliased persistent
     `Dict` so the persistent path cannot flip in place.
   - Report it as an end-to-end control, not as an arena-layout phase comparison.

### Timing boundaries

Use one warmup followed by at least three measured samples, rotate candidate and
builder order, keep GC enabled, and report raw ranges plus medians without
subtracting outliers.

Report separately:

- `construct_ms`;
- `overwrite_ms`;
- `remove_reinsert_ms`;
- `clone_ms`;
- `seam_ms`:
  - Candidate M: scan, dead-entry dropping, boxing, and immutable-entry allocation;
  - Candidate H dense: zero/direct handoff;
  - Candidate H churn: liveness scan, compaction, and corrected immutable entries;
- `hamt_ms` for bottom-up root construction;
- `order_ms` for bulk order construction;
- `publication_ms = seam + hamt + order`;
- `total_ms` for each complete workload;
- same-session persistent-control total.

Correctness scans, trace generation, hash preparation, printing, and source-level
Vector preparation remain outside timers. Allocation and GC caused by candidate
operations remain inside because they are part of the representation cost.

### Correctness and emitted-shape guards

Outside all timers, require:

- exact final live length;
- every expected key maps to its final expected value;
- an equally large disjoint key range is absent;
- `keys()` exactly matches expected insertion order;
- both candidate publications are observationally identical;
- clone mutation leaves the retained base unchanged;
- persistent `remove` after churn removes the correct order slot, proving compacted
  `order_index` values are fresh;
- persistent `set` and `remove` on the result do not mutate the retained published
  version.

Inspect emitted WAT and reject the measurement if:

- Candidate M's hot overwrite allocates `HamtEntry` or lacks `struct.set` on the
  mutable value field;
- Candidate H mutates an `HamtEntry` field instead of allocating and replacing a
  whole record;
- seam preparation calls `hash_key`, `hash_i64`, `node_get`, `node_set`, or a
  linear-memory Buffer operation;
- publication bypasses the landed bottom-up builder or bulk order builder;
- cached hashes are recomputed rather than copied from arena records.

## Representation decision gate

Task 3 must publish the raw results and a cost model over update density, deletion
fraction, and clone frequency. At minimum report the dense-update crossover:

```text
k_cross =
  (mutable_seam - canonical_seam)
  / (canonical_per_update - mutable_per_update)
```

Do not encode this value as a compiler threshold from the arena spike.

A future session decides as follows:

- **Retain Candidate M** only if its unboxed hot-operation savings amortize its
  conversion and deep-clone costs on the update-dense rows, its end-to-end result
  beats the same-session persistent control, and the winner is stable at 1M.
- **Request an explicit design reopening for Candidate H** only if its boxed
  zero-copy/cheap-clone result wins end-to-end materially outside sample noise on
  the target rows. Measurement alone does not override the unboxed storage-track
  commitment.
- **Stop without selecting either layout** if the winner reverses between dense,
  churn, and clone workloads or remains within noise. Obtain a real-program census
  of overwrites, removals, publication boundaries, and flat-preserving forks before
  implementing both layouts or guessing.
- **Stop S5 runtime expansion** if neither candidate beats persistent Dict
  end-to-end in the update-dense cases that are supposed to justify MutDict.

This Int-only slice proves that cached hashes survive publication but cannot
quantify the benefit for strings. A focused String→Int cached-hash-versus-rehash
measurement is a later follow-up, not part of the arena choice.

## Quick-spike result (2026-08-29)

The quick spike exercised the two real Wasm-GC arena layouts at 65K and 1M, with
rotated candidate order and post-warmup sampling. The decision evidence below
combines the two rotated runs at 1M. Each cell is the median followed by the full
observed range in milliseconds. Every `clone` timing below is a
**pre-churn-base clone**: the benchmark clones the original dense `n`-entry arena,
then applies overwrite and churn to that clone. It does not measure cloning the
resulting 1.25n-entry churned physical arena. Pre-publication includes that dense
base clone; it excludes `freeze_dense`, which is common work after the
representation seam.

| workload / phase | Candidate M | Candidate H |
|---|---:|---:|
| dense 1x clone | 27.62 (18.68–31.48) | 1.02 (0.90–1.07) |
| dense 1x overwrite | 1.29 (1.24–1.31) | 31.01 (21.50–44.48) |
| dense 1x seam | 45.04 (26.92–78.73) | 0 direct |
| dense 1x pre-publication incl. clone | 73.82 (53.32–110.20) | 31.98 (22.50–45.52) |
| dense 4x clone | 25.65 (22.08–31.50) | 1.08 (1.00–1.12) |
| dense 4x overwrite | 5.00 (4.82–6.39) | 106.06 (95.23–177.70) |
| dense 4x seam | 28.34 (27.23–38.86) | 0 direct |
| dense 4x pre-publication incl. clone | 62.10 (54.36–69.61) | 107.13 (96.34–178.70) |
| churn 1x pre-churn-base clone | 28.90 (13.63–35.68) | 1.29 (1.12–1.33) |
| churn 1x overwrite + churn + seam, no clone | 38.08 (30.41–76.66) | 57.09 (49.26–116.77) |
| churn 1x pre-publication incl. pre-churn-base clone | 64.17 (44.05–112.34) | 58.38 (50.58–117.99) |

Candidate M's in-place overwrite is about 20–24× faster at the stable medians in
these real GC layouts. Candidate H's clone is roughly 20–30× cheaper because it
shallow-copies immutable entries. H also has a genuinely zero dense seam, whereas
M must make one boxing and immutable-record conversion pass. Consequently H wins
clearly with one full overwrite and one flat clone, while M wins clearly at four
overwrites per entry. The crossover lies inside the measured 1x–4x interval, but
the GC-sensitive samples do not support a precise threshold.

The delete/reinsert workload makes both candidates compact. The measured dense
base clone erases or reverses M's pre-clone lead within the overlapping ranges,
but this is not the required clone of the 1.25n churned physical state; that cost
remains missing. `freeze_dense` then performs semantically common
bottom-up HAMT and bulk-order work. Its combined medians are only a sanity check,
not a representation advantage: dense 1x M 92.56 ms versus H 95.05 ms; dense 4x
M 93.44 ms versus H 93.30 ms; churn M 93.74 ms versus H 96.13 ms. Candidate order
was rotated, but GC and coarse timer effects remain visible in the ranges; no
outliers were removed.

All measured rows passed exhaustive value and equally sized absence checks,
exact insertion-order checks, clone isolation, and post-publication persistent
version guards. After churn publication, persistent removal also checks the
shortened length and exact remaining key order for both candidates, using the
first reinserted key so a stale pre-compaction `order_index` cannot pass.
Emitted-WAT review confirmed that Candidate M overwrite uses
`struct.set`, Candidate H overwrite replaces an exact immutable `HamtEntry`, seam
conversion does not hash or probe, and both paths call `freeze_dense`.

**Reviewed verdict: STOP WITHOUT SELECTION.** This quick spike materially
fortifies the tradeoff but is not decision-complete. The missing-evidence
inventory is explicit: timed construction; k/n=1/8 at both scales; clone cost for
the 1.25n churned physical state; separate HAMT-build, order-build, publication,
and complete-workload totals; and the same-session persistent control. String
keys also remain unmeasured. Candidate M and Candidate H therefore remain open,
and Task 4 stays hard-gated. Reopening requires that evidence plus a real-program
census of overwrite density, removals, publication boundaries, and flat-preserving
forks, or more decision-specific measurements.

## Evidence closure and boot-compiler census (2026-08-29)

The follow-up completed the quick spike's missing inventory. Key/hash streams are
prepared before timers; construction, overwrite, churn, post-churn physical clone,
seam, bottom-up HAMT build, bulk order build, and complete no-clone workload totals
are reported separately. The same process also runs an equivalent persistent
trace while retaining the original base, forcing later operations down the
persistent path. Candidate order remains rotated, with one warmup and three raw
samples per row.

`target/twk ir boot/main.tw --opt --census --sites` reports 777 static `dict_set`
and 25 `dict_remove` candidates across the linked optimized compiler. There are
449 selected `dict_set` rows across the census's optimized/specialized views;
this is a static site inventory, not a dynamic execution count. CFG/liveness
inspection shows the representative selected shape:

- `build_type_remap`, `local_map_from_slots`, and `build_trivia_map` carry fresh
  `Unique` maps through loops and insert at most once per qualifying input item;
- `compute_pinned` builds fresh set-like maps, with repeated writes only when
  inputs repeat;
- cache helpers such as `table_put` and `fix_cache_put` retain aliased record
  shells/fields and correctly stay persistent;
- selected removals and flat-preserving forks are not a material current pattern.

The current compiler workload is therefore construction-heavy and predominantly
build-once or low-overwrite. It does not supply evidence for treating the 1x–4x
overwrite rows as the default S5 customer.

The table below reports the final 1M-entry aggregation as median followed by the
full observed range in milliseconds. `total` excludes clone because the census
found no flat-preserving fork; dense and churned clone costs remain separate
decision inputs. Publication is `seam + HAMT + order`.

| workload / phase | Candidate M | Candidate H | aliased persistent total |
|---|---:|---:|---:|
| dense 1/8x construct | 24.84 (20.77–27.18) | 44.15 (37.03–44.53) | — |
| dense 1/8x overwrite | 0.14 (0.14–0.22) | 0.38 (0.37–0.65) | — |
| dense 1/8x seam | 37.06 (27.76–47.99) | 0 direct | — |
| dense 1/8x HAMT + order | 99.80 | 92.35 | — |
| dense 1/8x total | 163.98 (152.54–172.19) | 136.55 (128.92–149.04) | 740.00 (639.36–844.67) |
| dense 1x total | 158.90 (144.41–166.00) | 189.32 (162.85–274.86) | 1532.25 (1393.33–1620.45) |
| dense 4x total | 151.64 (150.28–222.20) | 299.80 (229.18–309.87) | 3829.95 (3684.39–4091.24) |
| churn 1x post-churn clone | 30.61 (30.31–40.35) | 1.38 (1.32–1.50) | — |
| churn 1x total | 164.28 (150.10–195.52) | 195.90 (181.91–240.93) | 1761.00 (1730.44–1875.42) |

Candidate M clears the persistent control by a wide margin and wins the 1x, 4x,
and churn medians. Its unboxed construction and overwrite advantages therefore
remain credible for an update-dense region. Candidate H wins the representative
1/8x row materially because its direct dense handoff avoids M's conversion pass;
it also retains the much cheaper clone. GC-sensitive builder and seam ranges are
still visible, so these samples are evidence for workload classes, not a numeric
compiler threshold.

All rows passed exhaustive value/absence/order checks, retained-base checks,
post-churn clone isolation, compacted-order removal, and post-publication
persistence checks. Emitted WAT confirms precomputed hashes feed construction,
M overwrite uses `struct.set`, H overwrite replaces immutable `HamtEntry`
records, seams neither hash nor probe, and publication invokes the bottom-up HAMT
and bulk-order builders.

**Final reviewed verdict: STOP WITHOUT SELECTION.** Candidate M satisfies the
technical update-dense gate, but the present boot compiler does not evidence that
workload. Candidate H's win on the observed build-once/low-overwrite class is not
permission to reopen boxed hot storage; those maps should remain on the existing
persistent/in-place HAMT path. Reopen S5 only with a concrete update-dense region
whose measured operations and publication boundary match the rows where M wins.

## Scope

This plan covers:

- the isolated real Wasm-GC arena representation gate;
- cached deterministic Int hashes;
- overwrite, remove/reinsert, clone, and ordered dense publication behavior;
- direct handoff versus one conversion/compaction pass;
- selection or explicit rejection of the retained storage element;
- a later minimal open-addressing runtime only after that selection.

It does not cover:

- selecting source regions for MutDict;
- ownership or freshness proofs;
- interprocedural mutable ABI routing;
- redesigning the landed bottom-up HAMT builder or `freeze_dense` adapter;
- changing the public `Dict<K,V>` representation;
- making flat Dict the language default.

## Work outline

### Task 1 — Settle the representation experiment contract

- [x] Name the boot runtime as the authoritative measurement target.
- [x] Record exact Int hash and equality behavior.
- [x] Specify Candidate M's mutation, deletion, publication, and clone rules.
- [x] Specify Candidate H's same-position overwrite, external liveness, direct
  handoff, compaction, and clone rules.
- [x] Classify Candidate H as a boxed control that requires an explicit design
  reopening before retained adoption.
- [x] Defer final layout selection until after measurement.

**Deliverable:** this document. Task 1 does not select the retained layout.

### Task 2 — Build the isolated dual-arena Wasm-GC benchmark target

**Likely files:**
- Modify `boot/compiler/codegen/runtime/types.tw` for the benchmark-only mutable
  arena record.
- Add focused benchmark-only functions under
  `boot/compiler/codegen/runtime/dict.tw` or a dedicated runtime benchmark module.
- Add a feature-named benchmark under `boot/bench/`; do not name it after an S5
  sequence number.

- [x] Add behavior tests first for overwrite order, remove/reinsert order, dense
  handoff, hole compaction, clone isolation, and post-publication persistence.
- [x] Implement only the two arena candidates and stable-ID operations described
  above.
- [x] Reuse the landed `freeze_dense` tail.
- [x] Keep all benchmark-only APIs source-invisible where practical and clearly
  marked for cleanup.
- [x] Inspect emitted WAT against the shape guards before accepting timings.

### Task 3 — Measure and record the representation decision

- [x] Run dense overwrite, churn, pre-churn-base clone, seam, and combined
  `freeze_dense` work in the same process.
- [x] Add timed construction, k/n=1/8 rows, a churned-physical-state clone, and a
  same-session persistent-control workload.
- [x] Split `freeze_dense` into HAMT-build and order-build timings and report
  publication and complete-workload totals separately.
- [x] Keep exhaustive content, absence, insertion-order, clone-isolation,
  persistent-remove-order, and persistent-version guards.
- [x] Record combined raw ranges, medians, the bounded crossover, and GC/timer
  caveats.
- [x] Stop without selection according to the gate above.
- [x] Add the result and rationale to this document and update the storage README.

**Hard gate:** a stop verdict keeps Task 4 closed. Do not begin Task 4 until the
representation gate is explicitly reopened, every missing-evidence item above is
recorded together with the workload evidence required by the verdict, and an
explicit reviewed verdict selects one retained layout.

### Task 4 — Build the minimal retained open-addressing MutDict runtime

This task is intentionally deferred and must be rewritten around Task 3's selected
layout before execution.

- [ ] Specify index capacity, load factor, empty marker, probe sequence, table
  tombstones, resize, and compaction policy.
- [ ] Add flat get/set/remove behavior using cached hashes and persistent-compatible
  equality.
- [ ] Add arena/index growth and clone behavior.
- [ ] Confirm the selected representation still behaves as measured once probing
  and resizing are present.
- [ ] Stop if index integration erases the measured end-to-end advantage.

### Task 5 — Integrate the retained runtime with publication lowering

- [ ] Produce the exact immutable `HamtEntry` prefix consumed by `freeze_dense`.
- [ ] Call the landed adapter through a compiler-private, source-invisible route.
- [ ] Prove direct handoff or the single conversion pass preserves hashes, rewrites
  order indices, and shares no subsequently mutable state with the returned
  `PDict`.
- [ ] Add compile-time consumed-handle validation and assertion-grade runtime
  poison according to the S5 lifecycle contract.
- [ ] Retain and re-check published versions after ordinary persistent operations.
- [ ] Remove temporary benchmark surfaces only after retained runtime evidence is
  recorded.

## Validation

For documentation-only Task 1 changes, check links and internal consistency; no
compiler build is required.

For retained Twinkle runtime changes in later tasks:

```bash
target/twk fmt <changed-.tw-files>
target/twk lint boot/main.tw
make stage2
make quick-bundle-cli
target/twk test
```

Also inspect relevant runtime calls with `target/twk wat ... --calls`. Preserve the
known lint baseline; the acceptance condition is no new finding from this work.
Do not run tree-sitter tests.

## Decision output

The future measurement session must answer:

1. What are construction and overwrite costs for mutable unboxed records versus
   boxed immutable-record replacement?
2. What does Candidate M's mandatory conversion cost, with and without holes?
3. What do Candidate H's direct dense handoff and deletion-triggered compaction
   cost?
4. How large is the deep-clone versus shallow-clone difference for observed old
   flat versions?
5. Where are the measured crossovers over update density, deletion fraction, and
   clone frequency?
6. Does either complete arena→`freeze_dense` path beat persistent Dict on the
   intended update-dense workloads?
7. Is Candidate M retained, is Candidate H strong enough to justify reopening the
   unboxed commitment, or should S5 stop before full MutDict implementation?
