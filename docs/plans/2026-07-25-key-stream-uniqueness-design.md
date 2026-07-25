# Design: Key-Stream Uniqueness Certification for the Copy-Carrier Engine

**Status:** **Decided — a general proof checker.** This design certifies key-stream
uniqueness by *proving* conservative obligations (O0–O4, §3), not by matching a shape. §5 is
the detailed design (implementable), §6 its failure mode, §7 the acceptance gates that must
pass before any consumer. Supersedes the ad-hoc "dedupe-helper" recognizer shipped in commit
`d05096e6` (found unsound — see §4). No consumer reads `dedupe_helpers` until this checker is
built default-deny, passes the adversarial negatives (§7), and is independently re-reviewed.

**Audience:** Reviewable independently, and detailed enough to write an implementation plan
from §5. §1–§2 give context without prior knowledge of the borrow/effect work; §3–§4 state
the precise problem and why it is hard; §5 is the design; §6–§7 the failure mode and gates.

**Related:**
[2026-07-24-copy-carrier-engine-impl-plan.md](2026-07-24-copy-carrier-engine-impl-plan.md)
(the engine plan whose Task 3 this design replaces),
[2026-07-24-ownership-borrow-effect-checker-plan.md](2026-07-24-ownership-borrow-effect-checker-plan.md)
(parent framework plan).

---

## 0. Issues being addressed

This design exists because the current implementation is **unsound**. Concretely:

- **BUG-1 — the shipped dedupe recognizer over-certifies (latent miscompile).** Commit
  `d05096e6` added `function_is_sort_insert_primitive` / `classify_dedupe_helpers` in
  `boot/compiler/ownership.tw`, which certify functions that produce **duplicate** outputs
  from duplicate-free inputs. Two independent holes, both confirmed by running the classifier
  on real pipeline output (details + counterexamples in §4):
  - **Hole 1 (flag guard, breaks O3):** an id-append is accepted merely because its block
    *sets* the set-once flag; setting a flag does not prevent re-entry. `bad([1,2,3],9)` →
    `[1,9,2,9,3,9]` certifies as a dedupe helper.
  - **Hole 2 (element index, breaks O1/O2):** the element-append is matched by "operand
    indexes the vector param," ignoring *which* index and never proving a full single
    traversal. `bad2([1,2,3],9)` → `[1,1,1]` certifies.
- **Severity: latent, not yet active.** `classify_dedupe_helpers` populates
  `SummaryTable.dedupe_helpers`, but **no consumer reads it yet** (`table_is_dedupe_helper`
  has no callers). So no wrong code is emitted today. Task 4 of the engine plan is the first
  consumer — it treats a helper-built key stream as unique to license in-place dict writes.
  Wiring the current recognizer as-is turns both holes into **active miscompiles** (duplicate
  keys treated as unique → in-place write corrupts live aliases). Hence: the recognizer must
  be made sound (or replaced) **before** Task 4 consumes it.
- **Root cause (why this needs a design, not a patch):** soundly certifying the *real*
  `insert_sorted` requires proving dynamic invariants (full traversal; id appended at most
  once) that its **short-circuit `!inserted and id < x` guard defeats for every
  control-flow-only analysis** — dominators, control-dependence, and must-dataflow all
  conservatively reject it (§4.1). Closing the holes therefore forces a genuine
  approach choice (§5), not a one-line fix.

Also fixed in passing during the original implementation (minor, not design-relevant, noted
for completeness): a `cond`-keyword collision (`cond` is reserved for the `cond {}`
construct) and a reference to a non-existent block constructor. Both were corrected before
`d05096e6`.

---

## 1. Context

Twinkle is a statically-typed language targeting WebAssembly GC, self-hosted in `boot/`.
All values are immutable; `m[k] = v` is sugar for "build a new dict and rebind `m`". The
optimizer's ownership analysis (`boot/compiler/ownership.tw`) proves when a rebind can
instead reuse storage in place (`dict$set_in_place`) rather than copy-on-write
(`dict$set`), which is a large performance lever for the self-hosted compiler.

The **copy-carrier** borrow/effect engine (the umbrella task this design serves) targets a
specific shape: a dict aliased from a source parameter, then written by key while the
source is *read* through compatible loans. The motivating function:

```tw
fn merge_targeted_min(old, next, prev, locked) MergeOut {
  out := next                                   // carrier aliases the source param `next`
  keys := int_keys_union(old.keys(), next.keys())
  for k in keys {
    old_x := lat_get(old, k, 0)
    next_x := lat_get(next, k, 0)               // reads the source `next` ...
    if oscillates { out[k] = old_x + next_x }   // ... while writing the carrier `out`
  }
  MergeOut.{ map: out, locked: next_locked }
}
```

The engine wants `out[k] = …` to lower to `dict$set_in_place`. For that to be **sound**,
the writes must not corrupt the interleaved reads of `next` (which aliases `out`). The
argument that makes it safe is:

1. dict values are immutable objects — `set_in_place` reuses the order/spine, never mutates
   an existing value object; and
2. **the loop key stream `keys` has no duplicate keys** — so `out[k]` at iteration *t* and a
   later read `lat_get(next, k')` at iteration *t'* touch *different* keys.

This design is about **requirement 2 only**: certifying that `keys` is duplicate-free.

---

## 2. Why key-stream uniqueness is a hard requirement, not a formality

If `keys` can contain a duplicate key `k`, in-place lowering **miscompiles**. Concretely, in
`merge_targeted_min` with `keys = [.., k, .., k, ..]`:

- Iteration *t* (first `k`): `next_x := lat_get(next, k)` reads the original `next[k]`; the
  write `out[k] = old_x + next_x` mutates the shared backing in place.
- Iteration *t'* (second `k`): `next_x := lat_get(next, k)` now reads `out[k]` — the value
  written at *t* — because `out` aliases `next`. It computes a different `next_x`, so the
  second write diverges from the persistent (copy-on-write) semantics.

Persistent semantics would have read the unchanged `next[k]` both times. So a duplicate key
turns in-place lowering into a wrong-value bug. **Uniqueness of `keys` is load-bearing for
soundness.** Under-certifying (failing to prove uniqueness) only forgoes the optimization;
over-certifying (wrongly proving it) is a miscompile.

The keys come from `int_keys_union(old.keys(), next.keys())`. Each `Dict.keys(d)` is
duplicate-free by construction (dict keys are distinct), but their *concatenation* is not —
`old` and `next` can share keys. So the union helper must **dedupe**, and the engine must
**prove** it dedupes. In the real self-hosted compiler (which we may not rewrite to make a
benchmark pass), that helper is:

```tw
fn int_keys_union(a: Vector<Int>, b: Vector<Int>) Vector<Int> {
  out := a
  for k in b { out = insert_sorted(out, k) }   // compositional: each step preserves dedup
  out
}

fn insert_sorted(v: Vector<Int>, id: Int) Vector<Int> {
  out: Vector<Int> = []
  inserted := false
  for x in v {
    if x == id { return v }                     // (i) id already present ⇒ return v unchanged
    if !inserted and id < x { out = .append(id); inserted = true }  // (ii) insert id once
    out = .append(x)                            // (iii) copy each element of v once
  }
  if !inserted { out = .append(id) }            // (ii') or append id at the end, once
  out
}
```

So the certification problem reduces to: **prove `insert_sorted` returns a duplicate-free
vector whenever `v` is duplicate-free**, then lift it compositionally to `int_keys_union`.

---

## 3. Precise soundness obligations for `insert_sorted`

`insert_sorted(v, id)` is dedup-preserving because, on the path that actually appends
(i.e. `id ∉ v`, guaranteed by the early return at (i)), the output is exactly the multiset
`{ x : x ∈ v } ∪ { id }` with **each element appearing once**. The chosen checker (Option
§5) is a **proof checker**: it certifies a function only when it can produce concrete
evidence for *every* obligation below. Each obligation is a **conservative sufficient
condition** — evidence ⇒ the property holds; absence of evidence ⇒ reject (never assume).
Missing any one admits a non-dedupe function, so all are mandatory.

- **O0 (output lineage).** The accumulator starts empty (`out := []`); every value that
  reaches the accumulator arrives only through an *approved* append (O2/O3), never through an
  unrecognized call result, concat, or rebind; and every `return` yields either the original
  vector param `v` (the early-return path) or the accumulator lineage. This bounds *what*
  can be in the output to `{elements appended} ⊆ {v-elements} ∪ {id}`, so O1–O3 fully
  characterize the output.
- **O1 (id absent on the accumulator-return path).** *Not* "whenever an id-append runs, `id ∉
  v`" — a speculative id-append can precede a later `return v` (which discards the accumulator).
  The provable, sufficient property is: **reaching the accumulator return implies `id ∉ v`**,
  because the only in-loop exit is `return v` on `v[induction] == id`, so completing the full
  traversal to the post-loop return means the equality was false for every element. Evidence
  (§5.5): the equality compares `id` against the **induction element** `v[induction]` (not a
  fixed index); the loop is a **full traversal** of `v` (§5.3); and that `return v` is the
  *only* in-loop return / extra loop exit.
- **O2 (each original element at most once).** Evidence (§5.6): **exactly one** static
  `append(v[induction])` site (operand is the verified induction element, not "some index"),
  **and** it executes at most once per induction step — *not inside a nested loop* (no extra
  back-edge / inner SCC around it). Both a second static append and a nested-loop append are
  rejected.
- **O3 (id at most once).** Evidence (§5.7): `inserted` is a set-once flag (init `false`,
  assigned only `true`, never reset); and each id-append is **instruction-sensitively** proved
  to run with the flag false at that instruction — a block-entry must-false dataflow (the
  short-circuit reconvergence that would defeat it, §4.1, is removed beforehand by the §5.0
  CFG-view threading) *plus* an intra-block walk that kills flag-false at `inserted = true`, so
  two id-appends in one `!inserted` block are rejected.
- **O4 (combinator soundness).** A compositional combinator (`out := a; loop { out =
  dh(out, k) }`) is certified only when: no parameter rebinding can invalidate the
  uniqueness assumption; the accumulator seed and every update are tracked in program order;
  every accumulator update is a call to an **already-certified** dedupe helper on the
  accumulator (default-deny on any other write); and the returned local is accumulator
  lineage with ≥1 such update. (The shipped `function_is_dedupe_combinator` from `d05096e6` is
  **not** sufficient — it only checks the accumulator at arg 0 and inherits the primitive's
  unsoundness. It must be **replaced** by the stricter certifier in §5.8, which requires the
  accumulator at the callee certificate's `vector_param` position. Do not reuse the shipped one
  as-is.)

---

## 4. Why naive recognition is unsound — two confirmed holes

The shipped recognizer (commit `d05096e6`) certified via local structural matching and was
found by review to over-certify. Both holes were confirmed end-to-end (the classifier
returns `true` for each):

- **Hole 1 (breaks O3).** It accepted an id-append if its block *sets* the flag true, on the
  theory that "once set, the block can't re-run." False: setting a flag does not gate
  re-entry unless something *reads* `!flag` before entering. Counterexample (certified,
  wrong):

  ```tw
  fn bad(v, id) {
    out := []; flag := false
    for x in v { if x == id { return v }
      out = .append(x)      // O2's single element-append
      out = .append(id)     // id every iteration; block sets flag after → old check passed
      flag = true }
    out
  }
  ```
  `bad([1,2,3], 9)` → `[1,9,2,9,3,9]`.

- **Hole 2 (breaks O2/O1).** It counted an element-append if the operand was "some index of
  the vector param," ignoring *which* index, and O2 was a static site count. So a fixed
  index re-appends the same element every iteration:

  ```tw
  fn bad2(v, id) { out := []
    for x in v { if x == id { return v }
      out = .append(v[0]) }   // fixed index; static count = 1 → old check passed
    out }
  ```
  `bad2([1,2,3], 9)` → `[1,1,1]`. (Also breaks O1: `x == id` no longer traverses `v`, since
  `x` here is still the loop element but the append ignores it — the deeper issue is that
  neither the guard element nor the append element is tied to a verified full traversal.)

### 4.1 Why the obvious sound fixes do **not** work

The natural fix for O3 is "prove the flag is false when each id-append executes." Every
CFG-only formulation of this fails on the *real* `insert_sorted`, because its guard is the
**short-circuit** `!inserted and id < x`. That lowers to two condbranches with a join
(verified from `--cfg`, see Appendix):

```
B8: condbranch !inserted → B9(then) | B10(else)
B9: id < x                     B10: (constant false)
B11: join(L26 = id<x from B9, false from B10); condbranch L26 → B12(then) | B13(else)
B12: append(out, id); inserted = true
```

The id-append `B12` is the *true* target of `B11`. Structurally, `B11`'s predecessors are
**both** `B9` (flag-false side) **and** `B10` (flag-true side) — so `B12` is reachable in the
CFG from the flag-true side. It is only the *value* `L26 = (…) or false` on the `B10` edge
that dynamically excludes that path (from `B10`, `L26 = false`, so `B11` must take the
`else` to `B13`, never `B12`).

Consequently, **every purely control-flow analysis conservatively rejects `insert_sorted`**:

- **Dominators / control-dependence:** `B12` is not dominated by (nor solely control-
  dependent on) the `!flag` branch `B8`, because the flag-true side `B10` reconverges at
  `B11` before `B12`.
- **Must-dataflow ("flag definitely false on entry"):** `B11`'s entry joins `B9`
  (flag-false) and `B10` (flag-true), so the must-analysis yields "maybe true" at `B11` and
  `B12` → reject.

The `B10 → B11 → B12` path is infeasible *because `B10` supplies the constant `false`* to the
join `L26` that `B11` then branches on. Note this is **not** what constant propagation / SCCP
would fix: `L26 = meet(id<x, false) = unknown` is not a constant, so SCCP leaves both `B11`
branches live. The fact is *per-edge* (this predecessor carries a constant into a branched
param), i.e. **jump-threading**, not value-constancy.

**Resolution (this design):** rather than build that correlation into the checker, a minimal
**jump-threading simplification of the ownership analysis's CFG view** (§5.0) rethreads the
`B10` edge straight to `B13` *before* the analysis runs. On the resulting view `B12` is
reachable only from the flag-false side, so O3 collapses to a **standard must-false dataflow**
(§5.7) — no checker-local value analysis. It is analysis-only (it does not touch the emitted
ANF/codegen); the point is to keep the miscompile-prone value reasoning out of the certifier.

---

## 5. Design: the A′ proof checker

The checker is a **proof engine, not a pattern recognizer**: it certifies a function only
when it can assemble concrete evidence for every obligation O0–O4 (§3), and rejects by
default otherwise. It keeps the "structural, not name-based" goal — it certifies any function
that *proves* the obligations, not one hard-coded shape. Its only risk is *implementation*
risk (a bug in the proof machinery over-certifies → miscompile); §7's gates contain that.

The one piece of value reasoning O3 would otherwise need is factored **out** of the checker
into a CFG-view simplification (§5.0, analysis-only), so the checker itself is pure structural
+ standard dataflow.

### 5.0 Prerequisite: minimal constant-branch threading — a **CFG-view** simplification

The hard part of O3 — proving the id-append runs only when `inserted` is false, despite the
short-circuit guard's reconverging join (§4.1) — is removed by a small, general simplification
run *before* the ownership/summary analysis, not by any checker-local value analysis.

**Where it lives (corrected + centralized).** The pattern below is expressed in **CFG** terms
(`CondBranch`, predecessor edges, block params — `boot/compiler/cfg.tw`). Those do **not**
exist in the ANF `opt/` pipeline, which rewrites `AnfExpr` *before* `cfg.build_view` runs. So
this is a **`CfgView → CfgView` simplification** — and it must be applied **inside
`classify_dedupe_helpers` itself**, threading the view it analyzes, so that *every* entry
point that classifies (full `summary.compute`, the scoped/production `compute_for_roots_cached`
path, and direct test calls) sees the **same** threaded view and cannot diverge. Do **not**
scatter the threading into one caller (e.g. only `ownership_verdicts.tw`); centralize it in the
classifier. **Test seam (two pieces, both required).** Exposing the transform alone is *not*
enough for gate 5, because the default `classify_dedupe_helpers` always threads internally —
there would be no way to observe classification *without* threading. So expose **both**: (a) `pub
fn thread_const_branches(view: CfgView) CfgView` — the transform, directly callable; and (b) an
**unthreaded classification mode** that runs the identical certifier over the *un*-threaded view
— either a `pub fn classify_dedupe_helpers_unthreaded(...)` or a `skip_threading` flag/options
record threaded into `classify_dedupe_helpers`. The default classification path threads; gate 5's
verdict-equivalence test selects the unthreaded path and diffs the two verdict sets. It is
**analysis-only**: the threaded view is local to classification, does not touch `artifacts.opt`,
and does **not** affect emitted code — removing the "wrong CFG for all codegen" blast radius.
(A general ANF-level jump-threading optimizer pass is a possible *separate* future change; out
of scope here.)

**Pattern (minimal, conservative).** For a block `B` such that:
- `B`'s terminator is `CondBranch(cond, T, [], F, [])` — a branch with **empty** edge args to
  both targets;
- `cond` resolves through trivial `AInit` aliases to a block **param** `p` of `B`; **and**
- `B` contains **no instructions other than those trivial `AInit` aliases** — no calls, no
  stores, no `flag = true`, no accumulator mutation (otherwise threading past `B` would drop a
  side effect the analysis must see);

then for every predecessor edge `Q → B` supplying a literal `ALitBool(c)` for `p`, rethread it
to `Q → T` (if `c` true) or `Q → F` (if `c` false). Sound because on the `Q` path `p ≡ c` so
`B`'s branch is determined, `B` does nothing observable, and the empty target args mean nothing
computed in `B` is needed on the threaded edge. **Anything not matching this exact shape is
left untouched** (non-empty target args, branch on a non-param, a `B` with real instructions) —
so a miss only forgoes the simplification, never miscompiles.

**Effect on `insert_sorted`.** `B11 = if.join(L26); condbranch L26 → B12() | B13()` has
predecessors `B9` (supplies `L26 = id<x`, non-constant → untouched) and `B10` (supplies
`L26 = false` → rethreaded to `B13`). After threading, the id-append block `B12` is reachable
only via `B9`, i.e. only from the `!inserted`-true side of `B8`. The reconvergence that
defeats every control-flow analysis (§4.1) is gone.

**Failure mode.** A bug here is a *wrong-CFG* bug **confined to the ownership analysis's view**
(it can cause a mis-certification, so it is still covered by §7's gates and the adversarial
battery), but because it is analysis-only it cannot corrupt codegen directly. It is a
standard, self-contained transform, tested independently (§7 gate 5) with a verdict-equivalence
check (analysis results with vs. without threading agree except at the intended sites).

### 5.1 Top-level structure

```tw
// A positive certificate names the concrete evidence for each obligation, so a reviewer and
// a test can inspect *why* a function certified, and a consumer can render the proof token.
type DedupeKind = { SortInsertPrimitive, Combinator }
type DedupeCertificate = .{
  kind: DedupeKind,
  vector_param: Int,     // vp — the traversed/returned vector param (primitive) or seed param (combinator)
  induction_local: Int,  // the verified 0/+1/<len(vp) counter (primitive; -1 for combinator)
  element_local: Int,    // vp[induction] — the guarded + appended element (primitive; -1 for combinator)
  flag_local: Int,       // the set-once `inserted` flag (primitive; -1 for combinator)
}

// func_id -> certificate. A func is certified iff it has an entry. Fixpoint so a combinator
// certifies after the primitive it calls is known (monotone: only adds; order-independent).
pub fn classify_dedupe_helpers(view, b, sem) Dict<Int, DedupeCertificate>
```

Per function, try `certify_sort_insert_primitive(f, ops)` first; if it yields no certificate,
try `certify_combinator(f, ops, known)` using the certificates found so far. Each certifier
returns `DedupeCertificate?` and is **default-deny**: it returns `.None` on the first
obligation it cannot evidence.

### 5.2 Shared infrastructure

Mostly already present in `ownership.tw`, but **ANF is non-SSA** (`AAssign(LocalId, Atom)`
re-defines a local), so a *function-wide* "local → op" map is unsound wherever a local can be
defined more than once. This forces two disciplines used throughout §5:

- **Reaching-definition, not last-write.** `build_def_map(blocks) Dict<Int, AnfOp>` (last-write)
  is safe **only** for locals with a single definition (SSA-like temps: `AArrayLit`, `AIndex`,
  `ABinOp`, `AInit`, `ACall` results). For any local that may be an `AAssign` target, resolve
  its value at a specific program point via a **block-local, instruction-ordered** walk (the
  reaching def at that point), not the function-wide map. §5.0 (threading, resolving `cond` to a
  param) and §5.7 (`!flag` freshness) require this instruction-ordered resolution.
- **Frozen evidence locals.** Any local whose *stable identity* a proof relies on must be pinned
  down, with two distinct disciplines: the induction counter, `element_local`, the len/index
  alias chain, and the returned local must have **exactly one definition** (never an `AAssign`
  target beyond the counter's sanctioned `+1` increment). The **flag** is *set-once*, not
  single-def: it must have **exactly one `false` definition and only sanctioned `true`
  assignments** — never reset to false and never otherwise redefined (§5.7). This is enforced
  explicitly in §5.3/§5.4/§5.7 rather than assumed.

Helpers:
- `build_def_map` / `build_block_map` — as above; `build_def_map` usable only for single-def
  locals (assert/verify single-def before trusting it).
- `DictVecOps` — method ids: `Vector.append`, **`Vector.len`** (add it), `Dict.get/set/remove/
  keys`, `dict$get_unsafe`. (`Vector.contains` is NOT used — prelude fn, no builtin id.)
- `alias_root(local, dmap)` — follows `AInit(ALocal x)` chains to a fixed point, **but only
  through single-def locals**; if any link is an `AAssign` target, `alias_root` must refuse
  (return the local itself / a "not a stable alias" marker), never silently root a rebindable
  local at `vp`.
- Edge helpers: `preds`/`succs` are `Vector<CfgEdge>`; **note the `preds` convention** — in a
  `preds` edge, `CfgEdge.target` holds the **predecessor (source) block id**, not the
  destination (see `ownership.tw` ~4262). `edge_arg_for_param(edge, target_block, param_index)`
  returns the atom for a target param and must hide this ambiguity.

### 5.3 Induction-variable identification (basis for O1 + O2)

`find_induction(f, vp, ops, dmap, bmap) Option<.{ counter: Int, element: Int }>`:

1. **Counter.** Find a loop-header block param `ctr` such that:
   - **exactly one** non-back-edge (pre-loop) predecessor, and it supplies `ctr = ALitInt(0)`
     (directly, or `ALocal(z)` where `z`'s def is `ALitInt(0)`);
   - **every** back-edge predecessor advances `ctr` by one — see "accepted advance" below; and
   - the loop-body entry is gated by a length comparison of `ctr` against `bound`, where
     `bound`'s def is `ACall(Vector.len, [w])` with `alias_root(w) == vp`. **Match the actual
     `for x in v` lowering** (`boot/compiler/lower_core/iteration.tw`): the emitted test is
     `ctr >= len` with **true → loop exit, false → body** (verified in the Appendix CFG:
     `condbranch L20 then B4(break) else B5(body)`). Accept either canonical polarity:
     `ABinOp(.Ge, ctr, bound, _)` true→exit/false→body, or `ABinOp(.Lt, ctr, bound, _)`
     true→body/false→exit. Reject any other comparison/target arrangement.
   (Identify the loop header by a block that is the target of one or more
   `LoopBackEdge`/back-edge terminators; the sole non-back-edge pred is the pre-loop entry.)

   **Monotonic advance (every back-edge advances the counter).** The header may have
   *multiple* back-edges (a `continue` in a **condition** loop forwards carried params
   unchanged and does **not** auto-increment — unlike the indexed-`for` `continue` rewrite).
   A back-edge that forwards `ctr` *unchanged* would re-run the body at the same index and
   re-append `v[ctr]`, with one static append site and no distinct inner SCC. So require: the
   header has exactly one init pred (ctr=0) and **every** back-edge advances `ctr` by one;
   reject any extra back-edge whose `ctr` arg is unchanged or is not a `+1` of the current
   `ctr`.

   **Accepted advance (stated in real ANF terms).** Note `AAssign`/`AInit` take an **`Atom`**,
   and `ABinOp` is an **`AnfOp`** bound to a local by a `Let` — the increment is never an atom
   directly. The real shape (verified Appendix CFG) is:
   ```text
   L34: ABinOp(.Add, L6, 1)      // op bound to local L34
   AAssign(L6, ALocal(L34))      // ctr := L34
   loop-back-edge B1(…, ALocal(L6), …)   // edge arg is ALocal(L6)
   ```
   So a back-edge `P →(back) header` advances `ctr` iff, following its edge arg for the `ctr`
   param **and any in-block `AAssign(ctr, ·)` / `AInit(ctr, ·)` source atoms back through
   `dmap`**, it reaches a local `t` whose **defining op** is `ABinOp(.Add, ctr, ALitInt(1),
   _)`, with the `ctr := t` assignment ordered before the back-edge and no intervening
   reassignment of `ctr`. Requiring a literal `add` *atom* on the edge (there is none) would
   wrongly reject the real `insert_sorted`. (Combined with §5.5's "equality false-edge
   dominates the increment," this makes each index visited exactly once.)
2. **Element.** Find `element` such that its def (through `AInit` aliases) is `AIndex(base,
   idxAtom, _, _)` with `alias_root(base) == vp` and `atom_local_id(idxAtom) == ctr`.
3. Return `{ counter: ctr, element }`, else `.None`.

This is the single fact that makes O1 (the accumulator-return path implies `id ∉ v`) and O2
(each element at most once) provable. A fixed index (`v[0]`) fails step 2; a non-`len(vp)`
bound or wrong branch polarity fails step 1.

### 5.4 O0 — output lineage (primitive)

For the primitive, the accumulator `acc` is a **fresh empty vector**:

- **Seed (real ANF shape).** `AInit` takes an `Atom`; the empty literal is a separate op. So
  the seed is `acc = AInit(ALocal(arr))` where `arr`'s (single) reaching def is
  `AArrayLit([])`. Build the accumulator lineage: `acc` plus every loop-carried rebind
  (`AAssign(accLineageLocal, src)` where `src` is lineage, and loop-header params carrying a
  lineage arg).
- **Frozen evidence locals (non-SSA guard).** `element_local` and every local on the
  `vp`-alias chain used for len/index/return evidence must have **exactly one definition** — no
  `AAssign` targets them anywhere in the function (else a later rebind would stale the proof;
  `alias_root` already refuses rebindable links, §5.2). The returned local must likewise be
  either `vp` (single-def, unrebound) or a lineage local.
- **Only approved appends reach the accumulator.** Every write into the lineage must be an
  *approved* append: `acc = Vector.append(acc, w)` with `w ∈ { element_local (O2), id_param
  (O3) }`. Any other producer of a lineage value (an unrecognized `ACall`, a `concat`, an
  `AAssign` from a non-lineage/non-approved source) ⇒ reject.
- **No rebinding of params or proof locals.** ANF is non-SSA — `AAssign` *redefines* its
  target local — so the certifier must forbid any rebinding that would make a proof rely on a
  stale value:
  - **no `AAssign` to any function param** (not just vector params): rebinding `id_param`
    makes `append(id_param)` no longer mean the certified `id`, and rebinding `vp` makes
    `return v` no longer the original input. (The shipped recognizer at
    `boot/compiler/ownership.tw` already forbids all-param `AAssign`; the design must too.)
  - **no `AAssign` to `element_local`**: its value must stay `vp[induction]` for the O1
    equality and the O2 append to mean what they claim.
  - the only permitted `AAssign` to a proof local are the induction counter's verified
    `ctr = ctr + 1` increment (§5.3) and the flag's set-once `flag = true` (§5.7). Any other
    `AAssign` to a proof local ⇒ reject.
- **Returns.** Every `Return(Some(a))` returns either `vp` (the O1 early return, now
  guaranteed to be the *original* input by the rule above) or an accumulator-lineage local.
  `Return(None)` or any other local ⇒ reject.

O0 bounds the output to `{appended elements} ⊆ {v-elements} ∪ {id}`, so O1–O3 fully
characterize duplicates.

### 5.5 O1 — id absent **on the accumulator-return path**

The property is *not* "whenever an id-append executes, `id ∉ v`" — that is false, because a
speculative id-append (`if !inserted and id < x`) can run *before* a later element equal to
`id` triggers `return v`. It is sound anyway because that speculative `out` is **discarded**
on the early-return path (which returns `v`, not `out`). So the correct, provable property is
**return-path-specific**:

> **If control reaches the accumulator return (returns an `out`-lineage local), then the
> equality false-edge was taken for every induction element — hence `id ∉ v`.**

Evidence:

- An early-return block `Return(Some(vp))` whose guard is `ABinOp(.Eq, element_local, id, _)`
  (order-insensitive), reached only on the equality-**true** edge, where `element_local` is
  the verified induction element (§5.3) and `id` is a scalar param `id_param`.
- **Full traversal** from §5.3: the counter ranges `0..len(vp)`, so reaching loop exit means
  the equality was evaluated for every element.
- This equality early-return is the **only** `Return` inside the loop body, and it is the
  **only** exit from the loop other than the normal `ctr`-bound exit.
- **The equality guard's false-edge dominates the induction increment / back-edge.** Every
  path from loop-body entry to the `ctr = ctr + 1` increment (and thus the back-edge) must pass
  through the equality's **false** edge. Without this, a `continue` *before* the equality test
  would advance to the next element having skipped its `x == id` check, so an `id` sitting at a
  skipped index would not trigger the early return (Twinkle lowers indexed-loop `continue` to
  increment-then-continue, `iteration.tw` / `control_flow.tw`). With domination, reaching the
  increment on any iteration means that element's equality was evaluated and false.

So the only way to reach the post-loop accumulator return is to have taken the equality
**false** edge on every element ⇒ `id ∉ v`. (Any other in-loop return, a second in-loop exit,
or a `continue`/branch that bypasses the equality guard breaks this and is rejected.)

Since the output that becomes the key stream is *either* `v` (early return; unique by the
input assumption) *or* the accumulator (this path; `id ∉ v`), duplicates are impossible on
both.

### 5.6 O2 — each original element at most once

A static site count is **not enough** — one `append(acc, element_local)` site can execute
multiple times per induction step if it sits inside a *nested* loop (`for x in v { for _ in
range(2) { out = .append(x) } }` appends each element twice). Evidence must bound *dynamic*
multiplicity:

- **exactly one** static `Vector.append(acc, element_local)` site (operand is the verified
  induction element), **and**
- that site executes **at most once per induction increment**: its block lies directly in the
  induction loop's body region and cannot be re-entered within one induction step. Concretely,
  **reject if any non-induction back-edge (a natural loop other than the induction loop itself)
  can re-enter the element-append site before the induction increment** — i.e. on the path from
  the loop-body entry to the `ctr + 1` increment, the append block must not be reachable from
  itself except by going around the induction back-edge. (Detect via `LoopBackEdge` terminators:
  the only back-edge dominating a path back to the append must be the induction loop's own.)

Skipping an element cannot create a duplicate, so conditional *omission* is fine; only a
*second* append of `element_local` (static or via an inner loop), or an append of
`vp[otherIndex]`, breaks O2 — the first two are now rejected, and O0 forbids the third.
(`bad_insert`: two static sites; the nested-loop variant: inner cycle; `bad2`: fixed index —
all rejected.)

### 5.7 O3 — id at most once (standard must-false flag dataflow)

Because the §5.0 threading pass has already removed the short-circuit reconvergence, O3 needs
**no** per-edge constant correlation. But block-entry sensitivity alone is **not enough** —
two id-appends in one `!inserted`-entered block both see a flag-false *entry* yet the second
runs after the flag is set:

```tw
if !inserted { out = .append(id); inserted = true; out = .append(id) }  // must reject
```

So O3 must be **instruction-sensitive**. Evidence:

- **Set-once flag, initialized outside the loop.** `flag_local` (= `inserted`) has **exactly
  one** `flag = false` definition — whether via `AInit(ALitBool(false))` **or** `AAssign(flag,
  false)` — it is assigned the literal `true` elsewhere, and it is **never otherwise reset**.
  Crucially, that single `false` definition's block must **dominate the induction loop header
  and not be reachable from any induction back-edge**, so the `false` init is not re-executed
  each iteration. Otherwise a helper resets the flag inside the loop and appends `id` every
  iteration: `for x in v { inserted := false; if !inserted { out=.append(id); inserted=true };
  out=.append(x) }` — the in-loop `AInit(false)` re-arms it. (`find_set_once_flag` must count
  **both** `AInit(false)` and `AAssign(flag,false)` as `false` definitions, require exactly one,
  and check it dominates the loop.)
- **Block-entry flag-false dataflow.** Compute `flag_false_entry: Dict<Int, Bool>` as a forward
  must-fixpoint over blocks, with a proper **entry→exit transfer** (absence of a `flag = true`
  assignment does *not* by itself make the flag false — the block may have been *entered* with
  flag maybe-true):
  1. seed the entry block after the `flag = false` init with flag false;
  2. `flag_false_exit[P]` = `flag_false_entry[P]` **and** `P` contains no `flag = true`
     assignment;
  3. edge `P → B` carries flag false = `flag_false_exit[P]`, with one guard refinement: apply it
     **only** when the `!flag` computation is **local to `P` and provably fresh**, defined as:
     `P`'s `CondBranch` condition atom's reaching def *in `P`* is `AUnOp(.Not, ALocal(flag), _)`,
     that `AUnOp` occurs **in `P`** (not an earlier block), and **no `flag = true` (nor accepted
     id-append) occurs in `P` between that `AUnOp` and the terminator** (a block-local,
     instruction-ordered check — §5.2). Then `P`'s **true** edge carries flag false **regardless
     of `flag_false_exit[P]`** (the branch itself establishes `flag == false`); its **false**
     edge maybe-true. Do **not** additionally require flag-false at `P`'s entry/terminator —
     that would wrongly reject the real primitive (`B8` entry is loop-carried maybe-true, yet
     `L24 = !L3` and the branch are both in `B8` with no intervening set → fresh). Withhold the
     refinement when the `!flag` op is in a **different block** than the branch (cross-block —
     freshness unproven) or when a `flag = true` intervenes in-block (stale, e.g.
     `c := !inserted; …; inserted = true; if c`); then the edge carries `flag_false_exit[P]`;
  4. `flag_false_entry[B]` = **AND** over incoming edges of "edge carries flag false".
- **Intra-block instruction walk (the O3 acceptance).** For each block, walk instructions in
  order with a running `flag_is_false := flag_false_entry[B]`; on an `AAssign(flag_local,
  true)` set `flag_is_false := false` for the rest of the block. Accept an id-append **iff
  `flag_is_false` holds at that instruction**. The second append in the example above is
  rejected because the intervening `inserted = true` killed `flag_is_false`. Additionally,
  each accepted id-append must be **either** (a) followed by `flag = true` in its own block
  before any back-edge (so a later iteration cannot re-append — the in-loop insert, B12),
  **or** (b) in the **terminal post-loop region** with no back-edge or further id-append
  reachable from it (the tail insert, B15). An id-append that is neither ⇒ reject.

On threaded `insert_sorted`, `B12`'s chain `B8 →(!inserted true) B9 → B11 → B12` gives
`flag_false_entry[B12] = true` (the `B8 → B9` edge carries flag *definitely* false via the
step-2 refinement, even though `B8`'s own entry is loop-carried maybe-true), the append is the
first instruction (flag false there), and the block sets the flag after → accept. `bad` (no
`!flag` test) and the two-append-in-one-block example → reject.

This is the same must-dataflow the compiler already runs for boolean/validity facts, plus a
straight-line intra-block walk; the only guard-specific rule is step 2's `!flag`-true-edge
refinement. No per-edge constant correlation (the §5.0 pass made that unnecessary).

### 5.8 O4 — combinator certifier

`certify_combinator(f, ops, known)` **replaces** `function_is_dedupe_combinator` from
`d05096e6` with a **stricter** certifier (the shipped one is not strict enough and inherits
the primitive's unsoundness) that returns a certificate. It certifies `out := a; loop { out =
dh(out, k) }`:

- **Seed.** Accumulator lineage seeded from `AInit(ALocal(p))` with `p` a vector param;
  build lineage over loop-carried rebinds.
- **Default-deny updates, at the certified param position.** Every write to a lineage local is
  `acc = dh(…)` where `dh ∈ known` (an already-certified dedupe helper) **and the accumulator
  is passed at `dh`'s certified vector-param position** — i.e. `cargs[known[dh].vector_param]`
  is the lineage local. A certified helper dedupe-preserves *its* `vector_param`, which need
  not be arg 0; passing the accumulator elsewhere proves nothing. Any other accumulator
  producer — a bare `Vector.append` on the accumulator, an unknown-callee result rebound to
  lineage, the accumulator passed at a non-certified position, or an `AAssign` from a
  non-lineage source ⇒ reject. (Sort-insert primitive certificates always have
  `vector_param = 0`, so `int_keys_union`'s `insert_sorted(out, k)` — `out` at arg 0 —
  matches.)
- **No param-invalidating rebind.** No `AAssign` targets a vector param.
- **Return + extension.** Every `Return` yields a lineage local, and the lineage was extended
  by ≥1 known-dedupe call.
- Certificate: `kind = Combinator`, `vector_param = p`, others `-1`.

Combinator soundness is **conditional on the primitive being sound**: it composes certified
helpers and never itself proves dedup. It inherits, and cannot create, unsoundness.

### 5.9 Composition and wiring

- `classify_dedupe_helpers(view, …)` **first threads the view (§5.0), then** runs the two
  certifiers to a bounded fixpoint (primitives certify first; combinators over them on the next
  pass), returning `Dict<Int, DedupeCertificate>`. Threading is internal so every caller is
  consistent.
- `SummaryTable.dedupe_helpers` changes from `Dict<Int, Bool>` to `Dict<Int,
  DedupeCertificate>` (or keep the bool map plus a parallel certificate map — implementer's
  choice; the certificate is what the acceptance test and the consumer inspect).
- **Populate on ALL summary paths, not just the full one.** `with_dedupe_helpers` must be
  called at the end of **both** `summary.compute` (full) **and** the scoped/production path
  `compute_for_roots_cached` (`boot/compiler/summary.tw`, reached from
  `ownership_verdicts.tw` — the path Task 4's production build actually uses). Missing the
  scoped path is safe (under-certification) but would mean the real build never sees
  certificates, so `merge_targeted__` would never flip. Best: centralize table construction so
  every consumer path returns a certified table. (Import is destructured — call unqualified.)

### 5.10 Scope, generality, and relation to the ownership analysis

**Two orthogonal axes.** The copy-carrier engine needs two independent facts, and this design
is *only* the second:

- **Ownership / aliasing axis** — "is the source dict uniquely owned, and do the reads borrow
  vs. publish it?" This is the subject of the sound-uniqueness worked examples
  (`docs/plans/sound-uniqueness/analysis/worked-examples.md`: Case W transport-wrappers, Cases
  B∩C per-call specialization, Case V threaded state, …) and is handled by the engine's
  **summary/loan machinery** (`base_role = Borrowed`, publication, projection-moves) in the
  engine plan's Tasks 4–6. `lat_get`/`int_keys_union` are recognized on this axis by their
  parameter summaries.
- **Value-content axis** — "does the loop key stream have duplicate elements?" That is this
  design. **No worked-examples case is on this axis** (verified), so this is not "narrow
  relative to those patterns" — it is a different question that *composes* with them: the
  ownership axis proves the write is legal; the content axis proves the keys are distinct.

**Current consumer surface (as of this design).** The sort-insert idiom is *pervasive* in the
boot compiler — `insert_sorted` is a shared primitive with an `Int` form defined (byte-identically)
in both `ownership.tw` and `summary.tw`, an `insert_sorted_int` clone in `cfg.tw`, and an
`insert_sorted_str` `String` form in `summary.tw`, driven by combinator call sites and union
helpers (`int_keys_union`, `union_sorted`, …) throughout the ownership/summary/cfg analysis. But **exactly one** of those streams currently
feeds a copy-carrier in-place dict write: `int_keys_union(old.keys(), next.keys())` in
`merge_targeted` (`ownership.tw`, the §1 motivating function). Every other `insert_sorted` call
builds an analysis-bookkeeping *vector*, not a dict key stream driving `dict$set_in_place`. So the
checker's generality is **future-proofing for the idiom family, not present breadth**: it lights
up one site today (engine-plan Task 4), and certifying the rest costs nothing extra but stays
unused until more copy-carrier sites consume their streams. This is a deliberate bet that the
idiom's ubiquity makes a structural checker better ROI than a one-off recognizer — worth naming
so the payoff is not overstated.

**Generality (honest bounds).** On its axis, the certifier is **general over an idiom family**,
not a universal "does this dedupe" oracle — which no purely-structural checker can be:

- **Covered:** direct `Dict.keys(d)` streams (unique by construction); the sort-insert /
  early-return primitive (`insert_sorted`) and *any* renaming/reordering that proves O0–O3; and
  compositional combinators (`int_keys_union` and friends) over already-certified helpers
  (O4). This is what idiomatic Twinkle and the real compiler use.
- **Element type — `Int` *and* `String` (verified).** The primitive is element-type-agnostic by
  construction: `Int` and `String` comparisons both lower to `ABinOp(.Eq/.Lt, …, opkind)` with
  the element type carried in the fourth `OpKind` field (`.Int` vs `.Str`) — *not* to Eq/Ord
  contract calls, because `is_primitive_cmp(.String)`/`uses_runtime_eq(.String)` treat `String`
  as primitive (`boot/compiler/lower_core/operators.tw`). O1's equality evidence (§5.5) and
  §5.3's bound comparison already **wildcard that `OpKind` field** (`ABinOp(.Eq, element_local,
  id, _)`), so the `String` variant `insert_sorted_str` certifies by the *same* evidence as the
  `Int` one (the `s < x` insert-guard is never structurally matched — only the `!inserted` flag
  is). No obligation is `Int`-specific. Gate 2 (§7) pins this with an `insert_sorted_str` positive
  fixture so a future OpKind-narrowing edit can't silently drop `String` coverage.
- **Not covered (⇒ safe under-certification, missed optimization, never miscompile):** dedupe
  algorithms outside the single-forward-pass / induction-indexed / set-once-flag /
  accumulator-append idiom — e.g. recursive dedupers, reverse/stepped traversals, hash-set
  approaches, or (for the §5.0 threading) phi-constant branches whose targets take args. These
  can be added later by extending an obligation or the threading shape; each extension is its
  own reviewed change with its own adversarial negatives.

The design deliberately trades breadth for a *safe* failure mode: anything it cannot prove
stays persistent. Adjacent content-uniqueness needs (should they arise elsewhere in the
compiler) would reuse the same O0–O4 + certificate scaffold rather than being precluded by it.

### 5.11 Soundness argument, and the obligation/enforcement split

This design has **two layers**, and keeping them distinct is what makes review tractable:

- **Obligations** O0–O4 (§3) — *semantic* sufficient conditions. The soundness guarantee is a
  proof that they imply a duplicate-free output.
- **Enforcement** (§5.0–§5.9) — the *structural/dataflow* checks that are supposed to imply the
  obligations on real ANF/CFG.

The seven review rounds almost entirely found cases where an **enforcement check failed to
imply its obligation** (e.g. `find_set_once_flag` not implying "set-once" because it ignored an
in-loop `AInit(false)`), **not** cases where the obligations themselves were insufficient. So
the argument below is the fixed target: every enforcement check must be shown to imply its
obligation, and the adversarial battery (§7) is the executable form of that check.

**Theorem (primitive).** If a function certifies as `SortInsertPrimitive`, then for any
duplicate-free input `v` and any `id`, `f(v, id)` returns a duplicate-free vector.

*Proof.* The result is one of two returns:
- **Early return `v`** (O1's equality-true edge): output is `v`, duplicate-free by hypothesis.
- **Accumulator return `out`**: by **O0**, every element of `out` came from an approved append,
  so `out`'s elements ⊆ {`v`-elements} ∪ {`id`}. By **O2** each `v`-element is appended at most
  once (one static induction-indexed append site, each index visited exactly once via §5.3's
  every-back-edge-advances rule and §5.5's false-edge domination), so — `v` being
  duplicate-free — the `v`-elements in `out` are distinct. By **O3**, `id` is appended at most
  once. By **O1** (accumulator-return path), reaching this return means the equality was false
  for every element, so `id ∉ v`. Hence `out` = (distinct subset of `v`) + (at most one `id`,
  not among them): no duplicates. ∎

**Theorem (combinator).** If a function certifies as `Combinator`, then for duplicate-free
vector inputs it returns a duplicate-free vector.

*Proof.* By **O4**, the accumulator is seeded from a vector param (duplicate-free by hypothesis)
and every update is `acc = dh(acc, …)` with `dh` an already-certified dedupe helper receiving
the accumulator at its certified `vector_param`. By the primitive/combinator theorems (well-
founded induction over the certification fixpoint order — combinators only depend on
already-certified callees), `dh` returns a duplicate-free vector whenever that argument is
duplicate-free. So by induction over the loop, `acc` stays duplicate-free; the returned lineage
local is `acc`. ∎

**Corollary.** A certified helper's result, used as a loop key stream, has no duplicate keys —
exactly the §2 requirement that licenses in-place copy-carrier writes.

**Consequence for review.** Because the obligations are proven sufficient, any future
over-certification is necessarily an **enforcement gap**: a structural check that does not
actually imply its obligation. Review — and each adversarial fixture — should therefore be read
as answering, per check, *"does this structural condition imply O\_k?"*. When the answer is no,
the fix is to tighten the enforcement, not the obligation. (The battery makes this executable:
a wrongly-certified counterexample is a check that fails to imply its obligation.)

---

## 6. Failure mode and why the acceptance gates

Because certification licenses in-place dict mutation, a bug in the **checker** that
over-certifies is a **silent miscompile** in the self-hosted compiler (duplicate keys treated
as unique → in-place write corrupts a live alias) — the highest-consequence failure class.
This is *implementation* risk, not design unsoundness: the obligations O0–O4 are conservative
sufficient conditions, so a correct implementation is sound. Moving the one value-reasoning
step out to the §5.0 CFG-view threading makes the checker itself pure structural + standard
dataflow, shrinking the miscompile-prone surface. The threading is **analysis-only** (it
rewrites the ownership analysis's `CfgView`, never `artifacts.opt`), so a bug there cannot
corrupt codegen directly — its worst case is a mis-certification, which is still covered by
the same gates. The gates in §7 contain both surfaces: they make over-certification hard to
introduce and easy to catch (default-deny + certificate + adversarial battery), add a
verdict-equivalence check on the threading, and block any consumer until an independent review
signs off.

---

## 7. Acceptance gates (ALL required before Task 4 consumes `dedupe_helpers`)

1. **Default-deny + certificate.** No positive result without a fully-populated
   `DedupeCertificate`; any obligation lacking evidence ⇒ `.None`. The certificate is the
   audit trail.
2. **Positive tests.** `insert_sorted` certifies as `SortInsertPrimitive` (with the expected
   `vector_param`/`induction_local`/`element_local`/`flag_local`); the `String`-element
   `insert_sorted_str` **also** certifies as `SortInsertPrimitive` (locks in that O1's equality
   evidence wildcards the `OpKind` field, so `Int` and `String` streams are both covered — §5.10);
   `int_keys_union` and the fixture `union_via_insert` certify as `Combinator`; the boot-main
   `insert_sorted` / `insert_sorted_str` / `int_keys_union` certify (verified after
   `make bundle-cli`).
3. **Adversarial negative battery — every one MUST be rejected** (each added as a
   classification unit-test fixture; each targets a specific obligation):
   - **repeated id-append** — id appended unconditionally every iteration (`bad`; O3).
   - **two id-appends in one `!inserted` block** — `if !inserted { append(id); inserted=true;
     append(id) }` (O3 instruction-sensitivity; the second append runs after the flag is set).
   - **fixed-index append** — `out = .append(v[0])` (`bad2`; O2).
   - **nested-loop element-append** — `for x in v { if x==id {return v} for _ in range(2) {
     out = .append(x) } }` (O2 dynamic multiplicity; one static site, two dynamic appends).
   - **fixed-index equality guard** — `if v[0] == id { return v }` (O1 full-traversal).
   - **second in-loop exit** — an extra `return`/break inside the loop besides the equality
     `return v` (O1 accumulator-return-path reasoning must not certify).
   - **`continue` before the equality guard** — an early `continue` that reaches the next
     induction step without evaluating `x == id` (O1 false-edge domination; §5.5).
   - **unchanged-counter back-edge** — a manual `i < v.len()` condition loop that appends
     `v[i]`, takes a `continue` forwarding `i` **unchanged**, then later falls through to
     `i = i + 1` (one static append site, no inner SCC, yet re-appends the same element).
     §5.3's every-back-edge-supplies-`ctr+1` rule must reject it.
   - **join block with a side effect** — the threaded diamond block `B` contains `flag = true`
     or a call before its `condbranch p` (§5.0 must NOT thread it away).
   - **rebound vector param return** — `v = <some other/duplicate vector>; … ; return v`
     (O0 no-vector-param-rebind).
   - **rebound `id`/element proof local** — `id = <other>` before an `append(id)`, or
     `x = <other>` before `append(x)` (O0 no-rebind of params / `element_local`).
   - **stale `!flag` guard** — `c := !inserted; append(id); inserted = true; if c { append(id)
     }` — the branch is on a stale `!inserted`; the §5.7 refinement must NOT treat the
     `if c` true edge as flag-false.
   - **flag reset inside the loop** — `for x in v { inserted := false; if !inserted {
     append(id); inserted = true }; append(x) }` — the in-loop `false` init re-arms the flag
     each iteration and appends `id` every time; §5.7's "one `false` def dominating the loop"
     must reject it.
   - **rebound vector alias base** — a local `w := v; …; w = <other vector>; …` used as the
     len/index base after the rebind (non-SSA staleness; §5.2 `alias_root` must refuse the
     rebindable link, §5.3/§5.4 frozen-evidence).
   - **cross-block stale `!flag` guard** — the `!flag` op is computed in one block and the
     branch is in a later block with an intervening `flag = true` on the path (§5.7 refinement
     must not fire — same-block-freshness only).
   - **accumulator at wrong arg position** — a combinator that passes the accumulator to a
     certified helper at a position other than that helper's certified `vector_param` (§5.8).
   - **short-circuit reconvergence trap** — an id-append that genuinely reconverges from the
     flag-true side with **no** excluding constant (so §5.0 threading does *not* fire and the
     flag-true edge really reaches it); the §5.7 dataflow must **not** certify it (guards
     against an over-eager O3 dataflow / a mis-scoped threading pass).
   - **param-rebind combinator** — rebinds a vector param or seeds the accumulator from a
     non-param/non-lineage value (O0/O4).
   - **wrong return value** — returns a non-lineage, non-`v` local (O0).
   - **extra accumulator mutation** — an accumulator write that is not a certified-helper call
     or an approved append (O0/O4).
4. **Independent review.** The analyzer is reviewed in isolation (subagent + human) against
   O0–O4 and the negative battery, specifically hunting for an over-certifying function,
   **before** Task 4 is unblocked.
5. **Threading-pass correctness (independent of the checker).** The §5.0 pass gets its own
   unit test: it rethreads `insert_sorted`'s constant `B10` edge, leaves a non-constant join
   untouched, and does not fire on non-empty-target-arg branches. A **verdict-equivalence
   check** compares the ownership analysis's rendered verdicts *with* vs. *without* the
   threading applied — running the classifier's **unthreaded mode** (§5.0 test seam (b):
   `classify_dedupe_helpers_unthreaded` or the `skip_threading` option) against the default
   threaded path — and requires them identical except at the intended copy-carrier sites
   (this is the actual soundness evidence — that threading changes nothing the analysis
   concludes elsewhere). The **full boot suite** (`target/twk run boot/tests/main.tw` after
   `make bundle-cli`) is run as regression coverage — it provides confidence, not a proof, of
   result preservation.

---

## 8. Decision log

- **2026-07-25:** Chose the **general proof-checker approach** and dropped the
  exact-shape-match and direct-`Dict.keys`-only alternatives. Generality preferred; the
  miscompile risk is accepted as *implementation* risk, contained by §7's gates (default-deny
  + explicit certificate + adversarial battery + independent review) rather than by narrowing
  the approach. The obligations were sharpened to O0–O4 (added output-lineage O0 and combinator
  O4; tied O1/O2 to a verified induction variable).
- **2026-07-25 (review round 9):** Grounding the design against the current boot tree folded in
  two **scope clarifications** (no soundness change): (A) although the sort-insert idiom is
  pervasive (`insert_sorted` int + `insert_sorted_int` + `insert_sorted_str`, plus combinator
  sites across `ownership.tw`/`summary.tw`/`cfg.tw`), **exactly one** stream currently feeds a
  copy-carrier in-place write (`int_keys_union` in `merge_targeted`) → §5.10 now states the
  consumer surface is one site and the generality is future-proofing, not present breadth; (B) a
  worry that the `String` variant `insert_sorted_str` would be **rejected** was checked and
  **dismissed** — `String` `==`/`<` lower to `ABinOp(.Eq/.Lt, …, .Str)` (not Eq/Ord-contract
  calls, since `is_primitive_cmp`/`uses_runtime_eq` treat `String` as primitive), and O1/§5.3
  already wildcard the `OpKind` field, so `String` certifies by the same evidence. §5.10 and gate
  2 now state `Int`+`String` coverage explicitly and add an `insert_sorted_str` positive fixture
  to guard it. Also folded four enforcement-wording cleanups (no semantic change): (1) gate 5's
  test seam now **requires** an unthreaded classification mode (`classify_dedupe_helpers_unthreaded`
  or a `skip_threading` option), since exposing `thread_const_branches` alone leaves no
  no-threading path through the full classifier for the verdict-equivalence diff (§5.0, gate 5);
  (2) §5.2 splits the flag out of "single-def" — it is *set-once* (exactly one `false` definition
  + only sanctioned `true` assignments), not single-def; (3) §5.6's inner-loop rule is restated
  as "reject if any non-induction back-edge / natural loop can re-enter the element-append before
  the induction increment," dropping the "second SCC" implementation phrasing; (4) removed
  count-based phrasing ("~9 sites") per project docs guidance.
- **2026-07-25 (review round 8):** An eighth review found the **non-SSA / reaching-definition**
  gaps, folded in: (1) `alias_root` could root a *rebindable* local at `vp` → refuse alias
  links through `AAssign` targets, and freeze all evidence locals (element/len-index chain/
  return) to single-def; (2) function-wide `build_def_map` (last-write) is unsound for
  multiply-defined locals → require **block-local instruction-ordered reaching-def** resolution
  for §5.0 threading and §5.7 flag freshness; (3) `!flag` freshness was underspecified
  cross-block → require the `!flag` op **in the same predecessor block** as the branch with no
  intervening `flag = true` (same-block check); (4) the empty-accumulator seed used the wrong
  ANF shape (`AInit` takes an atom) → `AInit(ALocal(arr))` with `arr`'s def `AArrayLit([])`;
  (5) gate 5 needs a **test seam** → expose `thread_const_branches` as `pub`. Also: clarified
  `CfgEdge.target` is the *predecessor* in a `preds` edge, and **restored the accidentally-
  dropped `## 6` heading** (lost when §5.11 was inserted). Added rebound-alias-base and
  cross-block-stale-guard negatives.
- **2026-07-25 (review round 7):** A seventh review found one blocker + one ANF-shape error,
  folded in: (1) the set-once flag could be **reset inside the loop** via an in-loop
  `inserted := false`, re-arming it each iteration (`find_set_once_flag` only guarded against
  `AAssign` resets and treated any `AInit(false)` as the init) → require **exactly one** `false`
  definition (counting both `AInit(false)` and `AAssign(flag,false)`) whose block **dominates
  the loop header** and is not on a back-edge; (2) the §5.3 back-edge advance wording used
  `AAssign(w, add)` with `add = ABinOp(...)`, but `AAssign`/`AInit` take an **`Atom`** while
  `ABinOp` is an `AnfOp` bound by `Let` → restated to follow the `ctr := t` source atom back to
  a local `t` whose *defining op* is `ABinOp(.Add, ctr, 1)`. Added the in-loop-flag-reset
  negative. Both are correctness/completeness fixes; no new over-certification route.
- **2026-07-25 (review round 6):** A sixth review caught three cases where prior tightenings
  would wrongly **reject the intended positive** (`insert_sorted`) or endorse stale code, folded
  in: (1) the round-4 O3 refinement required "flag false at the terminator," which contradicts
  the real primitive (`B8` entry is loop-carried maybe-true) → corrected: the `!flag` true edge
  establishes flag-false by itself; withhold the refinement **only** when the condition is
  *stale* (a `flag = true` between the `!flag` def and the branch); (2) §3 still endorsed the
  shipped `function_is_dedupe_combinator` (arg-0-only) → say it must be **replaced** by §5.8;
  (3) the back-edge advance rule literally demanded an `add` atom on the edge, but the CFG
  supplies `ALocal(ctr)` after an in-block `AAssign ctr = ctr+1` → accept both forms (else the
  real `insert_sorted` is rejected). These are false-*negative* fixes (they restore the
  acceptance target), not new soundness holes.
- **2026-07-25 (review round 5):** A fifth review found one blocker + two wiring gaps, folded
  in: (1) O1/O2 assumed a single counter-advancing back-edge, but a **condition** loop can have
  an extra back-edge that forwards `ctr` **unchanged** (a `continue` — not auto-incremented like
  indexed-`for`), re-appending `v[ctr]` with one static site and no inner SCC → §5.3 now
  requires exactly one init pred and **every** back-edge to supply `ctr + 1`; (2) the
  scoped/production summary path (`compute_for_roots_cached`) did not call `with_dedupe_helpers`
  → require certificate population on **all** summary paths; (3) threading was scattered in one
  caller → **centralize it inside `classify_dedupe_helpers`** so every entry point (full,
  scoped, tests) classifies over the same threaded view. Added the unchanged-counter-back-edge
  negative.
- **2026-07-25 (review round 4):** A fourth review found two more, both folded in: (1) the
  primitive forbade rebinding only *vector* params, but ANF is non-SSA — rebinding `id_param`
  or `element_local` makes an approved append stale → forbid `AAssign` to **all params** and to
  `element_local` (only the counter `+1` and the flag `= true` are permitted proof-local
  assigns); (2) the O3 `!flag`-true-edge refinement could fire on a **stale** condition
  (`c := !inserted; …; inserted = true; if c`) → apply the refinement only when the condition
  resolves to `AUnOp(.Not, flag)` *and* the flag is still definitely false at the terminator
  (no intervening set). Added the corresponding adversarial negatives.
- **2026-07-25 (review round 3):** A third review found five more over-certification routes,
  all folded in: (1) §5.0 threading could skip **ownership-relevant instructions** in the
  branch block → require `B` have no instructions but the trivial cond-alias; (2) the O3
  dataflow transfer was unsound (absence of a `flag=true` assignment ≠ flag false) → proper
  `flag_false_exit = flag_false_entry ∧ no-assign`, edge carries exit-state (except the
  `!flag`-true-edge refinement); (3) O1 could be bypassed by a `continue` before the equality
  → require the equality **false-edge dominates the induction increment/back-edge**; (4) the
  primitive allowed **vector-param rebinding** → forbid `AAssign` to vector params (as O4
  does); (5) the combinator ignored the callee's **certified `vector_param` position** → require
  the accumulator be passed there. Stale "optimizer/opt/codegen-benefit" wording removed;
  Appendix polarity corrected to `>=`; O3 tail-append rule restated (set flag before any later
  id-append/back-edge, or be in the terminal post-loop region).
- **2026-07-25 (review round 2):** A second independent review found five more soundness
  gaps, all folded in: (1) §5.0 was specified in CFG terms but placed in the ANF `opt/`
  pipeline where `CfgView` doesn't exist → reframed as an **analysis-only CFG-view
  simplification** (no codegen impact); (2) O3 was block-entry-sensitive only → made
  **instruction-sensitive** (kills flag-false at `inserted = true`); (3) O2 was a static site
  count → now bounds **dynamic** multiplicity (no nested loop around the element-append); (4)
  the induction check used the wrong branch polarity → matches the real `for` lowering
  (`ctr >= len` true→exit); (5) O1 stated a false property → restated as the **accumulator-
  return-path** property. Combinator wording corrected to "stricter replacement"; gate 5
  reworded (boot suite = regression coverage, not proof; added a verdict-equivalence check).
- **2026-07-25 (refinement):** Moved O3's value reasoning out of the checker into a **minimal
  jump-threading simplification** (§5.0), run before the ownership analysis. Rationale: the
  short-circuit reconvergence is a CFG-simplification concern, not a certifier concern, and it
  reduces O3 to a **standard must-false dataflow** (§5.7), removing the most miscompile-prone
  (per-edge constant-correlation) code from the certifier. Confirmed the existing
  `const_fold`/`branch_simplify` do **not** cover it (they fold only literal operands /
  literal `AIf` conditions, not param/phi-fed branches), and that general SCCP would not
  either (`L26 = meet(unknown, false)` is not a constant). *(Placement corrected in review
  round 2 below: it is a `CfgView → CfgView` analysis-only simplification, not an ANF `opt/`
  pass — CFG concepts don't exist in the ANF pipeline.)*

## Appendix: verified `insert_sorted` CFG (from `twk ir --cfg`)

Params `L0 = v` (vector), `L1 = id` (scalar). `L2 = out` (fresh `[]`), `L3 = inserted`
(init `false`, B0), `L6 = counter` (init `0`, B0). `Fn24 = Vector.append`; `Fn23` (in the
loop setup) = `Vector.len`.

```
B0  entry: L3 = false; L6 = 0; iterCount from Vector.len(v)
B3  loop.body: L20 = (L6 >= iterCount);  condbranch L20 → B4(exit) | B5(body)
B5  L21 = index(L4=v, L6);  x = L7;  L22 = (L7 == L1);  condbranch L22 → B6 | B7
B6  return L0            ; # early return of v when x == id   (O1)
B8  condbranch L24(= !L3 = !inserted) → B9 | B10
B9  L25 = (L1 < L7)       ; # id < x            B10  (constant false)
B11 join L26 (= L25 | false);  condbranch L26 → B12 | B13
B12 L28 = append(out, L1=id);  L30: inserted = true   ; # id-append #1 (O3)
B14 L32 = append(out, L7=x);   L34 = L6 + 1           ; # element-append (O2), counter++
B15 L39 = append(out, L1=id)   ; guarded by B2 condbranch !inserted   ; # id-append #2 (O3)
B17 return L2 (= out)
```

Two id-appends (`B12`, `B15`), both `!inserted`-guarded; one element-append (`B14`) of the
induction element `x = v[counter]`; equality early-return over the full traversal (`B6`).
The `B10` constant-`false` edge is what makes `B12` reachable only on the flag-false side —
the fact no control-flow-only analysis can see (§4.1).
