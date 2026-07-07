# Unify the two typed-vector typedness oracles

**Status:** design (approved 2026-07-07), not yet implemented
**Branch:** `typed-vector-crossfn-abi`
**Depends on:** the landed cross-fn ABI work (accessor returns B2, group-aware
copy propagation B3/B4, tailify) — commits ac30af11..c117dd73.
**Unblocks:** C2 (captured payload/call-result columns) → the dataframe `order_by`
sort win. See [boundary-tracklist.md](boundary-tracklist.md).

## Problem

Two functions answer the same question — "will slot S be physically `PVecI64`
after routing?" — with two different implementations:

- **`route_func`'s `eligible_v`** (route_typed_vec.tw ~250–360): the *ground truth*
  that drives actual slot retyping. Uses `build_copy_map` + `aliases_for` for
  builder candidates and the group-aware call-result/payload sections (2c/2c''), a
  `relaxed` set derived from `collect_relaxed_captures(capture_abi)`, and a gather
  fixpoint.
- **`slot_typed_after_route`** (route_typed_vec.tw ~1180): the *query* used by
  `return_is_typed` and `analyze_typed_captures`. Single-slot checks with an
  **empty** `relaxed` set and a `free_var_typed_local` proxy for the builder logic.

They agree on simple code but **diverge on complex multi-use groups**. The C2 spike
(2026-07-07) proved the failure: making the analysis more aggressive typed a
captured call-result column, but `route_func` and `slot_typed_after_route`
disagreed on the dataframe's key column (captured into two comparators + gathered +
copied) — `route_func` dropped the `box_i64` while the destination slot stayed
`PVec` → **invalid Wasm** (`PVecI64`→`PVec` store mismatch). Root cause recorded in
[boundary-tracklist.md](boundary-tracklist.md) ("Spike finding").

An additional ordering gap feeds the divergence: `capture_abi` is computed **once at
the end** of `analyze_typed_repr`'s fixpoint, so `route_func` (which runs after,
with the final `capture_abi`) can see inputs the analyses never iterated on.

## Goal

One definition of typedness. `route_func` produces it; every other consumer queries
the same computation, so the two views can never disagree and the store-mismatch
class of invalid Wasm becomes structurally impossible.

## Design

### 1. Extract `compute_eligible_v` (pure)

Lift `route_func`'s eligibility prefix into a pure function:

```
compute_eligible_v(
  pf: PreparedFunc,
  ids: RouteIds,
  builtins: BuiltinRegistry,
  typed_fields: Dict<String, Bool>,
  typed_payloads: Dict<String, Bool>,
  capture_abi: Dict<String, Vector<Int>>,
  typeable_return: Dict<String, Bool>,
) EligibleSets   // .{ eligible_v: Dict<Int, Bool>, eligible_b: Dict<Int, Bool> }
```

It contains everything from `copy_map := build_copy_map(...)` through the gather
fixpoint (the block that currently ends right before `if eligible_v.keys().len() == 0`).
`route_func` becomes: call `compute_eligible_v` → `rewrite` → retype slots (its tail
is unchanged). This step is **behavior-preserving for `route_func`**.

### 2. `slot_typed_after_route` becomes a thin query

```
pub fn slot_typed_after_route(pf, slot, ids, builtins, typed_fields,
                              typed_payloads, typeable_return, capture_abi) Bool {
  compute_eligible_v(pf, ids, builtins, typed_fields, typed_payloads,
                     capture_abi, typeable_return).eligible_v.has(slot)
}
```

The body drops the payload/field/call-result/`free_var_typed_local` cases entirely —
they are subsumed by `compute_eligible_v`. Signature gains `capture_abi`. Callers
(`return_is_typed`, `analyze_typed_captures`) thread it through. The second
implementation is deleted, so drift is impossible by construction.

### 3. Fold `typeable_return` + `capture_abi` into the fixpoint

`analyze_typed_repr` currently iterates `typed_payloads`/`typeable_params` and
computes `capture_abi` once at the end. Make both ABI products participate. Each
round:

```
tpay        = analyze_typed_payloads(funcs, builtins, first_user_tid, tp_params)
abi         = analyze_typed_params(funcs, builtins, tf, tpay, capture_abi_prev)
capture_abi = analyze_typed_captures(funcs, builtins, tf, tpay,
                                     abi.typeable_return, capture_abi_prev)
done when typeable_params AND typeable_return AND capture_abi are all stable
```

- `analyze_typed_params` (hence `return_is_typed`) and `analyze_typed_captures`
  gain a `capture_abi` input, which they pass into `slot_typed_after_route`.
- Round N uses round N−1's `capture_abi` — the standard fixpoint break.
- Bounded by the existing round cap (`funcs.len() + 4`).

### Why this is sound

At a fixpoint, one more application of the oracle changes nothing. So when
`route_func` runs afterward with the **final** `capture_abi`/`typeable_return`,
`compute_eligible_v` reproduces exactly the eligibility the analyses used to derive
those sets. **`route_func`'s view ≡ the analyses' view.** Emit never drops a box
that the slot's physical type contradicts. The `PVecI64`→`PVec` store mismatch is
structurally impossible.

### Expected side effect: C2 falls out

Because `slot_typed_after_route` now uses the full eligibility logic (proper
`relaxed` from the fixpoint's `capture_abi`, alias groups), a captured typed-return
call-result column becomes typeable *and* `route_func` agrees. So the dataframe key
column should type into the comparator, and — at minimum — the `order_by` bench must
now **validate**. Whether the sort number fully drops is the acceptance measurement,
not a guaranteed target.

## Risks

- **Convergence (mixed monotonicity).** `typeable_params` shrinks from an optimistic
  seed; `capture_abi`/`typeable_return` grow from empty. The existing round cap
  guarantees termination at a conservative state (worst case: some vectors stay
  boxed — always *safe*, never invalid). Verify empirically: self-host fixed point,
  and confirm no oscillation (stabilizes within the cap on boot + the dataframe).
- **Performance.** `compute_eligible_v` recomputed per-func per-round is heavier than
  today's single end-pass (each call does `build_copy_map` + escape walks + the
  gather fixpoint). Correctness first; measure boot compile time with
  `TWINKLE_TIMINGS=1`. If it regresses materially, cache `compute_eligible_v` results
  per round (keyed by func id) — the inputs are constant within a round.

## Acceptance / testing

- **Self-host fixed point** — boot compiles itself under the unified oracle (the
  strongest whole-program consistency check).
- **2979+ boot tests pass.**
- **The dataframe `order_by` bench validates** (no invalid Wasm) — the direct proof
  the two views agree. Record the `sort idx by amount` number.
- **Guard bench** (`typed_payload_capture_guard.tw`) stays fast (no per-read-unbox
  miscompile).
- **New regression:** a `Vector<Int>` column captured into two comparators and
  gathered compiles validly and returns correct values (the exact shape that broke
  in the spike).

## Verify loop

```
cargo run --release -- build boot/main.tw -o /tmp/x.wasm   # stage0 builds boot
make bundle-cli                                            # self-host fixed point
make boot-test                                             # suite
target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw
target/twk run examples/performance/sort-bench/typed_payload_capture_guard.tw
```

## Out of scope (deferred)

- **B5 / C3** — the field-read copy invalid-Wasm (make section 2b group-aware). The
  unified oracle may or may not subsume it; if a field-read copy still boxes
  inconsistently after unification, fix 2b's alias handling as a follow-up.
- **B8** — typed `take`/helper ABI, for the *full* `order_by` number.
- Performance tuning of the fixpoint beyond the caching fallback above.
