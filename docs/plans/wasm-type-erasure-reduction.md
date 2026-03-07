# Wasm Type Erasure Reduction

**Goal:** Reduce unnecessary `Anyref`-based representations in the Wasm backend after
typed closure specialization, so concrete Twinkle values do not lose type information
when they move through records, iterators, and other runtime helper structures.

This is a follow-up plan, not an extension of Stage 9.6 itself. Stage 9.6 focuses on
typed higher-order parameter calls. This plan tracks the remaining places where the
backend still falls back to universal `Anyref`-heavy layouts even when concrete Wasm
types are available.

## Status

Done so far:

* named function values now participate in typed closure specialization in the normal
  build path
* `Cell.update` is implemented on the Wasm backend

Still open:

* typed user-record fields
* typed iterator state / less-erased iterator helper paths
* targeted reduction of `Anyref` payload layouts in hot helper/variant paths

---

## Motivation

The typed-closure work removed one important source of overhead: boxing at concrete
higher-order call boundaries such as `fold(xs, init, f)`.

However, a WAT audit of `examples/*` and `tests/run/*` still shows several broader
type-erasure patterns:

* named function values passed as first-class arguments can still miss typed
  specialization
* user record fields are emitted as `Anyref` unconditionally
* iterator state is represented as a generic `[seed_anyref, step_closure_anyref]` array
* runtime helpers such as `Iterator.next` still unpack closures and payloads through the
  universal closure ABI
* iterator / helper code still uses universal closure dispatch and erased payloads more
  often than necessary

The result is that some hot paths still allocate argument arrays, cast through
`$rt_types__Closure`, or box concrete payloads even though the program is fully
monomorphized.

---

## Confirmed Gaps

### 1. Named function values

Example:

```tw
fn apply<A, B>(f: fn(A) B, x: A) B { f(x) }
println("${apply(double, 42)}")
```

Today this can still go through universal closure dispatch if the specialization logic
does not discover the concrete signature through a plain named-function path.

Target:

* Treat concrete `AGlobalFunc` values as specialization candidates in the same spirit as
  concrete `AMakeClosure` values.

### 2. User record fields are always `Anyref`

Today [records are emitted with `Anyref` fields](../../src/codegen/emit.rs), even when
the source field type is concrete.

Consequences:

* function-valued record fields lose typed-closure information
* scalar fields like `Int` / `Bool` / `Float` also lose their concrete Wasm types
* record field access reintroduces casts/boxing that should not be necessary

Target:

* emit record fields with concrete Wasm field types wherever the language type is
  concrete
* preserve `Anyref` only for genuinely erased or polymorphic fields

### 3. Iterator representation erases both seed and step closure

Today `Iterator.unfold(seed, step)` is represented as:

```text
[seed_anyref, step_closure_anyref]
```

and `Iterator.next` reconstructs the step call through universal closure dispatch.

Target:

* introduce a typed iterator state representation after monomorphization
* specialize the step closure call when the iterator state type is concrete

### 4. Variant / helper payloads remain array-of-anyref based

Option/Result/iterator-step payloads still travel through generic payload arrays in many
places. This is not always wrong, but it means concrete payload types are often erased
even after monomorphization.

Target:

* audit which variants are hot enough to justify typed payload structs or typed helper
  layouts
* avoid broad refactors where the payoff is too small

## Proposed Work Items

### A. Finish closure-adjacent specialization

* Extend concrete-signature discovery so plain named-function values participate in
  specialization.
* Add focused tests for `apply(double, 42)`-style monomorphized higher-order calls.

### B. Add typed user-record fields

* Change record type emission to use field-specific Wasm types instead of unconditional
  `Anyref`.
* Update record literal / get / update emission accordingly.
* Add regression tests for capability records and scalar-field records.

### C. Introduce typed iterator state

* Replace the generic array-backed iterator representation with a typed state record or
  typed runtime struct when the iterator is monomorphized.
* Specialize `Iterator.next` so the step closure call avoids universal arg packing.

### D. Audit hot sum-type payload paths

* Identify whether `Option`, `Result`, and `UnfoldStep` are large enough contributors to
  justify typed payload layouts.
* Prefer targeted hot-path wins over global complexity.

## Non-Goals

This plan does not require:

* eliminating all uses of `Anyref` from the backend
* rewriting every variant/runtime helper into a typed family immediately
* removing the universal closure ABI, which is still needed for escaping and erased
  function values

---

## Suggested Ordering

1. Finish named-function typed specialization.
2. Add typed user-record fields.
3. Revisit iterator representation.
4. Only then decide whether typed variant payloads are worth the complexity.

---

## Exit Criteria

This plan is successful when:

* monomorphized higher-order calls using named functions no longer fall back to universal
  closure dispatch
* record field access preserves concrete Wasm types where possible
* iterator step closures no longer require universal arg-array packing in the common
  monomorphized case
* representative WAT audits of `examples/*` and `tests/run/*` show materially less
  unnecessary `Anyref`, `BoxedInt`, `BoxedFloat`, and `$rt_types__ClosureFunc` traffic
  outside genuinely erased/escaping cases
