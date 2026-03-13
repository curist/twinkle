# 🌟 **Twinkle Language Specification**

## 1. Overview

Twinkle is a small statically typed language targeting **WebAssembly GC**.

Design goals:

* Concise, low-ceremony syntax.
* Rank-1 polymorphic (Damas–Milner) type system with bidirectional type checking.
* Unboxed primitives (`Int = i64`, `Float = f64`, `Bool = i32`, `Byte = i32 (0..255)`).
* GC-managed references for strings, arrays, records, dicts, and cells.
* Small runtime; rely on `struct`, `array`, reference types.
* Inherent methods only via module functions.
* Immutable values with rebindable names.
* No trait system; capabilities via records of functions.

Source files end with `.tw`.

---

## 2. Value Model

### Immutability

**All ordinary values in Twinkle are immutable.**

* Primitives, strings, arrays, records, dicts, and functions cannot be mutated in place.
* There is no observable in-place mutation of values in the language model.
* Updates are expressed through rebinding: constructing a new value and binding a name to it.
* Shared mutable state is explicit and only available through `Cell<T>` APIs (`Cell.set`, `Cell.update`).

### Primitives (unboxed)

* `Int` → wasm `i64`
* `Float` → wasm `f64`
* `Bool` →  wasm `i32`, 0/1
* `Byte` → wasm `i32`, range `0..255`
* `Void` → effect-only (no value).

### References (GC)

* `String` — immutable text.
* `Vector<T>` — immutable GC array; element unboxed/ref depending on `T`.
* `record` — immutable closed struct shape.
* `Dict<K,V>` — immutable hash map reference.
* `function` — closure with captured environment (GC).
* `Cell<T>` — mutable cell reference for explicit shared state.

### `Void`

* Used as function return type & block with no final expression.
* No literal and cannot be stored/bound.

---

## 3. Types & Generics

Parametric polymorphism:

```tw
fn map<A, B>(xs: Vector<A>, f: fn(A) B) Vector<B> { ... }
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

`T?` composes with `!E` (see §18):

```
T?!E  ==  Result<Option<T>, E>
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

Functions cannot mutate caller-visible ordinary values via assignment syntax.
All assignment-like updates create new values and rebind local names. Side effects are explicit (e.g. `print`, `println`, `error`, `Cell.set`, `Cell.update`).

Function declaration parameters must be explicitly annotated (`fn f(x: Int) ...`).
Parameters are ordinary local bindings and may be rebound within the function body (see §7.3).

The return type is written after the parameter list (no `->`). It may be omitted when inference suffices; when omitted, the function body’s value determines the return type.

For **function expressions** (`fn (...) { ... }`) used as callbacks, parameter and return types may be omitted when a contextual function type is available (for example, from a function parameter type or an annotated binding). If explicit callback annotations are present, they must agree with that contextual type.

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

1. `x = expr` is only legal if `x` refers to an existing binding in an enclosing lexical scope **within the same function**.
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

Desugars conceptually to the core record-update operation (not Twinkle surface syntax):

```tw
r = RecordUpdate(r, field, expr)
```

#### Vector index update

```tw
arr[i] = value
```

Desugars to:

```tw
arr = Vector.set_unsafe(arr, i, value)
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
// desugars conceptually to:
a = RecordUpdate(a, b, RecordUpdate(a.b, c, x))
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
Closures capture values. If a captured value is a `Cell<T>`, the closure captures that cell reference value, so cell effects remain shared.

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

Because rebinding always targets local bindings in the current function, closures cannot assign to variables defined outside their own function.

The following is an error:

```tw
x := 1

fn bad() {
  x = x + 1   // error: cannot rebind variable defined in outer scope
}
```

Compile-time rule:

> A closure may reference captured variables, but may **not** rebind them using `=`.

If shared mutable state is desired, express it explicitly using `Cell<T>` rather than by rebinding captured variables.

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
| Mutation                  | Not supported implicitly; shared mutable state must use explicit `Cell<T>` operations.        |

This model is simple and predictable, while still supporting direct rebinding syntax.

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

> **Design rationale:** See [docs/design/module.md](design/module.md).

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
`Vector`, `Dict`, `String`, `Range`, etc.) is always implicitly in scope — no
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

* `Vector<T>.len() Int` — number of elements
* `String.len() Int` — length of the string
* `Dict<K,V>.len() Int` — number of entries

#### Conversion

String conversion is exposed via inherent `.to_string()` methods.

Defined for:

* `Int.to_string() String`
* `Float.to_string() String`
* `Bool.to_string() String`
* `Byte.to_string() String`
* `String.to_string() String` (identity)

Additional numeric conversion helpers are available via stdlib extension
and built-in modules:

* `Int.to_float() Float` / `Int.to_float(n) Float`
* `Float.to_int() Int` / `Float.to_int(f) Int`
* `Byte.to_int() Int` / `Byte.to_int(b) Int`
* `Byte.from_int(n: Int) Option<Byte>`

#### Parsing

Parsing from strings to numeric types uses type-qualified constructors and returns
`Option<T>`:

* `Int.from_string(s: String) Option<Int>` — parses a decimal integer (optional `+`/`-` prefix)
* `Float.from_string(s: String) Option<Float>` — parses a floating-point number

The compiler/runtime may implement these through internal parsing intrinsics, but
the raw intrinsic names are not part of the user-facing language.

```tw
case Int.from_string("42") {
  .Some(n) => println("${n}"),   // prints 42
  .None => println("not a number"),
}
```

#### String ordering

Strings support lexicographic comparison via the standard relational operators:

```tw
"a" < "b"    // true
"abc" < "abcd"  // true (prefix is less)
"abc" <= "abc"  // true
```

These comparisons use byte-level lexicographic ordering (UTF-8).

#### Character utilities

* `String.char_code_at(s: String, i: Int) Int` — returns the byte value at byte offset `i` (0-based); compatibility alias for `Byte.to_int(s[i])`
* `String.from_char_code(n: Int) Option<String>` — converts an ASCII code (0–127) to a single-byte/single-character string; returns `None` for values outside that range

```tw
String.char_code_at("abc", 0)  // 97 (ASCII 'a')

case String.from_char_code(97) {
  .Some(s) => println(s),  // prints "a"
  .None => println("invalid"),
}
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
fn print_all<T>(xs: Vector<T>, show: Show<T>) {
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

users: Vector<User> = [...]
print_all(users, ShowUser)
```

**Key points:**

* The compiler does **not** invent or find `Show<User>` automatically.
* The call site is always **explicit** about which capability record is passed.

---

## **10.3 No Implicit Conversions**

Twinkle does **not** perform implicit conversions to satisfy capability records.
This also means ordinary function calls never apply silent argument coercions.

Built-in numeric operators do define explicit typing/promotion rules (for example,
`Byte` arithmetic yields `Int`), but those rules are part of operator semantics,
not general implicit conversions.

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
* No automatic rewriting of `Vector<T>` into `Vector<Show<T>>`,
* No chained or inferred conversions.

All adapter logic, if any, is explicit in user code.

---

## **10.4 Common Capability Patterns**

### Equality and Ordering

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

type Point = .{ x: Int, y: Int }

fn point_equals(a: Point, b: Point) Bool {
  a.x == b.x && a.y == b.y
}

EqPoint: Eq<Point> = .{
  equals: point_equals,
}

points: Vector<Point> = [...]
p: Point = .{ x: 1, y: 2 }
found := contains(points, p, EqPoint)
```

### Collection-Specific Helpers

Instead of a general "Iterable" trait, provide small, concrete helpers:

```tw
fn sum_array(xs: Vector<Int>) Int {
  acc := 0
  for x in xs {
    acc = acc + x
  }
  acc
}
```

User types that want to participate reuse these helpers by returning supported built-ins (e.g. `Vector<T>` or `Range`) from explicit conversion functions.

---

## 11. String Literals and Interpolation

### String Literal Escapes

Twinkle string literals support these escapes:

* `\n` newline
* `\t` tab
* `\r` carriage return
* `\"` double quote
* `\\` backslash
* `\$` literal `$` (suppresses interpolation start)
* `\xNN` exactly two hex digits (`N`), ASCII-only (`00..7F`)
* `\e` escape character (`U+001B`, equivalent to `\x1b`)
* `\u{...}` Unicode scalar escape with 1 to 6 hex digits

`"\x1b[31mred\x1b[0m"` is valid and can be used for ANSI control sequences.
`"\u{1F44D}"` is valid and produces `👍`.

`\u{...}` must decode to a valid Unicode scalar value:

* surrogate range values (`D800..DFFF`) are rejected
* values above `10FFFF` are rejected

Migration note: code that previously built ESC with `String.from_char_code(27)` can
be simplified to `"\e"` or `"\x1b"` in literals.

### String Interpolation

String interpolation uses:

```tw
"hello ${x}"
```

Interpolation is **not** driven by a capability or trait. It is defined in terms of
inherent `to_string` methods.

### Supported Conversion Rule

For each `${expr}`, the compiler resolves a zero-argument `to_string` method on the
type of `expr` with return type `String`.

Built-in support exists for:

* `String` — identity
* `Int` — decimal rendering
* `Float` — float rendering
* `Bool` — `true` / `false`

User-defined named types are interpolable when they define an inherent method:

```tw
fn to_string(x: MyType) String { ... }
```

If no valid `to_string() String` is available, interpolation is a compile-time error.

### Example

```tw
name: String = "Twinkle"
n: Int = 42
ok: Bool = true

s := "name=${name}, n=${n}, ok=${ok}"  // ✅ ok

type User = .{ name: String, age: Int }
fn to_string(u: User) String { "${u.name} (${u.age})" }
user: User = .{ name: "Ada", age: 30 }
s2 := "user=${user}"                    // ✅ ok (uses User.to_string())
```

### Explicit `to_string` Calls

Explicit method calls are valid inside interpolation and outside it:

```tw
println("${1.5.to_string()}")   // ✅ explicit call on Float literal

f := 1.5
println("${f.to_string()}")     // ✅ explicit call on identifier
```

Unary-minus literals must be parenthesized before method calls:

```tw
println("${(-1).to_string()}")  // ✅
// println("${-1.to_string()}") // parsed as -(1.to_string()), so this is invalid
```

### Desugaring

String literals with interpolation are desugared into string concatenation with method calls.

For example:

```tw
"n=${n}"
```

is conceptually lowered to:

```tw
"n=".concat(n.to_string())
```

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

The `for x in coll` syntax supports the following types, each with dedicated type-directed lowering:

* `Vector<T>` — lowered to an indexed loop over the array length.
* `String` — lowered to an indexed loop over UTF-8 bytes (`str[i]`).
* `Range` — lowered to a simple integer loop over the range bounds.
* `Dict<K, V>` — lowered to iteration over key–value pairs.
* `Iterator<T>` — lowered to repeated `Iterator.next` calls (see [docs/design/iterator.md](design/iterator.md)).

Any value used in `for x in coll` whose type is not one of the above is a **compile-time error**.

**Indexed form:**

* `i: Int` starts from 0 and increments each iteration.
* Break/continue as usual.
* The indexed form (`for x, i in coll`) is supported for `Vector<T>`, `String`, `Range`, and `Dict<K,V>`. It is not supported for `Iterator<T>`.

**User Extensions:**

To iterate over a custom type, either:

1. Define a helper that returns a supported built-in collection (`Vector<T>`, `Range`), or
2. Define a helper that returns `Iterator<T>` using `Iterator.unfold` (see [docs/design/iterator.md](design/iterator.md)).

```tw
// Option 1: convert to Vector
fn sum_tree(t: Tree<Int>) Int {
  acc := 0
  for x in t.to_vector() {
    acc = acc + x
  }
  acc
}

// Option 2: return Iterator<T>
fn tree_iter<T>(t: Tree<T>) Iterator<T> {
  Iterator.unfold(/* ... */)
}

for x in tree_iter(my_tree) { ... }
```

### Diverging expressions

Some expressions do not complete normally, for example:

* `return expr`
* `error("message")`
* infinite loops (e.g. `for true { ... }`)

Such expressions are allowed in any expression position.

When type-checking an expression with multiple branches (e.g. `if` or `case`), branches that do not complete normally do not affect the resulting type. The type of the whole expression is determined only by branches that complete normally.

```tw
x := case opt {
  .Some(v) => v,
  .None => return {},
}
```

Here the `.None` branch never returns, so the `case` expression has the type of the `.Some` branch.

### `defer`

`defer expr` schedules an expression to run when the **enclosing block** exits. It is a statement — it produces no value.

```tw
fn write_file(path: String, data: String) !IoError {
  f := try open(path)
  defer { close(f) }      // runs however write_file exits (except trap)
  try write(f, data)
}
```

**Scope:** `defer` is tied to the nearest enclosing `{ ... }` block, not the function. A `defer` inside a loop body runs at the end of **each iteration**:

```tw
for x in xs {
  defer { println("done with ${x}") }   // runs once per iteration, and on break
}
```

**Ordering:** multiple defers in the same block execute LIFO (last declared, first run):

```tw
{
  defer { println("1") }
  defer { println("2") }
  defer { println("3") }
}
// prints: 3, 2, 1
```

**Capture:** variables referenced in a `defer` are captured by value at declaration time, consistent with closure semantics:

```tw
x := 1
defer { println("x was ${x}") }   // captures x = 1
x = 2
// prints: x was 1
```

**Triggers:** normal completion, `return` (unwinds all enclosing blocks), `break`, `continue`, and `try`-propagated `Err`.

**Does not trigger on traps** (`error()`, out-of-bounds, division by zero). Traps are unrecoverable — no cleanup is possible.

**Type:** no constraint on the deferred expression; the result is silently discarded.

> **Implementation note:** `defer` is not implemented until Stage 7.6 (after CFG). At the CFG level it desugars completely via edge insertion — zero runtime overhead. See [docs/design/defer.md](design/defer.md).

---

## 13. Collect Comprehension

```tw
xs := collect x in range(10) { x * x }
ys := collect x, i in range(10) { x + i }
zs := collect n < 10 { n }
```

Rules:

* Produces `Vector<T>`.
* Works with the same collection types as `for` loops (see Section 12): `Vector<T>`, `String`, `Range`, `Dict<K,V>`, and `Iterator<T>`.
* Also supports conditional form `collect cond { body }`:
  * `cond` must be `Bool`.
  * Evaluates like a `while` loop and collects values produced by `body`.
* Supports indexed/binary form `collect x, i in coll { ... }` for `Vector<T>`, `String`, `Range`, and `Dict<K,V>`:
  * For `Vector<T>`, `String`, and `Range`, `i: Int` is the iteration index.
  * For `String`, `x: Byte` (byte iteration).
  * For `Dict<K,V>`, the second binder has type `V` (value), while the first binder is key `K`.
  * `Iterator<T>` does not support the two-binder form.
* `continue` skips emission.
* `break` ends early, returns partial array.
* If the body returns `Void` → error, because collect expects a value to push.
* The element type is inferred as the type of the body expression; all iterations must unify to same type; otherwise type error.

Example:

```tw
squares := collect x in range(1, 10) { x * x }
// squares: Vector<Int> = [1, 4, 9, 16, 25, 36, 49, 64, 81]

evens := collect x in range(1, 20) {
  if x % 2 == 0 { x } else { continue }
}
// evens: Vector<Int> = [2, 4, 6, 8, 10, 12, 14, 16, 18]
```

---

## 14. Vectors

Vectors are **immutable** sequences (`Vector<T>`).

`vec[i]` indexing, 0-based (traps on out-of-bounds).

Vector operations via method or module syntax:

* `vec.len() Int` / `Vector.len(vec) Int` — number of elements
* `vec.push(value) Vector<T>` — returns new vector with value appended
* `vec.concat(other) Vector<T>` / `Vector.concat(a, b) Vector<T>` — concatenate two vectors
* `vec.slice(start, end) Vector<T>` / `Vector.slice(vec, start, end) Vector<T>` — subset `[start, end)`
* `vec.get(i) Option<T>` — safe index access; returns `None` if out of bounds
* `vec.set(i, val) Option<Vector<T>>` — safe functional update; returns `None` if out of bounds
* `Vector.make(size, fill) Vector<T>` — create a vector of `size` elements all equal to `fill`

Unsafe index write (traps on out-of-bounds):

```tw
vec[i] = value
```

Desugars to:

```tw
vec = Vector.set_unsafe(vec, i, value)
```

Vector literals:

```tw
[1, 2, 3]  // Vector<Int>

xs: Vector<Int> = []  // empty vector requires type annotation
```

If context can't determine element type => compiler error.

```tw
[x, y, z]  // all elements must have the same type
```

---

## 15. Strings

Strings are **immutable** and always valid UTF-8.

`str.len()` returns UTF-8 byte length.

`str[i]` returns a `Byte` at byte offset `i` (0-based). Out-of-bounds access traps.

String interpolation is recommended for string assembly (see Section 11).

String operations via module functions (all return new strings):

* `String.concat(s1, s2) String`
* `String.slice(s, start, end) String` (byte range `[start, end)`; traps if indices are out of bounds or not UTF-8 scalar boundaries)
* `String.get(s, i) Byte?` (safe byte index; returns `None` if out-of-bounds)
* `String.char_code_at(s, i) Int` (compatibility alias for byte-at-offset as `Int`)
* `String.to_string(s) String` (identity helper; `s.to_string()` is preferred)
* `s.chars() Iterator<String>` — iterate Unicode scalar values (each yielded as a 1–4 byte `String`)
* `s.char_len() Int` — number of Unicode scalars (O(n))
* `s.graphemes() Iterator<String>` — iterate extended grapheme clusters (user-perceived characters); handles combining marks, ZWJ emoji sequences, and regional indicator flags via a simplified UAX #29 implementation
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

**Key type constraint:** `K` must be `Int` or `String`. No other types are allowed
as dict keys. `Bool` keys are excluded (a two-entry dict should be expressed as a
plain record). Since Twinkle has no trait system, this constraint is enforced as a
compiler-known closed set rather than a generic bound.

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
* `Dict.keys(m) Vector<K>` — returns array of keys
* `Dict.values(m) Vector<V>` — returns array of values (via `@std.dict_ext`)
* `Dict.len(m) Int` — returns length of keys

Indexing syntax:

* `m[k]` returns `V?` (Option<V>) for safe read access
* `m[k] = v` desugars to `m = Dict.set(m, k, v)`

### 17.1 Cell (Explicit Mutable State)

`Cell<T>` is an opaque mutable container type for explicit shared state.

Core API:

* `Cell.new(v: T) Cell<T>` — allocate a new cell
* `Cell.get(c: Cell<T>) T` — read current value
* `Cell.set(c: Cell<T>, v: T) Void` — write current value (side effect)
* `Cell.update(c: Cell<T>, f: fn(T) T) Void` — read/transform/write (side effect)

`Cell` does not change update-sugar semantics:

* `x.y = v` still means record rebuild + rebinding of `x`.
* `arr[i] = v` still means `arr = Vector.set_unsafe(arr, i, v)`.
* `m[k] = v` still means `m = Dict.set(m, k, v)`.

If multiple names refer to the same `Cell<T>`, updates through one name are visible through the others.

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

**Type shorthand:**

```
T!E   ==  Result<T, E>       // full form
!E    ==  Result<Void, E>    // operation that can fail with no return value
```

`T!` and bare `!` are **not** valid — the error type is always required.
`T?!E` composes naturally: `Option<T>!E` == `Result<Option<T>, E>`.

Examples:

```tw
fn validate(n: Int) !ParseError { ... }          // Result<Void, ParseError>
fn parse(s: String) Int!ParseError { ... }       // Result<Int, ParseError>
fn find(xs: Vector<Int>, k: Int) Int?!String { ... }  // Result<Option<Int>, String>
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
* types: `Int`, `Float`, `Byte`, `String`, `Bool`, `Void`, `Vector<T>`, `Dict<K,V>`, `Cell<T>`, `Option<T>`, `Result<T,E>`, `Iterator<T>`, `IterItem<T>`, `UnfoldStep<T,S>`
* range functions: `range`, `range_from`, `range_step`
* vector module: `Vector.make`, `Vector.len`, `Vector.concat`, `Vector.slice`, `Vector.get`, `Vector.set`, etc.
* dict module: `Dict.new`, `Dict.set`, `Dict.get`, etc.
* cell module: `Cell.new`, `Cell.get`, `Cell.set`, `Cell.update`
* string module: `String.concat`, `String.slice`, `String.get`, `String.char_code_at`, `String.from_char_code`, `String.to_string`, `s.chars()`, `s.char_len()`, `s.graphemes()`, etc.
* byte module: `Byte.to_int`, `Byte.from_int`, `Byte.to_string`
* iterator module: `Iterator.next`, `Iterator.unfold`, `Iterator.to_vector` (see [docs/design/iterator.md](design/iterator.md)). `to_vector` materializes the full iterator into a `Vector<T>` (equivalent to `collect x in it { x }`). Infinite iterators will not terminate; O(n) memory.
* naming convention: public surface APIs are PascalCase modules/types; internal compiler/runtime intrinsics use snake_case and are **not part of the user-visible language**.

---

## 20. Naming Conventions

Twinkle enforces naming conventions **at the parser level** — they are not style
lint but hard syntax rules. The parser uses the first character of an identifier
to determine what it can mean.

### Summary

| Thing | Convention | Example |
|---|---|---|
| Types | `PascalCase` | `Point`, `Option`, `HttpRequest` |
| Enum variants | `PascalCase` | `None`, `Ok`, `SomeLongName` |
| Functions | `snake_case` | `parse_int`, `to_string` |
| Local variables | `snake_case` | `result`, `my_count` |
| Record fields | `snake_case` | `x`, `name`, `created_at` |
| Module identifiers | `snake_case` | `math`, `http_client` |

### Parser enforcement

The distinction between variants (PascalCase) and fields/methods (lowercase) is
enforced by the parser via the first character of each identifier:

**Prefix position** — beginning of an expression:

* `.Foo` → variant literal; `Foo` must start with an uppercase letter (parse error otherwise).
* `Foo` → start of a qualified constructor path; further `.Bar` segments (all uppercase) are
  consumed greedily until a lowercase segment or non-ident token is reached.
  Examples: `Result.Ok(1)`, `http.Header.ContentType`.

**Postfix position** — `.name` after an expression on the **same line**:

* `.foo` → field access or method call (lowercase required).
* `.Foo` on the **same line**, not followed by another `.` → **parse error**
  (`ConstructorInPostfix`). Variant names never appear as the final component of
  a postfix chain.
* `.Foo.` on the same line, followed by more segments → allowed as an intermediate
  qualifier. This makes `pt.Point.{ x: 1, y: 2 }` (named record constructor)
  work even when the base `pt` is lowercase.

**Newline boundary**:

* `.Foo` that begins on a **new line** (the `.` has a newline before it) is **never**
  treated as postfix. It is parsed as the start of a new statement — a variant literal
  or qualified constructor path.

This rule makes the following code unambiguous:

```tw
fn double_parsed(s: String) Result<Int, String> {
  n := try parse_int(s)
  .Ok(n * 2)          // new statement; NOT postfix of the line above
}
```

### Rationale

Capitalisation-based disambiguation removes the need for newline-sensitive parsing
in the common case. Programs that place a `.Variant` on a new line after a `let`
always work as intended. The only rule a user needs to remember is:
**types and variants are PascalCase; everything else is lowercase**.

---

## 21. Type System and Checking

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

String interpolation is type-checked by resolving a zero-arg inherent `to_string() String`
on the interpolated expression type.

### Numeric Operators and Promotion

Arithmetic operators (`+`, `-`, `*`, `/`, `%`) are defined for:

* `Int × Int -> Int`
* `Byte × Byte -> Int`
* `Int × Byte -> Int`
* `Byte × Int -> Int`
* `Float × Float -> Float`

Bitwise operators (`&`, `|`, `^`, `<<`, `>>`, unary `~`) are defined for integer
types only:

* `Int` and `Byte` are accepted as operands.
* `Byte` operands are widened to their corresponding non-negative `Int` values
  (`0..255`) before applying the operator.
* Result type is always `Int`.
* Example: for a `Byte` value `b` whose numeric value is `255`, `~b` evaluates
  as `~255`, i.e. `-256`.

Shift semantics:

* `<<` and `>>` use 64-bit masked shift counts.
* Effective count is the low 6 bits of the right operand (`right & 63`),
  including when that operand is negative.
* `>>` is arithmetic right shift (sign-preserving).

No implicit narrowing conversion exists from `Int` to `Byte`; use `Byte.from_int`.
There is no implicit mixing between `Byte` and `Float`.

Comparison operators require both operands to have the same type; result is `Bool`.

Operator precedence (tight to loose):

1. unary (`-`, `!`, `~`, `try`)
2. multiplicative (`*`, `/`, `%`)
3. additive (`+`, `-`)
4. shift (`<<`, `>>`)
5. comparison (`<`, `<=`, `>`, `>=`)
6. equality (`==`, `!=`)
7. bitwise and (`&`)
8. bitwise xor (`^`)
9. bitwise or (`|`)
10. logical and (`and`)
11. logical or (`or`)
12. assignment (`=`)

This follows common C/JS-family expectations for mixed expressions.
Because equality binds tighter than bitwise operators, `x & mask == 0` parses
as `x & (mask == 0)`.
In practice, bit-test expressions should be written with explicit parentheses:
`(x & mask) == 0`.

---

## 22. Compilation to WebAssembly GC

* Primitives:
  * `Int` → unboxed `i64`
  * `Float` → unboxed `f64`
  * `Bool` → unboxed `i32`
  * `Byte` → unboxed `i32` (logical range `0..255`)
* Records → immutable `struct` (new values created via structural sharing where possible)
* Vectors → immutable `array` (new values created via structural sharing where possible)
* Dicts → immutable hash map structures (structural sharing where possible)
* Cells → mutable `struct` wrapper storing a `T` payload
* Functions → closures allocated as small structs
* Options:

  * ref types → nullable refs
  * value types → tagged struct
* String interpolation → compiler inserts `to_string()` calls and string concatenation
* For loops → type-directed lowering to primitive loops based on collection type

---

## 23. Error Messages

Examples:

**Invalid string interpolation**:

```
error: cannot interpolate value of type SocialPost
note: type SocialPost has no inherent method `to_string() -> String`
help: define `fn to_string(x: SocialPost) String { ... }` and use "${post}"
```

**No inherent method**:

```
error: no method 'translate' for type Point
note: dot syntax only resolves record fields and inherent methods from the defining module
```

**Invalid for loop collection**:

```
error: cannot iterate over value of type Tree<Int>
note: for loops only support Vector<T>, Range, and Dict<K,V>
help: consider defining a helper function that returns Vector<Int>
```

**Mutation attempt on non-name**:

```
error: cannot update expression that is not an assignable lvalue
note: only identifiers, field accesses, or indexed expressions can appear to the left of '='
help: bind to a local variable first if you need to reuse a computed value: 'tmp := foo(); tmp.x = 1'
```

---
