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

`defer` is not desugared during lowering — it remains as `ADefer` through Core
IR and into ANF. It is eliminated by the ANF optimisation pass
(`src/opt/defer_elim.rs` / `boot/compiler/opt/defer_elim.tw`) which runs
**before** any peephole passes. The pass threads two defer lists
(`fn_defers`, `loop_defers`) and a `branch_start` cursor through every node:

- **Branch entry** (`AIf`, `AMatch` arm): `branch_start` is set to
  `loop_defers.len()` so the branch remembers how many loop-defers existed
  when it was entered.
- **Terminal `Atom`** inside a branch: only defers registered *since* branch
  entry (`loop_defers[branch_start..]`) are prepended — block-scoped LIFO.
- **Terminal `Atom`** in a loop body (not inside a branch): all `loop_defers`
  are prepended (iteration-end semantics).
- **Terminal `Atom`** in function body: `fn_defers ++ loop_defers` are
  prepended (function-exit semantics).
- **`Return`**: all active defers (`fn_defers ++ loop_defers`) are prepended.
- **`Break` / `Continue`**: only `loop_defers` are prepended.

After this pass no `ADefer` nodes remain and the WAT backend sees only plain
sequenced code — zero runtime overhead.
