# Parser Diagnostic Parity Plan (Rust + Boot)

Last updated: 2026-03-16

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

### Rust side (current)

- Landed: `StatementInExpression` parse error kind in
  `src/syntax/parser.rs`.
- Landed: formatted message + fix hint in `src/syntax/mod.rs`:
  - message: `'<kw>' is a statement and cannot be used where an expression is expected`
  - hint: `wrap it in a block expression, e.g. => { <kw> ... }`
- Landed: targeted test in `src/syntax/mod.rs` for `return` in `case` arm.

### Boot side (current)

- `boot/compiler/parser.tw` still emits generic `expected expression` for
  the same failure shape.
- No structured distinction yet between:
  - generic unexpected token in expression position
  - statement keyword incorrectly used in expression position

## Parity Requirements

For equivalent parse failures, both sides should agree on:

1. **Category**: statement-in-expression vs generic expected-expression.
2. **Primary span**: underline the statement keyword token itself.
3. **Core message text**: mention offending keyword explicitly.
4. **Actionable hint**: recommend block-wrapping when valid.
5. **Recovery behavior**: continue parsing to preserve subsequent items.

Exact punctuation/wording can differ slightly, but semantics must match.

## Implementation Plan

## Phase 1: Land baseline parity for statement-in-expression

### 1.1 Boot parser detection

In `boot/compiler/parser.tw`, update `parse_expr_prefix` branching so these
tokens in expression position produce specialized diagnostics:

- `return`
- `break`
- `continue`
- `defer`
- `for`

Proposed helper:

- `fn diag_statement_in_expression(tokens_in, idx, kw) Diagnostic`

Message template (boot):

- `'<kw>' is a statement and cannot be used where an expression is expected`
- append hint text:
  - `hint: wrap it in a block expression, e.g. => { <kw> ... }`

### 1.2 Boot formatting plumbing

Ensure the diagnostic message reaches CLI output unchanged in
`boot/main.tw` check/parse reporting paths (no truncation of hint line).

### 1.3 Recovery guard

Keep existing recovery behavior (`ErrorExpr` + progress of index) so
multi-item files still parse enough for subsequent diagnostics.

## Phase 2: Canonical diagnostic catalog (parser-local)

Create a lightweight shared catalog for parser diagnostics semantics:

- human key (design-level): `statement_in_expression`
- canonical intent text
- recommended hint text

Rust mapping:

- `ParseErrorKind::StatementInExpression`

Boot mapping:

- helper function/constant message templates in `boot/compiler/parser.tw`
  (or a small boot parser diagnostics module if reuse grows).

This avoids drift as more contextual diagnostics are added.

## Phase 3: Contextualized expression-position diagnostics

Extend both parsers to include context-aware phrasing where beneficial:

- case arm body
- call argument
- array element
- assignment RHS
- parenthesized expression

Example style:

- `return` cannot be used as a case-arm expression; wrap arm body in `{ ... }`.

Keep generic fallback for unknown contexts.

## Phase 4: Fixture-driven parity checks

Add shared invalid-snippet fixtures and run them through both parsers:

- Rust asserts error message class + hint presence + snapshots.
- Boot asserts diagnostic count + key substring + recovered structure.

Goal: one fixture list, two harnesses, same semantic expectation.

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

