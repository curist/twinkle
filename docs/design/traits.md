# Capabilities Without Traits

Twinkle does not have traits, interfaces, or typeclasses. Polymorphic behavior
is expressed through ordinary functions and records of functions (capability
records). This document explains the rationale and the patterns that replace
traits.

For compiler-recognized syntax hooks and lightweight constrained generics,
see [Contracts](contracts.md). Contracts are method-signature requirements
satisfied through inherent methods, not traits.

---

## Why No Traits

Traits are callee-side magic — the callee declares what it needs, and the
compiler automatically finds the right implementation. Records of functions are
caller-side adaptation — the caller explicitly passes the capability.

Avoiding traits keeps the type system rank-1 polymorphic (Damas–Milner) and
the compiler free from trait solvers, global coherence checks, and complex
instance resolution.

---

## Capability Records

A capability is a nominal record type that captures a set of operations on
some type `T`:

```tw
type Show<T> = .{
  to_string: fn(T) String,
}
```

Functions that need "anything that can be shown" take both the value and a
corresponding capability record:

```tw
fn print_all<T>(xs: Vector<T>, show: Show<T>) {
  for x in xs {
    println(show.to_string(x))
  }
}
```

Usage:

```tw
type User = .{ name: String, age: Int }

fn show_user(u: User) String {
  "${u.name} (${u.age})"
}

ShowUser: Show<User> = .{
  to_string: show_user,
}

users: Vector<User> = ...
print_all(users, ShowUser)
```

The compiler does not find or invent `Show<User>` automatically. The call site
is always explicit about which capability record is passed.

---

## No Implicit Conversions

There is no automatic wrapping, rewriting, or chained conversion:

```tw
debug_value(user)             // error: missing Show<User>
debug_value(user, ShowUser)   // ok
```

All adapter logic is explicit in user code.

---

## `for` Over Collections

The `for` syntax works with a closed set of built-in collection types. There is
no "Iterable" trait that user types can implement.

Supported types:

* `Vector<T>` — lowered to an indexed loop over length
* `Range` — lowered to an integer loop over bounds
* `Dict<K, V>` — lowered to iteration over keys
* `Iterator<T>` — lowered to repeated `next` calls

The compiler performs type-directed lowering. Any other type in `for x in coll`
is a compile-time error. The exact IR is described in `docs/internals/ir.md`.

To iterate over a custom type, users write a helper that returns a built-in
collection or iterator:

```tw
fn tree_preorder_iter<T>(t: Tree<T>) Iterator<T> { ... }

for x in tree_preorder_iter(t) {
  // ...
}
```

This is still the current model. If Twinkle later adopts an `IntoIterator`
contract hook, it should remain grounded in inherent methods rather than a
separate trait implementation system.

---

## String Interpolation

String interpolation (`"${expr}"`) is not trait-driven. It is defined by the
`Stringify` contract:

```tw
to_string(self) -> String
```

Built-ins satisfy it via registered inherent methods. User-defined named types
can satisfy it by defining an inherent `to_string` method with that signature.

```tw
type User = .{ name: String, age: Int }
fn to_string(u: User) String { "${u.name} (${u.age})" }

u: User = .{ name: "Ada", age: 30 }
s := "user=${u}"   // ok
```

If no matching inherent method exists, interpolation is a compile-time error.

Explicit conversion is still valid and often useful:

```tw
s := "user=${user_to_string(user)}"
```

---

## Idiomatic Patterns

### Equality and ordering

Instead of `Eq`/`Ord` traits, define capability records and pass them explicitly:

```tw
type Eq<T> = .{
  equals: fn(T, T) Bool,
}

fn contains<T>(xs: Vector<T>, needle: T, eq: Eq<T>) Bool {
  for x in xs {
    if eq.equals(x, needle) {
      return true
    }
  }
  false
}
```

### Collection helpers

Instead of a general "Iterable" trait, provide concrete helpers:

```tw
fn sum_array(xs: Vector<Int>) Int {
  acc := 0
  for x in xs { acc = acc + x }
  acc
}
```

Or use `Iterator<T>` for generic versions.

---

## Future Ergonomics

Twinkle may add syntactic conveniences to make constructing capability records
easier, but:

* Sugar will desugar locally, where written
* No implicit conversions or trait-like instance search will be introduced
* All capability passing will remain explicit in function signatures and at call sites

Separately, Twinkle may define additional contract-backed hooks for language
syntax (see [Contracts](contracts.md)). These remain distinct from explicit
function-argument based API polymorphism.
