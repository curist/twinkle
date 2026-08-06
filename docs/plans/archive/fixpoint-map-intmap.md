# Ownership fixpoint maps → semantic `IntMap`/`IntSet` types

**Status:** Draft plan — design approved, unstarted. **Semantic refactor** (a
representation/perf swap was investigated and rejected — see
[Performance: measured null result](#performance-measured-null-result)).

The ownership/uniqueness analysis threads many `Dict<Int, T>` and `Dict<Int, Bool>`
maps whose key meaning (LocalId vs block id) and set-vs-map intent live only in
comments. This plan replaces them with **named types** so intent lives in the type
and the compiler enforces it. The backing stays the persistent HAMT `Dict` — a
spike showed a different backing does not help at the sizes these maps actually
reach, so this is a clarity/maintainability refactor, not a performance one.

## Why (semantic)

The pattern is pervasive and intent-erased. Census of the compiler:

```
Dict<Int, Bool> decls:  426     # hand-rolled int-SETS (value is always true)
Dict<Int, *>    decls: 1006     # int-keyed maps, key meaning by convention only
Set<Int>  uses in compiler: 0   # builtin Set unused in boot/compiler (5 in test fixtures)
```

A bare `Dict<Int, Bool>` doesn't say whether the `Int` is a LocalId or a block id,
or that it's really a set — `ownership.tw`/`route_typed_vec.tw` carry that only in
comments (e.g. `route_typed_vec.tw`'s `acc: Dict<Int, Bool>`, whose comment admits
"acc is a Set"). Named types fix this: `exits: BlockMap<LocalMap<Int>>` *says* the
outer key is a block and the inner a local, and the compiler rejects passing a
`BlockMap` where a `LocalMap` is expected.

## Goal / non-goals

**Goal.** Give the ownership fixpoint's threaded int-keyed maps/sets named types
(`IntMap`/`IntSet` core + `LocalMap`/`LocalSet`/`BlockMap`/`BlockSet` semantic
wrappers), adopted as a **byte-identical rename over the existing HAMT backing**, so
intent is in the type and mixing key kinds is a compile error.

**Non-goals.**
- **Changing the map backing / any performance swap** — investigated and rejected
  (below). The backing stays `Dict` (persistent HAMT).
- The broad 426-site sweep — deferred to a follow-up after the fixpoint surface is
  migrated (see [Footnote](#footnote-the-broad-sweep-deferred)).
- In-place mutation of these maps (the sound-uniqueness storage-track / `MutDict` /
  owned-specialized-ABI work) — a separate, orthogonal track, blocked on escape
  precision, not addressed here.

## Performance: measured null result

The original motivation was throughput: the ownership fixpoint dominates self-compile
(`variant_specialize` ~8.5s + `produce_mutable_decisions` ~4.1s of ~19.5s wall), and
its hot maps are HAMT `Dict<Int,T>`. Two spikes settled that a representation swap is
**not** the lever:

- `boot/bench/fixpoint_map_spike.tw` (round 1) measured a flat sorted-array **13–19×**
  faster than the HAMT — but only on the *fork-everything* (M=W) case, and measured
  no reads.
- `boot/bench/fixpoint_map_divergence_spike.tw` (round 2) measured the *real* access
  mix at realistic widths (W=32/64). Result:

  | path | winner | why |
  |---|---|---|
  | fresh **build** | array 2–5× | no hashing, sequential fill |
  | point **read** (`fact_of_local`, high volume) | **HAMT 7–12×** | at W=32–60 the HAMT is 1–2 levels deep → get ≈ one hash + a hop; binary search is 5–6 branchy iterations |
  | **merge** fork, M/W ≲ 20% (convergence common case) | **HAMT** | O(M·log W) structural sharing beats the array's unconditional O(W) rebuild |
  | merge fork, M/W → 100% | array | rebuild amortizes |

  Reads (the highest-volume op) and low-divergence merges — the bulk of the real
  workload — favor the HAMT, and a single backing can't win builds *and* reads. Net:
  a sorted-array (or Patricia) backing is a wash-to-regression at these sizes.

**Conclusion:** the HAMT is already well-suited to small, shallow int-keyed maps; the
perceived "hash+traverse overhead" is, at width 32–60, a near-constant 1–2-hop get.
Keep the HAMT backing. The only residual perf risk is the wrapper allocation this
refactor *adds* (below), which Slice 2 measures rather than assumes.

## Type design (two layers)

Separate representation from semantics; both layers sit over the HAMT.

**Representation core (shared).** Generic `IntMap<T>` and `IntSet`, **delegating to
`Dict`**. API is only the surface the fixpoint uses:
- `IntMap<T>`: `new`, `get`/`get_or(default)`, `has`, `set`, iterate, `len`.
- `IntSet`: `new`, `insert`, `contains`, iterate, `len`.

Iteration **delegates to `Dict`**, preserving current insertion-order iteration —
so adoption is byte-identical (see [Correctness](#correctness-discipline)).

**Semantic layer (distinct nominal types).** Thin nominal wrappers where the key's
meaning matters and mixing would be a latent bug:
- `LocalMap<T>`, `LocalSet` — keyed by a LocalId.
- `BlockMap<T>`, `BlockSet` — keyed by a block id.

Each wrapper is `.{ inner: IntMap<T> }` (or `.{ inner: IntSet }`) with delegating
inherent methods; the type name carries the semantics. Keys stay `Int` at the API
(matching how `blk.entry.live` threads raw ints); wrapping keys as `LocalId`/`BlockId`
records is a later step only where a site wants it. Map *values* stay as-is
(`Ownership` as its `own_tag()` Int) so adoption is a pure rename; value-detagging is
an optional readability follow-up, out of scope.

**Wrapper-cost note.** A nominal wrapper is a one-field GC struct, and a nested
`BlockMap<LocalMap<Int>>` stacks two wrappers over two `IntMap`s — so a fork can add
up to four small-struct allocations. On a HAMT-backed rename this is the *only* perf
delta, and it's a potential small regression (extra allocations, no offsetting
representation win). Slice 2 measures it (below); if it regresses materially, fall
back to type *aliases* (zero-cost, but non-nominal — documentation only) for the
hottest maps.

## Work slices

Each slice gates on the project standard: **byte-identical emitted output** (A/B
diff), the `make stage2` fixed point (stage3 == stage4), and boot-test green. The
FIXVERIFY acceptance is the census *delta* (with vs without), never "FIXVERIFY
prints nothing" — there is a known-red baseline.

### Slice 0 — baseline

- Record wall + `variant_specialize` / `produce_mutable_decisions` phase timings
  (`TWINKLE_TIMINGS=1`) as the before-picture, so Slice 2 can prove the rename did
  not regress. (No representation microbench needed — that question is settled.)

### Slice 1 — the types, HAMT-backed

- Define `IntMap<T>` / `IntSet` delegating to `Dict`, plus the `LocalMap<T>` /
  `LocalSet` / `BlockMap<T>` / `BlockSet` semantic wrappers, in a **new standalone
  module** imported by `ownership.tw` — *not* inline. Rationale: project memory
  records an unresolved crash class where adding a function to a heavily-imported
  module trips a boot-compiler PVec OOB tied to function-index shifts; `ownership.tw`
  (~8.7k lines) is centrally imported, so the new surface lives in its own module
  and `ownership.tw` only gains an import.
- Unit tests for the API surface. **Smoke gate:** run `make boot-test` immediately
  after this slice, before any adoption, specifically to catch that index-shift
  crash class early.
- Nothing adopts the types yet → byte-identical by construction.

### Slice 2 — adopt across the full fixpoint map surface (pure rename)

- Migrate the **complete** `run_fixpoint` int-keyed map/set surface — the "14 maps"
  (`ownership.tw:6148`) *plus* the block-keyed maps that live outside `FixState` and
  were easy to miss: `dirty` (`ownership.tw:6071/6244`), `succ`
  (`ownership.tw:5170`). Classify each:
  - **Inner (`LocalMap`/`LocalSet`):** `join_entry_ownership`/`_valid`/`_prov`
    results, `merge_targeted`, per-block own/valid/prov values, the live/owned
    `Dict<Int,Bool>` sets.
  - **Outer (`BlockMap`/`BlockSet`):** `exits`, `exit_valid`, `exit_prov`,
    `exit_field_own`, `exit_path_prov`, `prev_exits`/`_valid`/`_prov`, `prev_seen`,
    `changed_visits`, `processed`, `dirty`, `succ`. Include the `field_own`/`path_prov`
    exit maps and their bespoke merges (`merge_field_own_exit` ~5707,
    `merge_path_prov_exit` ~5750).
  - Any map deliberately left as raw `Dict` must be **named with a reason**, not
    silently omitted.
- **Update public API consumers.** `merge_targeted` is `pub` and called with literal
  `Dict<Int,…>` args at ~12 sites in `boot/tests/suites/cfg_lattice_suite.tw`
  (nominal types ⇒ these call sites must change to the new types). Rewriting that
  suite is an explicit task of this slice.
- **Perf gate (not just correctness):** besides byte-identical + stage2 + boot-test,
  re-record the Slice 0 timings and confirm no material wall/phase regression from
  the added wrapper allocations. If it regresses, downgrade the hottest wrappers to
  aliases per the wrapper-cost note.

### Slice 3 — optional micro-cleanup (independent of this refactor)

- `merge_targeted` materializes `int_keys_union(old.keys(), next.keys())` (two key
  vectors + sorted-insert) every merge. A fused sorted co-iteration over the two
  backings would drop that allocation — a small win independent of the type work.
  Low priority; do only if a profile still points here. Not required for the
  semantic goal.

## Correctness discipline

- Byte-identity at every slice. Because iteration delegates to `Dict`, insertion-order
  iteration is preserved exactly, so the rename is byte-identical by construction —
  the map's runtime behavior is unchanged, only its static type differs.
- **Iteration-order audit (recorded for any future backing change).** No fixpoint
  algorithm depends on map iteration order for correctness; order matters only for
  byte-identical reproducibility, and the codebase already secures that by **explicit
  ascending sort at every output boundary** (`int_keys_union`, `fixv_isort(m.keys())`
  ~5904–5994, `sorted_dict_keys` — comment: *"Ascending path-key iteration order … so
  Direct ret_paths come out sorted"*), not by relying on `Dict` insertion order. Every
  unsorted `.keys()` iteration is order-independent (map-union / merge-into-new-map /
  `*_eq`). So *if* a backing swap is ever reconsidered, ascending-key iteration would
  also be byte-identical — but this refactor doesn't change the backing, so the point
  is moot here and kept only as context.

## Footnote: the broad sweep (deferred)

The same pattern spans the compiler (426 `Dict<Int,Bool>` int-sets, 1006
`Dict<Int,*>` maps; counts approximate, `grep`-derived). These are a mechanical
semantic cleanup riding the same types. **Out of scope** for this plan; migrate only
after the fixpoint surface is done, as a separate follow-up — each site picks its
semantic wrapper; non-hot sites take the change purely for readability.

## Cross-reference: incremental backend cache

The in-flight per-module on-disk backend cache does not currently serialize
`FixResult`/`FixCache` (verified). If it later persists fixpoint state, the two
efforts should coordinate the on-disk shape then — but nothing in this rename affects
that.

## References

- Spikes: `boot/bench/fixpoint_map_spike.tw` (round 1, M=W only) and
  `boot/bench/fixpoint_map_divergence_spike.tw` (round 2, real mix → HAMT wins).
- Hot code: `boot/compiler/ownership.tw` (`run_fixpoint`, `merge_targeted`,
  `join_entry_ownership`/`_valid`/`_prov`, the `FixState` maps, `dirty`, `succ`).
- Baseline: `docs/plans/performance/compiler.md`.
- API consumer to migrate: `boot/tests/suites/cfg_lattice_suite.tw`.
