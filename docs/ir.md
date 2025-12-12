# Twinkle Intermediate Representations

Core IR & ANF IR Specification

This document defines the intermediate representations used by the Twinkle compiler.
The IR pipeline is:

```
Typed AST
   → Core IR        (semantic, block-structured)
   → ANF IR         (backend-oriented, optional)
   → { Interpreter | WAT/Wasm }
```

Both IRs are stable, explicit, and designed for self-hosting.

---

# 1. Overview

Twinkle uses **two IR layers**:

1. **Core IR**

   * High-level but *fully desugared*.
   * Encodes Twinkle semantics directly.
   * Block-structured, expression-oriented.
   * Suitable for interpretation and high-level optimizations.
   * Easy to generate from typed AST.

2. **ANF IR (Administrative Normal Form)**

   * Lower-level, linearized evaluation order.
   * Every intermediate value is bound to a `let`.
   * Designed for straightforward code generation (e.g., WAT/Wasm).
   * Optional for interpretation, but ideal for backends.

All Twinkle surface features (`collect`, `for x in`, `try`, `.Variant`, implicit returns, block expressions) are removed or made explicit during lowering to Core IR.

---

# 2. Core IR

## 2.1 Goals

Core IR must:

* Preserve Twinkle’s evaluation semantics faithfully.
* Be simple: no operator precedence, no sugar.
* Be explicit: no hidden control flow, no implicit returns.
* Be stable enough that interpreter & backend use the same semantics.
* Serve as the canonical truth of the language.

## 2.2 Core Concepts

### Modules

A module is a list of **function definitions** and **top-level expressions** (already lowered to `main()` or similar by the front end).

```
Module = { FunctionDef }
```

### Values

Core IR does not define a runtime; but the interpreter uses:

* integers, floats, bools, strings,
* arrays, dicts,
* records (nominal),
* variants (nominal),
* closures (captured env + function id),
* void.

### Local Identifiers

Core IR uses **integer-based locals** (`LocalId`) instead of names:

```
local: LocalId (u32 or usize)
```

Each function has its own local id space.

---

## 2.3 Core IR Node Definitions

### 2.3.1 Function Definition

```
FunctionDef {
    func_id: FuncId,
    params: Vec<LocalId>,
    body: Expr,
    return_ty: Type,       // from typechecker
}
```

### 2.3.2 Expressions

All Core IR nodes are **expressions** and produce a value (including void).

```
Expr =
    | LitInt(i64)
    | LitFloat(f64)
    | LitBool(bool)
    | LitStr(String)
    | LitVoid

    | Local(LocalId)

    | Let { local: LocalId, value: Box<Expr>, body: Box<Expr> }

    | Call { callee: Box<Expr>, args: Vec<Expr> }

    | Lambda { params: Vec<LocalId>, body: Box<Expr> }

    | If { cond: Box<Expr>, then_branch: Box<Expr>, else_branch: Box<Expr> }

    | Match { scrutinee: Box<Expr>, arms: Vec<MatchArm> }

    | Loop { body: Box<Expr> }
    | Break { value: Option<Box<Expr>> }
    | Continue

    | Record { type_id: TypeId, fields: Vec<(FieldId, Expr)> }
    | RecordGet { target: Box<Expr>, field: FieldId }

    | Variant { type_id: TypeId, variant: VariantId, args: Vec<Expr> }

    | ArrayLit(Vec<Expr>)
    | Index { base: Box<Expr>, index: Box<Expr> }
```

### 2.3.3 Match Arms

```
MatchArm {
    pattern: Pattern,
    body: Expr,
}
```

### 2.3.4 Patterns

Patterns are fully resolved (no identifiers as “variables vs variants” ambiguity):

```
Pattern =
    | PatWildcard
    | PatVar(LocalId)
    | PatLitInt(i64)
    | PatLitBool(bool)
    | PatLitStr(String)
    | PatVariant {
         type_id: TypeId,
         variant: VariantId,
         fields: Vec<Pattern>,
      }
```

---

## 2.4 Core IR Invariants

Core IR must satisfy:

1. **No surface syntax** remains:

   * No `for x in y`,
   * No `collect`,
   * No `.Variant` shorthand,
   * No implicit returns,
   * No block statements → all converted to `Let` or nested exprs.

2. `If` always has both branches.

3. `Match` arms are exhaustive or one arm is `_`.

4. `Loop { body }`:

   * `body` must be an expression.
   * `Break` may or may not carry a value (depending on loop usage).

5. All locals are pre-numbered and unique in function scope.

6. **All type information is known**:

   * Each Expr has a known type from typechecking,
   * Lowering attaches necessary `type_id`, `variant_id`, `field_id`.

---

## 2.5 Surface-to-Core Lowering Rules

### Rule 1 — Blocks become nested `Let`

Surface:

```
{
    a := expr1;
    b := expr2;
    a + b
}
```

Core:

```
Let(a, expr1,
  Let(b, expr2,
    Add(Local(a), Local(b))
  )
)
```

### Rule 2 — Implicit return is eliminated

Surface function:

```
fn f(x: int) int {
    x + 1   // implicit return
}
```

Core IR:

```
Lambda(...) => Let(tmp, Add(Local(x), LitInt(1)), Local(tmp))
```

### Rule 3 — `try expr` becomes a match over `Result`

Surface:

```
y := try foo(x)
```

Core:

```
Match foo(x) {
  PatVariant(Result, Ok, [v])   => Let(y, v, body...)
  PatVariant(Result, Err, [e])  => BreakReturn(Variant(Result, Err, [e]))
}
```

(Actual lowering depends on function context.)

### Rule 4 — `collect x in xs { body }`

Surface:

```
collect x in xs {
    x * 2
}
```

Core sketch:

```
Let(acc, ArrayLit([]),
  Loop {
    Let(it, next(xs),        // resolved via iterator lowering
    Match it {
      None       => Break(acc),
      Some(x)    =>
        Let(v, body,
        Let(_, push(acc, v),
        Continue)))
  })
```

### Rule 5 — `for x in xs { ... }`

Surface:

```
for x in xs {
   do_something(x)
}
```

Core IR lowered to explicit iteration logic (depending on type: array/range/dict).

### Rule 6 — `.Variant(args)` shorthand resolves to explicit variant construction

Surface:

```
.Ok(42)
```

Core:

```
Variant { type_id: Result, variant: Ok, args: [LitInt(42)] }
```

### Rule 7 — Record literals and access are explicit

Surface:

```
Point.{ x: 1, y: 2 }.x
```

Core:

```
Record(type=Point, fields=[(x,1),(y,2)])
→ RecordGet(field=x)
```

---

# 3. ANF IR (Administrative Normal Form)

ANF is optional but recommended for backend simplicity.

## 3.1 Goals

* Make evaluation order explicit.
* Move all intermediate expressions into explicit `let`s.
* Ensure each computation is either:

  * a **simple atom** (local or literal), or
  * a `let` returning an atom.

This simplifies mapping to stack-machine or SSA-like backends.

---

## 3.2 ANF Concepts

### Atom

```
Atom =
    | ALocal(LocalId)
    | ALitInt(i64)
    | ALitFloat(f64)
    | ALitBool(bool)
    | ALitStr(String)
    | ALitVoid
```

### ANF Expression

```
ANFExpr =
    | Let(local, AOp, ANFExpr)
    | AReturn(Atom)
```

### ANF Operation (non-atomic)

```
AOp =
    | ACall { callee: Atom, args: Vec<Atom> }
    | AIf { cond: Atom, then_branch: ANFExpr, else_branch: ANFExpr }
    | AMatch { scrutinee: Atom, arms: Vec<ANFMatchArm> }
    | ALoop { body: ANFExpr }
    | ABreak { value: Option<Atom> }
    | AContinue

    | ARecord { type_id, fields: Vec<(FieldId, Atom)> }
    | ARecordGet { target: Atom, field: FieldId }

    | AVariant { type_id, variant, args: Vec<Atom> }

    | AArrayLit(Vec<Atom>)
    | AIndex { base: Atom, index: Atom }

    | ALambda { params: Vec<LocalId>, body: ANFExpr }
```

### ANF Match Arm

```
ANFMatchArm {
    pattern: Pattern,      // Same resolved pattern format as Core IR
    body: ANFExpr,
}
```

---

# 4. Core IR → ANF Lowering Rules

### Rule A1: Atomic subexpressions

Anything not an atom must be `let`-bound.

Example:

```
Call( Add(x, 1), [y, Mul(z, 3)] )
```

becomes:

```
let t1 = Add(x, 1)
let t2 = Mul(z, 3)
let t3 = Call(t1, [y, t2])
return t3
```

### Rule A2: Branching

```
If(cond, then, else)
```

becomes:

```
let c = cond_atom
If { cond = c,
     then_branch = lower(then),
     else_branch = lower(else)
}
```

### Rule A3: Match

Scrutinee lowered to an atom before the `Match`.

### Rule A4: Loop

Loop body lowered independently into ANF form.

### Rule A5: Lambda

Body becomes its own ANFExpr; closure capture analysis happens later.

---

# 5. IR Stability & Versioning

* Core IR is stable and designed for long-term compatibility.
* ANF IR is a backend layer; structure may evolve but semantics do not change.
* Both IRs are self-hosting-friendly and intended to be implemented in Twinkle itself.

---

# 6. Interpreter Semantics (Core IR)

The reference interpreter operates directly on Core IR.
Semantics are:

* **Call-by-value**, left-to-right evaluation.
* **Lexically scoped** closures.
* **Loop** forms are structural:

  * `Loop { body }` repeatedly evaluates body until `Break`.
* **Match** evaluation:

  * First matching pattern wins.
  * Variant patterns check `type_id` + `variant_id`.

Interpreter behavior defines Twinkle semantics.

Backend (WAT/Wasm) must match interpreter semantics exactly.

---

# 7. Future Extensions

Core IR is designed to support:

* Optimizations (inlining, constant folding),
* Closure conversion before Wasm backend,
* Escape analysis,
* Tail-call detection (optional),
* Lowering to SSA for experimentation.

These are optional and not required for initial self-hosting.

---

# 8. Summary

| Layer       | Purpose                                     | Consumer          |
| ----------- | ------------------------------------------- | ----------------- |
| Typed AST   | Full surface structure, semantic info       | Core IR Lowerer   |
| **Core IR** | Canonical Twinkle semantics (block-based)   | Interpreter + ANF |
| **ANF IR**  | Linearized, backend-friendly representation | WAT/Wasm backend  |
| Backend     | Code for execution (interpret or compile)   | CLI / runtime     |

Core IR is the semantic truth of Twinkle.
ANF IR is the practical truth of code generation.
