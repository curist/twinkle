# Tooling

Design for formatter, linter, and LSP — all implemented as subcommands of the
`twk` binary initially, then rewritten in Twinkle as part of self-hosting.

---

## Prerequisites

Before any tool can be built properly, two parser-level capabilities are needed.

### Lossless Lexer (comment trivia)

Currently `skip_whitespace_and_comments` in `lexer.rs` discards comments entirely.
A formatter that strips all comments is unacceptable; an LSP that can't show doc
comments is incomplete.

Comments must become trivia tokens attached to adjacent AST nodes. Each token
carries a `Vec<Trivia>` prefix of comments/whitespace that appeared before it:

```rust
enum Trivia {
    LineComment(String),    // // text
    DocComment(String),     // /// text (future)
    Whitespace(String),
}

struct Token {
    kind: TokenKind,
    span: Span,
    leading_trivia: Vec<Trivia>,
    preceded_by_newline: bool,    // parser uses this for dot-postfix rule
}
```

`preceded_by_newline` can be derived from trivia at parse time but must remain
accessible. This change is isolated to `lexer.rs` and `tokens.rs`.

### Parser Error Recovery

The current parser fails hard on the first syntax error. Tools that work on
in-progress code need a partial AST.

The parser should recover at statement boundaries: on an unexpected token, emit
an error node and skip to the next newline or `}`, then continue parsing.
Return `(SourceFile, Vec<ParseError>)`. A minimal version (recover at top-level
item boundaries) is enough for an initial LSP.

---

## Formatter (`twk fmt`)

Format `.tw` source files to a canonical style. Single subcommand, no config —
one official style (like `gofmt` / `gleam format`).

```bash
twk fmt file.tw          # format in place
twk fmt --check file.tw  # exit 1 if not formatted (CI)
twk fmt -                # stdin → stdout
```

### Architecture

The formatter only needs the parse stage:

```
source → lex (with trivia) → parse → pretty-print → formatted source
```

No type checking needed.

### Style rules

- **Width**: 100 columns
- **Indentation**: 2 spaces
- **Comments**: preserve exactly, re-attach to the same AST node position
- **Trailing commas**: always in multi-line contexts, never in single-line
- **Blank lines**: preserve single blank lines between top-level items; collapse multiples

Tests should verify idempotency: `format(format(src)) == format(src)`.

---

## Linter (`twk lint`)

Report code quality issues in two tiers:

- **Syntactic rules** — only need the parse stage
- **Semantic rules** — need parse + resolve + typecheck

```bash
twk lint file.tw
twk lint --only=unused-vars file.tw
```

### Planned rules

**Syntactic** (parse only):
- Unreachable code after `return`, `break`, `continue`

**Semantic** (requires typecheck):
- Unused variable / function argument
- Rebinding without use — if `x = expr` or `x.field = expr` is the last use
  with no return or pass, warn that the update has no effect
- Dead pattern arm
- Function return type can be simplified to `Void`

### Architecture

Because each pipeline stage is independently invokable (see
[query-pipeline.md](query-pipeline.md)):

```rust
let ast = parse(&source)?;
run_syntactic_rules(&ast, &mut diagnostics);

let resolved = resolve(&ast, &deps)?;
let typed = typecheck(&ast, &resolved)?;
run_semantic_rules(&ast, &resolved, &typed, &mut diagnostics);
```

No lowering or linking needed.

---

## LSP Server (`twk lsp`)

Language server implementing LSP for IDE integration.

```bash
twk lsp      # start, communicate via stdin/stdout
```

### Initial feature set (priority order)

1. Diagnostics (type errors, lint warnings) on file save
2. Hover (show type of expression under cursor)
3. Go-to-definition
4. Completion (field names, function names in scope)

### Architecture requirements

1. **Query-friendly pipeline** — re-run only affected stages on change
2. **Lossless lexer** — hover over comments, doc comment display
3. **Parser error recovery** — diagnostics even when code is syntactically broken

### Implementation path

1. `twk lsp` stub that speaks LSP JSON-RPC
2. On `didOpen`/`didChange`: parse + typecheck, send diagnostics
3. On hover: find ExprId at cursor (via spans), look up in TypeMap
4. Go-to-definition: extend `ResolvedModule` with declaration spans
5. Completion: suggest names from current scope's `ValueEnv` / `TypeEnv`

### Position encoding boundary

Twinkle strings remain UTF-8 and the general string API stays byte-oriented.
LSP is the exception because the protocol uses UTF-16 positions by default
(with optional encoding negotiation in newer protocol versions). The boot
compiler should keep this conversion logic in tooling-private helpers such as
`boot/tooling/lsp/position.tw`, instead of adding UTF-16 methods to `String`
or folding protocol-specific behavior into `boot/lib/source`.

### Hosting

Initially: Rust host using `tower-lsp` or similar. After self-hosting: rewrite
in Twinkle, distribute as `.wasm` with a tiny Node.js/Deno wrapper for JSON-RPC
transport.

---

## Dependency Map

```
Formatter    requires: lossless lexer
Linter       requires: query-friendly pipeline
LSP          requires: query-friendly pipeline + lossless lexer + error recovery
```

Implementation order:
1. Query-friendly pipeline refactor
2. Lossless lexer
3. `twk fmt`
4. `twk lint` (syntactic rules)
5. Parser error recovery
6. `twk lint` (semantic rules)
7. `twk lsp` (diagnostics + hover)
