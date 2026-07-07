# Physical representation planning + post-route verification for typed vectors

**Status:** design (approved 2026-07-07, expanded to full soundness), not implemented
**Branch:** `typed-vector-crossfn-abi`
**Depends on:** landed cross-fn ABI work (accessor returns B2, group-aware copy
propagation B3/B4, tailify) — commits ac30af11..c117dd73.
**Unblocks:** C2 (captured columns) → the dataframe `order_by` sort win. See
[boundary-tracklist.md](boundary-tracklist.md).

## Goal

> Compute one whole-program physical representation plan for `Vector<Int>` storage
> sites. Routing, ABI decisions, slot retyping, typedness queries, and verifier
> expectations all consume that plan. The post-route verifier rejects any
> producer/destination physical mismatch unless the exact IR site is a declared
> emitter coercion site.

This is stronger than "unify the two oracles": it makes the `PVecI64`↔`PVec`
store-mismatch class structurally unrepresentable in prepared IR. The verifier is
part of the guarantee: if analysis misses a category, the build fails with a
Twinkle-level diagnostic instead of surfacing later as invalid Wasm.

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
  analysis can silently produce invalid Wasm rather than failing loudly.

## Design

### The physical representation plan

One whole-program value, computed at the fixpoint and consumed everywhere:

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

`Boxed` is the default for absent entries. The existing `typed_fields`,
`typed_payloads`, `typeable_return`, and `capture_abi` maps are the current partial
form of this plan.

### ABI ownership and invariants

`slot_repr` is the canonical source for prepared slot metadata. The ABI maps are
indexed projections used by layout, call, return, and closure construction logic.
They must agree with the corresponding slots after routing:

- `param_repr["${func}:${i}"] == slot_repr["${func}:${pf.params[i].id}"]`
- `capture_repr["${func}:${i}"] == slot_repr["${func}:${pf.captures[i].id}"]`
- `return_repr["${func}"]` is the function's physical result ABI (`phys_return`
  when typed, default boxed ABI otherwise)
- `field_repr` and `payload_repr` are the layout owners for record fields and sum
  payload slots

Routing materializes these invariants by retyping parameter/capture/local slots in
`pf.slots`, setting `phys_return` from `return_repr`, and leaving boxed sites at the
mono-derived default. Direct-call emission and verification then read the callee's
prepared parameter slots as the physical parameter ABI. Closure trampolines and
closure construction read `capture_repr`/capture slot metadata; no separate hidden
param/capture oracle is allowed.

### 1. `compute_eligible_v` extracted (pure) and materialized

Lift the complete physical eligibility decision out of `route_func`:

```
compute_eligible_v(pf, ids, builtins, plan) -> EligibleSets  // .{ eligible_v, eligible_b }
```

This includes every no-route gate and every source collection step, not just the
block after `copy_map`:

- the prepared-depth bailout;
- candidate, field-read, payload-read, typed-return, capture, and typed-gather
  source collection;
- relaxed capture computation from the input plan;
- alias-group decisions;
- gather-result fixpoint;
- the empty-result case.

The fixpoint's final action runs `compute_eligible_v` for every function and writes
the results into `plan.slot_repr`. `route_func` then **looks up** its slots in the
plan to rewrite and retype; it never recomputes eligibility. `slot_typed_after_route`
collapses to a `plan.slot_repr` lookup and the second implementation is deleted.

**Cost lever (call out now, don't discover later).** Today `compute_eligible_v`'s
logic runs *once per function* (inside the single post-fixpoint `route_typed_vectors`
pass). Folding `slot_repr` into the fixpoint runs it up to `cap = funcs.len() + 4`
times per function, and each run rebuilds `copy_map` and executes the bounded-512
gather-result loop — worst case O(funcs² × body × gather-inner-fixpoint). For boot
(many functions) this is the dominant compile-time risk, ahead of monotonicity. The
mitigation is designed in, not deferred: recompute a function's `slot_repr` only when
one of its input ABI facts (`field_repr`/`payload_repr`/`param_repr`/`return_repr`/
`capture_repr` entries it reads) changed since the previous round; otherwise carry the
prior round's `slot_repr` forward unchanged. String-key churn from the
`"${func}:${slot}"` scheme (replacing today's per-func `Dict<Int, Bool>`) pairs with
this — cache keys per function rather than rebuilding them each round.

### 2. All in-scope source categories are group-aware

Every in-scope category that can physically produce a typed vector gets the same
`aliases_for` + whole-group escape decision, so a copy of a typed source is typed iff
its source is:

- builder candidates;
- typed field reads (**B5** — currently single-slot, the known invalid-Wasm gap);
- typed payload reads;
- typed-return call-results;
- typed `gather` results;
- capture params / free vars;
- direct params / returns.

Helpers without a typed ABI in this plan remain `Boxed` sources. That keeps B8
(`take`/additional helper typed ABI) out of this soundness plan: boxed helper results
are safe, just slower. When B8 is implemented later, each helper joins the same
source-category framework and verifier coverage.

### 3. Fold every ABI product into one fixpoint

`analyze_typed_repr` computes the whole `PhysPlan` as a greatest fixpoint. Each
round derives, from the previous round's plan:

1. per-function `slot_repr` via `compute_eligible_v`;
2. `field_repr` and `payload_repr` from producer/consumer scans;
3. `param_repr`, `return_repr`, and `capture_repr` from ABI analyses that query the
   previous/materialized slot plan, not a separate predicate.

Round N uses round N−1's plan to break cycles. Capture typing must not start from an
empty non-bootstrapping state: it uses a provisional Condition-1 capture-candidate
set, then shrinks unsupported entries until stable. The final routed plan uses only
the converged `capture_repr`.

**Monotonicity (the load-bearing convergence argument).** Today's fixpoint is a
clean monotone shrink: params start optimistically all-typeable and only lose
typeability (typed_param_abi.tw). Folding `slot_repr` in mixes directions —
`param_repr` shrinks from optimistic, while `capture_repr`/`return_repr` may *grow*
(a capture becomes supported once its source slot is typed) before shrinking through
support checks, and `slot_repr` grows as more ABI facts become typed. Convergence
holds because every growth edge is gated by a *support* predicate (a capture/return
is typed only if a concrete typed source justifies it) that can flip a decision from
typed→boxed but never boxed→typed→boxed→typed in a cycle: once a support check
fails, that entry stays boxed for the rest of the run. The join therefore settles
rather than oscillates. This is the argument the fixpoint relies on in the common
case; §4's fail-closed fallback is the safety net for the case where the reasoning
(or the round cap) is wrong, not a substitute for it.

### 4. Cap exhaustion fails closed

The fixpoint must either stabilize or ship a plan whose consistency is obvious. If
it reaches the round cap before stability, discard the unstable ABI decisions and
build a conservative fallback plan:

- `field_repr`, `payload_repr`, `param_repr`, `return_repr`, and `capture_repr` are
  all `Boxed`/absent;
- recompute `slot_repr` once against that fixed boxed ABI, allowing only local
  non-escaping typed-vector slots and typed `gather` results whose receiver and
  result stay fully local under the boxed ABI;
- run the verifier against the fallback plan;
- emit a debug/timing diagnostic so non-convergence is noticed.

Do not ship a partially unstable plan. A debug build may choose to abort instead of
using the fallback, but release behavior must be conservative and verifier-clean.

### 5. Emission follows declared physical edges

Prepared IR does not currently have a generic `ACoerce` op. Coercions are emitted by
specific sites that already call `emit_coerce_stack` or a specialized ABI shim:
record fields, variant payloads, direct-call args/results, returns, runtime helper
results, anyref wrap/unwrap paths, and builder shims.

The plan therefore defines two classes of edges:

- **Non-coercing edges** must have equal physical repr in prepared IR. Examples:
  `AInit`/`AAssign` local stores, `ARecordGet` result slot, plain copy aliases,
  capture slot entry after closure construction, and routed helper result slots.
- **Declared coercion edges** may differ only if the corresponding emitter site is
  known to call the coercion path and the verifier confirms that exact source/target
  pair is supported (`TypedI64 → Boxed` uses `box_i64`, `Boxed → TypedI64` uses
  `unbox_i64`, equal uses no coercion).

No emit site re-derives typedness. It receives the source physical type from the
producer/slot metadata and the destination physical type from the plan/layout/ABI.

### 6. Post-route verifier — the structural backstop

Extend `verify_expr.tw` around an explicit producer-representation model.
Separate result-producing ops from storage/check-only edges so the verifier never
compares a side-effect op's bookkeeping result slot to the stored value:

```
result_producer_phys_repr(op, plan):
  AInit(atom)              -> atom_phys_repr(atom, plan)
  ARecordGet(target,f,t)   -> field_repr["${t}:${f}"]
  ACall(global,args)       -> callee return_repr or builtin/runtime ABI result
  routed gather_i64        -> TypedI64
  routed/default helpers   -> their declared ABI result repr
```

For every `Let(slot, op, body)` whose result is a `Vector<Int>` physical site, the
verifier compares `result_producer_phys_repr(op, plan)` to
`slot_repr[current_func:slot]` according to the edge class:

- non-coercing result edges require equality;
- declared coercion result edges must be one of the supported coercions;
- missing producer facts are verifier errors for `Vector<Int>` physical sites, not
  silent success.

Storage-side checks use the same physical comparison but target the destination
site, not the `Let` result slot:

```
storage_edge_check(edge, plan):
  AAssign(target, atom)    -> atom_phys_repr(atom) vs slot_repr[current_func:target]
  ARecord field value      -> atom_phys_repr(value) vs field_repr["${tid}:${fid}"]
  ARecordUpdate value      -> atom_phys_repr(value) vs field_repr["${tid}:${fid}"]
  AVariant payload arg     -> atom_phys_repr(arg) vs payload_repr["${tid}:${vid}:${idx}"]
  AMakeClosure free var    -> atom_phys_repr(free_var) vs capture_repr[target_func:idx]
  direct-call argument     -> atom_phys_repr(arg) vs param_repr[callee:idx]
  Return(atom)             -> atom_phys_repr(atom) vs return_repr[current_func]
```

Failures report the function, slot/site, producer repr, destination repr, and whether
the site was expected to be coercing. This replaces broad ref-to-ref permissiveness
for `PVec`/`PVecI64` edges.

Stage 1 can start by checking the current metadata/layout instead of the future
`PhysPlan`, but it must use the same producer/destination model: direct calls use
the callee's prepared parameter slot `wasm_type` and `phys_return`, record/sum sites
use layout, and local/capture sites use prepared slot metadata. After Stage 2 the
verifier reads the materialized plan directly.

**Prerequisite is already met.** The cross-function lookups this needs (callee param
slots + `phys_return` for direct-call checks) require the whole prepared func set at
verify time — `VerifyCtx` already carries `funcs_by_id: Dict<Int, PreparedFunc>`
(verify_expr.tw), so Stage 1 adds no new plumbing. Capture-store checks reuse the
existing `canonical_capture_source_local` machinery rather than a new capture oracle.
The existing `pvec_repr_mismatch` field-store check (verify_expr.tw) is the working
precedent this generalizes.

## Why this is sound

1. **One definition.** Physical typedness has one materialized source (`PhysPlan`);
   all old Bool maps and typedness queries are projections.
2. **ABI invariants.** Parameter, capture, return, field, payload, and slot views
   are required to agree after routing.
3. **Fixpoint stability.** `route_func` and the analyses consume the same converged
   plan, so their views are identical by construction.
4. **Fail-closed.** Non-convergence ships only a conservative boxed-ABI fallback,
   never a partially unstable plan.
5. **Verified physical edges.** The post-route verifier checks producer repr vs
   destination repr for every typed-vector edge, allowing mismatches only at
   declared coercion sites with supported box/unbox behavior.

## Staged implementation

The verifier model lands first, but in a form that matches today's IR: it checks
non-coercing edges directly and mirrors declared emitter coercion sites rather than
requiring an `ACoerce` op that does not exist.

- **Stage 1 — Verifier physical-edge model.** Extend `verify_expr.tw` with strict
  `PVec`/`PVecI64` producer-vs-destination checks for current metadata/layout:
  local stores, record-get result slots, record fields, variant payloads, returns,
  direct-call args/results, and closure captures. It must pass current code and
  tests before later routing changes rely on it.
  **B5 sequencing check (do this first).** The doc calls field-read copies (B5) a
  "known, still-open invalid-Wasm gap." Stage 1 being green on current code requires
  that gap to be *latent* — reachable in principle (`x := rec.field; y := x` makes
  `x` `eligible_v` single-slot at route_typed_vec.tw:292 while the copy `y` stays
  boxed) but not exercised by any boot source path or existing test. Confirm this
  before starting: build boot + run the suite with the strict verifier and check no
  current path trips B5. If some compiled path *does* hit it, Stage 1 is not
  independently landable — it must land together with the B5 fix (Stage 3's field-read
  group-awareness), and the staging collapses those two. The acceptance list's
  field-read-copy regression is a *new* test precisely because no current one covers
  the shape.
- **Stage 2 — Materialize `PhysPlan` + fold the fixpoint.** Extract
  `compute_eligible_v`; produce `slot_repr` plus ABI repr maps in one fixpoint;
  make `slot_typed_after_route` a lookup; materialize param/capture slot metadata
  and `phys_return`; implement conservative cap fallback.
- **Stage 3 — Close all in-scope category carve-outs.** Make field reads and every
  other in-scope source category group-aware. Verifier coverage is the gate for
  each category.
- **Stage 4 — Enable C2 + measure.** Captured columns type; dataframe `order_by`
  must validate; record the `sort idx by amount` timing; guard bench stays fast.

## Risks

- **Convergence (mixed monotonicity).** Params shrink from optimistic;
  capture/return may grow before shrinking through support checks. The support-gate
  monotonicity argument in §3 is why it settles; the boxed-ABI fallback handles the
  case where it doesn't, soundly. Verify self-host stabilization and the dataframe
  path.
- **Verifier false positives.** Stage 1 intentionally lands before plan changes so
  valid current code proves the verifier's declared coercion model matches emitter
  behavior.
- **Performance (the dominant risk, not monotonicity).** Folding `slot_repr` into the
  fixpoint turns one eligibility pass per function into up to `funcs.len() + 4` — see
  §1's O(funcs²) analysis. The dirty-tracking lever (recompute a function's
  `slot_repr` only when an ABI fact it reads changed) is designed in, not deferred.
  Measure boot compile time with `TWINKLE_TIMINGS=1`; if the lever is insufficient,
  the plan structure itself needs revisiting, so treat this as a gating measurement,
  not a post-hoc check.

## Acceptance / testing

- Self-host fixed point and boot tests pass.
- Post-route verifier is green on boot and the dataframe path.
- Dataframe `order_by` validates; record the sort timing; guard bench stays fast.
- New regression: a `Vector<Int>` column captured into two comparators and gathered
  compiles validly and returns correct values (the spike shape).
- Field-read-copy regression (the B5 shape) compiles validly and exercises the
  verifier's non-coercing `ARecordGet`/copy checks.
- Cap-exhaustion/unit regression proves the fallback ships boxed ABI decisions, not
  unstable typed ABI decisions.

## Verify loop

```
cargo run --release -- build boot/main.tw -o /tmp/x.wasm
make bundle-cli && make boot-test
target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw
target/twk run examples/performance/sort-bench/typed_payload_capture_guard.tw
```

## Out of scope

- **B8** — typed `take`/additional helper ABI for the full `order_by` number. These
  helpers remain boxed in this plan, which is sound because boxed helper results
  cross through declared coercion sites or stay boxed. When B8 lands, those helpers
  must be added to `PhysPlan`, `compute_eligible_v`, and verifier coverage.
- Fixpoint performance tuning beyond measuring and the per-round materialized plan
  structure.
