# Parser Diagnostic Parity Plan (Rust + Boot)

Last updated: 2026-03-20

## Goal

Make parser diagnostics precise, actionable, and behaviorally aligned between:

- Rust parser: `src/syntax/parser.rs` + formatter in `src/syntax/mod.rs`
- Boot parser: `boot/compiler/parser.tw` (+ CLI formatting in `boot/main.tw`)

Initial driver case: statement keywords used where expressions are required
(for example `case` arm body `=> return 1`).

## Scope

In scope:

- Parser-stage diagnostics only (message quality, span accuracy, recovery behavior)
- Cross-implementation parity for equivalent syntax errors
- Regression tests for both Rust and Boot pipelines

Out of scope:

- Resolver/type-checker diagnostics
- Full diagnostic code system across all compiler stages
- LSP transport details

## Current State

### Phase 1: COMPLETE (2026-03-20)

Both parsers now emit matching statement-in-expression diagnostics.

### Rust side

- `StatementInExpression` parse error kind in `src/syntax/parser.rs`
  for `return`, `break`, `continue`, `defer`, `for`.
- Formatted message + fix hint in `src/syntax/mod.rs`.
- Snapshot fixtures: `tests/parser_errors/statement_in_expression_{return,break,continue,defer,for}.tw`.

### Boot side

- `parse_prefix` in `boot/compiler/parser.tw` has explicit cases for
  `.Return`, `.Break`, `.Continue`, `.Defer`, `.For` — each emits
  the parity message and hint, then recovers with `ErrorExpr` + advance.
- Tests in `boot/tests/suites/parser_suite.tw`:
  - 5 keyword message/hint checks (assert `is a statement`, `where an expression is expected`, `wrap it in a block expression`)
  - 1 span check (`return` keyword span = 6 chars)
  - 1 recovery check (bad arm + following valid function still parsed)
  - 2 no-false-positive checks (valid `return`/`for` emit no diagnostic)

### Phase 2: COMPLETE (2026-03-20)

Boot parser: extracted `diag_statement_in_expression(c, kw)` helper,
5 case arms now delegate to it. Single message template, no duplication.

### Phase 3: COMPLETE (2026-03-20)

Context-aware messages for case arm body, call argument, array element,
grouped expression. Both parsers emit matching context strings.

### Phase 4: COMPLETE (2026-03-20)

Fixture-driven parity checks. Both parsers now validate against shared
`.tw` fixture files in `tests/parser_errors/`.

**Shared fixtures (8 total):**
- Keyword matrix (case arm body context): `statement_in_expression_{return,break,continue,defer,for}.tw`
- Context matrix (return in various positions): `statement_in_expression_{call_arg,array,grouped}.tw`

**Rust harness:** `test_parser_error_cases()` in `tests/integration_test.rs`
auto-discovers all `.tw` files, asserts parse failure + location info, snapshots error message.

**Boot harness:** fixture parity section in `boot/tests/suites/parser_suite.tw`
reads each fixture via `fs.read_text`, parses, asserts diagnostic count > 0,
validates message contains keyword, context, and hint substrings.
Helper: `assert_fixture_diagnostic(src, keyword, context)`.

## Parity Requirements

For equivalent parse failures, both sides should agree on:

1. **Category**: statement-in-expression vs generic expected-expression.
2. **Primary span**: underline the statement keyword token itself.
3. **Core message text**: mention offending keyword explicitly.
4. **Actionable hint**: recommend block-wrapping when valid.
5. **Recovery behavior**: continue parsing to preserve subsequent items.

Exact punctuation/wording can differ slightly, but semantics must match.

## Implementation Plan

## Phase 1: Land baseline parity for statement-in-expression — DONE

Inline message strings in each `parse_prefix` case arm (no shared helper
needed at this scale). Recovery via `ErrorExpr` + `c.advance()` preserved.

## Phase 2: Canonical diagnostic catalog (parser-local) — DONE

Rust: `ParseErrorKind::StatementInExpression { statement }` + single format
in `src/syntax/mod.rs`.

Boot: `diag_statement_in_expression(c, kw)` helper in
`boot/compiler/parser.tw` — one template, called from 5 case arms.
Message uses string interpolation so the keyword and hint are generated
from the single `kw` argument.

## Phase 3: Contextualized expression-position diagnostics — DONE

Both parsers now include context in the diagnostic message when a
statement keyword appears in a known expression context.

Rust: `parse_expr_in(context)` wraps `parse_expr` and enriches
`StatementInExpression` errors with an optional `context` field.
Used at: case arm body, call argument, array element, grouped expression.

Boot: `parse_expr_in(c, context)` pre-checks for statement keywords
before delegating to `parse_expr_bp`, emitting a context-enriched
message directly. `parse_expr_list_ctx` threads context into list
parsing (used for call arguments). Also applied at: array element,
grouped expression, case arm body.

Message format: `'<kw>' is a statement and cannot be used where an
expression is expected in <context>`.

Generic fallback (no context) preserved for all other positions.

## Phase 4: Fixture-driven parity checks — DONE

Shared `.tw` fixture files in `tests/parser_errors/` run through both parsers:

- Rust: auto-discovers fixtures, asserts parse failure, snapshots full error message.
- Boot: reads each fixture via `fs.read_text`, asserts diagnostic count + key
  substrings (keyword, context, hint) via `assert_fixture_diagnostic` helper.

Added 3 new context-specific fixtures (`call_arg`, `array`, `grouped`) alongside
the existing 5 keyword-matrix fixtures. Both harnesses validate all 8.

## Test Plan

## Rust-side tests (`tests/` + `src/syntax/mod.rs`)

### A. Core regression (already landed baseline)

- `case` arm with `return` expression body:
  - input: `case 1 { 1 => return 1, _ => 0 }`
  - expect: statement-specific message + hint

### B. Token coverage matrix

For each keyword in `{return, break, continue, defer, for}`:

- position: case arm body (`1 => <kw> ...`)
- expect: keyword appears in message; hint appears

### C. Context coverage

Use `return` representative in each context:

- call arg: `foo(return 1)`
- array element: `[return 1]`
- assignment RHS: `x = return 1`
- grouped expression: `(return 1)`

Expected:

- parse fails
- error points at `return`
- hint present

### D. No-regression controls

- valid statement usage remains valid:
  - `fn f() Int { return 1 }`
  - `fn f() { for x in xs { continue } }`
- no statement-in-expression diagnostic in valid files.

### E. Snapshot fixtures (`tests/parser_errors/*.tw`)

Add/extend fixtures:

- `statement_in_expression_return.tw`
- `statement_in_expression_break.tw`
- `statement_in_expression_continue.tw`
- `statement_in_expression_defer.tw`
- `statement_in_expression_for.tw`

Snapshots should lock:

- first-line message clarity
- hint line presence
- caret location at offending keyword

## Boot-side tests (`boot/tests/suites/parser_suite.tw`)

Add a focused subsection: `diagnostics clarity: statement in expression`.

### A. Message/hint checks

For each keyword in `{return, break, continue, defer, for}` in expression
position:

- parse source
- assert `diagnostics.len() > 0`
- assert first diagnostic message contains:
  - `is a statement`
  - `where an expression is expected`
  - `wrap it in a block expression`

### B. Span checks

For representative cases (`return`, `for`):

- assert primary diagnostic span starts on keyword token span
  (compare to token stream span from lexer output in test).

### C. Recovery checks

Single file with one bad arm and one following valid function:

- expect diagnostics present
- expect second function still parsed as `.Function`

### D. No-regression controls

- valid `return` statement in block should not emit diagnostic
- valid loop statements should not emit statement-in-expression diagnostic

## Cross-Parity Acceptance Criteria

This plan is complete when:

1. Rust + Boot both emit statement-specific diagnostics for the same fixture
   class.
2. Both include actionable block-wrap hint.
3. Both underline the offending keyword token.
4. Recovery behavior is demonstrated in both suites.
5. New fixture additions require updating both harnesses (parity discipline).

## Work Sequencing

1. Land Boot Phase 1 (minimal parity with current Rust behavior).
2. Expand Rust + Boot matrix tests in parallel.
3. Add fixture-driven parity harness discipline.
4. Iterate contextual diagnostics (Phase 3) once baseline parity is green.

## Risks and Mitigations

- Risk: wording drift between implementations.
  - Mitigation: centralized template strings + fixture assertions on key
    substrings, not exact full sentence where unnecessary.
- Risk: parser recovery regressions while specializing diagnostics.
  - Mitigation: explicit recovery tests with trailing valid items.
- Risk: overfitting to `return` only.
  - Mitigation: enforce full keyword matrix in both suites.

