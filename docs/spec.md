# 🌟 **Twinkle Language Specification**

## 1. Overview

Twinkle is a small statically typed language targeting **WebAssembly GC**.

Design goals:

* Concise, low-ceremony syntax.
* Rank-1 polymorphic (Damas–Milner) type system with bidirectional type checking.
* Unboxed primitives (`Int = i64`, `Float = f64`, `Bool`).
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

* `Int` → wasm `i64`
* `Float` → wasm `f64`
* `Bool` →  wasm `i32`, 0/1
* `Void` → effect-only (no value).

### References (GC)

* `String` — immutable text.
* `Array<T>` — immutable GC array; element unboxed/ref depending on `T`.
* `record` — immutable closed struct shape.
* `Dict<K,V>` — immutable hash map reference.
* `function` — closure with captured environment (GC).

### `Void`

* Used as function return type & block with no final expression.
* No literal and cannot be stored/bound.

---

## 3. Types & Generics

Parametric polymorphism:

```tw
fn map<A, B>(xs: Array<A>, f: fn(A) B) Array<B> { ... }
```

No higher-kinded types.

No trait constraints. Capabilities are passed as explicit function parameters (see Section 10).

Type alias:

```tw
type ID = Int
```

Type alias doesn't create new distinct nominal type.

---

## 4. Option & Nullability

`Option<T>` defined as:

```tw
type Option<T> = { None, Some(T) }
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
type Shape = {
  Circle(Float),
  Rect(Float, Float),
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
type Point = .{ x: Int, y: Int }
```

Record literal (two forms):

```tw
// Anonymous (requires expected type from context)
p: Point = .{ x: 10, y: 20 }

// Named constructor (explicit type)
p := Point.{ x: 10, y: 20 }
```

Field access: `p.x`

---

## **7. Functions, Bindings, and Rebinding**

### 7.1 Function Declaration

```tw
fn f(x: Int, y: Int) Int { x + y }
```

Functions are pure: they cannot mutate caller-visible state.
All “updates” create new values and rebind local names.

Parameters are ordinary local bindings and may be rebound within the function body (see §7.3).

The return type is written after the parameter list (no `->`). It may be omitted when inference suffices; when omitted, the function body’s value determines the return type.

Functions form **lexical scope boundaries**: names defined outside a function cannot be rebound inside the function.

---

### 7.2 Bindings

#### Initial binding

```tw
x := expr
x: T = expr
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
2. It introduces a **fresh binding identity** for `x` — the name now refers to a new immutable value. It does not mutate a stored cell; it changes what future references to the name resolve to.
3. Rebinding introduces a fresh binding identity for `x` that replaces the previous one for the remainder of the current lexical region; it does not introduce an additional scope layer.
4. If multiple bindings of `x` exist due to shadowing, the **innermost** one is the target.
5. It is a compile-time error to use `x = expr` if no such binding exists.
6. Rebinding cannot cross function boundaries. A function cannot rebind variables defined in its caller or outer functions.

Thus, rebinding is always contained within the function where the corresponding `:=`/typed binding appears.

Example:

```tw
fn bump(n: Int) Int {
  n = n + 1   // rebinds parameter 'n'
  n
}
```

---

### 7.4 Rebinding and Control Flow

Control-flow constructs (`if`, `for`, `case`, blocks `{ ... }`) **do not** introduce new rebinding scopes, except for any names they explicitly define (e.g., loop variables, pattern-bound names).

Inside a `for` loop, rebinding targets the same lexical binding as outside the loop:

```tw
acc := 0
for x in xs {
  acc = acc + x      // rebinds the acc defined above
}
acc                   // sees the final value
```

Nested bindings behave as expected with shadowing:

```tw
acc := 0

if cond {
  acc := 10          // new inner binding
  acc = acc + 1      // rebinds inner acc (11)
}

// outer acc is still 0
```

Pattern-bound names follow the same rules:

```tw
x := 1

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
arr = Array.set(arr, i, value)
```

#### Dict index update

```tw
m[k] = v
```

Desugars to:

```tw
m = Dict.set(m, k, v)
```

#### Assignment targets

The grammar allows identifiers, field accesses, and indexed expressions on the left of `=`. Field and index forms are still sugar that rebuild the owner value (see record/array/dict desugarings above); implementations should evaluate the left-hand side once when lowering.

Nested field chains (`a.b.c = x`) are supported and desugar recursively from the inside out:

```tw
a.b.c = x
// desugars to:
a.b = { a.b with c = x }
// which desugars to:
a = { a with b = { a.b with c = x } }
```

The root of the chain must be a local identifier. Chains starting with expressions (e.g., `foo().x = 1`) are not allowed.

---

### 7.6 Aliasing and Value Semantics

All values are immutable. Rebinding affects only the local name, not any other aliases:

```tw
type Pt = .{ y: Int }

p := Pt.{ y: 0 }
q := p

p.y = 1      // p = Pt.{ y: 1 }

q             // still Pt.{ y: 0 }
```

Twinkle has **value semantics**, not reference semantics.

### 7.7 Closure Capture

A function expression (`fn (...) { ... }`) may reference names defined in its surrounding lexical scopes.
When such a function is **defined**, Twinkle captures the **current value** of each free variable.
Closures capture *values*, not mutable cells.

This section formalizes the capture model.

---

#### 7.7.1 Capture-by-Value (Definition-Site Semantics)

When a closure is created:

* Each free variable `x` is resolved to the **innermost visible binding**.
* The **value** of that binding at the point of the closure's definition is captured.
* This captured value is final and does not change, even if the name is later rebound in the same scope.

Example:

```tw
x := 1
f := fn() Int { x }
x = 2

f()    // returns 1
```

Explanation:

* `f` captures the value `1` (the value of the `x` binding at definition time).
* `x = 2` introduces a new shadowing binding for later code, but does not affect `f`.

---

#### 7.7.2 Shadowing and Captured Variables

Closures always capture the **innermost lexical binding** visible at their definition site.

```tw
x := 0

fn outer() fn() Int {
  x := 10                // new shadowing binding
  fn() Int { x }         // captures the inner x = 10
}

f := outer()
x = 99                   // rebinding the outer x

f()                      // returns 10
```

* Inner `x` shadows outer `x`.
* The closure sees only the shadowing `x = 10`.

---

#### 7.7.3 Rebinding After Closure Creation Does Not Affect Closures

Rebinding (`x = expr`) is sugar for introducing a new shadowing binding.
Therefore, closures created **before** the rebinding continue to see the old binding.

```tw
x := 1

f := fn() Int { x }   // captures x = 1

x = 2                 // new binding shadows the old

f()                  // returns 1
```

This follows directly from capture-by-value semantics.

---

#### 7.7.4 Closures Cannot Rebind Captured Variables

Because closures capture **values**, not cells, they cannot assign to variables defined outside their own function.

The following is an error:

```tw
x := 1

fn bad() {
  x = x + 1   // error: cannot rebind variable defined in outer scope
}
```

Compile-time rule:

> A closure may reference captured variables, but may **not** rebind them using `=`.

If mutation-like behavior is desired in the future, it must be expressed explicitly using a type such as `Cell<T>` rather than via closures.

---

#### 7.7.5 Loop Variables Produce Fresh Bindings per Iteration

In `for` loops, loop variables are newly bound for each iteration.
Thus each closure created inside the loop captures the **iteration’s** value, not a shared accumulator.

```tw
fns := collect i in range(3) {
  fn() Int { i }
}

fns[0]()    // 0
fns[1]()    // 1
fns[2]()    // 2
```

Rationale:

* Each iteration introduces a new binding `i`.
* Closures capture the value of `i` at their own definition point.

This avoids common “loop capture traps” seen in other languages.

---

#### 7.7.6 Summary of Closure Capture Rules

| Behavior                  | Rule                                                                                        |
| ------------------------- | ------------------------------------------------------------------------------------------- |
| What is captured?         | The **value** of each free variable at closure definition time.                             |
| Shadowing                 | Closures capture the **innermost visible** binding.                                         |
| Rebinding afterwards      | Does **not** affect existing closures; it creates a new shadowing binding.                  |
| Assigning inside closures | Cannot rebind captured variables (compile-time error).                                      |
| Loops                     | Fresh binding per iteration; closures capture the iteration’s value.                        |
| Mutation                  | Not supported implicitly; must use explicit types (e.g. future `Cell<T>`) for shared state. |

This model is simple, predictable, and strictly functional in semantics, while still supporting direct rebinding syntax.

---

## 8. Modules & Imports

### 8.1 Top-Level Items

A Twinkle source file is a module. The following items are allowed at the top level, in any order:

* **Type declarations** (`type`) — define nominal record or enum types.
* **Function declarations** (`fn`) — define named functions.
* **Value bindings** (`:=` or `: T =`) — module-level names bound to values.
* **Expression statements** — side-effecting expressions (must be `Void`).

#### Value bindings

```tw
PI: Float = 3.14159
MAX_RETRIES := 5
```

Module-level value bindings are **module globals**:

* Their names are in scope for all functions in the module, regardless of source order.
* They can be marked `pub` to export them.
* They are evaluated once at module initialization time, top-to-bottom.
* Rebinding (`=`) is not allowed at module scope — each name may only be bound once.

#### Expression statements

```tw
println("module loaded")
```

Top-level expression statements execute at initialization time and introduce no name. They must have type `Void`. They run in source order, interleaved with value binding evaluation.

#### Initialization order

Type and function declarations are available everywhere in the module (no forward-declaration restriction). Value bindings and expression statements execute top-to-bottom in the order they appear.

#### Entry point

The entry point of a program is its top-level initialization sequence. There is no special `main` function — `fn main()` has no distinguished status and is not called automatically.

To run code, place it at the top level:

```tw
// top-level expressions are the program
println("hello")
```

For larger programs, define a function and call it from the top level:

```tw
fn run() Void {
  // ...
}

run()
```

A module with no top-level expression statements is a library module; its value and function exports are available to importers.

#### WebAssembly entry point

When compiling to WebAssembly, the top-level initialization sequence is lowered into a synthetic `__init__` function. This function is designated as the Wasm [start function](https://webassembly.github.io/spec/core/syntax/modules.html#start-function), so it runs automatically when the module is instantiated by the host (browser, Wasmtime, etc.).

There is no exported `main` symbol. If a host needs to call a specific function by name (e.g. for embedding), export it explicitly:

```tw
pub fn run() Void {
  // ...
}

run()   // also runs at startup via __init__
```

The host can then call `run()` again on demand via the Wasm export.

### 8.2 Imports

> **Design rationale:** See [docs/module.md](module.md) entries D-001 through D-009.

#### Syntax

```tw
use foo.bar           // import foo/bar.tw, bound as "bar"
use foo.bar as baz    // import foo/bar.tw, bound as "baz"
use utils             // import utils.tw at project root
use @array            // stdlib module, bound as "array"
use @std.json as json // stdlib module with alias
```

#### Filesystem mapping

A dot-separated path `a.b.c` maps to `<root>/a/b/c.tw`. The module identifier is
the last segment (`c`), or the alias if `as name` is provided.

#### Project root resolution

1. Walk up from the entry file's directory until `twinkle.toml` is found.
2. `TWINKLE_ROOT` environment variable overrides with an absolute path.
3. If neither is found, the entry file's directory is the root (single-file scripts).

#### Stdlib modules

Stdlib modules are prefixed with `@`. The prelude (primitive types, `println`,
`Array`, `Dict`, `String`, `Range`, etc.) is always implicitly in scope — no
`use` needed. Richer stdlib modules require an explicit `use @...`.

#### Aliasing

`use foo.bar as baz` binds the module under `baz`. Aliasing is required when two
imports share the same last-segment name:

```tw
use math.vector as mvec
use graphics.vector as gvec
```

Importing two modules with the same identifier without `as` is a compile-time error.

#### Visibility

```tw
pub fn foo() Int { ... }
fn bar() Int { ... }      // private

pub PI: Float = 3.14159   // exported constant
```

Exported names are accessed qualified: `math.add`, `math.Point`.

Separate namespaces exist for values and types — a module may export both a type
and a value with the same name; they are distinguished by context and never conflict
(e.g. `option.Option<T>`, `option.Some`, `option.None`).

#### Re-exports

No special re-export syntax. Use explicit `pub` rebinding:

```tw
use math.vector
pub translate := vector.translate
```

#### Circular imports

Circular imports are a compile-time error.

#### Destructuring (future, not in MVP)

Bringing specific names directly into scope is not supported in MVP. Future syntax:

```tw
use math.vector.{translate, scale}
use math.vector.{translate as tr, scale}

// to both alias the module and destructure, use two statements:
use math.vector as vec
use math.vector.{translate, scale}
```

Wildcard imports (`use foo.*`) will never be supported.

---

## 9. Inherent Methods (Module-Based)

Twinkle’s dot syntax only supports:

1. **Record fields**
2. **Inherent/module methods**

A module may associate functions with a type by making them first-argument style:

```tw
// point.tw
pub type Point = .{ x: Int, y: Int }

pub fn translate(p: Point, dx: Int, dy: Int) Point {
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

### Built-in inherent methods

Some built-in types define compiler-known inherent methods.

#### Length

Length is exposed only via an inherent method:

```tw
value.len()
```

Defined for:

* `Array<T>.len() Int` — number of elements
* `String.len() Int` — length of the string
* `Dict<K,V>.len() Int` — number of entries

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

* The type system rank-1 polymorphic (Damas–Milner) and simple,
* The compiler free from trait solvers, global coherence checks, and complex instance resolution.

---

## **10.2 Capabilities via Records of Functions**

A capability is a nominal type that captures a set of operations on some data type `T`.

**Example: Show capability**

```tw
type Show<T> = .{
  to_string: fn(T) String,
}
```

A function that needs "anything that can be shown" is written by taking both:

* the value(s),
* and a corresponding capability record.

**Example: Generic printing**

```tw
fn print_all<T>(xs: Array<T>, show: Show<T>) {
  for x in xs {
    println(show.to_string(x))
  }
}
```

**Usage:**

```tw
type User = .{ name: String, age: Int }

fn show_user(u: User) String {
  // yep, this is comment
  // twinkle will only have single line comment
  // twinkle currently doesn't support `s1 + s2`
  // string concat pattern, so we use string interpolation here
  "${u.name}(${u.age})"
}

ShowUser: Show<User> = .{
  to_string: show_user,
}

users: Array<User> = [...]
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
* No automatic rewriting of `Array<T>` into `Array<Show<T>>`,
* No chained or inferred conversions.

All adapter logic, if any, is explicit in user code.

---

## **10.4 Common Capability Patterns**

### Equality and Ordering

```tw
type Eq<T> = .{
  equals: fn(T, T) Bool,
}

fn contains<T>(xs: Array<T>, needle: T, eq: Eq<T>) Bool {
  for x in xs {
    if eq.equals(x, needle) {
      return true
    }
  }
  false
}

type Point = .{ x: Int, y: Int }

EqPoint: Eq<Point> = .{
  equals(a, b) => a.x == b.x && a.y == b.y,
}

points: Array<Point> = [...]
p: Point = .{ x: 1, y: 2 }
found := contains(points, p, EqPoint)
```

### Collection-Specific Helpers

Instead of a general "Iterable" trait, provide small, concrete helpers:

```tw
fn sum_array(xs: Array<Int>) Int {
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

* `String` — used as-is,
* `Int`    — converted via a core `String.of_int` function,
* `Float`  — converted via `String.of_float`,
* `Bool`   — converted via `String.of_bool`.

Attempting to interpolate a value of any other type is a **compile-time error**.

### Example

```tw
name: String = "Twinkle"
n: Int = 42
ok: Bool = true

s := "name=${name}, n=${n}, ok=${ok}"  // ✅ ok

type User = .{ name: String, age: Int }
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
String.concat_many([
  "n=",
  String.of_int(n),
])
```

(Canonical surface names use the `String.*` module namespace.)

### Extension: Explicit Conversion Functions

To interpolate user-defined types, users write **explicit conversion functions** and use them inside the interpolation expression:

```tw
type User = .{ name: String, age: Int }

fn user_to_string(u: User) String {
  "${u.name} (${u.age})"
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

All `for` loops are statements returning `Void`.

Forms:

```tw
for cond { body }

for x in coll { body }

for x,i in coll { body }
```

**Supported Collection Types:**

The `for x in coll` syntax is supported only for a **closed set** of built-in collection types:

* `Array<T>` — homogeneous indexable arrays,
* `Range`    — integer ranges (e.g. `0..10`),
* `Dict<K, V>` — dictionaries (iterates over key-value pairs),
* `Iterator<T>` — an explicit iterator type from the standard library.

The compiler performs a **type-directed** lowering:

* If `coll` has type `Array<T>`, the loop is lowered to an indexed loop over the array length.
* If `coll` has type `Range`, the loop is lowered to a simple integer loop over the range bounds.
* If `coll` has type `Dict<K, V>`, the loop is lowered to iteration over key–value pairs.
* If `coll` has type `Iterator<T>`, the loop is lowered to repeated `next` calls until the iterator is exhausted.

Any value used in `for x in coll` whose type is not one of the supported built-ins is a **compile-time error**.

**Indexed form:**

* `i: Int` starts from 0 and increments each iteration.
* Independent of the underlying iterator state.
* Break/continue as usual.

**User Extensions:**

To iterate over a custom type, users define a **helper function** that produces a built-in collection or iterator:

```tw
// tree.tw
type Tree<T> = ...

pub fn iter<T>(t: Tree<T>) Iterator<T> {
  // implementation creates an Iterator<T> over the tree
}

// usage
fn sum_tree(t: Tree<Int>) Int {
  acc := 0
  for x in t.iter() {    // desugars to: tree.iter(t)
    acc = acc + x
  }
  acc
}
```

### Diverging expressions

Some expressions do not complete normally, for example:

* `return expr`
* `error("message")`
* infinite loops (e.g. `loop { ... }`)

Such expressions are allowed in any expression position.

When type-checking an expression with multiple branches (e.g. `if` or `case`), branches that do not complete normally do not affect the resulting type. The type of the whole expression is determined only by branches that complete normally.

```tw
x := case opt {
  .Some(v) => v,
  .None => return {},
}
```

Here the `.None` branch never returns, so the `case` expression has the type of the `.Some` branch.

---

## 13. Collect Comprehension

```tw
xs := collect x in range(10) { x * x }
```

Rules:

* Produces `Array<T>`.
* Works with the same built-in collection types as `for` loops (see Section 12).
* `continue` skips emission.
* `break` ends early, returns partial array.
* If the body returns `Void` → error, because collect expects a value to push.
* The element type is inferred as the type of the body expression; all iterations must unify to same type; otherwise type error.

Example:

```tw
squares := collect x in range(1, 10) { x * x }
// squares: Array<Int> = [1, 4, 9, 16, 25, 36, 49, 64, 81]

evens := collect x in range(1, 20) {
  if x % 2 == 0 { x } else { continue }
}
// evens: Array<Int> = [2, 4, 6, 8, 10, 12, 14, 16, 18]
```

---

## 14. Arrays

Arrays are **immutable** sequences.

`arr[i]` indexing, 0-based (read-only access).

Array operations via module functions (all return new arrays):

* `Array.set(arr, index, value) Array<T>` — returns new array with element at index replaced
* `Array.append(arr, value) Array<T>` — returns new array with value appended
* `Array.concat(arr1, arr2) Array<T>` — returns new array combining both
* `Array.slice(arr, start, end) Array<T>` — returns new array with subset of elements
* `Array.len(arr) Int` — returns length of array
* etc.

Array literals:

```tw
[1, 2, 3]  // Array<Int>

xs: Array<Int> = []  // empty array requires type annotation
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
arr = Array.set(arr, i, value)
```

---

## 15. Strings

Strings are **immutable**.

`str.len()` returns the length of the string.

String interpolation is recommended for string assembly (see Section 11).

String operations via module functions (all return new strings):

* `String.concat(s1, s2) String`
* `String.substring(s, start, end) String`
* `String.of_int(n: Int) String`
* `String.of_float(f: Float) String`
* `String.of_bool(b: Bool) String`
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
m: Dict<String, Int> = Dict.new()
```

Type parameters are inferred from the annotation.

Dict operations via module functions (all return new dicts):

* `Dict.set(m, k, v) Dict<K, V>` — returns new dict with key-value pair added/updated
* `Dict.remove(m, k) Dict<K, V>` — returns new dict with key removed
* `Dict.get(m, k) V?` — returns Option<V> for safe access
* `Dict.has(m, k) Bool` — checks if key exists
* `Dict.keys(m) Array<K>` — returns array of keys
* `Dict.len(m) Int` — returns length of keys

Indexing syntax:

* `m[k]` returns `V?` (Option<V>) for safe read access
* `m[k] = v` desugars to `m = Dict.set(m, k, v)`

---

## 18. Error Handling

No exceptions.

Unrecoverable = trap:

* OOB
* division by zero
* explicit `error("msg")`

Recoverable via `Result<T,E>`:

```tw
type Result<T, E> = { Ok(T), Err(E) }
```

`try` sugar:

```tw
try expr
```

* Only for `Result<T,E>`.
* Returns early with `Err(e)` on error.
* For `Result<Void,E>` the `Ok` branch carries no value.
* `.Ok({})` is the way to present `Void` return for `Result`, as `{}` evals to `Void`.
* Cannot be applied to non-Result types (compile-time error).

---

## 19. Prelude

Implicitly imported.

Includes:

* primitive functions: `print`, `println`, `error`
* types: `Int`, `Float`, `String`, `Bool`, `Void`, `Array<T>`, `Dict<K,V>`, `Option<T>`, `Result<T,E>`
* range functions: `range`
* array module: `Array.set`, `Array.append`, `Array.concat`, etc.
* dict module: `Dict.new`, `Dict.set`, `Dict.get`, etc.
* string module: `String.concat`, `String.substring`, `String.of_int`, etc.
* naming convention: public surface APIs are PascalCase modules/types; internal compiler/runtime intrinsics may use snake_case.
* stage0 compatibility aliases may still exist (e.g. `int_to_string`); prefer `String.*` names in user-facing docs.

---

## 20. Type System and Checking

### Type System

Twinkle has a rank-1 polymorphic (Damas–Milner) type system: unification-based, principal types, no higher-ranked quantification, no trait constraints.

### Type Checking

Type checking is bidirectional:

* Most expressions **synthesize** a type bottom-up (classic HM inference).
* Certain expressions are **checked** against an expected type from context.

Expressions that require contextual type information:

* **Anonymous record literals** (`.{ ... }`) — the expected record type must be known from the surrounding context.
* **Annotated bindings** (`x: T = e`) — `e` is checked against `T` rather than synthesized.
* **Function arguments** — the argument is checked against the declared parameter type.

This is the same approach used by Gleam, Elm, and modern OCaml. The type system (what is typable) is unchanged from Damas–Milner; only the inference procedure is bidirectional rather than pure Algorithm W.

### Generalization Rules

1. **`fn` declarations are generalized** — type variables in the signature are universally quantified:
   ```tw
   fn id<A>(x: A) A { x }   // polymorphic; A is generic
   ```

2. **`:=` bindings are monomorphic** — the inferred type is instantiated to a specific monotype at the binding site:
   ```tw
   f := id     // error: cannot infer monomorphic type for polymorphic binding
               // help: annotate, e.g.  f: fn(Int) Int = id
   ```

3. **Type-annotated bindings** (`x: T = e`) use the annotation directly with no generalization.

This avoids value-restriction complexity and keeps local bindings simple. If you need a name for a polymorphic function, define it with `fn`.

Capabilities are ordinary values (records of functions), so they participate in normal type inference without special rules.

String interpolation is type-checked by verifying the expression type is one of: `String`, `Int`, `Float`, `Bool`.

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
* String interpolation → compiler inserts calls to `String.of_int`, `String.of_float`, `String.of_bool`
* For loops → type-directed lowering to primitive loops based on collection type

---

## 22. Error Messages

Examples:

**Invalid string interpolation**:

```
error: cannot interpolate value of type SocialPost
note: string interpolation only supports String, Int, Float, and Bool
help: consider using an explicit conversion function: "${post_to_string(post)}"
```

**No inherent method**:

```
error: no method 'translate' for type Point
note: dot syntax only resolves record fields and inherent methods from the defining module
```

**Invalid for loop collection**:

```
error: cannot iterate over value of type Tree<Int>
note: for loops only support Array<T>, Range, Dict<K,V>, and Iterator<T>
help: consider defining a helper function that returns Iterator<Int>
```

**Mutation attempt on non-name**:

```
error: cannot update expression that is not an assignable lvalue
note: only identifiers, field accesses, or indexed expressions can appear to the left of '='
help: bind to a local variable first if you need to reuse a computed value: 'tmp := foo(); tmp.x = 1'
```

---
