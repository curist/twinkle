Traits are all about callee-side magic.
Records-of-functions are all about caller-side adaptation.


Traits: “callee magically gets what it needs.”
Records-of-functions: “caller explicitly adapts args to what the callee needs.”



## 10. Capabilities, Traits, and Built-in Sugar

### 10.1. No Traits or Interfaces in Twinkle v1

Twinkle **does not** support traits, interfaces, or typeclass-style implicit capability systems in v1.

* There is **no** syntax for declaring traits/interfaces (e.g. `trait Show`, `interface Iterable`).
* There is **no** way to write generic constraints such as `T: Show` or `T: Iterable`.
* There is **no** implicit resolution of “methods provided by a trait” based on the static type of a value.

All polymorphic behavior is expressed using:

* Ordinary **functions**,
* **Records of functions** (capability records),
* Modules and first-class values.

This keeps:

* The type system Hindley–Milner–style and simple,
* The compiler free from trait solvers, global coherence checks, and complex instance resolution.

### 10.2. Capabilities via Records of Functions

Instead of traits, Twinkle uses **records of functions** to model capabilities.

A capability is a nominal type that captures a set of operations on some data type `T`. For example, a “Show”-like capability:

```twinkle
type Show<T> = .{
  to_string: fn(T) -> string,
}
```

A function that needs “anything that can be shown” is written by taking both:

* the value(s),
* and a corresponding capability record.

Example:

```twinkle
fn print_all<T>(xs: array<T>, show: Show<T>) {
  for x in xs {
    println(show.to_string(x))
  }
}
```

Usage:

```twinkle
type User = .{ name: string, age: int }

fn show_user(u: User) -> string {
  u.name + " (" + int.to_string(u.age) + ")"
}

let ShowUser: Show<User> = .{
  to_string: show_user,
}

let users: array<User> = ...
print_all(users, ShowUser)
```

Notes:

* The compiler does **not** invent or find `Show<User>` automatically.
* The call site is always **explicit** about which capability record is passed.

### 10.3. No Implicit Conversions

Twinkle does **not** perform implicit conversions to satisfy capability records.

Given a parameter of type `Show<T>`:

```twinkle
fn debug_value<T>(x: T, show: Show<T>) { ... }
```

the call:

```twinkle
debug_value(user)       // ❌ illegal: missing Show<User>
```

is rejected. The caller must explicitly supply a value of type `Show<User>`:

```twinkle
debug_value(user, ShowUser)  // ✅
```

This applies uniformly:

* No automatic wrapping of `T` into `Show<T>` (or similar),
* No automatic rewriting of `array<T>` into `array<Show<T>>`,
* No chained or inferred conversions.

All adapter logic, if any, is explicit in user code.

Twinkle may introduce future **syntactic sugar** to make it more convenient to construct capability records (e.g. shorter record literals or helper functions), but these are purely local syntactic conveniences. They **do not** change the explicitness of which values and capability records are passed where.

---

## 11. `for` over Collections

### 11.1. Overview

The `for` syntax in Twinkle:

```twinkle
for x in collection {
  body
}
```

is supported only for a **closed set** of built-in collection types. The compiler lowers `for` loops into primitive operations depending on the static type of `collection`.

There is **no** general “Iterable” trait or interface that user types can implement to participate in `for` syntactically in v1.

### 11.2. Supported Collection Types

In Twinkle v1, `for` is defined for the following core types (exact names may be adjusted as the core library evolves):

* `array<T>` — homogeneous indexable arrays,
* `Range`    — integer ranges (e.g. `0..10`),
* `dict<K, V>` — dictionaries (if present),
* `Iterator<T>` — an explicit iterator type from the standard library (if present).

The compiler performs a **type-directed** lowering:

* If `collection` has type `array<T>`, the loop is lowered to an indexed loop over the array length.
* If `collection` has type `Range`, the loop is lowered to a simple integer loop over the range bounds.
* If `collection` has type `dict<K, V>`, the loop is lowered to iteration over key–value pairs.
* If `collection` has type `Iterator<T>`, the loop is lowered to repeated `next` calls until the iterator is exhausted.

Any value used in `for x in collection` whose type is not one of the supported built-ins is a **compile-time error**.

### 11.3. Example Lowerings (Informal)

For arrays:

```twinkle
for x in xs {
  body
}
```

is conceptually lowered to:

```twinkle
let len = array.length(xs)
let i = 0
while i < len {
  let x = xs[i]
  body
  i = i + 1
}
```

For ranges:

```twinkle
for i in 0..n {
  body
}
```

is conceptually lowered to:

```twinkle
let start = 0
let end = n
let i = start
while i < end {
  let i_value = i
  body
  i = i + 1
}
```

For iterators:

```twinkle
for x in iter {
  body
}
```

is conceptually lowered to:

```twinkle
let current = Iterator.next(iter)
while current.is_some() {
  let x = Option.unwrap(current)
  body
  current = Iterator.next(iter)
}
```

(Exact iterator API naming may differ.)

### 11.4. Idiomatic User Extensions Without Traits

To iterate over a custom type without direct `for` support, users define a **helper function** that produces a built-in collection or iterator.

Example: iterate over a `Tree<T>` using an explicit iterator.

```twinkle
type Tree<T> =
  | Leaf(T)
  | Node(Tree<T>, Tree<T>)

fn tree_preorder_iter<T>(t: Tree<T>) -> Iterator<T> {
  // implementation creates an Iterator<T> over the tree
}

fn sum_tree(t: Tree<int>) -> int {
  let acc = 0
  for x in tree_preorder_iter(t) {
    acc = acc + x
  }
  acc
}
```

Here:

* `Tree<T>` does **not** participate directly in `for`.
* The user writes `for x in tree_preorder_iter(t)` instead of `for x in t`.

This pattern is preferred over adding a trait-style “Iterable” system.

---

## 12. String Interpolation

### 12.1. Overview

Twinkle supports string interpolation of the form:

```twinkle
"Value = ${expr}"
```

Interpolation is **not** driven by a `Show` trait or interface. Instead, it is defined only for a **small, fixed set** of primitive types.

### 12.2. Supported Types

In Twinkle v1, the expression inside `${...}` may have one of the following types:

* `string` — used as-is,
* `int`    — converted via a core `string.of_int` function,
* `float`  — converted via `string.of_float`,
* `bool`   — converted via `string.of_bool`.

Attempting to interpolate a value of any other type is a **compile-time error**.

Example:

```twinkle
let name: string = "Twinkle"
let n: int = 42
let ok: bool = true

let s = "name=${name}, n=${n}, ok=${ok}"  // ✅ ok

let user: User = .{ name: "Ada", age: 30 }
let s2 = "user=${user}"                    // ❌ error: User not interpolable
```

### 12.3. Informal Desugaring

String literals with interpolation are desugared into calls on core string utilities.

For example:

```twinkle
"n=${n}"
```

is conceptually lowered to:

```twinkle
string.concat_many([
  "n=",
  string.of_int(n),
])
```

Exact library function naming (e.g. `string.concat` or `string.concat_many`) may vary, but:

* desugaring is **local and explicit**, and
* interpolation does **not** perform implicit conversions for arbitrary types.

### 12.4. Idiomatic Extension: Explicit Conversion Functions

To interpolate user-defined types, users write **explicit conversion functions** and use them inside the interpolation expression:

```twinkle
type User = .{ name: string, age: int }

fn user_to_string(u: User) -> string {
  u.name + " (" + int.to_string(u.age) + ")"
}

let user: User = .{ name: "Ada", age: 30 }
let s = "user=${user_to_string(user)}"    // ✅ ok
```

There is no automatic association between `User` and `user_to_string`. The choice of conversion is explicit at the call site.

---

## 13. Idiomatic Patterns Without Traits

This section illustrates common patterns Twinkle programmers should prefer instead of traits/interfaces.

### 13.1. Generic Pretty-Printing via Capability Records

```twinkle
type Show<T> = .{
  to_string: fn(T) -> string,
}

fn print_all<T>(xs: array<T>, show: Show<T>) {
  for x in xs {
    println(show.to_string(x))
  }
}
```

Usage:

```twinkle
type User = .{ name: string, age: int }

fn show_user(u: User) -> string {
  u.name + " (" + int.to_string(u.age) + ")"
}

let ShowUser: Show<User> = .{
  to_string: show_user,
}

let users: array<User> = ...
print_all(users, ShowUser)
```

### 13.2. Equality and Ordering

Instead of `Eq`/`Ord` traits, define concrete capability records and pass them explicitly:

```twinkle
type Eq<T> = .{
  equals: fn(T, T) -> bool,
}

fn contains<T>(xs: array<T>, needle: T, eq: Eq<T>) -> bool {
  let i = 0
  let len = array.length(xs)
  while i < len {
    if eq.equals(xs[i], needle) {
      return true
    }
    i = i + 1
  }
  false
}

type Point = .{ x: int, y: int }

let EqPoint: Eq<Point> = .{
  equals(a, b) => a.x == b.x and a.y == b.y,
}

let points: array<Point> = ...
let p: Point = .{ x: 1, y: 2 }
let found = contains(points, p, EqPoint)
```

### 13.3. Collection-Specific Helpers

Instead of a general “Iterable” trait, provide small, concrete helpers:

```twinkle
fn sum_array(xs: array<int>) -> int {
  let acc = 0
  for x in xs {
    acc = acc + x
  }
  acc
}

fn sum_range(r: Range) -> int {
  let acc = 0
  for i in r {
    acc = acc + i
  }
  acc
}
```

Or, via an explicit iterator type:

```twinkle
fn sum_iter(it: Iterator<int>) -> int {
  let acc = 0
  for x in it {
    acc = acc + x
  }
  acc
}
```

User types that want to participate reuse these helpers by returning supported built-ins (e.g. `Iterator<T>`) from explicit functions.

---

## 14. Future Ergonomics (Non-Normative)

Twinkle may evolve syntactic conveniences to make constructing and passing capability records easier (e.g. more concise record-literal syntax or helper builders), but:

* Such sugar will desugar **locally**, where written,
* Twinkle will **not** introduce implicit conversions or trait-like instance search,
* All capability passing will remain **explicit** in function signatures and at call sites.

This preserves Twinkle’s design goals: a small, predictable, statically typed language with a scripting feel, where data and functions are the primary abstraction mechanisms, and built-ins provide a small amount of carefully delimited “magic” (`for` and string interpolation) without a general trait or interface system.
