# Boolean negation `not` transition

Status: **Planned**

## Goal

Make `not` the canonical boolean negation operator so Twinkle's boolean syntax is
internally consistent:

```tw
not ready
ready and enabled
ready or forced
```

Today Twinkle uses word operators for conjunction/disjunction, but a symbolic
operator for negation:

```tw
!ready
ready and enabled
ready or forced
```

The transition should avoid a flag day for users and, importantly, avoid a
bootstrap hazard for the self-hosted compiler. The existing semantic operation is
already named `Not` in the AST/Core IR, so this is primarily a syntax,
formatting, docs, and tooling migration.

## Non-goals

- Do not change `!=` equality syntax.
- Do not change result-type shorthand syntax (`T!E`, `T?!E`, or leading `!E`).
- Do not change the IR operation names or boolean semantics.
- Do not introduce symbolic aliases for `and`/`or` as canonical syntax.

## Phase 1: accept `not`, keep `!`, format to `not`

Phase 1 introduces `not` as the preferred spelling while keeping prefix `!`
accepted for compatibility. The formatter becomes the migration tool: any parsed
boolean negation is printed as `not ...`, while unrelated `!` syntax is preserved
by construction because it is represented by different AST nodes/tokens.

### Frontend syntax

Boot compiler:

- Add `Not` to `boot/compiler/tokens.tw`.
- Teach `boot/compiler/lexer.tw` to classify `not` as `TokenKind.Not`.
- Teach `boot/compiler/parser.tw` to parse prefix `not` as `UnOp.Not` with the
  same binding power as the current prefix `!`.
- Keep prefix `!` parsing as `UnOp.Not` during this phase.

Rust stage0:

- Mirror the token, lexer, and parser support in `src/syntax` before the boot
  sources start using `not`. This keeps the bootstrap path able to build the boot
  compiler after formatting.

### Formatting and source migration

- Change `boot/compiler/fmt/printer.tw` so `UnOp.Not` prints as `not ` instead
  of `!`.
- Run the Twinkle formatter on `.tw` sources after the compiler used for
  formatting accepts `not`.
- Avoid regex migration: `!` is also used by `!=` and result-type shorthand, so
  the AST-aware formatter is the safe migration mechanism.

### Diagnostics and compatibility

- Initially, prefix `!` can remain silent compatibility syntax.
- Optionally add a warning-style diagnostic later in Phase 1 if the diagnostic
  pipeline has a suitable non-fatal category. If diagnostics are error-only, do
  not force this into Phase 1.
- If a diagnostic is added, word it as a migration hint: "use `not` for boolean
  negation".

### Tooling and docs

- Add `not` to keyword completions in `boot/compiler/query/completion.tw`.
- Update tree-sitter grammar so unary expressions accept `not` as well as `!`.
  Regenerate the tracked parser artifacts and wasm together. Ask a human to run
  tree-sitter tests manually.
- Update docs and examples to show `not` as canonical boolean negation, while
  mentioning that `!` is temporarily accepted as compatibility syntax.

### Validation

- Confirm both `not x` and `!x` parse and type-check as boolean negation.
- Confirm formatter rewrites expression negation to `not x`.
- Confirm formatter preserves `!=`, `T!E`, `T?!E`, and leading result shorthand
  `!E`.
- Confirm stage0 can still build the boot compiler and the self-hosted compiler
  can run the formatted sources.

## Phase 2: remove prefix `!` as boolean negation

Phase 2 happens after the ecosystem and repository sources have had time to move
to formatter-produced `not` syntax.

### Parser policy

- Remove prefix `!` from expression parsing, or keep a targeted parser error
  that recognizes prefix `!` and reports the replacement.
- Preserve all non-negation uses of `!`:
  - `!=` remains the not-equal operator.
  - `T!E`, `T?!E`, and `!E` remain result-type shorthand unless a separate
    design changes them.

Suggested diagnostic:

```text
error: `!` is not valid boolean negation syntax
help: write `not condition`
```

The diagnostic should only apply in expression-prefix position. Type positions
must continue to route through the result-shorthand parser.

### Cleanup

- Remove prefix `!` from tree-sitter expression grammar while keeping result-type
  shorthand intact.
- Remove compatibility tests that assert `!x` is accepted, replacing them with a
  parser diagnostic test.
- Update docs to stop presenting `!` as accepted boolean negation.
- Keep formatter output unchanged: `UnOp.Not` continues to print as `not ...`.

### Validation

- Confirm `not x` remains accepted everywhere expression negation is valid.
- Confirm prefix `!x` produces the intended diagnostic.
- Confirm `!=` and result-type shorthand still parse normally.
- Run the normal Rust and boot compiler test workflows; ask a human to run
  tree-sitter tests if grammar artifacts changed.

## Ordering notes

The safe order is:

1. Add `not` support to Rust stage0.
2. Add `not` support to the boot compiler while keeping `!` accepted.
3. Rebuild the compiler used by the formatter.
4. Change formatter output to canonical `not`.
5. Format Twinkle sources and update docs/tooling.
6. After a transition window, make prefix `!` a targeted error in expression
   position.

This avoids landing formatted `not` syntax before the bootstrap compiler can
parse it, and it lets existing user code continue compiling during the migration.
