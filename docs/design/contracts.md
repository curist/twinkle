# Contracts

Twinkle uses **contracts** to name required method shapes for generic constraints
and selected syntax hooks.

A type **satisfies** a contract through Twinkle's existing inherent method
resolution. Twinkle does not use separate impl blocks, instance search, or a
second method-dispatch system for contracts.

This document supersedes the earlier syntax-hook-only direction by expanding
the same core idea into lightweight constrained polymorphism.

---

## Why This Exists

Twinkle wants pleasant, low-ceremony generic programming without taking on the
full complexity of a trait or interface system.

In particular, Twinkle wants to support code like:

```tw
fn to_string<T: Stringify>(xs: Vector<T>) String {
  body := xs.map(fn(x) { x.to_string() }).join(", ")
  "[${body}]"
}
```

without introducing:

* separate `impl` declarations,
* global instance search,
* coherence/orphan rules,
* dynamic dispatch,
* associated types,
* default methods,
* or multiple competing implementations for the same type.

Twinkle already has a practical mechanism for type-owned behavior: inherent
methods resolved from the static receiver type. Contracts formalize and extend
that mechanism.

---

## Definition

A **contract** is a named set of required inherent method signatures.

A type **satisfies** a contract when those method signatures resolve on the
static type through normal inherent method lookup and match the contract's
required shapes.

Contracts are therefore:

* **named** — reusable in generic bounds and diagnostics,
* **static** — checked during type checking,
* **inherent** — satisfied through existing type/module methods,
* **monomorphized** — no dynamic dispatch is required.

---

## Design Constraints

Contracts are intentionally smaller than traits, interfaces, or typeclasses.

### Twinkle contracts do have

* Named method requirements
* Generic bounds such as `T: Stringify`
* Static checking of required behavior
* Conformance derived from inherent methods
* Compiler-recognized use in selected syntax hooks

### Twinkle contracts do not have

* Separate `impl Contract for Type` blocks
* Global instance lookup
* Import-dependent instance resolution
* Coherence or orphan rules
* Dynamic dispatch through contract values
* Associated types or associated constants
* Default method bodies
* Contract inheritance or supercontracts in MVP
* Multiple implementations of the same contract for one type
* Retroactive conformance for foreign or builtin types outside their defining modules

These restrictions are deliberate. The goal is to enable constrained generic
programming while preserving Twinkle's simple method model.

---

## Satisfaction Model

Contracts are satisfied through the same mechanism Twinkle already uses for
ordinary method calls.

Given a required method such as:

```tw
to_string(self) -> String
```

checking whether a type satisfies `Stringify` means:

1. Determine the static receiver type.
2. Resolve the inherent method through the existing method registry.
3. Instantiate any generics involved.
4. Verify receiver shape, parameter types, and return type.

No separate implementation table is searched.

### Named types

A user-defined type satisfies a contract when its defining module provides the
required inherent methods.

Example:

```tw
type Point = .{ x: Int, y: Int }

fn to_string(p: Point) String {
  "Point(${p.x}, ${p.y})"
}
```

Here `Point` satisfies `Stringify` because `to_string(Point) -> String` is an
inherent method on `Point`.

### Builtin types

Builtin types may satisfy contracts through compiler-registered inherent
methods.

Examples:

* `Int` satisfies `Stringify` via `Int.to_string`
* `Float` satisfies `Stringify` via `Float.to_string`
* `Bool` satisfies `Stringify` via `Bool.to_string`
* `Byte` satisfies `Stringify` via `Byte.to_string`
* `String` satisfies `Stringify` via `String.to_string`

### Conditional satisfaction for builtin generic types

Some builtin generic type constructors may have compiler- or prelude-defined
conditional satisfaction rules.

This allows common container types to participate in generic APIs without
introducing general-purpose blanket impls. The current builtin rules are listed
in [../contracts.md](../contracts.md).

---

## Generic Bounds

A generic type parameter may require one or more contracts.

Example:

```tw
fn debug<T: Stringify>(x: T) String {
  x.to_string()
}
```

Within the function body, the checker may assume that `T` supports the methods
required by `Stringify`.

At each call site, the instantiated type must satisfy the required contracts.
If not, type checking fails at the use site.

Example:

```tw
type User = .{ name: String }

fn to_string(u: User) String { u.name }

ok := debug(User.{ name: "Ada" })
// legal: User satisfies Stringify
```

If a type does not satisfy the contract, the compiler should report that the
instantiated type is missing a required method or has the wrong signature.

### Why explicit bounds matter

Twinkle should not infer hidden method constraints from arbitrary generic bodies.
This is allowed:

```tw
fn show<T: Stringify>(x: T) String {
  x.to_string()
}
```

This is not:

```tw
fn show<T>(x: T) String {
  x.to_string()
}
```

The explicit bound keeps generic APIs honest and exportable.

---

## Contract Catalog

The current builtin contract reference lives in [../contracts.md](../contracts.md).

The language should add new contracts only when they correspond to canonical,
widely useful behavior. Possible future contracts include:

* `Slice`
* `IntoIterator`
* `IndexRead`
* `IndexWrite`

---

## Syntax Hooks

Contracts may also back selected language surface forms.

### Current syntax-backed contracts

Some language surface forms are typechecked through contracts, such as string
interpolation and comparison operators. The current mapping is listed in
[../contracts.md](../contracts.md).

### Future syntax-backed contracts

If Twinkle later generalizes more surface syntax, contracts provide a natural
vocabulary:

* slicing syntax could require `Slice`
* `for x in value` could require `IntoIterator`

Contracts therefore serve two roles:

* generic constraints in ordinary code,
* named method contracts for selected syntax features.

---

## Generic Conditional Satisfaction

Generic user-defined types may satisfy builtin contracts through constrained
inherent methods.

Example:

```tw
type Box<T> = .{ value: T }

fn to_string<T: Stringify>(b: Box<T>) String {
  "Box(${b.value.to_string()})"
}
```

This implies a rule of the form:

* `Box<T>` satisfies `Stringify` when its inherent `to_string` method matches
  the `Stringify` requirement and its own bounds can be satisfied.

This keeps generic user-defined wrappers and containers on equal footing with
builtin generic types.

## User-Defined Contracts

User-defined contract declarations are a plausible extension of this model, but
are not required by the core design.

If Twinkle later adds them, they should remain subject to the same limits as
builtin contracts:

* satisfied through inherent methods,
* static only,
* no separate impl blocks,
* no dynamic dispatch,
* no retroactive conformance.

Possible future syntax:

```tw
contract Parseable {
  fn parse(self, input: String) Result<Self, String>
}
```

### Important limitation

Because satisfaction is tied to inherent methods, a user can make **their own
types** satisfy an existing contract, but cannot retroactively make a foreign or
builtin type satisfy that contract outside the type's defining module.

This is a deliberate tradeoff. It preserves canonical, local, predictable
conformance and avoids coherence problems.

---

## Relationship to Records of Functions

Records of functions remain an ordinary Twinkle programming technique. They do
not need to be removed from the language.

However, they are no longer the primary answer to canonical shared behavior in
generic APIs.

Use **contracts** when:

* behavior is canonical,
* the behavior naturally belongs to the type,
* there should be one obvious meaning for a type/behavior pair,
* syntax or generic code should rely on it.

Use ordinary function arguments or records of functions when:

* behavior is chosen by the caller,
* multiple strategies are equally valid,
* behavior should be passed around as first-class data.

Examples of behavior that should likely stay explicit:

* custom formatting styles,
* custom comparators,
* serialization formats,
* alternate equality or ordering semantics.

In short:

* **contracts** are for canonical, type-owned behavior,
* **function values / records of functions** are for caller-chosen behavior.

---

## Naming and Terminology

Preferred terms for this design:

* **contract**
* **contract requirement**
* **satisfies a contract**
* **contract bound**
* **conditional satisfaction**

Avoid using terms that imply a larger feature set than intended:

* `trait`
* `interface`
* `instance`
* `impl`

Those terms carry expectations around explicit implementation declarations,
instance search, or runtime polymorphism that Twinkle contracts do not provide.

---

## Examples

### Generic vector stringification

```tw
fn to_string<T: Stringify>(xs: Vector<T>) String {
  body := xs.map(fn(x) { x.to_string() }).join(", ")
  "[${body}]"
}
```

### Generic wrapper type

```tw
type Box<T> = .{ value: T }

fn to_string<T: Stringify>(b: Box<T>) String {
  "Box(${b.value.to_string()})"
}
```

### Interpolation

```tw
type Point = .{ x: Int, y: Int }

fn to_string(p: Point) String {
  "(${p.x}, ${p.y})"
}

p := Point.{ x: 1, y: 2 }
println("point=${p}")
```

---

## Non-Goals

This design does not aim to add:

* a full trait system,
* trait objects or dynamic dispatch,
* retroactive implementations for foreign types,
* multiple competing implementations for one type,
* associated types,
* default method bodies,
* blanket impls in user code,
* or a second dispatch mechanism separate from inherent methods.

If Twinkle ever needs those features, they should be evaluated separately rather
than added accidentally under the name of contracts.

---

## Summary

Contracts give Twinkle a lightweight form of constrained polymorphism by naming
required inherent method shapes.

They extend the earlier syntax-hook-only direction in three ways:

* they support generic bounds,
* they provide a clearer foundation for syntax hooks such as interpolation,
* they replace records-of-functions as the promoted model for canonical shared behavior.

Twinkle contracts are intentionally narrow:

* no impl blocks,
* no instance search,
* no dynamic dispatch,
* generic user-defined types may satisfy builtin contracts through constrained inherent methods,
* just static constraints satisfied through inherent methods.
