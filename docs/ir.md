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

A module is a list of **function definitions** plus a synthetic **`__init__` function**
that contains all top-level value bindings and expression statements in source order.

Per spec §8.1, there is **no special `main` function**. The entry point of a program is
the `__init__` function. `fn main()` has no distinguished status and is not called
automatically. When compiling to Wasm, `__init__` becomes the Wasm start function.

```
Module = { FunctionDef }   // includes the synthetic __init__ function
```

### Values

Core IR does not define a runtime; but the interpreter uses:

* integers, floats, bools, strings,
* arrays, dicts,
* records (nominal),
* variants (nominal),
* closures — `Closure(FuncId, HashMap<LocalId, Value>)`:
  * `FuncId` points to the hoisted lambda body (a regular `FunctionDef`).
  * `HashMap<LocalId, Value>` is a snapshot of the free variables captured
    at closure-creation time (capture-by-value, spec §7.7).
  * When calling a closure, the captured env is merged into the call frame
    before binding the parameters.
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

    // Mutation: updates an existing local's value (no new binding introduced).
    // Used for `x = expr` rebinding and loop index/accumulator updates.
    // Maps to Wasm local.set.
    | Assign { local: LocalId, value: Box<Expr> }

    | BinOp { op: BinOp, left: Box<Expr>, right: Box<Expr> }
    | UnOp  { op: UnOp,  expr: Box<Expr> }

    | Call { callee: Box<Expr>, args: Vec<Expr> }

    // Closure creation: lambda body is hoisted to a FunctionDef with func_id.
    // free_vars lists the LocalIds from the enclosing scope that the closure captures.
    // At runtime, their current values are snapshotted into the Closure value.
    | MakeClosure { func_id: FuncId, free_vars: Vec<LocalId> }

    | If { cond: Box<Expr>, then_branch: Box<Expr>, else_branch: Box<Expr> }

    | Match { scrutinee: Box<Expr>, arms: Vec<MatchArm> }

    | Loop { body: Box<Expr> }
    | Break { value: Option<Box<Expr>> }
    | Continue

    // Early exit from a function; bubbles up to the nearest call boundary.
    // Return { value: None } is used for Void returns.
    | Return { value: Option<Box<Expr>> }

    | Record { type_id: TypeId, fields: Vec<(FieldId, Expr)> }
    | RecordGet { target: Box<Expr>, field: FieldId }

    // Functional record update: produces a new record with one field replaced.
    // No in-place mutation; a future optimization pass may lower to struct.set.
    | RecordUpdate { base: Box<Expr>, field: FieldId, value: Box<Expr> }

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

## 2.4 Prelude FuncId Assignments

Prelude functions occupy a fixed block of FuncIds starting at 1. The canonical,
up-to-date list lives in `src/ir/lower.rs` (the `prelude` module constants) and
`src/interp/eval.rs` (`call_builtin`). The `USER_FUNC_START` constant in `lower.rs`
marks the first FuncId available for user-defined functions; it advances as new
builtins are added.

Prelude functions cover (in order of FuncId):

* Core I/O: `print`, `println`, `error`
* String conversions: `String.of_int`, `String.of_float`, `String.of_bool`, `String.to_string`
* String operations: `String.len`, `String.concat`, `String.substring`
* Array operations: `Array.len`, `Array.append`, `Array.set`, `Array.concat`, `Array.slice`
* Dict operations: `Dict.set`, `Dict.keys`, `Dict.get`, `Dict.new`, `Dict.has`, `Dict.remove`, `Dict.len`
* Cell operations: `Cell.new`, `Cell.get`, `Cell.set`, `Cell.update`
* Range constructors: `range`, `range_from`, `range_step`

New builtins slot in before `USER_FUNC_START`; adding one requires bumping the constant
and updating `call_builtin`. Do not hardcode numeric FuncIds in documentation or tests —
refer to the named constants in `lower.rs` instead.

Inherent method calls (`x.method(args)`) are **not** a distinct IR node.
They lower to `Call { callee: GlobalFunc(func_id), args: [receiver, ...rest] }`.
There is no string-based dispatch in Core IR.

---

## 2.5 Core IR Invariants

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

    | AMakeClosure { func_id: FuncId, free_vars: Vec<Atom> }
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

### Rule A5: MakeClosure

`MakeClosure` is already atomic (it only references locals and a FuncId).
The hoisted lambda body (`FunctionDef`) is lowered to ANF independently.

---

# 5. IR Stability & Versioning

* Core IR is stable and designed for long-term compatibility.
* ANF IR is a backend layer; structure may evolve but semantics do not change.
* Both IRs are self-hosting-friendly and intended to be implemented in Twinkle itself.

---

# 6. Interpreter Semantics (Core IR)

The reference interpreter operates directly on Core IR.
Semantics are:

* **Entry point**: call the `__init__` function (the synthetic top-level init).
  There is no special `main` function (spec §8.1).

* **Call-by-value**, left-to-right evaluation.

* **Lexically scoped** closures.
  * `MakeClosure { func_id, free_vars }` snapshots the listed locals from the
    current frame into a `Closure(func_id, captured_env)` value.
  * Calling a `Closure` merges `captured_env` into the new call frame first,
    then binds the parameters.

* **Control flow signals** bubble up through the expression tree as Rust enum
  variants (not panics):
  * `Break(Option<Value>)` — caught by the enclosing `Loop`.
  * `Continue` — caught by the enclosing `Loop`.
  * `Return(Option<Value>)` — caught at the function call boundary (the point
    where `Call` is evaluated); escapes any nested loops.

* **Environment**:
  * One flat `HashMap<LocalId, Value>` per call frame.
  * `Let { local, value, body }` — evaluate `value`, insert `local → result`
    into env, then evaluate `body`.
  * `Assign { local, value }` — evaluate `value`, overwrite `local` in env,
    return `Void`. (Maps to Wasm `local.set`.)

* **Loop** forms are structural:
  * `Loop { body }` repeatedly evaluates body until `Break`.

* **Match** evaluation:
  * First matching pattern wins.
  * Variant patterns check `type_id` + `variant_id`.

* **Dict** runtime representation: `Vec<(Value, Value)>` in Stage 5
  (linear scan, no hashing needed). Key equality uses structural `==`.
  Note: `Dict<K,V>` currently has no compile-time constraint on K.
  Restricting K to `Int` or `String` is the intended long-term policy; the
  type-checker will enforce this in a later stage. `Bool` keys are excluded
  (a two-entry dict is just a pair).

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
