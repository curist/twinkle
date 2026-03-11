# String Escape Sequence Support Plan

## Goal

Allow ergonomic control-character literals in source strings (for example ANSI
escape), so users can write:

```tw
"\x1b[31mred\x1b[0m"
```

instead of:

```tw
fn esc() String {
  case String.from_char_code(27) {
    .Some(s) => s,
    .None => "",
  }
}
```

## Current State

Twinkle currently supports these escapes in string literals:

- `\n`, `\t`, `\r`
- `\"`, `\\`, `\$`

`src/syntax/lexer.rs` handles this via `escape_char()`, which returns a single
`char`. There is no hex or Unicode escape form.

## Scope

### Phase 1 (required)

Add two new escapes:

- `\xNN` (exactly 2 hex digits, ASCII range only `00..7F`)
- `\e` (alias for ESC, equivalent to `\x1b`)

### Phase 2 (optional follow-up)

Add `\u{...}` Unicode scalar escapes:

- 1 to 6 hex digits
- reject invalid scalars and surrogate range

## Non-Goals

- Raw string syntax (`r"..."`)
- Arbitrary non-UTF-8 bytes in `String`
- New runtime string representation

## Design

### Semantics

- Escape processing remains a lexer responsibility.
- String token `.text` continues to hold the already-decoded string content.
- Interpolation behavior is unchanged:
  - `$` starts interpolation
  - `\$` is a literal `$`

### Validation Rules

Phase 1:

- `\x` must be followed by exactly 2 hex digits.
- Decoded value must be `<= 0x7F`, otherwise lex error.
- `\e` decodes to U+001B.

Phase 2:

- `\u{...}` must contain 1..6 hex digits and closing `}`.
- Must decode to a valid Unicode scalar.

### Diagnostics

Keep existing `InvalidEscape(char)` for simple bad escapes.
Add targeted lexer errors for structured escapes:

- `InvalidHexEscape`
- `InvalidUnicodeEscape`

These should include spans that cover the full escape sequence.

## Implementation Tasks

1. Lexer updates (`src/syntax/lexer.rs`)
- Refactor escape handling to parse multi-character escapes.
- Implement `\xNN` and `\e`.
- Keep interpolation string-continuation path using the same escape parser.

2. Token/Parser impact
- No token shape changes expected.
- Parser should require no functional changes.

3. Spec/docs updates
- Add explicit escape grammar and semantics to `docs/spec.md` string section.
- Add examples including ANSI coloring.

4. Tests
- Extend lexer unit tests:
  - valid: `\x1b`, `\x0A`, `\e`
  - invalid: `\x`, `\xG0`, `\x80`, unterminated forms
- Add runtime tests under `tests/run/` to verify:
  - ANSI escape rendering path compiles/runs in interpreter and wasm
  - escaped bytes have expected `char_code_at` values
- Add parser/diagnostic snapshot coverage for new lexer errors.

## Checklist

### Phase 1: Lexer and diagnostics

- [x] Refactor escape parsing in `src/syntax/lexer.rs` so structured escapes can consume multiple chars
- [x] Add `\xNN` parsing with exactly two required hex digits
- [x] Enforce Phase 1 ASCII bound for `\xNN` (`00..7F`) and reject out-of-range values
- [x] Add `\e` as an alias for ESC (`U+001B`)
- [x] Keep `\$` behavior unchanged so interpolation suppression still works
- [x] Add lexer error variants/messages for structured escape failures (`InvalidHexEscape`, span coverage)

### Phase 1: Test coverage

- [x] Extend lexer unit tests for valid escapes (`\x1b`, `\x0A`, `\e`)
- [x] Extend lexer unit tests for invalid escapes (`\x`, `\xG0`, `\x80`, unterminated forms)
- [x] Add runtime fixture proving ANSI escape strings compile and run in interpreter mode
- [x] Add runtime fixture proving ANSI escape strings compile and run in wasm mode
- [x] Add diagnostic snapshot coverage for new lexer escape errors

### Phase 1: Spec and docs

- [x] Update `docs/spec.md` string section with supported escape forms and rules
- [x] Document `\xNN` constraints and `\e` alias with at least one ANSI example
- [x] Add a short migration note showing replacement of `String.from_char_code(27)` patterns

### Phase 2 (optional): Unicode escapes

- [x] Add `\u{...}` parser (1..6 hex digits, closing `}`)
- [x] Reject invalid Unicode scalar values (including surrogates)
- [x] Add lexer tests for valid/invalid `\u{...}` cases
- [x] Update spec/docs to include Unicode escape semantics and examples

## Compatibility and Rollout

- Backward compatible: existing escapes keep behavior.
- Existing code using `String.from_char_code(27)` continues to work.
- No runtime or backend ABI changes required for Phase 1.

## Open Questions

1. Should `\xNN` remain ASCII-only permanently, or later allow full byte range
with explicit UTF-8 decoding rules?
2. Do we want `\0` as a dedicated alias for NUL in Phase 1?
3. Should `\e` be supported long-term, or only `\x1b` for minimal syntax?
