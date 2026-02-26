# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Twinkle is a statically typed programming language targeting WebAssembly GC. It features a rank-1 polymorphic (Damas–Milner) type system with bidirectional type checking (similar to Gleam/Elm), unboxed primitives, GC-managed references, and **no trait system**—capabilities are passed explicitly as records of functions (see `docs/spec.md`).

**Key Design Principles:**
- Concise, low-ceremony syntax with `.tw` file extension
- Inherent methods only via module functions
- Small runtime relying on WebAssembly GC's `struct`, `array`, and reference types

## Development Commands

### Build
```bash
cargo build
```

### Run
```bash
cargo run
```

### Test
```bash
cargo test
```

## Communication Guidelines

**Focus on substance, not metrics:**
- ❌ Don't count: line numbers, test counts, assertion counts, file sizes, etc.
- ✅ Do explain: what changed, why it matters, how it works
- In documentation and commit messages, focus on **what/why/how**, not quantitative details
- Example: Instead of "Added 7 tests with 16 assertions", write "Added tests covering nested scopes, closures, and variable shadowing"


## Language Architecture

### Value Model
- **Primitives (unboxed):** `Int` (i64), `Float` (f64), `Bool` (i32), `Void`
- **References (GC):** `String`, `array<T>`, records, `dict<K,V>`, closures

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
- No aliasing or destructuring in MVP
- Exports accessed as `module.function`, `module.Type`
- Separate namespaces for values and types

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

### Control Flow
- `if` expressions: `if cond { a } else { b }`
- `case` pattern matching (exhaustive)
- `for` loops (all forms return `void`):
  - `for cond { body }`
  - `for x in coll { body }`
  - `for x,i in coll { body }`
- `collect` comprehension: `collect x in range(10) { x * x }` produces `array<T>`

