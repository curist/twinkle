# Byte Contextual Int Literal Plan

## Goal

Allow integer literals to satisfy `Byte` expectations in checking contexts, so:

```tw
b: Byte = 10
```

is valid when the literal is in range `0..255`.

This should behave similarly to how expected types flow into anonymous record
literals: context can refine a literal's final type during checking.

## Current State

- Integer literals synthesize as `Int` unconditionally (`Literal::Int -> Int`).
- `check_expr` then unifies `Int` with expected type.
- Therefore `b: Byte = 10` fails with `Expected Byte, Actual Int`.
- The spec currently states: "No implicit narrowing conversion exists from
  `Int` to `Byte`; use `Byte.from_int`."

## Target Semantics

Only integer **literals** are contextually narrowed:

- `b: Byte = 10` ✅
- `fn f(x: Byte) { ... } ; f(10)` ✅
- `bytes: Vector<Byte> = [65, 66, 67]` ✅ (each element checked as `Byte`)
- `b: Byte = 256` ❌ (out of range)
- `b: Byte = -1` ❌ (out of range / not a valid Byte literal)
- `n: Int = 10 ; b: Byte = n` ❌ (non-literal `Int` still not implicitly narrowed)
- `x := 10` remains `Int` (no expected `Byte` context).

## Non-Goals

- General implicit `Int -> Byte` conversion.
- Changing `Byte.from_int` APIs.
- Numeric defaulting changes for unconstrained literals.
- Pattern-literal coercion in `case` arms (can be follow-up).

## Design

### 1. Type checker: contextual literal narrowing

In `check_expr`, add a targeted fast-path for `Literal::Int(n)` when expected
type zonks to `MonoType::Byte`:

- If `0 <= n <= 255`, accept and record expression type as `Byte`.
- Else emit a focused type error (expected `Byte`, literal out of range).

All other paths remain unchanged:

- Synthesis mode still gives `Int`.
- Unconstrained inference remains unchanged.
- Non-literal expressions still require explicit conversion (`Byte.from_int`).

### 2. Core interpreter alignment

The interpreter currently evaluates `CoreExprKind::LitInt` as `Value::Int`
without using expression type. With contextual byte literals, this can produce
runtime mismatches for byte methods.

Update interpreter literal evaluation:

- If `expr.kind == LitInt(n)` and `expr.ty == MonoType::Byte`, produce
  `Value::Byte(n as u8)` (after validated checker range).
- Otherwise keep existing `Value::Int(n)`.

### 3. ANF lowering/local type stability

ANF let-init type tracking currently derives atomic literal type from atom kind
(`ALitInt -> Int`). That loses contextual `Byte` typing for `let b: Byte = 10`.

Adjust let-init typing to preserve Core expression type for contextual literals
(or more generally prefer `value.ty` for `AInit` local result typing).

This keeps local Wasm valtypes consistent (`Byte` as `i32`) and avoids
backend mismatches.

### 4. Diagnostics

Improve error quality for out-of-range byte literals:

- Keep `TypeMismatch` shape, but add note:
  - `"integer literal 300 is out of range for Byte (0..255)"`

This is clearer than a plain `Byte` vs `Int` mismatch in this specific case.

## Implementation Tasks

### Task A: Type checker contextual rule

- Update `src/types/check.rs`:
  - Add special-case in `check_expr` for `Literal::Int` + expected `Byte`.
  - Add range check and focused diagnostic note.
  - Ensure TypeMap records `Byte` for accepted literals.

### Task B: Interpreter runtime value shape

- Update `src/interp/eval.rs`:
  - `LitInt` branch should branch on `expr.ty` and produce `Value::Byte` when
    typed as `Byte`.

### Task C: ANF local typing preservation

- Update `src/ir/lower_anf.rs` let-init typing path to preserve contextual
  literal type for initialized locals (especially `Byte`).

### Task D: Tests (typecheck pass)

Add pass fixtures:

- `b: Byte = 10`
- function call with `Byte` parameter and int literal arg.
- `Vector<Byte>` literal elements using int literals.

### Task E: Tests (typecheck fail)

Add fail fixtures:

- `b: Byte = 256`
- `b: Byte = -1`
- `n: Int = 10 ; b: Byte = n` (still rejected implicit narrowing).

### Task F: Runtime tests

Add run fixture that proves end-to-end behavior:

- contextual byte literal binds and byte methods work (`b.to_int()`,
  `b.to_string()`),
- interpreter and wasm paths both pass.

## Validation

- `cargo test` passes.
- New pass/fail/run fixtures enforce intended boundary.
- No regressions in existing byte/string/index tests.

## Spec Update

Update `docs/spec.md` narrowing rule to carve out this exception:

- Keep "no implicit `Int -> Byte` conversion" for non-literals.
- Add that integer literals may be contextually accepted as `Byte` when an
  expected `Byte` type is present and the value is within `0..255`.

## Exit Criteria

- `b: Byte = 10` is valid.
- Out-of-range literal cases fail with clear diagnostics.
- Non-literal `Int -> Byte` remains disallowed.
- Interpreter + Wasm execute contextual byte literal programs consistently.
