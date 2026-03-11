# Frontend — Stages 0–2

## Stage 0 — Skeleton & Testing Infrastructure ✅

**Goal:** Basic structure and a test harness.

* Set up crate with the module layout above.
* Add a `twk` binary with stub subcommands:

  * `twk parse file.tw`
  * `twk check file.tw`
  * `twk run file.tw`
  * `twk build file.tw`
* Implement a minimal golden-test harness:

  * Read `.tw` files from `tests/parser/`,
  * For now, just assert "parses" or "returns an error".
* Wire CI (e.g. `cargo test`).

Deliverable:

* Project compiles.
* Tests run.
* No real language yet, but the skeleton is stable.

---

## Stage 1 — Lexer, Parser, Spans ✅

**Goal:** Parse full Twinkle surface syntax into an AST with precise spans.

Features:

* Tokens:

  * identifiers, keywords, literals (`Int`, `Float`, `String`, `Bool`),
  * operators (`+ - * / % == != < <= > >= and or`),
  * punctuation (`(` `)` `{` `}` `[` `]` `,` `:` `.` `:=` `=` etc.).
* Comments:

  * `//` line comments,
  * possibly doc comments (`/// ...`).
* String interpolation (spec §11):

  * Lexed as alternating `STRING_SEGMENT` + `${` *Expr* `}` tokens.
* Parser:

  * Expressions with precedence (`or` < `and` < `==` < `<` < `+ -` < `* / %`).
  * Blocks `{ ... }` as expression-with-statements.
  * `if` expressions (spec §12).
  * `case` expressions (spec §5, §12).
  * `for` / `collect` (spec §12, §13).
  * Function declarations (`fn name(...) [ReturnType] Block`) (spec §7.1).
  * Type declarations (records + sum types + type aliases) (spec §3, §5, §6).
  * Top-level statements and expressions (spec §8.1).

Every AST node carries a `Span`:

```rust
pub struct Span {
    pub file_id: FileId,
    pub start: u32,
    pub end: u32,
}

pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}
```

Deliverables:

* `twk parse file.tw` prints/unparses AST or a debug representation.
* Parser test cases:

  * Operator precedence and associativity.
  * Block nesting.
  * Basic error reporting with spans.

---

## Stage 2 — Name Resolution & Monomorphic Typechecking ✅

**Goal:** Typecheck non-generic programs with basic types and declarations.

Features:

* Type representation (monomorphic for now):

  * Primitive: `Int`, `Float`, `Bool`, `Str`, `Void` (spec §2).
  * Records: nominal record types with fields (spec §6).
  * Sum types: nominal variants (`type Result = { Ok(Int), Err(Str) }`) (spec §5).
  * Arrays & dicts: `Arr<T>`, `Dict<K,V>` (spec §14, §17).
  * Functions: `fn(T1, T2, ...) Tret` (spec §7.1).
  * Type aliases: `type ID = Int` — expands transparently, not a new nominal type (spec §3).

* Name resolution:

  * Module-level symbol table for:

    * `type` declarations,
    * `fn` declarations,
    * top-level values (spec §8.1, §8.2).
  * Basic support for qualified names in types and expressions (e.g. `Module.Point`).

* Typechecker:

  * Expression typechecking.
  * Let bindings (spec §7.2, §7.3):

    * `x := expr` (inferred).
    * `x: T = expr` (checked).
  * Function declarations and calls.
  * `if` expressions (branch type agreement) (spec §12).
  * `case` expressions (spec §5, §12):

    * scrutinee type must be a sum type.
    * arms must all produce a compatible result type.
    * basic exhaustiveness checking (can start minimal).

Deliverables:

* `twk check file.tw` reports:

  * success, or
  * clear type errors with locations.
* Typechecker tests:

  * Correct typing for simple examples.
  * Expected failures for incompatible types.
