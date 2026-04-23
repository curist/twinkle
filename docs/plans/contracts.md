# Contracts Implementation Plan

## Goal

Introduce contracts as Twinkle's lightweight constrained-polymorphism model,
built on top of existing inherent method resolution.

The initial implementation should make builtin contracts such as `Stringify`
usable in generic bounds and selected syntax hooks, without introducing impl
blocks, instance search, or dynamic dispatch.

---

## Scope

### In scope

* builtin contracts, starting with `Stringify`
* generic bounds such as `T: Stringify`
* method lookup on bounded type parameters
* contract satisfaction checks at generic instantiation sites
* conditional satisfaction for user-defined generic types through constrained inherent methods
* contract-backed interpolation

### Out of scope for the initial rollout

* user-defined contract declarations
* separate `impl Contract for Type` blocks
* dynamic dispatch through contract values
* associated types or associated constants
* default method bodies
* retroactive conformance for foreign or builtin types

User-defined contracts may be added later, but they are not required to make the
contracts model useful.

---

## Required MVP Shape

The MVP must support all of the following.

### 1. Builtin contracts

At minimum:

* `Stringify`

with requirement:

```tw
to_string(self) -> String
```

### 2. Generic bounds

The checker must understand bounds such as:

```tw
fn show<T: Stringify>(x: T) String {
  x.to_string()
}
```

Inside the function body, the bound makes `to_string()` available on `T`.

### 3. Satisfaction checking at call sites

When a generic function is instantiated, the concrete type arguments must be
verified against the required builtin contracts.

### 4. Conditional satisfaction for user-defined generic types

This is required for MVP.

Twinkle must support user-defined generic wrappers and containers satisfying
builtin contracts through constrained inherent methods.

Example:

```tw
type Box<T> = .{ value: T }

fn to_string<T: Stringify>(b: Box<T>) String {
  "Box(${b.value.to_string()})"
}
```

The implementation must understand the resulting rule:

* `Box<T>` satisfies `Stringify` when its inherent `to_string` method matches
  the `Stringify` requirement and its own bounds can be satisfied.

Without this, builtin contracts would work only for builtin containers and
monomorphic user types. That would leave generic user-defined containers as
second-class citizens, which is not acceptable for the contracts model.

### 5. Interpolation through `Stringify`

Interpolation should typecheck in terms of `Stringify` and lower through the
resolved `to_string` method.

---

## Why user-defined contracts are optional but conditional satisfaction is not

User-defined contract declarations are a language-surface extension. They can be
added later without changing the core semantics of builtin contracts plus bounds.

Conditional satisfaction for user-defined generic types is different. It is part
of the core usefulness of the model. Without it, contracts would not compose
through ordinary user-defined wrappers and containers.

That means the roadmap is:

1. builtin contracts,
2. contract bounds,
3. conditional satisfaction for user-defined generic types,
4. user-defined contract declarations only if later needed.

---

## Compiler Work

### Resolver and signature model

Add support for contract bounds on generic parameters in function signatures.

The internal function-signature representation needs to carry:

* type parameters
* bounds per type parameter

Builtin contracts can initially live in compiler-managed metadata rather than a
user-visible declaration table.

### Type checker

The checker needs three key additions.

### Bounded type parameter method lookup

When checking code under a bound such as `T: Stringify`, method lookup on `T`
must succeed for the methods required by `Stringify`.

### Instantiation checks

At call sites, inferred or explicit type arguments must satisfy all required
contract bounds.

### Conditional satisfaction

When checking whether a concrete generic type satisfies a builtin contract, the
checker must examine the resolved inherent method and determine whether its own
bounds are satisfiable.

That is the crucial step for cases like `Box<T>: Stringify`.

### Lowering

Interpolation lowering should use the resolved `to_string` method after the
checker has established `Stringify` satisfaction.

No separate runtime representation for contracts is needed.

### Monomorphization

No dynamic dispatch is introduced. Once bounds are checked, the existing
monomorphization pipeline should continue to generate concrete code for the
resolved functions.

---

## Suggested rollout order

### Step 1 — Define builtin `Stringify`

Add compiler-managed contract metadata for `Stringify` with requirement:

```tw
to_string(self) -> String
```

Hook interpolation checking to this metadata rather than ad-hoc type cases.

### Step 2 — Add generic bound syntax and internal representation

Support function signatures such as:

```tw
fn show<T: Stringify>(x: T) String { ... }
```

The parser, resolver, and signature model must all preserve the bound.

### Step 3 — Teach the checker bounded type parameter method lookup

Inside a function body, `x.to_string()` should typecheck when `x: T` and
`T: Stringify`.

### Step 4 — Teach the checker contract satisfaction at instantiation sites

Calls to bounded generic functions should fail clearly when the instantiated
type does not satisfy the required contract.

### Step 5 — Implement conditional satisfaction for generic user-defined types

This step is mandatory for MVP, not a follow-up.

The checker must recognize that a generic inherent method with matching bounds
allows the enclosing generic type to satisfy the contract conditionally.

### Step 6 — Lower interpolation through resolved `to_string`

Once checking is contract-based, lowering can rely on the resolved method call
rather than container-specific special cases.

---

## Diagnostics

Diagnostics should be phrased in contract terms.

Examples:

* `type Buffer does not satisfy Stringify: missing to_string(self) -> String`
* `type User has to_string(self) -> Byte, expected String for Stringify`
* `type Box<Foo> does not satisfy Stringify because Foo does not satisfy Stringify`

These diagnostics are especially important once conditional satisfaction enters
the picture.

---

## Tests

### Positive tests

* primitive `Stringify` bounds
* user-defined monomorphic type satisfying `Stringify`
* `Vector<T>` stringification when `T: Stringify`
* user-defined generic wrapper satisfying `Stringify` under bounds
* interpolation of values whose types satisfy `Stringify`

### Negative tests

* bounded generic call with unsatisfied `Stringify`
* type with wrong `to_string` return type
* generic wrapper whose inner type does not satisfy `Stringify`
* interpolation of non-`Stringify` values

---

## Follow-ups

Once the MVP is stable, Twinkle can evaluate:

* additional builtin contracts such as `Slice` or `IntoIterator`
* user-defined contract declarations
* better syntax for multiple bounds if needed

These are follow-ups. They should not block the core contracts rollout.
