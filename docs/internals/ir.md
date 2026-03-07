# Intermediate Representations

This document defines the intermediate representations used by the Twinkle
compiler. The IR pipeline is:

```
Typed AST
   → Core IR        (semantic, block-structured)
   → ANF IR         (backend-oriented, linearized)
   → { Interpreter | WAT/Wasm }
```

Both IRs are stable, explicit, and designed for self-hosting.

---

## Overview

Twinkle uses two IR layers:

**Core IR** — High-level but fully desugared. Encodes Twinkle semantics
directly. Block-structured, expression-oriented. Suitable for interpretation
and high-level optimizations.

**ANF IR (Administrative Normal Form)** — Lower-level, linearized evaluation
order. Every intermediate value is bound to a `let`. Designed for straightforward
code generation (WAT/Wasm). Optional for interpretation, but ideal for backends.

All surface features (`collect`, `for x in`, `try`, `.Variant`, implicit returns,
block expressions) are removed or made explicit during lowering to Core IR.

---

## Core IR

### Goals

Core IR must:

* Preserve Twinkle's evaluation semantics faithfully.
* Be simple: no operator precedence, no sugar.
* Be explicit: no hidden control flow, no implicit returns.
* Be stable enough that interpreter and backend use the same semantics.
* Serve as the canonical truth of the language.

### Modules

A module is a list of function definitions plus a synthetic `__init__` function
that contains all top-level value bindings and expression statements in source
order. There is no special `main` function (spec §8.1). When compiling to Wasm,
`__init__` becomes the Wasm start function.

### Local Identifiers

Core IR uses integer-based locals (`LocalId`) instead of names. Each function
has its own local id space.

### Expression Nodes

All Core IR nodes are expressions and produce a value (including void):

```
Expr =
    | LitInt(i64)
    | LitFloat(f64)
    | LitBool(bool)
    | LitStr(String)
    | LitVoid

    | Local(LocalId)

    | Let { local: LocalId, value: Box<Expr>, body: Box<Expr> }

    // Mutation: updates an existing local's value (no new binding).
    // Used for rebinding and loop index/accumulator updates.
    | Assign { local: LocalId, value: Box<Expr> }

    | BinOp { op: BinOp, left: Box<Expr>, right: Box<Expr> }
    | UnOp  { op: UnOp,  expr: Box<Expr> }

    | Call { callee: Box<Expr>, args: Vec<Expr> }

    // Lambda body hoisted to a FunctionDef; free_vars snapshotted at creation.
    | MakeClosure { func_id: FuncId, free_vars: Vec<LocalId> }

    | If { cond: Box<Expr>, then_branch: Box<Expr>, else_branch: Box<Expr> }
    | Match { scrutinee: Box<Expr>, arms: Vec<MatchArm> }

    | Loop { body: Box<Expr> }
    | Break { value: Option<Box<Expr>> }
    | Continue

    | Return { value: Option<Box<Expr>> }

    | Record { type_id: TypeId, fields: Vec<(FieldId, Expr)> }
    | RecordGet { target: Box<Expr>, field: FieldId }
    | RecordUpdate { base: Box<Expr>, field: FieldId, value: Box<Expr> }

    | Variant { type_id: TypeId, variant: VariantId, args: Vec<Expr> }

    | ArrayLit(Vec<Expr>)
    | Index { base: Box<Expr>, index: Box<Expr> }
```

### Patterns

Patterns are fully resolved (no identifier ambiguity):

```
Pattern =
    | PatWildcard
    | PatVar(LocalId)
    | PatLitInt(i64)
    | PatLitBool(bool)
    | PatLitStr(String)
    | PatVariant { type_id: TypeId, variant: VariantId, fields: Vec<Pattern> }
```

### Prelude FuncIds

Prelude functions occupy a fixed block of FuncIds starting at 1. The canonical
list lives in `src/ir/lower.rs` (the `prelude` module constants) and
`src/interp/eval.rs` (`call_builtin`). `USER_FUNC_START` marks the first FuncId
for user-defined functions; it advances as new builtins are added.

Categories: core I/O, string conversions, string operations, array operations,
dict operations, cell operations, range constructors.

Inherent method calls (`x.method(args)`) lower to
`Call { callee: GlobalFunc(func_id), args: [receiver, ...rest] }`.
There is no string-based dispatch in Core IR.

### Invariants

1. No surface syntax remains (`for x in`, `collect`, `.Variant` shorthand,
   implicit returns, block statements).
2. `If` always has both branches.
3. `Match` arms are exhaustive or include `_`.
4. `Loop { body }` — `Break` may or may not carry a value.
5. All locals are pre-numbered and unique in function scope.
6. All type information is known (type_id, variant_id, field_id attached).

### Surface-to-Core Lowering Rules

**Blocks → nested `Let`:**
`{ a := e1; b := e2; a + b }` → `Let(a, e1, Let(b, e2, Add(a, b)))`

**Implicit return eliminated:**
The last expression in a function body becomes the body expression directly.

**`try expr` → match over Result:**
`y := try foo(x)` → `Match foo(x) { Ok(v) → Let(y, v, body...), Err(e) → Return(Err(e)) }`

**`collect` → loop + array builder:**
Lowered to `Loop` with `array_builder_push` + `Break(array_builder_freeze(...))`.

**`for x in xs` → explicit iteration:**
Type-directed lowering depending on collection type (array/range/dict/iterator).

**`.Variant(args)` → explicit variant construction:**
`.Ok(42)` → `Variant { type_id: Result, variant: Ok, args: [LitInt(42)] }`

**Record literals → explicit construction:**
`Point.{ x: 1, y: 2 }.x` → `RecordGet(Record(Point, [(x,1),(y,2)]), x)`

---

## ANF IR

ANF is the backend-oriented representation where evaluation order is explicit
and all intermediate values are bound to locals.

### Atoms

```
Atom =
    | ALocal(LocalId)
    | ALitInt(i64)
    | ALitFloat(f64)
    | ALitBool(bool)
    | ALitStr(String)
    | ALitVoid
    | AGlobalFunc(FuncId)    // named function as first-class value
```

### Expressions

```
ANFExpr =
    | Let { local: LocalId, op: AnfOp, body: ANFExpr }
    | Return(Option<Atom>)       // terminal
    | Break(Option<Atom>)        // terminal
    | Continue                   // terminal
```

`Return`, `Break`, and `Continue` are terminals — code after them is unreachable.

### Operations

```
AnfOp =
    | AInit { value: Atom }
    | ABinOp { op: BinOp, left: Atom, right: Atom }
    | AUnOp  { op: UnOp,  expr: Atom }

    | ACall { callee: Atom, args: Vec<Atom> }

    | AIf    { cond: Atom, then_branch: ANFExpr, else_branch: ANFExpr }
    | AMatch { scrutinee: Atom, arms: Vec<ANFMatchArm> }
    | ALoop  { body: ANFExpr }

    | AAssign { local: LocalId, value: Atom }

    | ARecord       { type_id: TypeId, fields: Vec<(FieldId, Atom)> }
    | ARecordGet    { target: Atom, field: FieldId }
    | ARecordUpdate { base: Atom, field: FieldId, value: Atom, can_reuse_in_place: bool }

    | AVariant  { type_id: TypeId, variant: VariantId, args: Vec<Atom> }

    | AArrayLit(Vec<Atom>)
    | AIndex { base: Atom, index: Atom }

    | AMakeClosure { func_id: FuncId, free_vars: Vec<LocalId> }

    | ADefer(Box<ANFExpr>)   // eliminated before WAT backend
```

### Core → ANF Lowering Rules

**A1 — Atomic subexpressions:** Anything not an atom must be `let`-bound.
`Call(Add(x,1), [y, Mul(z,3)])` → `let t1=Add(x,1); let t2=Mul(z,3); let t3=Call(t1,[y,t2])`

**A2 — Branching:** Condition lowered to an atom before `If`.

**A3 — Match:** Scrutinee lowered to an atom before `Match`.

**A4 — Loop:** Body lowered independently into ANF form.

**A5 — MakeClosure:** Already atomic (only references locals and a FuncId).
The hoisted lambda body is lowered to ANF independently.

---

## Interpreter Semantics

The reference interpreter operates directly on Core IR:

* **Entry point**: call the `__init__` function (synthetic top-level init).
* **Call-by-value**, left-to-right evaluation.
* **Lexically scoped closures**: `MakeClosure` snapshots listed locals into a
  `Closure(func_id, captured_env)` value. Calling merges `captured_env` into
  the new frame, then binds parameters.
* **Control flow signals** bubble as Rust enum variants (not panics):
  `Break`, `Continue`, `Return` — caught at appropriate boundaries.
* **Environment**: one flat `HashMap<LocalId, Value>` per call frame.
  `Let` inserts, `Assign` overwrites.
* **Loop**: `Loop { body }` repeatedly evaluates body until `Break`.
* **Match**: first matching pattern wins. Variant patterns check type_id + variant_id.
* **Dict**: `Vec<(Value, Value)>` in the interpreter (linear scan). Key equality
  uses structural `==`. Long-term, K is restricted to `Int` or `String`.

Interpreter behavior defines Twinkle semantics. The WAT/Wasm backend must match
exactly.

---

## Stability

* Core IR is stable and designed for long-term compatibility.
* ANF IR is a backend layer; structure may evolve but semantics do not change.
* Both IRs are self-hosting-friendly.

---

## Summary

| Layer       | Purpose                                   | Consumer          |
|-------------|-------------------------------------------|-------------------|
| Typed AST   | Full surface structure, semantic info      | Core IR Lowerer   |
| **Core IR** | Canonical Twinkle semantics (block-based)  | Interpreter + ANF |
| **ANF IR**  | Linearized, backend-friendly               | WAT/Wasm backend  |
| Backend     | Code for execution (interpret or compile)  | CLI / runtime     |
