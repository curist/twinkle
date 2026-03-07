# Defer

## Overview

`defer` schedules an expression to run when the **current block** exits. It is designed for cleanup and side-effecting operations that must execute regardless of which exit path is taken.

---

## Syntax

```tw
defer expr
```

`defer` is a statement — it does not produce a value. Since `{ ... }` is an expression in Twinkle, both forms work naturally:

```tw
defer cleanup()                  // single call
defer { a(); b(); c() }         // block with multiple effects
defer Cell.set(counter, 0)      // any expression
```

There is no type constraint on the deferred expression; the result (if any) is silently discarded.

---

## Semantics

### Block-scoped, LIFO

`defer` is tied to the **enclosing block** — the nearest `{ ... }` that contains the `defer` statement. Multiple defers in the same block execute in **LIFO order** (last declared, first run):

```tw
{
  defer { println("1") }
  defer { println("2") }
  defer { println("3") }
}
// prints: 3, 2, 1
```

### Exit paths that trigger defer

A block's deferred expressions run whenever the block exits via:

| Exit path | Triggers defer? |
|---|---|
| Normal completion (last expression evaluated) | ✅ |
| `return` | ✅ (unwinds all enclosing blocks in the function) |
| `break` | ✅ (exits the loop body block) |
| `continue` | ✅ (exits the current iteration block) |
| `try` propagating `Err` | ✅ (unwinds through blocks as the error propagates) |
| Trap (`error()`, OOB, div-by-zero) | ❌ (unrecoverable; no cleanup) |

### Trap does not trigger defer

Traps are unrecoverable. This maps directly to Wasm's trap semantics — a Wasm trap is not a catchable exception and cannot be intercepted by any cleanup mechanism.

### Capture-by-value

Variables referenced in a `defer` expression are captured **by value at declaration time**, consistent with Twinkle's closure semantics:

```tw
x := 1
defer { println("x was ${x}") }  // captures x = 1
x = 2
// prints: x was 1
```

### `return` unwinds nested blocks

When `return` exits from inside a nested block, defers run from innermost block outward:

```tw
fn foo() Int {
  defer { println("outer") }
  {
    defer { println("inner") }
    return 0
  }
}
// prints: inner, then outer
// returns: 0
```

### Loop body — per-iteration execution

Since `defer` is block-scoped and a loop body is a block, a `defer` inside a loop runs at the end of **each iteration** — including the iteration that `break`s:

```tw
for x in xs {
  defer { println("end of iter ${x}") }
  if x < 0 { break }
}
// prints "end of iter ..." for every iteration, including the breaking one
```

This is a direct consequence of block-scope semantics, not a special rule.

---

## Implementation

### Why implementation is deferred to Stage 7.6

`defer` can be implemented superficially at the interpreter level, but its natural home is the **CFG**. At the CFG level, defer desugars completely via edge insertion — a single, clean, zero-overhead pass. Implementing it earlier would require a runtime defer-stack mechanism that gets thrown away once the CFG pass exists.

### Core IR

`defer expr` lowers to `CoreExprKind::Defer(expr_id)` — an opaque semantic node. It is not desugared during lowering; all stages up to CFG treat it as a black box.

### Interpreter

The interpreter maintains a **defer stack per block evaluation**. When a `Defer` node is encountered, the expression is pushed. When the block exits via any `Signal` except `Trap`, the stack is drained LIFO before the signal propagates outward.

### ANF IR

`Defer` nodes are preserved as-is through the ANF pass. ANF linearization does not desugar them.

### CFG desugaring (Stage 7.6)

At the CFG level, `defer` is eliminated completely via **edge insertion**. Each basic block accumulates its defer list during CFG construction. The desugaring pass then splices deferred expressions onto every non-trap outgoing edge:

```
Block B  (defers: [d1, d2], LIFO means d2 runs first)
  ├─ normal exit edge ──→ [d2; d1] → successor
  ├─ return edge ────────→ [d2; d1] → function exit
  ├─ break edge ─────────→ [d2; d1] → loop exit block
  ├─ continue edge ──────→ [d2; d1] → loop header
  └─ trap edge ──────────→ (no defers inserted)
```

After this pass, no `Defer` nodes remain. The WAT backend sees only plain sequenced code — no runtime mechanism required.

### WebAssembly backend

**Preferred:** static CFG edge insertion (zero runtime overhead, no Wasm EH required).

**Alternative:** Wasm exception handling (`try`/`catch_all`/`rethrow`) can implement defer dynamically. Since Twinkle traps are native Wasm traps — not tagged exceptions — they naturally bypass `catch_all`, giving correct trap-skips-defer behavior for free. This may be considered if static insertion proves impractical for certain edge cases.
