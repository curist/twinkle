# Storage Representation Track

**Status:** Planned; starts after existing-hook codegen lowering proves the
analysis/codegen seam, and before migration cleanup or Buffer retirement.

This track owns the mandatory performance substrate for closing the
sound-uniqueness project. Existing-hook lowering is the early integration proof:
it shows that sound ownership decisions can safely select private mutation. It is
not, by itself, the full performance story.

The storage-track north star is compiler-private mutation-enabled collection
storage that can flow through a proven-owned chain and materialize back to the
ordinary persistent representation only at the latest required publication
boundary. In shorthand: **stay low as long as possible, then materialize at the
boundary**. Existing `PVec`/`PDict` hooks remain useful compatibility and
proof-of-integration targets, but the end state should not require every internal
update chain to stay in persistent PVec/HAMT form.

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
that path.

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

The initial vector implementation may reuse existing PVec hooks where
appropriate, but the interface should be shaped so a later `MutVec` backend can
replace the storage without changing ownership legality. Even in the first slice,
materialization should be placed at the latest boundary the implementation can
soundly identify, not after every internal update.

Acceptance requirements:

- every S2 region has a decision record satisfying the lifecycle contract above;
- vector index/read/write/append operations preserve the selected representation
  or explicitly fall back;
- no S2 region includes dict remove/set semantics;
- fallback emits the ordinary persistent vector/builder path.

### S3 — Private mutable vector storage targets

Add or formalize private vector storage targets for update-heavy regions. The
implementation may include typed PVec in-place helpers, growable mutable arrays,
dense byte/int regions, or a combination selected by region shape.

Potential typed helper targets:

```text
vector$set_in_place_i64   PVecI64?, i32, i64 -> PVecI64
vector$set_in_place_bool  PVecBool?, i32, i32 -> PVecBool
```

Potential private storage target:

```text
MutVec<T> / MutVecI64 / MutVecByte
```

The important property is representation preservation across the hot update path:
no erased `anyref` element traffic for typed/unboxed sites unless crossing a real
boundary.

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
selection to the HAMT storage algorithm or to a private mutable hashmap/transient-
HAMT representation. The final target should let owned dict update chains remain
mutable internally and materialize to ordinary `Dict<K, V>` only when publication
requires it.

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

## References

- [../codegen/README.md](../codegen/README.md)
- [../migration/buffer-cleanup.md](../migration/buffer-cleanup.md)
- [../../performance/representation-boundary-policy.md](../../performance/representation-boundary-policy.md)
- [../../performance/vector/typed-vector-representation.md](../../performance/vector/typed-vector-representation.md)
- [../../archive/persistent-vector.md](../../archive/persistent-vector.md)
- [../../archive/persistent-dict.md](../../archive/persistent-dict.md)
- [../../archive/dict-performance-enhancements.md](../../archive/dict-performance-enhancements.md)
