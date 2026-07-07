# Physical representation planning + post-route verification for typed vectors

**Status:** design (approved 2026-07-07, expanded to full soundness), not implemented
**Branch:** `typed-vector-crossfn-abi`
**Depends on:** landed cross-fn ABI work (accessor returns B2, group-aware copy
propagation B3/B4, tailify) — commits ac30af11..c117dd73.
**Unblocks:** C2 (captured columns) → the dataframe `order_by` sort win. See
[boundary-tracklist.md](boundary-tracklist.md).

## Goal

> Compute one whole-program physical representation plan for `Vector<Int>` storage
> sites. Routing, ABI decisions, slot retyping, and typedness queries all consume
> that plan. A post-route verifier rejects any producer/destination physical
> mismatch not mediated by an explicit coercion.

This is stronger than "unify the two oracles": it makes the `PVecI64`→`PVec` (and
inverse) store-mismatch class **structurally unrepresentable** — not merely absent
in the cases we tested.

## Problem

"Will slot S be physically `PVecI64` after routing?" is answered by two drifting
implementations — `route_func`'s `eligible_v` (the ground truth that retypes slots)
and `slot_typed_after_route` (the analyses' query, empty `relaxed`, single-slot).
They diverge on complex multi-use groups. The C2 spike (2026-07-07) proved it:
typing a captured call-result column, the two disagreed on the dataframe key column
(captured into two comparators + gathered + copied) — `route_func` dropped the
`box_i64` while the slot stayed `PVec` → **invalid Wasm**. Root cause in
[boundary-tracklist.md](boundary-tracklist.md) ("Spike finding").

Contributing gaps:
- **Ordering.** `capture_abi` is computed once at the end of the
  `analyze_typed_repr` fixpoint, so `route_func` sees inputs the analyses never
  iterated on.
- **Incomplete category coverage.** Only builder candidates and (post-fix)
  call-result/payload aliases are group-aware. Field-read copies (B5) are a known,
  still-open mismatch source. A soundness claim cannot carve any category out.
- **No structural backstop.** Emit trusts the analysis. A conservative or buggy
  analysis silently produces invalid Wasm rather than failing loudly.

## Design

### The physical representation plan

One whole-program value, computed once (at the fixpoint), consumed everywhere:

```tw
type PhysRepr = { Boxed, TypedI64 }   // rt_types__PVec | rt_types__PVecI64

type PhysPlan = .{
  slot_repr: Dict<String, PhysRepr>,     // "${func_id}:${slot_id}" -> repr
  field_repr: Dict<String, PhysRepr>,    // "${tid}:${fid}"
  payload_repr: Dict<String, PhysRepr>,  // "${tid}:${vid}:${idx}"
  param_repr: Dict<String, PhysRepr>,    // "${func_id}:${param_idx}"
  return_repr: Dict<String, PhysRepr>,   // "${func_id}"
  capture_repr: Dict<String, PhysRepr>,  // "${func_id}:${capture_idx}"
}
```

The existing `typed_fields` / `typed_payloads` / `typeable_return` / `capture_abi`
`Bool` maps are the current *partial* form of this — `PhysPlan` unifies and
materializes them and adds per-`(func, slot)` `slot_repr`. `Boxed` is the default
for anything absent.

### 1. `compute_eligible_v` extracted (pure) and materialized

Lift `route_func`'s eligibility prefix (copy_map → candidates → all source-category
sections → gather fixpoint) into a pure

```
compute_eligible_v(pf, ids, builtins, plan) -> EligibleSets  // .{ eligible_v, eligible_b }
```

The fixpoint's final action runs it for **every** function and writes the results
into `plan.slot_repr`. `route_func` then just **looks up** its slots in the plan to
rewrite + retype — it never recomputes eligibility. `slot_typed_after_route`
collapses to `plan.slot_repr["${func}:${slot}"] == TypedI64`. The second
implementation is deleted.

### 2. All source categories are group-aware — no carve-outs

Every category that can produce a typed vector gets the same `aliases_for` +
whole-group `!v_group_escapes` decision, so a *copy* of a typed source is typed iff
its source is (the fix already applied to call-result/payload, generalized):

- builder candidates (already);
- typed field reads (**B5** — currently single-slot, the known invalid-Wasm gap);
- typed payload reads;
- typed-return call-results;
- `gather`/`take`/helper results;
- capture params / free vars;
- direct params / returns.

### 3. Fold every ABI product into one fixpoint (fail-closed)

`analyze_typed_repr` computes the whole `PhysPlan` as a greatest fixpoint. Each
round derives, from the current plan: `slot_repr` (via `compute_eligible_v` per
func), then `field/payload/param/return/capture_repr` (the ABI analyses, now plan
consumers/producers). Round N uses round N−1's plan — the standard break.

**Cap exhaustion fails closed.** If the plan has not stabilized within the round cap
(`funcs.len() + 4`), do **not** ship the unstable plan (it may be inconsistent).
Instead drop every not-yet-stable typed decision to `Boxed` and re-derive once, so
the shipped plan is a proven-consistent all-conservative-where-unsure state. (A
debug assertion can additionally flag cap exhaustion so it is noticed, not silent.)

### 4. Emission is a mechanical consequence of the plan

Emit inserts a coercion purely from *source `PhysRepr` vs destination `PhysRepr`*:
`TypedI64 → Boxed` ⇒ `box_i64`; `Boxed → TypedI64` ⇒ `unbox_i64`; equal ⇒ none. No
emit-site re-derives typedness. (This is what the landed `Expected{vt,mono}`
coercion path already does; it now keys off the plan uniformly.)

### 5. Post-route verifier — the structural backstop (build FIRST)

Extend the existing backend verifier (`verify_expr.tw`, which already runs in the
compile path and does repr-compat checks) to reject any routed IR where a producer's
physical repr and its destination's physical repr differ without an explicit
coercion op:

- slot store vs slot physical type;
- call arg vs callee param ABI;
- return value vs function return ABI;
- field / payload / capture store vs that site's physical repr;
- no `PVecI64` into `Boxed` storage without `box_i64`; no `Boxed` into `TypedI64`
  storage without `unbox_i64` (or a proven typed producer).

Failure is an internal compiler error with a **Twinkle-level location** (func +
slot/site), not a Wasm byte offset. This is the guarantee: even if the plan is
conservative or a category is missed, emit cannot *silently* produce invalid Wasm —
the build fails loudly instead.

## Why this is sound

1. **One definition.** Typedness has a single materialized source (`PhysPlan`);
   there is no second implementation to drift.
2. **Fixpoint stability.** `route_func` and the analyses consume the same converged
   plan, so their views are identical by construction.
3. **Fail-closed.** Non-convergence yields a conservative, consistent plan, never an
   inconsistent one.
4. **Verified.** The post-route verifier rejects any residual mismatch, so the
   soundness claim does not rest on the analysis being complete — only on the
   verifier being correct.

## Staged implementation (one architecture, incremental + guarded)

The verifier makes the later stages safe to land one at a time.

- **Stage 1 — Post-route verifier.** Extend `verify_expr.tw` for the typed-vector
  storage sites. Must pass on current code + all tests (no false positives). This is
  the safety net; nothing after it can regress silently.
- **Stage 2 — Materialize `PhysPlan` + fold the fixpoint.** Extract
  `compute_eligible_v`; produce `slot_repr` + the ABI repr maps in one fixpoint;
  `slot_typed_after_route` → lookup; fail-closed on cap. Behavior-preserving where
  the two oracles already agreed; the verifier catches any place they didn't.
- **Stage 3 — Close all category carve-outs (incl. B5).** Make every source
  category group-aware. Verifier + self-host confirm each.
- **Stage 4 — Enable C2 + measure.** Captured columns type; dataframe `order_by`
  must **validate**; record the `sort idx by amount` number; guard bench stays fast.

## Risks

- **Convergence (mixed monotonicity).** Params shrink from optimistic;
  capture/return grow from empty. Bounded by the round cap; fail-closed handles
  non-convergence soundly. Verify: self-host fixed point, no oscillation on boot +
  dataframe.
- **Performance.** Materializing the plan replaces per-query recompute with one
  compute-per-func-per-round; net likely neutral-to-better than recomputing in
  `slot_typed_after_route`. Measure boot compile time (`TWINKLE_TIMINGS=1`).
- **Verifier false positives** on valid current code (Stage 1 gate catches these
  before it becomes a trusted net).

## Acceptance / testing

- Self-host fixed point + 2979+ boot tests.
- **Post-route verifier green on all of boot + the dataframe.**
- **Dataframe `order_by` validates** (record the sort number); guard bench fast.
- New regression: a `Vector<Int>` column captured into two comparators + gathered
  compiles validly and returns correct values (the exact spike shape).
- A field-read-copy regression (the B5 shape) compiles validly.

## Verify loop

```
cargo run --release -- build boot/main.tw -o /tmp/x.wasm
make bundle-cli && make boot-test
target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw
target/twk run examples/performance/sort-bench/typed_payload_capture_guard.tw
```

## Out of scope (still deferred, not soundness-relevant)

- **B8** — typed `take`/helper ABI for the *full* `order_by` number (a performance
  boundary, not a mismatch source; `take` results already box safely).
- Fixpoint performance tuning beyond measuring + the obvious per-round structure.
