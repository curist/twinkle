# Wasm Type Erasure Reduction

**Goal:** Drive the Wasm backend toward monomorphized helper emission and concrete runtime
layouts for monomorphized Twinkle code, so concrete values do not lose type information
when they move through records, iterators, and other runtime helper structures.

This is a follow-up plan, not an extension of Stage 9.6 itself. Stage 9.6 focuses on
typed higher-order parameter calls. This plan tracks the remaining places where the
backend still falls back to universal `Anyref`-heavy layouts even when concrete Wasm
types are available.

For the monomorphization guarantees this plan relies on, and for the distinction between
“concrete IR type” and “concrete Wasm runtime layout,” see
[../../internals/monomorphization.md](../../internals/monomorphization.md).

## Intended End State

The intended direction is stronger than “reduce `Anyref` a bit.”

For concrete, monomorphized programs, the backend should prefer:

* concrete Wasm layouts for concrete Twinkle types
* monomorphized helper emission for hot concrete helper paths
* typed helper dispatch over universal helper dispatch

`Anyref` should remain only as the fallback for cases that are genuinely erased,
existential, or otherwise not worth specializing.

In other words:

* monomorphization should ensure codegen sees concrete types
* the backend should then exploit that by emitting concrete layouts and helper families
* universal `Anyref`-based helpers should be the fallback path, not the default path

This plan therefore assumes:

* monomorphization is the prerequisite that makes concrete helper/layout emission possible
* backend erasure reduction is the follow-up that actually exploits that information
* the backend pipeline alignment from
  [backend-pipeline-alignment.md](backend-pipeline-alignment.md) is now in place

## Status

Done so far:

* named function values now participate in typed closure specialization in the normal
  build path
* typed closures now use a stronger Wasm representation: typed closure structs are
  subtypes of the universal `$Closure`, so the same runtime value can flow through both
  universal and typed call paths
* `Cell.update` has a real Wasm implementation instead of trapping
* concrete `Cell<T>` instantiations now use typed Wasm cell layouts, with the old erased
  `anyref` container retained only as a fallback path
* concrete user-record fields now preserve their Wasm field types, including typed closure
  refs in capability-style records and raw scalar fields in plain data records
* concrete `Iterator.unfold` producers can now route `Iterator.next` through typed helper
  dispatch when the compiler can still prove the hidden seed/step state
* concrete `Option<T>` values created and matched locally use typed structs with unboxed
  payloads, with automatic erased conversion at function boundaries

These items build on top of the completed
[backend-pipeline-alignment.md](backend-pipeline-alignment.md) work: backend-facing ANF is
monomorphized before optimization/codegen, while the interpreter remains a separate Core IR
path.

---

## Motivation

The typed-closure work removed one important source of overhead: boxing at concrete
higher-order call boundaries such as `fold(xs, init, f)`.

However, a WAT audit of `examples/*` and `tests/run/*` still shows several broader
type-erasure patterns:

* iterator state is still represented as a generic `[seed_anyref, step_closure_anyref]` array
* iterator helper paths still erase seed and payload values even when the step dispatch itself
  can now specialize
* iterator / helper code still uses erased payloads more often than necessary

The result is that some hot paths still allocate argument arrays, cast through
`$rt_types__Closure`, or box concrete payloads even though the program is fully
monomorphized and should be eligible for a monomorphized helper/layout path.

---

## Confirmed Gaps

### 1. Named function values

Status: mostly done.

Example:

```tw
fn apply<A, B>(f: fn(A) B, x: A) B { f(x) }
println("${apply(double, 42)}")
```

This used to fall back to universal closure dispatch. The build path now specializes
plain named-function values in first-class positions as well.

Follow-up:

* keep regression coverage for named-function specialization
* make sure new closure representation changes do not accidentally drop this path

### 2. `Cell<T>` layout specialization

Status: mostly done.

Concrete `Cell<T>` instantiations now lower to typed Wasm structs, which means `Cell<Int>`
and `Cell<fn(...) ...>` can preserve their concrete payload type through `new`, `get`,
`set`, and `update`.

What changed:

* concrete payload cells no longer default to a 1-slot `anyref` runtime container
* typed closure payloads can stay concrete inside cells
* flow-sensitive local mono tracking keeps the typed path available even in `__init__`
  functions where module globals may be rebound to later values

Follow-up:

* keep regression coverage for concrete `Cell<Int>` and `Cell<fn(...) ...>` Wasm paths
* keep the erased fallback path only for genuinely unsupported or erased cases

### 3. User record field specialization

Status: mostly done.

Concrete record fields now lower to field-specific Wasm types instead of unconditional
`Anyref`.

What changed:

* scalar fields like `Int` / `Bool` / `Float` stay unboxed in record structs
* function-valued record fields preserve typed closure refs on the typed emitter path
* record literal / get / update lowering now uses the actual field Wasm type
* range constructor lowering was updated to match the typed record layout

Follow-up:

* keep regression coverage for scalar and capability-style records
* preserve `Anyref` only for genuinely erased or polymorphic record fields

### 4. Iterator representation and payload layout

Status: done for the concrete iterator path; universal fallback retained for erased cases.

What changed:

* concrete `Iterator.unfold(seed, step)` now emits a typed iterator-state struct instead of
  routing seed/step through the universal runtime container
* concrete `Iterator.next` now routes through a typed helper that:
  * calls the step function via concrete typed-closure `call_ref`
  * reads typed `UnfoldStep<T, S>` fields directly
  * builds typed `IterItem<T>` records directly
  * returns a typed iterator-adjacent `Option<IterItem<T>>` struct instead of a universal
    `Variant`
* match / `for` lowering on that concrete path now reads the typed `Option` / `IterItem`
  fields directly

What remains intentionally unchanged:

* genuinely erased `Iterator<T>` values, such as iterator parameters with no provable
  hidden state shape, still use the universal `__iterator_next` helper and the
  `Variant`/payload-array fallback path

### 5. Variant / helper payloads outside iterators

Iterator-adjacent hot paths now have typed payload layouts, but general `Option`/`Result`
and other helper payloads still often travel through universal payload arrays.

Target:

* audit which non-iterator variant paths are hot enough to justify typed payload structs
  or monomorphized helper families
* avoid broad refactors where the payoff is too small

## Proposed Work Items

### A. Stabilize closure-adjacent specialization ✅

* Named-function specialization coverage is in place.
* Typed-closure subtype layout has Wasm regression coverage.

### B. Typed iterator pipeline

Replace the erased iterator representation with concrete types end-to-end for
monomorphized iterator paths. This is broken into incremental steps below.

Each step should keep the universal fallback path working for genuinely erased cases
(e.g. `Iterator<T>` received as a function parameter with no provable concrete state).

#### B1. Typed iterator state struct

Replace the erased `array<anyref>` iterator container with a typed struct per concrete
`(seed_ty, step_closure_ty)` pair.

Current: `Iterator.unfold(seed, step)` emits `ArrayNewFixed([seed_anyref, step_anyref])`.

Target: emit a typed struct like `$iter_state__i64__closure_i64_unfoldstep` with fields
`(seed: i64, step: ref $typed_closure)` for concrete cases.

What changes:

* `emit_iterator_unfold_intrinsic`: emit typed struct instead of erased array when
  `IteratorStateInfo` is available with concrete types
* emit the struct type definitions for each concrete iterator state
* `emit_typed_iterator_next_helper`: read seed/step from typed struct fields instead of
  `ArrayGet` + cast — the seed comes out at its concrete Wasm type, no unboxing needed
* keep the universal path (`ArrayNewFixed` + `__iterator_next`) for non-concrete cases

#### B2. Typed UnfoldStep payload

Replace erased `Variant(type_id, variant_id, [value_anyref, next_seed_anyref])` with a
typed variant struct for concrete `UnfoldStep<T, S>` instantiations.

Current: `UnfoldStep.Yield(value, next_seed)` packs both into `array<anyref>`, boxes
scalars. The iterator next helper then extracts them via `ArrayGet` + cast.

Target: emit a typed struct like `$unfold_step__i64__i64` with fields
`(variant_id: i32, f0: i64, f1: i64)` so payloads stay unboxed.

What changes:

* `emit_variant_literal`: detect `UnfoldStep` type_id with concrete args, emit typed
  struct instead of universal `$Variant`
* emit typed `UnfoldStep` struct type definitions
* `emit_typed_iterator_next_helper`: read Yield fields from typed struct fields instead
  of `ArrayGet` on erased payload array
* pattern matching on `UnfoldStep` variants: extract fields from typed struct

#### B3. Typed IterItem record fields

Replace the anyref-field `IterItem { value: anyref, rest: anyref }` record with concrete
field types.

Current: `UserRecord_5` has two `(mut anyref)` fields. The `value` field always erases
the yielded item type; the `rest` field always erases the next iterator state.

Target: for concrete `IterItem<Int>`, emit `(field $f0 (mut i64))` for value and
`(field $f1 (mut (ref null $iter_state__i64__...)))` for rest.

What changes:

* `IterItem` struct emission should specialize field types when the yield type and
  iterator state type are concrete
* the typed iterator next helper should construct the specialized `IterItem` struct
* `for x in iter` desugaring should read value/rest from typed fields

#### B4. Typed Option wrapping for iterator results

Replace erased `Option.Some(iter_item)` with a typed variant struct for concrete
`Option<IterItem<T>>` in iterator next return position.

Current: `Option.Some(item)` wraps the `IterItem` ref into `Variant(OPTION_TYPE_ID, 1,
[item_anyref])`. `Option.None` is `Variant(OPTION_TYPE_ID, 0, [])`.

Target: emit a typed struct like `$option__iteritem_i64` with fields
`(variant_id: i32, payload: ref null $IterItem_typed)` so the `IterItem` ref stays
concrete through the `Option` wrapper.

What changes:

* typed iterator next helper returns typed Option struct instead of universal Variant
* `for x in iter` desugaring: match on typed Option struct to extract IterItem
* this is scoped to the iterator-adjacent Option path; general Option specialization
  is out of scope for now

### C. General variant payload specialization

Status: done — `Option<T>` and `Result<T, E>` local specialization is implemented.

Concrete `Option<T>` values created and consumed within a function body now use a typed
struct (e.g. `$option__Int` with fields `(variant_id: i32, payload: i64)`) instead of the
universal `$Variant + payload array + BoxedInt` path.

What changed:

* `emit_variant_literal` intercepts `OPTION_TYPE_ID` with a concrete inner type and
  emits a typed option struct via `emit_typed_general_option_literal`
* `emit_pattern_condition` and `emit_variant_field_anyref` recognize typed option locals
  and use direct `struct.get` instead of payload array indirection
* `emit_local_atom` converts typed option locals back to erased `$rt_types__Variant`
  when consumed outside of match contexts (e.g. returned from a function, passed to a
  call expecting universal Variant)
* match scrutinee reads use `expected_ty = None` to bypass the conversion and access
  the raw typed struct for direct field access
* flow metadata (`LocalBackendInfo::typed_option`) tracks which locals hold typed option
  values, with proper save/restore across branches
* `is_typed_general_option_candidate` (shared between ctx.rs and emit.rs) excludes
  `Option<IterItem<T>>` which has its own dedicated iterator-adjacent path

Completed follow-up:

* `AMatch` results that produce typed `Option<T>` / `Result<T,E>` now preserve typed flow
  metadata when arm source analysis proves compatible typed-sum sources

## Non-Goals

This plan does not require:

* eliminating all uses of `Anyref` from the backend
* rewriting every variant/runtime helper into a typed family immediately
* removing the universal closure ABI, which is still needed for escaping and erased
  function values
* specializing all Option/Result variants globally (only the iterator-adjacent path)

What it does require is making the concrete iterator path fully typed end-to-end:

* `Iterator.unfold` → typed state struct
* step function call → typed dispatch (already done)
* `UnfoldStep.Yield` → typed payload struct
* `IterItem` → typed record fields
* `Option<IterItem>` → typed wrapping in iterator next return

**Related docs:**

* [../../internals/monomorphization.md](../../internals/monomorphization.md)
* [backend-pipeline-alignment.md](backend-pipeline-alignment.md)

---

## Suggested Ordering

1. ~~Finish [backend-pipeline-alignment.md](backend-pipeline-alignment.md).~~ Done.
2. ~~B1: typed iterator state struct (eliminates erased array container).~~ Done.
3. ~~B2: typed UnfoldStep payload (eliminates erased variant payload for step results).~~ Done.
4. ~~B3: typed IterItem record fields (eliminates anyref fields in IterItem).~~ Done.
5. ~~B4: typed Option wrapping (eliminates erased Option variant for iterator next results).~~ Done.
6. ~~C: typed `Option<T>` and `Result<T,E>` local specialization.~~ Done.

---

## Exit Criteria

Previously completed:

* monomorphized higher-order calls using named functions no longer fall back to universal
  closure dispatch
* closure helper/intrinsic paths remain correct under the subtype-based typed closure
  representation
* concrete `Cell<T>` uses no longer default to an erased `anyref` cell layout
* record field access preserves concrete Wasm types where possible
* iterator step closures no longer require universal closure dispatch in the common
  monomorphized case

Completed (B1–B4):

* monomorphized iterator state uses a typed struct instead of an erased array container
* concrete `UnfoldStep.Yield` payloads stay at their Wasm types instead of boxing to anyref
* `IterItem` record fields preserve concrete value/rest types
* `Option<IterItem>` wrapping in iterator next return uses a typed struct instead of
  universal Variant
* the universal fallback path still works for genuinely erased iterator parameters
* WAT audit of `tests/run/iterator*.tw` shows no unnecessary `Anyref` boxing or
  `ArrayGet` casts in the concrete iterator path

Completed (C — Option<T>):

* concrete `Option<T>` locals use typed structs with unboxed payloads instead of universal
  Variant + payload array
* typed option values are automatically converted back to erased Variant when crossing
  function boundaries or consumed outside match contexts
* the `twinkle_typechecker.tw` self-hosted test exercises this boundary heavily (recursive
  dict-lookup functions returning `Option<Record>`)

Completed (C — Result<T,E>):

* concrete `Result<T, E>` locals use typed structs with layout
  `(variant_id: i32, ok_payload: T, err_payload: E)` instead of universal Variant
* typed result values are automatically converted back to erased Variant at boundaries
* `is_typed_general_result_candidate` validates both type args are concrete
* pattern matching extracts Ok payload from struct field 1, Err from struct field 2

Follow-up completion:

* [wasm-option-amatch-typed-metadata.md](wasm-option-amatch-typed-metadata.md) — typed sum metadata for `AMatch`-produced locals
