# Direct Control Flow in Case Arms Plan

## Goal

Allow common terminal control flow directly in `case` arms without forcing an
extra block expression.

```tw
case opt {
  .Some(value) => value,
  .None => return fallback,
}

for item in items {
  case classify(item) {
    .Skip => continue,
    .Stop => break,
    .Use(value) => consume(value),
  }
}
```

This keeps `case` arms expression-oriented for ordinary values while removing
ceremony around arms that do not complete normally.

## Motivation

The current spelling requires users to write terminal control flow as a block:

```tw
.None => { return fallback },
.Skip => { continue },
.Stop => { break },
```

That is internally consistent if every arm body must be an expression, but it
feels like the language is exposing an implementation detail. The block does
not add scope, sequencing, or clarity in these cases; it only says "this arm
exits here" more noisily.

## Proposed Surface Rule

A `case` arm body may be either:

- a normal expression,
- `return` with an optional value,
- bare `break`, or
- `continue`.

This plan does not bless `break expr` as direct arm syntax. The tree-sitter
grammar already models `break` as bare, the published grammar documents bare
`break`, and the backend currently rejects break values. Cleaning up the wider
`parse_break_stmt` mismatch can happen separately.

Arbitrary statements still require an explicit block.

```tw
case x {
  .A => expr,
  .B => return expr,
  .C => break,
  .D => continue,
  .E => {
    log("multi-step arm")
    expr
  },
}
```

Recommended companion rule: apply the same arm-body grammar to `cond` arms.
`case` is the main ergonomic pain, but sharing the rule avoids making `cond`
feel like a second special case.

## Semantics

Direct `return`, `break`, and `continue` arm bodies are terminal arm bodies:
they do not produce a normal value and therefore do not contribute to the
result type of the surrounding `case` or `cond` expression.

```tw
x := case opt {
  .Some(v) => v,
  .None => return 0,
}
```

The expression above has the type of `v`; the `.None` arm exits the enclosing
function.

This feature should provide frontend validity checks instead of relying on
backend emission failures. Today, an existing braced arm such as `{ break }`
outside a loop can reach codegen and fail with an internal label-stack error.
As part of this work, add a checker/lowering validation step so users get a
normal diagnostic when terminal control flow appears in an invalid context.

Required validity rules:

- `return` is only valid where the enclosing function return rules permit it.
- bare `return` is allowed only for `Void` returns, matching existing checker
  behavior.
- `break` and `continue` are only valid in loop/collect contexts where they are
  semantically meaningful.
- `break expr` remains unsupported for direct arm bodies.
- This does not make `return`, `break`, or `continue` general-purpose
  expressions outside arm bodies.

## Grammar Updates

Update `docs/grammar.ebnf` to introduce a shared arm body production:

```ebnf
ArmBody =
      Expr
    | ArmReturn
    | ArmBreak
    | ContinueStmt ;

ArmReturn =
    "return" [ Expr ] ;

ArmBreak =
    "break" ;

CaseArm =
    Pattern "=>" ArmBody ;

CondArm =
      Expr "=>" ArmBody
    | "_" "=>" ArmBody ;
```

Also update nearby implementation notes to state that terminal arm bodies are
parsed only in arm-body position and are treated as diverging branches. While in
this area, reconcile the statement grammar with implemented behavior for bare
`return`, or explicitly document that `ArmReturn` is the arm-position form.

## Documentation Updates

Update `docs/spec.md` so the prose and examples match the implementation:

- show direct `return`, `return expr`, bare `break`, and `continue` in arm
  bodies,
- clarify that these are accepted as arm bodies, not as arbitrary expressions,
- keep block arms for multi-statement bodies,
- mention that diverging arms do not affect branch type unification.

## Boot Compiler Parser Plan

Update `boot/compiler/parser.tw`.

Add a small helper, for example `parse_arm_body_expr(c, context)`, used by
`parse_case_expr` and, if included in scope, `parse_cond_expr`.

Suggested lowering strategy:

1. If the next token is `return`, `break`, or `continue`, parse an
   arm-position terminal form instead of calling `parse_return_stmt` or
   `parse_break_stmt` unchanged.
2. Treat `,`, `}`, `;`, and EOF as terminators for optional terminal-statement
   values in arm position. This is required for bare arms such as
   `.Stop => break,` and `.Done => return,`; the current block-position parsers
   only stop optional values at `;`, `}`, or EOF.
3. Parse `return` with an optional expression value.
4. Parse `break` as bare in arm position. If a value appears after direct
   `break`, emit a normal diagnostic instead of accepting syntax that the
   backend cannot lower.
5. Parse `continue` as bare.
6. Wrap the parsed statement in a synthetic block expression:

   ```tw
   ExprKind.BlockExpr(Block.{ stmts: [stmt], tail: .None, span: stmt_span })
   ```

7. Otherwise, parse the body with the existing expression parser.

This keeps the AST shape stable (`CaseArm.expr: Expr`, `CondArm.body: Expr`) and
lets later compiler phases reuse existing block/statement handling.

Keep the existing diagnostic for other statement keywords in expression
position. For example, `=> defer cleanup()` and `=> for ...` should still point
users toward an explicit block.

Also add frontend validation for invalid terminal-control contexts, including
braced forms that already parse today. Direct arms should not merely make it
easier to reach the current backend "Break with no label stack" failure.

## Formatter Plan

The user mentioned `boot/compiler/fmt/formatter.tw`; the actual arm formatting
logic currently lives under the formatter entrypoint in
`boot/compiler/fmt/printer.tw`.

`boot/compiler/fmt/printer.tw` already special-cases block expressions in arms
that contain exactly one terminal statement and no tail expression, but today it
prints the terminal statement inside braces. Update that existing branch so such
blocks format as the direct arm body:

```tw
.None => return fallback,
.Skip => continue,
.Stop => break,
```

Keep explicit blocks when unwrapping would move or drop comments. A conservative
rule is fine: only unwrap when the terminal statement has no leading/trailing
trivia that must remain inside the block.

Consider extracting a shared helper for `case` and `cond` arm bodies so both
constructs stay in sync.

Add formatter fixtures under `boot/tests/suites/fmt_cases/` and wire them into
`boot/tests/suites/fmt_suite.tw`. Suggested cases:

- direct terminal arm bodies are preserved,
- old braced terminal arm bodies normalize to the new direct style,
- multi-statement block arms remain blocks,
- arm-body comments keep blocks when needed,
- mirrored `cond` behavior if `cond` is included in the parser change.

## Tree-sitter Plan

Update `tree-sitter-twinkle/grammar.js`.

The tree-sitter grammar already accepts `break_statement` and
`continue_statement` in `case_arm` and `cond_arm` bodies. Add
`return_statement` to the same choices, or replace the duplicated choices with a
shared arm-body rule.

Update `tree-sitter-twinkle/queries/highlights.scm` if the grammar shape changes
or if direct arm-body returns need an explicit query. The existing
`return_statement`, `break_statement`, and `continue_statement` captures should
continue to highlight correctly if those node names are preserved.

After grammar changes, regenerate the tree-sitter artifacts:

```bash
cd tree-sitter-twinkle
npx tree-sitter generate
npx tree-sitter build --wasm
```

Do not run `tree-sitter test` from the agent; ask a human to run it manually.

## Tests

Add boot compiler tests covering:

- direct `return` and bare `return` in a `case` arm used as an expression,
- direct `continue` in a `case` arm inside a loop or collect,
- direct bare `break` in a `case` arm inside a loop,
- diagnostics for direct `break expr`, unless broader break-value support is
  intentionally implemented first,
- diagnostics for `break` / `continue` outside valid loop or collect contexts,
- normal expression arms still type-check and infer as before,
- non-terminal statements still require an explicit block,
- existing parser diagnostics in `boot/tests/suites/parser_suite.tw` for
  `=> return`, `=> break`, and `=> continue` are replaced with success tests,
  while diagnostics remain for `=> defer`, `=> for`, and other non-terminal
  statements,
- formatter output for both newly direct syntax and old braced syntax via new
  `boot/tests/suites/fmt_cases/` fixtures,
- comments around arm bodies are preserved conservatively.

If `cond` is included, mirror the parser and formatter tests for `cond` arms.

## Non-goals

- Do not allow arbitrary statements directly after `=>`.
- Do not make `return`, `break`, or `continue` general expressions everywhere.
- Do not expose a user-facing `Never` type as part of this change.
- Do not change the runtime semantics of pattern matching or branching.

## Commit Strategy

Keep the implementation and mechanical reformatting in separate commits.

Recommended split:

1. **Language/parser/docs commit** — grammar/spec updates, parser support,
   semantic tests, and tree-sitter grammar/highlight updates.
2. **Formatter commit** — formatter logic plus new
   `boot/tests/suites/fmt_cases/` fixtures.
3. **Mechanical reformat commit** — run the updated formatter over existing
   Twinkle sources to normalize old braced terminal arms, e.g.
   `=> { return ... }`, `=> { break }`, and `=> { continue }`, into the new
   direct style.

The final commit should be intentionally mechanical: no semantic edits, only the
changes produced by the updated formatter. This makes the real implementation
reviewable without a large formatting diff mixed in.

## Implementation Order

1. Update `docs/grammar.ebnf` and `docs/spec.md` to pin down the intended
   surface rule.
2. Add parser support by wrapping terminal arm statements in synthetic block
   expressions.
3. Add frontend validation for invalid `break` / `continue` contexts so these
   paths fail with diagnostics before backend emission.
4. Add type-check/run tests for the new syntax and validation failures.
5. Update formatter arm-body printing and add formatter fixtures.
6. Update tree-sitter grammar, highlights if needed, and regenerate artifacts.
7. Commit the implementation and formatter work before running the broad
   mechanical reformat.
8. Run the updated formatter over affected Twinkle sources and commit that
   output separately.
9. Run the normal Twinkle checks:

   ```bash
   target/twk fmt boot/compiler/parser.tw
   target/twk fmt boot/compiler/fmt/formatter.tw
   target/twk fmt boot/compiler/fmt/printer.tw
   target/twk lint boot/main.tw
   target/twk run boot/tests/main.tw
   ```
