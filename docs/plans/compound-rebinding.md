# Compound rebinding with method shorthand

## Context

Twinkle's immutable-by-default model means accumulation and transformation
patterns require explicit rebinding:

```tw
deps = deps.append(item)
canonical_paths = canonical_paths.append(path)
buf = buf.concat(other)
items = items.filter(fn(x) { x.active }).map(fn(x) { x.name })
state.items = state.items.append(entry)
```

The left-hand side appears twice on every such line. This is the most common
source of repetition in the boot compiler, appearing in nearly every function
that builds up a collection or threads state through method calls.

## Goal

Allow `.method(args)` on the right-hand side of a rebinding to implicitly
use the left-hand side as the receiver:

```tw
deps = .append(item)
canonical_paths = .append(path)
buf = .concat(other)
items = .filter(fn(x) { x.active }).map(fn(x) { x.name })
state.items = .append(entry)
```

Each of these desugars mechanically:

```tw
x = .method(args)
// becomes:
x = x.method(args)

obj.field = .method(args)
// becomes:
obj.field = obj.field.method(args)
```

When the method call is followed by a chain, the entire chain is part of the
right-hand side:

```tw
x = .foo().bar().baz()
// becomes:
x = x.foo().bar().baz()
```

### Why not a separate `.=` operator?

Twinkle deliberately avoids compound assignment operators (`+=`, `-=`, etc.).
Introducing `.=` as a one-off special form would create an operator family of
one — inconsistent with the language's preference for a single `=` token.

Reusing `=` with a `.method()` shorthand on the RHS keeps the assignment
story unified. The parser-enforced naming convention (`.lowercase` is always a
method/field, `.Uppercase` is always a variant) ensures there is no ambiguity
with existing contextual expressions like `x = .None` or `x = .Some(v)`.

## Non-goals

- No new semantics beyond syntactic desugaring into existing rebinding.
- No interaction with `Cell` or mutable references.
- No compound arithmetic operators (`+=`, `-=`, etc.).
- The shorthand does not create a general "implicit receiver" expression form.
  `.lowercase(args)` is only valid directly after `=` in an assignment — it is
  not a standalone expression.

## Design

### Syntax

The shorthand applies to any valid assignment target (LValue):

```
<ident> = .<method-or-field> ...
<lvalue>.<field> = .<method-or-field> ...
<lvalue>[<index>] = .<method-or-field> ...
```

When the right-hand side of a rebinding starts with `.lowercase`, the parser
reconstructs the full left-hand-side expression and inserts it as the receiver.

This does not apply when the right-hand side starts with `.Uppercase` (variant
literal) or `.{` (anonymous record literal) — those retain their existing
contextual meaning.

### Disambiguation

The parser decides based on the token after `.`:

| RHS starts with | Meaning | Example |
|---|---|---|
| `.lowercase(` | Method shorthand — insert LHS as receiver | `x = .append(1)` |
| `.lowercase` (no parens) | Field shorthand — insert LHS as receiver | `x = .len` |
| `.Uppercase` | Variant literal (existing behavior) | `x = .None` |
| `.{` | Anonymous record literal (existing behavior) | `x = .{ a: 1 }` |

The field shorthand (`x = .len`) follows the same logic — it desugars to
`x = x.len`. This is less common but consistent.

Chained field access works naturally: `x = .items.len` desugars to
`x = x.items.len`. The parser inserts the LHS as the receiver of the first
`.items` access, and `.len` is parsed as a normal postfix chain on the result.

### Evaluation order for index targets

`arr[i] = .method()` desugars to `arr[i] = arr[i].method()`. The index
expression `i` appears twice in the desugared AST and may be evaluated twice.
This is consistent with the existing update syntax behavior described in
spec §7.6, where "implementations should evaluate the left-hand side once
when lowering." The same lowering strategy applies here — if the
implementation introduces a temporary for the index in existing update sugar,
compound rebinding should use the same mechanism.

### Type checking

No changes. The desugared form is a regular rebinding statement, which the
checker already handles.

### Codegen

No changes. The desugared AST uses existing nodes.

### Error messages

If the left-hand side is not a rebindable target, the existing rebinding
error applies.

If the method does not exist on the target's type, the existing method
resolution error applies.

## Implementation

### Parser interception point

Assignment is handled as an infix operator in the Pratt parser
(`parse_expr_bp` in `parser.tw`). When the parser sees `=`, it calls
`parse_expr_bp(c.advance(), info.right)` to parse the RHS, which enters
`parse_prefix`. Today, `parse_prefix` handles `.` + `Ident` as a variant
node regardless of identifier case (line 1794).

The desugaring must intercept **before** the recursive `parse_expr_bp` call
for the RHS. Specifically, in the `.Eq` infix handler at line 2189:

1. After consuming `=`, check whether the next tokens are `.` + lowercase
   `Ident` (using the existing `is_upper_ascii_name` helper to distinguish).
2. If so, **do not** recurse into `parse_expr_bp` normally. Instead:
   a. Reconstruct the LHS (`lhs`) as a receiver expression. For a simple
      `Ident` node, reuse it directly. For a `Binary(.FieldAccess, ...)` or
      `Binary(.Index, ...)`, reconstruct the full access chain.
   b. Parse the `.method(args)` as a postfix expression on the reconstructed
      receiver (reuse `parse_postfix` or equivalent).
   c. Continue parsing any further postfix chain.
   d. Emit the normal `Binary(.Assign, lhs, method_call_expr)` node.
3. If the tokens after `=` are `.` + uppercase `Ident` or `.{`, fall through
   to the existing `parse_expr_bp` path (variant / record literal).
4. Otherwise, fall through to the existing `parse_expr_bp` path.

This keeps the change localized to the `=` infix handler and does not alter
`parse_prefix` behavior for `.` in any other context.

### Scope of the shorthand

The shorthand applies only in the assignment RHS context described above. It
does not make `.lowercase(args)` a valid expression in other positions (e.g.,
function arguments, return expressions, `let` initializers). The `parse_prefix`
handler for `.Dot` continues to produce Variant nodes for `.Ident` regardless
of case, as it does today.

### Files to change

- `boot/compiler/parser.tw` — intercept in the `=` infix handler
- `src/parse/parser.rs` — same interception in stage0
- `docs/grammar.ebnf` — document the shorthand in the `AssignExpr` rule
- `tree-sitter-twinkle/grammar.js` — update assignment rule; this likely
  requires adding an alternative in the assignment RHS that matches
  `.` + lowercase identifier + optional arguments + optional postfix chain,
  with appropriate precedence annotations to avoid conflicts with variant
  literals

## Examples

Before and after for common boot compiler patterns:

```tw
// Vector accumulation
deps = .append(item)
parts = .append(segment)
errors = .concat(new_errors)

// Dict update
env = .set(key, value)

// Chained transforms
items = .filter(fn(x) { x.active }).map(fn(x) { x.name })

// String building
out = .concat(prefix)
out = .concat(body)
out = .concat(suffix)

// Record field rebinding
state.items = .append(entry)
state.errors = .concat(new_errors)
ctx.locals = .set(idx, frame)

// Nested field
a.b.c = .method()

// Index target
arr[i] = .append(x)
```

## Tests

- Basic desugaring: `x = .append(1)` on a `Vector<Int>` produces the
  expected value.
- Chained methods: `x = .filter(...).map(...)` applies the full chain.
- Field shorthand: `x = .len` desugars to `x = x.len`.
- Chained field access: `x = .items.len` desugars to `x = x.items.len`.
- Record field LHS: `r.items = .append(1)` desugars to
  `r.items = r.items.append(1)`.
- Nested field LHS: `a.b.c = .method()` desugars to
  `a.b.c = a.b.c.method()`.
- Index LHS: `arr[i] = .method()` desugars to `arr[i] = arr[i].method()`.
- Variant literals still work: `x = .None` remains a variant assignment.
- Record literals still work: `x = .{ a: 1 }` remains an anonymous record.
- Not valid outside assignment: `.method()` in a function argument or `let`
  initializer remains a parse error (or variant node as today).
- Type error: `.method()` with a non-existent method reports the standard
  method resolution error.
- Scope error: used on a name that is not a rebindable target reports the
  standard rebinding error.
