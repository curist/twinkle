# Typed Vector Record Fields — Design

**Status:** design approved, pre-implementation
**Date:** 2026-06-10
**Branch:** `native-typed-value-sort`
**Parent plan:** [typed-vector-representation.md](typed-vector-representation.md) (representation flow target: "record fields containing typed vectors")
**Builds on:** S2.1 boundary boxing for return + direct-call args (commit `e59b873`)

## Goal

Let a `Vector<Int>` survive in its typed `PVecI64` physical representation when it
is stored in a record field, so that reads through the field (`r.col[i]`,
`r.col.len()`) hit raw `i64` leaves instead of the boxed `PVec` pointer-chase.

This is the next representation boundary after S2.1. Today, storing a typed
vector into a record field counts as an escape, forcing a fall back to boxed
`PVec` (`route_typed_vec.tw:368` `ARecord`, `:377` `ARecordGet`). Records are the
carrier for dataframe columns, so this is the infrastructure step toward fast
column reads.

## Scope

- **Element type:** `Vector<Int>` only (mono key `vec_i64`), consistent with S2.1.
  Other element types are unchanged.
- **Records only.** Variant-held columns are a separate boundary and a follow-up,
  even though the dataframe also uses variants.
- **Decision unit:** one boolean per `(TypeId, FieldId)` whose field mono type is
  `Vector<Int>`. Typed ⇒ that struct slot is physically `PVecI64`; otherwise the
  field is unchanged (`PVec`).

### Explicit non-goals

- No typed combinators (`map`/`filter`/`gather`) — that is Phase 5 of the parent
  plan. See the producer-dependency caveat below.
- No chained typed records (vector flowing `local → field → local → field → …`).
  That is the general fixpoint (Approach A), deferred.
- No variant payloads, closures, or cross-module typed ABIs.
- No new user-visible syntax. The decision is entirely inferred.

## Decision policy (conservative inference, gated)

A field `(R, f)` is physically `PVecI64` **iff** it has at least one producer and
**every** producer and **every** consumer across the whole program is
typed-friendly. Any incompatible site anywhere demotes the field to boxed `PVec`.
No representation-boundary coercions are ever inserted for fields: a field is
**fully typed or fully boxed**.

This is deliberate. `box_i64` (the `PVecI64 → PVec` adapter, `arr.tw:2081`) is
**O(n)** — it re-boxes every element and rebuilds a `PVec`. So a field that
sometimes carries typed and sometimes boxed values would pay an O(n) copy at
every mismatched site. Fully-typed-or-fully-boxed avoids that entirely.

**Producer-dependency caveat (honest expectation):** dataframe columns are
typically built by combinators (`map`/`filter`/`gather`), which still emit boxed
`PVec` until typed combinators land. Such fields will stay boxed under this
policy, so the immediate dataframe `order_by` win is gated on Phase 5. This step
is the *infrastructure* that pays off once producers are typed; it also
immediately covers any column built by a typed `collect`.

## Approach: conservative single-direction (no fixpoint)

The record field is the **only** cross-function carrier of the typed vector. The
vector never has to flow typed through any other boundary, so we reuse the
existing intra-function routing classification almost verbatim and avoid a
fixpoint.

A field `(R, f)` is typed when:

- **every** construction `R.{ f: v, … }` (`ARecord`) and **every** update setting
  `f` (`ARecordUpdate`) has `v` produced by an intra-function **typed-routable**
  expression — the typed `collect`/builder result that the existing routing
  already recognizes, in the same function as the construction/update; **and**
- **every** read `r.f` (`ARecordGet`) has its result local consumed **locally**
  only by `xs[i]` (`AIndex .Array`) and `.len()` — the same uses S2.1 already
  routes.

Producers feeding the field from a function parameter or a call result are
**not** typed-routable (conservative) and demote the field.

The general fixpoint form (Approach A — mutually-recursive local/field typedness,
chained records) is documented as the future graduation path but is out of scope.

## Architecture

Four pieces. Three are small; the routing extension reuses existing machinery.

### 1. Whole-program analysis pass (`analyze_typed_fields`)

New function, run inside `prepare_backend` after `assign_repr_for_module` and
before `route_typed_vectors`:

```
analyze_typed_fields(funcs: Vector<PreparedFunc>, env, builtins)
  -> Dict<String, Bool>   // key "${tid.id}:${fid.id}", present+true ⇒ typed
```

1. **Collect sites** across all funcs, per candidate `(R, f)` whose field mono is
   `Vector<Int>`:
   - *Producers:* `ARecord` field values and `ARecordUpdate` values setting `f`.
   - *Consumers:* every `ARecordGet(_, f, R)` result.
2. **Classify each site** with the existing intra-function logic:
   - producer typed-friendly ⇔ value atom is a local that the per-function
     routing would mark typed (typed `collect`/builder result in that function);
   - consumer typed-friendly ⇔ result local used only by `AIndex .Array` / `len`
     locally.
3. **AND-fold:** `(R, f)` is typed iff it has ≥1 producer and every producer and
   every consumer is typed-friendly.

The producer/consumer classifiers share code with `route_typed_vec.tw` (the
candidate detection and the `v_group_escapes` use-shape check). Factor the
shared shape predicates so the two passes can't drift.

### 2. Routing extension (`route_typed_vec.tw`)

`route_typed_vectors` takes the typed-field set. Given it:

- **Escape analysis:** `ARecord`/`ARecordUpdate` storing `v` into a *typed* field
  is no longer an escape (it is a typed sink). Storing into a *boxed* field stays
  an escape (current behavior).
- **New typed source:** `ARecordGet` from a *typed* field becomes a candidate,
  like `builder_freeze`. Its result slot retypes to `PVecI64`, and downstream
  `xs[i]` / `.len()` route to the `_i64` ops.
- Construction needs no op rewrite: the value local is already typed, and the
  struct slot now matches.

### 3. Representation plumbing (the single override point)

The decision reaches `layout_of` through `env`, which is already a parameter of
`layout_of` and its ~30 call sites — so no signature churn.

- **Carrier:** add to `ResolvedEnv` (`resolver.tw:110`):
  ```
  typed_vector_fields: Dict<String, Bool>   // key "${tid.id}:${fid.id}"
  ```
  Default empty in `empty_env()`; add a `with_typed_vector_fields` rebinder.
  (Mild layering note: a backend decision riding in a resolver record; accepted
  because the alternative is threading a new param through every `layout_of`
  caller.)
- **Surface:** add `typed_vector_fields` to `PreparedModule` (`prepare.tw:33`);
  `prepare_backend` returns the set computed in piece 1.
- **Enrich:** in `codegen.tw`, build `env2 := env.with_typed_vector_fields(
  prepared.typed_vector_fields)` and pass `env2` (not `env`) to
  `verify_prepared_module_with_level`, `plan_wasm_types`, and `emit_module`, so
  the type definition, every `struct.new`, every `struct.get`, and verification
  all read one source of truth.
- **Consume:** single conditional in `layout_of_named`'s field loop
  (`wasm_layout.tw:212-219`):
  ```
  val_type: if env.typed_vector_fields.has("${tid.id}:${i}")
              and field_ty is Vector<Int>
              { pvec_i64_ref() }   // .Ref(true, .Named("rt_types__PVecI64"))
            else { val_type_of_mono(field_ty, env) }
  ```

**Ordering:** `prepare_backend`'s internal passes (`insert_boundaries`,
`assign_repr_for_module`) run on the un-enriched `env`. That is correct: in this
approach a typed field carries a fully-typed value with no coercion; record
locals get `TypedRef`-by-struct-name, and field `val_type`s only matter at
type-def / `struct.new` / `struct.get`, which are all in plan+emit under `env2`.
No chicken-and-egg.

### 4. `box_i64` reuse

No new runtime ops. The existing `PVecI64` family (`len_i64`, `get_i64`,
builder `_i64`, `box_i64`) covers typed field reads. `box_i64` is only used at
genuine S2.1 boundaries (e.g. a single return), never at a typed-field store or
load.

## Blast radius

- `resolver.tw`: one new `ResolvedEnv` field + `empty_env` default + a
  `with_typed_vector_fields` rebinder.
- `prepare.tw`: one new `PreparedModule` field; `prepare_backend` computes and
  returns the set.
- `route_typed_vec.tw`: new `analyze_typed_fields` (sharing shape predicates);
  `route_typed_vectors` gains the set param and the `ARecord*`/`ARecordGet`
  escape/source cases.
- `codegen.tw`: three call-site swaps to `env2`.
- `wasm_layout.tw`: one conditional in `layout_of_named`'s field loop.
- The ~30 other `layout_of` callers are untouched.

## Verification & testing

- **Positive probe** (permanent, `examples/sort-bench/`): a record-held `Int`
  column built by `collect` in a constructor function, then read+indexed in a
  separate function. Assert the emitted WAT shows `rt_arr__get_i64` on the field
  read and a `PVecI64` struct field type, and **no** `box_i64` at the field
  store/load.
- **Negative probe:** a record whose field has one boxed producer (value from a
  parameter or a combinator) ⇒ field stays `PVec` everywhere, no `_i64` on its
  reads.
- **Verifier:** prepared-IR verification under `env2` must accept a typed value
  stored into a typed field slot and reject a representation mismatch (typed
  value into a boxed field slot, or vice versa).
- **Suite:** full `make boot-test` green; `make stage2` self-host fixed point
  holds. No regression in dataframe `filter`/`join`/`group_by` (parent-plan
  guardrail).
- **Stage0 parity:** not required for this boot-codegen optimization (per the
  no-stage0-parity rule for backend-only typed-vector opts).

## Risks and open questions

- **Classifier drift:** the field analysis and `route_typed_vec` must agree on
  what "typed-routable producer" and "typed-friendly consumer" mean. Mitigation:
  share the shape predicates between the two passes.
- **Verifier representation rules:** confirm the prepared-IR verifier expresses
  "this struct field is `PVecI64`" so a typed store/load type-checks. May need a
  small verifier extension paralleling the slot-level typed-vector handling
  (`verify_slots.tw:184`).
- **`ARecordUpdate` copy semantics:** updating a *different* field of a record
  whose typed field is copied through must carry the `PVecI64` slot intact
  (struct.get/struct.new of the typed field). Confirm the update lowering copies
  the field by its struct type, not a re-derived boxed type.
- **Caching:** `cached_layout_of` keys on `mono_to_key(mono)`. The same record
  `MonoType` now yields a field-representation that depends on `env`, not just
  the mono. The cache key must stay valid because the typed-field set is fixed
  for the whole emit (`env2` is constant across plan+emit), so per-emit caching
  is sound; verify no cache instance is shared across compilations with a
  different set.
- **Limited immediate payoff:** until typed combinators (Phase 5), most real
  dataframe columns stay boxed. This step is justified as infrastructure plus the
  typed-`collect` column case, not as an immediate `order_by` win.

## Graduation path (out of scope, for reference)

- Approach A fixpoint: chained typed records and mutually-recursive local/field
  typedness.
- Typed combinators (Phase 5) so combinator-built columns become typed
  producers — this is what unlocks the dataframe `order_by` win.
- Variant-held typed payloads, for columns stored in sum types.
