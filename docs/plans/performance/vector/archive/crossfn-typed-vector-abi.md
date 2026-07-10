# Cross-function typed-vector ABI — design

**Branch:** `typed-vector-crossfn-abi`. Written 2026-07-05.
Step 1 of [typed-vector-continue-here.md](typed-vector-continue-here.md): let typed
`PVecI64` vectors flow through function calls instead of boxing at every boundary.
Grounded in the WAT evidence in
[crossfn-abi-instrumentation.md](crossfn-abi-instrumentation.md).

## Problem

Milestone A made storage typed (`ColData.IntCol_0 : PVecI64`), but the typed
representation boxes the instant it is *consumed* across a boundary. The
instrumentation found three boxed-only boundaries in the dataframe `order_by`:

1. **Return-as-`Vector<Int>` ABI** (`as_ints`) — extract typed payload, `box_i64`,
   return boxed.
2. **Generic runtime-helper ABI** (`gather`/`take`) — extract typed, `box_i64`,
   call the generic boxed `rt_arr__gather`, `unbox_i64` back. The gather loop runs
   on boxed `PVec`; typed storage buys zero read speedup. (~830ms.)
3. **Closure-env capture ABI** (`sort_indices_by_column`) — box the key column and
   capture it into the `anyref` closure env; comparator reads it boxed in the
   O(n log n) loop. (~1317ms.)

Root cause: function params/returns and the closure env are **boxed-only ABIs**.
The typed rep is confined to storage sites and boxes on any consumption.

This work covers boundaries #1 and #2. Boundary #3 (the closure env) is
**step 2 / M1b** and is out of scope here — but Stage 2 is its prerequisite (the
key column can only reach `sort_indices` typed once the param ABI stops boxing).

## Goals

- Type the `gather`/`take` direct-read path (boundary #2) so column reorders run
  on unboxed `PVecI64`. (~830ms lever.)
- Let typed `Vector<Int>` flow across monomorphic function call boundaries
  (boundary #1) so the dataframe column stays typed from `int_col` build through
  the read path, and so a typed column can reach `sort_indices` (unblocking step 2).

## Non-goals

- **No typed closure environments** (boundary #3 / step 2). A `Vector<Int>`
  captured into the `anyref` closure env still boxes.
- **No global typing of `Vector<Int>`.** `repr_of_mono(Vector<Int>)` stays
  `TypedRef` (erased/default). Typing remains opt-in per proven flow path. Making
  the classifier globally typed is the reverted-uniform-typing bug
  ([m1a-anyref-readback-investigation.md](m1a-anyref-readback-investigation.md));
  we do not go there.
- **No new element families.** I64 only (matches `ElemRepr` M1a scope).
- **Index vector (`idx`) typing** in `gather_i64` is deferred (see Stage 1).

## Invariant (unchanged)

A `Vector<Int>` that reaches a genuinely-erased boundary — `anyref`, the closure
env, a generic container, an erased (builtin-sum) variant — is **boxed at the
crossing and read boxed**. Stage 2 adds exactly one new *non-erased* boundary: a
monomorphic function's param/return ABI. Nothing else changes. The
`route_typed_vec` escape guard remains the safety mechanism; we relax it in
exactly one place (arg into a param-typeable slot) and nowhere else.

## Stage 1 — typed `gather` twin (contained, independently shippable)

`table.take` is `column.gather` under the hood, so the direct-read path is one
runtime op: `gather(vec: PVec, idx: PVec) → PVec` in `codegen/runtime/arr.tw`,
which reads `vec` via the boxed `get`.

### New runtime op

Add `gather_i64(vec: PVecI64, idx: PVec) → PVecI64` following the
runtime-builtin-wiring recipe (append-at-end FuncId discipline, boot + stage0):

- `vec` read via `get_i64` — raw i64 leaves, no `BoxedInt` pointer-chase. **The win.**
- `idx` stays boxed `PVec` for the first cut. It is the permutation vector, read
  once per output element and truncated to i32 anyway; typing it is a separable,
  marginal follow-up.
- result built via `builder_*_i64` → returns `PVecI64`.

### Routing (extends the existing swap table in `route_typed_vec.tw`)

1. Add `gather` / `gather_i64` funcids to `RouteIds` + `route_ids`.
2. Add one arm to `rewrite_op`: `fid.id == ids.gather and arg0 (vec) ∈ eligible_v
   → gather_i64`. The result slot joins `eligible_v` so it stores typed straight
   into the `IntCol` payload — the existing `all_payload_keys_typed` path handles
   the typed store, no new machinery.

### Where it fires

`column.gather` is `.IntCol(v) => ColData.IntCol(v.gather(idx))`. `v` is the
extracted payload — already `PVecI64` (Milestone A) → a typed payload read; the
result flows into a typed payload store. The `struct.get ColData 1 → box_i64 →
rt_arr__gather` sequence becomes `struct.get ColData 1 → rt_arr__gather_i64` —
box eliminated, reads unboxed.

### Scope guard

Stage 1 touches **only** `gather` (+ `gather_i64`) and the two routing edits.
`as_ints`/`int_col`/`sort_indices` boxes are out of scope (Stage 2 / step 2).

## Stage 2 — cross-call typed ABI (specialize-by-representation)

**Location.** A whole-program backend pass over `PreparedFunc`s, running
with/after `route_typed_vec`. The dataframe functions are **not generic** (they
take concrete `Vector<Int>`), so Core-IR monomorphization never clones them —
repr-specialization is a **new axis at the prepared-IR layer**, not an extension
of type-args monomorphization. (Precedent: `tramp_N` / `typed_tramp_N` repr
variants already coexist in emitted code.)

### (a) Per-function typeability — local, monotone

For each function compute two facts from its body:

- **param-typeable(f, i)** — the `Vector<Int>` param `i` is used only in
  typed-compatible ways: indexed (`get_i64`), `len_i64`, stored into a typed
  field/payload, returned as the typed result, or passed to another function's
  *param-typeable* slot. It is **not** typeable if it reaches an erased boundary
  (captured into a closure env, placed in a generic container / erased variant,
  or passed to an `anyref`).
- **return-typeable(f)** — f's returned value is a typed (`PVecI64`) slot.

The "passed to another param-typeable slot" clause makes this a **bounded
monotone fixpoint** over the call graph (typeability only grows; converges in a
few iterations). Dataframe chains are shallow, so it settles fast. Cap iterations
to stay safe on pathological graphs.

### (b) Repr-variant emission

For each `f` with ≥1 typeable param or a typeable return, clone the
`PreparedFunc` into an `f$i64` variant: retype those param/result slots to
`PVecI64`, then re-run `route_typed_vec` on the clone so internal uses stay typed
and box only at genuine internal erased boundaries. The boxed original remains for
boxed callers.

### (c) Call-site ABI selection + escape-guard relaxation

The one change that lets typed-ness cross calls: in `route_typed_vec`'s escape
classifier, "arg passed to a *param-typeable* slot" stops counting as an escape.
Then:

- rewrite a call whose actual arg is `eligible_v` to target `f$i64`;
- if `f$i64` returns `PVecI64` and the caller's result slot is typed, no box.

This resolves the `amounts ↔ int_col` chicken-and-egg: `int_col`'s `values` is
param-typeable (stored into the typed `IntCol` payload), so `amounts` escaping
into it is no longer a boxing escape → `amounts` types through `gen.table`'s
build loop, and `int_col$i64` takes `PVecI64` directly.

### What stays boxed (correctly)

- `sort_indices_by_column`'s key column — captured into the `anyref` closure env,
  so not param-typeable. Stays boxed until step 2 (typed closure env).
- Any `Vector<Int>` reaching a generic container, erased (builtin-sum) variant, or
  `anyref`.

### Honest perf expectation

Stage 2's *direct* `order_by` movement is modest — the columns are already typed
in storage (Milestone A), so Stage 1 alone captures the gather win. Stage 2's
value is (1) end-to-end typing of the **build** path (`gen.table` → `int_col`),
and (2) it is the **prerequisite for step 2**, which owns the big comparator win
(~1317ms). Set expectations accordingly: the umbrella `order_by` number moves
meaningfully only once step 2 lands on top of Stage 2.

## Testing & validation

- **Boot suites.** Extend `backend_repr_suite` / `route_typed_vec` coverage:
  Stage 1 — a `gather` on a typed payload lowers to `gather_i64` with no
  `box_i64`. Stage 2 — a function taking `Vector<Int>` stored into a typed
  payload emits an `$i64` variant; a typed caller targets it; a boxed caller
  targets the original; a param captured into a closure stays boxed (guard test).
- **WAT probes** (new, under `examples/performance/sort-bench/`): a
  build-then-gather program where the column stays `PVecI64` end to end — assert
  `grep -c box_i64` drops to the expected count and `gather_i64` appears.
- **Escape-guard regression.** A probe that passes a typed vector into a function
  that captures it into a closure and reads it in a loop must NOT type it — assert
  runtime stays ~ms (the reverted-bug guard, mirroring
  `typed_payload_capture_guard.tw`).
- **Self-host + full suite.** `make bundle-cli` reaches fixed point; `make
  boot-test` all green.
- **Bench.** `examples/performance/dataframe/bench/main.tw` — record `order_by`
  and the `filter`/`group_by`/`join` numbers before/after each stage; expect the
  gather path (Stage 1) and build path (Stage 2) to move, `order_by` umbrella to
  move materially only after step 2.

## Risks

- **Fixpoint termination / cost.** Mitigated by monotonicity + an iteration cap;
  the pass is analysis-only until emission.
- **Instance blow-up.** Up to 2× prepared funcs for functions on typed paths.
  Bounded by "emit a variant only when a typed caller actually demands it"
  (demand-driven emission), and DCE drops unreferenced variants.
- **Correctness of ABI selection.** A mis-selected typed call to a boxed callee
  (or vice-versa) is a hard type error at wasm validation, so mistakes fail loud
  at build, not silently at runtime.
- **stage0 parity.** Backend-codegen opts follow the "no stage0 parity required"
  rule for the boot codegen path; Stage 1's runtime op still needs the stage0
  trap-stub/registration so `make stage2` bootstraps.

## Doc map

- [crossfn-abi-instrumentation.md](crossfn-abi-instrumentation.md) — the WAT
  evidence this design is grounded in.
- [typed-vector-continue-here.md](typed-vector-continue-here.md) — the handoff
  that prioritized this as step 1.
- [../representation-boundary-policy.md](../../representation-boundary-policy.md) —
  the storage-site model + the reverted-uniform-typing lesson.
