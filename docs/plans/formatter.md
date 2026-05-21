# Formatter Plan (`twk fmt`)

Canonical source formatter for Twinkle. One official style, no configuration.
Preserves comments, breaks long lines, and is idempotent.

## Design Overview

The formatter is an **AST pretty-printer with a document intermediate
representation**. The pipeline is:

```
source → lossless lex → parse → AST + trivia → Doc IR → layout (line-width aware) → output
```

The key moving parts:

1. **Lossless lexer** — comments are captured as trivia on tokens instead of
   being discarded.
2. **Doc IR** — an intermediate representation of "how to print this" that
   supports both flat (single-line) and broken (multi-line) renderings. The
   layout algorithm chooses between them based on a line-width budget.
3. **AST-to-Doc conversion** — walks the parsed AST and builds a Doc tree,
   inserting trivia (comments) at the appropriate positions.
4. **Layout engine** — renders the Doc tree to a string, greedily fitting
   groups onto single lines when they fit within the width limit.

## Step 1: Lossless Lexer (Trivia Model)

**Goal:** Make the lexer preserve comments so the formatter can re-emit them.

### Token changes (`boot/compiler/tokens.tw`)

Add a trivia field to `Token`:

```tw
pub type Token = .{
  kind: TokenKind,
  text: String,
  span: Span,
  preceded_by_newline: Bool,
  leading_trivia: Vector<Trivia>,  // NEW
}

pub type Trivia = .{
  kind: TriviaKind,
  text: String,
  span: Span,
}

pub type TriviaKind = {
  LineComment,   // "// ..."
  DocComment,    // "/// ..."
  BlankLine,     // signals an intentional blank line between items
}
```

Each token carries the comments and blank lines that appeared *before* it in
the source. A comment on line 5 followed by a token on line 6 means the
comment is `leading_trivia` of that token.

### Lexer changes (`boot/compiler/lexer.tw`)

Instead of `continue` when hitting `//`, collect the comment text and span
into a pending trivia buffer. When the next real token is produced, move the
buffer into its `leading_trivia` field. Track blank lines (two consecutive
newlines) as `BlankLine` trivia.

**Invariant:** Trivia entries within a token's `leading_trivia` vector are
ordered by source position. The lexer naturally produces them in order since
it scans left-to-right; this must be preserved (no reordering, no dedup).

**Blank line collapsing** happens at collection time: the lexer collapses 2+
consecutive blank lines into a single `BlankLine` trivia entry. This ensures
the trivia vector is the formatter's source of truth and idempotence holds
without the printer needing its own collapsing logic.

**`preceded_by_newline` preservation:** The `preceded_by_newline` field stays
unchanged — it is used by the parser for newline-sensitivity. Care is needed
when implementing the trivia buffer: after collecting a comment line, the
`saw_newline` flag must still be set from the newline at the end of the
comment line (or the newline that preceded it). The current lexer already
processes whitespace after skipping a comment, which sets `saw_newline`
correctly. The trivia-buffer change must preserve this: after appending a
comment to the trivia buffer, continue the whitespace-scanning loop exactly
as before so that `saw_newline` is set by the trailing newline. Do not reset
`saw_newline` when collecting trivia. Add a targeted test: a token preceded
by a comment on the previous line must have `preceded_by_newline = true`.

### Callers of `tokens.make` and `tokens.eof`

Adding `leading_trivia` to `Token` changes the call signature of
`tokens.make` and `tokens.eof`. All callers must be updated to supply the
new field (typically an empty vector). Known callers beyond `lex` itself:

- `lex_with_cursor` (in `lexer.tw`) — used by LSP completion. It injects a
  `CursorHole` token via `tokens.make`. Update to pass `leading_trivia: []`.
- Any test helpers that construct tokens directly.

Audit all `tokens.make` and `tokens.eof` call sites before merging Step 1.

### Parser impact

The parser reads tokens via `Cursor` and only inspects `kind`, `text`,
`span`, and `preceded_by_newline`. The new `leading_trivia` field is ignored
by the parser — it is only consumed by the formatter. No parser changes
required.

The existing `attach_doc_comments` / `extract_doc_comment` post-pass in the
parser can eventually be replaced by reading `DocComment` trivia from the
token stream, but this is not required for the initial formatter. It can
remain as-is.

**Doc comment double-emission guard:** Once the lexer classifies `///` lines
as `DocComment` trivia, the same content will exist in two places: the
token's `leading_trivia` and the AST node's `doc` field (populated by the
parser's `attach_doc_comments` pass). The printer must **skip `DocComment`
trivia entries** when emitting a node whose `doc` field is `Some(_)`, and
instead emit doc comments from the `doc` field in canonical `///` format.
This avoids printing every doc comment twice.

### Validation

- All existing tests pass unchanged (trivia is additive, parser ignores it).
- New test: lex a source with comments, verify trivia is attached to the
  correct tokens with correct spans and text.

## Step 2: Doc IR and Layout Engine

**Goal:** A line-width-aware layout engine that chooses between single-line
and multi-line renderings.

### Doc IR (`boot/compiler/fmt/doc.tw`)

The Doc type is a small algebraic data type inspired by Wadler-Lindig /
Prettier:

```tw
type Doc = {
  Nil,
  Text(String),
  Line,                         // newline + indent (when broken), space (when flat)
  HardLine,                     // always a newline (e.g. between top-level items)
  Indent(Int, Doc),             // increase indent by N for the inner doc
  Concat(Doc, Doc),             // sequencing
  Group(Doc),                   // try to fit inner doc on one line; break if it doesn't fit
  IfBreak(Doc, Doc),            // first doc when broken, second when flat
}
```

Key semantics:
- **`Group(doc)`** — the layout engine tries to render `doc` flat (all `Line`
  nodes become spaces). If the flat rendering exceeds the line width, it
  switches to broken mode (all `Line` nodes become newline + indent).
- **`IfBreak(broken, flat)`** — emit `broken` when the enclosing group is
  broken, `flat` when it fits on one line. Useful for trailing commas,
  leading separators, etc.
- **`HardLine`** — always breaks. Used between statements and top-level items.
  Any group containing a `HardLine` is always broken.

### Layout algorithm (`boot/compiler/fmt/layout.tw`)

A greedy left-to-right algorithm. Maintains a stack of `(indent, mode, doc)`
where mode is `Flat | Break`. Processes Doc nodes:

- `Text(s)` — append `s`, advance column.
- `Line` in Flat mode — append `" "`.
- `Line` in Break mode — append `"\n"` + indent spaces.
- `HardLine` — always append `"\n"` + indent spaces; force enclosing group to
  break.
- `Group(doc)` — measure whether `doc` fits flat in remaining width. If yes,
  push `(indent, Flat, doc)`. If no, push `(indent, Break, doc)`.
- `Indent(n, doc)` — push `(indent + n, mode, doc)`.
- `Concat(a, b)` — push b then a (stack is LIFO).
- `IfBreak(broken, flat)` — pick based on current mode.

The "fits?" check walks the doc in flat mode until it either exceeds the
remaining width (doesn't fit) or reaches the end of the group (fits).
**Critical:** encountering `HardLine` during the fits check must immediately
return false — a group containing `HardLine` can never fit flat. This is
O(width) per group, making the overall algorithm efficient.

**Line width:** 80 columns default. Hardcoded — no configuration.

### Builder helpers (`boot/compiler/fmt/doc.tw`)

Convenience functions to reduce boilerplate in the AST-to-Doc pass:

```tw
fn text(s: String) Doc { .Text(s) }
fn line() Doc { .Line }
fn hard_line() Doc { .HardLine }
fn nil() Doc { .Nil }
fn group(d: Doc) Doc { .Group(d) }

fn indent(d: Doc) Doc { .Indent(2, d) }

fn concat(docs: Vector<Doc>) Doc {
  docs.fold(.Nil, fn(acc, d) { .Concat(acc, d) })
}

fn join(docs: Vector<Doc>, sep: Doc) Doc { ... }

// Tight brackets: "foo(a, b, c)" on one line, or:
// foo(
//   a,
//   b,
//   c,       ← trailing comma when broken
// )
// Used for: function params, function calls, variant fields, type args.
fn tight_bracketed(open: String, close: String, docs: Vector<Doc>) Doc {
  if docs.len() == 0 { return text("${open}${close}") }
  group(concat([
    text(open),
    indent(concat([
      if_break(hard_line(), nil()),
      join(docs, concat([text(","), line()])),
      if_break(text(","), nil()),
    ])),
    if_break(hard_line(), nil()),
    text(close),
  ]))
}

// Spaced brackets: ".{ x: 1, y: 2 }" on one line, or:
// .{
//   x: 1,
//   y: 2,
// }
// Used for: record literals, record type defs, enum variant lists.
fn spaced_bracketed(open: String, close: String, docs: Vector<Doc>) Doc {
  if docs.len() == 0 { return text("${open}${close}") }
  group(concat([
    text(open),
    indent(concat([
      line(),
      join(docs, concat([text(","), line()])),
      if_break(text(","), nil()),
    ])),
    line(),
    text(close),
  ]))
}
```

## Step 3: AST-to-Doc Conversion

**Goal:** Walk the AST and produce a Doc tree, attaching trivia (comments) at
the right positions.

### New file: `boot/compiler/fmt/printer.tw`

The printer is a recursive function over AST node types. A representative
sketch of key constructs:

### Trivia attachment

The formatter needs access to the original token stream (with trivia) in
addition to the AST. The approach:

1. Build a **trivia map** from byte offset → trivia list. For each token,
   map `token.span.start → token.leading_trivia`.
2. When emitting a Doc for an AST node, look up `node.span.start` in the
   trivia map. If there's leading trivia, emit it before the node's Doc.
   For nodes with a `doc` field that is `Some(_)`, skip `DocComment` trivia
   entries (see the double-emission guard in Step 1).
3. Comments within expressions are rare in Twinkle (only `//` line comments
   exist). A `//` comment mid-expression forces a line break — emit it as
   `HardLine` + comment text + `HardLine`.
4. **Comments inside string interpolations:** The AST represents an entire
   interpolated string as a single `StringInterp(parts)` node. A comment
   inside a `${}` block is attached as trivia on an interior token, but the
   printer walks `StringPart` values from the AST, not interior tokens. To
   handle this, the printer must walk the token stream between the `span`
   boundaries of each `Interpolation(expr)` part and emit any trivia found
   on interior tokens. In practice `//` comments inside `${}` are extremely
   rare (since the interpolation is typically a short expression on one
   line), but the mechanism must exist for correctness.
5. **Trailing trivia (EOF):** Trivia after the last real token (e.g. a
   trailing comment at end-of-file) is attached as `leading_trivia` on the
   `Eof` token. The printer must emit this trivia after the last item.

### Top-level items

```
// Between items: preserve user's blank lines (up to 1),
// always at least 1 blank line between fn/type declarations.
// Consecutive `use` statements: no blank line between them.
```

### Formatting rules by construct

**Imports:**

Import groups are separated by a blank line, in this order:
1. Standard library (`use @std.*`)
2. Project imports (`use foo.*`)
3. Relative imports (`use .foo`)

Within each group, imports are sorted alphabetically by their full dotted
path string, reconstructed as `path.join(".")` (the `@` and `.` prefix
sigils are already handled by the group ordering and are excluded from the
sort key). When two imports share the same path, they are ordered:
1. Plain import (`use foo.bar`) first
2. Aliased import (`use foo.bar as baz`) second
3. Selective import (`use foo.bar.{A, B}`) third

Selective imports (`{...}`) sort their items alphabetically by name. Type
imports and value imports occupy separate namespaces in Twinkle, so name
collisions between them don't arise in practice.

```tw
use @std.fs
use @std.proc

use lib.argparse.app
use lib.source.span.{Span}

use .tokens
use .tokens.{Token, TokenKind}
```

Selective import lists break like other bracketed constructs:
```tw
// Short — stays on one line:
use foo.bar.{A, B}

// Long — breaks:
use foo.bar.{
  very_long_name,
  another_long_name,
  YetAnotherType,
}
```

**Function declarations:**
```tw
// Fits on one line:
fn add(a: Int, b: Int) Int { a + b }

// Parameters break:
fn transform(
  input: String,
  options: TransformOptions,
  callback: fn(Result<Output, Error>) Void,
) Output {
  ...
}
```

**Function calls:**
```tw
// Fits:
foo(a, b, c)

// Breaks:
foo(
  very_long_argument,
  another_argument,
  third_argument,
)
```

**Records:**
```tw
// Fits:
.{ x: 1, y: 2 }

// Breaks:
.{
  name: value,
  other_field: other_value,
}
```

**Type declarations (records):**
```tw
// Short:
type Point = .{ x: Int, y: Int }

// Long:
type FunctionDecl = .{
  is_pub: Bool,
  name: String,
  type_params: Vector<TypeParam>,
  params: Vector<Param>,
}
```

**Type declarations (enums):**
```tw
// Short:
type Option<T> = { None, Some(T) }

// Long:
type ExprKind = {
  Ident(String),
  IntLit(Int),
  Binary(BinOp, Expr, Expr),
  Call(Expr, Vector<Expr>),
}
```

**If expressions:**
```tw
// Short:
if cond { a } else { b }

// Body breaks:
if cond {
  long_expression
} else {
  other_expression
}
```

**Case expressions:**
```tw
case expr {
  .None => default_value,
  .Some(x) => x.process(),
}
```
Case arms always break (one per line). Short arms stay on one line. Long arm
bodies indent:
```tw
case expr {
  .Some(x) =>
    very_long_expression(x, other, args),
  .None => default,
}
```

**Binary expressions:**
Break before the operator when the line is too long:
```tw
result := first_thing
  + second_thing
  + third_thing
```

**Method chains:**
Break before each `.method(` when the chain is too long:
```tw
items
  .filter(fn(x) { x.is_valid() })
  .map(fn(x) { x.transform() })
  .collect()
```

**Extern declarations:**
```tw
// Extern types — always one line:
extern type "canvas" CanvasContext

// Extern functions — same rules as regular function params:
extern fn "console" log(msg: String)

extern fn "canvas" draw_image(
  ctx: CanvasContext,
  img: ImageData,
  x: Float,
  y: Float,
)
```

**Collect comprehensions:**
```tw
// Short:
collect x in items { x.name }

// Breaks:
collect item in long_collection_name {
  transform(item, options)
}

// With index:
collect value, i in items {
  .{ index: i, data: value.process() }
}

// Condition form (while-style):
collect n < limit { next() }
```

**For loops:**
```tw
// Condition form:
for running {
  step()
}

// Iterator form:
for item in items {
  process(item)
}

// With index:
for item, i in items {
  process(item, i)
}
```

**Closures:**
```tw
// Short — inline:
fn(x) { x + 1 }

// Long params or body:
fn(item: Item, index: Int) {
  transform(item, index)
}
```

**Type expressions:**

Type expressions appear in let bindings, function params, return types, type
aliases, and record fields. Formatting rules:

```tw
// Simple path types — no spaces:
Int
Vector<String>

// Applied generics — break if long:
Dict<String, Vector<Item>>

// Nested generics that break:
Dict<
  String,
  Vector<Item>,
>

// Function types — no spaces around arrow:
fn(Int, String) Bool

// Function types that break:
fn(
  LongParamType,
  AnotherParamType,
) ReturnType

// Optional shorthand — no space before ?:
String?
Vector<Item>?

// Result shorthand — no space around !:
String!Error
fn(input: String) Ast!ParseError

// Void result shorthand:
!Error
```

The `?` and `!` shorthands are postfix/infix on types. No spaces around them.
Generic argument lists (`<...>`) use `tight_bracketed` and break like
function parameter lists.

**Qualified variant patterns (in case arms):**
```tw
case item {
  .Function(decl) => handle_fn(decl),
  ast.Item.Function(decl) => handle_fn(decl),
  _ => skip(),
}
```
Qualified paths are printed as `module.Variant(args)` with no special line
breaking beyond the normal case arm rules.

**Top-level statements:**

Module-level statements (let bindings and expressions outside any function)
are formatted the same as their in-function counterparts:
```tw
// Module-level let bindings:
parse_cmd := file_command("parse", "Parse source")

// Module-level expressions:
case app.parse(cli, argv) {
  .Ok(parsed) => run_command(parsed),
  .Err(err) => exit_usage_error(app.error_message(err)),
}
```
No blank line is required between consecutive module-level statements, but
one blank line separates a statement block from a `fn`/`type` declaration.

**Bitwise and range operators:**

Bitwise operators (`&`, `|`, `^`, `<<`, `>>`, `~`) and range (`..`) follow
the same formatting rules as other binary/unary expressions. Spaces around
binary operators, no space after unary `~`:
```tw
mask := flags & 0x1F
shifted := value << 5
inverted := ~bits
indices := 0..items.len()
```
Range `..` has no spaces around it (it binds tightly, like field access).

**Defer:**
```tw
defer cleanup()

defer {
  resource.close()
  log("done")
}
```
`defer` followed by a single expression stays on one line. `defer` with a
block expression breaks normally.

**Pipelines / chained field rebinding:**
Each rebinding statement is its own statement and naturally occupies its own
line — no special formatting needed.

## Step 4: CLI Wiring

### New file: `boot/commands/fmt.tw`

```tw
pub fn run_fmt_command(parsed: ParseResult) {
  file := ...  // extract positional arg
  source := fs.read_text(file)
  
  // lex with trivia
  tokens := lexer.lex(source, file_id)
  
  // parse (ignores trivia, produces AST)
  module := parser.parse(source, file_id)
  
  // format
  formatted := formatter.format(module, tokens)
  
  if has_flag(parsed, "check") {
    if source != formatted {
      eprintln("Would reformat: ${file}")
      proc.exit(1)
    }
  } else {
    if source != formatted {
      fs.write_text(file, formatted)
    }
  }
}
```

### Changes to `boot/main.tw`

```tw
fmt_cmd := file_command("fmt", "Format source code")
  .add_flag("check", "Check formatting without modifying files")
```

Add to the `cli` commands array and `run_command` dispatch.

### Future: `--all` mode

Not in scope for the initial implementation. Later, add `--all` flag that
discovers all `.tw` files from the project root (using `twinkle.toml`) and
formats them. Report files that changed, exit non-zero if `--check` and any
file would change.

## Step 5: Testing

### Idempotence property

The most important formatter invariant: `fmt(fmt(x)) == fmt(x)`. Every test
case should verify this.

### Golden tests

A set of input `.tw` files and their expected formatted output. Directory
structure:

```
boot/tests/fixtures/fmt/
  simple_fn.input.tw
  simple_fn.expected.tw
  long_params.input.tw
  long_params.expected.tw
  comments.input.tw
  comments.expected.tw
  ...
```

Test runner reads each `.input.tw`, formats it, and compares to
`.expected.tw`.

### Coverage areas

- Imports (single, aliased, selective, long selective lists, group ordering, alphabetical sorting)
- Function declarations (short, long params, type params, no return type)
- Extern declarations (extern type, extern fn with short/long params)
- Type declarations (records, enums, aliases, generic, nested)
- Type expressions (path, applied generics, function types, optional `?`, result `!`)
- Expressions (binary chains, method chains, function calls, closures)
- Bitwise and range operators (`&`, `|`, `^`, `<<`, `>>`, `~`, `..`)
- Control flow (if/else, case, for, collect — all forms including condition-only)
- Pattern matching (wildcards, variant, qualified variant, nested patterns)
- Top-level module statements (let bindings, expressions outside functions)
- Defer (single expression, block)
- Comments (line comments before items, between items, inline, doc comments)
- Blank line preservation (between items, not excessive)
- String interpolation
- Nested structures (records in records, closures in calls)
- Already-formatted code (should be unchanged — idempotence)

## Implementation Order

1. **Trivia model** — tokens.tw + lexer.tw changes. Verify all existing
   tests still pass. Add lexer tests for trivia attachment, blank-line
   collapsing, and `preceded_by_newline` preservation after comments.
2. **Doc IR + layout engine** — doc.tw + layout.tw. Unit-test with hand-built
   Doc trees to validate line-breaking behavior, including the `HardLine`
   forces-break invariant.
3. **AST-to-Doc printer** — printer.tw. Start with a subset: imports
   (including grouping/sorting), function decls, let bindings, simple
   expressions. Golden tests at this stage are **comment-free by design**
   (trivia is not yet wired).
4. **Expand printer** — add remaining constructs: case, for, collect,
   closures, type decls, extern decls, all expression types. Continue with
   comment-free golden tests.
5. **Comment attachment** — wire trivia into the printer, including the
   doc-comment double-emission guard and string-interpolation interior walk.
   **Add new** golden test fixtures for comments (do not modify the
   comment-free fixtures from steps 3–4, so the test history stays clean).
6. **CLI wiring** — fmt.tw command, main.tw registration.
7. **Self-test** — run `twk fmt` on the boot compiler source itself. Fix any
   issues. Verify idempotence.

## Non-Goals (for now)

- **Configuration** — no line-width option, no style options. One style.
- **`--all` project-wide mode** — add later once single-file is solid.
- **Removing the `attach_doc_comments` parser pass** — can be done later once
  the formatter's trivia model is proven stable.
- **Preserving original formatting when "close enough"** — the formatter is
  opinionated. It always emits canonical output.

## Decisions

- **Trailing commas:** Yes — add when broken, omit when flat. Matches
  existing codebase convention and produces cleaner diffs.
- **Max blank lines:** Yes — collapse 2+ consecutive blank lines to 1. This
  is enforced at trivia collection time in the lexer (see Step 1).
- **Single-expression function bodies:** Keep on one line if it fits within
  the line width. `fn add(a: Int, b: Int) Int { a + b }` stays flat.
- **Case arm body threshold:** Break if the full arm (pattern + ` => ` +
  body) exceeds the line width. The body goes on a new indented line.
