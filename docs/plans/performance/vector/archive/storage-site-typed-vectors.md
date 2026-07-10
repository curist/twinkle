# Storage-Site Typed Vectors — Milestone A design

**Status:** ✅ IMPLEMENTED (2026-07-05) on branch `typed-vector-repr-m1a` (not
merged to main) — see the completion note in
[storage-site-typed-vectors-plan.md](storage-site-typed-vectors-plan.md). The
mechanism landed safely (typed variant payloads, capture-safe, no M1a pathology);
the conservative eligibility does not yet type real dataframe columns, so the app
speedup is deferred to broader eligibility. Design approved 2026-07-04, for the
redesign called for in
[../representation-boundary-policy.md](../../representation-boundary-policy.md) after
the uniform-typing activation was reverted
([m1a-anyref-readback-investigation.md](m1a-anyref-readback-investigation.md)).

**Scope:** Milestone A = *foundation + typed variant/sum payloads*. Typed closure
environments are **Milestone B (M1b)**, explicitly out of scope here (see
Non-goals). B builds on A as "one more typed storage-site kind."

## Goal

Make `TypedVec(I64)`/`PVecI64` a **per-storage-site** representation and support it
through **closed typed storage**: non-escaping locals (exists — S2.0), typed
record fields (exists — S2.2), and **typed sum/variant payloads** (new). Any
durable erased boundary stays boxed `PVec`. This yields typed *direct* reads of
variant-held columns (dataframe `IntCol(Vector<Int>)` → `gather`/`take`/indexed
reads) without reintroducing the M1a per-read pathology.

## The load-bearing invariant

> Typed storage is **site-local**. Typedness does **not** follow the semantic
> `Vector<Int>` value. Every extraction / rebinding / capture / erase chooses a
> representation again from the **destination** site.

Corollaries:

- `Vector<Int>` is stored as `PVecI64` only at a **closed typed site** (physical
  storage is `PVecI64`, not `anyref`).
- When the value leaves that site into a durable erased boundary (`anyref`, the
  universal `ClosureEnv`, generic container payload, erased `Variant`, unknown
  ABI), it is **boxed to `PVec` once** at the crossing.
- Reads from a closed site may stay typed; **extracted copies are reclassified by
  their destination site**. A typed variant payload does **not** imply a typed
  extracted copy — e.g. `keys := extract IntCol; idx.sort_by(fn(a,b){ keys[a] })`
  captures `keys` into today's `anyref` env, so escape analysis keeps `keys`
  **boxed** (one `box_i64` at extraction, boxed reads in the comparator). This is
  what makes the M1a O(n)-per-read trap **structurally impossible** in A.

## Candidate family ≠ actual storage repr

`repr_of_mono(Vector<Int>)` stays `TypedRef(Vector<Int>)` — the default/erased
answer. It must **not** globally return `TypedVec(I64)`; doing so recreates the
M1a bug. `PVecI64` comes **only** from site-aware sources:

- `route_typed_vec` (local slot `wasm_type` override), and
- typed-storage-site layout (record fields, variant payloads).

`backend/repr_policy.tw` exposes a **candidate** classifier only —
`candidate_typed_vec_family(Vector<Int>) = Some(I64)` — used by the site-aware
code to know *what the typed form would be*, never to decide *that a site is
typed*.

## Mechanism

Two separated decisions:

**WHICH typed family (candidate).** One classifier in `repr_policy.tw`. The three
`repr_of_mono` paths (`repr_of_mono`, `repr_of_named_cached`,
`cached_repr_of_mono` in `backend/repr_assign.tw`) route through it so they return
a **consistent default** (`TypedRef`) and cannot diverge again (the activation
exposed a divergence where the cached path was not flipped).

**WHERE it is used (actual).** A whole-program **typed-storage-site analysis**,
extending the proven S2.2 `analyze_typed_fields` to also cover
`(TypeId, VariantId, payloadIdx)` sum-payload sites. Local non-escaping slots keep
their existing `route_typed_vec` escape analysis. Eligibility for a typed site:

- every **producer** writes a typed-routable value (or a boxed value that is
  `unbox_i64`'d once at the store), and
- every **consumer** is either typed-compatible (direct typed read/`len`, or
  transfer into another typed site) **or** an explicit **one-time `box_i64`
  egress** to a boxed destination.

Forbidden (and prevented by escape analysis): `typed payload → boxed durable
boundary → repeated typed read-back`.

**How it reaches codegen.**
- `wasm_layout` emits `PVecI64` for a field/payload **only** where the analysis
  marked that site typed — record fields already (S2.2), variant payloads new
  (`layout_of_sum_def` gains the override record fields have).
  `val_type_of_mono(Vector<Int>)` stays `PVec` (default/erased ABI).
- **Implementation caution:** *every* variant constructor / extractor / helper /
  match-payload path must consult the typed-site layout. Any path that
  reconstructs payload valtypes from bare `val_type_of_mono(payload_mono)` will
  silently fall back to `PVec` and lose the typed payload.

**Safety by slot mismatch.** Because typed sites are `PVecI64` and everything else
is `PVec`, *every* value movement across a repr boundary is a `PVecI64`↔`PVec`
slot-type mismatch that `emit_coerce_stack` already resolves with **one**
`box_i64`/`unbox_i64` — never per read. `unbox_i64`/`box_i64` are the existing
adapters; no per-read cost is introduced because nothing typed ever lands in the
`anyref` env (escape analysis boxes escaping locals).

## Staging (each step self-host-green)

1. **Classifier unification.** Route the three `repr_of_mono` vector arms through
   `repr_policy` (all return `TypedRef`; add `candidate_typed_vec_family`). Pure
   cleanup, inert, fixes the divergence.
2. **Extend the typed-storage analysis to variant payloads.** Generalize
   `analyze_typed_fields` to sum-payload sites with the amended eligibility.
3. **Typed variant payload layout.** `layout_of_sum_def` override + audit every
   constructor/extractor/helper/match path to consult it. Direct-read win lands
   here.
4. **Verify/complete coercion at payload boundaries.** Extract/store across the
   `PVecI64`↔`PVec` mismatch should already emit one adapter via
   `emit_coerce_stack`; add cases only where a path is missed.

## Test strategy (the guards are the deliverable)

- **Positive:** `examples/performance/sort-bench/typed_variant_column_probe.tw`
  direct reads emit `rt_arr__get_i64` (0 → N) via the typed payload.
- **Critical regression guard (the M1a tripwire):** a `Vector<Int>` extracted from
  a typed payload *and captured into a closure* emits **zero per-call
  `rt_arr__unbox_i64`** on the read path — build to WAT, grep the closure /
  trampoline body, assert no `unbox_i64` inside the comparator loop. Automated.
- **`order_by`:** `gather`/`take`/direct reads improve; `sort` unchanged (boxed
  comparator — M1b); **no hang/regression**, with a wall-clock ceiling so a
  pathology fails loudly instead of running for minutes.
- **Capture microbench** (`cap.tw`-style: 200k reads of a captured 5000-elem
  vector) stays in the low-ms range (was ~8100 ms on the M1a branch).
- Full self-host to fixed point + boot suite green.

## Non-goals

- **Typed closure environments (M1b).** The captured-comparator sort stays boxed
  in A. B adds closure captures as one more typed storage-site kind, building on
  A's rule — not a trampoline special-case.
- **Uniform typing.** `Vector<Int>` is not globally `PVecI64`; that is the
  reverted M1a approach.
- Other primitive families (`Float`/`Bool`/`Byte`) and typed `Dict`.

## Relationship to existing work

- **Extends** S2.2 typed record fields (`analyze_typed_fields`) to variant payloads.
- **Keeps** the conservative `route_typed_vec` local routing as-is.
- **Reuses** the T0–T3 infrastructure retained after the M1a revert: `PVecI64` +
  `_i64` helpers, `box_i64`/`unbox_i64`, `ReprKind.TypedVec`, `emit_coerce_stack`
  arms, verifier support.
- **Feeds** M1b (typed closure envs), which the design leaves as the next
  milestone.
