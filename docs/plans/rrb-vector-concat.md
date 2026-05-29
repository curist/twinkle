# RRB-Tree Vector: O(log n) Concat

Status: proposal. Primary target: `boot/compiler/codegen/runtime/arr.tw` (the
boot compiler is the main implementation). `src/runtime/arr.rs` (stage0) is
mirrored afterward to stay a correctness reference.

## Problem

Twinkle's `Vector<T>` is a Clojure-style **bit-partitioned persistent trie**
(branching factor 32, 32-element tail). Its cost model today:

| Op | Cost | Loop-accumulation cost |
|---|---|---|
| `append` (`push`) | O(log₃₂ n) amortized (tail copy + occasional spine) | `acc = acc.append(x)` → **O(n log n)**, effectively linear |
| `concat(a, b)` | **O(\|b\|)** — proportional to the **right** operand; `a` is shared, `b` is replayed | depends on which side grows (below) |
| `get` / `set` | O(log₃₂ n) | — |
| `slice` | O(n) — replays elements | — |

`concat` is implemented differently in the two runtimes but with the **same
asymptotics**: stage0 (`src/runtime/arr.rs:995`) literally *"iterate b and push
each element onto a"*; boot (`boot/compiler/codegen/runtime/arr.tw`, the primary
target) does `builder_freeze(builder_extend(builder_from(a), b))`, where
`builder_from(a)` shares `a`'s trie and copies only its ≤32 tail (O(1)), and
`builder_extend` flattens `b` via `to_array` and pushes each element (O(\|b\|)).
Either way the left operand is cheap/shared and the cost is proportional to the
right operand.

Because `concat` cost is proportional to the **right** operand:

- `acc = acc.concat(piece)` (append at end) → O(Σ\|pieceᵢ\|) = **O(n)** (boot) /
  O(n log n) (stage0). Fine.
- `acc = piece.concat(acc)` (**prepend / accumulator on the right**) → the entire
  accumulator is replayed every iteration → **O(n²)**.

The prepend / right-operand case is genuinely polynomial, and it is inherent to a
**non-relaxed** trie: true sub-linear concatenation of two persistent vectors
requires relaxed radix-balanced (RRB) nodes.

The static-uniqueness optimizer (`docs/plans/static-uniqueness-plan.md`) does
**not** fix this. It is a constant-factor / allocation-churn optimization
(in-place builder reuse for *append-at-end* consume-reassign chains) and changes
nothing asymptotically. Its `concat → builder_extend` rewrite only fires for
left-base consume-reassign shapes; a right-operand-accumulator concat is left as
a real persistent `concat` call. So the persistent `concat` itself must become
sub-linear.

## Goal

Upgrade the persistent vector to an **RRB-tree** (relaxed radix-balanced) so that:

- `concat(a, b)` is **O(log n)** for arbitrary operands (fixes prepend and all
  general concatenation).
- `append`, `get`, `set` keep their current asymptotics (O(log₃₂ n)); the common
  append-built vector stays fully *regular* (no size tables) so its constant
  factors do not regress.
- `slice` optionally drops to O(log n) (a natural RRB bonus; see Phasing).

References: Bagwell & Rompf, *RRB-Trees: Efficient Immutable Vectors* (2011);
L'orange, *Improving RRB-Tree Performance through Transience* (2014);
Stucki et al., *RRB Vector* (Scala, 2015).

## Non-goals

- No change to the user-facing API or `Vector<T>` semantics — purely a runtime
  representation upgrade. Programs observe the same results, only faster concat.
- No change to the static-uniqueness model (no refcounts, no ownership syntax).
- Not required to make `get`/`set` faster; only to avoid regressing them.
- Not a general balancing/persistence rework beyond what O(log n) concat needs.

## Background: current representation

```
$PVec        { len: i32, shift: i32, root: ref null $VecInternal, tail: ref $Array }
$VecInternal { children: ref $VecChildren }          // VI_CHILDREN = 0
$VecChildren = array of eqref                         // slot = VecInternal | leaf Array
```

Identical field layout in both runtimes: `PV_LEN=0`, `PV_SHIFT=1`, `PV_ROOT=2`,
`PV_TAIL=3`, `VI_CHILDREN=0`. Leaf arrays live directly in a parent's `children`
(no leaf wrapper). Navigation (`get_leaf`, `do_set`, `push_tail`, `new_path`)
uses pure radix indexing `(idx >> shift) & 31`. `tailoff` splits trie-resident
elements from the tail. `empty_pvec` / `empty_leaf` are shared singletons.

Boot-specific notes (the primary target has slightly diverged from stage0):

- `concat` delegates to the transient builder path
  (`builder_from` → `builder_extend` → `builder_freeze`), so the RRB rewrite in
  Phase 3 replaces `concat`'s body with the spine-merge algorithm while leaving
  `builder_extend` (used by the optimizer's append-at-end rewrite) intact.
- Boot also defines `set_in_place` (optimizer in-place set) and
  `promote_full_tail`, which **construct/copy `VecInternal` nodes** and so must be
  audited in Phase 1 alongside `push_tail`, `new_path`, `do_set`, and the builder
  freeze/promote paths.

## RRB design

### 1. Relaxed nodes via an optional size table

Add a size-table field to internal nodes:

```
$VecInternal { children: ref $VecChildren, sizes: ref null $I32Array }
```

- `sizes == null` ⇒ **regular** node: every child except possibly the last is
  full for its level (`1 << shift` elements). Indexed by pure radix — the fast
  path, unchanged.
- `sizes != null` ⇒ **relaxed** node: `sizes[i]` = cumulative element count
  through child `i`. Children may be under-full. Created only at concat seams.

This is additive and keeps the common case (append-built trees are entirely
regular) free of size tables and free of slowdown. Needs an i32-array GC type
for `sizes` (reuse an existing i32 array type if one exists, else add one in
`types.rs` / `types.tw`).

### 2. Indexing with relaxed nodes (`get`, `set`)

Per level, branch selection becomes:

- regular node → `slot = (idx >> shift) & 31` (current behavior).
- relaxed node → radix *guess* `slot = (idx >> shift) & 31`, then advance while
  `sizes[slot] <= idx` (a short forward scan, ≤ slack), then
  `idx -= sizes[slot-1]` before descending.

Still O(log n); the only overhead is the size-table step on relaxed nodes, which
append-built vectors never hit. `do_set` gets the same treatment and must
preserve/copy the `sizes` table when copying a relaxed node.

### 3. Concat algorithm (the payoff)

Standard RRB concatenation, O(log n):

1. **Fold tails.** Concat operates on full trees. Push `a`'s tail into `a`'s trie
   (or treat it as the rightmost leaf to merge); `b`'s tail becomes the new
   result tail after merge (or is folded and re-extracted). Decide one
   convention and apply consistently (see Open questions).
2. **Merge spines bottom-up.** Walk `a`'s right spine and `b`'s left spine to the
   leaves. At the leaf level, the rightmost leaf of `a` and leftmost of `b` are
   the merge boundary.
3. **Rebalance the merged level.** Redistribute the boundary nodes' children so
   each node holds within the slack invariant of full (`m - e` … `m` children,
   `e` typically 1–2). This is what keeps height bounded under repeated concat
   (prevents "height creep"). The rebalanced parent becomes a **relaxed** node
   with a freshly computed size table (unless all children came out full, in
   which case it may stay regular — see canonicalization).
4. **Propagate upward**, creating relaxed parents at each level of the seam, up
   to a (possibly new) root. Recompute `shift`/`len`.

Total work touches only the two spines and the seam → O(log n) nodes.

### 4. Canonicalization (keep get fast)

After building a relaxed node, if all its children turn out full for the level,
drop the size table (store `null`) so it stays radix-indexable. This keeps
append-built and aligned-concat results on the fast path and bounds the spread
of relaxed nodes.

### 5. Builder / transient interaction

The transient builder path (`builder_new/from/push/extend/freeze`) is the
linear append-accumulation fast path and is unaffected asymptotically:

- `builder_freeze` builds a sequential, fully-regular tree (no size tables) —
  unchanged.
- `builder_extend(builder, vec)` currently replays `vec` element-by-element. It
  *may* later be upgraded to splice `vec`'s leaves in bulk, but that is an
  optimization, not required by this plan.
- The static-uniqueness `concat → builder_extend` rewrite for *append-at-end*
  consume-reassign chains remains valid and is still preferred where it fires.
  The new RRB `concat` is the O(log n) **persistent fallback** that the optimizer
  cannot rewrite — notably the prepend / right-operand-accumulator case.

### 6. `slice` (optional, phased)

RRB supports O(log n) left/right slice (trim spine, mark boundary nodes relaxed).
This is a natural follow-on once relaxed nodes exist, replacing the current O(n)
replay. Treated as a later phase, not part of the core concat fix.

## Invariants

- **Regular**: `sizes == null`; all children but the last are full for the level.
- **Relaxed**: `sizes != null`, monotonically increasing, `sizes[last] == ` node
  element count; children may be under-full.
- A relaxed node's children may themselves be regular or relaxed.
- Height stays within the RRB slack bound after concat (rebalancing guarantee).
- `empty_pvec` and append-only construction remain entirely regular.

These join the existing structural invariants documented at the top of
`arr.rs` (tail length, `root == null iff len <= 32`, `shift == 5·depth`, etc.).

## Post-change cost contract

| Op | Before | After |
|---|---|---|
| `append` | O(log₃₂ n) amortized | unchanged |
| `get` / `set` | O(log₃₂ n) | unchanged on regular nodes; O(log n) with small size-table constant on relaxed |
| `concat(a,b)` | O(\|b\|) → **O(n²)** prepend loop | **O(log n)**; prepend loop → O(n log n) |
| `slice` | O(n) | O(n) now, O(log n) after the optional slice phase |

Publishing this table in `docs/spec.md` / `docs/API.md` as the vector cost
contract is part of the deliverable, so the performance characteristics are a
documented guarantee rather than an emergent property.

## Justification & benchmarking gate

RRB concat is intricate. Before committing to it, the benefit must be **measured
and obvious**, not assumed. This work is gated on two pre-implementation steps
that are cheap (no RRB code required) and can kill or green-light the project:

### Gate A — Is the pattern actually hit? (real-world audit)

Grep every `.concat(` / `Vector.concat` call site in `boot/` and the stdlib and
classify each as append-at-end (left-operand accumulator, fine), prepend /
right-operand accumulator (the O(n²) case), or one-shot. If essentially nothing
in real code prepends or right-accumulates in a loop, the polynomial blowup is
theoretical and the complexity is **not** justified — stop here and instead
document the pitfall (cheap) and rely on the cost contract.

Output: a short table of concat call sites and their classification, plus any
known user/program workloads (text/rope building, list prepend) that would hit it.

### Gate B — Quantify the blowup and the win on the *current* runtime

Write microbenchmarks against the **current** vector (no RRB needed) to (1) prove
the O(n²) curve empirically and (2) establish baselines RRB must beat. Time the
hot loop internally with `@std.date` (`date.now()` returns a `Float` timestamp,
already used by `TWINKLE_TIMINGS`) so startup/compile time is excluded; run via
`target/twk run bench/<name>.tw`. Use `hyperfine` only for whole-process sanity.

Benchmarks (each parameterized by N, run at N = 1k, 2k, 4k, 8k, 16k, 32k):

```tw
// bench/concat_prepend.tw — the target case. Expect ~4× per doubling (quadratic).
acc: Vector<Int> = []
for i in range(n) { acc = [i].concat(acc) }       // right-operand accumulator

// bench/concat_append.tw — control. Must stay linear on BOTH old and new.
acc: Vector<Int> = []
for i in range(n) { acc = acc.concat([i]) }

// bench/concat_balanced.tw — pairwise/tree concat of many small vectors.
// bench/get_regular.tw — N random get() on an append-built (regular) vector.
// bench/get_relaxed.tw — N random get() on a concat-built vector (post-RRB:
//   exercises relaxed-node navigation; pre-RRB: baseline get cost).
// bench/set_regular.tw, bench/set_relaxed.tw — same for set().
```

Record a table of `N → ms` per benchmark and confirm the prepend curve is
quadratic (each doubling of N ≈ 4× time) while append is linear (≈ 2×).

### Decision criteria (when RRB is worth it)

Re-run the same benchmarks after a prototype Phase 3 and require **all** of:

- **Prepend / right-accumulator**: clearly sub-quadratic — each doubling of N
  trends toward ≈2× (linear-ish), and an order-of-magnitude wall-clock win at
  N ≥ 8k. This is the headline justification and must be unmistakable.
- **Append-at-end (control)**: no regression beyond noise (RRB must not slow the
  common path).
- **`get`/`set` on regular (append-built) vectors**: no regression — these stay
  on the radix fast path with no size table.
- **`get`/`set` on relaxed (concat-built) vectors**: regression bounded and
  documented (target ≤ ~1.5–2× the regular cost; size-table navigation is the
  known tax). If relaxed get/set is dramatically slower, reconsider.

If the prepend win is not obvious, or regular get/set regresses, **do not merge**
— the complexity is not paid for. Keep the benchmark suite in `bench/` as a
permanent regression guard regardless of outcome.

## Implementation plan

The `rt.arr` module is emitted by Wasm-building code in **both** runtimes. Per
CLAUDE.md, the boot compiler is primary: **all phases land in
`boot/compiler/codegen/runtime/arr.tw` first**, then mirror into stage0
`src/runtime/arr.rs` so it stays a correctness reference. Differential tests
(stage0 vs boot) are the gate that the two agree.

**Phase 0 — Justify, spec & harness.** Run **Gate A** (call-site audit) and
**Gate B** (baseline benchmarks on the current runtime) above; proceed only if
the pattern is real and the blowup is measurable. Then write a precise pseudocode
spec of relaxed nodes, indexing, and the concat/rebalance algorithm, and add the
differential + scaling test harness *first* (see Testing) so every later phase is
checked. The `bench/` suite from Gate B becomes the before/after evidence and a
permanent regression guard.

**Phase 1 — Representation.** Add the `sizes` field to `$VecInternal` and the
i32-array type, in the boot `types.tw` first (then mirror to `types.rs`). Update
every site that constructs/copies a `VecInternal` to pass `sizes` (null for all
existing regular construction). No behavior change yet; all nodes regular.
Re-run full suite.

**Phase 2 — Relaxed-aware indexing.** Update `get`/`get_leaf`/`do_set`/`set`
navigation to handle relaxed nodes (radix guess + size-table correction).
Still no relaxed nodes are *produced*, so this is exercised via hand-built
fixtures until Phase 3.

**Phase 3 — RRB concat.** Replace `concat` with the merge-spines + rebalance
algorithm producing relaxed seam nodes, with canonicalization. Land in **boot
`arr.tw` first**, validate against the scaling + invariant harness, then mirror
into stage0 `arr.rs`. Differential-test the two.

**Phase 4 — Optimizer reconciliation.** Confirm the static-uniqueness pass still
behaves: append-at-end still rewrites to builder; prepend/right-operand concat
falls through to the new O(log n) `concat`. Add a guard fixture for the prepend
loop demonstrating O(n log n) scaling.

**Phase 5 (optional) — O(log n) slice** and **bulk `builder_extend`**.

## Testing & characterization

- **Scaling guard**: a fixture that prepends in a loop (`acc = [x].concat(acc)`)
  for growing N and asserts near-linear (not quadratic) growth — the regression
  this plan exists to prevent.
- **Invariant checker**: a debug routine validating size-table monotonicity,
  regular-node fullness, and height bound on random concat trees.
- **Differential opt/no-opt** and **stage0/boot** correctness on randomized
  append/concat/slice/get/set sequences (extend the existing `cow_analysis` and
  vector test fixtures).
- Keep all current `tests/opt/*vector*` characterization fixtures green;
  rebalancing must not change observable results.

## Risks & mitigations

- **Algorithm complexity.** RRB concat with rebalancing is intricate and easy to
  get subtly wrong. Mitigation: spec-first, fuzz/differential harness before
  implementation, land in one runtime before mirroring.
- **`get`/`set` slowdown.** Only on relaxed nodes; append-built trees stay
  regular. Mitigation: radix-first guess + canonicalization to drop size tables
  when children are full.
- **Height creep** under repeated concat if rebalancing is skipped. Mitigation:
  implement the bounded-slack rebalance from the start; do not ship a
  naive "single relaxed root" concat as the end state.
- **GC layout change** ripples to every `VecInternal` constructor and to
  `from_array`/`to_array`/builder freeze. Mitigation: Phase 1 isolates the
  additive field change with the suite green before any algorithmic change.
- **Two runtimes drift.** Mitigation: differential tests are the gate; mirror
  only after the lead runtime is validated.

## Relationship to other work

- **Static-uniqueness plan** — complementary and unchanged. That plan minimizes
  COW/allocation for linear append-accumulation; this plan fixes the *asymptotic*
  cost of persistent `concat`. They do not conflict.
- **Bulk `builder_extend`** and a **written cost contract** were the other
  candidate directions; the cost contract is folded in here (the post-change
  table), and bulk extend is an optional Phase 5 once relaxed nodes exist.

## Open questions

- **Tail convention in concat**: fold both tails into the tries and re-extract a
  tail from the result, or keep `b`'s tail as the result tail and only fold `a`'s?
  Pick the simpler-to-verify convention in Phase 0.
- **Slack parameter `e`**: 1 or 2 children of slack. Standard RRB uses `e = 2`;
  confirm against the branching factor 32 and measured get cost.
- **i32 size-table type**: reuse an existing i32 array GC type if present, or add
  a dedicated `$I32Array` (and whether size tables should instead be packed into
  the existing eqref `VecChildren` to avoid a second array type).

(Lead runtime is settled: boot `arr.tw` leads every phase, stage0 `arr.rs`
mirrors after.)
