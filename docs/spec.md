# 🌟 **Twinkle Language Specification**

## 1. Overview

Twinkle is a small statically typed language targeting **WebAssembly GC**.

Design goals:

* Lightweight, scripting-like syntax.
* Hindley–Milner type inference (Gleam/OCaml style).
* Unboxed primitives (`int = i64`, `float = f64`, `bool`).
* GC-managed references for strings, arrays, records, dicts.
* Small runtime; rely on `struct`, `array`, reference types.
* Inherent methods only via module functions.
* Immutable values with rebindable names.
* No trait system; capabilities via records of functions.

Source files end with `.tw`.

---

## 2. Value Model

### Immutability

**All values in Twinkle are immutable.**

* Primitives, strings, arrays, records, dicts, and functions cannot be mutated in place.
* There is no observable in-place mutation of values in the language model.
* Updates are expressed through rebinding: constructing a new value and binding a name to it.

### Primitives (unboxed)

* `int` → wasm `i64`
* `float` → wasm `f64`
* `bool` →  wasm `i32`, 0/1
* `void` → effect-only (no value).

### References (GC)

* `string` — immutable text.
* `array<T>` — immutable GC array; element unboxed/ref depending on `T`.
* `record` — immutable closed struct shape.
* `dict<K,V>` — immutable hash map reference.
* `function` — closure with captured environment (GC).

### `void`

* Used as function return type & block with no final expression.
* No literal and cannot be stored/bound.

---

## 3. Types & Generics

Parametric polymorphism:

```tw
fn map<A, B>(xs: array<A>, f: (A) -> B) -> array<B> { ... }
```

No higher-kinded types.

No trait constraints. Capabilities are passed as explicit function parameters (see Section 10).

Type alias:

```tw
type ID = int
```

Type alias doesn't create new distinct nominal type.

---

## 4. Option & Nullability

`Option<T>` defined as:

```tw
enum Option<T> { None, Some(T) }
```

Sugar:

```
T?  ==  Option<T>
```

No `null`.

Compiler optimizes reference-type options into nullable refs.

Pattern example:

```tw
case x {
  .None => ...,
  .Some(v) => ...,
}
```

---

## 5. Enums & Pattern Matching

Enum example:

```tw
enum Shape {
  Circle(float),
  Rect(float, float),
  UnitSquare,
}
```

Usage:

```tw
s := Shape.Circle(3.0)
```

Pattern:

```tw
case s {
  .Circle(r) => r*r*3.14159,
  .Rect(w, h) => w*h,
  .UnitSquare => 1.0,
}
```

Match must be exhaustive unless `_ => ...`.

---

## 6. Records

Named record type:

```tw
type Point = .{ x: int, y: int }
```

Record literal (two forms):

```tw
// Anonymous (requires expected type from context)
p: Point = .{ x: 10, y: 20 }

// Named constructor (explicit type)
p := Point.{ x: 10, y: 20 }
```

Field access: `p.x`

consider below as extension, and is outside of the scope of MVP

Anonymous record literal .{ field₁: e₁, ..., fieldₙ: eₙ } introduces a fresh type variable τ with a constraint:

* τ must be a nominal struct type whose declared fields are exactly { fieldᵢ: type(eᵢ) }.
* During type inference, τ may be unified with a concrete nominal struct type (e.g. Person). This succeeds iff that struct’s field set and field types match the constraint.
* All uses of the variable must agree on a single nominal struct type; otherwise, inference fails.
* If, after solving, τ is still unconstrained (no nominal type chosen) and the value **escapes** the function or is otherwise observable at an interface boundary, an explicit type annotation is required.
* This mechanism does not introduce structural record types into the language; it is solely a constraint-solving aid for anonymous record literals.

**escape** means:
* returned from function
* stored in array/dict
* included in record fields
* passed to another function
* assigned to a let-binding without annotation

---

## **7. Functions, Bindings, and Rebinding**

### 7.1 Function Declaration

```tw
fn f(x: int, y: int) -> int { x + y }
```

Functions are pure: they cannot mutate caller-visible state.
All “updates” create new values and rebind local names.

Parameters are ordinary local bindings and may be rebound within the function body (see §7.3).

Functions form **lexical scope boundaries**: names defined outside a function cannot be rebound inside the function.

---

### 7.2 Bindings

#### Initial binding

```tw
let x = expr
let x: T = expr
```

* Introduces a **new binding** `x` in the **current lexical scope**.
* If a binding with the same name exists in an outer scope, the new binding **shadows** it.
* Bindings refer to **immutable values**; the value cannot be changed in place.

Lexical scopes are introduced by:

* function bodies,
* blocks created with braces,
* pattern-bound names in `case` arms,
* loop variables in `for`,
* top-level module scope.

---

### 7.3 Rebinding

Rebinding provides *syntactic convenience* for expressing new values that replace old ones.

```tw
x = expr
```

#### Rules

1. `x = expr` is only legal if `x` refers to an existing binding in an enclosing lexical scope **within the same function** (or at top-level).
2. It **rebinds that binding**: i.e., reassigns the name to a new immutable value.
3. Rebinding **does not** create a new binding.
4. If multiple bindings of `x` exist due to shadowing, the **innermost** one is the target.
5. It is a compile-time error to use `x = expr` if no such binding exists.
6. Rebinding cannot cross function boundaries. A function cannot rebind variables defined in its caller or outer functions.

Thus, rebinding is always contained within the function where the corresponding `let` appears.

Example:

```tw
fn bump(n: int) -> int {
  n = n + 1   // rebinds parameter 'n'
  n
}
```

---

### 7.4 Rebinding and Control Flow

Control-flow constructs (`if`, `for`, `case`, blocks `{ ... }`) **do not** introduce new rebinding scopes, except for any names they explicitly define (e.g., loop variables, pattern-bound names).

Inside a `for` loop, rebinding targets the same lexical binding as outside the loop:

```tw
let acc = 0
for x in xs {
  acc = acc + x      // rebinds the acc defined above
}
acc                   // sees the final value
```

Nested bindings behave as expected with shadowing:

```tw
let acc = 0

if cond {
  let acc = 10       // new inner binding
  acc = acc + 1      // rebinds inner acc (11)
}

// outer acc is still 0
```

Pattern-bound names follow the same rules:

```tw
let x = 1

case opt {
  .Some(x) => {      // new binding shadows outer x
    x = x + 1        // rebinds pattern-bound x
    println(x)
  }
}

// outer x is unchanged (1)
```

---

### 7.5 Update Syntax (Desugaring)

Twinkle provides update-like syntax for ergonomics, but all updates are expressed as **rebinding to newly constructed values**.

#### Record field update

```tw
r.field = expr
```

Desugars to:

```tw
r = { r with field = expr }
```

#### Array index update

```tw
arr[i] = value
```

Desugars to:

```tw
arr = array.set(arr, i, value)
```

#### Compound assignment

```tw
x += y
```

Desugars to:

```tw
x = x + y
```

#### Left-hand side restrictions

Only **simple local names** may appear on the left of update or rebinding.
You cannot update through an expression:

* `foo().x = 1` — error
* `user.profile = ...` — allowed only if `user` is a local binding and the update desugars to rebinding `user`.

A *simple local name* is an identifier that resolves to a local binding in the current lexical scope and is not:

* a field access,
* an indexed expression,
* a module-qualified name,
* a function call.

---

### 7.6 Aliasing and Value Semantics

All values are immutable. Rebinding affects only the local name, not any other aliases:

```tw
let p = .{ y: 0 }
let q = p

p.y = 1      // p = { p with y = 1 }

q             // still { y: 0 }
```

Twinkle has **value semantics**, not reference semantics.

---

If you want, I can also cleanly integrate this into a fully revised “Bindings & Mutation Model” chapter later, or produce diagrams showing rebinding propagation and shadowing resolution.

## 8. Modules & Imports

XXX: we may need to rethink about the syntax / semantic of our module system, if we want Twinkle to be scripting friendly. in another word, without proper project setup, and we could drop a source code everywhere, even support shebang, etc.

Module:

```tw
pub fn foo() -> int { ... }
fn bar() -> int { ... }   // private
```

Import:

```tw
import "math"
```

Exports accessed as `math.f`, `math.Type`.

Prelude is implicitly imported.

- The last path segment (without extension) is the module identifier.
- No aliasing or destructuring in MVP.
- Resolution: string-literal paths (relative to the current working dir) with per-path caching; package/name resolution and richer import forms can be added later.
- Namespacing:
- Exported values and types are referred to with the module name (e.g., `math.add`, `math.Point`).
- Separate namespaces for values and types:
- A module may export both a type and a value/function with the same name.
- They are distinguished by context and never conflict.
- Example: `option.Option<T>`, `option.Some`, `option.None`.
- Future extensions:
- `import "mod" .{ Foo, Bar }` to bring specific exports into local scope (not in MVP).

---

## 9. Inherent Methods (Module-Based)

Twinkle’s dot syntax only supports:

1. **Record fields**
2. **Inherent/module methods**

A module may associate functions with a type by making them first-argument style:

```tw
// point.tw
pub type Point = .{ x: int, y: int }

pub fn translate(p: Point, dx: int, dy: int) -> Point {
  .{ x: p.x + dx, y: p.y + dy }
}
```

Dot sugar:

```tw
p.translate(1,2)
```

desugars to:

```tw
point.translate(p,1,2)
```

### Dot resolution rules

* Check record fields first.
* Then check module of the type for a matching inherent method.
* No trait involvement.
* If a name collision exists (field vs inherent), dot is illegal.

---

# **10. Capabilities via Records of Functions**

Twinkle **does not** support traits, interfaces, or typeclass-style implicit capability systems.

Instead, Twinkle uses **records of functions** to model capabilities.

---

## **10.1 No Traits or Interfaces**

* There is **no** syntax for declaring traits/interfaces (e.g. `trait Show`, `interface Iterable`).
* There is **no** way to write generic constraints such as `T: Show` or `T: Iterable`.
* There is **no** implicit resolution of "methods provided by a trait" based on the static type of a value.

All polymorphic behavior is expressed using:

* Ordinary **functions**,
* **Records of functions** (capability records),
* Modules and first-class values.

This keeps:

* The type system Hindley–Milner–style and simple,
* The compiler free from trait solvers, global coherence checks, and complex instance resolution.

---

## **10.2 Capabilities via Records of Functions**

A capability is a nominal type that captures a set of operations on some data type `T`.

**Example: Show capability**

```tw
type Show<T> = .{
  to_string: fn(T) -> string,
}
```

A function that needs "anything that can be shown" is written by taking both:

* the value(s),
* and a corresponding capability record.

**Example: Generic printing**

```tw
fn print_all<T>(xs: array<T>, show: Show<T>) {
  for x in xs {
    println(show.to_string(x))
  }
}
```

**Usage:**

```tw
type User = .{ name: string, age: int }

fn show_user(u: User) -> string {
  // yep, this is comment
  // twinkle will only have single line comment
  // twinkle currently doesn't support `s1 + s2`
  // string concat pattern, so we use string interpolation here
  "${u.name}(${u.age})"
}

ShowUser: Show<User> = .{
  to_string: show_user,
}

users: array<User> = ...
print_all(users, ShowUser)
```

**Key points:**

* The compiler does **not** invent or find `Show<User>` automatically.
* The call site is always **explicit** about which capability record is passed.

---

## **10.3 No Implicit Conversions**

Twinkle does **not** perform implicit conversions to satisfy capability records.

Given a parameter of type `Show<T>`:

```tw
fn debug_value<T>(x: T, show: Show<T>) { ... }
```

the call:

```tw
debug_value(user)       // ❌ illegal: missing Show<User>
```

is rejected. The caller must explicitly supply a value of type `Show<User>`:

```tw
debug_value(user, ShowUser)  // ✅
```

This applies uniformly:

* No automatic wrapping of `T` into `Show<T>` (or similar),
* No automatic rewriting of `array<T>` into `array<Show<T>>`,
* No chained or inferred conversions.

All adapter logic, if any, is explicit in user code.

---

## **10.4 Common Capability Patterns**

### Equality and Ordering

```tw
type Eq<T> = .{
  equals: fn(T, T) -> bool,
}

fn contains<T>(xs: array<T>, needle: T, eq: Eq<T>) -> bool {
  for x in xs {
    if eq.equals(x, needle) {
      return true
    }
  }
  false
}

type Point = .{ x: int, y: int }

EqPoint: Eq<Point> = .{
  equals(a, b) => a.x == b.x && a.y == b.y,
}

points: array<Point> = ...
p: Point = .{ x: 1, y: 2 }
found := contains(points, p, EqPoint)
```

### Collection-Specific Helpers

Instead of a general "Iterable" trait, provide small, concrete helpers:

```tw
fn sum_array(xs: array<int>) -> int {
  acc := 0
  for x in xs {
    acc = acc + x
  }
  acc
}
```

User types that want to participate reuse these helpers by returning supported built-ins (e.g. `Iterator<T>`) from explicit functions.

---

## 11. String Interpolation

String interpolation uses:

```tw
"hello ${x}"
```

Interpolation is **not** driven by a capability or trait. Instead, it is defined only for a **small, fixed set** of primitive types.

### Supported Types

In Twinkle, the expression inside `${...}` may have one of the following types:

* `string` — used as-is,
* `int`    — converted via a core `string.of_int` function,
* `float`  — converted via `string.of_float`,
* `bool`   — converted via `string.of_bool`.

Attempting to interpolate a value of any other type is a **compile-time error**.

### Example

```tw
name: string = "Twinkle"
n: int = 42
ok: bool = true

s := "name=${name}, n=${n}, ok=${ok}"  // ✅ ok

type User = .{ name: string, age: int }
user: User = .{ name: "Ada", age: 30 }
s2 := "user=${user}"                    // ❌ error: User not interpolable
```

### Desugaring

String literals with interpolation are desugared into calls on core string utilities.

For example:

```tw
"n=${n}"
```

is conceptually lowered to:

```tw
string.concat_many([
  "n=",
  string.of_int(n),
])
```

(Exact library function naming may vary.)

### Extension: Explicit Conversion Functions

To interpolate user-defined types, users write **explicit conversion functions** and use them inside the interpolation expression:

```tw
type User = .{ name: string, age: int }

fn user_to_string(u: User) -> string {
  u.name + " (" + int.to_string(u.age) + ")"
}

user: User = .{ name: "Ada", age: 30 }
s := "user=${user_to_string(user)}"    // ✅ ok
```

There is no automatic association between `User` and `user_to_string`. The choice of conversion is explicit at the call site.


---

## 12. Control Flow

### `if`

Expression:

```tw
if cond { a } else { b }
```

### `case`

On enums: exhaustive or `_ =>`.
On primitives: must include `_`.

### Loops

All `for` loops are statements returning `void`.

Forms:

```tw
for cond { body }

for x in coll { body }

for x,i in coll { body }
```

**Supported Collection Types:**

The `for x in coll` syntax is supported only for a **closed set** of built-in collection types:

* `array<T>` — homogeneous indexable arrays,
* `Range`    — integer ranges (e.g. `0..10`),
* `dict<K, V>` — dictionaries (iterates over key-value pairs),
* `Iterator<T>` — an explicit iterator type from the standard library.

The compiler performs a **type-directed** lowering:

* If `coll` has type `array<T>`, the loop is lowered to an indexed loop over the array length.
* If `coll` has type `Range`, the loop is lowered to a simple integer loop over the range bounds.
* If `coll` has type `dict<K, V>`, the loop is lowered to iteration over key–value pairs.
* If `coll` has type `Iterator<T>`, the loop is lowered to repeated `next` calls until the iterator is exhausted.

Any value used in `for x in coll` whose type is not one of the supported built-ins is a **compile-time error**.

**Indexed form:**

* `i: int` starts from 0 and increments each iteration.
* Independent of the underlying iterator state.
* Break/continue as usual.

**User Extensions:**

To iterate over a custom type, users define a **helper function** that produces a built-in collection or iterator:

```tw
// tree.tw
type Tree<T> = ...

pub fn iter<T>(t: Tree<T>) -> Iterator<T> {
  // implementation creates an Iterator<T> over the tree
}

// usage
fn sum_tree(t: Tree<int>) -> int {
  acc := 0
  for x in t.iter() {    // desugars to: tree.iter(t)
    acc = acc + x
  }
  acc
}
```

---

## 13. Collect Comprehension

```tw
xs := collect x in range(10) { x * x }
```

Rules:

* Produces `array<T>`.
* Works with the same built-in collection types as `for` loops (see Section 12).
* `continue` skips emission.
* `break` ends early, returns partial array.
* If the body returns void → error, because collect expects a value to push.
* The element type is inferred as the type of the body expression; all iterations must unify to same type; otherwise type error.

Example:

```tw
squares := collect x in range(1, 10) { x * x }
// squares: array<int> = [1, 4, 9, 16, 25, 36, 49, 64, 81]

evens := collect x in range(1, 20) {
  if x % 2 == 0 { x } else { continue }
}
// evens: array<int> = [2, 4, 6, 8, 10, 12, 14, 16, 18]
```

---

## 14. Arrays

Arrays are **immutable** sequences.

`arr[i]` indexing, 0-based (read-only access).

Built-in:

```tw
len(arr)
```

Array operations via module functions (all return new arrays):

* `array.set(arr, index, value) -> array<T>` — returns new array with element at index replaced
* `array.append(arr, value) -> array<T>` — returns new array with value appended
* `array.concat(arr1, arr2) -> array<T>` — returns new array combining both
* `array.slice(arr, start, end) -> array<T>` — returns new array with subset of elements
* etc.

Array literals:

```tw
[1, 2, 3]  // array<int>

xs: array<int> = []  // empty array requires type annotation
```

If context can't determine element type => compiler error.

```tw
[x, y, z]  // all elements must have the same type
```

**Update syntax:**

```tw
arr[i] = value
```

Desugars to:

```tw
arr = array.set(arr, i, value)
```


---

## 15. Strings

Strings are **immutable**.

`len(str)` returns the length of the string.

String interpolation is recommended for string assembly (see Section 11).

String operations via module functions (all return new strings):

* `string.concat(s1, s2) -> string`
* `string.substring(s, start, end) -> string`
* `string.of_int(n: int) -> string`
* `string.of_float(f: float) -> string`
* `string.of_bool(b: bool) -> string`
* etc.

---

## 16. Range

`range(10)` → 0..9
`range_from(a,b)` -> [a, b)
`range_step(a,b,step)`

Used by `for` and `collect`.

---

## 17. Dict

Dicts are **immutable** hash maps.

Creation:

```tw
m: dict<string, int> = dict.new()
```

Type parameters are inferred from the annotation.

Dict operations via module functions (all return new dicts):

* `dict.set(m, k, v) -> dict<K, V>` — returns new dict with key-value pair added/updated
* `dict.remove(m, k) -> dict<K, V>` — returns new dict with key removed
* `dict.get(m, k) -> V?` — returns Option<V> for safe access
* `dict.has(m, k) -> bool` — checks if key exists
* `dict.keys(m) -> array<K>` — returns array of keys
* `len(m)` — returns number of entries

Indexing syntax:

* `m[k]` returns `V?` (Option<V>) for safe read access
* `m[k] = v` desugars to `m = dict.set(m, k, v)`

---

## 18. Error Handling

No exceptions.

Unrecoverable = trap:

* OOB
* division by zero
* explicit `error("msg")`

Recoverable via `Result<T,E>`:

```tw
enum Result<T, E> { Ok(T), Err(E) }
```

`try` sugar:

```tw
try expr
```

* Only for `Result<T,E>`.
* Returns early with `Err(e)` on error.
* For `Result<void,E>` the `Ok` branch carries no value.
* `.Ok({})` is the way to present `void` return for `Result`, as `{}` evals to `void`.
* Cannot be applied to non-Result types (compile-time error).

---

## 19. Prelude

Implicitly imported.

Includes:

* primitive functions: `print`, `println`, `len`, `error`
* types: `int`, `float`, `string`, `bool`, `void`, `array<T>`, `dict<K,V>`, `Option<T>`, `Result<T,E>`
* range functions: `range`, `range_from`, `range_step`
* array module: `array.set`, `array.append`, `array.concat`, etc.
* dict module: `dict.new`, `dict.set`, `dict.get`, etc.
* string module: `string.concat`, `string.of_int`, `string.of_float`, `string.of_bool`, etc.

The prelude does not include any traits or implicit conversions.

---

## 20. Type Inference

Standard Hindley–Milner type inference:

* Unification
* Let-generalization (value restriction applies for refs)
* No trait constraints

Capabilities are ordinary values (records of functions), so they participate in normal type inference without special rules.

String interpolation is type-checked by verifying the expression type is one of: `string`, `int`, `float`, `bool`.

---

## 21. Compilation to WebAssembly GC

* Primitives → unboxed `i64/f64`
* Records → immutable `struct` (new values created via structural sharing where possible)
* Arrays → immutable `array` (new values created via structural sharing where possible)
* Dicts → immutable hash map structures (structural sharing where possible)
* Functions → closures allocated as small structs
* Options:

  * ref types → nullable refs
  * value types → tagged struct
* String interpolation → compiler inserts calls to `string.of_int`, `string.of_float`, `string.of_bool`
* For loops → type-directed lowering to primitive loops based on collection type

---

## 22. Error Messages

Examples:

**Invalid string interpolation**:

```
error: cannot interpolate value of type SocialPost
note: string interpolation only supports string, int, float, and bool
help: consider using an explicit conversion function: "${post_to_string(post)}"
```

**No inherent method**:

```
error: no method 'translate' for type Point
note: dot syntax only resolves record fields and inherent methods from the defining module
```

**Invalid for loop collection**:

```
error: cannot iterate over value of type Tree<int>
note: for loops only support array<T>, Range, dict<K,V>, and Iterator<T>
help: consider defining a helper function that returns Iterator<int>
```

**Mutation attempt on non-name**:

```
error: cannot update expression that is not a simple local name
note: only local variables can be updated; expressions like 'foo().x = 1' are not allowed
help: bind to a local variable first: 'tmp := foo(); tmp.x = 1'
```

---
