# Boot Compiler Refactor Plan

## Goal

Reduce duplication and improve maintainability in `boot/compiler/parser.tw` and
`boot/compiler/lexer.tw` through targeted extractions and simplifications, without
changing any parsing behavior.

## Scope

All changes are internal refactors — no new syntax, no behavioral changes, no new
test files needed (existing tests must continue to pass).

---

## Phase 1: Extract `parse_unary_op` helper

**Files:** `boot/compiler/parser.tw`

Four arms in `parse_prefix` (`.Minus`, `.Bang`, `.Tilde`, `.Try`) duplicate the
same body, differing only in the `UnOp` variant.

**Before (repeated 4x):**
```tw
.Minus => {
  parsed_rhs := parse_expr_bp(tokens_in, start + 1, 22)
  diagnostics_out = diagnostics_out.concat(parsed_rhs.diagnostics)
  expr := Expr.{ kind: .Unary(UnOp.Neg, parsed_rhs.expr), span: ... }
  .{ expr, next_index: parsed_rhs.next_index, diagnostics: diagnostics_out }
},
```

**After:**
```tw
fn parse_unary_op(tokens_in: Vector<Token>, start: Int, op: UnOp) ExprParse {
  parsed_rhs := parse_expr_bp(tokens_in, start + 1, 22)
  expr := Expr.{
    kind: .Unary(op, parsed_rhs.expr),
    span: span.span_merge(token_span_at(tokens_in, start), parsed_rhs.expr.span),
  }
  .{ expr, next_index: parsed_rhs.next_index, diagnostics: parsed_rhs.diagnostics }
}
```

Each arm becomes one line:
```tw
.Minus => parse_unary_op(tokens_in, start, UnOp.Neg),
.Bang  => parse_unary_op(tokens_in, start, UnOp.Not),
.Tilde => parse_unary_op(tokens_in, start, UnOp.BitNot),
.Try   => parse_unary_op(tokens_in, start, UnOp.Try),
```

**Risk:** Very low — pure extraction with no logic change.

---

## Phase 2: Collapse literal pattern branches

**Files:** `boot/compiler/parser.tw`

Three sequential `if` blocks in `parse_pattern` handle `IntLit`, `FloatLit`, and
`StringLit` with identical structure.

**After:**
```tw
if k == .IntLit or k == .FloatLit or k == .StringLit {
  expr_kind: ExprKind = case k {
    .IntLit   => .IntLit(token_text_at(tokens_in, i)),
    .FloatLit => .FloatLit(token_text_at(tokens_in, i)),
    _         => .StringLit(token_text_at(tokens_in, i)),
  }
  s := token_span_at(tokens_in, i)
  lit := Expr.{ kind: expr_kind, span: s }
  patt := Pattern.{ kind: .Literal(lit), span: s }
  return .{ pattern: patt, next_index: i + 1, diagnostics: diagnostics_out }
}
```

**Risk:** Low — same logic, just consolidated.

---

## Phase 3: Extract `parse_comma_list` helper

**Files:** `boot/compiler/parser.tw`

The comma-separated-list-within-delimiters loop appears 6 times with identical
structure (function params, call args, closure params, pattern variant args x2,
variant literal args). There is already a `parse_type_list_until` helper for type
lists — this phase creates the expression-level equivalent.

The tricky part is that different call sites parse different item types (expressions,
patterns, params). Two approaches:

**Option A — One helper per item type:**
```tw
// For expression lists (call args, variant literal args)
type ExprListParse = .{
  value: Vector<Expr>,
  next_index: Int,
  diagnostics: Vector<Diagnostic>,
}

fn parse_expr_list(tokens_in: Vector<Token>, start: Int, end_kind: TokenKind) ExprListParse {
  args: Vector<Expr> = []
  diagnostics_out: Vector<Diagnostic> = []
  i := start
  for i < tokens_in.len() and token_kind_at(tokens_in, i) != end_kind {
    parsed := parse_expr(tokens_in, i)
    args = args.push(parsed.expr)
    diagnostics_out = diagnostics_out.concat(parsed.diagnostics)
    i = parsed.next_index
    if token_kind_at(tokens_in, i) == .Comma {
      i = i + 1
    } else if token_kind_at(tokens_in, i) != end_kind {
      diagnostics_out = diagnostics_out.push(diagnostic.error(
        token_span_at(tokens_in, i),
        "expected ',' or closing delimiter",
      ))
      if token_kind_at(tokens_in, i) == .Eof { break }
      i = i + 1
    }
  }
  if token_kind_at(tokens_in, i) == end_kind { i = i + 1 }
  .{ value: args, next_index: i, diagnostics: diagnostics_out }
}
```

Similarly, `parse_pattern_list` and `parse_param_list` for the other item types.

**Option B — Just extract expression lists, leave the rest.**
Patterns and params only have 2–3 sites each and may have subtle differences worth
keeping inline. Focus the helper on expression lists (3 sites) where the duplication
is cleanest.

**Recommendation:** Option B for now — extract `parse_expr_list` for the 3 pure
expression-list sites, leave pattern/param loops inline.

**Risk:** Low — mirrors existing `parse_type_list_until` pattern.

---

## Phase 4: Uniform `XxxParse` payload field name — SKIPPED

**Decision:** On closer inspection, ~40+ construction sites use Twinkle's shorthand
field syntax (e.g., `.{ expr, next_index: i, diagnostics: ... }` where the local
variable `expr` matches the field name). Renaming to `value` would force all these
to the explicit form `.{ value: expr, ... }`, making construction sites *more*
verbose. The net effect is roughly a wash — uniform access (`parsed.value`) vs
concise construction (shorthand). The current names also aid readability at access
sites by indicating what type is being extracted.

**Verdict:** Not worth the churn. Keep descriptive field names.

---

## Phase 5: Remove dead helpers and inline trivial accessors

**Files:** `boot/compiler/parser.tw`, `boot/compiler/lexer.tw`

### parser.tw
- **Remove `span_of_expr`** — defined but never called.
- **Inline `span_of_type`** — called exactly once; replace with `ty.span` at call site.

### lexer.tw
- **Inline `push_depth`, `pop_depth`, `top_depth`, `set_top_depth`** — each wraps
  a single `Vector` operation and is called 1–2 times. Replace with direct vector
  method calls (e.g., `interp_depths.push(1)`, `interp_depths[interp_depths.len() - 1]`).

**Risk:** Very low — trivial inlining.

---

## Status

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | Extract `parse_unary_op` helper | Done |
| 2 | Collapse literal pattern branches | Done |
| 3 | Extract comma-list helpers | Done (all 3 item types: expr, pattern, param) |
| 4 | Uniform payload field name | Skipped (shorthand construction makes it a net negative) |
| 5 | Remove dead helpers / inline trivials | Done |

## Non-Goals

- Changing AST types or parser behavior
- Introducing new language features to support the refactoring (e.g., generics for parse results)
- Refactoring the Rust-side compiler
