# Boot Parser Test Coverage Plan

## Goal

Bring the boot parser test suite (`boot/tests/suites/parser_suite.tw`) to
comprehensive coverage of all grammar productions, surfacing any unimplemented
syntax in the boot parser.

## Current State

The boot parser suite has 70 tests. Three tests intentionally fail to surface
parser gaps (hex literals, `!E` void-result type, `collect` comprehension).

## Parser Implementation Gaps (revealed by failing tests)

| Feature | Test | Status |
|---|---|---|
| Hex integer literals `0xFF` | `expr: hex integer literal` | **Missing in parser** |
| Void-result type `!E` | `type expr: void result !E` | **Missing in parser** |
| `collect` comprehension | `expr: collect comprehension` | **Missing in parser** |

## Coverage Tracker

### Imports

| Production | Status |
|---|---|
| Bare `use foo.bar` | Covered |
| Aliased `use foo.bar as baz` | Covered |
| Stdlib `use @std.fs` | Covered |
| Relative `use .helper` | Covered |
| Relative nested `use .sub.mod` | Covered |
| Destructuring `use m.{f, type T}` | Covered |
| Destructuring with aliases | Covered |

### Type Declarations

| Production | Status |
|---|---|
| Record with type params | Covered |
| Sum with type params | Covered |
| Type alias | Covered |
| `pub type` visibility | Covered |
| Multi-payload variant `V(T1, T2)` | Covered |

### Function Declarations

| Production | Status |
|---|---|
| Basic `fn name(params) Ret {}` | Covered |
| `pub fn` visibility | Covered |
| Generic `fn name<T>(...)` | Covered |
| No return type (implicit void) | Covered |
| No params `fn foo() {}` | Covered |

### Expressions — Literals & Primaries

| Production | Status |
|---|---|
| Integer literal | Covered |
| Float literal | Covered |
| String literal | Covered |
| String interpolation | Covered |
| Bool literal (`true`/`false`) | Covered |
| Hex literal `0xFF` | Failing (parser gap) |
| Identifier | Covered |
| `( expr )` grouping | Covered |

### Expressions — Compound

| Production | Status |
|---|---|
| `if cond {} else {}` | Covered |
| `if/else if/else` chain | Covered |
| `case expr { arms }` | Covered |
| Block expression `{ stmts; tail }` | Covered |
| Array literal `[a, b]` | Covered |
| Record literal `.{ x: 1 }` | Covered |
| Record field punning `.{ x }` | Covered |
| Variant `.None` / `.Some(x)` | Covered |
| Closure `fn(x) { body }` | Covered |
| Closure without return type | Covered |
| `collect x in e { body }` | Failing (parser gap) |

### Expressions — Operators

| Production | Status |
|---|---|
| Binary `+`, `-`, `*`, `/`, `%` | Covered |
| Comparison `==`, `!=`, `<`, `<=`, `>`, `>=` | Covered |
| Logical `and`, `or` | Covered |
| Bitwise `&`, `\|`, `^` | Covered |
| Range `..` | Covered |
| Unary `-` (negation) | Covered |
| Unary `!` (not) | Covered |
| Unary `~` (bitwise not) | Covered |
| Unary `try` | Covered |
| Precedence: mul before add | Covered |
| Precedence: comparison vs logical | Covered |

### Expressions — Postfix

| Production | Status |
|---|---|
| Function call `f(args)` | Covered |
| Field access `a.b` | Covered |
| Index `a[i]` | Covered |
| Method call chain `a.b().c()` | Covered |

### Statements

| Production | Status |
|---|---|
| `let` with `:=` (inferred) | Covered |
| `let` with `: T =` (explicit) | Covered |
| Rebind `x = expr` | Covered |
| `for x in coll {}` | Covered |
| `for x, i in coll {}` | Covered |
| `for cond {}` (while-style) | Covered |
| `return expr` / bare `return` | Covered |
| `break` | Covered |
| `continue` | Covered |
| `defer expr` | Covered |

### Type Expressions

| Production | Status |
|---|---|
| Simple path `Int`, `foo.Bar` | Covered |
| Generic `Vector<Int>` | Covered |
| Nested generic `Dict<String, Vector<Int>>` | Covered |
| Function type `fn(Int) Bool` | Covered |
| Optional `T?` | Covered |
| Result `T!E` | Covered |
| Void result `!E` | Failing (parser gap) |

### Patterns

| Production | Status |
|---|---|
| Wildcard `_` | Covered |
| Identifier binding | Covered |
| Literal (int, bool) | Covered |
| Variant `.Some(x)` / `.None` | Covered |
| Nested variant `Some(Some(x))` | Covered |
| Qualified `Type.Variant(x)` | Covered |

### Error Recovery

| Case | Status |
|---|---|
| Malformed `use`, parse continues | Covered |
| Missing operand in expression | Covered |
| Unterminated string | Covered |

### Not Yet Tested

| Feature | Notes |
|---|---|
| Shift operators `<<`, `>>` | Not in boot AST `BinOp` yet |
| Named record literal `Point.{ x: 1 }` | Postfix form, needs careful testing |
| Naming convention enforcement | Parser rejects uppercase fn/var names |
| Constructor-in-postfix rejection | `x.Some` should be rejected |
| `pub` on top-level let bindings | |
| Destructuring import edge cases | Empty braces, trailing comma, relative+destructuring |
