# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Twinkle is a statically typed programming language targeting WebAssembly GC. It features Hindley-Milner type inference (similar to Gleam/OCaml), unboxed primitives, GC-managed references, and **no trait system**—capabilities are passed explicitly as records of functions (see `docs/spec.md`).

**Key Design Principles:**
- Lightweight, scripting-like syntax with `.tw` file extension
- No trait methods callable from user code (traits = contracts only)
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

## Language Architecture

### Value Model
- **Primitives (unboxed):** `Int` (i64), `Float` (f64), `Bool` (i32), `Void`
- **References (GC):** `String`, `array<T>`, records, `dict<K,V>`, closures

### Type System
- Hindley-Milner type inference
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
p.translate(1,2)  →  point.translate(p,1,2)
```

**Resolution order:** Check record fields first, then module inherent methods. Field vs inherent name collision is illegal.

### Capabilities (Explicit Records)
Twinkle does not have traits. Capabilities are passed explicitly as records of functions:

```tw
type Show<T> = .{ to_string: fn(T) String }
fn log<T>(x: T, show: Show<T>) { println(show.to_string(x)) }
```

### Enums & Pattern Matching
```tw
type Option<T> = { None, Some(T) }
type Shape = { Circle(Float), Rect(Float, Float), UnitSquare }
```

Pattern matching must be exhaustive unless using `_ => ...`.

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

## Implementation Stages (from docs/plan.md)

The project follows a staged development approach:

**Stage 0:** Repo skeleton + test harness
**Stage 1:** Parser for expressions only (no types yet) with pretty-printer
**Stage 2:** Monomorphic typechecker (no generics, no traits)
**Stage 3:** Records, modules, inherent methods
**Stage 4:** Enums, `Option`, `Result`, `case`, `try`
**Stage 5:** Traits (contract only) + `Show` + string interpolation
**Stage 6:** Generics (HM polymorphism) + trait constraints
**Stage 7:** Backend (interpreter or WebAssembly GC text output)

## Testing Strategy (from docs/test-plan.md)

### Test Categories
- **Parser tests:** Round-trip parsing + pretty-printing, operator precedence, error reporting
- **Type tests:** Both positive (valid programs) and negative (type errors) cases
- **Module tests:** Cross-module resolution, inherent method desugaring
- **Pattern matching tests:** Exhaustiveness checking, type consistency across arms
- **Trait tests:** Constraint checking, Show implementation verification
- **Generic tests:** Polymorphic functions with/without constraints, instantiation

### Test Organization
```
tests/
  parser_cases/
  type_ok/         # Programs that should typecheck
  type_err/        # Programs that should fail with specific errors
  parser_errors/   # Malformed syntax
```

Use golden/snapshot tests (consider `insta` crate) for expected outputs.

**Critical:** Never delete old tests; each stage's tests should keep passing as new features are added.

## Important Type System Details

### String Interpolation
`"hello ${x}"` requires `x: Show`. This is the ONLY user-facing stringification facility. No `to_string` function exists.

### Iterable Trait
```tw
trait Iterable(T) {
  type Item
  type State
  fn init(x: T) -> State
  fn next(s: State) -> Step<State, Item>
}
```

Used internally by compiler for `for` loops and `collect`. User never calls these methods.

### Prelude
Implicitly imported, includes:
- Primitive functions: `print`, `println`, `len`, `error`
- Types: `Int`, `Float`, `String`, `array<T>`, `dict<K,V>`, `Option<T>`, `Result<T,E>`
- Builtin traits: `Show`, `Iterable`, `Eq`, `Ord`, arithmetic traits, indexing traits
- Range functions: `range`, `range_from`, `range_step`

Does NOT include trait methods exposed to user or any implicit global dispatch functions.

## Current Project Status

The project is in initial setup phase (Stage 0). The Cargo.toml specifies:
- Package name: `twinkle`
- Rust edition: 2024
- No dependencies yet

Source structure:
- `src/main.rs`: Currently contains only a "Hello, world!" placeholder
- `docs/`: Comprehensive specifications and implementation plans
