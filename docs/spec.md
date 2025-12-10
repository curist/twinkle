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
let s = Shape.Circle(3.0)
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
let p = Point{ x: 10, y: 20 }
```

Field access: `p.x`

consider below as extension, and is outside of the scope of MVP

Anonymous record literal .{ field₁: e₁, ..., fieldₙ: eₙ } introduces a fresh type variable τ with a constraint:

* τ must be a nominal struct type whose declared fields are exactly { fieldᵢ: type(eᵢ) }.
* During type inference, τ may be unified with a concrete nominal struct type (e.g. Person). This succeeds iff that struct’s field set and field types match the constraint.
* All uses of the variable must agree on a single nominal struct type; otherwise, inference fails.
* If, after solving, τ is still unconstrained (no nominal type chosen) and the value escapes the function or is otherwise observable at an interface boundary, an explicit type annotation is required.
* This mechanism does not introduce structural record types into the language; it is solely a constraint-solving aid for anonymous record literals.

---

## 7. Functions & Bindings

Declaration:

```tw
fn f(x: int, y: int) -> int { x + y }
```

Bindings:

```tw
let x = expr // immutable
let x: int = expr
y = something    // reassignment
```

Blocks are expressions; final expression determines type.

Only syntactic values that contain no allocations (ints, floats, bools, enum constructors, `.{ ... }` with value-only fields, etc.) are generalized. Ref-y values (arrays, strings, dicts, closures) get monomorphic type variables.

---

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

# **10. Traits (Capabilities With Callable Namespace Functions)**

Traits define **capabilities** for types.
They introduce *functions* that may be implemented for a specific type, and may optionally be called by user code **through the trait’s namespace**, never via dot.

Rationale of the calling convention of trait methods:

* Keep dot resolution trivial.
* Avoid typeclass-style global function overloading.
* Make trait dispatch visually obvious (Show.show(x)).
* Prevent method collision between traits.

---

## **10.1 Declaration**

```tw
trait Show<T> {
  fn show(x: T) -> string
}
```

A trait defines:

* A set of **functions** (not methods bound to a value).
* A single type parameter describing the implementing type.

Trait functions belong to the **trait namespace**, not the value’s namespace.

---

## **10.2 Implementation**

```tw
impl Show(Point) {
  fn show(p: Point) -> string {
    "Point(${p.x}, ${p.y})"
  }
}

```

Each implementation provides concrete definitions for the trait’s functions.

A trait may be implemented for a type only once (coherence rules below).

---

## **10.3 Calling Trait Functions**

Trait functions **are callable**, but only through the trait name:

```tw
Show.show(p)
Eq.eq(a, b)
Ord.compare(x, y)
```

### **Not allowed:**

```tw
p.show()          // forbidden: trait functions are not dot methods
eq(a, b)          // forbidden unless user defines such a function
```

Trait functions never participate in dot lookup.

This preserves:

* simple dot resolution (fields + inherent methods only),
* no multi-trait method search,
* no trait-based method overloading.

---

## **10.4 Where Traits Are Used by the Compiler**

Even though trait functions are callable by users, the compiler also relies on traits for certain built-in language features:

* **String interpolation** → requires `Show<T>`
* **Equality & ordering operators** → require `Eq<T>`, `Ord<T>`
* **Arithmetic operators** → require `Add<T>`, `Sub<T>`, etc.
* **Indexing** → requires `Index<T,I>` / `IndexMut<T,I>`
* **`for` and `collect` loops** → require `Iterable<T>`

These features are lowered to trait function calls, e.g.:

```
a == b   →   Eq.eq(a, b)
a + b    →   Add.add(a, b)
"${x}"   →   Show.show(x)
```

Trait functions used for lowering remain normal functions and can still be called explicitly by user code.

---

## **10.5 Trait Constraints**

Trait constraints appear on generics:

```tw
fn print_all<T: Show>(xs: array<T>) {
  for x in xs {
    println(Show.show(x))
  }
}
```

Constraints must be resolvable at compile time.
If no implementation is available, the compiler reports an error at the usage site.

Multiple traits may define functions with the same name; there is no conflict unless users qualify incorrectly.

---

## **10.6 Coherence Rules**

To prevent ambiguous implementations:

* **At most one** implementation is allowed for each `(Trait, TypeHead)` pair.
* An implementation is allowed only if **either**:

  * the trait or
  * the type
    is defined in the current module/package (the “orphan rule”).

This ensures deterministic resolution and avoids cross-package conflicts.

---

## **10.7 Trait Functions vs Dot Syntax**

Dot resolution **never** looks at traits.

Valid:

```tw
p.x                // record field
point.translate(p) // inherent method desugared
p.translate(1,2)   // dot sugar → point.translate(p, 1, 2)
Show.show(p)       // explicit trait function call
```

Invalid:

```tw
p.show()           // trait methods are not dot-callable
p.eq(q)            // no trait methods in dot namespace
```

This separation ensures:

* predictable name resolution,
* no hidden dispatch pathways,
* no trait-object behavior,
* simpler reasoning about code generation.

---

## 11. String Interpolation

String interpolation uses:

```
"hello ${x}"
```

For an expression `x`:

1. The compiler introduces a constraint `Show<T>` where `T = type(x)`.

2. Interpolation desugars to:

   ```tw
   Show.show(x)
   ```

3. The resulting strings are concatenated in evaluation order.

### Example

```tw
let name = "Ada"
n = 3

let message = "Hello ${name}, you have ${n} messages."
```

Desugaring (conceptual):

```tw
let message =
  String.concat([
    "Hello ",
    Show.show(name),
    ", you have ",
    Show.show(n),
    " messages."
  ])
```

(Exact lowering left to the implementation.)

### Errors

If no `Show<T>` implementation exists:

```
error: cannot interpolate value of type SocialPost
note: string interpolation requires an implementation of Show<SocialPost>
```

### User invocation of trait functions

Users may also call trait functions directly:

```tw
println( Show.show(x) )
```

Dot-call syntax is forbidden:

```tw
x.show()        // invalid
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

All `for` loops are statements returning `void`.

Forms:

```tw
for cond { body }

for x in coll { body }

for x,i in coll { body }
```

* `i: int` starts from 0 and increments each iteration.
* Independent of the underlying Iterable.State.
* Break/continue as usual.

---

## 13. Collect Comprehension

```tw
let xs = collect x in range(10) { x * x }
```

Rules:

* Produces `array<T>`.
* `continue` skips emission.
* `break` ends early, returns partial array.
* If the body returns void → error, because collect expects a value to push.
* The element type is inferred as the type of the body expression; all iterations must unify to same type; otherwise type error.

---

## 14. Iterable Trait

```tw
trait Iterable<T> {
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

* Only for `Result<T,E>`.
* Returns early with `Err(e)` on error.
* For `Result<void,E>` the `Ok` branch carries no value.
* `.Ok({})` is the way to present `void` return for `Result`, as `{}` evals to `void`.
* Cannot be applied to non-Result types (compile-time error).

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

The standard library provides Show impls for core collections and enums (with some default formatting).

We may want to have a `Lenable` trait, so `len()` work on user custom types.

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

### 24. Operators

Twinkle provides a small, fixed set of operators.
Operators are **desugared into trait function calls**, where traits define the required behavior for the operator.

Traits remain **namespace-qualified function providers**, not dot methods.
Example:

```tw
a + b     →    Add.add(a, b)
a == b    →    Eq.eq(a, b)
a[i]      →    Index.get(a, i)
```

Operators never trigger trait-based method lookup on values.

---

## **24.1 Operator Categories**

Operators fall into these groups:

1. Equality and ordering operators
2. Arithmetic operators
3. Indexing operators
4. Non-overloadable operators (logical, assignment)

Only categories (1–3) rely on trait functions.

---

# **24.2 Equality and Ordering Operators**

Equality and ordering depend on these traits:

```tw
trait Eq<T> {
  fn eq(a: T, b: T) -> bool
}

enum Ordering { Lt, Eq, Gt }

trait Ord<T> {
  fn compare(a: T, b: T) -> Ordering
}
```

### **24.2.1 Equality operators**

```
a == b
a != b
```

Typing rule:

* Let `T = type(a) = type(b)`
* Requires constraint `Eq<T>`

Desugaring:

```tw
a == b    →    Eq.eq(a, b)
a != b    →    !Eq.eq(a, b)
```

Users may call:

```tw
Eq.eq(a, b)
```

directly when needed.

---

### **24.2.2 Ordering operators**

```
a <  b
a <= b
a >  b
a >= b
```

Typing rule:

* Let `T = type(a) = type(b)`
* Requires `Ord<T>`

Desugaring pattern:

```tw
let cmp = Ord.compare(a, b)

a <  b   →   cmp == .Lt
a <= b   →   cmp == .Lt || cmp == .Eq
a >  b   →   cmp == .Gt
a >= b   →   cmp == .Gt || cmp == .Eq
```

Users may call `Ord.compare(a, b)` explicitly in generic functions.

---

# **24.3 Arithmetic Operators**

Backed by numeric traits with associated output types:

```tw
trait Add<T> {
  type Output
  fn add(a: T, b: T) -> Output
}
```

(And similar for `Sub`, `Mul`, `Div`, `Rem`, `Neg`.)

### Supported operators:

```
+   -   *   /   %   unary -
```

Typing rule:

```
a + b   requires Add<T>, yields Add.Output<T>
```

Desugaring example:

```tw
a + b     →    Add.add(a, b)
-x        →    Neg.neg(x)
```

Trait functions may be called by users:

```tw
let y = Add.add(a, b)
```

---

# **24.4 Indexing Operators**

Indexing is expressed through `Index` and `IndexMut` traits:

```tw
trait Index<T, I> {
  type Output
  fn get(target: T, index: I) -> Output
}

trait IndexMut<T, I> {
  type Output
  fn set(target: T, index: I, value: Output) -> void
}
```

### Read:

```
a[i]    →   Index.get(a, i)
```

### Write:

```
a[i] = v    →   IndexMut.set(a, i, v)
```

Users may call these functions explicitly:

```tw
Index.get(arr, 2)
IndexMut.set(map, "k", 10)
```

---

# **24.5 Non-Overloadable Operators**

* Logical: `!x`, `x && y`, `x || y` (bool only)
* Assignment: `=`, `x: T = expr`
* No user-defined operator syntax

---

# **24.6 Type Inference with Operator Constraints**

Operators inject trait constraints into HM inference:

```
a == b     →   Eq<T>
a < b      →   Ord<T>
a + b      →   Add<T>
a[i]       →   Index<T,I>
a[i] = v   →   IndexMut<T,I>
```

If constraints cannot be solved, the compiler errors at the operator site.

---

# **25. Trait Resolution**

Twinkle’s trait system is designed to be **simple, deterministic, and fast**:

* Coherent (no overlapping impls).
* HM-friendly.
* Powerful enough for operator lowering and “derived” impls (e.g. `Index<Option<T>, I> where Index(T, I)`).
* Still small compared to Rust/Haskell style solvers.

This section defines how trait implementations are matched and how trait constraints are solved.

---

## **25.1 Coherence**

For any given trait and type head, Twinkle enforces:

> **At most one `impl` per `(Trait, TypeHead)` pair.**

* The **type head** is the outer constructor of the implementing type (e.g. `array<T>`, `Dict<K, V>`, `Option<T>`, `Point`).
* No overlapping impls are allowed.

Consequences:

* Impl selection is always a **single, unambiguous match**.
* There is no backtracking or global “best candidate” search.

---

## **25.2 Generic Implementations**

Impls may be generic over type parameters:

```tw
impl<T> Show(array<T>) {
  fn show(xs: array<T>) -> string { ... }
}

impl<K, V> Index(Dict<K, V>, K) {
  fn get(m: Dict<K, V>, k: K) -> Option<V> { ... }
}
```

Rules:

* Type parameters are introduced in `impl<...>`.
* They are instantiated by **unification** with the concrete types at the usage site.
* Associated types declared in the trait (e.g. `type Output`) are **determined by the impl**, usually via function signatures (see 25.3).

---

## **25.3 Associated Types in Impls**

Traits may declare associated types:

```tw
trait Index<T, I> {
  type Output
  fn get(target: T, index: I) -> Output
}
```

In an `impl`, associated types are typically determined by the function signatures:

```tw
impl<T> Index(array<T>, int) {
  fn get(xs: array<T>, i: int) -> Option<T> {
    ...
  }
}
```

Here, the compiler infers:

```tw
Index.Output(array<T>, int) = Option<T>
```

You may optionally write the associated type explicitly:

```tw
impl<T> Index(array<T>, int) {
  type Output = Option<T>
  fn get(xs: array<T>, i: int) -> Option<T> { ... }
}
```

but this is usually redundant; the function return type will be unified with the associated type.

---

## **25.4 `where` Constraints**

Impls may declare **explicit trait dependencies** using `where`:

```tw
impl<T> Index(Option<T>, int) where Index(T, int) {
  fn get(opt: Option<T>, i: int) -> Index.Output(T, int) {
    match opt {
      Some(inner) -> Index.get(inner, i)
      None        -> None
    }
  }
}
```

Meaning:

* To use `Index(Option<T>, int)`, the solver must also resolve `Index(T, int)`.

Restrictions (v1):

* `where` clauses may only mention traits applied to the impl’s type parameters (e.g. `Index(T, int)`, `Ord(T)`).
* No exotic forms (no higher-rank / nested type-level logic).

---

## **25.5 Constraint Solving Algorithm**

Trait solving is a **small, memoized graph walk**.

When the typechecker needs a constraint like:

```tw
Index(T0, I0)
```

it:

1. **Lookup impl head**

   * Find the unique `impl` whose head matches `Index(…, …)`.
   * Unify the impl’s type parameters with `(T0, I0)`.

2. **Collect `where` dependencies**

   * For an impl:

     ```tw
     impl<T> Index(Option<T>, int) where Index(T, int) { ... }
     ```

     and a usage `Index(Option<Row>, int)`, the solver adds a new obligation `Index(Row, int)`.

3. **Solve dependencies recursively**

   * Each `where` constraint is solved using the same process (lookup → unify → recurse).

4. **Memoize**

   * Results are cached per `(Trait, ConcreteTypes)`, so each unique constraint is solved at most once.

5. **Cycle detection**

   * If solving `C` requires solving `C` again (directly or indirectly), the compiler reports a **cyclic trait dependency** error.
   * Cycles are illegal.

Because there is:

* exactly one matching impl per head (coherence), and
* a finite, acyclic graph of dependencies in well-formed programs,

this process is deterministic and linear in the size of the constraint graph.

---

## **25.6 Interaction with Operators**

Operators are desugared to trait functions (see Section 24):

```tw
a + b   →   Add.add(a, b)
a[i]    →   Index.get(a, i)
a == b  →   Eq.eq(a, b)
```

These desugarings introduce trait constraints such as:

* `Add(T)` for `a + b`
* `Index(T, I)` for `a[i]`
* `Eq(T)` for `a == b`

The constraint solver described above is used to resolve these in the same way as explicit calls like `Index.get(a, i)`.

---

## **25.7 Allowed & Disallowed Patterns (v1)**

**Allowed:**

* Simple, “leaf” impls:

  ```tw
  impl<K, V> Index(Dict<K, V>, K) {
    fn get(m: Dict<K, V>, k: K) -> Option<V> { ... }
  }
  ```

* Derived impls with explicit `where` dependencies:

  ```tw
  impl<T> Index(Option<T>, int) where Index(T, int) {
    fn get(opt: Option<T>, i: int) -> Index.Output(T, int) { ... }
  }
  ```

* Traits with associated types used to tie result types to the implementing type (e.g. `Iterable<T> { type Item; ... }`).

**Disallowed / rejected (v1):**

* Overlapping impls for the same `(Trait, TypeHead)`.
* Cyclic `where` dependency graphs.
* Higher-order or higher-rank trait constraints.

These restrictions keep resolution simple, predictable, and friendly to fast HM-style compilation.

---

# **26. Iterable and Loop Lowering**

This section defines Twinkle’s iteration model: how containers provide iteration, how `for` loops desugar, and how `collect` drives comprehension-like expressions.

Twinkle does **not** use trait-based method lookup. Iteration is expressed through the `Iterable` trait, whose functions live in its namespace.

---

## **26.1 The `Step` enum**

Iteration is driven by a small control enum:

```tw
enum Step<S, I> {
  Done
  Yield(I, S)
}
```

Meaning:

* `Done` — iteration has completed
* `Yield(item, next_state)` — produce one element and continue with updated state

This corresponds to a simple pull-based iterator.

---

## **26.2 The `Iterable<T>` Trait**

Iteration capability for a type `T` is provided by:

```tw
trait Iterable<T> {
  type Item     // the produced item type
  type State    // the internal iteration state

  fn init(x: T) -> State
  fn next(s: State) -> Step<State, Item>
}
```

Notes:

* `State` is an associated type to allow optimized state (e.g. an index for arrays, a cursor for strings, a hash-bucket cursor for Dict).
* `Item` is an associated type determined by the container element type.
* Nothing requires that `State = T`.

---

## 26.3 `for` Loop Syntax and Lowering

Twinkle supports two `for` forms:

```tw
for x in xs { body }
for x, i in xs { body }
```

Where:

* `x` is a pattern matching the element (`Iterable.Item<XsType>`),
* `i` is an integer index starting from 0 and incrementing by 1 on each iteration,
* `i` is **independent** of the underlying `Iterable.State`.

---

### 26.3.1 `for x in xs { ... }`

As before:

```tw
for x in xs {
  body
}
```

Desugars to:

```tw
{
  let _state: Iterable.State(type_of(xs)) = Iterable.init(xs)
  loop {
    match Iterable.next(_state) {
      .Done ->
        break
      .Yield(x, next_state) -> {
        _state = next_state
        body
      }
    }
  }
}
```

Typing:

* Constraint: `Iterable<XsType>`
* `x` pattern must match `Iterable.Item<XsType>`.

---

### 26.3.2 `for x, i in xs { ... }` (indexed form)

```tw
for x, i in xs {
  body
}
```

Desugars to:

```tw
{
  let _state: Iterable.State(type_of(xs)) = Iterable.init(xs)
  let _i: int = 0

  loop {
    match Iterable.next(_state) {
      .Done ->
        break
      .Yield(x, next_state) -> {
        let i: int = _i      // bind user-visible index
        _i = _i + 1
        _state = next_state
        body
      }
    }
  }
}
```

Notes:

* `i` is **always** a zero-based sequential counter:

  * starts at 0,
  * increments by 1 per successful `Yield`,
  * does **not** depend on `Iterable.State` or how the container internally tracks progress.
* Even if `Iterable.State` is non-integer (like a cursor, pointer, or opaque handle), `i` is still just a simple `int` counter.

Typing:

* Constraint: `Iterable<XsType>`
* `x` pattern must match `Iterable.Item<XsType>`.
* `i` must be a pattern compatible with `int` (usually a simple identifier).

---

## **26.4 The `collect` Expression**

Twinkle allows:

```tw
collect for x in xs { expr }
```

This builds a new array containing the result of evaluating `expr` for each element.

Lowering:

```tw
{
  let builder = array_builder_new<ItemType>
  let state = Iterable.init(xs)

  loop {
    match Iterable.next(state) {
      .Done -> break
      .Yield(x, next) -> {
        state = next
        let element: ItemType = expr
        array_builder_push(builder, element)
      }
    }
  }

  array_builder_finish(builder)
}
```

Where:

* `ItemType = type_of(expr)`
* No short-circuiting; strict evaluation for each step.

This behaves like array comprehensions, but with explicit lowering.

---

## **26.5 Example: Arrays**

```tw
impl<T> Iterable<array<T>> {
  type Item  = T
  type State = int  // index

  fn init(xs: array<T>) -> int { 0 }

  fn next(i: int) -> Step<int, T> {
    if i < array.length(xs) {
      Step.Yield(xs[i], i + 1)
    } else {
      Step.Done
    }
  }
}
```

This matches languages where arrays are trivially iterable.

---

## **26.6 Example: Option<T>**

You may choose to make `Option<T>` iterable over 0–1 elements:

```tw
impl<T> Iterable<Option<T>> {
  type Item  = T
  type State = Option<T>

  fn init(opt: Option<T>) -> Option<T> { opt }

  fn next(s: Option<T>) -> Step<Option<T>, T> {
    match s {
      Some(v) -> Step.Yield(v, None)
      None    -> Step.Done
    }
  }
}
```

This is useful for chaining with `collect` or for ergonomically handling optional values.

---

## **26.7 Example: Strings**

Assuming UTF-8 iteration produces full decoded characters (`char`):

```tw
impl Iterable<string> {
  type Item  = char
  type State = int   // byte position

  fn init(s: string) -> int { 0 }

  fn next(i: int) -> Step<int, char> {
    if i < string.byte_length(s) {
      let (ch, next_i) = decode_next_utf8(s, i)
      Step.Yield(ch, next_i)
    } else {
      Step.Done
    }
  }
}
```

---

## **26.8 Derived Iterable: `Option<T>` or other wrappers**

Derived impls may depend on the underlying type using `where`:

```tw
impl<T> Iterable<Result<T, E>> where Iterable<T> {
  type Item  = T
  type State = Iterable.State(T)

  fn init(r: Result<T, E>) -> State {
    match r {
      Ok(v) -> Iterable.init(v)
      Err(_) -> Iterable.init(empty_container<T>)
    }
  }

  fn next(s: State) -> Step<State, Item> {
    Iterable.next(s)
  }
}
```

This shows that derived iterable types are easy to express without special syntax.

---

## **26.9 Disallowed Patterns**

To preserve predictable trait resolution:

* No overlapping `Iterable` impls for the same type head.
* No cyclic `where` dependencies (enforced by trait solver).
* No implicit lifting of iteration through nested types (must be spelled out explicitly with `where`).

---

## **26.10 Summary**

Twinkle’s iteration system is:

* **Explicit** — iteration capability must be provided by the `Iterable` trait.
* **Efficient** — each container controls its own optimized `State`.
* **Predictable** — compiler lowering is transparent, no hidden dispatch.
* **Generic** — works for arrays, strings, maps, options, and user-defined containers.

This design maintains Twinkle’s simplicity while providing powerful, ergonomic iteration with zero magic.

---
