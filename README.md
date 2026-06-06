# Twinkle 🌟

[![Test](https://github.com/curist/twinkle/actions/workflows/test.yml/badge.svg)](https://github.com/curist/twinkle/actions/workflows/test.yml)
[![npm](https://img.shields.io/npm/v/%40twinkle-lang%2Ftwinkle)](https://www.npmjs.com/package/@twinkle-lang/twinkle)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Twinkle is a statically typed programming language with a simple, value-oriented core and a small WebAssembly GC runtime.

It is designed around immutable data, persistent collections, top-level executable code, and a self-hosted compiler written in Twinkle itself.

**Try it:** [Twinkle Playground](https://curist.github.io/twinkle/)

## Why Twinkle?

Twinkle is for writing direct, typed programs without making mutation the default.

Many languages make you choose: functional languages offer strong data modeling
and explicit failure, but often come with unfamiliar control flow; imperative
languages feel direct, but mutation and hidden failure can make code harder to
reason about.

Twinkle tries to keep the directness without giving up the guarantees. Values are
immutable, errors are explicit, pattern matching is exhaustive, and collections
are persistent — but ordinary code still uses loops, rebinding, early returns,
and method-style calls.

## Highlights

Key language features:

- **Values are immutable.** Rebinding syntax such as `todo = .complete()` and `updated = .append(todo)` creates new values while keeping transformation code direct.
- **Records and enums are the core data model.** Records are nominal, enums pattern-match exhaustively, and both work naturally with generics.
- **Persistent collections are ordinary values.** `Vector` and `Dict` use persistent-vector and HAMT-style structures, with update syntax and method calls for ergonomic transformations.
- **Control flow is typed but familiar.** `Option`, `Result`, and `try` handle absence and recoverable errors; loops, early returns, and `case` expressions are part of everyday code.
- **Modules define APIs.** Functions become method-style calls when their first parameter is a module-defined type, and contracts provide syntax hooks for interpolation, equality, and ordering.

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

## Documentation

- **[Language Specification](docs/spec.md)** — complete language reference
- **[API Reference](docs/API.md)** — built-in and standard-library APIs
- **[Contracts Reference](docs/contracts.md)** — `Stringify`, `Eq`, and `Ord`
- **[Contract Design Notes](docs/design/contracts.md)** — rationale and design details
- **[Grammar](docs/grammar.ebnf)** — formal EBNF grammar
- **[Examples](examples/)** — sample Twinkle programs

## Install from npm

Twinkle ships on npm as [`@twinkle-lang/twinkle`](https://www.npmjs.com/package/@twinkle-lang/twinkle),
providing both the `twk` CLI and an embeddable compile/run library:

```bash
npm install -g @twinkle-lang/twinkle   # CLI
npm install @twinkle-lang/twinkle      # library
```

See [docs/js-embedding.md](docs/js-embedding.md) for CLI usage and the
JavaScript embedding/extern-wiring guide.

## CLI Usage

After a global install (`npm install -g @twinkle-lang/twinkle`), use `twk` directly:

```bash
twk run program.tw            # compile and run
twk build program.tw -o out.wasm
twk check program.tw          # type-check only
twk fmt program.tw            # format in place
```

To run a one-off without installing, invoke it through `npx` by naming the
**package** (the bare bin name `twk` won't resolve to the scoped package):

```bash
npx @twinkle-lang/twinkle run program.tw
```

Full command list:

```text
❯ twk --help
twk - Twinkle compiler

Usage: twk <command> [options]

Commands:
  run                 Run program
  check               Type-check source
  build               Compile to linked WAT or Wasm
  ir                  Compile and print compiler IR
  parse               Parse source and print diagnostics
  fmt                 Format source
  lsp                 Start the Language Server Protocol server

Run 'twk <command> --help' for more information.
```

## License

[MIT](LICENSE)

## Acknowledgments

Twinkle is a human-directed, AI-assisted language project. The design, implementation, documentation, and tests were developed primarily through collaboration with LLM coding agents.
