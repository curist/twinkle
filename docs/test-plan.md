## Stage 0 – Skeleton & Harness

**Goal:** Have a way to add Twinkle snippets + expected outputs and run `cargo test`.

### What to test

* That the compiler *plumbing* works:

  * Can read a `.tw` file.
  * Can produce *some* debug string (even just “OK: parsed 0 items”).
* Test harness itself, not language features.

### How

* Add one test like:

  ```rust
  #[test]
  fn smoke_compiler_runs() {
      let src = "fn main() -> void { }";
      let out = twinklec::compile_to_debug_string(src).unwrap();
      assert!(out.contains("OK"));
  }
  ```

* If you use snapshot/golden tests:

  * `tests/snapshots/smoke.snap` with whatever output you print now.
  * You’ll rewrite later, but this proves the harness works.

> ✅ After Stage 0, you have the *workflow*: add input → run tests → see diff.

---

## Stage 1 – Parser + Pretty-Printer

**Goal:** Verify that source → AST → pretty-source is correct.

### What to test

1. **Round-trip**: parsing + pretty-printing doesn’t break structure.
2. **Operator precedence & associativity**.
3. **Error reporting** for malformed input.

### How

**1. Golden “parse/format” tests**

* Directory: `tests/parser/`

  * `simple_fn.tw`
  * `binary_ops.tw`
  * `blocks.tw`
* Each test:

  * Read `.tw`
  * Parse to AST
  * Pretty-print (normalized format)
  * Compare to expected `.out` or snapshot.

Example:

`tests/parser/simple_fn.tw`:

```tw
fn add(x: int, y: int) -> int { x + y }
```

`tests/parser/simple_fn.out`:

```tw
fn add(x: int, y: int) -> int {
  x + y
}
```

**2. Error tests**

* `tests/parser_errors/` with bad code:

  * Unclosed string
  * Missing `}` / `)` etc.
* Check:

  * `parse` returns `Err`
  * Error spans and messages are sane (`"expected '}'"` etc.)

> ✅ After Stage 1, you can trust your parser for a small subset and have concrete Twinkle syntax examples.

---

## Stage 2 – Monomorphic Typechecker (No Generics/Traits)

**Goal:** Expression-level typechecking for primitives + basic functions.

### What to test

1. **Success cases**: type inference matches annotations.
2. **Failure cases**: obvious type errors produce good messages.
3. **Edge cases**: `if` type mismatch, wrong arg counts, etc.

### How

**1. Positive type tests**

Folder: `tests/type_ok/`

Each file might contain 1–3 small functions:

```tw
fn add(x: int, y: int) -> int {
  x + y
}

fn negate(b: bool) -> bool {
  if b { false } else { true }
}
```

Compiler function:

```rust
fn typecheck_module(src: &str) -> Result<TypecheckedModule, Error>;
```

Test:

```rust
#[test]
fn type_ok_add_and_negate() {
    let src = include_str!("type_ok/add_and_negate.tw");
    let typed = typecheck_module(src).unwrap();
    // Optional: snapshot the pretty-printed typed AST
}
```

**2. Negative type tests**

Folder: `tests/type_err/`

Example `if_mismatch.tw`:

```tw
fn bad(x: int) -> int {
  if x > 0 { 1 } else { "oops" }
}
```

Test:

```rust
#[test]
fn type_error_if_mismatch() {
    let src = include_str!("type_err/if_mismatch.tw");
    let err = typecheck_module(src).unwrap_err();
    assert!(err.to_string().contains("branch types do not match"));
}
```

You don’t need perfect error text yet — “error happens at right place” is enough.

> ✅ After Stage 2, you’re confident basic type checking works and errors are visible.

---

## Stage 3 – Records, Modules, Inherent Methods

**Goal:** Test dot sugar and cross-module resolution.

### What to test

1. **Record creation and field access**.
2. **Module import + type resolution**.
3. **Inherent method desugaring** `p.m()` → `module.m(p, ...)`.

### How

**1. Record tests**

`tests/type_ok/records.tw`:

```tw
type Point = .{ x: int, y: int }

fn origin() -> Point {
  .{ x: 0, y: 0 }
}

fn shift_right(p: Point) -> Point {
  .{ x: p.x + 1, y: p.y }
}
```

Check it typechecks.

**2. Multi-module test**

Simulate a “filesystem” in tests or use real files:

* `tests/modules/point.tw`
* `tests/modules/use_point.tw`

`point.tw`:

```tw
pub type Point = .{ x: int, y: int }

pub fn translate(p: Point, dx: int, dy: int) -> Point {
  .{ x: p.x + dx, y: p.y + dy }
}
```

`use_point.tw`:

```tw
import "point"

fn main() -> void {
  let p := .{ x: 1, y: 2 }
  let q := p.translate(3, 4)
}
```

Test harness: either load those from disk or build a fake “module provider” that maps `"point"` to a string.

**3. Error tests**

* Calling unknown method: `p.rotate()`.
* Using record with missing field.

> ✅ After Stage 3, the “object-ish” story (`Point` + `p.method`) is fully tested.

---

## Stage 4 – Enums, `Option`, `Result`, `case`, `try`

**Goal:** Algebraic data types and pattern matching semantics.

### What to test

1. Enum construction + usage.
2. Exhaustive `case` on enums.
3. `Option`/`Result` patterns.
4. `try` sugar rewrite.

### How

**1. Enum + case**

`tests/type_ok/enums.tw`:

```tw
enum Shape {
  Circle(float),
  Rect(float, float),
}

fn area(s: Shape) -> float {
  case s {
    .Circle(r) => r * r * 3.14,
    .Rect(w, h) => w * h,
  }
}
```

Check typechecking success.

**2. Non-exhaustive match error**

`tests/type_err/non_exhaustive.tw`:

```tw
enum Token {
  A, B
}

fn bad(t: Token) -> int {
  case t {
    .A => 1,
  }
}
```

Check that you get an error with a message containing “non-exhaustive” / “missing B”.

**3. `Result` + `try`**

Positive:

```tw
enum Result<T, E> { Ok(T), Err(E) }

fn div(x: int, y: int) -> Result<int, string> {
  if y == 0 { Result.Err("div by zero") }
  else      { Result.Ok(x / y) }
}

fn use_div(x: int, y: int) -> Result<int, string> {
  let v := try div(x, y)
  Result.Ok(v + 1)
}
```

Negative:

* Use `try` on `int` (not `Result`) → error.
* Use `try` in a function whose return type is not `Result<_,E>`.

> ✅ After Stage 4, your ADTs + pattern matching + `try` are validated.

---

## Stage 5 – Traits + `Show` + `${x}`

**Goal:** Check that trait constraints & compiler-only methods work.

### What to test

1. `Show` impls for base types.
2. `Show` impl for user type.
3. Interpolation works iff `Show` exists.

### How

At this stage, you might *not* generate real strings yet; you can just test constraint checking.

**1. Positive Show**

`tests/type_ok/show_point.tw`:

```tw
trait Show(T) { fn show(x: T) -> string }

type Point = .{ x: int, y: int }

impl Show(Point) {
  fn show(p: Point) -> string {
    "Point(${p.x}, ${p.y})"
  }
}

fn log(p: Point) -> void {
  println("p = ${p}")
}
```

Typechecking should succeed:

* when you see `"p = ${p}"` you add constraint `Point : Show`.
* find `impl Show(Point)`.

**2. Missing Show error**

`tests/type_err/show_missing.tw`:

```tw
trait Show(T) { fn show(x: T) -> string }

type Foo = .{ n: int }

fn log(f: Foo) -> void {
  println("f = ${f}")
}
```

Expect type error:

* message: `Foo does not implement Show` or similar.

**3. Multi-trait same method name (sanity)**

You can add:

```tw
trait Show(T) { fn show(x: T) -> string }
trait Debug(T) { fn show(x: T) -> string }

type Point = .{ x:int, y:int }

impl Show(Point) { fn show(p: Point) -> string { "Point(${p.x},${p.y})" } }
impl Debug(Point){ fn show(p: Point) -> string { "DBG(${p.x},${p.y})" } }

fn log(p: Point) {
  println("p = ${p}")  // uses Show, not Debug
}
```

This should typecheck fine (no ambiguity; only `Show` is used for `${p}`).

> ✅ After Stage 5, trait constraints + `${x}` semantics are test-covered.

---

## Stage 6 – Generics + Constraints (HM)

**Goal:** Test polymorphic functions and trait constraints.

### What to test

1. Generic functions **without** constraints.
2. Generic functions **with** constraints.
3. Missing constraints cause errors.
4. Instantiation with different concrete types.

### How

**1. Generic identity**

`tests/type_ok/generic_id.tw`:

```tw
fn id<T>(x: T) -> T { x }

fn use_id() -> int {
  id(5)
}
```

* Check type inference: `id(5)` typed as `int`.

**2. Generic with `Show` constraint**

```tw
trait Show(T) { fn show(x:T) -> string }

impl Show(int) { fn show(x:int) -> string { "int" } }

fn print_twice<T: Show>(x: T) -> void {
  println("${x}, ${x}")
}

fn main() {
  print_twice(42)
}
```

Should typecheck.

**3. Missing constraint**

```tw
fn bad<T>(x: T) -> void {
  println("${x}")
}
```

Should fail with “T requires Show” or similar.

**4. Cross-module generic use**

* `lib.tw` defines `fn log<T: Show>(x:T) -> void`.
* `main.tw` imports it and uses it with various types; those that lack `Show` should fail.

> ✅ After Stage 6, you trust your generic + trait constraint story.

---

## Stage 7 – Backend / Execution

**Goal:** At least some tiny Twinkle programs can run and produce real output.

### What to test

1. Single-file programs that use:

   * `main() -> void`
   * `println`
   * basic arithmetic, strings, interpolation.
2. Multi-module programs.
3. A small “integration suite” that you can run end-to-end.

### How

Depending on backend:

**If interpreter:**

* Run Twinkle code and assert on runtime output:

  * capture stdout.
  * Example:

    ```tw
    fn main() -> void {
      println("hello")
    }
    ```
  * Test: assert stdout == "hello\n".

**If Wasm text (`.wat`) backend:**

* Golden tests:

  * Source `.tw` → expected `.wat` snippet.
* Or run through wasmtime/wasmer in tests (if you want):

  ```rust
  let wat = compile_to_wat(src);
  let wasm = wat::parse_str(&wat).unwrap();
  let output = run_wasm_main(&wasm);
  assert_eq!(output, "hello\n");
  ```

> ✅ After Stage 7, Twinkle programs are not only typechecked but actually *run* in tests.

---

## Cross-cutting Testing Practices

* **Never delete old tests**: each stage’s tests should keep passing as you add features.

* **Tag tests logically**:

  * `parser_*`, `type_*`, `trait_*`, `runtime_*`.

* **Keep some “canonical examples”**:

  * `examples/point.tw`
  * `examples/option_result.tw`
  * `examples/show_and_iterable.tw`
  * Use them as both docs and tests.

* **Error tests are as important as success tests**:

  * Every time you add a rule in the spec, ask:

    > “How do I show this fails gracefully when violated?”

