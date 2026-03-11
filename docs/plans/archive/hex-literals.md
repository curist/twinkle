# Hexadecimal Integer Literals

**Status:** Proposed
**Last updated:** 2026-03-09

## Goal

Add `0x` hexadecimal integer literals to Twinkle. Hexadecimal integer literals
evaluate to values of type `Int`.

This pairs naturally with the `Byte` type and byte-first string model, where hex
notation makes UTF-8 bytes, bit masks, and binary data much more readable.

---

## Specification

### Grammar

```ebnf
int_literal  := decimal_literal | hex_literal
hex_literal  := "0x" hex_digit { hex_digit }
hex_digit    := "0".."9" | "a".."f" | "A".."F"
```

`decimal_literal` is defined in the existing numeric literals grammar.
Only the lowercase prefix `0x` is recognized (not `0X`).

Hex literals that exceed the range of `Int` (i64) are a compile-time error.

### Typing

All hex literals have type `Int`. Conversion to `Byte` is explicit:

```tw
0xFF              // Int
Byte.from_int(0xFF)  // Byte?
```

### Examples

```tw
0x0
0x1
0xA
0x10
0xFF
0xC3
0xdeadbeef
```

### Invalid forms

```tw
0x        // lex error: no digits after 0x
0xG1      // lex error: invalid hex digit
0xFFFFFFFFFFFFFFFFF  // compile error: out of range for Int
```

### Negation

`-0x1` is parsed as unary minus applied to `0x1`, not as part of the literal token.

---

## Motivation

### Byte-first string model

With byte-based string APIs, hex is the natural notation for UTF-8 bytes:

```tw
"é"[0] == 0xC3
"é"[1] == 0xA9
```

Much clearer than `195` and `169`.

### Byte type interaction

```tw
b := Byte.from_int(0xC3)?

for b in "é" {
  if Byte.to_int(b) == 0xC3 { ... }
}

magic := [0x89, 0x50, 0x4E, 0x47]  // PNG header bytes
```

### Bit operations and masks

```tw
x & 0xFF
flag = 0x10
```

### Compiler/runtime internals

Wasm opcodes, encoded values, and binary protocol work all benefit from hex notation.

---

## What we defer

- `0b` (binary literals)
- `0o` (octal literals)
- Digit separators (`0xFF_FF`)
- Typed suffixes (`0xFFu8`)

These can be added later if needed. `0x` alone covers the vast majority of use cases.

---

## Implementation

### Lexer (`src/syntax/lexer.rs`)

In `lex_number()`, after consuming the first digit, check if it is `0` followed by `x`.
If so, consume hex digits instead of decimal digits. Store the full text including the
`0x` prefix in the token.

The token kind remains `IntLit` — no new token variant needed.

### Parser (`src/syntax/parser.rs`)

In `parse_int_literal()`, detect the `0x` prefix and use `i64::from_str_radix(&text[2..], 16)`
instead of `text.parse::<i64>()`. Overflow should produce a clear error message
(e.g., "hexadecimal literal out of range for Int"), not a raw Rust parse error.

Same for integer patterns in `parse_pattern()`.

### No changes needed downstream

The AST already uses `Literal::Int(i64)`. Once the parser produces the correct i64 value,
the rest of the pipeline (type checker, lowering, codegen, interpreter) works unchanged.

---

## Testing

### Lexer tests

- `0xFF` → `IntLit` with text `"0xFF"`
- `0x0` → `IntLit`
- `0xdeadbeef` → `IntLit`
- `0x` alone → lex error
- `0xG` → lex error

### Parser tests

- `0xFF` → `Literal::Int(255)`
- `0x10` → `Literal::Int(16)`
- `-0x1` → unary minus of `Literal::Int(1)`
- `0xFF + 1` → binary add
- `0x7FFFFFFFFFFFFFFF` → `Literal::Int(i64::MAX)`
- `0x8000000000000000` → overflow compile error

### Integration tests

- Arithmetic with hex: `0xFF + 1 == 256`
- Comparison: `0xC3 == 195`
- Use with `Byte.from_int`: `Byte.from_int(0xFF)`
- Pattern matching on hex values
