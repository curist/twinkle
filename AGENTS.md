## Project Overview

Twinkle is a statically typed programming language targeting WebAssembly GC. It features a rank-1 polymorphic (Damas–Milner) type system with bidirectional type checking (similar to Gleam/Elm), unboxed primitives, GC-managed references, and **no trait system**—capabilities are passed explicitly as records of functions (see `docs/spec.md`).

**Primary documentation:**
- `docs/spec.md` — canonical language specification
- `docs/API.md` — built-in and standard-library API reference
- `docs/contracts.md` — builtin contract reference (`Stringify`, `Eq`, `Ord`)
- `docs/design/contracts.md` — contract-system design rationale and non-goals

**Key Design Principles:**
- Concise, low-ceremony syntax with `.tw` file extension
- Inherent methods only via module functions
- Small runtime relying on WebAssembly GC's `struct`, `array`, and reference types

## Development Commands

### Primary compiler workflow
`target/twk` is the standalone CLI produced by `make bundle-cli` (see below).
```bash
target/twk build boot/main.tw -o /tmp/boot.wasm
target/twk ir boot/main.tw --opt
target/twk run boot/tests/main.tw
```

### Debugging with WAT output
To emit human-readable WAT instead of binary Wasm, use a `.wat` output path:
```bash
target/twk build boot/main.tw -o /tmp/debug.wat   # full program WAT
target/twk build some/file.tw -o /tmp/debug.wat    # any entry file
```
This is useful for inspecting generated code, tracing Wasm function indices
from runtime stack traces, and verifying codegen correctness.

### Bootstrap the boot compiler
```bash
cargo run --release -- build boot/main.tw -o target/boot-main.wasm
```

### Build the standalone CLI
`make bundle-cli` rebuilds the self-hosted compiler payload (`target/boot.wasm`) via the
self-host loop and then builds `target/twk` as a Deno standalone executable:
```bash
make bundle-cli        # rebuild target/boot.wasm, then build ./target/twk
make quick-bundle-cli  # rebuild ./target/twk from an already-fresh target/boot.wasm
make stage2            # rebuild target/boot.wasm only (no CLI bundle)
```

### Update the tree-sitter grammar
After editing `tree-sitter-twinkle/grammar.js`, regenerate and rebuild:
```bash
cd tree-sitter-twinkle
npx tree-sitter generate   # regenerates src/parser.c, src/grammar.json, src/node-types.json
npx tree-sitter build --wasm  # requires Docker; rebuilds tree-sitter-twinkle.wasm
```
Commit `grammar.js`, the regenerated `src/` files, and `tree-sitter-twinkle.wasm` together.
The wasm is tracked in git so CI doesn't need Docker.

**Important:** Never run `tree-sitter test` from the agent. When tree-sitter work is done, ask the human to run the tests manually.

### Test
```bash
cargo test --release                # Rust test suite
target/twk run boot/tests/main.tw   # boot compiler test suite
make test                           # both of the above
make boot-test                      # boot tests only
make rust-test                      # Rust tests only
```

### Implementation focus
- Treat the boot compiler in `boot/` as the primary implementation.
- Put new compiler features, optimizations, and CLI behavior in the boot compiler.
- Update Rust stage0 in `src/` when required to bootstrap `boot/main.tw` or to keep it as a correctness reference.
- Prefer the boot compiler path over the Rust interpreter for day-to-day compiler work.

### Format Twinkle source
After editing a `.tw` file, run the formatter to ensure canonical style:
```bash
target/twk fmt path/to/file.tw
```
The formatter is idempotent — running it twice produces no further changes.

### Run Twinkle Programs
- No `main` function — top-level statements execute directly.
- `TWINKLE_ROOT` env var overrides project root (see Modules & Imports below).

## Communication Guidelines

**Focus on substance, not metrics:**
- ❌ Don't count: line numbers, test counts, assertion counts, file sizes, etc.
- ✅ Do explain: what changed, why it matters, how it works
- In documentation and commit messages, focus on **what/why/how**, not quantitative details
- Example: Instead of "Added 7 tests with 16 assertions", write "Added tests covering nested scopes, closures, and variable shadowing"

**Commit message style:**
- Match the style of recent repository commits.
- Use a short imperative subject line.
- Include a body for non-trivial changes explaining what changed and why.
- Only add `Co-Authored-By` trailers when they are actually correct for the current session/tooling.

## Language Architecture

### Value Model
- **Primitives (unboxed):** `Int` (i64), `Float` (f64), `Bool` (i32), `Void`
- **References (GC):** `String`, `array<T>`, records, `dict<K,V>`, closures

### Immutability and Rebinding
All values are immutable. Assignment syntax (`=`) is **rebinding**, not mutation.
Record field updates, array index updates, and dict updates are sugar for
"build a new value and rebind the name":

```tw
// These are equivalent:
p.x = 1                    // sugar: rebinds p to a new record
p = RecordUpdate(p, x, 1)  // desugared form

arr[i] = v                 // sugar: rebinds arr
arr = Vector.set(arr, i, v)

m[k] = v                   // sugar: rebinds m
m = Dict.set(m, k, v)
```

**Do NOT write `with_*` helper functions** that copy all record fields just to
update one or two. Use field rebinding directly:

```tw
// ❌ Don't write this:
fn with_documents(state: State, documents: Store) State {
  .{
    initialized: state.initialized,
    shutdown_requested: state.shutdown_requested,
    documents,
    query_cache: state.query_cache,
  }
}
next := with_documents(state, new_store)

// ✅ Write this instead:
state.documents = new_store
```

This applies to any record type. The only exception is functions that do
non-trivial work beyond field assignment (e.g. rebuilding indexes, filtering,
merging collections).

### Type System
- Rank-1 polymorphic (Damas–Milner) type system with bidirectional type checking
- Parametric polymorphism: `fn map<A, B>(xs: array<A>, f: fn(A) B) array<B>`
- No higher-kinded types in MVP
- Type aliases don't create distinct nominal types

### Records (Nominal, Not Structural)
Records are **nominal types** mapping to WebAssembly GC `struct` types.

**Declaration:**
```tw
type Point = .{ x: Int, y: Int }
```

**Construction (two equivalent forms):**
```tw
// Contextual anonymous literal (requires expected type)
p: Point = .{ x: 1, y: 2 }

// Named constructor form (always produces Point)
p := Point.{ x: 1, y: 2 }
```

**Important:** Anonymous `.{ ... }` literals are ONLY allowed where an expected record type is known (annotated bindings, function parameters, return expressions, record fields). They do NOT create structural types.

### Modules & Imports
- Last path segment (without extension) is the module identifier
- Aliasing: `use foo.bar as baz`
- Destructuring: `use foo.bar.{fn1, fn2, MyType}`
- Relative imports: `use .sibling` resolves from the importing file's parent namespace
- Stdlib: `use @std.fs` (prelude modules auto-imported, no `use` needed)
- Exports accessed as `module.function`, `module.Type`
- Separate namespaces for values and types
- Project root: walks up from entry file to find `twinkle.toml`; `TWINKLE_ROOT` env var overrides

### Inherent Methods (Dot Sugar)
Dot syntax supports:
1. Record fields
2. Inherent/module methods

**Example:**
```tw
// point.tw
pub type Point = .{ x: Int, y: Int }
pub fn translate(p: Point, dx: Int, dy: Int) Point { ... }
```

**Desugaring:**
```tw
p.translate(1,2)  →  Point.translate(p,1,2)
```

**Resolution order:** Check record fields first, then module inherent methods. Field vs inherent name collision is illegal.

**When to design a function as an inherent method:** Put a type as the first parameter when:
1. The type is defined in the same module (required — inherent methods only resolve for types defined in the defining module)
2. The function returns the same type (builder/transform pattern), e.g. `env.with_types(...) ResolvedEnv`
3. Multiple functions share the same receiver type, forming a cohesive API surface, e.g. `env.lookup_type(name)`, `env.has_type(name)`, `env.add_function(sig)`

### Capabilities (Explicit Records)
Twinkle does not have traits. Capabilities are passed explicitly as records of functions:

```tw
type Show<T> = .{ to_string: fn(T) String }
fn log<T>(x: T, show: Show<T>) { println(show.to_string(x)) }
```

### Naming Conventions (parser-enforced)

The parser uses the **first character** of an identifier to determine its syntactic role. These are hard rules, not style suggestions:

| Thing | Convention | Example |
|---|---|---|
| Types, variants | `PascalCase` (uppercase first) | `Point`, `Ok`, `SomeName` |
| Functions, variables, fields, modules | `snake_case` (lowercase first) | `parse_int`, `my_var`, `pt` |

**Postfix rule:** `expr.name` after an expression on the **same line**:
- `.lowercase` → field access or method call ✓
- `.Uppercase` (terminal, same line) → **parse error** — use `.lowercase` or put it on a new line
- `.Uppercase.` (intermediate qualifier, same line) → allowed (e.g. `pt.Point.{ x: 1 }`)
- `.Uppercase` on a **new line** → new statement (variant literal or constructor path) ✓

### Enums & Pattern Matching
```tw
type Option<T> = { None, Some(T) }
type Shape = { Circle(Float), Rect(Float, Float), UnitSquare }
```

Variant names must be `PascalCase`. Pattern matching must be exhaustive unless using `_ => ...`.

### Error Handling
- No exceptions
- Unrecoverable errors trap: OOB access, division by zero, explicit `error("msg")`
- Recoverable via `Result<T,E>` with `try` sugar
- `try` works with both `Result` and `Option`: early-returns `.Err(e)` or `.None` respectively
- `opt.ok_or(err)` converts `Option<T>` to `Result<T, E>` for use with `try` in Result-returning functions

```tw
// try with Option — early-returns .None
fn find(items: Vector<Item>, id: Int) Item? {
  index := try items.position(id)   // returns .None if position returns .None
  .Some(items[index])
}

// try with Result — early-returns .Err
fn parse(input: String) Result<Ast, String> {
  token := try tokenize(input)       // returns .Err(e) if tokenize fails
  .Ok(build_ast(token))
}

// .ok_or bridges Option into Result
fn lookup(reg: Registry, id: Int) Result<Entry, String> {
  entry := try reg.find(id).ok_or("not found")
  .Ok(entry)
}
```

### Control Flow
- `if` expressions: `if cond { a } else { b }`
- `case` pattern matching (exhaustive)
- `for` loops (all forms return `void`):
  - `for cond { body }`
  - `for x in coll { body }`
  - `for x,i in coll { body }`
- `collect` comprehension: `collect x in range(10) { x * x }` produces `Vector<T>`

