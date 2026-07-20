# Storage Representation Track

**Status:** Planned; starts after existing-hook codegen lowering proves the
analysis/codegen seam, and before migration cleanup or Buffer retirement.

This track owns the mandatory performance substrate for closing the
sound-uniqueness project. Existing-hook lowering is the early integration proof:
it shows that sound ownership decisions can safely select private mutation. It is
not, by itself, the full performance story. To retire `Buffer` as the ordinary
local-update workaround, mutable lowering must also preserve or select optimized
storage representations.

## Track invariants

- Storage optimization does not create a second legality path. Ownership facts
  and codegen decisions remain the only authority for destructive mutation.
- Lowering must be representation-aware: it may not degrade a typed/unboxed
  storage site to an erased `anyref` representation merely to use an existing
  mutable helper.
- User-facing immutable APIs remain unchanged. New storage forms are compiler-
  private implementation targets.
- Persistent fallback remains mandatory whenever a proof, representation match,
  helper family, or runtime target is unavailable.
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
- True dict wins require HAMT mutation/transient-HAMT work beyond simply naming a
  mutable helper.

Therefore optimized storage representation is required end-of-track work, not
optional polish after migration.

## Work packages

### S1 — Repr-aware mutable selection guards

Make the codegen selector and operation catalog distinguish semantic collection
type from physical storage representation. A mutable decision for a typed vector
site must require a typed mutable target; otherwise it falls back to the
persistent typed path.

Acceptance examples:

- boxed `PVec` site may select boxed `vector$set_in_place` when ownership proves
  uniqueness;
- `PVecI64` / `PVecBool` site must select a matching typed target or remain
  persistent;
- no optimization inserts a `PVecI64 -> PVec -> PVecI64` round trip just to use a
  boxed mutable helper.

### S2 — Typed mutable vector targets

Add or formalize repr-preserving mutable vector helper families for the typed
storage sites that matter first, especially `Vector<Int>` and `Vector<Bool>`.
The helper family should preserve the source/result physical representation and
avoid `anyref` element traffic in the hot update path.

Potential targets:

```text
vector$set_in_place_i64   PVecI64?, i32, i64 -> PVecI64
vector$set_in_place_bool  PVecBool?, i32, i32 -> PVecBool
```

Broader scalar or typed-reference families should be demand-driven by benchmark
and code-size evidence.

### S3 — Dense mutable regions for byte/int kernels

For workloads where trie-shaped persistent storage cannot reach the target class,
add compiler-private dense regions or kernel lowering. This covers byte/int loops,
crypto-style workloads, gather/permute, and sort/order-by support where a flat
working set beats repeated PVec traversal.

Dense regions are not a replacement for ordinary `Vector` semantics. They are an
internal storage choice selected only when ownership/publication facts and region
shape make materialization/freeze profitable and safe.

### S4 — True mutable/transient dict storage

Extend dict lowering beyond helper-call selection to the HAMT storage algorithm.
Targets include true in-place HAMT mutation when the old version is dead, or a
compiler-private transient HAMT region with explicit freeze back to ordinary
`Dict<K, V>`.

This work must preserve:

- insertion-order iteration;
- lookup/update/remove semantics;
- old-version observability;
- the rule that ownership of a dict backing does not imply ownership of
  reference-typed values stored inside it.

### S5 — Storage performance gate for Buffer retirement

Before migration can retire Buffer workaround usage, ordinary `Vector`/`Dict` /
record code must reach the same performance class as the current manual mutable
variants where storage mutation is the bottleneck. `Vector<Byte>` must cover the
standard byte/crypto workloads well enough that `Buffer` is no longer the normal
answer for local byte updates.

This gate belongs here because Buffer retirement depends on storage performance,
not merely on hook consolidation.

## Relationship to other tracks

- **Analysis** proves ownership, liveness, publication, field paths, and variant
  facts. Storage work consumes those facts; it does not rediscover them.
- **Codegen** proves the first end-to-end selection path through existing hooks.
  Storage work extends that path with representation-preserving targets and
  runtime storage choices.
- **Migration** runs after successful codegen/storage work. It consolidates hooks
  behind compiler-private intrinsics, removes split-brain mutability paths, and
  evaluates Buffer cleanup.

## References

- [../codegen/README.md](../codegen/README.md)
- [../migration/buffer-cleanup.md](../migration/buffer-cleanup.md)
- [../../performance/representation-boundary-policy.md](../../performance/representation-boundary-policy.md)
- [../../performance/vector/typed-vector-representation.md](../../performance/vector/typed-vector-representation.md)
- [../../archive/persistent-vector.md](../../archive/persistent-vector.md)
- [../../archive/persistent-dict.md](../../archive/persistent-dict.md)
- [../../archive/dict-performance-enhancements.md](../../archive/dict-performance-enhancements.md)
