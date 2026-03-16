# Boot Type Checker — Implementation Plan

**File:** `boot/compiler/checker.tw`
**Depends on:** `boot/compiler/resolver.tw` (MonoType, ResolvedEnv, FunctionSig, ResolvedTypeDef), `boot/compiler/ast.tw` (Expr, Stmt, Pattern, Block), `boot/lib/source/diagnostic.tw` (StageResult, Diagnostic)
**Tests:** `boot/tests/suites/checker_suite.tw`

---

## Overview

The type checker is the last piece of Phase A (self-hosted frontend). It consumes the resolver's `ResolvedEnv` plus the `Module` AST and produces:

- A `TypeMap` mapping expression span offsets to inferred `MonoType`
- An updated `ResolvedEnv` with inferred return types for unannotated functions
- All diagnostics accumulated without short-circuiting

Implements bidirectional type inference (synth + check modes) with Damas-Milner unification via `MetaVar`. Threads an immutable `InferCtx` record through every call.

**Methodology:** Every milestone uses **red/green TDD** — write failing tests first, then implement until they pass.

---

## MonoType Extension

Add `MetaVar(Int)` and `Never` to the `MonoType` enum in `resolver.tw`. This keeps one canonical type representation. Resolver tests are unaffected (they never construct these variants). Add the two arms to `mono_to_string` in `resolver_suite.tw`.

---

## New Types in `checker.tw`

```tw
type Subst = Dict<Int, MonoType>          // solved MetaVar id -> type
type TypeMap = Dict<Int, MonoType>         // expr span.start -> type

type LocalBinding = .{ name: String, ty: MonoType }

type InferCtx = .{
  env: ResolvedEnv,
  locals: Vector<Vector<LocalBinding>>,   // scoped local bindings
  subst: Subst,
  next_meta: Int,
  current_ret: MonoType?,                 // expected return type
  type_var_scope: Vector<String>,         // generic type params in scope
  type_map: TypeMap,
}

type SynthOut = .{ ty: MonoType, ctx: InferCtx, diags: Vector<Diagnostic> }
type CheckOut = .{ ctx: InferCtx, diags: Vector<Diagnostic> }
type UnifyOut = .{ ctx: InferCtx, diags: Vector<Diagnostic> }

type CheckResult = .{
  type_map: TypeMap,
  env: ResolvedEnv,
  diagnostics: Vector<Diagnostic>,
}
```

---

## Core Engine

- **`fresh_meta(ctx)`** — returns `MetaVar(n)` and updated ctx
- **`zonk(ty, subst)`** — recursively resolve MetaVar chains (pure, reads subst only)
- **`occurs(id, ty, subst)`** — standard occurs-check
- **`unify(a, b, span, ctx, diags)`** — structural unification; `Never` unifies with anything
- **`instantiate(sig, ctx)`** — replace `Var(name)` in a FunctionSig with fresh MetaVars
- **`apply_type_subst(ty, params, args)`** — substitute `Var("T")` with concrete types

---

## Public Entry Point

```tw
pub fn check(module: Module, env: ResolvedEnv) CheckResult
```

**Pass 1:** Walk top-level let statements, bind types into module scope.
**Pass 2:** Walk function bodies via `check_function` — push params, set `current_ret`, check/synth body, zonk type_map entries.
**Final:** Zonk all type_map entries against accumulated subst.

---

## Milestones

### M1 — Scaffolding and Literals

Scaffold `checker.tw` with types, `empty_ctx`, `fresh_meta`, `zonk`, `unify`, `synth` dispatch (only literals), and the top-level `check` function.

**Tests:**
- `fn answer() Int { 42 }` — TypeMap has Int for the literal
- `fn greeting() String { "hello" }` — String
- `fn flag() Bool { true }` — Bool
- `fn ratio() Float { 3.14 }` — Float

### M2 — Unification, Let Bindings, Identifiers

Implement scope management (`push_scope`/`pop_scope`/`bind_local`/`lookup_local`), `instantiate`, ident synthesis, let statement handling, `synth_block`/`check_block`, function body checking.

**Tests:**
- Annotated let: `x: Int = 5`
- Inferred let: `x := 5`
- Ident usage: `fn f(x: Int) Int { x }` resolves x
- Type mismatch: `x: Bool = 5` — diagnostic
- Function call: `fn add(a: Int, b: Int) Int { a }\nfn main() Int { add(1, 2) }`

### M3 — Binary/Unary Operators and Indexing

Arithmetic, comparison, logical, bitwise ops. Index expressions for Vector, String, Dict.

**Tests:**
- `1 + 2` → Int, `1 < 2` → Bool, `true && false` → Bool
- `-5` → Int, `!true` → Bool
- `v[0]` where `v: Vector<Int>` → Int
- Type error: `true + 1` → diagnostic

### M4 — Records, Fields, Variants

Named record constructors, field access, anonymous record literals (check mode), variant literals (check mode).

**Tests:**
- `Point.{ x: 1, y: 2 }` → Named(point_id)
- `p.x` on Point → Int
- `p.z` on Point → diagnostic
- Anonymous `.{ x: 1, y: 2 }` with annotation → ok
- `case c { .Red => true, _ => false }` on Color enum → Bool

### M5 — Generics and Instantiation

Generic function instantiation at call sites. MetaVar solving through unification. Ambiguous type detection.

**Tests:**
- `identity(42)` where `identity<A>` → Int
- `first([1, 2, 3])` where `first<T>` → Int
- Ambiguous: unapplied generic → diagnostic

### M6 — If, Case, Pattern Matching, Exhaustiveness

If expression branch unification. Case scrutinee + arm checking. Pattern variable binding. Exhaustiveness checking for sum types.

**Tests:**
- `if x < 0 { -x } else { x }` → Int
- Case on Shape with Circle/Rect arms → Float, pattern vars bound
- Non-exhaustive case → diagnostic
- Wildcard covers all → no diagnostic

### M7 — Closures, Try, Control Flow

Closure type inference (annotated params, check-mode from expected Function type). `try` desugaring for Option/Result. `return`/`break`/`continue` produce `Never`.

**Tests:**
- `fn(x: Int) Int { x + 1 }` as argument to higher-order function
- `try` on Option in Option-returning function
- `try` on Result in Result-returning function
- `try` in wrong context → diagnostic

### M8 — Arrays, Collect, String Interpolation

Array literal element type inference. Empty array in check mode. Collect comprehension. String interpolation type checking.

**Tests:**
- `[1, 2, 3]` → Vector<Int>
- `[]` with annotation → Vector<Int>
- `collect x in xs { x * 2 }` → Vector<Int>
- `"Hello, ${name}!"` → String
- Non-stringifiable interpolation → diagnostic

### M9 — Integration and Wiring

Register `checker_suite` in main.tw. End-to-end tests chaining parse → resolve → check. Verify partial results on error.

**Tests:**
- Complete program with no errors → empty diagnostics, populated TypeMap
- Program with errors → diagnostics present, TypeMap still partially populated

---

## Key Differences from Stage0

| Stage0 Rust | Self-hosted boot |
|-------------|-----------------|
| `TypeChecker` struct with `&mut self` | `InferCtx` record threaded functionally |
| `errors: Vec<TypeError>` pushed as side effect | `diags: Vector<Diagnostic>` threaded and returned |
| `ExprId` as separate index | `expr.span.start` as TypeMap key |
| `Result<TypedModule, Vec<TypeError>>` | `CheckResult` with partial map always |
| Separate `PatternChecker` struct | Inline `check_pattern` function |

---

## Files to Create or Modify

- **Create:** `boot/compiler/checker.tw`
- **Create:** `boot/tests/suites/checker_suite.tw`
- **Modify:** `boot/compiler/resolver.tw` — add `MetaVar(Int)` and `Never` to MonoType
- **Modify:** `boot/tests/suites/resolver_suite.tw` — add arms to `mono_to_string`
- **Modify:** `boot/tests/main.tw` — register `checker_suite`
