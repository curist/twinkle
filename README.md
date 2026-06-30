# Twinkle 🌟

[![Test](https://github.com/curist/twinkle/actions/workflows/test.yml/badge.svg)](https://github.com/curist/twinkle/actions/workflows/test.yml)
[![npm](https://img.shields.io/npm/v/%40twinkle-lang%2Ftwinkle)](https://www.npmjs.com/package/@twinkle-lang/twinkle)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Twinkle is a statically typed, value-oriented language targeting Wasm GC.

It is for writing direct, typed programs without making mutation the default:
values are immutable, errors are explicit, pattern matching is exhaustive, and
collections are persistent; everyday code still uses loops, rebinding, early
returns, and method-style calls.

Twinkle supports top-level executable code and has a self-hosted compiler written
in Twinkle itself.

**Try it:** [Twinkle Playground](https://curist.github.io/twinkle/)

## Highlights

- **Immutable values:** Rebinding and update syntax keep transformations direct without mutation.
- **Records and enums:** Nominal records, exhaustive pattern matching, and generics form the core data model.
- **Persistent collections:** `Vector` and `Dict` are ordinary values with ergonomic update and method syntax.
- **Typed control flow:** `Option`, `Result`, `try`, loops, early returns, and `case` expressions work together.
- **Module-defined APIs:** Functions can be called as methods, and contracts support interpolation, equality, and ordering.

## Quick Example

```twinkle
type Todo = .{ name: String, done: Bool }

fn complete(todo: Todo) Todo {
  todo.done = true
  todo
}

fn to_string(todo: Todo) String {
  mark := if todo.done {
    "✅"
  } else {
    "⬜"
  }
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

todos: Vector<Todo> = [
  .{ name: "parse", done: false },
  .{ name: "check", done: false },
  .{ name: "build", done: false },
]

case complete_named(todos, "build") {
  .Ok(updated) => println("updated ${updated}"), // updated [⬜ parse, ⬜ check, ✅ build]
  .Err(msg) => eprintln("error: ${msg}"),
}
```

[Run it.](https://curist.github.io/twinkle/?example=todo)

## Documentation

- **[Language Specification](docs/spec.md):** complete language reference
- **[API Reference](docs/API.md):** built-in and standard-library APIs
- **[Contracts Reference](docs/contracts.md):** `Stringify`, `Eq`, and `Ord`
- **[Contract Design Notes](docs/design/contracts.md):** rationale and design details
- **[Grammar](docs/grammar.ebnf):** formal EBNF grammar
- **[Examples](examples/):** sample Twinkle programs

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
twk lint program.tw           # report lints and rewrites
```

### Project mode

`fmt`, `lint`, `check`, `build`, `run`, and `test` also work without a file
argument. In that **project mode** they discover the nearest `twinkle.toml`
(walking up from the current directory) and operate on its configured entries:

```toml
# twinkle.toml
[project]
name = "demo"
entries = ["cmd/server.tw"]   # buildable/runnable program roots

[test]
entries = ["tests/main.tw"]   # executable test programs
```

```bash
twk fmt          # format every project-local module reachable from the entries
twk lint         # lint the same set (twk lint --fix applies safe rewrites)
twk check        # type-check the project entries (--all also checks test entries)
twk build        # build the entries to target/<name>.wasm
twk run          # run the single project entry
twk test         # run the configured test entries
```

A target name is derived from each entry file stem (`cmd/server.tw` → `server`,
`cmd/server/main.tw` → `main`). With more than one project entry, `build` needs
`--all` or `--target <name>`, and `run` needs `--target <name>`:

```bash
twk build --all              # build every project entry
twk build --target server    # build one, to target/server.wasm
twk run --target server      # run one entry
```

Passing explicit file paths always overrides project mode and operates only on
those files, exactly as before. `twk test` also accepts `--filter <substr>` and
`--verbose` (equivalent to the `TWK_TEST_FILTER` / `TWK_TEST_REPORT=verbose`
environment variables).

To run a one-off without installing, invoke it through `npx` by naming the
**package** (the bare bin name `twk` won't resolve to the scoped package):

```bash
npx @twinkle-lang/twinkle run program.tw
```

```text
❯ twk
twk - Twinkle compiler

Usage: twk <command> [options]

Commands:
  run                 Run program
  check               Type-check source
  lint                Review code: report lints and rewrites
  build               Compile to linked WAT or Wasm
  ir                  Compile and print compiler IR
  parse               Parse source and print diagnostics
  fmt                 Format source
  test                Run configured project test entries
  lsp                 Start the Language Server Protocol server
  version             Print Twinkle compiler version
  help                Print help information

Run 'twk <command> --help' for more information.
```

## License

[MIT](LICENSE)

## Acknowledgments

Twinkle is a human-directed, AI-assisted language project. The design, implementation, documentation, and tests were developed primarily through collaboration with LLM coding agents.
