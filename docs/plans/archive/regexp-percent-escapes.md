# `@std.regexp` percent escapes

**Status:** SUPERSEDED by `docs/plans/archive/string-literals.md`.

> Raw string literals (`r"…"`) solve the underlying problem — backslash-heavy
> regex patterns needing doubled escapes in Twinkle strings — at the right layer,
> without forking the regex dialect. With `r"…"`, `regexp.must(r"\d+")` needs no
> `%`-aliases and stays portable standard regex syntax. This `%`-escape proposal
> is therefore retired in favor of `docs/plans/archive/string-literals.md`; it is kept for
> historical context only and should not be implemented.

## Goal

Improve ergonomics for regex patterns written inside Twinkle string literals by
adding `%`-based aliases for the common regexp escapes. Today users write
standard regex escapes through Twinkle strings, so `\d+` in regex syntax becomes
`"\\d+"` in source. Percent escapes let users write `"%d+"` instead, while
keeping the standard backslash forms fully supported.

This is a library-only extension to `@std.regexp`: no compiler changes, no new
string literal syntax, and no change to the VM/program representation.

## Non-goals

- Do not remove or deprecate standard backslash escapes (`\d`, `\w`, `\s`, etc.).
- Do not add raw strings to Twinkle in this task.
- Do not add new regex features beyond escape aliases.
- Do not make `%` an inline flag or mode prefix beyond the aliases listed here.

## Design

`%` behaves as an alternate escape introducer in regexp patterns. It is accepted
where `\` escapes are accepted: in atoms and inside character classes.

Supported aliases:

| Percent escape | Meaning | Backslash equivalent |
|---|---|---|
| `%d` | digit class `[0-9]` | `\d` |
| `%D` | non-digit class | `\D` |
| `%w` | word class `[0-9A-Za-z_]` | `\w` |
| `%W` | non-word class | `\W` |
| `%s` | whitespace class | `\s` |
| `%S` | non-whitespace class | `\S` |
| `%n` | newline scalar | `\n` |
| `%t` | tab scalar | `\t` |
| `%r` | carriage return scalar | `\r` |
| `%f` | form feed scalar | `\f` |
| `%v` | vertical tab scalar | `\v` |
| `%%` | literal `%` | `\%` |

For any other `%X`, parse it as a literal `X`, matching the current forgiving
backslash behavior for escaped metacharacters. This means `%(`, `%[`, `%*`, `%+`,
`%.`, `%^`, `%$`, `%|`, `%]`, `%}` can be used to quote regex metacharacters, and
`%a` is simply literal `a`.

A trailing `%` is a parse error at the `%` position, analogous to trailing `\`.
Use `%%` or `\%` to match a literal percent sign.

## Semantics and examples

These pairs are equivalent:

```tw
regexp.must("%d+")
regexp.must("\\d+")

regexp.must("(%d+) (%w+)")
regexp.must("(\\d+) (\\w+)")

regexp.must("[%d-]+")
regexp.must("[\\d-]+")
```

Literal percent signs:

```tw
regexp.must("100%%").test("100%")
regexp.must("100\\%").test("100%")
```

Escaped metacharacters without doubled Twinkle backslashes:

```tw
regexp.must("mul%(%d+,%d+%)").test("mul(2,4)")
```

## Parser changes

Only `boot/stdlib/regexp/parse.tw` should need changes.

Refactor escape parsing so both `\` and `%` go through the same helpers:

- Atom context:
  - current `ch == 92` branch becomes `is_escape_intro(ch)`.
  - if the introducer is at end of pattern, error with:
    - `"trailing backslash"` for `\`
    - `"trailing percent escape"` for `%`
  - otherwise inspect the next scalar:
    - `class_escape(next)` still handles `d/D/w/W/s/S`.
    - `decode_escape(next)` handles control escapes; extend or reuse it so `%n`
      and `\n` both decode to newline.
    - all other escaped scalars become literals.

- Class context:
  - current class-element `ch == 92` branch also accepts `%`.
  - `%d`, `%w`, `%s` expand to class ranges, just like `\d`, `\w`, `\s`.
  - `%D`, `%W`, `%S` inside `[]` should follow current `\D`, `\W`, `\S`
    behavior: reject negated class escapes inside character classes.
  - `%%` becomes a literal `%` item.
  - trailing `%` in a class errors at the `%` position.

Suggested helpers:

```tw
fn is_escape_intro(ch: Int) Bool {
  ch == 92 or ch == 37
}

fn trailing_escape_error(pos: Int, intro: Int) RegexError {
  if intro == 92 {
    err(pos, "trailing backslash")
  } else {
    err(pos, "trailing percent escape")
  }
}
```

The AST, program compiler, and VM do not need changes because percent escapes are
pure parser sugar.

## Tests

Add cases to the dev harness first (`/tmp/rxdev/tests.tw`), then port important
coverage into `boot/tests/suites/stdlib_regexp_suite.tw`.

Required cases:

```tw
// atom escapes
regexp.must("%d+").find("a123b").unwrap().text() == "123"
regexp.must("%w+").find("!!ab_12").unwrap().text() == "ab_12"
regexp.must("a%sb").test("a b")
regexp.must("a%nb").test("a\nb")

// metachar quoting
regexp.must("mul%(%d+,%d+%)").test("mul(2,4)")
regexp.must("%.").test(".")

// literal percent
regexp.must("%%").test("%")
regexp.must("\\%").test("%")
regexp.must("100%%").test("100%")

// classes
regexp.must("[%d-]+").find("x12-34y").unwrap().text() == "12-34"
regexp.must("[^%d]+").find("123abc").unwrap().text() == "abc"
regexp.must("[%%]").test("%")

// errors
regexp.compile("abc%") errors at pos 3
regexp.compile("[abc%") errors at pos 4
```

Also add parity tests showing old backslash forms continue to work.

## Documentation updates

Update `docs/API.md` under `@std.regexp`:

- Mention that `%` escapes are Twinkle-friendly aliases for common regex escapes.
- Add `%d %w %s` and `%%` to the supported syntax description.
- Update the examples to prefer `%` escapes where it improves readability, while
  noting standard regex backslash escapes remain supported.

Example replacement:

```tw
re := regexp.must("(%d+) (red|green|blue)")
```

## Implementation steps

1. Add failing dev-harness tests for `%d`, `%w`, `%s`, control escapes,
   metachar quoting, literal `%`, class usage, and trailing `%` errors.
2. Refactor `parse_atom` and `parse_class_element` to accept both escape
   introducers.
3. Run `target/twk run /tmp/rxdev/tests.tw` until green.
4. Port important cases to `boot/tests/suites/stdlib_regexp_suite.tw`.
5. Update `docs/API.md`.
6. Run formatter on touched `.tw` files.
7. Run `make bundle-cli` and `target/twk run boot/tests/main.tw`.
8. Run `make test` before finalizing.

## Compatibility

This is backward-compatible except for patterns that currently intend a literal
percent followed by a recognized alias letter, such as `%d`. Such patterns must
be written as `%%d` or `\%d` after this change. Literal percent before any
non-alias character that is not otherwise meaningful will continue to behave as
that character escaped by `%`, so users should prefer `%%` for clarity whenever a
literal percent is intended.
