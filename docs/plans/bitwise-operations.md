# Bitwise Operations Support

**Status:** Phases 1-2 complete (Frontend + Execution parity)
**Last updated:** 2026-03-10

## Goal

Add first-class bitwise operations to Twinkle with consistent semantics across:

- parser/typechecker
- Core IR interpreter
- ANF + Wasm backend
- optimizer constant folding

The feature should integrate cleanly with existing numeric rules (`Int`, `Byte`, `Float`) and avoid regressions in generic type parsing.

---

## Current State

- Numeric types: `Int` (i64), `Float` (f64), `Byte` (0..255).
- Arithmetic (`+ - * / %`) already supports `Int` and `Byte` with promotion to `Int`.
- No bitwise operators exist in `BinOp`/`UnOp`.
- Wasm IR currently has i64 arithmetic/comparison opcodes, but no i64 bitwise opcodes.
- Language spec documents arithmetic/comparison only.

---

## Proposed Surface Design

### Operators

- Binary: `&`, `|`, `^`, `<<`, `>>`
- Unary: `~`

Keep logical operators unchanged:

- `and`, `or`, `!` remain boolean operators
- `and`/`or` preserve short-circuit behavior

### Typing rules

- Bitwise ops are valid only for `Int`/`Byte` operands.
- Result type is always `Int`.
- `Byte` operands are widened to their non-negative `Int` values (`0..255`) before applying the operation.
- Example: for a `Byte` value `b = 255`, `~b` is `~255`, i.e. `-256`.
- Promotion rules:
  - `Int op Int -> Int`
  - `Byte op Byte -> Int`
  - `Int op Byte -> Int`
  - `Byte op Int -> Int`
- `Float` with bitwise ops is a type error.
- `Bool` with bitwise ops is a type error.

### Shift semantics

- `<<` and `>>` use 64-bit masked shift counts.
- Effective shift count is the low 6 bits of the right operand (`right & 63`), including when the right operand is negative.
- `>>` is arithmetic right shift (sign-preserving), mapped to `i64.shr_s`.
- No new runtime traps are introduced by bitwise/shift operators.

### Precedence and associativity

All new binary operators are left-associative.

Recommended precedence (tight to loose):

1. unary (`-`, `!`, `~`, `try`)
2. multiplicative (`* / %`)
3. additive (`+ -`)
4. shift (`<< >>`)
5. comparison (`< <= > >=`)
6. equality (`== !=`)
7. bitwise and (`&`)
8. bitwise xor (`^`)
9. bitwise or (`|`)
10. logical and (`and`)
11. logical or (`or`)
12. assignment (`=`)

This follows common C/JS-family expectations for mixed expressions.

Implication:

- `x & mask == 0` parses as `x & (mask == 0)`.
- We should strongly recommend parentheses for mixed bitwise/comparison expressions, e.g. `(x & mask) == 0`.

---

## Parser and Lexer Strategy

### Tokens

Add single-character tokens:

- `Amp` for `&`
- `Pipe` for `|`
- `Caret` for `^`
- `Tilde` for `~`

### `<<` and `>>` handling

Do **not** introduce dedicated `Shl`/`Shr` lexer tokens.

Instead, recognize shifts only during **expression** Pratt parsing by combining adjacent token pairs:

- `Lt` + `Lt` (adjacent spans) => `<<`
- `Gt` + `Gt` (adjacent spans) => `>>`

`adjacent spans` means:

- no intervening whitespace
- no intervening comments
- i.e. first token end offset equals second token start offset

When three or more `<` or `>` tokens are consecutive (e.g. `<<<`), expression parsing greedily forms the first valid shift operator and continues.

Type parsing continues to treat `>` tokens independently, so nested generics like `Vector<Vector<Int>>` remain unchanged.

Rationale:

- avoids ambiguity/regression in nested generic type syntax such as `Vector<Vector<Int>>`
- keeps type parsing unchanged

---

## Compiler Pipeline Changes

### AST / syntax

- Extend `BinOp` with `BitAnd`, `BitOr`, `BitXor`, `Shl`, `Shr`.
- Extend `UnOp` with `BitNot`.
- Update parser precedence tables and token-to-op mapping.
- Add lexer support for `& | ^ ~`.

### Type checker

- Add bitwise branch in `synth_binary`.
- Add `BitNot` branch in `synth_unary`.
- Reuse current `Byte -> Int` promotion behavior used by arithmetic.

### Core lowering

- Bitwise ops lower as ordinary eager primitive operations in Core.
- Keep short-circuit rewrite restricted to logical `And`/`Or`.

### Interpreter

- Extend `eval_binop`/`eval_unop`:
  - widen `Byte` operands to `Int` before applying bitwise ops
  - apply masked shift count (`& 63`) before shifting

### ANF + codegen

- Ensure bitwise ops lower via `OpKind::Int`.
- Extend op-result type inference to return `Int`.
- Extend `emit_binop` with i64 bitwise instruction mapping.

### Wasm IR + WAT emitter

Add i64 instructions to IR and text emitter:

- `I64And`, `I64Or`, `I64Xor`, `I64Shl`, `I64ShrS`

### Optimizer

- Extend constant folding for integer literals:
  - fold `& | ^ << >>` with masked shift count
  - fold `>>` with arithmetic-right-shift semantics matching Wasm `i64.shr_s`
  - fold unary `~`
- Purity analysis: mark bitwise ops pure (same as add/sub/mul).

### Tree-sitter

- Update [`tree-sitter-twinkle/grammar.js`](../../tree-sitter-twinkle/grammar.js) with bitwise/shift precedence levels and unary `~`.
- Regenerate generated parser artifacts after grammar updates:
  - `tree-sitter-twinkle/src/grammar.json`
  - `tree-sitter-twinkle/src/node-types.json`
  - `tree-sitter-twinkle/src/parser.c`
- Update highlight queries:
  - `tree-sitter-twinkle/queries/highlights.scm`
- Update/add tree-sitter tests:
  - corpus cases in `tree-sitter-twinkle/test/corpus/01_basics.txt` (or a new corpus file)
  - highlight assertions in `tree-sitter-twinkle/test/highlight/operators.tw`

### Docs

- Update `docs/spec.md` numeric operator section.
- Update `docs/grammar.ebnf` expression grammar.

---

## Test Plan

### Parser / lexer tests

- tokenization of `& | ^ ~`
- precedence snapshots including combinations with comparison/logical ops
- precedence snapshots for:
  - `x & mask == 0` => `x & (mask == 0)`
  - `(x & mask) == 0` => explicit grouped bit-test
- parse `<<`/`>>` without breaking `Vector<Vector<Int>>` type syntax
- `a < < b` does not parse as shift
- `a > > b` does not parse as shift
- mixed generic/shift contexts parse correctly in one file

Concrete compiler parser cases to add:

- `x := a & b`
- `x := a | b`
- `x := a ^ b`
- `x := ~a`
- `x := a << 3`
- `x := a >> 2`
- `x := a < < b` (must not parse as shift)
- `x := a > > b` (must not parse as shift)
- `x := a <<< b` (greedy first shift pairing)
- `type T = Vector<Vector<Int>>` in same file as `x := y >> 1`

### Typecheck tests

Pass:

- `Int & Int`
- `Byte & Byte`
- `Int << Byte`
- `~Int`

Fail:

- `Float & Int`
- `Bool | Bool` (bitwise operator on bool)
- `~Float`

### Runtime tests (interpreter + wasm)

- basic bit masks
- sign behavior of `>>` on negative integers
- shift count masking cases (`n << 64`, `n >> 129`)
- negative shift counts (`1 << -1`, `-8 >> -1`)
- mixed `Byte`/`Int` expressions
- byte widening semantics (e.g. `b: Byte = "A"[0]`; `~b` and `b & 1` behave as widened `Int`)

### Optimization tests

- constant folding coverage for new operators
- ensure no folding mismatches between interpreter and wasm behavior

### Tree-sitter tests

Corpus parsing cases (`tree-sitter-twinkle/test/corpus/...`):

- binary bitwise ops: `a & b`, `a | b`, `a ^ b`
- shifts: `a << b`, `a >> b`
- unary bitwise not: `~a`
- mixed precedence:
  - `a + b << c`
  - `a == b & c`
  - `a & b == c`
- spacing-sensitive non-shift forms:
  - `a < < b`
  - `a > > b`
- greedy sequence case:
  - `a <<< b`

Highlight cases (`tree-sitter-twinkle/test/highlight/operators.tw`):

- operator captures for `& | ^ ~ << >>`
- ensure `and` / `or` remain `@keyword.operator` and are not confused with `&` / `|`

---

## Rollout Plan

1. **Frontend + type system**
   - AST, lexer, parser, typechecker, parser/typecheck tests
2. **Execution parity**
   - interpreter + ANF/codegen + Wasm IR/WAT emitter + run tests
3. **Optimization + tooling/docs**
   - constant folding/purity, tree-sitter, spec/grammar docs

---

## Non-Goals (This Plan)

- unsigned right shift (`>>>`)
- bitwise assignment operators (`&=`, `|=`, etc.)
- integer width/signedness type expansion (`U32`, `U64`, etc.)

These can be proposed as follow-up plans after this baseline lands.
