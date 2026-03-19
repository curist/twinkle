# Parser Cursor Refactor

## Problem

Every parse function in `boot/compiler/parser.tw` takes `(tokens_in: Vector<Token>, start: Int)` as its first two parameters, and every return type includes `next_index: Int`. The `tokens_in` vector is passed unchanged through ~45 functions and ~303 call sites that use the helper trio:

- `token_kind_at(tokens_in, i)` — 160 calls
- `token_span_at(tokens_in, i)` — 110 calls
- `token_text_at(tokens_in, i)` — 33 calls

This creates noise: the real moving part is the position, but every call must also thread the immutable token vector.

## Proposal

Bundle `(tokens: Vector<Token>, pos: Int)` into a `Cursor` record with inherent methods. Parse functions take and return a `Cursor` instead of the pair.

### The Cursor type

```tw
// In boot/compiler/cursor.tw (new file)
use lib.source.span
use .tokens.{Token, TokenKind}

pub type Cursor = .{
  tokens: Vector<Token>,
  pos: Int,
}

pub fn new(tokens: Vector<Token>) Cursor {
  .{ tokens, pos: 0 }
}

pub fn kind(c: Cursor) TokenKind {
  if c.pos < 0 or c.pos >= c.tokens.len() { .Eof } else { c.tokens[c.pos].kind }
}

pub fn text(c: Cursor) String {
  if c.pos < 0 or c.pos >= c.tokens.len() { "" } else { c.tokens[c.pos].text }
}

pub fn span(c: Cursor) span.Span {
  if c.pos < 0 or c.pos >= c.tokens.len() { span.new(0, 0, 0) } else { c.tokens[c.pos].span }
}

pub fn token(c: Cursor) Token? {
  c.tokens.get(c.pos)
}

// Is the current token preceded by a newline?
pub fn preceded_by_newline(c: Cursor) Bool {
  if c.pos < 0 or c.pos >= c.tokens.len() { false } else { c.tokens[c.pos].preceded_by_newline }
}

// Advance by 1
pub fn advance(c: Cursor) Cursor {
  .{ tokens: c.tokens, pos: c.pos + 1 }
}

// Peek ahead by offset (0 = current)
pub fn at(c: Cursor, offset: Int) Cursor {
  .{ tokens: c.tokens, pos: c.pos + offset }
}

// Are we past the end?
pub fn is_eof(c: Cursor) Bool {
  c.pos >= c.tokens.len() or c.kind() == .Eof
}

// Merge span from saved position to current position (exclusive)
pub fn span_from(c: Cursor, from_pos: Int) span.Span {
  if from_pos < 0 or from_pos >= c.tokens.len() {
    return span.new(0, 0, 0)
  }
  end_pos := if c.pos - 1 >= c.tokens.len() { c.tokens.len() - 1 } else { c.pos - 1 }
  if end_pos < from_pos {
    return c.tokens[from_pos].span
  }
  c.tokens[from_pos].span.merge(c.tokens[end_pos].span)
}

// Merge span from saved position to current position (inclusive of current)
pub fn span_through(c: Cursor, from_pos: Int) span.Span {
  if from_pos < 0 or from_pos >= c.tokens.len() {
    return span.new(0, 0, 0)
  }
  end_pos := if c.pos >= c.tokens.len() { c.tokens.len() - 1 } else { c.pos }
  if end_pos < from_pos {
    return c.tokens[from_pos].span
  }
  c.tokens[from_pos].span.merge(c.tokens[end_pos].span)
}
```

### Return type changes

Every `XxxParse` record drops `next_index` and gains `cursor`:

```tw
// Before:
type ExprParse = .{ expr: Expr, next_index: Int, diagnostics: Vector<Diagnostic> }

// After:
type ExprParse = .{ expr: Expr, cursor: Cursor, diagnostics: Vector<Diagnostic> }
```

All 15 parse result types (`ItemParse`, `StmtParse`, `ExprParse`, `TypeParse`, `BlockParse`, `PatternParse`, `TypeListParse`, `TypeParamsParse`, `ImportItemsParse`, `RecordFieldsParse`, `SumVariantsParse`, `ParamParse`, `ExprListParse`, `PatternListParse`, `ParamListParse`) change the same way.

### Call site transformation

```tw
// Before:
k := token_kind_at(tokens_in, i)
s := token_span_at(tokens_in, i)
t := token_text_at(tokens_in, i)
i = i + 1

// After:
k := c.kind()
s := c.span()
t := c.text()
c = c.advance()
```

```tw
// Lookahead — before:
token_kind_at(tokens_in, i + 1)

// After:
c.at(1).kind()
```

```tw
// Sub-parse call — before:
parsed := parse_expr_bp(tokens_in, i, 0)
i = parsed.next_index

// After:
parsed := parse_expr_bp(c, 0)
c = parsed.cursor
```

```tw
// Span merging — before:
merge_range(tokens_in, start, i - 1)

// After (where start_pos was saved earlier):
c.span_from(start_pos)
```

### Functions that change signature

All ~45 functions that currently take `tokens_in: Vector<Token>` as first param change to take `c: Cursor` (or use it as a local). The three helper functions (`token_kind_at`, `token_text_at`, `token_span_at`) and `merge_range` are replaced by cursor methods and deleted.

Functions that took `(tokens_in, start)` become `(c)` — the start position is implicit in `c.pos`.

Two special cases:
- `is_stmt_leader(tokens_in, idx)` → `is_stmt_leader(c)` (peeks at `c` and `c.at(1)`)
- `recover_to_next_item(tokens_in, from)` → `recover_to_next_item(c)` → returns `Cursor`

### Entry point

```tw
// Before:
pub fn parse(source: String, file_id: Int) StageResult<Module> {
  lexed := lexer.lex(source, file_id)
  tokens_in := lexed.value
  state := ParseState.{ i: 0, items: [], diagnostics: lexed.diagnostics }
  for state.i < tokens_in.len() {
    if token_kind_at(tokens_in, state.i) == .Eof { break }
    state = state.step(tokens_in)
  }
  ...
}

// After:
pub fn parse(source: String, file_id: Int) StageResult<Module> {
  lexed := lexer.lex(source, file_id)
  c := cursor.new(lexed.value)
  state := ParseState.{ cursor: c, items: [], diagnostics: lexed.diagnostics }
  for !state.cursor.is_eof() {
    state = state.step()
  }
  ...
}
```

`ParseState` changes from `{ i: Int, items, diagnostics }` to `{ cursor: Cursor, items, diagnostics }`, and `step` no longer needs the `tokens_in` parameter.

### Forward-progress guard

The current code has `advance_parse_index` (line 2312) that forces `i` forward by at least 1 when a parse function returns the same position, preventing infinite loops. The cursor-based `step` must preserve this:

```tw
fn step(self: ParseState) ParseState {
  prev_pos := self.cursor.pos
  parsed_item := parse_item_at(self.cursor)
  next_cursor := parsed_item.cursor
  // Force progress: if parse_item_at didn't advance, skip one token
  if next_cursor.pos <= prev_pos {
    next_cursor = self.cursor.advance()
  }
  .{
    cursor: next_cursor,
    items: self.items.push(parsed_item.item),
    diagnostics: self.diagnostics.concat(parsed_item.diagnostics),
  }
}
```

This is the cursor equivalent of `advance_parse_index`. It must not be dropped during the refactor.

## Execution plan

### Phase 1: Add `cursor.tw`, update types

1. Create `boot/compiler/cursor.tw` with the `Cursor` type and methods.
2. In `parser.tw`, add `use .cursor.{Cursor}`.
3. Change all 15 `XxxParse` types: `next_index: Int` → `cursor: Cursor`.
4. Change `ParseState`: `i: Int` → `cursor: Cursor`.

### Phase 2: Convert parse functions (bottom-up)

Convert leaf functions first, then callers. Each function:
- Replace `(tokens_in: Vector<Token>, start: Int)` with `(c: Cursor)`.
- Replace `i := start` with just using `c` directly (reassigning `c = c.advance()` etc.).
- Replace helper calls with cursor methods.
- Return `cursor: c` instead of `next_index: i`.

Order (leaves → roots):
1. `parse_type_params`, `parse_type_record_fields`, `parse_sum_variants`
2. `parse_type_expr_base`, `parse_type_expr`, `parse_type_list_until`
3. `parse_import_items`, `parse_param`
4. `parse_expr_list`, `parse_pattern_list`, `parse_param_list`
5. `parse_pattern`, `parse_record_literal`
6. `parse_prefix`, `parse_closure_expr`, `parse_collect_expr`
7. `parse_postfix`, `parse_expr_bp`
8. `parse_block_expr`, `parse_if_expr`, `parse_case_expr`, `parse_unary_op`
9. `parse_binding_stmt`, `parse_return_stmt`, `parse_break_stmt`, `parse_continue_stmt`, `parse_defer_stmt`, `parse_for_stmt`
10. `parse_stmt`, `parse_block`
11. `parse_use`, `parse_function`, `parse_type`
12. `parse_top_level_stmt`, `parse_item_at`, `step`, `parse`

### Phase 3: Delete old helpers

Remove `token_kind_at`, `token_text_at`, `token_span_at`, `merge_range` — all call sites now use cursor methods.

### Phase 4: Verify

1. Run `cargo test` to confirm Rust-side tests pass.
2. Run `cargo run --release -- run boot/tests/main.tw` to confirm the boot parser test suite (`boot/tests/suites/parser_suite.tw`) and all other boot tests pass.

Both are required — `cargo test` covers the Rust stage0 pipeline, while the boot test runner exercises the self-hosted parser that this refactor modifies.

## Risks

- **Large diff**: ~2300-line file, touching nearly every function. But each change is mechanical (search-and-replace shaped), so risk of logic errors is low.
- **Span positions**: `merge_range` uses inclusive end index; cursor's `span_from`/`span_through` must match exactly. Careful with off-by-one at boundaries.
- **`preceded_by_newline` access**: A few spots access `tokens_in[i].preceded_by_newline` directly. The cursor method covers this.
- **Direct `tokens_in[i]` access**: 10 occurrences that bypass the helpers (e.g., `tokens_in[i].span.end == tokens_in[i+1].span.start` in the `>>` detection). These use `c.token()` or `c.at(n).token()` with pattern matching, or direct field access on the cursor's underlying vector where needed.

## Non-goals

- No behavioral changes to parsing logic.
- No new features or error messages.
- No changes to AST types or downstream consumers.
