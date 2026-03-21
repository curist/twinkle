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

type InferCtx = .{
  env: ResolvedEnv,
  locals: Vector<Dict<String, MonoType>>, // scoped frames; lookup walks back from last
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
- **`unify(a, b, span, ctx, diags)`** — structural unification; `Never` unifies with anything (but see Never-handling note in M6)
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

Scope operations use `Dict<String, MonoType>` frames:
- `push_scope(ctx)` — push an empty `Dict.new()` frame
- `pop_scope(ctx)` — drop the last frame
- `bind_local(ctx, name, ty)` — insert into the topmost frame
- `lookup_local(ctx, name)` — walk frames back-to-front, first hit wins (shadowing)

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

### M4 — Records, Fields, Method Calls, Variants

Named record constructors, field access, anonymous record literals (check mode), variant literals (check mode). Method call desugaring: `x.method(args)` is resolved by the resolver to a global function call — the checker types it as `Call(GlobalFunc, [x, ...args])`, but must verify the first parameter type matches the receiver.

**Tests:**
- `Point.{ x: 1, y: 2 }` → Named(point_id)
- `p.x` on Point → Int
- `p.z` on Point → diagnostic
- Anonymous `.{ x: 1, y: 2 }` with annotation → ok
- `case c { .Red => true, _ => false }` on Color enum → Bool
- Method call: `v.push(1)` where `v: Vector<Int>` → Vector<Int>

### M5 — Generics and Instantiation

Generic function instantiation at call sites. MetaVar solving through unification. Ambiguous type detection.

**Tests:**
- `identity(42)` where `identity<A>` → Int
- `first([1, 2, 3])` where `first<T>` → Int
- Ambiguous: unapplied generic → diagnostic

### M6 — If, Case, For Loops, Pattern Matching, Exhaustiveness

If expression branch unification. Case scrutinee + arm checking. Pattern variable binding. For loops (condition form, iterator form with optional index binding). Exhaustiveness checking for sum types.

**Never in case expressions:** When unifying arm types, skip `Never`-typed arms (diverging arms like `error(...)` or `return`) — the result type is the join of non-diverging arms only. If all arms diverge, the case is `Never`. This mirrors the fix from stage0 (commit `eaf3027`).

**Exhaustiveness:** Flat variant coverage check — collect variant names from the scrutinee's sum type, subtract names covered by `Variant` patterns, report missing ones. `Wildcard` and `Ident` patterns cover all remaining. Nested pattern exhaustiveness is deferred.

**Tests:**
- `if x < 0 { -x } else { x }` → Int
- Case on Shape with Circle/Rect arms → Float, pattern vars bound
- Non-exhaustive case → diagnostic
- Wildcard covers all → no diagnostic
- Case with diverging arm: `case o { .None => error(""), .Some(x) => x }` → Int, not Never
- Case where all arms diverge → Never
- `for x < 10 { ... }` → Void
- `for item in items { ... }` → Void, `item` bound to element type
- `for item, i in items { ... }` → Void, `i` bound to Int

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

**Builtin environment for tests:** Currently `check_src` resolves against `resolver.empty_env()`, which has no builtin functions registered. This means tests can't use `range`, `error`, `println`, or any other builtin — only user-defined functions within the test source. M9 should add a `test_env()` helper that registers builtin function signatures (at minimum: `range`, `range_from`, `error`, `println`, `to_string`) so tests can exercise realistic programs. Once available, revisit earlier milestone tests:
- M6: add `case o { .None => error(""), .Some(x) => x }` test for diverging arms with `error`
- M6: add exhaustive variant coverage tests without wildcard (`.Some`/`.None` covering `Option`)
- M8: test `collect i in range(n) { ... }` with real `range`

**Tests:**
- Complete program with no errors → empty diagnostics, populated TypeMap
- Program with errors → diagnostics present, TypeMap still partially populated

**Status:** M1–M9 implemented. Exhaustiveness checker upgraded to track covered variants. Call arguments now use check mode (enabling anonymous record/variant literals as args). `test_env()` with builtins added.

**Remaining gap — string interpolation validation:** The checker has `is_interpolatable` which rejects obviously wrong types (functions, vectors, dicts), but cannot validate that a `Named` type has `to_string` because the resolver has no method registry. This is blocked on [boot-resolver-method-registry.md](boot-resolver-method-registry.md). String interpolation tests are also blocked by a stage0 type leak bug that prevents constructing source strings containing `${}` in test code.

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
