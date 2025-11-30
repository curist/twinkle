# 🌟 **Twinkle Language Specification**

## 1. Overview

Twinkle is a small statically typed language targeting **WebAssembly GC**.

Design goals:

* Lightweight, scripting-like syntax.
* Hindley–Milner type inference (Gleam/OCaml style).
* Unboxed primitives (`int = i64`, `float = f64`, `bool`).
* GC-managed references for strings, arrays, records, dicts.
* Small runtime; rely on `struct`, `array`, reference types.
* No trait methods callable from user code (traits = contracts).
* Inherent methods only via module functions.

Source files end with `.tw`.

---

## 2. Value Model

### Primitives (unboxed)

* `int` → wasm `i64`
* `float` → wasm `f64`
* `bool` →  wasm `i32`, 0/1
* `void` → effect-only (no value).

### References (GC)

* `string` — immutable text.
* `array<T>` — GC array; element unboxed/ref depending on `T`.
* `record` — closed struct shape.
* `dict<K,V>` — hash map reference.
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

Constraints allowed:

```tw
fn log<T: Show>(x: T) -> void { ... }
```

No higher-kinded types.

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
p := Point{ x: 10, y: 20 }
```

Field access: `p.x`

Closed shape; no row polymorphism in MVP.

---

## 7. Functions & Bindings

Declaration:

```tw
fn f(x: int, y: int) -> int { x + y }
```

Bindings:

```tw
x := expr
x: int = expr
x = expr         // reassignment
```

Blocks are expressions; final expression determines type.

Variables are mutable.

---

## 8. Modules & Imports

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

## 10. Traits (Contract-Only)

Traits define **capabilities**, not callable methods.

Declaration:

```tw
trait Show(T) {
  fn show(x: T) -> string
}
```

Implementation:

```tw
impl Show(Point) {
  fn show(p: Point) -> string {
    "Point(${p.x}, ${p.y})"
  }
}
```

### Key rules

* Trait methods are **NOT visible** in user code.
* Not callable.
* Not accessible via dot.
* Different traits may reuse method names freely.
* Compiler calls trait methods only for specific language-defined features:

  * string interpolation → `Show`
  * `for` / `collect` → `Iterable`
* Traits appear only in type constraints.

### Coherence

One implementation per `(Trait, TypeHead)` pair.

---

## 11. String Interpolation

```tw
"hello ${x}"
```

This:

1. Adds a constraint: `TypeOf(x): Show`
2. Lowers internally to a call to the `Show` implementation for that type.
3. `Show.show` is never callable explicitly.

If no `Show` impl exists → compile-time error.

Interpolation is the **only** user-facing stringification facility.

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

Break/continue as usual.

---

## 13. Collect Comprehension

```tw
xs := collect x in range(10) { x * x }
```

Rules:

* Produces `array<T>`.
* `continue` skips emission.
* `break` ends early, returns partial array.

---

## 14. Iterable Trait

```tw
trait Iterable(T) {
  type Item
  type State
  fn init(x: T) -> State
  fn next(s: State) -> Step<State, Item>
}
```

`Step<S, A>`:

```tw
enum Step<S, A> {
  Done,
  Yield(A, S),
}
```

User never calls these.
Compiler uses them for:

* `for x in coll {…}`
* `collect`

Std lib implements Iterable for arrays, strings, dicts, ranges, options.

---

## 15. Arrays

`arr[i]` indexing, 0-based.

Built-in:

```tw
len(arr)
```

Inherent helpers allowed via module of array type:

* `array.push(a, x)`
* `array.pop(a)`
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


---

## 16. Strings

Immutable, `len(str)` valid.

Interpolation recommended for assembly.

---

## 17. Range

`range(10)` → 0..9
`range_from(a,b)`
`range_step(a,b,step)`

Used by `for` and `collect`.

---

## 18. Dict

Creation:

```tw
m: dict<string, int> = dict.new()
```

Type parameters are inferred from the annotation.

Inherent methods:

* `m.put(k,v)`
* `m.get(k) -> V?`
* `m.has(k)`
* `m.keys() -> array<K>`
* `len(m)`

Indexing syntax:

* `m[k]` returns `V?` (Option<V>) for safe access
* `m[k] = v` inserts or updates the value

---

## 19. Error Handling

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

Only for `Result<T,E>`.
Returns early with `Err(e)` on error.
For `Result<void,E>` the `Ok` branch carries no value.

---

## 20. Prelude

Implicitly imported.

Includes:

* primitive functions: `print`, `println`, `len`
* types: `int`, `float`, `string`, `array<T>`, `dict<K,V>`
* builtin traits used by compiler:

  * `Show`
  * `Iterable`
  * `Eq`, `Ord`
* range functions

Does **not** include:

* trait methods exposed to user
* `to_string` (removed; use `${x}` instead)
* any implicit global dispatch functions

---

## 21. Type Inference (HM + Traits)

* Standard Hindley–Milner with:

  * unification
  * let-generalization (value restriction applies for refs)
  * trait constraints as simple class constraints

Interpolation introduces trait constraints:

```
"${x}"  →  x : Show
```

Generic function:

```tw
fn f<T: Show>(x: T) -> string {
  "${x}"
}
```

Trait method bodies type-check normally but are not callable.

Inference never performs method-name search — traits do not leak into the value namespace.

---

## 22. Compilation to WebAssembly GC

* Primitives → unboxed `i64/f64`
* Records → `struct`
* Arrays → `array`
* Functions → closures allocated as small structs
* Options:

  * ref types → nullable refs
  * value types → tagged struct
* Interpolation → compiler calls into Show impl
* Iterable lowering → loops over State + next()

---

## 23. Error Messages

Examples:

**No `Show` impl**:

```
error: cannot interpolate value of type SocialPost
note: string interpolation requires an implementation of Show(SocialPost)
```

**No inherent method**:

```
error: no method 'translate' for type Point
note: dot syntax only resolves record fields and inherent methods from the defining module
```

---

## 24. Operators

Twinkle provides a small, fixed set of operators.
Some of these are **primitive-only** and never overloadable.
Others are **backed by traits** (similar to `Show` and `Iterable`):

* The operator syntax is user-facing (`==`, `+`, `[]`, …).
* The compiler lowers these into **trait method calls**.
* Trait methods remain **inaccessible** in user code; only `impl` bodies may define them.

This preserves Twinkle’s design:

* Traits are *contracts*, not method providers.
* Operator overloading is **opt-in per type** via trait instances.
* No method-name search leaks into the value namespace.

### 24.1 Operator Categories

Twinkle distinguishes:

1. **Equality and ordering operators**
2. **Arithmetic operators**
3. **Indexing operators**
4. **Logical and assignment operators** (non-overloadable)

Only categories (1–3) are trait-backed and extensible.
Logical operators and assignment are fixed.

---

### 24.2 Equality and Ordering

Equality and ordering operators are backed by the `Eq` and `Ord` traits from the prelude.

#### 24.2.1 Traits and enums

```tw
trait Eq(T) {
  fn eq(a: T, b: T) -> bool
}

enum Ordering {
  Lt,
  Eq,
  Gt,
}

trait Ord(T) {
  fn compare(a: T, b: T) -> Ordering
}
```

* These trait methods are **not callable** from user code.
* Only the compiler may call them when lowering operators.

The prelude provides implementations for primitive types (`int`, `float`, `bool`, `string`, etc.).
User code may define `impl Eq(T)` / `impl Ord(T)` for user-defined types.

#### 24.2.2 Equality operators

Overloadable operators:

```tw
a == b
a != b
```

Typing & constraints:

* Let `T = type(a) = type(b)`.
* Both `==` and `!=` require a constraint `Eq(T)`.

Desugaring:

* `a == b` is lowered to an internal call:

  ```tw
  Eq.eq(a, b)
  ```

* `a != b` is lowered to:

  ```tw
  !Eq.eq(a, b)
  ```

If no `Eq(T)` instance is available, the compiler reports an error at the operator site.

#### 24.2.3 Ordering operators

Overloadable operators:

```tw
a <  b
a <= b
a >  b
a >= b
```

Typing & constraints:

* Let `T = type(a) = type(b)`.
* All ordering operators require a constraint `Ord(T)`.

Desugaring (conceptual):

```tw
cmp := Ord.compare(a, b)

a <  b  // cmp == .Lt
a <= b  // cmp == .Lt || cmp == .Eq
a >  b  // cmp == .Gt
a >= b  // cmp == .Gt || cmp == .Eq
```

The compiler is free to inline or optimize this pattern.
If no `Ord(T)` instance exists, the operator usage is rejected.

---

### 24.3 Arithmetic Operators

Arithmetic operators are backed by a family of traits with associated `Output` types.
This allows, for example, vector addition, complex numbers, or custom numeric types.

#### 24.3.1 Traits

```tw
trait Add(T) {
  type Output
  fn add(a: T, b: T) -> Output
}

trait Sub(T) {
  type Output
  fn sub(a: T, b: T) -> Output
}

trait Mul(T) {
  type Output
  fn mul(a: T, b: T) -> Output
}

trait Div(T) {
  type Output
  fn div(a: T, b: T) -> Output
}

trait Rem(T) {
  type Output
  fn rem(a: T, b: T) -> Output
}

trait Neg(T) {
  fn neg(x: T) -> T
}
```

* The prelude provides implementations for primitives (`int`, `float`) with `Output = T`.
* User code can implement these traits for user-defined types.
* Methods are **not callable** directly; they exist only for operator lowering.

#### 24.3.2 Binary arithmetic operators

Overloadable binary operators:

```tw
a + b
a - b
a * b
a / b
a % b
```

Typing & constraints:

* Let `T = type(a) = type(b)`.
* `a + b` requires `Add(T)` and has type `Add.Output(T)`.
* `a - b` requires `Sub(T)` and has type `Sub.Output(T)`.
* `a * b` requires `Mul(T)` and has type `Mul.Output(T)`.
* `a / b` requires `Div(T)` and has type `Div.Output(T)`.
* `a % b` requires `Rem(T)` and has type `Rem.Output(T)`.

Desugaring (conceptual):

```tw
a + b  // Add.add(a, b)
a - b  // Sub.sub(a, b)
a * b  // Mul.mul(a, b)
a / b  // Div.div(a, b)
a % b  // Rem.rem(a, b)
```

Division by zero and other invalid arithmetic operations follow the general error/trap rules.

#### 24.3.3 Unary arithmetic operator

Unary negation:

```tw
-x
```

Typing & constraints:

* Let `T = type(x)`.
* Requires `Neg(T)` and has type `T`.

Desugaring:

```tw
-x  // Neg.neg(x)
```

#### 24.3.4 Compound assignment

Compound assignments are pure syntactic sugar:

```tw
x += y
x -= y
x *= y
x /= y
x %= y
```

Desugaring:

```tw
x += y  // x = x + y
x -= y  // x = x - y
x *= y  // x = x * y
x /= y  // x = x / y
x %= y  // x = x % y
```

The underlying `+`, `-`, `*`, `/`, `%` are resolved via the arithmetic traits as described above.

---

### 24.4 Indexing Operators

Indexing uses two traits: `Index` for read access, `IndexMut` for write access.
This powers array indexing, dictionary lookup, and user-defined indexable types.

#### 24.4.1 Traits

```tw
trait Index(T, I) {
  type Output
  fn get(target: T, index: I) -> Output
}

trait IndexMut(T, I) {
  type Output
  fn set(target: T, index: I, value: Output) -> void
}
```

* Implementations for built-in types (`array`, `dict`) are provided by the prelude.
* User types may implement `Index` and `IndexMut` to support `[]` syntax.
* `get` and `set` are not callable directly; they are used only by the compiler.

#### 24.4.2 Read access: `a[i]`

Expression form:

```tw
a[i]
```

Typing & constraints:

* Let `T = type(a)` and `I = type(i)`.
* Introduces the constraint `Index(T, I)`.
* The expression has type `Index.Output(T, I)`.

Desugaring:

```tw
a[i]  // Index.get(a, i)
```

If no `Index(T, I)` implementation exists, the operator usage is rejected.

#### 24.4.3 Write access: `a[i] = v`

Assignment form:

```tw
a[i] = v
```

Typing & constraints:

* Let `T = type(a)` and `I = type(i)`.
* Introduces constraint `IndexMut(T, I)`.
* Checks that `type(v)` unifies with `IndexMut.Output(T, I)`.
* Has type `void`.

Desugaring:

```tw
a[i] = v  // IndexMut.set(a, i, v)
```

If no `IndexMut(T, I)` implementation exists, or if `v`’s type does not match `Output`, the assignment is rejected.

#### 24.4.4 Prelude implementations

The prelude defines at least:

1. **Arrays**

   ```tw
   impl Index(array<T>, int) {
     type Output = T
     fn get(xs: array<T>, i: int) -> T { /* traps on OOB */ }
   }

   impl IndexMut(array<T>, int) {
     type Output = T
     fn set(xs: array<T>, i: int, value: T) -> void { /* traps on OOB */ }
   }
   ```

   Out-of-bounds indices trap per the general error rules.

2. **Dictionaries**

   ```tw
   impl Index(dict<K, V>, K) {
     type Output = V?
     fn get(m: dict<K, V>, k: K) -> V? { /* Some(v) or None */ }
   }

   impl IndexMut(dict<K, V>, K) {
     type Output = V
     fn set(m: dict<K, V>, k: K, value: V) -> void { /* insert or update */ }
   }
   ```

   * `m[k]` returns `V?` (`Option<V>`) to safely represent missing keys.
   * Explicit helpers (e.g., `dict.get(k) -> V?`) may also be provided.

Other core types (e.g. strings) may gain `Index` implementations in future versions.

---

### 24.5 Non-Overloadable Operators

The following operators are **fixed** and never trait-backed:

1. **Logical operators**

   ```tw
   !x
   x && y
   x || y
   ```

   * Only valid for `bool`.
   * Preserve short-circuit evaluation for `&&` and `||`.
   * Not overloadable.

2. **Assignment and bindings**

   ```tw
   x = expr     // assignment
   x := expr    // binding
   x: T = expr  // binding with annotation
   ```

   * Not overloadable.
   * Have statement-like behavior (`void` type).

3. **Other syntax forms**

   * Control flow keywords (`if`, `case`, `for`, `collect`, `try`) are not overloadable.
   * There is no user-defined operator syntax in Twinkle; the set of operators is closed.

---

### 24.6 Type Inference and Trait Constraints

Operators participate in Hindley–Milner type inference by introducing **trait constraints**:

* `a == b` introduces `Eq(T)`.
* `a < b` introduces `Ord(T)`.
* `a + b` introduces `Add(T)`.
* `a[i]` introduces `Index(T, I)`.
* `a[i] = v` introduces `IndexMut(T, I)`.

These constraints behave like other trait constraints in Twinkle:

* They must be resolved by available `impl` instances.
* If they cannot be resolved, the compiler reports a type error at the operator site.
* Trait methods remain invisible in user code; only the compiler generates calls to them when lowering operators.

This keeps Twinkle’s value namespace free of implicit overload resolution while still allowing a small, controlled form of operator overloading via traits.
