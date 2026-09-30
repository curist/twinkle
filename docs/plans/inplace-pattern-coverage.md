# In-place pattern coverage for AWFY parity

**Status:** Umbrella / catalog. No implementation yet — this doc names the
remaining in-place-optimization patterns surfaced by the AWFY benchmarks, maps
each to an existing track or a genuinely new capability, and defers detailed
design. Each pattern below is expected to spin into its own spec later.

## Context

After landing native `Int.to_float` / `Float.to_int` intrinsics (2026-09-30),
Twinkle sits at roughly **1.06× the geometric mean of LuaJIT** on the AWFY
suite. It *wins* on allocation- and collection-bound benchmarks (json, list,
storage, and — post-`to_float` — mandelbrot) and *loses* on a cluster of hot
numeric/recursive loops.

The important finding: those remaining losses are **not one "MutVec pattern"
away** — but they are also *not* three unrelated projects. They sort cleanly
against the **single existing ownership analysis**: two are precision gaps on its
one linearity hinge, one extends that hinge, and one is an orthogonal
representation choice (see "One analysis, three positions" below). `mandelbrot`
flipped from 2.8× behind to 2.7× ahead off a *single* prelude function because
its gap was an accidental string round-trip; there is no equivalent free lunch
left in the losing benches. This catalog exists so we can pick and sequence the
real work deliberately.

### Foundation this builds on

- [`sound-uniqueness/`](sound-uniqueness/) — the single home for owned-collection
  in-place analysis and mutable lowering; printable ownership facts before codegen.
- [`mutvec-checklist.md`](mutvec-checklist.md) — living MutVec tracker (Int/Bool/
  Float/Byte shipped; boxed + `set_in_place` write-routing + param-sourced thaw
  outstanding).
- [`mutvec-later-slices.md`](mutvec-later-slices.md) — generalizing the seven
  `mutvec_*` ops across element families.

### How to read each entry

Benchmark exemplar → what it compiles to **today** (measured) → root cause →
what it would need → existing track or NEW → difficulty → benchmarks unlocked →
open questions.

### One analysis, three positions relative to the linearity hinge

These are **not three separate analyses.** `sound-uniqueness/` is already a single
unified dataflow analysis — a fact lattice + an exhaustive per-op transfer
function over the closed `AnfOp` enum + a CFG fixpoint + interprocedural
summaries. Its whole decision reduces to one **linearity hinge** (from
[`analysis/worked-examples.md`](sound-uniqueness/analysis/worked-examples.md)):

> `AInit` is a **move** iff the source is dead afterward, else an **alias**
> (→ stay persistent).

Each pattern below is a *position relative to that hinge*, which is what actually
sets its difficulty:

- **Precision gaps (A, B-spine).** Semantically already **move/dead** cases the
  hinge *should* accept, but the analysis can't yet see the ownership through a
  dynamic field projection (A) or route it to the boxed in-place op (B-spine).
  These extend the **precision** of the existing hinge — no new lattice state, no
  new runtime.
- **Expressiveness gap (C).** The hinge is *binary* (dead→move / live→persistent).
  Backtracking is **live-but-restorable**, a third outcome the lattice cannot
  express today. C extends the hinge from binary to **ternary** (move / transient /
  alias) — a genuine core change with new runtime (undo-log) and lowering.
- **Orthogonal axis (B-record-alloc).** nbody's dominant cost is *record
  allocation*, which is not an ownership question at all — it's a **representation**
  choice (in-place struct fields / struct-of-arrays). The hinge does nothing for
  it either way.

So the leverage isn't a mythical "one algorithm for all patterns" (the engine is
already one algorithm; exact aliasing/liveness is undecidable, so a sound,
annotation-free, never-reject analysis *always* has a precision frontier). The
leverage is: raise precision on the **shared hinge** (A, B-spine), extend the hinge
to ternary once for all backtracking (C), and treat representation (B-record-alloc)
as its own track.

---

## Pattern A — Vector nested in a linearly-threaded record, selected by a dynamic index

**Exemplar:** `towers` (2.87× slower). Three `Vector<Int>` pegs live inside a
`Pegs` record, threaded linearly through the recursion, and selected/updated by
a runtime `case which`:

```tw
fn move_top(p: Pegs, from: Int, to: Int) Pegs {
  src := p.peg(from)                       // case-based field read
  disk := src[src.len() - 1]
  p2 := p.set_peg(from, src.drop_last())   // write same field back
  dst := p2.peg(to)
  p3 := p2.set_peg(to, dst.append(disk))
  ...
}
```

**Today:** `move_top` lowers to persistent `rt_arr__drop_last` / `rt_arr__push`
/ `rt_arr__get` — no in-place op fires. Each `drop_last`/`append` copies.

**Root cause:** `src` aliases `p.a`/`p.b`/`p.c` while `p` is still live (used by
the following `set_peg`). Ownership analysis cannot prove `src` uniquely owns the
underlying vector across a *dynamic* (`case`-selected) projection, so it can't
route `drop_last`/`append` to the flat `mutvec_*` path. The vectors are `Int`
(unboxed i64), so a flat MutVec here would be an allocation-free O(1) win with no
per-element boxing — this is the cleanest of the remaining cases.

**What it would need:** alias-aware ownership through record-field projection with
a dynamic selector — recognize the "read field *k*, transform, write field *k*
back, old field value dead" idiom even when *k* is a `case`/runtime index.

**Classification:** *Precision gap on the linearity hinge.* This is already a
**move/dead** case (the old field value is consumed by `drop_last`); the analysis
just can't prove it through the dynamic projection. No new lattice state, no new
runtime — the destructive lowering already exists (MutVec).

**Track:** extends `sound-uniqueness/` (aggregate/field ownership). Related to
`archive/loop-threaded-field-ownership.md` and
`archive/sound-uniqueness-recursive-summary-ownership.md`, but adds the dynamic
selector + read-alias-then-overwrite shape.

**Difficulty:** Medium. Self-contained analysis-precision extension; no new runtime rep.

**Unlocks:** towers; generalizes to any record-of-vectors threaded linearly
(stacks/queues/pegs held in a record).

**Open questions:** Does the analysis need to case-split on the selector, or can
it prove field-*k*-in / field-*k*-out uniqueness abstractly? Interaction with the
existing recursive-summary ownership facts.

---

## Pattern B — In-place record-field mutation / struct unboxing (`Vector<record>` update loops)

**Exemplar:** `nbody` (3.13× slower; biggest absolute gap). `bs` is a
linearly-threaded `Vector<Body>` rebuilt element-by-element:

```tw
bs = .set_at(j, Body.{ x: bj.x, y: bj.y, z: bj.z,
                       vx: bj.vx + dx * bi.mass * mag, ..., mass: bj.mass })
```

**Today:** each update lowers to a generic `set_at<Body>` call — **neither** flat
MutVec **nor** boxed-MutVec fires. So every update pays *two* costs: (1) a
persistent trie copy of the array spine, and (2) a fresh `Body` struct
allocation. `bounce` has the same `Vector<Ball>` shape; its `bounce_mut` variant
sidesteps both by returning a mutable-field result struct and already *beats*
LuaJIT (0.59×), which is the existence proof that the win is real.

**Root cause:** two independent gaps.
1. **Spine:** boxed MutVec (`array<anyref>`) is landed but does **not** auto-fire
   on this owned `Vector<Body>` — it stays on generic persistent `set_at`. Why
   is an open question (analysis routing vs ABI precondition).
2. **Element:** even with an in-place spine, `Body.{...}` allocates a new record
   per update. The dominant cost is the *record* allocation, not the array copy.
   MutVec is vector-only and does nothing for this.

**What it would need:** one or both of —
- **Boxed-MutVec routing** for owned `Vector<GC-ref>` spines (close gap 1), and
- **In-place struct-field mutation** for a uniquely-owned record (a *sibling of
  MutVec for structs*), or **struct-of-arrays unboxing** of `Vector<Body>` into
  parallel `PVecF64` columns (close gap 2). `nbody_mut` validates the SoA/flat
  approach manually.

**Classification:** two independent things wearing one benchmark. Gap 1 (spine) is
a *precision/routing gap on the linearity hinge* — same class as Pattern A (a
move/dead case not reaching the in-place op). Gap 2 (record alloc) is **not an
ownership question at all**; it is an *orthogonal representation axis*. Don't
conflate them: closing gap 1 alone leaves most of nbody on the table.

**Track:** gap 1 → `mutvec-later-slices.md` (boxed family) + `mutvec-checklist.md`
Phase 7. Gap 2 → **NEW representation axis** (no current plan covers in-place
struct fields / SoA unboxing).

**Difficulty:** Medium (gap 1 routing) to Hard (gap 2 struct in-place / SoA needs
a representation decision).

**Unlocks:** nbody, bounce, storage, and any `Vector<record>` update loop.

**Open questions:** Why doesn't boxed MutVec fire on `bs` today? Is struct
in-place mutation or SoA-column unboxing the better bet — and can the compiler
choose automatically, or does it need a representation-boundary policy (cf.
`performance/representation-boundary-policy.md`)?

---

## Pattern C — Transient / backtracking mutation (mutate-and-restore)

**Exemplar:** `queens` (3.23× slower). Backtracking search sets one element,
recurses, then the loop reads the **original** vector again:

```tw
for !solved and r < 8 {
  if get_row_column(free_rows, free_maxs, free_mins, r, c) {
    fr := free_rows.set_at(r, false)     // modified copy
    fx := free_maxs.set_at(c + r, false)
    fm := free_mins.set_at(c - r + 7, false)
    if place(fr, fx, fm, c + 1) { solved = true }   // recurse
  }
  r = r + 1                              // next iter reads ORIGINAL free_rows
}
```

**Today:** persistent `set_at` copies per candidate (small vectors: 8/15/15).

**Root cause:** genuinely persistent — the original is read-after-write across
loop iterations, so it is *not* owned-linear. No amount of ownership analysis
makes this a MutVec case; the semantics require the old value to survive.

**What it would need:** transient / undo-log mutation scoped to the recursion —
mutate in place before the recursive call, restore on backtrack. This is a new
runtime + analysis capability (recognize the mutate→recurse→restore idiom, or
expose a scoped-transient API the pattern lowers to).

**Classification:** *Expressiveness extension of the linearity hinge.* The only
pattern here that changes the analysis core: it adds a third outcome —
**live-but-restorable → transient** — generalizing the current binary
(move / alias) hinge to ternary (move / transient / alias), plus a new runtime
(undo-log) and lowering. Everything else in this doc is precision or
representation; this is the one that touches the lattice itself.

**Track:** **NEW capability.** No current plan. Distinct from all owned-linear
work.

**Difficulty:** Hard, but high generality — every backtracking / DFS / constraint
search has this shape, well beyond AWFY.

**Open questions:** Automatic idiom recognition vs an explicit scoped-transient
construct? Soundness of restore across early exit / traps / `try`. Overlap with
region-based allocation ideas.

---

## Out of scope for this umbrella: recursion & arithmetic overhead

**Exemplar:** `permute` (1.46× slower). Its `permute_mut` variant rewrites the
state onto `@std.buffer` (fully mutable linear memory) and **still does not
improve** — its own note says non-inlined `inc`/`dec` calls and recursive-call
overhead dominate, not the vector ops. So `permute` is **not** an in-place
target; it belongs to the inlining / devirtualization axis
(cf. the closure-devirt / copy-carrier inliner work). Recorded here so it isn't
mistaken for a MutVec gap.

`sieve` (1.62×) is nearly closed by `sieve_direct` (1.07×, direct-array form);
its residual is small and read-path/representation-bound, not an in-place gap.

---

## Common threads & suggested sequencing

Sorted by *what they touch in the one analysis*, not by benchmark:

- **Precision on the shared hinge (A, B-spine).** Both are already move/dead cases
  the linearity hinge should accept but can't yet — A through a dynamic field
  projection, B-spine through boxed-spine routing. They push the **precision** of
  the *same* rule; worth checking whether one lattice/summary extension serves
  both.
- **Expressiveness of the hinge (C).** The one core change: binary → ternary
  (move / transient / alias). Generalizes furthest beyond benchmarks (all
  backtracking / DFS / search), and is the hardest — new lattice state, undo-log
  runtime, and lowering.
- **Representation, off the ownership axis entirely (B-record-alloc).** In-place
  struct fields or SoA unboxing. No hinge work touches it; it is nbody's dominant
  cost and needs its own representation-boundary decision (cf.
  `performance/representation-boundary-policy.md`).

The lesson from reviewing the analysis core: the leverage is **the hinge**, not any
one benchmark. Raise its precision (A, B-spine), extend it to ternary once for all
of backtracking (C), and keep representation (B-record-alloc) as a separate track.

**Suggested first spin-off:** Pattern A. It is the most self-contained (a precision
extension of the existing hinge — no new lattice state, no new runtime), a clean
unboxed-Int win, and it exercises the record-field-ownership machinery that
B-spine also needs — so it de-risks the harder aggregate work while delivering
towers on its own.

**Next steps:** promote one pattern at a time into `docs/plans/` as its own spec
(brainstorm → design → implementation plan), starting from the sequencing above.
This umbrella stays the index of what's left and why.
