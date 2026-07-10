# Scope: routing typed vectors at top level (module-global producers)

**Date:** 2026-07-09
**Status:** scoping — not started
**Context:** follow-up surfaced by the typed `Vector.make` + Bool parity work
(`project_typed_vector_repr` memory, design `typed-vector-make-and-bool-parity-design.md`).

## Problem

A typed-vector producer written at **top level** stays physically boxed, even
when every use is typed-friendly:

```tw
xs := collect i in range(8) { i }   // stays boxed PVec
println(xs[0].to_string())          // boxed rt_arr__get, not get_i64
```

The identical shape **inside a function** routes to `PVecI64`/`get_i64`. This is
family-neutral: the `Vector<Int>` and `Vector<Bool>` cases behave identically
(the Int top-level analogue is also boxed). It is not a Bool bug and not a
`Vector.make` bug.

## Root cause (verified)

Top-level bindings are **module globals**, not `Let`-bound local slots. A
top-level `xs := producer` compiles to:

- a mutable module global `$user__global_local_N` whose wasm type is
  `(ref null $rt_types__PVec)` — the **boxed** PVec, derived in
  `emit_module_globals` from `val_type_of_mono(gentry.mono, env)`;
- an `AGlobalSet` storing the producer result into that global;
- `AGlobalLocal` reads at each use.

Typed-vector routing (`route_typed_vec.tw`) operates on `Let`-bound local slots
and treats a store into a module global as a **durable erased boundary**:

```
// classify_expr / op_group_escapes
.AGlobalSet(_, a) => slot_in(a, vs)   // storing a v-group slot into a global == escape
```

So the collect/`make` freeze result (a local in the init func) escapes at the
`AGlobalSet`, its group is not typeable, and it demotes to boxed. Reads then load
a boxed `PVec` global and use boxed `get`. This is the representation-boundary
policy working as designed — a boxed global is a durable erased boundary, exactly
like an un-typed record field.

**Conclusion:** "route top level" == "give module globals a typed-PVec
representation when whole-program-safe." This is directly analogous to the
existing `typed_fields` / `typed_payloads` / typed-capture / typed-return
machinery, but keyed by `GlobalId`.

## Why this is the correct framing

The typed-vector system already types every *other* durable boundary through a
whole-program analysis that a producer/consumer must both clear:

- record fields → `analyze_typed_fields` → `typed_fields`
- variant payloads → `analyze_typed_payloads` → `typed_payloads`
- captured free vars → capture fixpoint → `capture_abi`
- function returns → `typeable_return`

Module globals are the one durable boundary with no such analysis. Adding
`typed_globals` completes the set.

## Options

### Option A — typed module-global ABI (principled, general)

Add a whole-program per-`GlobalId` typedness analysis and route through it.

A global `G` is typed in family `F` iff, across **all** functions:
- every `AGlobalSet(G, a)` stores a clean typed producer for `F` (collect/make
  freeze result, typed field/payload read, typed call result, or another typed
  global read); and
- every `AGlobalLocal(G)` consumer is typed-friendly for `F` (index/len/gather,
  typed field/payload store, typed return, typed capture); and
- no durable-erased use of `G` (boxed param, boxed field/payload store, boxed
  return, boxed global store).

Because a typed global read can feed a typed field/payload producer and vice
versa, `typed_globals` is **mutually recursive** with `typed_fields`/
`typed_payloads` and must join the existing joint greatest-fixpoint in
`analyze_typed_repr` (not run as a separate pass).

Touch points:
1. **Analysis** (`typed_param_abi.tw` / `route_typed_vec.tw`): `analyze_typed_globals`
   → `typed_globals: Dict<String, family_key>` (GlobalId-keyed), folded into the
   joint fixpoint; add global producer/consumer scans mirroring the field/payload
   scans.
2. **Producer routing** (`route_typed_vec.tw`): thread `typed_globals` into the
   classifier; `AGlobalSet(G, a)` is **not** an escape when `G ∈ typed_globals[F]`
   (a typed store); add an `AGlobalLocal(G)` read (`G ∈ typed_globals`) as a typed
   producer source, retyping its result slot to `PVecF` (mirrors typed-field-read
   case 2b and typed-return call results 2c'').
3. **Global repr** (`emit_module_globals`): when `G ∈ typed_globals[F]`, declare
   the global as `(mut (ref null PVecF))` instead of the mono-derived boxed PVec.
4. **Emit** (`AGlobalSet`/`AGlobalLocal` paths): store/load the typed global type;
   box/unbox at any residual boxed boundary (as typed fields already do).
5. **Verifier** (`verify_expr.tw`): a typed global store/load must have matching
   repr — add the edge alongside `verify_capture_store_repr` and the field-store
   repr check.

Mutability caveat (already handled in spirit): globals are `mut` and may be
reassigned (`xs = xs.append(...)`). The existing soundness rule — any boxed store
into a would-be-typed slot disqualifies it (`AAssign` line ~1190) — must extend to
`AGlobalSet`: every store site for `G` is considered, and any boxed store demotes
`G`.

Effort: comparable to adding `typed_payloads` (a new fixpoint member + classifier
threading + repr + one verifier edge + self-host). The infra to extend already
exists; no new concepts.

### Option B — keep init-only globals as init-local slots (pragmatic subset)

When a top-level binding's global is **only** written and read within the init
func (never read by another function), rewrite it to a `Let`-bound local in the
init func before routing. Existing routing then applies with **zero** new ABI.

- Covers the common case (top-level scratch producers used locally: benchmarks,
  probes, scripts).
- Does **not** help a global read by a user function (needs the typed-global ABI).
- Smaller: one pre-route transform in `prepare.tw` + reuse of existing routing;
  no repr/verifier changes for the local case.
- Requires a use/def scan to prove init-locality (a global read outside the init
  func disqualifies it).

### Option C — document only

Top level is not a hot path; real code (dataframe, leetcode, self-hosted
compiler) puts producers in functions. Record the gap and stop.

## Recommendation

If pursued, **Option A** — it is the principled fix, completes the durable-boundary
set, and extends established machinery rather than adding a special case. Option B
is a legitimate cheaper subset if only the local-scratch case matters, but it does
not generalize and adds a bespoke rewrite that A would later subsume.

However, this is **low priority**: top-level producers are not on any hot path,
and the representation-boundary policy is behaving correctly, not buggily. Suggest
Option C (this doc) as the default and Option A only if a concrete workload needs
top-level typed vectors.

## Non-goals

- Function/closure-valued module globals (module-global type tracking)
  are a separate concern; this touches only PVec-typed globals.
- No change to the per-function routing that already works.
- No new element families.

## Open questions

- Does any real workload benefit? (None known — decision input for A vs C.)
- Init-func ordering under SCC module groups: typed globals change repr, not init
  order, but confirm the typed empty-PVec default (`empty_pvec_i64`) is a valid
  global initializer in all module-group orderings.
