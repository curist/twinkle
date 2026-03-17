# Range Literal Syntax (`m..n`)

**Status:** Planned
**Priority:** Low (ergonomic improvement)

---

## Motivation

Twinkle has `range(n)` (0..n) and `range_from(start, end)` as builtin functions, plus `range_step(start, end, step)`. The `..` token already exists in the lexer (`DotDot`) and the parser maps it to `BinOp::Range`, but it is not currently usable as an expression-level operator.

Writing `for i in range_from(1, n)` is verbose compared to the more natural `for i in 1..n`, which other languages support and which Twinkle's own grammar already partially accommodates.

## Current State

`Range` is a plain record type (`TypeId(3)`) with three `Int` fields:

```tw
type Range = .{ start: Int, end: Int, step: Int }
```

The builtin functions are just constructors for this record:
- `range(n)` → `Range.{ start: 0, end: n, step: 1 }`
- `range_from(start, end)` → `Range.{ start: start, end: end, step: 1 }`
- `range_step(start, end, step)` → `Range.{ start: start, end: end, step: step }`

There is no special runtime or language-level support for ranges beyond this — `for i in r` iterates any `Range` record by stepping from `start` to `end`.

## Proposal

Support `m..n` as a binary expression that desugars to `range_from(m, n)` — i.e., it constructs the same `Range` record.

### Syntax

```tw
for i in 0..10 { ... }        // equivalent to range(10) when start is 0
for i in start..end { ... }   // equivalent to range_from(start, end)
xs := collect i in 0..n { i * i }
```

### Semantics

- `m..n` produces a `Range` value (same as `range_from(m, n)`)
- Both operands must be `Int`
- Half-open: includes `m`, excludes `n` (consistent with `range`/`range_from`)

### Implementation

1. **Parser**: The `DotDot` token is already recognized and mapped to `BinOp::Range` with precedence `(2, 3)`. Verify it works in expression position (currently may only work in `for` headers or type contexts).

2. **Type checker**: `BinOp::Range` already handled — unifies both sides with `Int`. Update the result type from the current placeholder to the proper `Range` type (`Named(TypeId(3), [])`).

3. **Lowering**: Lower `Binary(Range, lhs, rhs)` to `Call(RANGE_FROM, [lhs, rhs])`. Check if this already happens — the lowerer may already handle `BinOp::Range`.

4. **Boot compiler**: Mirror the same handling in the self-hosted parser/checker/lowerer.

### Open Questions

- Should `m..=n` (inclusive range) be supported? Not in MVP.
- Should `m..n..step` or `m..n step s` syntax exist, or keep `range_step` as a function?

### Scope

Small change — mostly wiring up what's already partially there. Main work is verifying the parser accepts `m..n` in general expression position and that lowering produces the right call.
