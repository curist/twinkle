# Twinkle 🌟

Twinkle is a small, statically typed programming language for value-oriented programs that compile to WebAssembly GC.
It is designed around immutable data, persistent collections, top-level executable code, and a self-hosted compiler written in Twinkle itself.

**Try it:** [Twinkle Playground](https://curist.github.io/twinkle/)

## Why Twinkle?

Twinkle is inspired by functional and value-oriented languages such as Gleam, but aims for a more direct and lightweight programming style.

Immutable values, persistent collections, pattern matching, and higher-order functions are central to the language. Twinkle also embraces straightforward control flow: loops, rebinding, and early returns are all part of everyday programming.

The goal is to make value-oriented programming practical and ergonomic without pushing programmers into ceremony when direct code is clearer.

Twinkle also explores a compact WebAssembly GC–based runtime model with an emphasis on portability, tooling friendliness, and self-hosting.

## Quick Example

```twinkle
type Todo = .{ name: String, done: Bool }

fn complete(todo: Todo) Todo {
  todo.done = true
  todo
}

fn to_string(todo: Todo) String {
  mark := if todo.done { "✅" } else { "⬜" }
  "${mark} ${todo.name}"
}

fn complete_named(todos: Vector<Todo>, name: String) Vector<Todo>!String {
  target := try todos
    .find(fn(todo) { todo.name == name })
    .ok_or("unknown task: ${name}")

  updated: Vector<Todo> = []
  for todo in todos {
    if todo.name == target.name {
      todo = .complete()
    }
    updated = .append(todo)
  }

  .Ok(updated)
}

todos: Vector<Todo> = []
todos = .append(.{ name: "parse", done: false })
todos = .append(.{ name: "check", done: false })
todos = .append(.{ name: "build", done: false })

case complete_named(todos, "build") {
  .Ok(updated) => println("updated ${updated}"), // updated [⬜ parse, ⬜ check, ✅ build]
  .Err(msg) => eprintln("error: ${msg}"),
}
```

## Language Shape

- **Values are immutable.** Rebinding syntax such as `todo = .complete()` and `updated = .append(todo)` creates new values while keeping transformation code direct.
- **Records and enums are the core data model.** Records are nominal, enums pattern-match exhaustively, and both work naturally with generics.
- **Persistent collections are ordinary values.** `Vector` and `Dict` use persistent-vector and HAMT-style structures, with update syntax and method calls for ergonomic transformations.
- **Control flow is typed but familiar.** `Option`, `Result`, and `try` handle absence and recoverable errors; loops, early returns, and `case` expressions are part of everyday code.
- **Modules define APIs.** Functions become method-style calls when their first parameter is a module-defined type, and contracts provide syntax hooks for interpolation, equality, and ordering.

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
