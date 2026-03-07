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
[../internals/monomorphization.md](../internals/monomorphization.md).

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

Still open:

* fully typed iterator state representation instead of the current erased array payload
* targeted reduction of `Anyref` payload layouts in hot helper/variant paths

These remaining items now build on top of the completed
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

### 4. Iterator representation still erases seed and payload layout

Status: partially done.

Today `Iterator.unfold(seed, step)` still stores state as:

```text
[seed_anyref, step_closure_anyref]
```

What changed:

* when the compiler can still prove the concrete unfold state, `Iterator.next` now routes
  through a typed helper that:
  * casts the stored step closure back to its typed subtype
  * calls the step function via concrete `call_ref`
  * avoids the universal `$ClosureFunc` dispatch path
* this currently works for direct unfold-backed locals and for user functions whose return
  value is provably a concrete unfold state

What is still missing:

* the iterator state container itself is still an erased array
* seeds and `UnfoldStep` payloads still move through `Anyref`
* generic `Iterator<T>` parameters still use the fallback helper path

Target:

* replace the erased array-backed iterator representation with a typed state record or
  typed runtime struct after monomorphization
* preserve the typed helper dispatch as the fast path on top of that concrete state layout
* move toward a monomorphized `Iterator.next__T__S`-style helper family for concrete cases

### 5. Variant / helper payloads remain array-of-anyref based

Option/Result/iterator-step payloads still travel through generic payload arrays in many
places. This is not always wrong, but it means concrete payload types are often erased
even after monomorphization.

Target:

* audit which variants are hot enough to justify typed payload structs or monomorphized
  helper families
* avoid broad refactors where the payoff is too small

## Proposed Work Items

### A. Stabilize closure-adjacent specialization

* Keep the named-function specialization coverage in place.
* Add focused Wasm regression coverage for helper/intrinsic paths that depend on the
  subtype-based typed-closure layout.

### B. Introduce typed iterator state

* Replace the generic array-backed iterator representation with a typed state record or
  typed runtime struct when the iterator is monomorphized.
* Extend the current typed `Iterator.next` helper path so generic iterator plumbing no longer
  erases seed/rest payload layout in the common concrete case.
* Treat the long-term target as monomorphized helper emission for concrete iterator state,
  with the current universal helper retained only as fallback.

### C. Audit hot sum-type payload paths

* Identify whether `Option`, `Result`, and `UnfoldStep` are large enough contributors to
  justify typed payload layouts or monomorphized helper emission.
* Prefer targeted hot-path wins over global complexity.

## Non-Goals

This plan does not require:

* eliminating all uses of `Anyref` from the backend
* rewriting every variant/runtime helper into a typed family immediately
* removing the universal closure ABI, which is still needed for escaping and erased
  function values

What it does require is making the default concrete path better:

* concrete monomorphized code should not pay `Anyref` costs by default when the backend can
  reasonably emit a concrete layout/helper instead

**Related docs:**

* [../internals/monomorphization.md](../internals/monomorphization.md)
* [backend-pipeline-alignment.md](backend-pipeline-alignment.md)

---

## Suggested Ordering

1. Finish [backend-pipeline-alignment.md](backend-pipeline-alignment.md).
2. Finish iterator state layout specialization.
3. Only then decide whether typed variant payloads are worth the complexity.

---

## Exit Criteria

This plan is successful when:

* monomorphized higher-order calls using named functions no longer fall back to universal
  closure dispatch
* closure helper/intrinsic paths remain correct under the subtype-based typed closure
  representation
* concrete `Cell<T>` uses no longer default to an erased `anyref` cell layout
* record field access preserves concrete Wasm types where possible
* iterator step closures no longer require universal closure dispatch in the common
  monomorphized case
* monomorphized iterator state no longer defaults to an erased array container
* hot helper paths such as iterator/variant plumbing prefer monomorphized helper emission
  when the relevant concrete types are available
* representative WAT audits of `examples/*` and `tests/run/*` show materially less
  unnecessary `Anyref`, `BoxedInt`, `BoxedFloat`, and `$rt_types__ClosureFunc` traffic
  outside genuinely erased/escaping cases
