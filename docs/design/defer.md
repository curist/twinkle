# Defer

`defer` schedules an expression to run when the current block exits. It is
designed for cleanup and side-effecting operations that must execute regardless
of which exit path is taken.

---

## Syntax

```tw
defer expr
```

`defer` is a statement — it does not produce a value. Since `{ ... }` is an
expression in Twinkle, both forms work naturally:

```tw
defer cleanup()                  // single call
defer { a(); b(); c() }         // block with multiple effects
defer Cell.set(counter, 0)      // any expression
```

There is no type constraint on the deferred expression; the result is silently
discarded.

---

## Semantics

### Block-scoped, LIFO

`defer` is tied to the enclosing block — the nearest `{ ... }` that contains
the `defer` statement. Multiple defers in the same block execute in LIFO order:

```tw
{
  defer { println("1") }
  defer { println("2") }
  defer { println("3") }
}
// prints: 3, 2, 1
```

### Exit paths

| Exit path | Triggers defer? |
|---|---|
| Normal completion | Yes |
| `return` | Yes (unwinds all enclosing blocks) |
| `break` | Yes (exits the loop body block) |
| `continue` | Yes (exits the current iteration block) |
| `try` propagating `Err` | Yes (unwinds as the error propagates) |
| Trap (`error()`, OOB, div-by-zero) | No (unrecoverable) |

Traps do not trigger defer — this maps directly to Wasm's trap semantics, which
cannot be intercepted by any cleanup mechanism.

### Capture-by-value

Variables are captured at declaration time, consistent with Twinkle's closure
semantics:

```tw
x := 1
defer { println("x was ${x}") }  // captures x = 1
x = 2
// prints: x was 1
```

### `return` unwinds nested blocks

When `return` exits from inside a nested block, defers run from innermost
block outward:

```tw
fn foo() Int {
  defer { println("outer") }
  {
    defer { println("inner") }
    return 0
  }
}
// prints: inner, then outer
```

### Loop body — per-iteration execution

A `defer` inside a loop runs at the end of each iteration, including the one
that `break`s. This is a direct consequence of block-scope semantics:

```tw
for x in xs {
  defer { println("end of iter ${x}") }
  if x < 0 { break }
}
```

---

## Implementation Notes

`defer` is not desugared during lowering — it remains as `CoreExprKind::Defer`
through Core IR and ANF. At the CFG level (Stage 7.6), it is eliminated via
edge insertion: deferred expressions are spliced onto every non-trap outgoing
edge in LIFO order. After this pass, no `Defer` nodes remain and the WAT
backend sees only plain sequenced code — zero runtime overhead.
