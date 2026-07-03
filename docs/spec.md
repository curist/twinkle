# 🌟 **Twinkle Language Specification**

## 1. Overview

Twinkle is a statically typed language for value-oriented programs that compile to **WebAssembly GC**. It favors a concise, low-ceremony surface over breadth, and deliberately leaves out several big-ticket features — no traits or typeclasses, no higher-kinded types, and no exceptions.

Source files end with `.tw`. A source file is a module, and its top-level statements execute directly — there is no `main` function (see §8).

Identifiers follow a strict, **parser-enforced** case rule: types, enum variants, and extern namespaces start with an uppercase letter; functions, variables, fields, and module names start lowercase. Case is not style — it determines how a name parses (see §16).

### Comments and documentation comments

Line comments start with `//` and continue to the end of the line.

Documentation comments are line comments with a documentation marker:

```tw
//! Module documentation for the containing file/module.

/// Documentation for the next declaration.
pub fn answer() Int { 42 }
```

A contiguous leading `//!` block at the start of a file documents the module. A
contiguous `///` block immediately before a function or type declaration
documents that declaration. Plain `//` comments are never documentation comments.

---

## 2. Value Model

### Immutability and value semantics

**All ordinary values in Twinkle are immutable.** Primitives, strings, vectors,
dicts, sets, records, and functions cannot be mutated in place; there is no
observable in-place mutation of values in the language model. Updates are
expressed through **rebinding**: constructing a new value and binding a name to
it (see §7.4–7.6).

Twinkle has **value semantics**, not reference semantics. Rebinding affects only
the local name, never any other alias:

```tw
type Pt = .{ y: Int }

p := Pt.{ y: 0 }
q := p

p.y = 1      // p = Pt.{ y: 1 }
q            // still Pt.{ y: 0 }
```

Shared mutable state is explicit and only available through two opt-in,
mutate-in-place reference types: `Cell<T>` (a typed GC-managed cell; see §13.6)
and `@std.buffer`'s `Buffer` (a sandboxed linear-memory region, manually
allocated and freed; see [docs/design/buffer.md](design/buffer.md)).

### Primitives (unboxed)

* `Int` → wasm `i64`
* `Float` → wasm `f64`
* `Bool` → wasm `i32`, 0/1
* `Byte` → wasm `i32`, range `0..255`
* `Void` → effect-only; used as a function return type and as the value of a
  block with no final expression. It has no literal and cannot be stored or bound.

### References (GC)

* `String` — immutable, always-valid UTF-8 text.
* `Vector<T>` — immutable persistent vector; elements unboxed or ref depending on `T`.
* `record` — immutable closed struct shape.
* `Dict<K,V>` — immutable persistent hash map (HAMT-style structural sharing).
* `Set<K>` — immutable persistent set (backed by `Dict<K, Void>`).
* `function` — closure with captured environment.
* `Cell<T>` — mutable cell reference for explicit shared state (§13.6).
* `Buffer` (from `@std.buffer`) — sandboxed linear-memory region; mutate-in-place, manually allocated and freed.

---

## 3. Types & Generics

Parametric polymorphism (rank-1, no higher-kinded types):

```tw
fn map<A, B>(xs: Vector<A>, f: fn(A) B) Vector<B> { ... }
```

Type alias — does **not** create a new distinct nominal type:

```tw
type ID = Int
```

Generic parameters may require one of Twinkle's compiler-recognized **contracts**
as a bound (e.g. `<T: Stringify>`, `<C: IndexRead<E>, E>`) for syntax-level
behavior. All other reusable behavior is passed explicitly as ordinary values,
usually records of functions. Both mechanisms are described in §10.

---

## 4. Records

Named record type (nominal, closed shape):

```tw
type Point = .{ x: Int, y: Int }
```

Record literal (two forms):

```tw
// Anonymous — requires an expected record type from context
p: Point = .{ x: 10, y: 20 }

// Named constructor — always produces Point
p := Point.{ x: 10, y: 20 }

// Field punning shorthand
p2 := Point.{ x, y }      // == Point.{ x: x, y: y }
p3: Point = .{ x, y: 99 } // mixed shorthand + explicit value
```

Field access: `p.x`.

---

## 5. Enums & Pattern Matching

```tw
type Shape = {
  Circle(Float),
  Rect(Float, Float),
  UnitSquare,
}

s := Shape.Circle(3.0)

case s {
  .Circle(r) => r * r * 3.14159,
  .Rect(w, h) => w * h,
  .UnitSquare => 1.0,
}
```

Variant names are `PascalCase`. A `case` on an enum must be exhaustive unless it
uses a `_ => ...` catch-all.

### Integer tags (field-less enums)

A **field-less** enum (every variant nullary) may carry explicit integer tags,
turning a group of named integers — wire kinds, section IDs, error codes — into a
real nominal type instead of loose `Int` constants:

```tw
type CompletionKind = { Text = 1, Method = 2, Function = 3, Field = 5, Constant = 21 }
```

- **Values:** the first variant defaults to `0`; each subsequent variant is one
  past the previous resolved value unless an explicit `= N` overrides it (which is
  what produces holes). `N` is an integer literal, optionally negative. Every
  resolved tag must be distinct.
- **`k.tag : Int`** — the integer for a variant value (the wire boundary).
- **`T.from_tag(n: Int) : T?`** — recover a variant from its integer, `.None` for
  any unmapped value.

```tw
CompletionKind.Method.tag        // 2
CompletionKind.from_tag(5)       // .Some(.Field)
CompletionKind.from_tag(4)       // .None  (a hole)
```

Keep the enum type in your model and call `.tag` only at the serialization edge;
`from_tag` recovers the type on decode. The tag is a separate value mapping from
the enum's dispatch discriminant, so pattern matching is unaffected. `= N`,
`.tag`, and `from_tag` are rejected on enums with any payload-carrying variant.

---

## 6. Optionality and Errors

Twinkle has no `null` and no exceptions. Absence is modeled with `Option<T>` and
recoverable failure with `Result<T, E>`; both integrate with the `try` operator.

### Option

```tw
type Option<T> = { None, Some(T) }
```

Sugar: `T?` == `Option<T>`. The compiler optimizes reference-type options into
nullable refs.

```tw
case x {
  .None => ...,
  .Some(v) => ...,
}
```

Option → Result bridge:

```tw
opt.ok_or("missing")         // Some(v) → Ok(v), None → Err("missing")
opt.ok_or_else(fn() { ... }) // lazy — closure called only on None
```

### Result

```tw
type Result<T, E> = { Ok(T), Err(E) }
```

Type shorthand — the error type is always required:

```
T!E   ==  Result<T, E>       // full form
!E    ==  Result<Void, E>    // fallible operation with no return value
T?!E  ==  Result<Option<T>, E>   // composes with T?
```

`T!` and bare `!` are **not** valid.

```tw
fn validate(n: Int) !ParseError { ... }               // Result<Void, ParseError>
fn parse(s: String) Int!ParseError { ... }            // Result<Int, ParseError>
fn find(xs: Vector<Int>, k: Int) Int?!String { ... }  // Result<Option<Int>, String>
```

### `try`

```tw
try expr
```

* **On `Result<T,E>`:** returns early with `Err(e)` on error, extracts `Ok(v)` on
  success. For `Result<Void,E>` the `Ok` branch carries no value; present a `Void`
  success as `.Ok({})` (since `{}` evaluates to `Void`).
* **On `Option<T>`:** returns early with `None` on absence, extracts `Some(v)` on
  success. Only valid in functions returning `Option<U>`. To use `try` on an
  `Option` inside a `Result`-returning function, bridge first:
  `x := try opt.ok_or("missing")`.
* **Not valid on any other type** (compile-time error).

### Traps (unrecoverable)

Unrecoverable errors trap and cannot be caught: out-of-bounds access, division by
zero, and explicit `error("msg")`.

---

## 7. Functions, Bindings, and Rebinding

### 7.1 Function Declaration

```tw
fn f(x: Int, y: Int) Int { x + y }
```

Function parameters must be explicitly annotated. The return type is written
after the parameter list (no `->`); it may be omitted when inference suffices, in
which case the body's value determines it.

For **function expressions** (`fn (...) { ... }`) used as callbacks, parameter and
return types may be omitted when a contextual function type is available (from a
parameter type or an annotated binding). Explicit callback annotations, if
present, must agree with that contextual type.

Functions cannot mutate caller-visible values via assignment; all assignment-like
updates create new values and rebind local names. Side effects are explicit
(`print`, `println`, `error`, `Cell.set`, `Cell.update`). Functions form
**lexical scope boundaries**: names defined outside a function cannot be rebound
inside it.

### 7.2 Extern Declarations

Extern declarations describe host-provided functions and opaque host types.
Extern functions compile to Wasm imports:

```tw
extern console fn log(msg: String)
extern crypto fn random() Float
pub extern canvas {
  type Context
  fn get_context(id: String) Context
  fn clear(ctx: Context)
  fn width() Int
}
```

The module name is a bare identifier that doubles as the Wasm import module name
and the call-site namespace (`console.log(...)`, `canvas.clear(...)`). The
function name becomes the Wasm import field name. `pub` controls Twinkle module
visibility only; every extern function declaration emits/reuses a Wasm import.

Extern types are opaque nominal handles backed by non-null Wasm `(ref extern)`.
They live in the declaring Twinkle module's **type** namespace, not the extern
function namespace: inside the module above the type is `Context`, not
`canvas.Context`. When public, other modules import it like any other type. Extern
types have no fields or variants, cannot be pattern matched, and provide no
equality, ordering, or hashing by default — use explicit host functions for those.

Extern parameters must be annotated. An omitted return type means `Void`. Boundary
types are `Int`, `Float`, `Bool`, `String`, extern types, `Option<ExternType>`,
and `Void`/`()`. Other compound values (records, enums, `Vector`, `Dict`,
callbacks, and `Option`/`Result` of non-extern types) are not valid extern
boundary types.

A non-nullable extern type is non-null: if a host function declared as returning
one returns `null`/`undefined`, the runtime traps at the import boundary. To
accept a possibly-absent handle, declare the boundary as `Option<ExternType>`
(spelled `ExternType?`), which lowers to a nullable `externref` — `null`/`undefined`
becomes `.None`.

### 7.3 Bindings

```tw
x := expr      // inferred, monomorphic
x: T = expr    // annotated
```

An initial binding introduces a **new binding** in the current lexical scope; if a
same-named binding exists in an outer scope, it is **shadowed**. Bindings refer to
immutable values.

Lexical scopes are introduced by: function bodies, brace blocks, pattern-bound
names in `case` arms, loop variables in `for`, and top-level module scope.

### 7.4 Rebinding

```tw
x = expr
```

Rebinding is *syntactic convenience* for expressing a new value that replaces the
old one. Rules:

1. Legal only if `x` refers to an existing binding in an enclosing lexical scope
   **within the same function**.
2. It introduces a **fresh binding identity** for `x` — the name now refers to a
   new immutable value. It does not mutate a stored cell; it changes what future
   references to the name resolve to, for the remainder of the current lexical
   region. It does not add a scope layer.
3. If multiple bindings of `x` exist due to shadowing, the **innermost** is the target.
4. Using `x = expr` with no such binding is a compile-time error.
5. Rebinding cannot cross function boundaries.

```tw
fn bump(n: Int) Int {
  n = n + 1   // rebinds parameter 'n'
  n
}
```

### 7.5 Rebinding and Control Flow

Control-flow constructs (`if`, `for`, `case`, blocks) do **not** introduce new
rebinding scopes, except for the names they explicitly define (loop variables,
pattern-bound names). Inside a `for` loop, rebinding targets the same lexical
binding as outside it:

```tw
acc := 0
for x in xs {
  acc = acc + x      // rebinds the acc defined above
}
acc                   // sees the final value
```

Inner initial bindings shadow, and rebinding then targets the innermost:

```tw
acc := 0
if x > 0 {
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

### 7.6 Update Syntax (Desugaring)

Update-like syntax is ergonomic sugar; every update is **rebinding to a newly
constructed value**. The grammar allows identifiers, field accesses, and indexed
expressions on the left of `=`.

```tw
r.field = expr    // r = RecordUpdate(r, field, expr)
arr[i]  = value   // arr = Vector.set_unsafe(arr, i, value)   (traps OOB)
m[k]    = v       // m = Dict.set(m, k, v)
```

Nested field chains desugar recursively from the inside out; the root must be a
local identifier (`foo().x = 1` is not allowed):

```tw
a.b.c = x
// a = RecordUpdate(a, b, RecordUpdate(a.b, c, x))
```

For a receiver whose type is a generic parameter bounded by `IndexWrite<E>` (§10),
indexed assignment lowers through the contract's `set_at` instead of a concrete
builtin.

#### Rebinding receiver shorthand

When the right-hand side of a rebinding begins with `.lowercase(`, the assignment
target is used as the implicit receiver:

```tw
xs = .append(item)              // xs = xs.append(item)
state.items = .append(entry)    // state.items = state.items.append(entry)
items = .filter(f).map(g)       // items = items.filter(f).map(g)  (chain is part of RHS)
```

The shorthand is valid **only** at the head of a rebinding RHS — not in `:=`
bindings or other expression positions. Disambiguation by leading token:
`.lowercase(` → receiver shorthand; `.Uppercase` → variant literal; `.{` →
anonymous record literal.

### 7.7 Closure Capture

A function expression may reference names from its surrounding lexical scopes.
Capture is **by value at definition time**: each free variable resolves to the
innermost visible binding, and the closure captures that binding's *value* at the
point of definition. This value is final — later rebinding of the same name
introduces a new shadowing binding and does not affect closures created earlier.

```tw
x := 1
f := fn() Int { x }   // captures the value 1
x = 2                 // new shadowing binding
f()                   // 1

fn outer() fn() Int {
  x := 10             // inner binding
  fn() Int { x }      // captures inner x = 10
}
```

Consequences:

* **Loop variables are fresh per iteration**, so a closure created in a loop
  captures that iteration's value — avoiding the classic loop-capture trap:

  ```tw
  fns := collect i in range(3) { fn() Int { i } }
  fns[0]()  // 0
  fns[1]()  // 1
  fns[2]()  // 2
  ```

* **A closure cannot rebind a captured variable** — `x = ...` targets local
  bindings in the current function, so assigning to an outer variable is a
  compile-time error. For shared mutable state, capture a `Cell<T>`: the closure
  captures the cell reference, so cell effects remain shared.

  ```tw
  x := 1
  fn bad() { x = x + 1 }   // error: cannot rebind an outer-scope variable
  ```

---

## 8. Modules & Imports

> **Design rationale:** See [docs/design/module.md](design/module.md).

### 8.1 Top-Level Items

A Twinkle source file is a module. The following items are allowed at the top
level, in any order:

* **Type declarations** (`type`) — nominal record or enum types.
* **Function declarations** (`fn`) — named functions.
* **Value bindings** (`:=` or `: T =`) — module-level names bound to values.
* **Expression statements** — side-effecting expressions (must be `Void`).

```tw
pi: Float = 3.14159
max_retries := 5

println("module loaded")
```

Module-level value bindings are **module globals**: in scope for all functions
regardless of source order, optionally `pub` for export, evaluated once at
initialization. `pub` bindings cannot be rebound (each exported name is bound
once); private bindings may be rebound at module scope following §7.4–7.5.

**Initialization order:** type and function declarations are available everywhere
(no forward-declaration restriction). Value bindings and top-level expression
statements execute top-to-bottom in source order, interleaved. Top-level
expression statements introduce no name and must have type `Void`.

**Entry point:** the program *is* its top-level initialization sequence; there is
no special `main`. When compiling to WebAssembly, this sequence lowers into a
synthetic `__init__` function designated as the Wasm
[start function](https://webassembly.github.io/spec/core/syntax/modules.html#start-function),
so it runs automatically on instantiation. To let a host call code by name, export
it explicitly:

```tw
pub fn run() Void { ... }
run()   // also runs at startup via __init__
```

A module with no top-level expression statements is a library module; its value
and function exports are available to importers.

### 8.2 Imports

```tw
use foo.bar           // import foo/bar.tw, bound as "bar"
use foo.bar as baz    // aliased
use utils             // utils.tw at project root
use .helper           // relative: sibling module in same directory
use .sub.mod          // relative: nested path from same directory
use @std.fs           // stdlib module, bound as "fs"
use @std.path as path // stdlib module with alias
```

**Filesystem mapping.** An absolute dot path `a.b.c` maps to `<root>/a/b/c.tw`;
the module identifier is the last segment (`c`), or the alias. A relative import
(leading dot) resolves from the importing file's parent directory.

**Project root.** Walk up from the entry file's directory until `twinkle.toml` is
found; otherwise the entry file's directory is the root (single-file scripts).

**Stdlib.** Stdlib modules are prefixed with `@`. The prelude (primitive types,
`println`, `Vector`, `Dict`, `Set`, `String`, `Range`, etc.) is always implicitly
in scope — no `use` needed. Richer stdlib modules require an explicit `use @...`.

**Aliasing** is required when two imports share the same last-segment name;
importing two same-identifier modules without `as` is a compile-time error:

```tw
use math.vector as mvec
use graphics.vector as gvec
```

**Visibility.** `pub` exports a name; exported names are accessed qualified
(`math.add`, `math.Point`). Values and types occupy **separate namespaces**, so a
module may export both a type and a value of the same name without conflict
(`option.Option<T>`, `option.Some`).

**Re-exports** have no special syntax — use explicit `pub` rebinding:

```tw
use math.vector
pub translate := vector.translate
```

**Circular imports** are allowed when the cycle is through type and function
signatures/bodies only. A cycle that requires top-level value initialization order
is a compile-time error.

**Destructuring** brings specific names directly into scope — PascalCase as types,
snake_case as values, no `type` keyword needed. It does **not** import the parent
module name:

```tw
use math.vector.{translate, scale, Vec2}
use math.vector.{Vec2 as V2}

v := Vec2.{ x: 1, y: 2 }
v.translate(3, 4)          // ok — inherent methods resolve via the type
vector.scale(v, 2)         // error — "vector" is not in scope
```

To use the type's inherent methods *and* the module as a qualified namespace,
import both:

```tw
use math.vector
use math.vector.{Vec2}
```

Wildcard imports (`use foo.*`) will never be supported.

---

## 9. Inherent Methods (Module-Based)

Twinkle's dot syntax resolves exactly two things: **record fields** and
**inherent/module methods**. A module associates functions with a type by making
them first-argument style:

```tw
// point.tw
pub type Point = .{ x: Int, y: Int }

pub fn translate(p: Point, dx: Int, dy: Int) Point {
  .{ x: p.x + dx, y: p.y + dy }
}
```

Then `p.translate(1, 2)` desugars to a call to `translate` in the module where
`Point` is declared. **Resolution is based on the receiver's type origin, not on
imported module names** — the defining module need not be in scope, and a type
obtained via destructured import (or received transitively) resolves methods the
same way.

**Dot resolution rules:**

* Check record fields first, then the defining module's inherent methods.
* A field-vs-inherent name collision makes the dot illegal.
* No trait/typeclass involvement; contract hooks (§10) are separate compiler rules.

### Built-in inherent methods

Some built-in types define compiler-known inherent methods.

**Length** — exposed only via `.len()`:
`Vector<T>.len()`, `String.len()` (UTF-8 byte length), `Dict<K,V>.len()`,
`Set<K>.len()`.

**String conversion** — via `.to_string()`:
`Int`, `Float`, `Bool`, `Byte`, and `String` (identity).

**Numeric conversion helpers:**

* `Int.to_float() Float`
* `Float.to_int() Int`
* `Byte.to_int() Int`
* `Byte.from_int(n: Int) Option<Byte>`

**Parsing** — type-qualified constructors returning `Option<T>`:

* `Int.from_string(s: String) Option<Int>` — decimal integer (optional `+`/`-`)
* `Float.from_string(s: String) Option<Float>`

```tw
case Int.from_string("42") {
  .Some(n) => println("${n}"),
  .None => println("not a number"),
}
```

**String ordering** — the relational operators do byte-level lexicographic (UTF-8)
comparison: `"abc" < "abcd"` is `true`.

**Character utilities:**

* `String.char_code_at(s, i) Int` — byte value at byte offset `i` (alias for `Byte.to_int(s[i])`)
* `String.from_char_code(n: Int) Option<String>` — ASCII code (0–127) → single-byte string; `None` otherwise

---

## 10. Contracts and Capabilities

Twinkle expresses reusable behavior two ways:

* **Contracts** — a small, closed set of compiler-recognized named requirements
  that back syntax hooks and selected generic APIs. Users cannot declare new
  contracts (no general trait system).
* **Capability records** — ordinary record values containing functions, passed
  explicitly like any other argument. This is the general design pattern.

### 10.1 Built-in Contracts

A generic parameter may require a contract as a bound; the compiler then proves
the argument type satisfies it. Types satisfy contracts through inherent methods,
builtin witness rules, or compiler-supported derivation where noted in
[docs/contracts.md](contracts.md) (design rationale:
[docs/design/contracts.md](design/contracts.md)).

**Value contracts** gate operators and stringification:

| Contract | Method | Backs |
|---|---|---|
| `Stringify` | `to_string(self) String` | string interpolation, generic stringification |
| `Eq` | `eq(self, Self) Bool` | `==`, `!=` |
| `Ord` | `compare(self, Self) Order` | `<`, `<=`, `>`, `>=`, canonical sorting APIs |

**Access contracts** gate collection syntax; each has a functional dependency
`Self -> Elem`, so the element type is recovered from the satisfier:

| Contract | Methods | Backs |
|---|---|---|
| `IndexRead<E>` | `len(self) Int`, `at(self, Int) E` | `c[i]` read, `for x in c` |
| `IndexWrite<E>` | `set_at(self, Int, E) Self`, `append(self, E) Self` | `c[i] = v` |
| `IntoIterator<E>` | `iter(self) Iterator<E>` | `for x in c` (by iteration) |
| `Sliceable` | `slice(self, Int, Int) Self` | `c[a..b]` |

```tw
fn describe<T: Stringify>(x: T) String { "value=${x}" }

fn first<C: IndexRead<E>, E>(c: C) E? {
  if c.len() == 0 { .None } else { .Some(c.at(0)) }
}
```

Builtin types satisfy the relevant access contracts: `Vector`, `String`, and
`View<C>` are `Sliceable`; `Vector`/`String`/`Dict`/`Set`/`Range` iterate via
`for` (§12); `@std.view`'s `View<C>` satisfies `IndexRead`/`IntoIterator`.

### 10.2 Capability Records

A capability is a nominal record type capturing operations on some data type `T`.
The compiler never invents, searches for, or implicitly passes a capability — the
caller supplies it explicitly:

```tw
type Encoder<T> = .{ encode: fn(T) String }

fn write_all<T>(xs: Vector<T>, enc: Encoder<T>) {
  for x in xs { println(enc.encode(x)) }
}

type User = .{ name: String, age: Int }
fn encode_user(u: User) String { "${u.name}(${u.age})" }

user_encoder: Encoder<User> = .{ encode: encode_user }
write_all(users, user_encoder)
```

Use a capability record when equality, ordering, rendering, or matching should be
**caller-selected** rather than canonical language behavior. Instead of a general
"Iterable" trait, provide small concrete helpers, or let user types participate by
returning a supported built-in (`Vector<T>`, `Range`) from an explicit conversion
function.

### 10.3 No Implicit Conversions

Twinkle performs **no** implicit conversions to satisfy capability records, and
ordinary calls never apply silent argument coercions. A missing capability
argument is rejected:

```tw
fn debug_value<T>(x: T, enc: Encoder<T>) { ... }
debug_value(user)              // ❌ missing Encoder<User>
debug_value(user, user_encoder) // ✅
```

There is no automatic wrapping of `T` into `Encoder<T>`, no rewriting of
`Vector<T>` into `Vector<Encoder<T>>`, and no chained/inferred conversions. Any
adapter logic is explicit in user code. Built-in numeric operators do define
explicit typing/promotion rules (e.g. `Byte` arithmetic yields `Int`; see §14),
but those are operator semantics, not general implicit conversions.

---

## 11. String Literals and Interpolation

### Escapes

Cooked string literals support:

* `\n` newline, `\t` tab, `\r` carriage return
* `\"` double quote, `\\` backslash
* `\$` literal `$` (suppresses interpolation)
* `\xNN` exactly two hex digits, ASCII-only (`00..7F`)
* `\e` escape character (`U+001B`, i.e. `\x1b`)
* `\u{...}` Unicode scalar, 1–6 hex digits

`\u{...}` must decode to a valid Unicode scalar value: surrogate range
(`D800..DFFF`) and values above `10FFFF` are rejected. `"\x1b[31mred\x1b[0m"` and
`"\u{1F44D}"` (👍) are valid.

### Raw string literals

A raw string `r"…"` performs **no escape processing** — `\` is an ordinary
character, so a regex is `r"\d+"` rather than `"\\d+"`. The `r` is a raw prefix
only when immediately followed by `"`; elsewhere it is an ordinary identifier.
Because `\` is literal there is no `\"` escape, so a raw string cannot contain `"`
and cannot span a line. Interpolation still fires inside a raw string; only escape
processing is off: `r"id=${user.id}"`.

### Multiline string literals

A multiline string is one or more consecutive `\\`-prefixed lines (Zig-style).
Each line's content is everything after its `\\`; lines join with `\n`:

```tw
query :=
  \\SELECT *
  \\FROM users
```

* **No trailing newline** — add a final empty `\\` line to get one.
* **Marker indentation is excluded**; whitespace *after* `\\` is content.
* **The block ends** at the first line whose first non-whitespace characters are
  not `\\` (including a bare blank line). There is no closing delimiter.
* **No escape processing** — `\` is literal.
* **CRLF is normalized** to `\n`.

### Character literals

A character literal `'c'` denotes the **integer code point** of a single
character:

```tw
'A'         // 65
'\n'        // 10
'\u{1F600}' // 128512
```

It is an *integer-literal form*: it defaults to `Int`, narrows to `Byte` where a
`Byte` is expected and the value is in `0..255`, and may be used as a `case`
pattern. Supported escapes mirror string escapes plus `\'`:
`\n \t \r \\ \' \" \0 \e \xNN \u{...}`.

```tw
op: Byte = '+'
if code >= '0' and code <= '9' { ... }
case b { '\n' => ..., _ => ... }
```

### Interpolation

Interpolation `"hello ${x}"` is driven by the `Stringify` contract (§10.1): for
each `${expr}`, the compiler proves the expression type satisfies `Stringify` and
emits the corresponding `to_string` witness call. Builtin witnesses exist for
`String` (identity), `Int`, `Float`, `Bool`, `Byte`, and `Vector<T>` when
`T: Stringify`. User-defined types are interpolable when they define an inherent
`fn to_string(x: MyType) String`. If no witness is available, interpolation is a
compile-time error.

```tw
type User = .{ name: String, age: Int }
fn to_string(u: User) String { "${u.name} (${u.age})" }
user: User = .{ name: "Ada", age: 30 }
"user=${user}"                    // uses User.to_string()
```

Explicit `to_string` calls work inside and outside interpolation. Unary-minus
literals must be parenthesized before a method call, since `-1.to_string()` parses
as `-(1.to_string())`:

```tw
println("${(-1).to_string()}")    // ✅
```

Conceptually, `"n=${n}"` lowers to `"n=".concat(n.to_string())`.

---

## 12. Control Flow

### `if`

```tw
if x > 0 { a } else { b }     // expression
```

### `case`

On enums: exhaustive or `_ =>`. On primitives (`Int`, `Bool`, `String`, `Byte`):
must include `_`. Arm bodies are normally expressions; terminal control flow may
be written without an extra block:

```tw
case opt {
  .Some(v) => v,
  .None => return 0,
}
```

Use a block for multi-statement or non-terminal arms.

### `cond`

Multi-way conditional; the first arm whose boolean condition is `true` wins, with
`_` as default. As an **expression** a `_` arm is required for exhaustiveness; in
statement position it may be omitted (the `cond` evaluates to `Void` if nothing
matches). `return`/`break`/`continue` are allowed as terminal arm bodies; use a
block for multi-statement bodies. `cond` nests.

```tw
result := cond {
  x < 0 => "negative",
  x == 0 => "zero",
  _ => "big",
}
```

### Loops

All `for` loops are statements returning `Void`:

```tw
for condition { body }
for x in coll { body }
for x, i in coll { body }
```

<a id="iterable-collections"></a>**Iterable collections.** `for x in coll` (and
`collect`, below) support exactly these types, each with dedicated type-directed
lowering:

* `Vector<T>` — indexed loop over the length.
* `String` — indexed loop over UTF-8 bytes (`str[i]` yields `Byte`).
* `Range` — integer loop over the bounds.
* `Dict<K,V>` — iteration over key–value pairs.
* `Set<K>` — iteration via `iter()` (insertion order).
* `Iterator<T>` — repeated `Iterator.next` calls (see [docs/design/iterator.md](design/iterator.md)).
* `Channel<T>` — drains received values until the channel is closed (§15).
* A generic type parameter bounded by an **access contract** — `IndexRead<E>` (by
  index) or `IntoIterator<E>` (by `iter()`).

Any other value in `for x in coll` is a compile-time error. Types that are not
directly iterable (e.g. `View<C>` and `@std.buffer` element views) iterate through
an explicit `iter()`: `for x in value.iter()`.

**Indexed form.** `i: Int` starts at 0 and increments each iteration. It is
supported for `Vector<T>`, `String`, `Range`, `Dict<K,V>`, and `Set<K>` — not for
`Iterator<T>`.

**User extensions.** To iterate a custom type, either return a supported built-in
collection (`Vector<T>`, `Range`) from a helper, or return `Iterator<T>` via
`Iterator.unfold` (see [docs/design/iterator.md](design/iterator.md)).

### `collect` comprehension

```tw
xs := collect x in range(10) { x * x }
ys := collect x, i in range(10) { x + i }
zs := collect n < 10 { n }              // conditional (while) form
```

* Produces `Vector<T>`; the element type is the body expression's type, and all
  iterations must unify.
* Works with the same [iterable collections](#iterable-collections) as `for`, plus
  a conditional `collect condition { body }` form (`condition: Bool`, evaluated
  like a `while`).
* The two-binder form `collect x, i in coll` is supported for `Vector<T>`,
  `String`, `Range`, `Dict<K,V>`, and `Set<K>` (not `Iterator<T>`). For
  `Dict<K,V>`, the binders are key `K` and value `V`; for `String`, `x: Byte`.
* `continue` skips emission; `break` ends early and returns the partial vector.
* A body of type `Void` is an error (collect expects a value to push).

### Diverging branches

Some paths do not complete normally: `return expr`, `break`, `continue`,
`error("message")`, and infinite loops. `return`/`break`/`continue` are
statements, accepted directly as `case`/`cond` arm bodies (branch positions);
elsewhere use a block when a statement is needed in expression position. When
type-checking a multi-branch expression, branches that do not complete normally do
not contribute to its type:

```tw
x := case opt {
  .Some(v) => v,
  .None => return {},   // never completes; type comes from .Some
}
```

### `defer`

`defer expr` schedules an expression to run when the **enclosing block** exits. It
is a statement and produces no value; the deferred result is discarded.

```tw
fn write_file(path: String, data: String) !IoError {
  f := try open(path)
  defer { close(f) }      // runs however write_file exits (except trap)
  try write(f, data)
}
```

* **Scope:** tied to the nearest enclosing `{ ... }`, not the function. A `defer`
  in a loop body runs at the end of each iteration (and on `break`).
* **Ordering:** multiple defers in a block run LIFO.
* **Capture:** variables are captured by value at declaration time (like closures).
* **Triggers:** normal completion, `return` (unwinds all enclosing blocks),
  `break`, `continue`, and `try`-propagated `Err`.
* **Does not trigger on traps** (`error()`, OOB, division by zero) — no cleanup is
  possible.

> **Implementation note:** `defer` desugars at the CFG level via edge insertion —
> zero runtime overhead. See [docs/design/defer.md](design/defer.md).

---

## 13. Built-in Collections and Types

This section covers the *language-level* semantics of the built-in types — their
model, the syntax and desugarings they participate in, and their type
constraints. The full method surface (signatures, complexities, the derived
combinators like `map`/`filter`/`fold`) lives in [docs/API.md](API.md).

### 13.1 Vector

Vectors are **immutable** persistent sequences with structural sharing —
Clojure's `PersistentVector` lineage: a 32-way bit-partitioned trie with a tail
buffer, giving O(log₃₂ n) indexing and O(1) amortized append.

* `vec[i]` — 0-based indexing, traps on out-of-bounds.
* `vec[a..b]` — range-slice sugar for `vec.slice(a, b)`, the half-open `[a, b)`
  subvector. The index must be a literal range; it is backed by the `Sliceable`
  contract (§10.1), so the same `c[a..b]` form works on `String` and `View<C>`.
* `vec[i] = value` — unsafe index write (traps OOB); desugars to
  `vec = Vector.set_unsafe(vec, i, value)`.

Vector literals require the element type to be determinable from context, and all
elements must share a type:

```tw
[1, 2, 3]              // Vector<Int>
xs: Vector<Int> = []   // empty vector requires an annotation
```

### 13.2 String

Strings are **immutable** and always valid UTF-8. `str.len()` is the UTF-8 byte
length, and `str[i]` returns a `Byte` at byte offset `i` (0-based, traps OOB).
`str[a..b]` slices a byte range via the `Sliceable` contract. Prefer string
interpolation (§11) for assembly. Byte offsets that split a UTF-8 scalar trap.

### 13.3 Range

`range(n)` (`0..n`), `range_from(a, b)` (`[a, b)`), and `range_step(a, b, step)`
produce `Range` values, consumed by `for` and `collect` (§12).

### 13.4 Dict

Dicts are **immutable** persistent hash maps with structural sharing — a HAMT
(hash array mapped trie), the structure behind Clojure's `PersistentHashMap`,
giving O(log₃₂ n) get/set/has.

* **Key constraint:** `K` must be `Int`, `String`, or `Byte` — a compiler-known
  closed set. `Bool` keys are excluded (a two-entry dict should be a record).
* `m[k]` reads a key, returning `V?`; `m[k] = v` desugars to `m = Dict.set(m, k, v)`.
* **Order:** iteration (via `for`, `collect`, `Dict.keys`) is first-insertion
  order — updating an existing key keeps its position, removing a key preserves
  the relative order of the rest, and remove-then-reinsert appends at the end.

### 13.5 Set

Sets are **immutable** persistent collections of unique elements, backed by
`Dict<K, Void>` — so the same key constraint applies (`K` is `Int`, `String`, or
`Byte`) and elements iterate in **first-insertion order**. `==`/`!=` compare by
membership, so insertion order does not affect equality. `Set<K>` participates in
`for` and `collect` (§12).

### 13.6 Cell (explicit mutable state)

`Cell<T>` is an opaque, mutate-in-place container for explicit shared state (one
of the two mutable reference types, alongside `@std.buffer`'s `Buffer`; see §2).
If multiple names refer to the same cell, an update through one is visible through
all of them. A `Cell` does **not** change update-sugar semantics — `x.y = v`,
`arr[i] = v`, and `m[k] = v` still rebuild-and-rebind. Its operations
(`Cell.new`/`get`/`set`/`update`) are in [docs/API.md](API.md).

---

## 14. Type System and Checking

### Type system

Rank-1 polymorphic (Damas–Milner): unification-based, principal types, no
higher-ranked quantification, and no general trait/typeclass constraints. The
built-in contracts (§10.1) provide the only named bounds, for syntax-level
behavior.

### Bidirectional checking

Most expressions **synthesize** a type bottom-up (classic HM); certain expressions
are **checked** against an expected type from context:

* **Anonymous record literals** (`.{ ... }`) — need an expected record type.
* **Annotated bindings** (`x: T = e`) — `e` is checked against `T`.
* **Function arguments** — checked against the declared parameter type.
* **Integer/character literals in `Byte` context** — a literal is accepted as
  `Byte` only when a `Byte` is expected and the value is in `0..255`.

### Generalization

1. **`fn` declarations are generalized** — signature type variables are
   universally quantified (`fn id<A>(x: A) A { x }`).
2. **`:=` bindings are monomorphic** — instantiated to a specific monotype at the
   binding site, so `f := id` is an error; annotate (`f: fn(Int) Int = id`) or use
   `fn`.
3. **Annotated bindings** (`x: T = e`) use the annotation directly, no
   generalization.

Capabilities are ordinary values and participate in normal inference with no
special rules. String interpolation is checked by proving the interpolated
expression satisfies `Stringify`.

### Numeric operators and promotion

Arithmetic (`+`, `-`, `*`, `/`, `%`):

* `Int × Int -> Int`
* `Byte × Byte -> Int`, `Int × Byte -> Int`, `Byte × Int -> Int`
* `Float × Float -> Float`

Bitwise (`&`, `|`, `^`, `<<`, `>>`, unary `~`) accept `Int` and `Byte`; `Byte`
operands are widened to their non-negative `Int` value (`0..255`) first, and the
result is always `Int`. Example: for `Byte` `b == 255`, `~b == ~255 == -256`.
Shifts use 64-bit masked counts — the effective count is the low 6 bits of the
right operand (`right & 63`), including when negative; `>>` is arithmetic
(sign-preserving).

There is **no** implicit narrowing from `Int` to `Byte` (use `Byte.from_int`); the
only exception is contextual typing of integer/character literals in a `Byte`
position (`0..255`). There is no implicit mixing between `Byte`/`Int` and `Float`.
Comparison operators require both operands to have the same type and yield `Bool`.

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

Because equality binds tighter than the bitwise operators, `x & mask == 0` parses
as `x & (mask == 0)`; write bit tests with explicit parentheses: `(x & mask) == 0`.

---

## 15. Concurrency

Twinkle provides **cooperative** concurrency through two compiler-recognized
prelude types, `Task<T>` and `Channel<T>`. Tasks run on a single program thread
and are **not** CPU-parallel; they interleave only at explicit task points. Full
signatures are in [docs/API.md](API.md).

### Tasks

`Task<T>` is a handle to a computation that runs cooperatively:

* `Task.spawn(f: fn() T) Task<T>` — start `f` as a task and return a handle.
* `Task.await(t: Task<T>) T` — suspend the current task until `t` completes, then
  return its result; a task failure propagates as a trap.
* `Task.yield() Void` — yield control to the scheduler so another runnable task
  can make progress.

Control switches between tasks only at these **task points** (`await`, `yield`) or
at task-aware host operations (e.g. `time.sleep`, stdin reads). Between task points
a task runs without interruption, so ordinary immutable values are never observed
mid-update by another task.

### Channels

`Channel<T>` is a typed channel for passing values between tasks:

* `Channel.new() Channel<T>` — unbuffered rendezvous channel (a send and a receive
  hand off directly).
* `Channel.bounded(capacity: Int) Channel<T>` — buffered channel with a fixed
  positive capacity.
* `ch.send(value) Bool` — send, suspending under backpressure; returns `false` if
  the channel is closed.
* `ch.recv() T?` — receive the next value, or `.None` once the channel is closed
  and drained.
* `ch.close() Void` — close the channel (closing an already-closed channel is a
  no-op).

A channel is iterable: `for value in ch { ... }` receives values until the channel
is closed and drained (§12).

---

## 16. Naming Conventions

Twinkle enforces naming conventions **at the parser level** — they are hard syntax
rules, not style lint. The parser uses the **first character** of an identifier to
decide what it can mean, so the wrong case changes how code parses (or makes it a
parse error).

**The rule:** an identifier that starts with an **uppercase** letter is a type, an
enum variant, or an extern namespace; **everything else starts lowercase**.

| Thing | Convention | Example |
|---|---|---|
| Types | `PascalCase` | `Point`, `Option`, `HttpRequest` |
| Enum variants | `PascalCase` | `None`, `Ok`, `SomeName` |
| Extern namespaces | `PascalCase` or `snake_case` | `Math`, `console` |
| Functions | `snake_case` | `parse_int`, `to_string` |
| Variables & module globals | `snake_case` | `result`, `max_retries` |
| Record fields | `snake_case` | `x`, `created_at` |
| Module identifiers | `snake_case` | `math`, `http_client` |

There is **no** `SCREAMING_SNAKE_CASE` for constants: a module global is an ordinary
value binding, so `max_retries := 5` is legal but `MAX_RETRIES := 5` is a parse
error — the parser reads `MAX_RETRIES` as a type name. (A host object bound with
`extern Math { ... }` may be `PascalCase`, but the Twinkle *values* it exports are
still bound to lowercase names, e.g. `pub pi := 3.14159`.)

### Parser disambiguation

The uppercase/lowercase split lets the parser resolve constructor-vs-field
ambiguity by first character.

**Prefix position** (start of an expression):

* `.Foo` → variant literal (`Foo` must be uppercase, else a parse error).
* `Foo` → start of a qualified constructor path; further uppercase `.Bar` segments
  are consumed greedily until a lowercase segment or a non-identifier token
  (`Result.Ok(1)`, `http.Header.ContentType`).

**Postfix position** (`expr.name`):

* `.foo` → field access or method call (lowercase); stays postfix even across a
  newline, so multiline method chains work.
* `.Foo` on the **same line**, not followed by another `.` → parse error (a variant
  name never terminates a postfix chain).
* `.Foo.` on the same line, as an intermediate qualifier → allowed
  (`pt.Point.{ x: 1 }`).

**Newline boundary:** a `(` or `[` that begins a new line is never postfix, and
`.Foo`/`.{` beginning a new line starts a fresh prefix expression (variant literal,
constructor path, or record literal) rather than continuing the previous line. This
keeps statement boundaries unambiguous:

```tw
fn double(s: String) Result<Int, String> {
  n := try parse_int(s)
  .Ok(n * 2)          // new statement, not postfix of the line above
}
```
