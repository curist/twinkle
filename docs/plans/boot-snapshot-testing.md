# Boot Compiler Snapshot Testing

Last updated: 2026-03-20

## Background

[Parser Diagnostic Parity](archive/parser-diagnostic-parity.md) phases 1–4
established shared `.tw` fixtures in `tests/parser_errors/` and validated
them from both the Rust and boot parsers. However, the boot side only
performs substring assertions (`msg.contains("is a statement")`), while
the Rust side snapshots the entire formatted error via `insta`. This
means the boot compiler's diagnostic output can silently drift without
detection.

## Goal

Add snapshot testing to the boot compiler so that its formatted
diagnostic output is locked against `.expected` files — same discipline
the Rust side gets from `insta` snapshots.

## Scope

In scope:

- A `format_diagnostic` function in boot that produces a deterministic
  single-string representation of a `Diagnostic` (message + span context).
- `.expected` snapshot files alongside fixtures, compared by boot tests.
- An update mode (`TWK_SNAP_UPDATE=1`) that writes new `.expected` files
  when they're missing or explicitly being refreshed.
- Parser diagnostic fixtures as the initial coverage set.

Out of scope:

- Resolver/checker diagnostics (future extension, same mechanism).
- Binary/structural snapshot formats.
- Cross-compiler identical output (messages may differ slightly; the
  point is each compiler locks its own output).

## Design

### Snapshot format

Each fixture `tests/parser_errors/foo.tw` gets a companion
`tests/parser_errors/foo.boot.expected` containing the formatted
diagnostic output the boot parser produces. One line per diagnostic,
format:

```
<line>:<col>: <message>
```

Example `statement_in_expression_return.boot.expected`:

```
3:10: 'return' is a statement and cannot be used where an expression is expected in case arm body
hint: wrap it in a block expression, e.g. `=> { return ... }`
```

We intentionally omit the file name (it varies by path) and source
context lines (they duplicate the fixture). This keeps snapshots
stable and diff-friendly.

### `format_diagnostic` helper

Add to `boot/lib/source/diagnostic.tw` (or a new
`boot/lib/source/format.tw`):

```tw
pub fn format_diagnostic(reg: FileRegistry, d: Diagnostic) String
```

Produces the canonical snapshot string for one diagnostic:
`<line>:<col>: <message>`. The registry resolves span → line/col.

### Test flow

```
read fixture .tw file
  → register source in FileRegistry
  → parser.parse(source, file_id)
  → format each diagnostic via format_diagnostic
  → join with newline → actual_output
  → read .boot.expected file
  → compare actual_output vs expected
  → if mismatch and TWK_SNAP_UPDATE=1: overwrite .expected, pass
  → if mismatch and no update mode: fail with diff
  → if .expected missing and TWK_SNAP_UPDATE=1: write it, pass
  → if .expected missing and no update mode: fail
```

### Helpers

In `boot/tests/suites/parser_suite.tw`:

```tw
fn assert_snapshot(fixture_path: String) Result<Void, String>
```

Encapsulates the full read-parse-format-compare flow. Each fixture
test becomes a one-liner.

## Implementation Plan

### Phase 1: format_diagnostic + snapshot assertion helper

1. Add `format_diagnostic(reg, d)` to `boot/lib/source/diagnostic.tw`
   or a sibling file. Format: `<line>:<col>: <message>`.
2. Add `assert_snapshot(fixture_path)` helper to `parser_suite.tw`:
   - Reads `.tw` fixture, registers in a fresh `FileRegistry`.
   - Parses, formats diagnostics, joins.
   - Reads `.boot.expected` (or writes it if `TWK_SNAP_UPDATE=1`).
   - Compares strings.
3. Convert existing fixture parity tests to use `assert_snapshot`.

### Phase 2: Generate initial .expected files

Run tests with `TWK_SNAP_UPDATE=1` to generate all `.boot.expected`
files for the 8 existing `statement_in_expression_*.tw` fixtures.
Commit them.

### Phase 3: Extend to other parser error fixtures

Apply `assert_snapshot` to the remaining `tests/parser_errors/` fixtures
(`case_missing_comma`, `missing_operand`, `missing_paren`,
`unterminated_string`, `invalid_hex_escape`, `invalid_unicode_escape`).
Generate their `.boot.expected` files.

### Phase 4 (future): Resolver/checker snapshots

Same mechanism, different fixture directories. The `format_diagnostic`
helper is already stage-agnostic.

## Test Plan

- All existing fixture parity tests continue to pass (they're replaced
  by stronger snapshot assertions, not removed).
- `TWK_SNAP_UPDATE=1` generates correct `.expected` files.
- Intentionally changing a diagnostic message causes a test failure
  (not a silent pass).
- Adding a new `.tw` fixture without a `.boot.expected` fails until
  snapshots are generated.

## Risks and Mitigations

- **Risk:** Snapshot churn from formatting changes.
  - **Mitigation:** Keep format minimal (line:col + message only, no
    source context or caret). Formatting changes are intentional and
    should be reviewed.
- **Risk:** `TWK_SNAP_UPDATE` accidentally left on in CI.
  - **Mitigation:** Default is strict (no update). CI never sets the
    env var.
- **Risk:** File I/O in tests is slow.
  - **Mitigation:** Already proven fast — existing fixture tests read
    files without measurable overhead.
