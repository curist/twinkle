## Stage 0 – Repo + Skeleton + Test Harness

**Goal:** Have a Rust project where you can add Twinkle snippets + golden outputs and run them in one command.

**Concrete steps**

* Create the repo:

  ```bash
  cargo new twinklec --bin
  ```

* Suggested layout:

  ```text
  twinklec/
    src/
      main.rs          // CLI entry
      lib.rs           // compiler API
      lexer.rs
      parser.rs
      ast.rs
      typecheck.rs
      hir.rs           // later
      error.rs
    tests/
      parser_cases/
      type_cases/
      compile_cases/
  ```

* Add a very simple CLI:

  ```rust
  // main.rs
  fn main() {
      let src = std::fs::read_to_string("input.tw").unwrap();
      let result = twinklec::compile_to_debug_string(&src);
      println!("{result}");
  }
  ```

* Pick a “golden test” mechanism:

  * Either:

    * use [`insta`](https://docs.rs/insta/latest/insta/) for snapshot tests, *or*
    * roll your own tiny `tests/parser_cases/*.tw` + `*.expected` harness.

**Deliverable:**

* `cargo test` runs at least one fake test that says “Twinkle compiler boots”.

---

## Stage 1 – Tiny Surface: Expressions Only (No Types Yet)

**Goal:** Be able to parse and pretty-print a tiny Twinkle subset.

**Subset:**

* Literals: `123`, `3.14`, `"hi"`, `true`, `false`
* Binary ops: `+ - * /`
* Variables: `x`, `foo_bar`
* Simple `fn` with one parameter and body as single expression:

  ```tw
  fn add(x: int, y: int) -> int { x + y }
  ```

  (you’ll *parse* the types but ignore them at first)
* Calls: `f(1,2)`
* Blocks:

  ```tw
  { a + b }
  { a; b; c }
  ```

**Work:**

* Implement `Token`, `Lexer`.
* Implement hand-written recursive-descent parser (`parser.rs`).
* Define `Ast` enums:

  ```rust
  enum Expr { Int(i64), Float(f64), Bool(bool), String(String), Var(String),
              Call { callee: Box<Expr>, args: Vec<Expr> },
              Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
              Block(Vec<Expr>) /* etc. */ }
  ```
* Implement a pretty-printer:

  ```rust
  fn format_expr(expr: &Expr) -> String
  ```

**Tests:**

* Add files like `tests/parser_cases/simple_fn.tw`:

  ```tw
  fn add(x: int, y: int) -> int { x + y }
  ```
* Expected snapshot: the pretty-printed AST or source-normalized form.

**Deliverable:**

* `cargo test` validates parsing/printing; you now have **valid Twinkle syntax examples**.

---

## Stage 2 – Monomorphic Typechecker (No Generics, No Traits)

**Goal:** Typecheck a small subset of the language with explicit types.

**Subset:**

* `int`, `float`, `bool`, `string`
* `fn` with explicit argument + return types.
* Let-bindings:

  ```tw
  x := 1
  y: int = x + 2
  ```
* `if`:

  ```tw
  if cond { a } else { b }
  ```

**Work:**

* Add simple `Type` enum:

  ```rust
  enum Type {
      Int, Float, Bool, String, Void,
      // later: Record, Array(Box<Type>), etc.
  }
  ```

* Implement an environment:

  ```rust
  struct Env { vars: HashMap<String, Type> }
  ```

* Implement `fn typecheck_expr(expr: &Expr, env: &mut Env) -> Result<Type, Error>`.

* At this stage **ignore generics and traits**:

  * Every function must have explicit argument/return types.
  * No `T`, `Option<T>`, etc. yet.
  * Just enforce: expr type matches declared type.

**Tests:**

* Positive cases: “inferred type matches declared return type”.
* Negative cases: “type mismatch in `if` branches”, “arg type mismatch on call”.
* Golden files that show:

  * input `.tw`
  * output: pretty-printed AST with types:

    ```text
    fn add(x: int, y: int) -> int {
      (x + y): int
    }
    ```

**Deliverable:**

* First real **static type errors** from Twinkle programs.

---

## Stage 3 – Records, Modules, Inherent Methods

**Goal:** Have `Point`, dot syntax, and basic modules working.

**Subset to add:**

* Record types + literals:

  ```tw
  type Point = .{ x: int, y: int }
  let p := .{ x: 1, y: 2 }
  ```
* Field access: `p.x`
* Modules:

  ```tw
  // point.tw
  pub type Point = .{ x: int, y: int }

  pub fn translate(p: Point, dx: int, dy: int) -> Point {
    .{ x: p.x + dx, y: p.y + dy }
  }
  ```
* Dot sugar:

  ```tw
  p.translate(1,2)
  ```

**Work:**

* Extend `Type` with `Record(Rc<RecordTypeId>)`.
* Maintain a **ModuleTable** in the compiler:

  ```rust
  struct ModuleTable {
      types: HashMap<String, TypeDef>,
      functions: HashMap<String, FuncSig>,
  }
  ```
* Implement:

  * `resolve_type_name("Point") -> Type`.
  * `resolve_function("point", "translate")`.
* Add dot desugaring in the AST or a later IR:

  ```tw
  p.translate(1,2) → point.translate(p,1,2)
  ```

**Tests:**

* `tests/type_cases/point_methods.tw`:

  ```tw
  import "point"

  fn main() -> void {
    let p := .{ x: 1, y: 2 }
    let p2 := p.translate(3,4)
    println("p2 = (${p2.x}, ${p2.y})")
  }
  ```
* Typechecker ensures all types, dot resolution correct.

**Deliverable:**

* Twinkle with records + inherent method sugar working end-to-end (parse → typecheck).

---

## Stage 4 – Enums, Option, Result, `if`/`case` Expressiveness

**Goal:** Add algebraic data types (enums), pattern matching, `Option`/`Result`, and `try`.

**Features to add:**

* Enum definitions + constructors:

  ```tw
  enum Option<T> { None, Some(T) }
  ```
* For now, implement `Option<int>` monomorphically (or start adding type parameters).
* `case` expression semantics & exhaustiveness.
* `Result<T,E>` and `try expr` desugaring (to `case`).

**Work:**

* Extend `Type` with `Enum(EnumId, Vec<TypeParam>)`.
* Enum environment: a table of variants & payload types.
* Implement:

  * match scrutinee type,
  * pattern binding env for arms,
  * ensure all arms return same type.
* Implement `try expr` rewrite at AST/HIR level.

**Tests:**

* Good: simple `Option<int>` match.
* Good: `Result<int,string>` with `try`.
* Bad: non-exhaustive match on enum without `_`.
* Bad: type mismatch across arms.

**Deliverable:**

* Twinkle programs using `Option`, `Result`, `case`, and `try` typecheck correctly.

---

## Stage 5 – Traits (Contract Only) + `Show` + Interpolation

**Goal:** Wire in the trait model *just enough* for `Show` and `${x}` to work.

**Features:**

* Trait declarations:

  ```tw
  trait Show(T) {
    fn show(x: T) -> string
  }
  ```
* Impl for built-in types and some user types:

  ```tw
  impl Show(int)   { fn show(x: int) -> string { ... } }
  impl Show(string){ fn show(s: string) -> string { s } }
  impl Show(Point) { ... }
  ```
* String interpolation using `Show`:

  ```tw
  "p = ${p}"
  ```

**Work:**

* Extend typechecker with **trait environment**:

  ```rust
  struct TraitImpls {
      // TraitName -> Vec<(TypeHead, ImplId)>
  }
  ```
* When parsing `trait` and `impl`, register them.
* On string interpolation:

  * Resolve type `T` of `expr`.
  * Require: there exists `impl Show(T)` in the trait impl table.
  * If not found → type error.
* No need to expose trait methods as value-level; **no dot, no `Show.show` calls**.

**Tests:**

* Good:

  ```tw
  impl Show(Point) { ... }
  println("P = ${p}")
  ```
* Bad:

  ```tw
  type Foo = .{ x: int }
  // no Show(Foo)
  println("Foo: ${f}")  // error: no Show(Foo)
  ```

**Deliverable:**

* Fully functioning `${x}` with trait-driven constraints.

---

## Stage 6 – Generics (HM Polymorphism) & Trait Constraints

**Goal:** Enable type parameters + constraints in functions.

**Features:**

```tw
fn log<T: Show>(x: T) -> void {
  println("x = ${x}")
}
```

**Work:**

* Upgrade `Type` to support type variables + schemes:

  ```rust
  enum Type {
      Var(TypeVarId),
      Int, Float, Bool, String, Void,
      Record(RecordId),
      Enum(EnumId, Vec<Type>),
      // etc.
  }

  struct Scheme {
      vars: Vec<TypeVarId>,
      constraints: Vec<TraitConstraint>, // T: Show, etc.
      ty: Type,
  }
  ```
* Algorithm:

  * At `fn` declaration, build a `Scheme` from generics + constraints.
  * At call site, instantiate scheme with fresh type vars.
  * Use unification + trait constraints (no trait-based method search).
* For interpolation inside `fn log<T: Show>`:

  * You already know `T: Show`, so treat `${x}` as consistent.

**Tests:**

* Generic `log` and `describe` functions.
* Cases where constraints missing:

  ```tw
  fn bad<T>(x: T) {
    println("${x}") // error: T: Show constraint missing
  }
  ```

**Deliverable:**

* Real HM + simple trait constraints in a clean, debuggable way.

---

## Stage 7 – Backend: “Toy Execution” or Wasm Stub

You can choose:

* **Option 1: Simple Interpreter** for typed AST:

  * Evaluate expressions in a runtime `Value` enum.
  * This gives you immediate feedback on semantics (good for tests).
* **Option 2: Wasm GC text backend**:

  * Generate `.wat` with minimal features (just integers & simple calls).
  * Expand over time to handle all Twinkle constructs.

Either way, your early tests stay valid:
you already have parsing + typechecking + desugaring tested; backend just becomes one more phase in golden tests.

---

## TL;DR strategy

1. **Build from parser → monomorphic types → records/modules → enums/case → traits for Show → HM generics.**
2. At each stage:

   * Keep the subset small.
   * Add a handful of canonical Twinkle examples.
   * Add both “good” and “error” golden tests.
3. Only then worry about Wasm codegen or runtime optimizations.

You’ll get **valid Twinkle code** running through your pipeline very early (even before traits), and by the time you add `Show` + `${x}`, you’ll already trust your infrastructure.

If you want, next step I can:

* sketch concrete Rust types for `Ast`, `Type`, `Scheme`, and the trait/impl tables, so you can basically paste them into `ast.rs` / `typecheck.rs` and start filling in functions.
