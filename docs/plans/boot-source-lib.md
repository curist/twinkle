# `boot/lib/source` — Spans, File Registry, Diagnostics

## Goal

Implement the foundational source-mapping library that all self-hosted compiler
stages depend on. This is the first prerequisite for Phase A (Frontend) of the
self-hosting plan.

## Why Now

The lexer produces tokens with `Span`, the parser builds AST nodes with `Span`,
and every stage emits `Diagnostic` which contains `Span`. The `StageResult<T>`
type from self-hosting.md depends on `Diagnostic`. None of the frontend work
can begin without this library.

## Reference

Rust stage0: `src/syntax/span.rs`.

## Responsibilities

- Represent `FileId` and `Span`.
- Span utilities: merge, contains, length, empty check.
- File registry with file text and line start offsets.
- Lookup helpers: file name, source text, snippet by span, line/col conversion,
  full line text.
- Diagnostics helpers that convert spans into stable human-readable location data.

## Target API Shape

```tw
type FileId = Int
type Span = .{ file_id: FileId, start: Int, end: Int }

// Span utilities
fn span_merge(a: Span, b: Span) Span
fn span_contains(s: Span, offset: Int) Bool
fn span_len(s: Span) Int
fn span_is_empty(s: Span) Bool

// File registry
type FileRegistry = ...
fn empty() FileRegistry
fn add_file(reg: FileRegistry, name: String, source: String) AddFileResult
fn snippet(reg: FileRegistry, span: Span) String?
fn line_col(reg: FileRegistry, span: Span) .{ line: Int, column: Int }?
fn line_text(reg: FileRegistry, span: Span) String?

// Diagnostics
type Severity = { Error, Warning, Hint, Info }
type RelatedInfo = .{ span: Span, message: String }
type Diagnostic = .{
  span: Span,
  severity: Severity,
  message: String,
  related: Vector<RelatedInfo>,
}

// Stage result (used by all compiler stages)
type StageResult<T> = .{ value: T, diagnostics: Vector<Diagnostic> }
```

## File Layout

```text
boot/lib/source/
  span.tw           # FileId, Span, span utilities
  registry.tw       # FileRegistry, add_file, snippet, line_col, line_text
  diagnostic.tw     # Severity, Diagnostic, RelatedInfo, StageResult
```

## Tests

- New suite: `boot/tests/suites/source_suite.tw`.
- Cover: line start computation, line/col boundaries, multi-line snippets,
  empty spans, out-of-bounds behavior.
- Run in both backends: `run -i` and `run`.

## Done Criteria

- API supports parser/typechecker diagnostic formatting needs.
- Deterministic outputs for same input text.
- No host interaction required.
- Suite passes in both backends.
