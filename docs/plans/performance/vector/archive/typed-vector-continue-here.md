# Typed vectors — continue here (next-session handoff)

> ⚠️ **SUPERSEDED (2026-07-08).** This is a point-in-time handoff from 2026-07-05,
> before the cross-fn ABI + C2 work. For current status start with
> **[boundary-tracklist.md](../boundary-tracklist.md)** (the living per-boundary map)
> and the [README](../README.md). Since this was written: work moved to branch
> `typed-vector-crossfn-abi`; B2 (accessor returns), B3/B4 (copy propagation), B5
> (field-read copies), and **C2 (captured columns → the `order_by` sort win)** all
> landed. The "where things stand" and "first decision" below are stale; the
> **design model** section further down is still the valid reference.

**Start here to continue the typed-`Vector<T>` work.** Written 2026-07-05. Assumes
no prior session context.

## Where things stand

**On `main`** (S1–S2.2, verified): `Vector<Int>` is stored unboxed as `PVecI64`
(raw i64 leaves, ~8× faster reads than boxed) at *conservative* closed sites —
non-escaping locals (S2.0) and typed record fields (S2.2). Everything else stays
boxed `rt_types__PVec`. Infra: `ReprKind.TypedVec`, `repr_policy` candidate
classifier, `box_i64`/`unbox_i64` adapters, `emit_coerce_stack` (`PVecI64↔PVec`
both ways), the `route_typed_vec` whole-program analysis.

**On branch `typed-vector-repr-m1a` (NOT merged):**
- **Milestone A — typed user-variant payloads** (`IntCol(Vector<Int>)` → `PVecI64`
  in the variant struct). Complete, self-hosting, capture-safe, reviewed, and one
  critical review bug fixed (equality of typed payloads, `5451d45f`). Docs:
  [storage-site-typed-vectors.md](storage-site-typed-vectors.md),
  [plan](storage-site-typed-vectors-plan.md).
- Also on the branch: a **reverted** "uniform typing" attempt (kept in history as
  the lesson — see the model below).

**First decision for the next session:** merge the branch to `main` (it's green
and self-hosting) or keep working on it. The history is layered (infra → reverted
uniform activation → Milestone A → eq fix); consider a clean merge/squash.

## The design model (do not violate)

`PVecI64` is a **per-storage-site optimization**, never a global property of
`Vector<Int>`. Full statement + rationale:
[../representation-boundary-policy.md](../../representation-boundary-policy.md).

**The invariant that prevents catastrophe:** a `Vector<Int>` that escapes to a
durable erased boundary — `anyref`, the universal `ClosureEnv`, a generic
container, the erased `Variant` — must be **boxed** (boxed once at the crossing,
read boxed). **A typed vector must never land in the `anyref` closure env and be
read in a loop** — that is the O(n)-per-read pathology that got uniform typing
reverted (post-mortem:
[m1a-anyref-readback-investigation.md](m1a-anyref-readback-investigation.md)).
The safety mechanism is `route_typed_vec`'s escape guard
(`result_consumed_typed_only`): a captured/escaping vector is left boxed.

## The real limitation (why `order_by` didn't move)

The typed machinery is *sound* but *low-coverage*. Two boundaries box typed
vectors, so real multi-function code rarely stays typed:

1. **Function-call ABI.** A typed `PVecI64` boxes at every call — S2.1 only added
   return/direct-arg *boxing* adapters, not typed cross-function ABIs. The
   dataframe's `IntCol` columns are built/passed across functions, so they box and
   Milestone A never types them → `order_by` unchanged (~2403ms).
2. **Closure capture.** `sort_by(fn(a,b){ keys[a] … })` captures the key column
   into the `anyref` `ClosureEnv` → boxed reads in the comparator.

## Next steps (prioritized)

### 1. Cross-function monomorphic typed vector ABIs — the coverage frontier (do first)

Let a monomorphized `fn(xs: Vector<Int>) …` physically take/return `PVecI64` when
the instance is repr-consistent, so typed vectors flow through calls (column
builders, `gather`, the comparator) without boxing at every boundary. This is the
biggest lever: it's what makes the typed machinery actually *fire* on real code,
and it's the prerequisite for step 2 (a captured column can't be typed if it boxed
crossing into the query functions first).

- **First, instrument WHY** the concrete dataframe `IntCol` column boxes — confirm
  it's the call ABI (build `examples/performance/dataframe/bench/main.tw` to WAT,
  trace a column from `collect` to `IntCol` construction; look for `box_i64` at a
  call boundary). Don't design blind.
- **The design fork** (brainstorm this): *specialize-by-representation*
  (monomorphization emits a `PVecI64`-ABI instance) vs *adapt-at-call* (box/unbox
  at mismatched call sites). The umbrella's "Representation-boundary policy"
  section frames it: [typed-vector-representation.md](../typed-vector-representation.md).
- **Lower risk than step 2** (extends monomorphization + repr, no new subsystem).
  Already wins the direct-read paths (`gather`/`take`, ~830ms of `order_by`).

### 2. M1b — typed closure environments (the `sort_by` comparator)

Give closures a per-capture-repr env layout so a captured `Vector<Int>` is stored
`PVecI64` in the closure object and read typed by the trampoline — no `anyref`
round-trip. Today `ClosureEnv = array anyref` and closure layout is keyed by
*function signature*, which cannot express typed captures (two closures with the
same signature can capture different reprs), so this needs a real env-layout
change keyed by capture reprs, threaded from `closure_convert`. This is the
riskiest piece and the other half of the `order_by` sort (~1317ms). It needs step
1 first (nothing typed to capture otherwise).

## Gotchas for whoever continues (learned the hard way)

- **Any new typed-storage site needs the equality path.** `field_eq_instrs`
  (`codegen/runtime/core.tw`) drives both sum and record equality; a `PVecI64`
  field/payload must be `box_i64`'d before generic `eq` (fixed in `5451d45f`).
  **Rule:** any generated code that hands a physical vector to a generic `anyref`
  helper (`eq`, and audit stringify / hashing / dict-key paths) must box a
  `PVecI64` first — generic helpers only understand boxed `PVec`.
- **Builtin sums are excluded** from typed payloads (`base_env.first_user_type_id()`
  threshold) because `Option`/`Result`/`Iterator`/… route their payloads through
  *specialized* erased bridge helpers that don't box `PVecI64` (only the generic
  `bridge_funcs.tw` bridge does). If you broaden typing, keep this exclusion or
  extend those helpers.
- **`candidate ≠ actual`:** `repr_of_mono(Vector<Int>)` must stay `TypedRef`
  (default/erased). `PVecI64` comes only from site-aware sources (route_typed_vec
  slots, typed-storage layout). Never make the classifier globally return typed —
  that recreates the reverted bug.
- **Coercions are cheap and correct at boundaries** (`emit_coerce_stack` does
  `box_i64`/`unbox_i64` once per crossing). The danger is never *a* coercion; it's
  a coercion **per read** (the anyref-env-in-a-loop case), which the escape guard
  prevents.

## Verify the branch is healthy before building on it

```bash
make bundle-cli        # "Fixed point reached" (if it says "Nothing to be done",
                       #  touch a boot source file — stale timestamps)
make boot-test         # all green (2967 at handoff)
target/twk build examples/performance/sort-bench/typed_variant_payload_probe.tw -o /tmp/p.wat
grep -c rt_arr__get_i64 /tmp/p.wat                                   # >= 1 (typed)
timeout 15 target/twk run examples/performance/sort-bench/typed_payload_capture_guard.tw  # ~5ms, not seconds
```

## Doc map

- **This file** — actionable next steps.
- [typed-vector-representation.md](../typed-vector-representation.md) — the live
  umbrella (per-phase status + open next).
- [../representation-boundary-policy.md](../../representation-boundary-policy.md) —
  the storage-site model + the reverted uniform-typing lesson.
- [generic-sort-by-vector-read-perf.md](../generic-sort-by-vector-read-perf.md) —
  the read-wall measurement/decomposition (still the reference for `order_by`).
- Milestone A: [design](storage-site-typed-vectors.md) /
  [plan](storage-site-typed-vectors-plan.md).
