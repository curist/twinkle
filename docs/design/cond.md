# Cond

`cond` is an expression that evaluates a sequence of boolean conditions in
order, producing the value of the first branch whose condition is true. It
replaces deeply nested `if/else if/else` chains when the conditions are
heterogeneous — checking different variables, calling different functions, or
testing ranges — so `case` pattern matching does not apply.

---

## Syntax

```tw
cond {
  condition1 => expr1,
  condition2 => expr2,
  _ => default_expr,
}
```

Each arm is `bool_expr => body_expr`, separated by commas.  The wildcard `_`
arm serves as the default and must appear last when present.

Braces and commas follow the same rules as `case` arms.

---

## Semantics

Arms are evaluated top-to-bottom. The first arm whose condition is `true`
determines the value of the entire `cond` expression.  Subsequent conditions
are not evaluated (short-circuit).

If no arm matches and there is no `_` arm, the behavior depends on context:

- **Expression position** (the result is used): a `_` arm is required.
  Omitting it is a compile-time error.
- **Statement position** (result is discarded / type is `Void`): the `_` arm
  may be omitted, equivalent to `_ => {}`.

This mirrors `case` exhaustiveness: `case` on enums requires all variants
or `_`; `cond` always requires `_` when used as an expression.

---

## Type Rules

- Every condition must have type `Bool`.
- All arm bodies must unify to a common type (the type of the `cond`
  expression), following the same rules as `case` and `if/else`.
- Diverging arms (`return`, `error()`) do not participate in type unification,
  same as `case` and `if/else`.

---

## Motivating Examples

### Range classification

```tw
// before
fn utf8_len_at(s: String, pos: Int) Int {
  lead := Byte.to_int(s[pos])
  if lead < 0x80 {
    1
  } else if lead < 0xE0 {
    2
  } else if lead < 0xF0 {
    3
  } else {
    4
  }
}

// after
fn utf8_len_at(s: String, pos: Int) Int {
  lead := Byte.to_int(s[pos])
  cond {
    lead < 0x80  => 1,
    lead < 0xE0  => 2,
    lead < 0xF0  => 3,
    _            => 4,
  }
}
```

### Method-call dispatch

```tw
// before
if decl.is_stdlib {
  loader.resolve_stdlib_module_path(stdlib_root, decl.path)
} else if decl.is_relative {
  loader.resolve_relative_module_path(importing_file, decl.path)
} else {
  loader.resolve_module_path(import_root, decl.path)
}

// after
cond {
  decl.is_stdlib   => loader.resolve_stdlib_module_path(stdlib_root, decl.path),
  decl.is_relative => loader.resolve_relative_module_path(importing_file, decl.path),
  _                => loader.resolve_module_path(import_root, decl.path),
}
```

### Side-effecting arms with blocks

```tw
cond {
  is_byte(next, "n")  => { parts = .append("\n") },
  is_byte(next, "t")  => { parts = .append("\t") },
  is_byte(next, "r")  => { parts = .append("\r") },
  is_byte(next, "\\") => { parts = .append("\\") },
  is_byte(next, "\"") => { parts = .append("\"") },
  is_byte(next, "\$") => { parts = .append("\$") },
  is_byte(next, "e")  => { parts = .append("\e") },
  is_byte(next, "x") and i + 3 < n => {
    hi := hex_digit_value(raw[i + 2])
    lo := hex_digit_value(raw[i + 3])
    // ...
  },
  _ => {},
}
```

### Hex digit parsing

```tw
fn hex_digit_value(ch: Byte) Int {
  cond {
    in_byte_range(ch, "0", "9") => ch - byte_value("0"),
    in_byte_range(ch, "A", "F") => ch - byte_value("A") + 10,
    in_byte_range(ch, "a", "f") => ch - byte_value("a") + 10,
    _                           => -1,
  }
}
```

---

## Why Not `case` With Guards?

Some languages (Haskell, Rust) support `case expr { pat if cond => ... }`.
This was considered but rejected for Twinkle:

1. **No scrutinee.** Most cond-eligible chains do not have a single value to
   match — they check different variables, call different functions, or test
   compound boolean expressions.  Writing `case true { _ if cond => ... }`
   works but is awkward and misleading: it suggests pattern matching is
   happening when it is not.

2. **Orthogonal concerns.** Pattern matching decomposes data (`case`); boolean
   dispatch selects a code path (`cond`). Keeping them separate makes intent
   clear at the call site.

3. **Simpler implementation.** `cond` desugars directly to nested `if/else` in
   the IR. No changes to pattern-match compilation, exhaustiveness checking, or
   the type-directed lowering of `case`.

Guards on `case` may still be added in the future for the case where you want
both destructuring *and* additional boolean conditions on the same scrutinee.
`cond` does not preclude that.

---

## Implementation

### Parser

- New keyword: `cond`.
- Parse: `cond { (expr => expr ,)* (_ => expr ,)? }`.
- AST node: `Cond(arms: Vector<CondArm>)` where
  `CondArm = .{ condition: Expr?, body: Expr }` (condition is `None` for the
  `_` arm).

### Type Checking

- Check each condition has type `Bool`.
- Unify all arm bodies.
- If the cond appears in expression position and has no `_` arm, emit an error.

### Lowering

Desugar to nested `If` in Core IR:

```
cond { c1 => e1, c2 => e2, _ => e3 }
→ If(c1, e1, If(c2, e2, e3))
```

No new IR nodes required. The optimizer, monomorphizer, and codegen see
ordinary `If` chains.

### Formatter

Format `cond` with the same rules as `case`:
- Arms aligned, one per line.
- Trailing comma after each arm.
- Short single-arm forms on one line if they fit.
