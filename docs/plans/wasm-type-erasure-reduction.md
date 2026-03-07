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
* typed closures now use a stronger Wasm representation: typed closure structs are
  subtypes of the universal `$Closure`, so the same runtime value can flow through both
  universal and typed call paths
* `Cell.update` has a real Wasm implementation instead of trapping
* concrete `Cell<T>` instantiations now use typed Wasm cell layouts, with the old erased
  `anyref` container retained only as a fallback path

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

* `Cell<T>` is still represented as a 1-slot `anyref` container even when `T` is concrete
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

### 3. User record fields are always `Anyref`

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

### 4. Iterator representation erases both seed and step closure

Today `Iterator.unfold(seed, step)` is represented as:

```text
[seed_anyref, step_closure_anyref]
```

and `Iterator.next` reconstructs the step call through universal closure dispatch.

Target:

* introduce a typed iterator state representation after monomorphization
* specialize the step closure call when the iterator state type is concrete

### 5. Variant / helper payloads remain array-of-anyref based

Option/Result/iterator-step payloads still travel through generic payload arrays in many
places. This is not always wrong, but it means concrete payload types are often erased
even after monomorphization.

Target:

* audit which variants are hot enough to justify typed payload structs or typed helper
  layouts
* avoid broad refactors where the payoff is too small

## Proposed Work Items

### A. Stabilize closure-adjacent specialization

* Keep the named-function specialization coverage in place.
* Add focused Wasm regression coverage for helper/intrinsic paths that depend on the
  subtype-based typed-closure layout.

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

1. Finish the closure-subtyping follow-up fixes.
2. Add typed user-record fields.
3. Revisit iterator representation.
4. Only then decide whether typed variant payloads are worth the complexity.

---

## Exit Criteria

This plan is successful when:

* monomorphized higher-order calls using named functions no longer fall back to universal
  closure dispatch
* closure helper/intrinsic paths remain correct under the subtype-based typed closure
  representation
* concrete `Cell<T>` uses no longer default to an erased `anyref` cell layout
* record field access preserves concrete Wasm types where possible
* iterator step closures no longer require universal arg-array packing in the common
  monomorphized case
* representative WAT audits of `examples/*` and `tests/run/*` show materially less
  unnecessary `Anyref`, `BoxedInt`, `BoxedFloat`, and `$rt_types__ClosureFunc` traffic
  outside genuinely erased/escaping cases
