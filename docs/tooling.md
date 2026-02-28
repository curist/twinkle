# Tooling: Formatter, Linter, LSP

Tools beyond the core compiler. All implemented as subcommands of the `twk` binary
initially, then rewritten in Twinkle as part of self-hosting.

---

## Prerequisites (shared by all tools)

Before any of the below can be built properly, two parser-level capabilities are needed:

### 1. Lossless Lexer (comment trivia)

Currently `skip_whitespace_and_comments` in `lexer.rs` discards comments entirely.
A formatter that strips all comments is unacceptable; an LSP that can't show doc
comments is incomplete.

**Change needed**: comments become trivia tokens attached to adjacent AST nodes.
Concretely, each token (or each AST node) carries a `Vec<Trivia>` prefix of
comments/whitespace that appeared before it. The lexer collects these instead of
discarding them.

```rust
enum Trivia {
    LineComment(String),    // // text
    DocComment(String),     // /// text (future)
    Whitespace(String),
}

struct Token {
    kind: TokenKind,
    span: Span,
    leading_trivia: Vec<Trivia>,  // comments before this token
    preceded_by_newline: bool,    // must be preserved — parser uses this for dot-postfix rule
}
```

**Important**: `Token` already has `preceded_by_newline: bool` (used by the parser's
newline-aware dot-postfix rule — `.Variant` on a new line is not consumed as postfix).
The trivia redesign must preserve or subsume this boolean so existing parser logic
continues to work. `preceded_by_newline` can be derived from trivia at parse time
(check whether any leading trivia contains a newline), but it must remain accessible.

This change is isolated to `lexer.rs` and `tokens.rs`. The parser does not need to
change structurally — it can continue ignoring most trivia for the AST, but the
formatter reads trivia from the token stream directly.

### 2. Parser Error Recovery

The current parser fails hard on the first syntax error. Tools that work on
in-progress code (half-written function, missing closing brace) need a partial AST.

**Change needed**: the parser should recover at statement boundaries:
- On an unexpected token inside a statement, emit an error node and skip tokens
  until the next newline or `}` / `;`
- Continue parsing subsequent statements
- Return `(SourceFile, Vec<ParseError>)` — partial AST plus accumulated errors

Full error recovery is complex; a minimal version (recover at top-level item
boundaries) is enough for an initial LSP. Formatter can refuse to format files
with parse errors.

---

## Formatter (`twk fmt`)

### Goal

Format `.tw` source files to a canonical style. Single subcommand, no config:
Twinkle has one official style (similar to `gofmt` / `gleam format`).

```bash
twk fmt file.tw          # format in place
twk fmt --check file.tw  # exit 1 if not formatted (for CI)
twk fmt -                # read from stdin, write to stdout
```

### Architecture

The formatter only needs the **parse stage**. It reads the token stream (with trivia)
and the AST, then pretty-prints them with canonical whitespace.

```
source → lex (with trivia) → parse → pretty-print → formatted source
```

No type checking needed. This is why the lossless lexer is the primary prerequisite —
without comment trivia, the formatter destroys information.

### Key design decisions

- **Width**: 100 columns (tentative; matches current source style)
- **Indentation**: 2 spaces (as used throughout existing `.tw` files)
- **Comments**: preserve exactly, re-attach to the same AST node position
- **Trailing commas**: always in multi-line contexts, never in single-line
- **Blank lines**: preserve single blank lines between top-level items; collapse
  multiple blank lines to one

### Implementation note

The formatter does NOT re-parse its own output to check stability — instead write
tests that format a file, then format again, and assert idempotency. This catches
bugs cheaply.

---

## Linter (`twk lint`)

### Goal

Report code quality issues. Two tiers:
- **Syntactic rules**: only need the parse stage (no type info required)
- **Semantic rules**: need parse + resolve + typecheck

```bash
twk lint file.tw
twk lint --only=unused-vars file.tw
```

### Planned rules

**Syntactic** (parse only):
- Unreachable code after `return`, `break`, `continue` (basic)

**Semantic** (requires typecheck):
- Unused variable warning (introduced but never referenced)
- Unused function argument
- **Rebinding without use** (from `gaps.md` §1): if `x = expr` or `x.field = expr`
  is the last use of `x` with no `return x` or pass to another function, warn that
  the update has no effect — this catches the "looks like mutation but isn't" trap
- Dead pattern arm (arm can never match given the scrutinee type)
- Function return type can be simplified to `Void` (always returns `Void`)

### Architecture

Because the query-friendly refactor (see [query-pipeline.md](query-pipeline.md)) makes
each stage independently invokable, a linter can:

```rust
// Syntactic rules: only parse
let ast = parse(&source)?;
run_syntactic_rules(&ast, &mut diagnostics);

// Semantic rules: parse + resolve + typecheck
let resolved = resolve(&ast, &deps)?;
let typed = typecheck(&ast, &resolved)?;
run_semantic_rules(&ast, &resolved, &typed, &mut diagnostics);
```

No lowering or linking needed. Much faster than a full compile.

---

## LSP Server (`twk lsp`)

### Goal

Language server implementing the Language Server Protocol for IDE integration.

```bash
twk lsp      # start LSP server, communicate via stdin/stdout
```

Initial feature set (in priority order):
1. Diagnostics (type errors, lint warnings) on file save
2. Hover (show type of expression under cursor)
3. Go-to-definition
4. Completion (field names, function names in scope)

### Architecture requirements

The LSP server is long-lived and needs to respond to keystrokes quickly.
This requires:

1. **Query-friendly pipeline** (see [query-pipeline.md](query-pipeline.md)) — re-run
   only the stages affected by the changed file; upstream modules with unchanged
   source skip all stages via content-hash cache
2. **Lossless lexer** — hover over a comment, doc comment display
3. **Parser error recovery** — provide diagnostics even when code is syntactically broken

The LSP does NOT need the lowering or linking stages for most features. Only if a user
wants "run in editor" integration would the full pipeline be needed.

### Implementation path

1. Add `twk lsp` stub that speaks LSP JSON-RPC
2. On `textDocument/didOpen` and `textDocument/didChange`: parse + typecheck the file,
   send diagnostics
3. On `textDocument/hover`: find the ExprId at cursor position (using spans), look up
   in TypeMap, return type string
4. Go-to-definition: resolver already tracks where names are declared; extend
   `ResolvedModule` to expose declaration spans
5. Completion: suggest names from the current scope's `ValueEnv` / `TypeEnv`

### Hosting

Initially: thin Rust host, LSP server in Rust (using the `tower-lsp` crate or similar).

After self-hosting: rewrite in Twinkle, distribute as a `.wasm` module with a tiny
host wrapper (Node.js / Deno) that handles the LSP JSON-RPC transport. This removes
the Rust dependency for IDE users.

---

## Dependency Map

```
Formatter    requires: lossless lexer
             nice-to-have: error recovery (to format partial files)

Linter       requires: query-friendly pipeline
             syntactic rules need only parse
             semantic rules need typecheck (and a parseable file)
             nice-to-have: error recovery (to lint partial files; same as formatter)

LSP          requires: query-friendly pipeline + lossless lexer + error recovery
```

Implementation order:
1. Query-friendly pipeline refactor (unlocks linter and makes LSP feasible)
2. Lossless lexer (unlocks formatter)
3. `twk fmt` implementation
4. `twk lint` with syntactic rules
5. Parser error recovery
6. `twk lint` semantic rules
7. `twk lsp` basic diagnostics + hover
