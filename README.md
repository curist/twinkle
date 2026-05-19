# Twinkle 🌟

Twinkle is a small, statically typed programming language for value-oriented programs that compile to WebAssembly GC.
It is designed around immutable data, persistent collections, top-level executable code, and a self-hosted compiler written in Twinkle itself.

**Try it:** [Twinkle Playground](https://curist.github.io/twinkle/)

## Overview

Twinkle aims to make functional, persistent-data programming feel direct:

- Programs are ordinary `.tw` files with top-level statements, so scripts and applications use the same shape.
- Records, enums, pattern matching, closures, and generics form the core language.
- Values are immutable, while rebinding and update syntax keep everyday transformations concise.
- `Vector` and `Dict` are persistent collections backed by persistent-vector and HAMT-style data structures.
- `Option`, `Result`, and `try` provide typed control flow for absence and recoverable errors.
- Module functions become inherent methods through dot-call syntax, keeping APIs discoverable without extra declarations.
- Contracts provide syntax hooks for string interpolation, equality, and ordering.
- The compiler emits WebAssembly GC for portable execution on modern Wasm runtimes.

## Quick Example

```twinkle
type Todo = .{ name: String, done: Bool }

fn complete(todo: Todo) Todo {
  todo.done = true
  todo
}

fn complete_named(todos: Vector<Todo>, name: String) Todo!String {
  todo := try todos
    .find(fn(t) { t.name == name })
    .ok_or("unknown task: ${name}")

  .Ok(todo.complete())
}

todos: Vector<Todo> = [
  Todo.{ name: "parse", done: false },
  Todo.{ name: "check", done: false },
  Todo.{ name: "build", done: false },
]

case complete_named(todos, "build") {
  .Ok(todo) => println("completed ${todo.name}"),
  .Err(msg) => eprintln("error: ${msg}"),
}
```

## Language Highlights

### Records, Rebinding, and Update Syntax

Records are nominal types with concise construction syntax. Values are immutable, and assignment syntax rebinds a local name to a new value.

```twinkle
type Point = .{ x: Int, y: Int }

fn translate(p: Point, dx: Int, dy: Int) Point {
  p.x = p.x + dx
  p.y = p.y + dy
  p
}
```

The field updates above rebuild and rebind `p`. Vector and dictionary index updates follow the same value-semantics model.

### Module Functions as Methods

Module functions can form method-style APIs for the types they define.

```twinkle
type Point = .{ x: Int, y: Int }

fn translate(p: Point, dx: Int, dy: Int) Point {
  Point.{ x: p.x + dx, y: p.y + dy }
}

p := Point.{ x: 1, y: 2 }
q := p.translate(10, 20)
```

### Enums and Pattern Matching

```twinkle
type Tree<T> = {
  Empty,
  Node(T, Tree<T>, Tree<T>),
}

fn sum(t: Tree<Int>) Int {
  case t {
    .Empty => 0,
    .Node(value, left, right) => value + sum(left) + sum(right),
  }
}
```

### Option, Result, and `try`

`Option<T>` and `Result<T, E>` are built-in enum types with shorthand forms `T?` and `T!E`. The `try` expression propagates `.None` or `.Err(...)` from functions returning compatible types.

```twinkle
fn parse_pair(a: String, b: String) Int!String {
  x := try Int.from_string(a).ok_or("invalid first integer")
  y := try Int.from_string(b).ok_or("invalid second integer")
  .Ok(x + y)
}
```

### Generics, Capabilities, and Contracts

Generic functions use explicit type parameters. Behavior can be passed through ordinary records of functions, while contracts cover common syntax-level behavior.

```twinkle
fn map<A, B>(xs: Vector<A>, f: fn(A) B) Vector<B> {
  collect x in xs { f(x) }
}

type Show<T> = .{ to_string: fn(T) String }

fn log<T>(x: T, show: Show<T>) {
  println(show.to_string(x))
}

fn describe<T: Stringify>(x: T) String {
  "value=${x}"
}
```

## Documentation

- **[Language Specification](docs/spec.md)** — complete language reference
- **[API Reference](docs/API.md)** — built-in and standard-library APIs
- **[Contracts Reference](docs/contracts.md)** — `Stringify`, `Eq`, and `Ord`
- **[Contract Design Notes](docs/design/contracts.md)** — rationale and design details
- **[Grammar](docs/grammar.ebnf)** — formal EBNF grammar
- **[Examples](examples/)** — sample Twinkle programs

## Building

The compiler self-hosts. A Rust stage0 compiler bootstraps the Twinkle boot compiler, then the boot compiler rebuilds itself to a fixed point.

```bash
make bundle-cli  # rebuild target/boot.wasm, then build ./target/twk
make test        # run Rust and boot compiler test suites
```

## CLI Usage

```text
❯ target/twk --help
twk - Twinkle compiler

Usage: twk <command> [options]

Commands:
  run                 Run program
  check               Type-check source
  build               Compile to linked WAT or Wasm
  ir                  Compile and print compiler IR
  parse               Parse source and print diagnostics
  lsp                 Start the Language Server Protocol server

Run 'twk <command> --help' for more information.
```

## License

[MIT](LICENSE)

## Acknowledgments

Twinkle is built through iterative collaboration between human language design direction and AI-powered development tools.
