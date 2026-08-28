# Storage Representation Track

**Status:** In progress. **MutVec slice 1 has landed and runs unconditionally.** The
S1/S2/S3 first vector slice — an owned, locally-born `collect`/`make` region with ≥1
indexed write, lowered to flat typed `MutVec<fam>` and frozen back to a typed
`PVec<fam>` — now compiles end-to-end for **Int, Bool, Float, and Byte** (typed
`array<i64>` / `array<i32>` / `array<f64>` / packed `array<i8>`); the self-host fixed
point holds and boot-test is green. See [../../mutvec-checklist.md](../../mutvec-checklist.md)
for the per-family ledger and [../../mutvec-later-slices.md](../../mutvec-later-slices.md)
for the family-fold mechanism. Remaining storage work: the boxed/record element family (spike
done, ~2× lever, deferred pending a workload) and Float/Byte **param-sourced**
`set_in_place` write-routing; and the later packages **S4** (owned-specialized mutable
ABI / param-sourced thaw-from-`PVec`), **S5** (`MutDict`), and **S6** (Buffer-retirement
perf gate) are not started. Representation decisions for S2/S3/S5 and vector append
settled 2026-08-01 (see [Settled decisions](#settled-decisions)); crossover thresholds
inside them are spike-gated.

This track owns the mandatory performance substrate for closing the
sound-uniqueness project. Existing-hook lowering is the early integration proof:
it shows that sound ownership decisions can safely select private mutation. It is
not, by itself, the full performance story.

The storage-track north star is compiler-private mutation-enabled collection
storage that can flow through a proven-owned chain and materialize back to the
ordinary persistent representation only at the latest required publication
boundary. In shorthand: **stay low as long as possible, then materialize only if a
boundary demands it** — and if the value never escapes the owned region,
**never materialize at all**. Materialization is a boundary *adapter*, not an
inevitable final step: a value that dies inside the owned region (mutated, then
read-reduced to a scalar and dropped, never published as a collection) has no
publication boundary, so there is nothing to freeze. This is the collections
analog of escape analysis — freeze is the cost of crossing out, and code that
never crosses out never pays it. (Already realized for vectors: the MutVec region
detector accepts the no-escape scratch-buffer shape and emits no `mutvec_freeze` —
commit `f83ecf78`, `MutVecRegion.escapes == false`.) Existing `PVec`/`PDict` hooks
remain useful compatibility and proof-of-integration targets, but the end state
should not require every internal update chain to stay in persistent PVec/HAMT
form.

## Settled decisions

These resolve the previously-open S2/S3/S5 representation questions. They are
design commitments, not yet-implemented code; the crossover thresholds inside them
are spike-gated (see [Spike-first methodology](#spike-first-methodology)).

### Backing representation: Wasm GC, not linear memory

Private mutable storage is backed by **Wasm GC collections**, consistent with the
language's no-linear-memory-by-default rule. The vector target is `MutVec<T>` — a
**growable mutable GC `array` plus a length field, grown by doubling** — not a
reused `PVec` and not a linear-memory buffer.

Why this over reusing PVec `set_in_place`:

- PVec `set_in_place` walks the 32-branch trie (**O(log n)**) over **boxed
  `anyref`** elements.
- `MutVec` read/write is `array.get`/`array.set` — **O(1) and unboxed**. Typed
  element sites use typed element arrays (`array<i64>`, `array<i32>` for `Bool`,
  packed `array<i8>` for `Byte`), preserving the typed/unboxed representation the
  "representation-aware lowering" track invariant requires.

Materializing `MutVec -> PVec` is an **O(n) single-pass copy** (approaching O(1)
for a vector small enough to become a PVec tail directly). Because the stay-low
model freezes once per owned chain, this is paid once at the boundary, not per
update.

**S3 prerequisite to confirm:** Wasm GC **packed arrays** (`array i8`/`array i16`,
`array.get_u/_s`) must be supported by the target runtime/toolchain. Packed
`array i8` is what lets `Vector<Byte>` become dense unboxed GC storage without
linear memory — the S6 path to retiring `Buffer` on its own terms.

### Flat mutable vs transient persistent is a symmetric 2×2

Every flat mutable backing carries an O(n) freeze cost; every collection also has
a *transient* persistent form that freezes in ~O(1) at the price of O(log32 n)
ops. The choice has the same shape for vectors and dicts:

| | flat mutable (`MutVec`, flat hashmap) | transient persistent (transient PVec / HAMT) |
|---|---|---|
| op cost | O(1) | O(log32 n) |
| freeze cost | O(n) | ~O(1) |

The freeze concern therefore applies to **both** families; what differs is the
freeze *constant* and the size of the win from going flat:

- **Vector:** flat `MutVec` freeze is a contiguous copy (cheap, cache-friendly),
  and the flat form's win is large (unboxed, typed, true O(1) random access). Flat
  wins despite the O(n) freeze; a transient PVec is not needed. *(Assumed from the
  constant-factor argument; spike-verifiable — see below.)*
- **Dict:** measurement revised this (see
  [spike-tier0-dict.md](spike-tier0-dict.md)). The operation-throughput lever is
  **flat unboxed storage (`MutDict`)**: 40–70× faster on mutate. A true editable
  HAMT builder, which the original inplace benchmark did not actually measure,
  improves HAMT rebuilding by about 1.3× but retains per-entry hashing and trie
  traversal. It may help as a materialization adapter; it is not the private
  region representation. Flat→HAMT freeze remains an O(n) full build, creating a
  real crossover near k/n ≈ 1 with the current sequential builder. `MutDict` is
  therefore **conditional**: gated on a proven update-dense / wide region (the S4
  stay-low-across-a-chain case), with persistent fallback otherwise. It keeps an
  insertion-order sidecar, and old-version observability stays gated on the
  ownership proof.

### Size-dependence and where the decision lives

The flat-vs-transient crossover is governed by data size **n**, mutations per
element **k/n**, and how often the chain materializes — not by n alone.
Critically, **static ownership analysis never knows n**, so the representation
cannot be picked per-site from size at compile time. That leaves two design
endpoints, and the spike decides between them:

1. **Cliff-free default** — pick the form with no size-dependent freeze penalty
   (transient) and accept its op constant everywhere, *if* the spike shows flat's
   advantage is within noise across realistic sizes.
2. **Runtime size-tagged dispatch** — the "private tagged representation" pressure
   valve — justified only if the spike finds a crossover *inside* the range real
   workloads hit.

If flat `MutVec` wins or ties up through large n, runtime dispatch is dropped and
the design simplifies to "flat for vectors, transient for dicts."

### Spike-first methodology

The representation choices above are validated by benchmark before the real
backends are built:

- **Tier 0 (proxy, no new runtime):** approximate flat mutable storage with
  `@std.buffer` for the mutate phase plus a `collect`/builder freeze into
  `Vector`; compare against today's persistent `Vector`/`Dict` baseline. Sweep
  **n × k** (e.g. n in {16, 256, 4K, 64K, 1M}, k in {n/4, n, 4n, 16n}) and record
  mutate-phase, freeze-phase, and end-to-end separately. This slightly understates
  the real GC-array case (GC `array.get/set` is at least as fast and stays
  unboxed) but establishes the crossover shape before committing runtime + codegen
  work.
- **Tier 1 (real):** once a minimal `MutVec` GC-array runtime op exists, re-measure
  against the proxy and against the transient forms.

Benches live alongside the existing `boot/bench/` set.

**Tier-0 vector result (2026-08-01, [spike-tier0-vector.md](spike-tier0-vector.md)):**
unboxed flat mutation is **15–35× faster than boxed PVec `set_in_place`** (widening
with n), and the O(n) freeze is a cheap one-time tax with **no meaningful crossover**
in the range that matters. This confirms "build `MutVec`, flat for vectors, no
runtime size dispatch."

**Tier-0 dict result (2026-08-01, extended 2026-08-28,
[spike-tier0-dict.md](spike-tier0-dict.md)):** flat `MutDict` mutate is **40–70×
faster** than boxed HAMT `set_in_place`, but its O(n) freeze *is* a full dict
build, producing a **real crossover near k/n ≈ 1** with sequential insertion.
The existing in-place helper still path-copies internal nodes, so it was not a
transient-HAMT proxy. A true owned/editable builder improves rebuilding by about
1.3×: useful as a freeze helper, but not competitive with flat storage as the
region representation. `MutDict` remains a *conditional* win.

## Track invariants

- Storage optimization does not create a second legality path. Ownership facts
  and codegen decisions remain the only authority for destructive mutation.
- Lowering must be representation-aware: it may not degrade a typed/unboxed
  storage site to an erased `anyref` representation merely to use an existing
  mutable helper.
- User-facing immutable APIs remain unchanged. Mutable storage forms are
  compiler-private implementation targets.
- Persistent materialization is a last-responsible-boundary operation, not an
  internal bookkeeping step. It happens when a persistent representation is
  demanded by return to a persistent/generic ABI, unknown call, closure capture,
  storage in an escaping aggregate, task/channel publication, host boundary, or
  any other edge where the value may be observed outside the proven-owned region.
- Persistent fallback remains mandatory whenever a proof, representation match,
  helper family, runtime target, or safe freeze/materialization point is
  unavailable.
- Performance claims require same-session benchmarks and codegen inspection;
  intermediate milestones are judged by correctness and emitted shape.

## Why this track exists

The codegen track initially reuses surviving hooks: boxed `PVec` vector set,
dict in-place helpers, vector/string builders, and record shell reuse. That is the
right proof-of-integration path, but those hooks do not fully replace the manual
`@std.buffer` workaround:

- `Buffer` provides flat mutable storage and unboxed primitive reads/writes.
- Current boxed vector `set_in_place` operates on `PVec` with `anyref` elements.
- Typed-vector wins (`PVecI64`, `PVecBool`, typed builders, typed reads) are
  storage-site dependent and must not be lost by mutable lowering.
- Full update-chain performance needs private mutable representations such as a
  growable `MutVec`, dense byte/int regions, mutable/transient HAMT storage, or
  owned-specialized mutable ABI variants that avoid repeated freeze/thaw between
  helper calls.

Therefore optimized mutable storage is required end-of-track work, not optional
polish after migration.

## Latest-boundary materialization

"Freeze only at final publication" means the same thing as materialize-at-
boundary, but "final" must be read per proven-owned path rather than as only a
function's last statement. A function can have multiple publication exits —
return, `break value`, `try` error arm, closure capture, unknown call, record /
variant storage that escapes — and each such edge may be the latest safe point on
that path. It can also have **zero** publication exits: if no path ever publishes
the value as a collection (it is consumed entirely by in-region reads and then
dropped), there is no boundary and **no freeze is emitted** — the lowest private
representation is also the final one. Count the publication exits first; freeze is
per-exit, so zero exits means zero freezes.

The goal is to keep the value in the lowest private representation across all
internal edges that preserve ownership: local loops, helper calls, owned-
specialized variants, and recursive/SCC steps. Do not freeze after every helper
call just because the source-level type is `Vector<T>` or `Dict<K, V>`. Do not
bounce `MutVec -> PVec -> MutVec` or `MutDict -> PDict -> MutDict` inside a chain
unless a real boundary or unsupported operation forces it.

This makes materialization a boundary adapter, analogous to a calling-convention
or representation adapter, not the default representation of an owned update
chain.

## Region decision-record lifecycle

Scoped regions and owned-specialized mutable ABI need a richer decision record
than the first existing-hook selector. Before either S2 or S4 emits mutable
storage, each accepted region decision must name:

- a stable region id and proof/debug id;
- the begin site, source value, source physical representation, and selected
  private storage family;
- every operation site inside the region and its storage-compatible operand /
  result shape;
- every materialization exit, including normal return, `break value`, `try` /
  early-return arm, unknown call, closure capture, escaping aggregate storage,
  task/channel publication, and host/import boundary;
- the post-materialization persistent value and the point after which the mutable
  handle is invalid;
- any owned-specialized callee ABI variant that receives or returns the private
  storage;
- fallback behavior when the region is stale, ambiguous, missing an exit, missing
  a compatible ABI/storage target, or fails validation.

Required validation:

- no path from `begin` reaches a publication sink with an unfrozen mutable handle;
- no path uses the mutable handle after materialization;
- no double-begin for the same live storage and no double-materialization of the
  same handle;
- all region operations use a compatible storage family and representation;
- persistent fallback is available for every begin, operation, and call-site
  route.

These lifecycle checks belong to the centralized storage/region selector. Backend
emitters consume the selected target; they must not re-prove ownership, region
exits, or handle validity.

## Storage model options

### Scoped mutable regions — first implementation slice

A scoped mutable region begins from a proven-owned value, performs local
read/write/append/remove operations through private mutable storage, and freezes
back to ordinary `Vector`/`Dict` at the latest boundary the first implementation
can prove. Early slices may choose conservative local boundaries, but the design
should always prefer the widest safe region.

This is the best first storage slice because it has simple publication rules and
covers local loops, builders, and AWFY-style update regions. It may still
materialize too early for helper-heavy code, so it is not the final model.

### Owned-specialized mutable ABI — north-star chain model

An owned-specialized function variant may accept and return a private mutable
storage representation, such as `MutVec<T>` or `MutDict<K, V>`, when all callers
and callees in the chain have compatible ownership decisions. The value remains
in mutable storage across helper calls and recursive/SCC edges until a real
publication boundary forces materialization. This is the main mechanism for
"staying low" beyond one local region.

This is the model that can avoid repeated persistent materialization in compiler
state-threading code. It must be demand-driven and capped, with generic
persistent variants as fallback, to avoid specialization explosion.

### Private tagged representation — optional pressure valve

A private representation tag such as `Persistent(PVec)` vs `Mutable(MutVec)` may
reduce the number of cloned variants, but the tag is not proof of uniqueness.
Static ownership decisions still license mutation. Any runtime dispatch must stay
outside hot loops where possible.

## Work packages

### S1 — Repr-aware mutable selection guards

Make the codegen selector and operation catalog distinguish semantic collection
type from physical storage representation. A mutable decision for a typed vector
site must require a typed mutable target or a compatible private mutable-storage
route; otherwise it falls back to the persistent typed path.

Acceptance examples:

- boxed `PVec` site may select boxed `vector$set_in_place` when ownership proves
  uniqueness;
- `PVecI64` / `PVecBool` site must select a matching typed target, enter a
  compatible private mutable-storage region, or remain persistent;
- no optimization inserts a `PVecI64 -> PVec -> PVecI64` round trip just to use a
  boxed mutable helper.

### S2 — Scoped mutable vector/builder regions

**First slice LANDED** (MutVec slice 1, unconditional for Int/Bool/Float/Byte): an
owned locally-born region with ≥1 indexed write lowers to `MutVec<fam>` and freezes
at a single boundary (no-escape scratch regions freeze-free). Param-sourced /
written-outside-a-claimable-region cases are **S4** and still open.

Introduce the first private mutable-region lowering within a local vector scope:

```text
begin_owned_vector
read / write / append
materialize_to_persistent_vector
```

Scope S2 to vector and builder-shaped regions. Dict `begin_owned_dict`, `remove`,
and mutable/transient HAMT behavior are deferred to S5 so dict ordering,
old-version observability, and nested-value semantics are handled in the same
slice as dict storage.

The storage backend for these regions is the S3 `MutVec` GC-array target (see
[Settled decisions](#settled-decisions)). Reusing existing PVec hooks is allowed
only as an interim proof-of-seam if the `MutVec` runtime op is not yet ready;
because the goal is max performance, the region interface should be shaped to go
straight to `MutVec` rather than treating boxed-PVec reuse as the destination.
Even in the first slice, materialization should be placed at the latest boundary
the implementation can soundly identify, not after every internal update.

Acceptance requirements:

- every S2 region has a decision record satisfying the lifecycle contract above;
- vector index/read/write/append operations preserve the selected representation
  or explicitly fall back;
- no S2 region includes dict remove/set semantics;
- fallback emits the ordinary persistent vector/builder path.

**Design spec:** the first S1/S2/S3 vector slice is specified in
[mutvec-slice1-design.md](mutvec-slice1-design.md) (Int-only `MutVecI64`,
dedicated region pass, converging to a unified typed-vector-region pass).

### S3 — Private mutable vector storage targets

**LANDED for Int/Bool/Float/Byte** (MutVec slice 1): each family has a typed
`MutVec<fam>` GC struct + runtime ops, family-generated off `PVecFamily`, freezing to
the matching typed `PVec<fam>`. The boxed/record element family (`array<anyref>`) is
spike-done but deferred pending a workload.

The primary private vector storage target is **`MutVec<T>` — a growable mutable
Wasm GC `array` plus a length field, grown by doubling** (decision folded into
[Settled decisions](#settled-decisions)). The element array is typed per site:

```text
MutVec<Int>    backed by  array<i64>
MutVec<Bool>   backed by  array<i32>
MutVec<Byte>   backed by  packed array<i8>   (S6/Buffer-retirement enabler)
MutVec<T:ref>  backed by  array<anyref>
```

Typed PVec in-place helpers (`vector$set_in_place_i64` / `_bool`) may still be
emitted as an interim step, but the destination is `MutVec`, not a boxed-PVec
hook. The important property is representation preservation across the hot update
path: no erased `anyref` element traffic for typed/unboxed sites unless crossing a
real boundary.

Open sub-decisions carried into implementation: growth policy (doubling vs 1.5×)
and initial capacity heuristic; the freeze primitive (`MutVec -> PVec` O(n) copy,
with an O(1) small-vector tail-handoff fast path). Both are settled by the S3
spike.

### S4 — Owned-specialized mutable ABI across calls

Extend ownership-specialized variants so selected functions can accept and return
private mutable storage. This is what lets a proven-owned collection stay low
through a whole helper chain instead of freezing after each function.

Requirements:

- every S4 route has a decision record satisfying the lifecycle contract above;
- deterministic variant naming and routing;
- variant caps with persistent fallback;
- exact compatibility with Phase 6/8 ownership-specialization decisions;
- ABI/storage compatibility for each parameter, return value, and recursive/SCC
  edge carrying private storage;
- fallback to the generic persistent ABI when a mutable-storage route is stale,
  ambiguous, over cap, unsupported, or missing a materialization exit;
- clear inspection output showing where mutable storage enters, flows, stays low,
  materializes at a boundary, or falls back.

### S5 — True mutable/transient dict storage

Introduce dict scoped regions and extend dict lowering beyond helper-call
selection. The Tier-0 dict spike ([spike-tier0-dict.md](spike-tier0-dict.md))
settled the region representation: the target is a **flat unboxed mutable hashmap
(`MutDict`)** with an insertion-order sidecar, not a transient HAMT. A true
owned/editable HAMT builder improves rebuilding by about 1.3× and remains a
candidate freeze helper, but it retains per-entry HAMT traversal and is not the
throughput target. `MutDict` mutate is 40–70× faster, but its O(n) flat→HAMT
freeze *is* a full dict build, so it only pays off when **k/n ≳ 1** (updates
exceed distinct keys). `MutDict` is therefore **conditional**: emit it only for a
proven update-dense / wide region where the freeze amortizes across many ops
(the S4 stay-low-across-a-chain shape, but not `run_fixpoint`'s sparse transfer
maps), and fall back to the persistent path for build-once / lookup-heavy dicts.
The final target lets such owned dict update chains remain mutable internally and
materialize to ordinary `Dict<K, V>` only when publication requires it.

Publication is three-tier, not binary (see
[spike-tier0-dict.md](spike-tier0-dict.md) boundary result): old-version-dead →
in-place; old version observed but consumers stay in the private flat
representation → **clone the flat backing** (a bulk `array.copy`, measured 14–40×
cheaper than freeze); old version escapes the persistent `Dict` ABI → freeze to
HAMT (deferred to the true edge). Cheap clone-on-fork can keep `MutDict` low
across update-dense forks, so the S5 selector keys on ops-per-fork and whether
the snapshot stays flat — not k/n alone. It does **not** make `run_fixpoint` a
candidate: those maps are small and fork with sparse divergence, where the HAMT
wins on both reads and structural sharing.

Dict region decisions must satisfy the same lifecycle contract as S2/S4, plus the
dict-specific semantic gates below.

This work must preserve:

- insertion-order iteration;
- lookup/update/remove semantics;
- old-version observability;
- the rule that ownership of a dict backing does not imply ownership of
  reference-typed values stored inside it.

### S6 — Storage performance gate for Buffer retirement

Before migration can retire Buffer workaround usage, ordinary `Vector`/`Dict` /
record code must reach the same performance class as the current manual mutable
variants where storage mutation is the bottleneck. `Vector<Byte>` must cover the
standard byte/crypto workloads well enough that `Buffer` is no longer the normal
answer for local byte updates.

This gate belongs here because Buffer retirement depends on private mutable
storage performance, not merely on hook consolidation.

## Relationship to other tracks

- **Analysis** proves ownership, liveness, publication, field paths, and variant
  facts. Storage work consumes those facts; it does not rediscover them.
- **Codegen** proves the first end-to-end selection path through existing hooks.
  Storage work extends that path with private mutable storage and representation-
  preserving runtime choices.
- **Migration** runs after successful codegen/storage work. It consolidates hooks
  and private storage operations behind compiler-private intrinsics, removes
  split-brain mutability paths, and evaluates Buffer cleanup.

## Follow-up: revisit vector append in place

The copy-carrier borrow/effect engine
([../../archive/2026-07-24-copy-carrier-engine-impl-plan.md](../../archive/2026-07-24-copy-carrier-engine-impl-plan.md))
surfaced a representation gap worth revisiting under this track: **vector append is not a
`mutable_produce` update-call candidate.** `decision_family_for_persistent`
(`boot/compiler/codegen/mutable_catalog.tw`) assigns an in-place decision family only to
`Dict.set`, `Dict.remove`, and `vector$set_unsafe`; for a vector append (`sem.builder.push_id`)
`update_call_target` returns `None`, so it never produces an in-place decision through the
ownership-decision path. Vector append in place today comes *only* from the separate
loop-builder optimization pass.

Consequence: a **param-sourced vector copy-carrier** — e.g. `next_locked := locked;
next_locked = .append(k)` in `merge_targeted_min` — is proven `base=reuse(unique)` by ownership
analysis but does **not** flip through `mutable_produce`, because append has no decision family.
The dict half of the same shape flips fine.

**Decision (2026-08-01):** once `MutVec<T>` exists (S3), vector append gets its own
in-place decision family. On `MutVec` append is `array.set` at `len` plus a
`len++` (amortized O(1) via doubling), the *same* backend as indexed update, so
append and `xs[i] = v` unify under one storage form. This closes the gap so
param-sourced vector copy-carriers (e.g. `next_locked := locked; next_locked =
.append(k)` in `merge_targeted_min`) flip, matching their dict half. The dependency
is strict: this is an S3-gated follow-up, not doable through the boxed-PVec hooks
alone. Orthogonal to the copy-carrier dict engine, which is complete.

## Follow-up: `run_fixpoint`'s own dataflow maps — S4 analysis case, not an S5 storage customer

A 2026-07-27 investigation (three self-host-verified spikes, all reverted; the standalone
`fixpoint-map-inplace` diagnosis doc it produced is archived) established that the compiler's
own ownership fixpoint — `run_fixpoint`'s per-block `own`/`valid`/`prov` maps, merged via
`merge_targeted` (`ownership.tw`) — **cannot be flipped in-place through the existing
in-place-decision hooks (8D)**, and is instead a textbook customer for **S4 (owned-specialized
mutable ABI)**.

Findings worth keeping:

- **Not a soundness wall, and not an `exits`-aliasing problem.** The per-block maps are freshly
  allocated (`join_entry_ownership`/`_valid`/`_prov` each build a new `Dict.new()`; `ret=fresh`).
- **The blocker is escape over-approximation across the transfer tree.** The `ForwardState`
  record that holds the maps is threaded through `seed_payload_binding` and `forward_block`, both
  summarized `p_st=Published, ret=alias`. The `Published` cascades transitively (the escape rule
  `own_is_shared(exits, st)`, `ownership.tw`) from passing `st` to `Published`-param callees down
  the whole `transfer_op` tree — it is **conservative, not a real leak** (no collection/global
  stores a `ForwardState`; `publish_atom` only sets a key). So by the time `merge_targeted`
  receives a map it is Shared → `arg_unique=false` at every call site (verified by instrumenting
  `uniform_entry_seeds`), even after restructuring `run_fixpoint` to pass each map as a
  single-reader last-use.
- **Why the hook route is also low-yield even if it flipped.** `dict$set_in_place` operates on
  boxed HAMT with `anyref` elements; it saves the persistent rebuild allocation only. Measured:
  the per-block merge region is ~37% of `run_fixpoint`, `run_fixpoint` is a few seconds of a ~17s
  full build, so the merge region is ~10% of total compile and the in-place slice is a ~1–2%
  ceiling. Census context: 396/719 `dict_set` sites already flip via 8D; 303 remain
  `persistent(aliased shell)` (the general precision ceiling), of which this is a hard,
  transitively-published sub-class.

**Revised implication for this track (2026-08-28):** this remains a useful S4
analysis case because owned-specialized variants could preserve `ForwardState`
field ownership across the transfer chain. It is **not** an S5 `MutDict` storage
customer, however: the maps are only about 32–60 entries and each live-base fork
changes few entries, so the read-and-fork spike shows that persistent HAMT storage
is the better representation. Do **not** re-attempt the existing 8A–8E hook route,
and do not use this workload to justify flat storage. (The `merge_targeted` body
rewrite that hoists its `next` reads is a harmless, self-host-safe cleanup that
yields a clean `p1=Consumed` carrier shape; it flips nothing on its own and can be
cherry-picked if convenient.)

## References

- [../codegen/README.md](../codegen/README.md)
- [../migration/buffer-cleanup.md](../migration/buffer-cleanup.md)
- [../../performance/representation-boundary-policy.md](../../performance/representation-boundary-policy.md)
- [../../performance/vector/typed-vector-representation.md](../../performance/vector/typed-vector-representation.md)
- [../../archive/persistent-vector.md](../../archive/persistent-vector.md)
- [../../archive/persistent-dict.md](../../archive/persistent-dict.md)
- [../../archive/dict-performance-enhancements.md](../../archive/dict-performance-enhancements.md)
