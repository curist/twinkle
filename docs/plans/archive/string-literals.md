# Raw and multiline string literals

**Status:** COMPLETE — shipped across both lexers, the formatter, the
tree-sitter grammar, spec/API docs, and tests; self-host fixed point holds. All
milestones (1–3), the formatter (M4a), and lint L1 are done. The optional L2
lint and F1 layout for non-tail multiline contexts are intentionally deferred.

## Goal

Add two new string-literal forms to Twinkle, both purely at the lexer level:

- **`r"…"` — raw single-line.** No backslash-escape processing, so a regex like
  `\d+` is written `r"\d+"` instead of `"\\d+"`. Interpolation still works.
- **`\\`-prefixed lines — raw multiline.** Zig-style line-prefixed blocks for SQL,
  HTML, help text, and other embedded content, with clean indentation and local
  error recovery. Interpolation still works.

Neither form changes the AST, parser, type checker, or codegen: raw-vs-cooked and
single-vs-multi are fully resolved in the lexer, which emits the **same string
tokens** (`StringLit` / `StringStart` / `StringContinue` / `StringEnd`) it emits
today. Everything downstream sees an ordinary decoded `String`.

This supersedes `docs/plans/archive/regexp-percent-escapes.md`: with `r"…"`,
`regexp.must(r"\d+")` needs no `%`-aliases and no doubled backslashes, and the
regex stays portable standard syntax.

## Why these two forms

The pain points are on two orthogonal axes — *escaping* and *line-spanning* —
and Twinkle's existing `"…"` is cooked, single-line, and interpolated. The
universal rule across all forms is:

> **`${…}` interpolation is always on. The only knobs are escaping (`r` and `\\`
> turn off backslash escapes) and shape (`"…"` single-line vs `\\`-lines multi).**

| Form | Escaping | Lines | Interp | Primary use |
|---|---|---|---|---|
| `"…"` | cooked | single | yes | normal strings (unchanged) |
| `r"…"` | raw | single | yes | regex: `r"\d+"` |
| `\\`-lines | raw | multi | yes | SQL / HTML / help text |

There is deliberately **no `"""` triple-quote form**. The Zig-style `\\` block was
chosen over closing-delimiter triple-quotes because it solves three problems
structurally rather than by heuristic:

1. **Indentation.** The `\\` marker shows where each line's content begins, so the
   marker's own indentation is *not* part of the value. Code indentation and
   string indentation are cleanly separated — no "strip common leading
   whitespace" rule, no tabs-vs-spaces edge cases.
2. **Error recovery.** There is no closing delimiter to forget. A block ends at
   the first line that does not start with `\\`, exactly like a `//` comment ends
   at end-of-line. A typo can never swallow the rest of the file.
3. **Line-start legibility.** Every content line is explicitly marked, so leading
   whitespace and block boundaries are always visible.

The honest tradeoff: every line carries a `\\` marker (heavier than a triple-quote
block), and there are no inline escapes (`\t` etc.) — use a literal tab or
interpolate `${"\t"}`. For the target use cases this is fine.

## Semantics

### `r"…"` — raw single-line

```tw
r"\d+"            // value: \d+   (two chars: backslash, d, then +)
r"C:\temp"        // value: C:\temp
r"a ${x} b"       // interpolation still fires
```

- `\` is an ordinary character; `unescape_string` is **not** applied.
- The string still terminates at `"` and still triggers interpolation at `${`.
  Because `\` is literal, **a raw single-line string cannot contain `"`** (there
  is no `\"` escape). This is the standard raw-string limitation; for regex and
  paths it does not come up. Patterns that need a literal `"` use cooked `"…"` or
  a `\\` block.
- A literal newline inside `r"…"` is an error (`unterminated string literal`),
  same as `"…"`.
- `r` is a raw-string prefix **only** when immediately followed by `"`
  (`r"…"`). `r` followed by anything else lexes as an ordinary identifier, so
  `range`, `red`, and a variable named `r` are unaffected. This is safe because
  an identifier can never be immediately followed by `"` in a valid expression.

### `\\`-lines — raw multiline

```tw
query :=
  \\SELECT *
  \\FROM users
  \\WHERE active = ${flag}
```

Value: `SELECT *\nFROM users\nWHERE active = <flag>`.

- Each line's content is everything after its `\\` marker to end-of-line.
- Lines are joined with `\n`. There is **no trailing newline** after the last
  line. To get one, add a final empty `\\` line:
  ```tw
  \\last
  \\
  ```
  → `"last\n"`.
- A **blank line inside** the block is an empty `\\` line. A bare blank line (no
  `\\`) *ends* the block.
- The block ends at the first line whose first non-whitespace characters are not
  `\\`. There is no closing delimiter.
- **Marker indentation is excluded.** The whitespace before each `\\` is consumed
  as ordinary inter-token whitespace and never enters the value, so the block can
  be indented to match surrounding code:
  ```tw
  fn render(user: User) String {
    \\<div>
    \\  <h1>${user.name}</h1>
    \\</div>
  }
  ```
  Value: `<div>\n  <h1><user.name></h1>\n</div>` — the two spaces before `<h1>`
  are in the value (after the `\\`); the indentation of the `\\` markers is not.
- `\` inside the content is literal (raw); there is no escape processing.
- `${…}` interpolation fires, but an interpolation must **open and close within a
  single `\\` line** (it cannot span a line break).
- `\r\n` line endings are normalized to `\n` so a repo checked out on Windows
  produces byte-identical values.
- No `//` comments inside a block — a comment line does not start with `\\`, so it
  ends the block.

## Formatter & lint rules

The governing invariant: **the formatter is value-preserving for every string
literal.** For a `\\` block, the only things fmt may touch are the *marker
indentation* (semantically irrelevant — it is excluded from the value) and the
block's placement relative to the tokens before it. fmt must never alter anything
after a `\\` marker.

**How fmt recovers the surface form.** The AST keeps only the *decoded* value
(`StringLit(String)` / `StringInterp(...)`), so by itself it cannot tell raw from
cooked, or a `\\` block from a cooked string with `\n`. Rather than thread a form
tag through the AST (and its ~30 match sites), the printer recovers the form the
same way it already preserves int/float spelling: `build_trivia_map` indexes the
string tokens by `span.start` and reads `source[span.start]` (`r` → raw, `\` →
multiline, else cooked) into a `string_form` map, which the `StringLit`/
`StringInterp` emit sites consult. This keeps the parser/AST/checker/codegen
genuinely unchanged; only `formatter.format` gains a `source` parameter. Done for
`r"…"`; the `\\` arm slots into the same map in Milestone 4.

### Formatter rules (`target/twk fmt` — automatic, value-preserving)

- **F1 — block on its own lines.** A `\\` block is laid out as its own run of
  lines and never shares a line with the tokens that introduce it. The introducer
  — a binding `x :=`, a **rebinding `x =`**, `return`, a record `field:`, or a call
  argument — ends its line, and the block begins on the next line:
  ```tw
  // before
  multi_line := \\starts
    \\here
  // after
  multi_line :=
    \\starts
    \\here
  ```
  The identical shape applies to rebinding with `=`:
  ```tw
  multi_line =
    \\updated
    \\multi
    \\line
    \\string
  ```
  Canonical shape in every case: the introducer line, then the indented block.
- **F2 — canonical marker indentation.** All `\\` markers in one block align at a
  single indentation step deeper than the enclosing statement. Because marker
  indentation is excluded from the value, this reflow is always safe.
- **F3 — preserve content exactly.** fmt passes that would mutate string content
  are disabled inside `\\` content: do **not** trim trailing whitespace on content
  lines (trailing spaces after the marker are part of the value), do **not**
  re-indent or rewrap text after the marker, and keep empty `\\` lines verbatim
  (they encode blank lines and the trailing-newline convention).
- **F4 — never convert string forms.** fmt does not rewrite `\\` ↔ `"…"` ↔
  `r"…"`. Form choice is the author's; changing it is a lint *suggestion* (below),
  never an automatic edit.

### Lint rules (diagnostics — suggestions, not auto-applied)

- **L1 — single-line `\\` block.** A block of exactly one marker line (its value
  has no newline) should be a single-line string. Emit a suggestion-level
  diagnostic on the block, with a value-preserving suggested fix:
  - content has no `\` and no `"` → `"content"`,
  - content has `\` but no `"` → `r"content"`,
  - content contains `"` → cooked `"…\"…"`.

  A one-marker block followed by an empty `\\` line is **not** single-line (it has
  a trailing newline), so it does not fire.
- **L2 — escape-heavy cooked string (optional / future).** A `"…"` carrying
  several `\\` escapes that a raw string would simplify → suggest `r"…"`. Off by
  default to avoid noise; listed here so the rule space is explicit, not
  necessarily shipped in v1.

These mirror how the codebase already models style guidance — a `DiagKind` variant
carrying a suggestion, like the parser's `CStyleLogicalOp` rejection of `&&`/`||`.

## Token-model mapping (why downstream is unchanged)

The lexer already produces, in `boot/compiler/tokens.tw`:

- `StringLit` — a complete string with no interpolation.
- `StringStart` / `StringContinue` / `StringEnd` — bracketing the expression
  tokens of an interpolated string, with `interp_depths` tracking brace nesting.

Both new forms map onto exactly these tokens:

- `r"…"` with no `${}` → `StringLit`; with `${}` → `StringStart … StringEnd`.
- `\\` block with no `${}` → a single `StringLit` whose text is the newline-joined
  content; with `${}` → `StringStart … StringEnd` whose segments span lines.

So the parser, AST, and everything downstream need **no changes**. The work is in
the two lexers and the grammar.

## File layout & touch points

The change touches two executable lexers (which must round-trip in lockstep) plus
the descriptive grammar / highlighting / spec surfaces (kept in sync so the docs
and editor tooling match the lexers):

1. `boot/compiler/lexer.tw` — the primary lexer (string scanning lives in
   `scan_string_segment`, `unescape_string`, and the `c == '\"'` branch around
   line 503; interpolation resumes around lines 317–364 via the `interp_depths`
   stack).
2. `src/syntax/lexer.rs` — the stage0 Rust lexer; the bootstrap reference. It
   must accept identical tokens or `boot/main.tw` will not round-trip.
3. `docs/grammar.ebnf` — the canonical EBNF. Extend the `StringLiteral` rule
   (line 539) with raw-string and multiline productions so the spec grammar stays
   authoritative.
4. `tree-sitter-twinkle/grammar.js` — editor/parser grammar. After editing,
   regenerate `src/parser.c` and rebuild `tree-sitter-twinkle.wasm`, and commit
   `grammar.js`, the regenerated `src/`, and the wasm together.
5. `tree-sitter-twinkle/queries/highlights.scm` — syntax-highlight queries; add
   captures for the new raw/multiline string nodes (the current strings block at
   lines 74–77 maps `string_literal` / `string_content` / `escape_sequence`).

**Per CLAUDE.md, never run `tree-sitter test` from the agent — hand that step to
the human.** Plus tests (`boot/tests/…`, Rust lexer tests) and prose docs
(`docs/spec.md`).

## Milestones

Three milestones, each independently shippable. Milestone 1 (`r"…"`) delivers the
regex win on its own; milestone 3 (`\\` interpolation) is the highest-risk piece
and can land as a fast-follow without blocking the rest.

---

### Milestone 1 — `r"…"` raw single-line

**Task 1.1 — boot lexer** ✅ done

- [x] In `boot/compiler/lexer.tw`, recognize the raw prefix: when the current
      char is `r` and `source[i+1] == '"'`, consume both and scan a raw segment.
- [x] Add a `scan_raw_string_segment(source, from, content_start)` parallel to
      `scan_string_segment` that is identical except it (a) does **not** set
      `escaped` on `\` (so `\` is ordinary and `\"` does not escape the
      terminator) and (b) returns `text` **without** calling `unescape_string`.
      It still terminates on `"`, still returns `found_interp` on `${`, and still
      breaks on `\n` (→ unterminated).
- [x] Emit the same `StringLit` / `StringStart` tokens as the cooked path. The
      `interp_depths` stack became `Vector<InterpFrame>` (`.{ depth, host }`,
      `StringKind = { Cooked, Raw }`); resume after `}` dispatches via
      `scan_host_segment` so a raw string's continuation stays raw.

Sketch:
```tw
// raw: no `\` handling; `"` always terminates, `${` always interpolates.
fn scan_raw_string_segment(source: String, from: Int, content_start: Int) ScanStringResult {
  i := from
  n := source.len()
  for i < n {
    ch := source[i]
    if ch == '\"' {
      return ScanStringResult.{ next_index: i + 1, text: source.slice(content_start, i), terminated: true, found_interp: false }
    }
    if ch == '$' and i + 1 < n and source[i + 1] == '{' {
      return ScanStringResult.{ next_index: i + 2, text: source.slice(content_start, i), terminated: false, found_interp: true }
    }
    if ch == '\n' { break }
    i = i + 1
  }
  ScanStringResult.{ next_index: i, text: source.slice(content_start, i), terminated: false, found_interp: false }
}
```
Note: when interpolation in a raw string resumes after `}`, the continuation
segment is *also* raw, so the resume path must remember that the host string was
raw and pick `scan_raw_string_segment` rather than the cooked scanner. This is the
same "which scanner resumes after `}`" problem that Milestone 3 solves in full for
`\\` blocks. To keep Milestone 1 self-contained and correct for `r"a ${x} b"`, M1
introduces the **minimal** host-kind distinction needed here — a single bit (or
two-value `StringKind = { Cooked, Raw }`) threaded onto the `interp_depths`
entries — and M3 widens it to the three-way `{ Cooked, Raw, Multiline }` stack.
The host-kind stack is therefore *started* in M1, not deferred wholesale to M3;
M3 only adds the `Multiline` arm and its `scan_multiline_segment` resume.

**Task 1.2 — stage0 Rust lexer** ✅ done

- [x] Mirror the same `r"…"` recognition and raw scan in `src/syntax/lexer.rs`
      (`StringKind` enum, `interpolation_stack: Vec<(u32, StringKind)>`,
      `lex_raw_string` / `scan_raw_segment`), emitting identical tokens.
      Self-host fixed point (`make stage2`) reached.

**Task 1.3 — tests** ✅ done

- [x] Boot lexer/parse coverage (`boot/tests/suites/string_literal_suite.tw`):
      `r"\d+"` → value `\d+`; raw interpolation keeps both segments raw; `r"\n"`
      is backslash-n; a literal newline errors; `r` disambiguation. End-to-end
      `regexp.must(r"(\d+)-(\d+)").find(...)` in the regexp suite. Plus an
      fmt-preservation test (`fmt_suite`).
- [x] Rust lexer tests for the same (`src/syntax/lexer.rs` tests).

**Task 1.4 — grammar & highlighting** — code done; wasm rebuild + tests pending (human)

- [x] Extend `docs/grammar.ebnf` with a `RawStringLiteral` production alongside
      `StringLiteral`, with no escape alternatives (only "any char except `"`/
      newline" and the `${` Expr `}` interpolation alternative).
- [x] Add a `raw_string_literal` rule (+ `raw_string_content`) to
      `tree-sitter-twinkle/grammar.js`, registered in `_literal` and
      `literal_pattern`; ran `npx tree-sitter generate` (clean, no conflicts).
      `tree-sitter parse` confirms `r"\d+"` / `r"a ${z} b"` produce
      `raw_string_literal` nodes with no `escape_sequence` and no ERROR nodes;
      `red` stays an identifier. **Still TODO (human): `npx tree-sitter build
      --wasm` (Docker) to rebuild `tree-sitter-twinkle.wasm`, and `tree-sitter
      test`.**
- [x] In `tree-sitter-twinkle/queries/highlights.scm`, capture
      `raw_string_literal` / `raw_string_content` as `@string` with **no**
      `@string.escape` (since `\` is literal in raw strings).

**Task 1.5 — docs** ✅ done

- [x] Document `r"…"` in `docs/spec.md`'s string-literal section (raw, no escapes,
      interpolation on, cannot contain `"`).

---

### Milestone 2 — `\\` multiline (no interpolation)

**Task 2.1 — boot lexer** ✅ done

- [x] In the main loop's token-start handling, recognize `\\` (`c == '\\' and
      source[i+1] == '\\'`) as the start of a multiline string. Indentation before
      the `\\` is already consumed as whitespace, so the value excludes it for
      free.
- [x] Add `scan_multiline_string(source, from)` that consumes consecutive `\\`
      lines, joins their content with `\n`, normalizes `\r\n` → `\n`, and stops at
      the first non-`\\` line. Emit one `StringLit`.

Sketch (no-interpolation core):
```tw
type MultilineResult = .{ text: String, next_index: Int }

fn scan_multiline_string(source: String, from: Int) MultilineResult {
  i := from
  n := source.len()
  parts: Vector<String> = []
  for true {
    i = i + 2                                   // consume the `\\` marker
    line_start := i
    for i < n and source[i] != '\n' { i = i + 1 }
    end := if i > line_start and source[i - 1] == '\r' { i - 1 } else { i }  // CRLF → LF
    parts = .append(source.slice(line_start, end))
    // peek next line: newline, then horizontal whitespace, then `\\`?
    j := if i < n and source[i] == '\n' { i + 1 } else { i }
    k := j
    for k < n and (source[k] == ' ' or source[k] == '\t') { k = k + 1 }
    if k + 1 < n and source[k] == '\\' and source[k + 1] == '\\' {
      i = k
    } else {
      break   // leave i at the trailing newline (if any) for the main loop
    }
  }
  MultilineResult.{ text: join_newline(parts), next_index: i }
}
```
(`join_newline` joins with `\n`; reuse an existing join helper or inline it.)

**Task 2.2 — stage0 Rust lexer** ✅ done

- [x] Mirror `\\`-block scanning in `src/syntax/lexer.rs` (`lex_multiline_string`,
      peek-ahead line grouping). Self-host fixed point reached.

**Task 2.3 — tests** ✅ done

- [x] Boot (`string_literal_suite.tw`) and Rust (`lexer.rs`) coverage: two-line
      block → `"a\nb"`; trailing empty `\\` line adds the newline; marker
      indentation excluded while content indentation preserved; non-`\\` line ends
      the block; `\r\n` normalizes; single `\\foo` → `foo`. Plus a parser-level
      check that a block parses as a binding value on following lines.

**Task 2.4 — grammar & highlighting** — code done; wasm rebuild + tests pending (human)

- [x] Add a `MultilineStringLiteral` production to `docs/grammar.ebnf`.
- [x] Add `multiline_string` / `multiline_line` rules to
      `tree-sitter-twinkle/grammar.js` (registered in `_literal` and
      `literal_pattern`); `tree-sitter generate` clean, `tree-sitter parse`
      confirms a block becomes a `multiline_string` of `multiline_line`s with the
      following statement separate and no ERROR nodes. **TODO (human): `npx
      tree-sitter build --wasm` + `tree-sitter test`.**
- [x] Capture `multiline_string` / `multiline_line` in
      `tree-sitter-twinkle/queries/highlights.scm` as `@string` (no
      `@string.escape`).

**Task 2.5 — docs** ✅ done

- [x] Document `\\` multiline strings in `docs/spec.md` (line prefix, newline
      joining, no trailing newline, blank-line rule, indentation exclusion, CRLF
      normalization, no inline escapes).

---

### Milestone 3 — `\\` multiline interpolation

The hard part: an interpolation in a `\\` block must resume into the *multiline*
scanner, not the `"…"` scanner. Today the `interp_depths` stack records only
brace depth; it must also record which **host string kind** each interpolation
belongs to (cooked `"…"`, raw `r"…"`, or `\\` block) so the resume after `}`
picks the right segment scanner.

**Task 3.1 — boot lexer** ✅ done

- [x] Widened the host-kind stack to three-way
      `StringKind = { Cooked, Raw, Multiline }`.
- [x] `scan_multiline_string` became `scan_multiline_segment(source, from)`: scans
      from a content cursor (past a marker, or past a `}` on resume), accumulates
      across `\\` lines, and stops at the first `${` (StringStart/Continue) or the
      first non-`\\` line (StringLit/StringEnd). `scan_host_segment` dispatches
      `.Multiline` to it.
- [x] Enforced same-line interpolation: a newline while the top frame is
      `.Multiline` emits a diagnostic and drops the frame.

**Task 3.2 — stage0 Rust lexer** ✅ done

- [x] Mirrored: `StringKind::Multiline`, `scan_multiline_segment`, the resume arm,
      a quote-consumption guard (multiline has no closing delimiter), and a
      `MultilineInterpAcrossLines` error on a newline inside a multiline interp.

**Task 3.3 — tests** ✅ done

- [x] Boot + Rust: single `${…}`, accumulation across lines, multiple `${…}` (→
      `StringContinue`/`StringEnd`), lone `$` stays literal, line-break-in-interp
      errors.

**Task 3.4 — grammar & highlighting** — code done; wasm rebuild + tests pending (human)

- [x] `docs/grammar.ebnf` multiline production already admits `${ Expr }`.
- [x] `multiline_string`/`multiline_line` in `tree-sitter-twinkle/grammar.js`
      restructured so each line is `\\` + `repeat(multiline_content | interpolation
      | '$')`; `tree-sitter generate` clean; `tree-sitter parse` shows
      `multiline_content` + `interpolation` nodes, no ERRORs. **TODO (human): wasm
      rebuild + `tree-sitter test`.**
- [x] `highlights.scm` captures `multiline_content` as `@string`; interpolations
      use the shared `interpolation` rule.

---

### Milestone 4 — formatter & lint

Depends on Milestone 2 (the `\\` node must lex/parse first). The formatter and
lint rules are defined in "Formatter & lint rules" above.

**Task 4a.1 — formatter layout (F1–F3)** ✅ done (tail introducers)

- [x] `printer.tw` lays out a `\\` block as its own indented run of lines (F1),
      markers one indent step (2 spaces) past the enclosing statement (F2):
      `format_assigned` drops the separator's trailing space and emits
      `indent(hard_line + multiline_block_doc(value))`. Content after each marker
      is emitted verbatim (F3) — the layout renderer keeps literal text and never
      trims emitted trailing spaces; empty `\\` lines and the trailing-newline
      convention survive.
- [x] Idempotence confirmed by tests.
- **Scope note / safety:** a `\\` block eats the rest of its line and the next,
      so it is only emitted in **tail positions** — binding `:=`, typed binding
      `: ty =`, rebinding `=`, and `return`. A multiline value in a non-tail
      context (call arg, record field, array element) would have a following
      delimiter swallowed, so there it originally fell back to the cooked escaped
      form (value-preserving). **Update:** F1 now also covers those non-tail
      contexts — record fields, call args, array elements, and variant args
      preserve `\\` form, placing the terminating comma on its own line
      (`printer.tw` `format_items_with_multiline`). Fixtures:
      `multiline_record_field`, `multiline_call_arg`, `multiline_array_element`,
      `multiline_variant_arg`.

**Task 4a.2 — formatter test cases** ✅ done

- [x] Covered in `fmt_suite.tw`: canonical block round-trips; same-line
      introducer-then-`\\` reflows to the next line for binding `:=` and rebinding
      `=`; trailing spaces inside content survive (F3); a trailing-newline block
      (final empty `\\` line) survives; idempotence.

**Task 4a.3 — single-line `\\` lint (L1)** ✅ done

- [x] Added a `WarningDiag` variant `MultilineSingleLine(.{ span, suggestion })`,
      wired through `span`/`message`/`diag_render`/`query/diagnostics`.
- [x] The lexer raises it (suggestion-level) when a `\\` block produces a single
      StringLit whose value has no newline, with `single_line_suggestion`
      computing the value-preserving form (`"…"`, `r"…"`, or escaped cooked).

**Task 4a.4 — lint tests** ✅ done

- [x] Boot tests: each of the three content cases gets the right suggested form;
      a one-line block with a trailing empty `\\` line does **not** warn; a
      genuinely multi-line block does not warn.

### Wrap-up

**Task 4.1 — adopt `r"…"` in regexp docs/examples** ✅ done

- [x] Updated `docs/API.md`'s `@std.regexp` examples to use `r"(\d+) …"` /
      `r"mul\((\d+),(\d+)\)"`, noting both spellings compile identically.

**Task 4.2 — retire the percent-escapes plan** ✅ done

- [x] `docs/plans/archive/regexp-percent-escapes.md` is marked SUPERSEDED by this
      plan.

**Task 4.3 — full check** ✅ done

- [x] Self-host fixed point (`make stage2`), `target/twk run boot/tests/main.tw`,
      and the full `make test` all green. Tree-sitter `build --wasm` + tests run by
      the human.

## Testing summary

- **boot:** lexer/parse round-trip for every form and edge (raw escapes, can't
  contain `"`, multiline joining/trailing-newline/blank-line/indentation/CRLF,
  interpolation per line), plus an end-to-end `@std.regexp` case using `r"…"`.
- **stage0:** the Rust lexer test suite must accept identical tokens (`cargo test`
  lexer tests), and `boot/main.tw` must still self-host (the changed lexer is
  itself compiled by stage0).
- **formatter & lint:** `fmt_cases` fixtures for the layout rules (F1–F3) plus
  idempotence, and lint tests for the single-line `\\` suggestion (L1) including
  the trailing-newline non-firing case.
- **tree-sitter:** grammar tests run by the human.

## Risks & notes

- **Milestone 3 is the entanglement risk.** If interpolation-in-`\\` proves
  costly, Milestones 1–2 ship a coherent feature on their own (raw single-line +
  raw multiline without interpolation); the universal-interp rule is then
  completed by Milestone 3 as a fast-follow.
- **`r` prefix disambiguation** is safe (identifier-then-`"` is never valid), but
  the test suite should pin `range`/`red`/a variable `r` to guard against a greedy
  prefix rule.
- **Self-host hazard:** the lexer change is compiled by the *current* stage0, then
  recompiles itself. Land the stage0 Rust change and the boot change together and
  verify the self-host fixed point (`make bundle-cli`) before trusting either.
- **Future (out of scope):** a `String.dedent()` helper or closing-column dedent;
  a `b"…"` byte-string prefix; allowing `"` inside raw single-line via a hashed
  form (`r#"…"#`). None of these change the architecture here.
