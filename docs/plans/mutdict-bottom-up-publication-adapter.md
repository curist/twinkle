# MutDict Bottom-Up Publication Adapter — Design

> **For agentic workers:** This is a settled *design*, not an implementation
> ticket. Do not write production code from it directly. After approval, spin a
> separate test-first implementation plan (`executing-plans` /
> `subagent-driven-development`) with review checkpoints. The spike surfaces named
> under "Migration and cleanup" must survive until their replacement lands, then
> be removed as a unit.

**Status:** Design proposed. The bottom-up builder core, dense entry type, and a
first customer (`Dict.compact()`) already exist from the spike; this document
settles the compiler-private *interface* that turns that core into the MutDict
publication adapter and the eventual `Dict.compact()` implementation.

**Goal:** Specify a compiler-private adapter that materializes a dense,
cached-hash, insertion-ordered stream of live entries into an ordinary immutable
`PDict` in one bottom-up pass, with no scratch or subsequently-mutable builder
state reachable from the result, observationally equivalent to sequential
persistent `Dict` construction.

**Shape:**

```text
MutDict dense storage
  -> build_hamt_bottom_up(dense)     // exact-sized HAMT, no per-entry trie walk
  -> build_order_bulk(dense)         // insertion order in one pass
  -> ordinary immutable PDict
```

**Primary context:**
- [dict-bottom-up-hamt-builder.md](dict-bottom-up-hamt-builder.md) — spike result and crossover calibration
- [mutdict-dense-freeze-input.md](mutdict-dense-freeze-input.md) — the dense freeze input / layout-C recommendation
- [sound-uniqueness/storage/spike-tier0-dict.md](sound-uniqueness/storage/spike-tier0-dict.md)
- [sound-uniqueness/storage/README.md](sound-uniqueness/storage/README.md) — S5 track and lifecycle contract
- `boot/compiler/codegen/runtime/dict.tw`, `boot/compiler/codegen/runtime/types.tw`

---

## Decisions settled up front

Three interface forks were resolved before drafting; the rest of the document
follows from them.

1. **Dense element = reuse `HamtEntry`.** The dense freeze contract element is the
   existing `HamtEntry { hash: i64, key: anyref, val: anyref, order_index: i32 }`.
   It is structurally the `DenseFreezeEntry` of
   [mutdict-dense-freeze-input.md](mutdict-dense-freeze-input.md) *and* is exactly
   what HAMT leaves store, so a singleton bucket emits its entry as a leaf with
   zero conversion. Note that the *builder input* is `array<HamtEntry>`; the MutDict
   arena element is not literally `HamtEntry` (it needs a mutable `val` and a `live`
   flag — see §7), so producing the dense array from the arena is one field-copy
   pass, not a pointer handoff, except in the special case where the arena stores
   canonical immutable `HamtEntry`.
2. **Naive scratch behind a stable outer entry point.** The recursion keeps
   recursion-local counting-partition scratch (proven in the spike). The only
   stable interface is the dense→`PDict` entry point; the recursion signature is
   private to `rt.dict` and may change freely. Ping-pong scratch is a
   profiling-gated, contained follow-up because the recursion has no callers
   outside the module.
3. **Compile-time authority + assertion-grade runtime poison.** Compile-time
   ownership/region analysis is the *sole authority* that licenses the freeze and
   proves the mutable handle dead afterward. In addition, freeze poisons the
   consumed MutDict handle so a stray post-freeze operation traps. The poison is a
   **defensive assertion, not a second legality path**: a fired poison indicates a
   compiler bug, never a supported runtime fallback, and it must be compile-out-able
   if its field/branch cost ever matters on the hot path. This preserves the track
   invariant "ownership facts and codegen decisions remain the only authority for
   destructive mutation."

## Now-implementable vs forward contract

MutDict does not exist yet (S5 runtime is unbuilt). The adapter therefore splits
cleanly:

- **Implementable now** (§1, §2, §5, §6, §8, and the `Dict.compact()` slice of §7,
  §9): the shared builder tail — `build_hamt_bottom_up` + `build_order_bulk` behind
  a single `freeze_dense` entry point — with `Dict.compact()` as its first and only
  customer. The dense array is synthesized by `compact()` itself.
- **Forward contract** (§3 workspace evolution, §4 lifecycle/poison ABI, the
  MutDict-consumption half of §7): the ABI that the future S5 MutDict lowering
  targets. Specified here so the builder tail is shaped to receive a real MutDict
  dense stream through at most one conversion pass (zero in the layout-(a)
  hole-free case, §7), but not landed until MutDict exists.

The dense `array<HamtEntry>` is the seam between the two halves. Both `compact()`
and future MutDict freeze produce that array their own way and call the identical
shared tail.

---

## 1. Dense input representation

**Logical contract.** A proven-owned publication input enumerates every live
entry exactly once, in insertion order, as `HamtEntry`:

```text
HamtEntry { hash: i64, key: anyref, val: anyref, order_index: i32 }
```

Invariants the producer must guarantee (unchanged from
[mutdict-dense-freeze-input.md](mutdict-dense-freeze-input.md)):

- owns every live *unique* key and value (ownership of the backing, not of the
  reference-typed keys/values, which may stay shared);
- `hash` is the same deterministic hash persistent `Dict` uses (`hash_key`), never
  recomputed by the builder;
- `order_index == array position`, contiguous `0..len`;
- no empty slots and no deletion tombstones;
- directly enumerable without `node_get` and without rehashing.

**Physical representation.** A single dense `array<HamtEntry>` (`rt_types__Array`
of `HamtEntry`), no holes, `len` live entries. Chosen over parallel typed arrays
because HAMT leaves *are* `HamtEntry`: a singleton bucket emits its entry as a leaf
with zero conversion, so the builder never reshapes. Parallel typed arrays would
save the per-entry struct allocation but require a leaf-materialization pass at
every singleton and index-permutation partitioning — trading a paid-once allocation
for pervasive extra work on the hot construction path. This consciously supersedes
[mutdict-dense-freeze-input.md](mutdict-dense-freeze-input.md)'s "parallel typed GC
arrays … likely preferable" lean: that lean was about the *arena/storage* layout,
whereas the deciding factor here is the *builder input*, where matching the leaf
type wins.

The `order_index` field is redundant with array position for a fully dense input
and is retained only because (a) `HamtEntry` already carries it and (b) it lets an
arena that is *not yet* compacted still carry stable order through a compaction
pass (see §7).

## 2. Compiler-private interfaces

All names below are `rt.dict`-internal emitted Wasm functions. **None are public
`Dict` methods and none get a prelude signature**, so source code cannot name
them. They have two distinct *call routes*, which the doc previously conflated:

- **Now / `compact()` route — no builtin.** `compact()` is itself an `rt.dict`
  function, so it calls `freeze_dense` / `build_hamt_bottom_up` / `build_order_bulk`
  directly by runtime symbol, exactly as the runtime already calls `node_set` /
  `node_get`. No builtin id, no ABI declaration, no `rt(...)` registration is
  needed for this route.
- **Forward / MutDict-lowering route — private builtin, source-invisible.** When
  the S5 MutDict lowering must *emit* a freeze call from compiled user code, it
  needs a builtin the backend can target. This follows the established
  compiler-private-op precedent, e.g. `vector$__mutvec_freeze_i64`
  (`builtins.tw`): a private builtin id such as `dict$__freeze_dense` with an ABI
  declaration and an `rt(...)` registration mapping it to the runtime symbol, and a
  **`.None` prelude method name** so it is emittable by lowering but never
  source-callable. Still no prelude signature file entry. The exact id and ABI are
  settled with the S5 lowering, not here.

**Stable outer entry point** (the only signature callers depend on):

```text
freeze_dense(dense: array<HamtEntry>, len: i32) -> PDict
  // Precondition:
  //   0 <= len <= dense.length
  //   dense[0..len) are initialized, live, core_eq-unique HamtEntry
  //   dense[len..] is ignored (an over-allocated arena passes a live prefix
  //   without slicing; compact() already relies on this, passing j < n).
  // root = len == 0 ? null : build_hamt_bottom_up(dense, 0, len, 0)
  // order = build_order_bulk(dense, len)
  // PDict { size: len, root, order, tombstones: 0 }
```

**Private builder core** (may change signature freely; no external callers):

```text
build_hamt_bottom_up(dense: array<HamtEntry>, lo: i32, hi: i32, depth: i32) -> HamtNode
  // exactly the current node_build_bottom_up: counting-partition [lo,hi) by the
  // depth's 5-bit hash fragment, exact-sized bitmap + entries array per node,
  // singleton -> leaf HamtEntry, depth>=12 multi -> HamtCollision, else recurse.
```

`node_build_bottom_up` is renamed to `build_hamt_bottom_up` at productionization
(or kept — naming is cosmetic; the point is it is private). Its recursion-local
scratch is unchanged.

**Private order builder:**

```text
build_order_bulk(dense: array<HamtEntry>, len: i32) -> PVec
  // keys = fresh array<anyref>[len]; keys[j] = dense[j].key; arr_from_array(keys)
```

This subsumes the parallel `keys` array `compact()` currently maintains: order
keys are pulled from `dense[j].key`, since dense is already in insertion order.

**Fits the naive partitioner behind a workspace-compatible interface how?** The
outer `freeze_dense` signature is stable and workspace-free. When ping-pong is
justified (§3), the workspace is allocated inside `freeze_dense` and threaded into
a new private `build_hamt_bottom_up` arity — a change contained to `rt.dict` with
no effect on `freeze_dense`'s signature or on any caller. The naive partitioner
therefore already *is* "behind" the stable interface.

**Does `Dict.compact()` adapt into the same interface?** Yes. `compact()` becomes
the first `freeze_dense` customer: it synthesizes the dense `array<HamtEntry>` from
its order sidecar (its current per-entry `node_get` value-recovery loop, which is
also the tombstone-dropping compaction), then calls `freeze_dense(dense, live)`.
This keeps the builder continuously exercised and regression-tested before MutDict
lands. `compact()`'s dense *preparation* (the `node_get` recovery) is synthetic
seam cost and is **not** part of real MutDict publication cost — the two must be
reported separately (§7).

## 3. Workspace ownership

**First implementation: recursion-local scratch.** `build_hamt_bottom_up`
allocates its own `counts` / `cursor` / `sorted` arrays per node, as the spike
does. No workspace parameter, no shared buffers. This is the proven, correct form
and the starting point.

**Who allocates counts/cursors/partition scratch:** the builder, per recursive
call, from GC. `freeze_dense` allocates only the final `order` keys array and the
`PDict`.

**Introducing ping-pong without changing semantics.** A future optimization may
allocate two reusable scratch buffers once in `freeze_dense` and thread them
through the recursion, partitioning into the inactive buffer and swapping. Because
`build_hamt_bottom_up` has no callers outside `rt.dict`, this is a localized
signature change (add a workspace operand) invisible to `freeze_dense`'s callers.
Semantics are identical: partition order, node shape, and leaf/collision rules are
unchanged; only the *allocation source* of scratch moves from per-node-fresh to
shared-reused.

**Gate:** no ping-pong is implemented until allocation-volume, peak-live-memory,
and GC-time profiling on the real MutDict dense stream justifies it. The spike
already shows the naive builder captures the modeled slot-copy reduction and is
the dominant win; ping-pong is a constant-factor follow-up, not a prerequisite.

## 4. Publication lifecycle

*(Forward contract — realized when S5 MutDict exists. `Dict.compact()` satisfies
it trivially today because it owns the dense array it builds and touches no
external mutable handle.)*

**Publication consumes the mutable handle.** At the selected freeze boundary,
`freeze_dense` receives the MutDict's dense stream (arena arrays or a compacted
copy — §7) and returns a `PDict`. After that point the MutDict handle is
**consumed**: no reads, no writes, no second publication, no clone.

**How validity is represented — two layers, one authority:**

- **Compile-time (authoritative).** The storage/region selector (S5, per the
  README lifecycle contract) proves the handle is dead after the freeze site: no
  path from freeze reaches any use of the handle, no double-publication, no
  use-after-materialize. This is the *only* thing that licenses the freeze. If the
  proof is unavailable, the site does not freeze — it falls back to the ordinary
  persistent path (§ "persistent fallback").
- **Runtime poison (assertion-grade defense).** `freeze` sets a `consumed` marker
  on the MutDict shell and drops its references to the backing arrays (so the
  published `PDict` is the sole owner of nothing mutable — see §5). A subsequent
  MutDict operation on a consumed handle **traps**. This never fires in correct
  compiler output; it is a defensive assertion catching a codegen bug, not a
  supported runtime path and not a fallback. It must be structured so it can be
  compiled out (release builds) if the field/branch cost is ever measurable on the
  hot path. It does **not** constitute a second legality path: it cannot *license*
  a mutation and cannot *change* observable behavior of correct programs.

**Runtime ABI sketch** (MutDict shell, illustrative — settled with the S5 runtime,
not here):

```text
MutDict { ..backing.., consumed: i32 }
freeze(mut): assert consumed == 0; build dense; consumed = 1; backing = null; -> PDict
get/set/remove/clone(mut): if consumed trap "use after freeze"
```

**Persistent fallback.** When compile-time proof, a compatible storage/ABI route,
runtime support, or a safe freeze point is unavailable, the region never enters
MutDict form: the value stays an ordinary persistent `PDict` and all operations
use the existing `node_set` / `node_remove` paths. Fallback is mandatory, per the
track invariants.

## 5. Isolation and immutability

The published `PDict` must be fully persistent and share nothing *mutable* with the
builder or the consumed handle. The precise invariant is **not** "no builder-owned
arrays reachable" — finalized `HamtNode.entries` slot arrays are builder-created
*and* reachable, which is correct. It is: **no scratch and no subsequently-mutable
state is reachable from the result.**

- **Finalized nodes are immutable-after-fill.** `HamtNode.entries` and `.bitmap`
  are mutable-*typed* GC fields, but every node the builder emits is filled exactly
  once at final occupancy in the `sl` loop *before* its single `StructNew` and
  never mutated afterward. No `counts` / `cursor` / `sorted` scratch array is
  reachable from any emitted node; scratch is dropped as recursion unwinds.
- **Immutable entry records may be shared, and are.** The builder installs the
  *same* `HamtEntry` references from the dense array into singleton leaves and
  `array.copy`s those references into collision buckets — it does **not** copy
  `hash`/`key`/`val`. This is sound because `HamtEntry` fields are immutable
  (`mutable: false`): a shared entry cannot be observed to change. (Where the dense
  array is itself materialized from a mutable arena — §7 — that one conversion pass
  produces *fresh* immutable `HamtEntry`; the sharing above is of those fresh
  records, never of arena slots.)
- **Keys and values may remain shared references** — ownership of the backing does
  not imply ownership of reference-typed keys/values. The result holds the same
  key/value references; that is intended structural sharing, not mutable leakage.
- **The MutDict's mutable arena, scratch, and mutable shell must not be reachable**
  from the `PDict`. Freeze drops the shell's backing references (§4). This depends
  on MutDict updates **replacing** whole entries rather than mutating fields of a
  shared entry record — otherwise a post-freeze in-place field write could be
  observed through a shared leaf. That constraint is part of the §7 storage/seam
  boundary, not an assumption the builder can make on its own.
- **The `order` PVec is freshly built** from a fresh keys array via
  `arr_from_array`; it aliases no MutDict order sidecar.
- **Post-publication `Dict.set` / `Dict.remove` follow ordinary persistent paths.**
  They operate on the returned `PDict` via `node_set` / `node_remove`, path-copying
  as usual, and never mutate the published root. The published root is an ordinary
  immutable HAMT indistinguishable from one built by sequential `set`.

## 6. Collision and ordering semantics

**The contract is observational equivalence to sequential persistent construction
— every live key maps to the same value, size matches, iteration order matches, and
same-full-hash unequal keys stay distinct — not bit-identical tree shape.** The
emitted tree *does* currently coincide with `node_set`'s shape (see below), but that
is an incidental property of today's `node_set`, not the promised invariant; a
future `node_set` change must break tests only if it changes *observable* behavior.

- **Singleton nodes.** A bucket with one entry emits that `HamtEntry` as a leaf in
  the parent's compressed slot — never a one-child intermediate node.
- **Deep shared prefixes.** Entries sharing hash fragments recurse to increasing
  depth, one intermediate `HamtNode` per shared fragment. This currently matches
  `node_set`: on a non-equal-key slot collision at `depth < 12`, `node_set` builds
  an intermediate node and **re-inserts both entries at depth+1** (dict.tw:1311–
  1382), so equal-full-hash keys chain down to `depth >= 12` there too — it does
  *not* short-circuit to a collision at the first equal hash. Bottom-up reproduces
  the same chain.
- **Full-hash unequal-key collisions.** When all 64 hash bits are consumed
  (depth ≥ 12) and a bucket still holds multiple entries — i.e. equal full hash but
  **`core_eq(k1, k2) == false`** — emit a `HamtCollision` retaining every distinct
  key. Distinctness is by `core_eq`, not reference identity: two distinct references
  with structurally equal contents are the *same* key and collapse to one entry.
  Collision bucket order is the dense (insertion) order, matching `node_set` +
  `collision_set`, which append in insertion order.
- **Updates preserve original insertion position.** An updated key keeps its
  `order_index`; the producer overwrites the value in place in the dense stream,
  not appends.
- **Remove/reinsert appends at the end.** A removed-then-reinserted key gets a new
  trailing `order_index`; the dense stream reflects the producer's order sidecar.
- **Deterministic iteration.** `keys()` order from the built `PDict` matches
  sequential persistent construction, because `order` is `dense[j].key` in position
  order.

These are the correctness oracle for §8.

## 7. Representation integration

**Three concerns are kept separate** (this doc settles only the middle one now):

1. **The hot MutDict *storage element*** — flat, unboxed, in-place-updatable. This
   is S5's performance target and is **not** settled here. `HamtEntry` is explicitly
   *not* proposed as the storage element: its `key`/`val` are immutable `anyref`
   (`types.tw:149-150`), so using it as live storage would box primitives and force
   whole-record replacement on every update — the opposite of the flat/unboxed goal.
   Settling the storage element is benchmarked in S5, or a conversion pass is paid.
2. **The publication *seam*** — an immutable `array<HamtEntry>`, `dense[0..len)`.
   This doc settles this. It is the builder's sole input contract.
3. **The MutDict-consuming *freeze wrapper*** — the forward `dict$__freeze_dense`
   lowering entry (§2) that takes a MutDict handle, produces the seam array, calls
   `freeze_dense`, and consumes the handle (§4).

Reconciling the seam with layout-C in
[mutdict-dense-freeze-input.md](mutdict-dense-freeze-input.md) (stable dense entry
arena + open-addressing index):

- **Arena element is not structurally `HamtEntry`.** Layout-C's element is
  `{hash, key, val, live}` with a **mutable** `val` (for in-place update) and a
  `live` flag (for dead-marking). That is a distinct nominal Wasm struct; an
  `array<ArenaEntry>` cannot be passed where the builder casts to `HamtEntry`. So
  the seam is produced from the arena by **one conversion pass**, not a pointer
  handoff. Two layouts make the pass cheap or avoidable — pick in S5 by measurement:
  - **(a) Canonical `HamtEntry` arena + external liveness.** Store exact immutable
    `HamtEntry` records plus a separate liveness bitmap / nullable slots. Then a
    hole-free live prefix *is* `array<HamtEntry>` and can be consumed with (near-)
    zero copy — but immutable `val` means an update must **replace** the whole entry
    record (append-and-remap), never mutate a field. This keeps §5's sharing sound.
  - **(b) Mutable arena record + one conversion pass.** Store the mutable
    `{hash,key,val,live}` record for cheap in-place updates; at publication, copy
    live entries into a fresh `array<HamtEntry>`. This is the baseline assumption.
- **Dead entries force compaction into the conversion pass.** Interior dead entries
  (holes) are dropped during the conversion/compaction pass, which copies live
  entries in order.
- **Compaction must rewrite `order_index`.** Layout-C's arena element carries no
  `order_index` (position gives order when hole-free); the conversion pass assigns
  `order_index = output_position` for each copied entry. This is load-bearing:
  `node_remove` tombstones the `order` slot named by an entry's `order_index`
  (`compact` refreshes it likewise), so a stale index would tombstone the wrong slot
  after a later persistent removal.
- **At most one pass, never a rehash.** Publication performs **exactly one** dense
  reshaping/compaction pass (zero in the layout-(a) hole-free best case), and never
  a per-entry rehash or `node_get`. The cached `hash` rides through untouched.
- **Real vs synthetic cost.** Real MutDict publication cost is
  `build_hamt_bottom_up + build_order_bulk` (+ at most one conversion pass).
  `Dict.compact()`'s dense preparation — recovering values via `node_get` from the
  old HAMT — is **synthetic seam overhead** absent from real MutDict publication
  and must be reported as a separate phase, never charged to publication.

## 8. Required tests

Runtime-shape fixtures (not only the large random-hash parity oracle), each
asserting the built `PDict` is observationally equivalent (§6) to sequential
`Dict.set` construction:

- **empty** — zero live entries → `PDict{ size:0, root:null, order:empty }`.
- **singleton** — one entry → leaf-rooted dict, correct value and order.
- **deep shared hash prefix** — keys with engineered equal leading 5-bit fragments
  → correct nested `HamtNode` chain, all values retrievable.
- **full-hash collision, `core_eq`-unequal keys** — two keys with equal full hash
  but `core_eq(k1,k2) == false` → both retained in a `HamtCollision`, both
  retrievable. (Reference-distinct-but-`core_eq`-equal keys must instead collapse
  to one entry — cover that too.)
- **insertion-order preservation** — `keys()` matches insertion order.
- **remove/reinsert ordering** — removed-then-reinserted key appears at the end.
- **interior dead entries → publish → persistent remove** — a dense input with
  holes compacted at publication, then a persistent `remove` on the result, proving
  the conversion pass rewrote `order_index = output_position` (§7) so the correct
  order slot is tombstoned.
- **post-publication persistent update** — `set`/`remove` on the published dict
  produces a correct new version and does **not** mutate the published root
  (retain and re-check the pre-update version).
- **parity against sequential construction** — the large random-hash oracle
  (already present in the spike) comparing `freeze_dense` output to
  `Dict`-of-sequential-`set` for values, size, absence, and order.

**Test seam for engineered hashes (required).** `Dict.compact()` recomputes hashes
from real source keys via `hash_key`, so it *cannot* inject engineered full-hash
collisions or deep shared prefixes — the collision and deep-prefix fixtures above
are **not** producible through the `compact()` seam. A test-only dense-entry
adapter (an `rt.dict` fixture, or a test-only builtin, that accepts an explicit
`array<HamtEntry>` with caller-supplied hashes and calls `freeze_dense`) is
required to drive these shapes. It is a temporary surface with the same cleanup
policy as the other spike surfaces (§9.7): removed once real MutDict tests exist,
or retained only if it becomes the standing builder unit-test entry point.

**Forward lifecycle tests (when MutDict exists).** Per the storage-README
lifecycle contract, the freeze wrapper's validation must cover, beyond a single
consumed-handle rejection:

- double publication of the same handle;
- every publication-exit category (return, `break value`, `try`/early-return arm,
  unknown call, closure capture, escaping-aggregate storage, task/channel
  publication, host/import boundary);
- post-freeze reads *and* writes through the consumed handle (poison trap and/or
  compile-time selector rejection);
- incompatible storage / ABI routes;
- stale or incomplete region decisions;
- persistent fallback on every unsupported route.

Until a MutDict test entry point exists, non-lifecycle tests target
`build_hamt_bottom_up` / `freeze_dense` through `Dict.compact()` (its first
customer) plus the test-only dense adapter for engineered shapes. Prefer dedicated
shape fixtures over relying on the compact seam alone.

## 9. Migration and cleanup sequence

1. **Add the test seam and §8 shape fixtures first (test-first).** Land the
   test-only dense-entry adapter (engineered hashes) and the empty / singleton /
   deep-prefix / `core_eq`-collision / order / interior-dead-entry fixtures against
   the *existing* `node_build_bottom_up` before any extraction, so the extraction is
   refactoring under a green oracle.
2. **Introduce the private adapter.** Extract `freeze_dense` + `build_order_bulk`;
   rename/retain `node_build_bottom_up` as the private `build_hamt_bottom_up`. Route
   `Dict.compact()` through `freeze_dense`. Fixtures from step 1 stay green.
3. **Route real MutDict publication through `freeze_dense`** when the S5 MutDict
   runtime exists (dense stream / arena → `freeze_dense`), with the §4 lifecycle
   (compile-time proof + poison) and §7 direct-consume-or-one-compaction rule.
4. **Optionally retain `Dict.compact()`** as a synthetic `freeze_dense` customer,
   with its `node_get` preparation reported as synthetic seam cost.
5. **Add allocation / peak-memory / GC profiling** on the real MutDict stream.
6. **Only then decide ping-pong** (§3), gated on that profiling.
7. **Remove all temporary spike surfaces as a unit** once real publication evidence
   is recorded:
   - runtime: `node_build_sequential`, `node_build_editable`,
     `node_build_incremental_fn`, `bench_builders`, `bench_timings`,
     `bench_builder_rotation`, the `builder_bench_rotation` /
     `builder_bench_timings` globals, and the `twinkle_runtime.now` (`host_now`)
     import in `rt.dict`;
   - the now-orphaned owned/editable-builder chain that `node_build_editable` was
     the sole entry into: `node_set_owned` (`dict.tw:1493`), `owned_replace_slot`
     (`dict.tw:1444`), and `owned_insert_slot` (`dict.tw:1461`). (Grep-confirmed no
     other callers in `boot/` or `src/`; the *production* persistent path is
     `node_set`, which `node_build_sequential` uses and which is **not** removed.)
   - `boot/compiler/builtins.tw`: the `dict$bench_*` abi entries and `rt(...)`
     registrations;
   - `boot/prelude/signatures/dict.tw`: `bench_builders`, `bench_timings`,
     `bench_builder_rotation`;
   - `boot/bench/dict_compact_builder_spike.tw` (retire once the comparison is
     archived);
   - the test-only dense-entry adapter (§8), unless kept as the standing builder
     unit-test entry point.
   Ordinary persistent `Dict.set` / `remove` / `get` / `node_set` remain untouched
   throughout.
8. **Update evidence docs.** Record the measured real-MutDict publication result,
   the provisional crossover range, and remaining caveats in
   `sound-uniqueness/storage/spike-tier0-dict.md` and the storage `README.md`.

**Stage0 disposition — intentionally boot-only.** The adapter is Wasm-emitting
codegen runtime (it *emits* `rt.dict` functions as data), not a language feature
*used* in boot source, so the "a feature used in boot source needs happy-path
stage0 support" rule does not apply: stage0 only has to compile the boot source
that emits it, which it already does. Stage0's Rust `src/runtime/dict.rs` carries
none of the spike or adapter functions today, and `Dict.compact()` routing through
`freeze_dense` changes only the *emitted* runtime, not the boot compiler's own
type-checking of user programs. Therefore the `boot/` path is the sole
implementation and **no `src/runtime/` mirror is added**. The one thing to hold: if
a future step makes boot *source* call a new adapter-specific builtin (e.g. the
test-only dense adapter as a real builtin), that builtin needs a stage0 happy-path
stub so `make stage2` still bootstraps — the standard stage0-bootstrap-dependency
rule, not a full Rust reimplementation.

## 10. Explicit non-goals

- **No general public mutable `Dict` API.** `freeze_dense` and the builder are
  compiler-private; no user-facing mutable dict.
- **No change to ordinary persistent `Dict.set` / `remove`.** They keep their
  existing `node_set` / `node_remove` paths.
- **No flat-first public `Dict` redesign.** Persistent HAMT remains the public
  representation; this adapter is a publication boundary, not a default.
- **MutDict is not for sparse-divergence forks.** The read/fork spike stands:
  sparse forks favor persistent HAMT structural sharing; MutDict is for
  proven-owned, update-dense / wide, stay-low chains.
- **`run_fixpoint`'s small sparse-forked maps are not a MutDict customer.** They
  are ~32–60 entries with sparse per-fork divergence — an S4 owned-ABI analysis
  case, not S5 storage.
- **No universal compiler threshold encoded here.** Size dispatch and
  adversarial-hash-shape coverage remain separate, open work; this document
  designs the adapter, not the decision to invoke it.

---

## Validation (for the eventual implementation, not this doc)

```bash
target/twk fmt boot/compiler/codegen/runtime/dict.tw \
  boot/compiler/codegen/runtime/types.tw
target/twk lint boot/main.tw          # baseline: 5 unrelated record-copy-helper findings
make stage2
make quick-bundle-cli
target/twk test
```

Inspect emitted construction calls to confirm no `node_set` / `node_get` /
per-entry `arr_push` inside the builder path, e.g.:

```bash
target/twk wat boot/tests/main.tw --func freeze_dense --calls
target/twk wat boot/tests/main.tw --func build_hamt_bottom_up --list
```

Do not run tree-sitter tests; this work does not touch the grammar.
