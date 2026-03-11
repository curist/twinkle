# `to_string` Method Unification Plan

## Goal

Unify Twinkle string conversion so all user-facing conversions use `.to_string()` method dispatch:

* Primitive types (`Int`, `Float`, `Bool`, `String`) expose builtin inherent `.to_string()`.
* User-defined named types can provide inherent `to_string(receiver) String`.
* String interpolation (`"${x}"`) lowers through the same `.to_string()` mechanism.
* Legacy free functions (`int_to_string`, `float_to_string`, `bool_to_string`, `string_to_string`) are removed from the surface language (not optional).

This plan is intentionally red/green TDD-first.

---

## Scope

In scope:

* Type checker method resolution for interpolation and method calls.
* Lowering/codegen/interpreter/runtime paths needed for method-based conversion.
* Removal of free conversion names from prelude/user API.
* Spec and user docs updates.

Out of scope:

* Traits/typeclasses/capability-based implicit conversion.
* Any dynamic fallback formatting (`Debug`, reflection, runtime type inspection).

---

## Behavior Contract (target state)

1. `x.to_string()` works for:
   * `Int`, `Float`, `Bool`, `String` (builtin inherent methods),
   * named user types with inherent `to_string(self) String`.
2. `"${x}"` is equivalent to `"${x.to_string()}"` after type checking/lowering.
3. Interpolation fails at compile time when no valid zero-arg `to_string` returning `String` exists.
4. Calling `int_to_string`, `float_to_string`, `bool_to_string`, or `string_to_string` in user code is a compile-time undefined-name error.

---

## Embedded Spec Snapshot (self-contained)

This section copies the relevant spec intent into this plan so implementation work
does not need to cross-reference `docs/spec.md`.

### Method model (normative)

* String conversion is exposed via inherent `.to_string()` methods.
* Built-in methods:
  * `Int.to_string() String`
  * `Float.to_string() String`
  * `Bool.to_string() String`
  * `String.to_string() String` (identity)
* Dot resolution remains:
  * check record fields first,
  * then check inherent methods for the type,
  * no trait/typeclass dispatch.

### Interpolation model (normative)

* Interpolation does not use a trait/typeclass system.
* `${expr}` uses a compiler-recognized conversion hook:
  resolve a zero-arg inherent `to_string() -> String` on `expr`'s type.
* Built-in interpolation support exists for `String`, `Int`, `Float`, `Bool`.
* User-defined named types are interpolable when they define:

```tw
fn to_string(x: MyType) String { ... }
```

* If no valid `to_string() String` exists, interpolation is a compile-time error.

### Explicit call examples (normative)

```tw
println("${1.5.to_string()}")   // explicit call on Float literal

f := 1.5
println("${f.to_string()}")     // explicit call on identifier

println("${(-1).to_string()}")  // unary-minus literal must be parenthesized
// println("${-1.to_string()}") // parsed as -(1.to_string()), invalid
```

### Lowering expectation (normative)

* String interpolation lowers to `to_string()` calls plus string concatenation.
* It must not depend on user-callable free conversion names.

### Prelude/API boundary (normative)

* snake_case conversion helpers
  (`int_to_string`, `float_to_string`, `bool_to_string`, `string_to_string`)
  are not part of the language and must not be callable from user code.

### Error-message targets (normative)

Invalid interpolation (missing method):

```text
error: cannot interpolate value of type SocialPost
note: type SocialPost has no inherent method `to_string() -> String`
help: define `fn to_string(x: SocialPost) String { ... }` and use "${post}"
```

No inherent method in dot call:

```text
error: no method 'translate' for type Point
note: dot syntax only resolves record fields and inherent methods from the defining module
```

---

## Red/Green Plan

## Phase 0: Spec-first (docs red)

Red:

* Add/adjust spec statements that forbid free conversion functions and define method-based interpolation.

Green:

* `docs/spec.md` reflects the target behavior exactly (Sections 9/11/15/19/22/23).

---

## Phase 1: Remove free conversion surface APIs

Red tests:

* New fail fixtures: calls to `int_to_string(1)`, `float_to_string(1.0)`, `bool_to_string(true)`, `string_to_string("x")` must fail as undefined.
* Existing tests currently using these names are updated to `.to_string()` and should fail before implementation.

Green implementation:

* Remove free conversion entries from user-visible prelude/type environment.
* Keep internal runtime helpers as private backend implementation detail if still needed.

---

## Phase 2: Builtin primitive `to_string` as inherent methods

Red tests:

* Pass fixtures:
  * Primitive `.to_string()` works for `Int`, `Float`, `Bool`, `String`
    (receiver may be a literal or identifier).
  * `n.to_string() == "1"` where `n: Int = 1`
  * `f.to_string()` formatting matches current float formatting contract where `f: Float = 1.5`
  * `b.to_string() == "true"` where `b: Bool = true`
  * `s.to_string() == "abc"` where `s: String = "abc"`
  * unary-minus literal case is parenthesized: `(-1).to_string()`
* Check both interpreter path and wasm backend path.

Green implementation:

* Ensure method resolution and lowering handle primitive `.to_string()` uniformly.
* Ensure no dependence on removed free surface function names.

---

## Phase 3: Interpolation dispatch through `to_string`

Red tests:

* Pass fixtures:
  * user type with inherent `to_string` interpolates directly: `"${point}"`.
  * cross-module inherent `to_string` also works.
* Fail fixtures:
  * named type without inherent `to_string` in interpolation.
  * wrong-signature `to_string` (args > 0 or non-`String` return) rejected.

Green implementation:

* Type checker validates interpolation by resolving method-style conversion constraints.
* Lowering emits method call equivalent (`x.to_string()`) instead of primitive-only switch.
* Error text points to missing/invalid `to_string` for interpolation.

---

## Phase 4: Cleanup and migration

Red:

* Stale docs/tests referring to free conversion functions fail lint/check.

Green:

* Update remaining examples/docs/tests to method style.
* Remove dead compatibility code paths if no longer needed.
* Add regression test ensuring no reintroduction of free conversion names in prelude.

---

## Suggested Test Matrix

* `tests/typecheck/pass/*`:
  * primitive `.to_string()`
  * user inherent `.to_string()`
  * interpolation with all supported paths
* `tests/typecheck/fail/*`:
  * missing/invalid `.to_string()` for interpolation
  * removed free conversion names
* `tests/run/*`:
  * output correctness (interpreter)
* `tests/run_wasm_*` or wasm snapshot tests:
  * output parity and no surface free-function dependency

---

## Risks and Mitigations

* Risk: typecheck accepts interpolation but lowering rejects (current mismatch).
  * Mitigation: add paired tests that run through full compile/lower path, not typecheck-only.
* Risk: float formatting divergence between interpreter and wasm.
  * Mitigation: pin expected format in one shared test fixture for both paths.
* Risk: docs drift across `spec.md` and secondary docs.
  * Mitigation: follow-up doc sweep after spec lands; treat spec as source of truth.

---

## Execution Order

1. Land spec update (this change set).
2. Add red tests for removed free functions and interpolation on user types.
3. Implement minimal green for prelude removal + primitive method dispatch.
4. Implement interpolation method dispatch for named types.
5. Run full test suite and clean docs/examples.
