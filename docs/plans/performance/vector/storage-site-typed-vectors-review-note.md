# Review handoff — Milestone A: storage-site typed variant payloads

**For a reviewer with no prior session context.** This note describes what to
review, why it's shaped this way, and the exact soundness questions that matter.

## What this is

Branch `typed-vector-repr-m1a` (NOT merged to `main`). It extends the boot
compiler's conservative typed-`Vector<Int>` storage so that **user-defined
sum/variant payloads** of type `Vector<Int>` (e.g. `type Col = { IntCol(Vector<Int>) }`)
are stored as the unboxed typed representation `rt_types__PVecI64` in the variant
struct, instead of the boxed universal `rt_types__PVec`. Reads of such a payload
become the fast typed read (`rt_arr__get_i64`) instead of a boxed pointer-chase.

Design + plan: [storage-site-typed-vectors.md](storage-site-typed-vectors.md),
[storage-site-typed-vectors-plan.md](storage-site-typed-vectors-plan.md).
Policy/model: [../representation-boundary-policy.md](../representation-boundary-policy.md).

## Essential background: the failure this avoids

A prior attempt on this same branch made `Vector<Int>` physically `PVecI64`
**everywhere** ("uniform typing", commit `f26cd7a3`). It self-hosted and passed
tests but was **catastrophically slow**: a captured `Vector<Int>` was boxed into
the `anyref` closure env and then `unbox_i64`'d (a full O(n) rebuild) on **every
read**, so a captured-vector read loop went from ~ms to ~8 seconds; `order_by`
hung. It was **reverted** (`66f90d21`). Post-mortem:
[m1a-anyref-readback-investigation.md](m1a-anyref-readback-investigation.md).

**The load-bearing invariant of the corrected design (verify it holds):**
`PVecI64` is a **per-storage-site** representation, never a global property of the
value. A `Vector<Int>` is typed only at a *closed* site (non-escaping local, typed
record field, typed variant payload). Anything that **escapes to a durable erased
boundary** — `anyref`, the universal `ClosureEnv`, generic containers, the erased
`Variant` — stays boxed `PVec`, boxed **once** at the crossing, read boxed. **No
typed vector may ever land in the `anyref` closure env** — that's the pathology.

## Commit trail to review (on top of `main`)

Retained infrastructure (built earlier, kept after the uniform-typing revert):
- `84d9ad84` repr_policy module (candidate classifier)
- `fd19e3f8` `ReprKind.TypedVec(ElemRepr)`
- `46dd30aa` `unbox_i64` runtime adapter (`PVec → PVecI64`)
- `325755bd` `emit_coerce_stack` handles `PVecI64 ↔ PVec` both ways

Milestone A (the work to review), built **build-then-activate** so each step is
inert until the last:
- `4a5bc161` **T1** unify the 3 `repr_of_mono` vector arms behind one
  `vector_default_repr` (all still return `TypedRef`); rename classifier to
  `candidate_typed_vec_family`. *Guards against the divergence that caused the
  original bug. No behavior change.*
- `605a710d` **T2** add `typed_vector_payloads: Dict<String,Bool>` plumbing
  (`ResolvedEnv` → `PreparedModule` → env → layout), empty/inert; fix test fixtures.
- `d432f8f1` **T3** `layout_of_sum_def` emits `PVecI64` for a payload when the site
  key is present **and** the field is `Vector<Int>` (guarded, inert while empty).
- `c78bf6ae` **T4a** `emit_pattern_bindings` coerces an extracted payload to the
  bound slot's type (inert: no-op when both are `PVec`).
- `d2cc1cb8` **T4b** the erased-Variant bridge (`emit_sum_to_variant_helper` /
  `emit_variant_to_sum_helper`) `box_i64`/`unbox_i64`s `PVecI64` payloads (inert).
- `57f603cf` **T5** route non-escaping match-payload reads to `PVecI64`; **escaping
  ones stay boxed** (escape guard `result_consumed_typed_only`). Inert (empty set).
- `0168753a` **T6 ACTIVATION** extend the whole-program analysis
  (`analyze_typed_payloads`) to populate the dict; wire it to BOTH consumers
  (layout + routing). This is the integration point — everything above turns on here.

Key canonical detail: payload site key is `"${tid.id}:${vid.id}:${payloadIdx}"`,
used identically by the analysis (`route_typed_vec.tw`), the layout
(`wasm_layout.tw` `layout_of_sum_def`), and the read routing.

Files changed by T1–T6: `backend/{repr_policy,repr_assign,prepare,route_typed_vec}.tw`,
`codegen/{codegen,emit,wasm_layout}.tw`, `codegen/emit/bridge_funcs.tw`,
`base_env.tw`, `resolver.tw`, and several test suites.

## How to verify (gates that must pass)

```bash
make bundle-cli        # must print "Fixed point reached" (self-host)
make boot-test         # must be all green (2966 at time of writing)

# Positive: a variant-only column reads typed (no bare typed local to mask it)
target/twk build examples/performance/sort-bench/typed_variant_payload_probe.tw -o /tmp/p.wat
grep -c rt_arr__get_i64 /tmp/p.wat     # >= 1

# THE regression tripwire: a captured payload read loop must stay fast (boxed),
# NOT rebuild per read. ~5ms here; the reverted uniform build was ~8100ms.
timeout 15 target/twk run examples/performance/sort-bench/typed_payload_capture_guard.tw
#   prints elapsed well under 200ms, exits 0

timeout 60 target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw   # completes, no hang
```

## Soundness questions to focus the review on

1. **Escape/capture safety (the pathology).** Confirm from the code that a
   `Vector<Int>` extracted from a typed payload and **captured by a closure** is
   NOT retyped to `PVecI64` — the read routing (`result_consumed_typed_only` in
   `route_typed_vec.tw`, gating the `payload_read_sids` retype loop) must keep it
   boxed. Typing a payload *site* must not force the extracted *slot* typed.
2. **Builtin exclusion.** The analysis must type ONLY user-defined sums, excluding
   `Option`/`Result`/`Cell`/`Range`/`Iterator`/etc. — their `Vector<Int>` payloads
   route through *specialized* erased helpers (`emit_option_from_variant_helper`,
   `emit_iterator_next_helper`, …) that do NOT box/unbox `PVecI64` (only the
   generic bridge in `bridge_funcs.tw` does). Exclusion is via
   `base_env.first_user_type_id()` + `user_payload_keys` (parses the leading TypeId
   from each key, drops any below the threshold). Verify no builtin sum has a
   TypeId ≥ the threshold, and that every candidate key passes through the filter.
3. **Every payload boundary coerces.** Construction (`emit_variant_literal`,
   already coerces to `payload_types[i]`), extraction (T4a), the erased bridge
   (T4b), and any other variant payload emit path must derive payload wasm types
   from the **layout** (`payload_types`), never from bare
   `val_type_of_mono(payload_mono)` (which would silently fall back to `PVec`).
4. **Both consumers agree.** `prepare.tw` must pass the *same* `analyze_typed_payloads`
   result to both `PreparedModule.typed_vector_payloads` (→ layout) and the
   `route_typed_vectors` argument (→ routing). If they diverge, a typed field could
   meet a boxed-expecting read.
5. **"Never mark bad_consumer" decision.** T6 types a site purely on having a clean
   typed producer; consumers are never disqualifying (variants are immutable, so no
   store-back). Check there is no consumer path that reads a payload without going
   through a coercion.

## Outcome & known limitation (not a bug)

- Mechanism is correct and **capture-safe** (tripwire 5.34ms). Self-host + 2966
  tests green.
- **`order_by` is unchanged** (~2403ms). The producer eligibility is *conservative*
  ("a clean `collect`-built typed vector placed directly into a variant
  construction") and does **not** match how the real dataframe builds its `IntCol`
  columns (cross-function / combinator-fed / parameter-passed), so the real columns
  stay boxed. This is an accepted, intended limitation of the first cut — the
  app-level win needs broader producer eligibility (cross-function typed ABIs) and,
  for the sort, M1b typed closure envs. See the umbrella
  [typed-vector-representation.md](typed-vector-representation.md) "open next".

## What a review should conclude

Whether the change is **sound** (cannot reintroduce the per-read `unbox_i64`
pathology; cannot miscompile a payload; builtin sums are safely excluded) and
**clean** — not whether it speeds up the dataframe (it intentionally doesn't yet).
