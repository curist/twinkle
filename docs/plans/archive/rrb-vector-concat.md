# RRB-Tree Vector: O(log n) Concat & Slice

Status: **archived (2026-05-31).** RRB concat/slice work landed in the boot
runtime and was mirrored to stage0. The Phase 7 classical-slack follow-up
recorded below ended as a wash for the narrow prototype and was reverted.

Originally unparked from the Gate A red result below by an explicit design decision: make
`Vector<T>` the **single universal sequence** — cheap at *both* ends and for
*arbitrary* concat/slice — so one type covers stack / queue / deque / rope
without bespoke wrapper types (consistent with deleting `@std.stack`: the
capability belongs on `Vector`, not in wrappers). This is a **forward-looking**
choice, not a reaction to a new hot loop: Gate A still finds no dominant
prepend/left-drop loop in today's codebase ([Gate A result](#gate-a-result-2026-05-29--red--parked)),
and we are deliberately building the capability ahead of a proven in-repo
workload. Sequence: Gate B baselines (quantify today's curves) → relaxed-node
implementation. Primary target: `boot/compiler/codegen/runtime/arr.tw` (the boot
compiler is the main implementation); `src/runtime/arr.rs` (stage0) is mirrored
afterward to stay a correctness reference.

> **Two O(n²) loop hazards motivate this**: prepend/right-operand `concat`, and
> any `slice` that trims a little each iteration (drop-last / drop-first /
> sliding window). The **LIFO drop-last** case is already covered more cheaply by
> the shipped O(log n) `Vector.drop_last` op ([stack.md](../stack.md)), with
> read-only traversal served by `View<C>` ([view.md](../view.md)). RRB is the
> general-purpose fix for the rest — *arbitrary* concat, arbitrary-range slice,
> and cheap `drop_first`/`prepend` (the queue/deque half). See
> [Alternatives](#alternatives--complementary-work).

## Problem

Twinkle's `Vector<T>` is a Clojure-style **bit-partitioned persistent trie**
(branching factor 32, 32-element tail). Its cost model today:

| Op | Cost | Loop-accumulation cost |
|---|---|---|
| `append` (`push`) | O(log₃₂ n) amortized (tail copy + occasional spine) | `acc = acc.append(x)` → **O(n log n)**, effectively linear |
| `concat(a, b)` | **O(\|b\|)** — proportional to the **right** operand; `a` is shared, `b` is replayed | depends on which side grows (below) |
| `get` / `set` | O(log₃₂ n) | — |
| `slice(v, s, e)` | **O(m)**, m = e−s — fully re-materializes the range into a fresh vector, shares nothing with the source | trim-one loop → **O(n²)** (below) |

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

`slice` has the **same hazard family**. A single `slice` is O(m) in the slice
length (boot copies leaf-runs in bulk via `ArrayCopy` + `from_array`; stage0
replays element-by-element at O(m·log n)) — fine one-shot, but it shares nothing
with the source, so trimming a little each iteration re-copies almost everything:

- `acc = acc.slice(1, acc.len())` (**dequeue / drop-first**) → O(n²)
- `acc = acc.slice(0, acc.len() - 1)` (**drop-last / pop**) → O(n²)
- sliding-window scans → O(n·w) per element copied repeatedly

This is arguably **more common than prepend-concat** — it is exactly what using a
`Vector` as a stack looks like. Since drop-last/stack is an essential workload,
slice is treated here as a **co-primary** motivation, not a bonus. (The cheap
non-RRB answer for it is [stack.md](../stack.md); RRB still covers arbitrary and
left-drop slice.)

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
- `slice(v, s, e)` is **O(log n)** (relaxed left/right slice that shares the
  spine), fixing dequeue / drop-last / window loops. Co-primary with concat.
- `append`, `get`, `set` keep their current asymptotics (O(log₃₂ n)); the common
  append-built vector stays fully *regular* (no size tables) so its constant
  factors do not regress.

For the **LIFO drop-last workload** (the audit's real finding), the shipped
O(log n) `Vector.drop_last` op ([stack.md](../stack.md)) is the cheaper, non-RRB
answer; read-only traversal goes through `View<C>` ([view.md](../view.md)). RRB is
the general-purpose fix. See [Alternatives](#alternatives--complementary-work).

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
   each node holds within the classical RRB slack invariant of near-full
   occupancy (`m - e` … `m` children for interior nodes, with bounded edge
   exceptions when the local window cannot mathematically fill another node).
   This is what keeps height bounded under repeated concat (prevents "height
   creep") while preserving good constants. The rebalanced parent becomes a
   **relaxed** node with a freshly computed size table (unless all children came
   out full, in which case it may stay regular — see canonicalization).

   **Current boot baseline.** The first implementation uses a simpler local
   even-redistribution rule across the minimum number of parent nodes. This is
   not the classical slack invariant; it is a pragmatic baseline that avoids
   one-child overflow chains and has passed the scaling guards. Phase 7 below
   exists to compare that baseline against stricter classical/windowed RRB
   rebalancing.
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

### 6. `slice` (co-primary)

RRB supports O(log n) left/right slice: descend to the start and end boundaries,
trim the spine, and mark the boundary nodes relaxed (the trimmed children become
under-full, hence size tables). The result shares all interior structure with the
source instead of re-materializing it. This is what turns dequeue / drop-last /
window loops from O(n²) into O(n log n). It reuses the same relaxed-node
machinery as concat, so it is part of the core work, not a bonus.

## Invariants

- **Regular**: `sizes == null`; all children but the last are full for the level.
- **Relaxed**: `sizes != null`, monotonically increasing, `sizes[last] == ` node
  element count; children may be under-full.
- A relaxed node's children may themselves be regular or relaxed.
- Height stays bounded after concat. The current boot baseline guarantees this
  by evenly redistributing overflow across the minimum number of parent nodes;
  Phase 7 evaluates the stricter classical near-full slack invariant.
- `empty_pvec` and append-only construction remain entirely regular.

These join the existing structural invariants documented at the top of
`arr.rs` (tail length, `root == null iff len <= 32`, `shift == 5·depth`, etc.).

## Post-change cost contract

| Op | Before | After |
|---|---|---|
| `append` | O(log₃₂ n) amortized | unchanged |
| `get` / `set` | O(log₃₂ n) | unchanged on regular nodes; O(log n) with small size-table constant on relaxed |
| `concat(a,b)` | O(\|b\|) → **O(n²)** prepend loop | **O(log n)**; prepend loop → O(n log n) |
| `slice(v,s,e)` | O(m) → **O(n²)** trim/dequeue loop | **O(log n)**; trim loop → O(n log n) |

Publishing this table in `docs/spec.md` / `docs/API.md` as the vector cost
contract is part of the deliverable, so the performance characteristics are a
documented guarantee rather than an emergent property.

## Justification & benchmarking gate

RRB concat is intricate. Before committing to it, the benefit must be **measured
and obvious**, not assumed. This work is gated on two pre-implementation steps
that are cheap (no RRB code required) and can kill or green-light the project:

### Gate A — Is the pattern actually hit? (real-world audit)

Audit both operations across `boot/` and the stdlib:

- Every `.concat(` / `Vector.concat` site → append-at-end (fine), prepend /
  right-operand accumulator (O(n²)), or one-shot.
- Every `.slice(` site → one-shot (fine) or trim-in-a-loop (dequeue / drop-last /
  window — the O(n²) case).

If essentially nothing loops on the bad pattern, the blowup is theoretical and
RRB is **not** justified — stop and just document the pitfall.

Crucially, **separate the LIFO/traversal findings from the rest**: drop-last and
head/tail traversal are far better served by `drop_last`/`View`
([Alternatives](#alternatives--complementary-work)) than by RRB. So the audit
should report (a) drop-last + read-only drop-first sites → motivate
`drop_last`/`View`, and (b) *arbitrary* concat/slice loop sites
(esp. left-drop) → the residual that only RRB fixes. RRB's go/no-go rests on (b),
since (a) is solved more cheaply. (This audit is already done — see
[slice-performance.md](../slice-performance.md).)

Output: a table of concat + slice call sites and their classification, plus known
workloads (text/rope building, list prepend, windowed scans).

#### Gate A result (2026-05-29) — RED → parked

Classification of the ~553 `.concat(` sites + all `.slice(` sites across `boot/`
and the stdlib:

| Pattern | Count / location | Verdict |
|---|---|---|
| String `.concat` / `.slice` | printer, parser, lexer, json, source modules | **Not RRB** — String slice goes the `View` route ([slice-performance.md](../slice-performance.md)) |
| `doc.concat([...])` | `fmt/doc.tw` pretty-printer combinator | **Not `Vector.concat`** at all |
| Append-at-end `acc = acc.concat(x)` (incl. dot-shorthand `acc = .concat(x)`) | the large majority of Vector sites | **Safe** — already rewritten to `builder_extend` by `opt/builder_region.tw` |
| Prepend-in-loop `acc = X.concat(acc)` | only `lines = [rest].concat(lines)` — `signatures.tw:208`, `parser.tw:2972`, `query/hover.tw:896` | **Bounded** — doc-comment (`///`) gathering; N = comment-block size (tiny), not a hot loop |
| Vector left-drop `xs = xs.slice(1, …)` | `loader.tw:74` (strip leading `"std"`), `checker.tw:1935/2006` (drop receiver param) | **One-shot** — no loop |
| `Vector.drop_first` | tests only (`api_vector_suite.tw`) | **No production caller** |

**Conclusion (as of the audit).** No production hot loop hits the prepend-concat
or left-drop/`drop_first` pattern that RRB would accelerate. The LIFO drop-last
residual that *was* real is already covered by the shipped O(1)-amortized
`Vector.drop_last` op (a regular-radix-trie tail shrink — **never needed RRB**).
On the audit's own gate ("if essentially nothing loops on the bad pattern … stop
and just document the pitfall"), RRB was **not data-justified**, so it was parked.

**Override (2026-05-30) — pursuing RRB anyway, as a design decision.** The project
has chosen to make `Vector<T>` the single universal sequence (stack/queue/deque/
rope) rather than wait for a hot loop or add per-shape wrapper types. This is an
explicit, eyes-open override of the Gate A finding: the data above still stands
(no dominant bad loop *today*), and we accept investing ahead of demand because
`drop_first`/`prepend` are the missing half of the "one collection for everything"
goal, and a cheap arbitrary `concat`/`slice` is broadly enabling (e.g. ropes for
the editor/LSP). **Honesty note for whoever picks this up:** the justification is
strategic, not a measured regression — so still build Gate B baselines first
(quantify the current curves and set the bar RRB must beat), but treat them as
*baselines to establish*, not a go/no-go gate that could re-park the work.

### Gate B — Quantify the blowup and the win on the *current* runtime

Write microbenchmarks against the **current** vector (no RRB needed) to (1) prove
the O(n²) curve empirically and (2) establish baselines RRB must beat. Time the
hot loop internally with `@std.date` (`date.now()` returns a `Float` timestamp,
already used by `TWINKLE_TIMINGS`) so startup/compile time is excluded; run via
`target/twk run boot/bench/<name>.tw`. Use `hyperfine` only for whole-process sanity.

Benchmarks (each parameterized by N, run at N = 1k, 2k, 4k, 8k, 16k, 32k):

```tw
// boot/bench/concat_prepend.tw — the target case. Expect ~4× per doubling (quadratic).
acc: Vector<Int> = []
for i in range(n) { acc = [i].concat(acc) }       // right-operand accumulator

// boot/bench/concat_append.tw — control. Must stay linear on BOTH old and new.
acc: Vector<Int> = []
for i in range(n) { acc = acc.concat([i]) }

// boot/bench/slice_droplast.tw — the LIFO case. Expect ~4× per doubling (quadratic).
acc: Vector<Int> = range(n).to_vector()
for ... { _ = acc[acc.len() - 1]; acc = acc.slice(0, acc.len() - 1) }   // drop-last

// boot/bench/slice_dropfirst.tw — acc = acc.slice(1, acc.len()) in a loop (left-drop).
// boot/bench/droplast_baseline.tw — same drop-last workload via the proposed
//   `drop_last` op, to show the O(log n) target it would hit.

// boot/bench/concat_balanced.tw — pairwise/tree concat of many small vectors.
// boot/bench/get_regular.tw — N random get() on an append-built (regular) vector.
// boot/bench/get_relaxed.tw — N random get() on a concat-built vector (post-RRB:
//   exercises relaxed-node navigation; pre-RRB: baseline get cost).
// boot/bench/set_regular.tw, boot/bench/set_relaxed.tw — same for set().
```

Record a table of `N → ms` per benchmark and confirm the prepend **and drop-last**
curves are quadratic (each doubling of N ≈ 4× time) while append is linear (≈ 2×).
The `droplast_baseline` benchmark also quantifies how much `drop_last`
would win versus slice-based drop-last, informing the Alternatives decision.

### Decision criteria (when RRB is worth it)

Re-run the same benchmarks after a prototype Phase 3 and require **all** of:

- **Prepend / right-accumulator and trim loops**: clearly sub-quadratic — each
  doubling of N trends toward ≈2× (linear-ish), and an order-of-magnitude
  wall-clock win at N ≥ 8k. This is the headline justification and must be
  unmistakable. (If the only motivating workload is LIFO drop-last, prefer
  `drop_last` — see Alternatives — and require RRB to be justified by
  arbitrary and left-drop slice instead.)
- **Append-at-end (control)**: no regression beyond noise (RRB must not slow the
  common path).
- **`get`/`set` on regular (append-built) vectors**: no regression — these stay
  on the radix fast path with no size table.
- **`get`/`set` on relaxed (concat-built) vectors**: regression bounded and
  documented (target ≤ ~1.5–2× the regular cost; size-table navigation is the
  known tax). If relaxed get/set is dramatically slower, reconsider.

If the prepend win is not obvious, or regular get/set regresses, **do not merge**
— the complexity is not paid for. Keep the benchmark suite in `boot/bench/` as a
permanent regression guard regardless of outcome.

#### Gate B baselines established (2026-05-30)

The suite is built and run — see `boot/bench/` (`README.md` has the full table,
per-doubling ratios, and the post-prototype acceptance criteria). On the current
non-RRB runtime the curves are exactly as predicted:

- **Quadratic (RRB's targets):** `concat_prepend`, `slice_dropfirst`,
  `slice_droplast` each grow ≈4× per doubling of N (e.g. prepend 3.6 ms → 736 ms
  across 1k→16k).
- **Linear controls (must stay linear):** `concat_append`, `concat_balanced`,
  `droplast_baseline` grow ≈2× per doubling. The shipped `drop_last` op does the
  drop-last workload ~400× faster than slice at 16k — confirming LIFO pop is not
  RRB's job.
- **Fast-path baselines:** `get`/`set` are linear in total ops; `*_relaxed`
  reads ≈ `*_regular` today (concat builds regular trees pre-RRB), giving the
  bar that relaxed-node get/set must stay within (~1.5–2×) after Phase 3/4.

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
checked. The `boot/bench/` suite from Gate B becomes the before/after evidence and a
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

**Phase 4 — RRB slice.** Replace `slice` with the O(log n) left/right boundary
trim that shares interior structure and marks boundary nodes relaxed, reusing the
Phase 1–2 relaxed machinery. Same boot-first-then-mirror discipline. This is
co-primary with concat (dequeue/trim loops), not optional.

**Phase 5 — Optimizer reconciliation.** Confirm the static-uniqueness pass still
behaves: append-at-end still rewrites to builder; prepend/right-operand concat
and trim/dequeue slice fall through to the new O(log n) ops. Add guard fixtures
for the prepend and dequeue loops demonstrating O(n log n) scaling.

**Phase 6 (optional) — bulk `builder_extend`** (splice leaves instead of
replaying elements).

**Phase 7 — classical RRB slack comparison.** Treat the current boot concat
packing (minimum-parent, even redistribution) as the baseline and prototype a
stricter classical/windowed rebalance. The stricter variant should gather a
bounded window of adjacent seam siblings where needed, redistribute so interior
nodes are near-full (`m - e` … `m`, with only bounded edge exceptions), and then
measure against the baseline before replacing it. Compare at least:

- prepend/right-operand concat scaling and wall-clock constants;
- `concat_balanced` and append-at-end controls;
- `get`/`set` on regular and relaxed vectors (especially whether more nodes
  canonicalize back to regular and avoid size-table navigation);
- memory/shape proxies where available: number of relaxed nodes, average child
  occupancy, and tree height on deterministic adversarial concat patterns.

Do not switch to naive full-left packing (`32,1` for 33 children): that is the
height-creep shape the current even-redistribution baseline intentionally avoids.

**Phase 7 note (2026-05-31).** A narrow `pack_children` prototype was tried and
reverted. It kept interior parents near the classical slack floor when a local
minimum-parent pack had too few children to make every parent near-full, splitting
the unavoidable slack across the two edges instead of using naive full-left
packing. Controlled A/B runs showed it was effectively a wash on the existing
benchmarks: curves and results matched the even-redistribution baseline within
noise. This is not evidence against classical/windowed RRB slack in general; the
branch barely fires on the current workloads, and the current wall-clock suite is
mostly insensitive to the shape questions Phase 7 is meant to answer. The next
Phase 7 step should be shape instrumentation plus deterministic adversarial
concat fixtures that exercise high fan-out seam repacking before attempting
another replacement.

## Testing & characterization

- **Scaling guards**: fixtures that (a) prepend in a loop (`acc = [x].concat(acc)`)
  and (b) dequeue in a loop (`acc = acc.slice(1, acc.len())`) for growing N and
  assert near-linear (not quadratic) growth — the two regressions this plan exists
  to prevent.
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
  redistribute seam overflow from the start; do not ship a naive "single relaxed
  root" or one-child overflow-chain concat as the end state. Phase 7 compares the
  current even-redistribution baseline with stricter classical/windowed RRB
  slack to see whether the extra implementation complexity buys better constants.
- **GC layout change** ripples to every `VecInternal` constructor and to
  `from_array`/`to_array`/builder freeze. Mitigation: Phase 1 isolates the
  additive field change with the suite green before any algorithmic change.
- **Two runtimes drift.** Mitigation: differential tests are the gate; mirror
  only after the lead runtime is validated.

## Alternatives & complementary work

### The cheaper non-RRB pieces — `drop_last` / `View` (both shipped)

The boot-compiler audit ([slice-performance.md](../slice-performance.md)) showed the
real in-loop slice usage is **LIFO drop-last** (scope stacks, the Tarjan
worklist, fmt stacks) and a few **read-only head/tail recursions** (match arms) —
*not* FIFO. (A queue/deque was considered for this and dropped.) Those were served
by cheaper, non-RRB pieces, both now **shipped**:

- **`Vector.drop_last`** ([stack.md](../stack.md)) — O(log n) drop-last runtime op
  (no RRB needed; right-drop, unlike left-drop, needs no relaxed nodes). The boot
  compiler's LIFO pop sites are migrated onto it. (The `@std.stack` wrapper that
  rode along was later removed — unused; the op is the lasting artifact.)
- **`View<C>`** ([view.md](../view.md)) — a generic read-only window for the
  drop-first/traversal sites: O(1) `drop_first`, no copy, no hand-threaded index.

### Coexistence: this is **not** an either/or with RRB

These operate at different layers and **coexist permanently**:

| | `drop_last` / `View` | RRB `Vector<T>` |
|---|---|---|
| What it is | one small runtime op + a read-only window type | upgrade of the existing Vector internals |
| Fast ops | LIFO pop, read-only drop-first — O(1)/O(log n) | `concat`/`slice` at **arbitrary** positions + cheap `drop_first`/`prepend`, **O(log n)** |
| Opt-in | yes — reach for `drop_last`/`View` | no — all Vector code benefits transparently |
| Complexity | low (shipped) | high (relaxed nodes + rebalance) |
| Doesn't give you | arbitrary-range slice / arbitrary concat / *mutating* drop-first | the ergonomic read-only window surface |

Neither subsumes the other. The cheap pieces shipped first; **RRB is now the
residual** — arbitrary `concat`, arbitrary-range (esp. left-drop) `slice`, and the
`drop_first`/`prepend` that make a `Vector` a real queue/deque. With the easy
cases already stripped out, RRB's job is precisely the "universal sequence"
capability that motivated unparking it.

## Relationship to other work

- **Static-uniqueness plan** — complementary and unchanged. That plan minimizes
  COW/allocation for linear append-accumulation; this plan fixes the *asymptotic*
  cost of persistent `concat`/`slice`. They do not conflict.
- **Bulk `builder_extend`** and a **written cost contract** were the other
  candidate directions; the cost contract is folded in here (the post-change
  table), and bulk extend is the optional Phase 6 once relaxed nodes exist.

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
