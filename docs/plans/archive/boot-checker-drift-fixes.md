# Boot Checker Drift Fixes — Post-Parity Plan

**Status:** Complete
**Date:** 2026-03-19
**Scope:** `boot/compiler/checker.tw` drift against stage0 `src/types/check.rs` discovered after stage0-parity plan (D1–D8) was completed
**Primary tests:** `boot/tests/suites/checker_suite.tw`, `boot/tests/suites/checker_coverage_suite.tw`

---

## Verified Drift (Current HEAD)

### D9 — Range operator returns `Vector<Int>` instead of `Named(RANGE_TYPE_ID)`

- **Boot:** `synth_range_op` returns `MonoType.Vector(MonoType.Int)` (`boot/compiler/checker.tw:1765`)
- **Stage0:** range operator `a..b` produces `MonoType::Named { type_id: RANGE_TYPE_ID }` — the Range type registered as TypeId(3)

**Note:** Boot parses `..` as a `BinOp::Range` binary operator and synthesizes it inline. Stage0 does NOT support `..` syntax at all — users call `range()`, `range_from()`, `range_step()` functions. The `..` operator is a boot-only extension; fixing the return type to `Named(3)` aligns the type semantics even though the syntax path differs.

**Impact:** downstream for-loop/collect iterable checking works accidentally (boot handles Range TypeId 3 correctly in `iterable_binding_info_of`), but the type exposed to the user and stored in the TypeMap is wrong. Any code that inspects or constrains the range type (e.g., passing to a function expecting `Range`) will fail.

### D10 — Byte/Int cross-promotion missing in arithmetic

- **Boot:** `synth_arith_op` unifies `lr.ty` and `rr.ty` directly (`boot/compiler/checker.tw:1829`), which rejects `Byte + Int` or `Int - Byte` as a type mismatch
- **Stage0:** arithmetic operators explicitly handle mixed `Int × Byte` and `Byte × Int`, promoting the result to `Int` (`src/types/check.rs:973-976`); similarly `Byte × Byte → Int` (`src/types/check.rs:973`)

**Impact:** valid programs using `byte_val + 1` or `int_val - byte_val` produce type errors in boot.

### D11 — Shift operators (`Shl`, `Shr`) not in boot AST or checker

- **Boot:** `BinOp` enum has no `Shl`/`Shr` variants (`boot/compiler/ast.tw:232-251`); parser does not recognize `<<`/`>>`
- **Stage0:** `Shl`/`Shr` are parsed and type-checked as bitwise ops (`Int|Byte → Int`) (`src/types/check.rs:1018`)

**Impact:** programs using shift operators fail at parse time in boot.

### D12 — `CollectWhile` expression not in boot AST or checker

- **Boot:** `ExprKind` has no `CollectWhile` variant (`boot/compiler/ast.tw:169-191`)
- **Stage0:** `ExprKind::CollectWhile { cond, body }` is parsed and type-checked (`src/types/check.rs:617-619`, `src/types/check.rs:3403-3412`)

**Note:** Boot already handles CollectWhile semantics via an optional `condition` field in `CollectExpr` (lines 193-199 in ast.tw), and the checker handles condition in both `synth_collect` and `check_collect`. The difference is AST design (boot uses one variant with optional fields; stage0 uses a separate `CollectWhile` variant), not missing functionality. May not need a fix unless exact AST parity is required.

**Impact:** If boot's parser correctly routes `collect cond { body }` into `CollectExpr` with `condition` set, this works. Verify parser behavior before deciding on fix scope.

### D13 — `call_expected_ret` pre-unification missing

- **Boot:** generic function calls instantiate type parameters but do not pre-solve the return type MetaVar from calling context
- **Stage0:** `call_expected_ret` field captures expected return type and pre-unifies it with the instantiated return type before checking arguments (`src/types/check.rs:914-916`, `src/types/check.rs:1213-1228`)

**Impact:** inference quality degrades for generic calls inside annotated bindings or check-mode positions. MetaVars that stage0 solves eagerly remain unsolved until final zonk, potentially causing spurious AmbiguousType errors.

### D14 — Directional equality (`try_eq_directional`) missing

- **Boot:** `synth_eq_op` synthesizes both sides and unifies (`boot/compiler/checker.tw:1853-1857`)
- **Stage0:** `==`/`!=` first synth left, then try `check_expr(right, left_ty)` with rollback, falling back to synth-both if check fails (`src/types/check.rs:1057-1065`, `src/types/check.rs:1118-1149`)

**Impact:** equality comparisons involving anonymous record literals or variant literals on the right-hand side fail where stage0 succeeds (stage0 pushes the left type into the right expression via check mode).

### D15 — Defer body rejects `Never`-typed expressions in stage0

- **Boot:** `Defer` handler just synthesizes the expression with no additional validation (`boot/compiler/checker.tw:2183-2186`)
- **Stage0:** rejects `Never`-typed defer body (return/break/continue/error in defer) with an explicit diagnostic in **three** places: `check_stmt` Defer arm, `check_top_level_stmt` Defer arm, and synth-mode Defer handling (`src/types/check.rs:398-407`, `1814-1826`, `1911-1923`)

**Impact:** boot silently accepts defer bodies that diverge (e.g., `defer error("cleanup")`), which has undefined semantics at scope exit.

### D16 — `canonicalize_record_constructor` missing

- **Boot:** named record constructor looks up type directly by name
- **Stage0:** `canonicalize_record_constructor` follows type aliases to find the underlying record type (`src/types/check.rs:2354`, `src/types/check.rs:2477`)

**Impact:** `Alias.{ ... }` where `Alias` is a type alias for a record type fails in boot.

---

## Implementation Plan

### M1 — Range Operator Type Fix

1. Change `synth_range_op` to return `MonoType.Named(3, [])` (Range TypeId) instead of `MonoType.Vector(MonoType.Int)`.
2. Verify `iterable_binding_info_of` still works (it already handles TypeId 3 correctly).
3. Add test: `1..10` produces Range type, not `Vector<Int>`.

**Files:**
- `boot/compiler/checker.tw`
- `boot/tests/suites/checker_suite.tw`

### M2 — Byte/Int Arithmetic Cross-Promotion

1. Replace the simple `unify(lr.ty, rr.ty)` in `synth_arith_op` with explicit match on zonked operand types:
   - `Int × Int → Int`
   - `Float × Float → Float`
   - `Byte × Byte → Int`
   - `Int × Byte` / `Byte × Int` → `Int`
   - `MetaVar × concrete` → solve MetaVar, follow same rules
   - `String × String` → `String` (Add only)
2. Apply similar cross-promotion logic to `synth_cmp_op` (comparison allows `Byte` vs `Int`).
3. Apply to `synth_bitwise_op` (already returns `Int`, but should accept mixed `Int`/`Byte` pairs).

**Files:**
- `boot/compiler/checker.tw`
- `boot/tests/suites/checker_suite.tw`

### M3 — Shift Operators (`Shl`, `Shr`)

1. Add `Shl` and `Shr` to `BinOp` in `boot/compiler/ast.tw`.
2. Parse `<<` and `>>` tokens in `boot/compiler/parser.tw`.
3. Handle `Shl`/`Shr` in checker's binary dispatch — same semantics as bitwise ops (`Int|Byte → Int`).

**Files:**
- `boot/compiler/ast.tw`
- `boot/compiler/parser.tw`
- `boot/compiler/checker.tw`
- `boot/tests/suites/checker_suite.tw`

### M4 — `CollectWhile` Expression

Boot already models this via optional `condition` field in `CollectExpr`. Verify:
1. Check that the parser routes `collect cond { body }` (no `in`) into `CollectExpr` with `condition` set and `iter` as None.
2. Check that `synth_collect` / `check_collect` handle condition-only correctly.
3. If parser doesn't route correctly, fix the parser path. No AST variant change needed.
4. Add test confirming `collect while_cond { body }` type-checks correctly.

**Files:**
- `boot/compiler/ast.tw`
- `boot/compiler/parser.tw`
- `boot/compiler/checker.tw`
- `boot/tests/suites/checker_suite.tw`

### M5 — Defer Never Rejection

1. In `check_stmt`'s `Defer` arm, after synthesizing the expression, check if result type is `Never`.
2. If so, emit diagnostic: "defer body cannot diverge (return, break, continue, or error(...))".
3. Same check in `check_top_level_stmt`'s `Defer` arm.
4. Same check in synth-mode `Defer` handling (stage0 checks in three places total).

**Files:**
- `boot/compiler/checker.tw`
- `boot/tests/suites/checker_suite.tw`

### M6 — Directional Equality

1. Change `synth_eq_op` to:
   - Synth left side to get `left_ty`.
   - Try `check_expr(right, left_ty)` — if it succeeds without new errors, use that result.
   - If check fails or produces errors, fall back to synth-both-and-unify.
2. This requires a way to snapshot and restore `InferCtx` (since it's threaded functionally, just keep the old copy as fallback).

**Files:**
- `boot/compiler/checker.tw`
- `boot/tests/suites/checker_suite.tw`

### M7 — `call_expected_ret` Pre-Unification

1. Add `call_expected_ret: MonoType?` field to `InferCtx`.
2. In `check_expr`, when expected type flows into a `Call` expression, set `call_expected_ret` before dispatching to `synth_call`.
3. In `synth_call`, after instantiating a generic function, if `call_expected_ret` is set, unify it with the instantiated return type before checking arguments.
4. Clear `call_expected_ret` after use.

**Files:**
- `boot/compiler/checker.tw`
- `boot/tests/suites/checker_suite.tw`

### M8 — Type Alias Record Constructor (`canonicalize_record_constructor`)

1. When looking up a named record constructor (`TypeName.{ ... }`), follow type alias chains to find the underlying record TypeId.
2. Use the canonical TypeId for field lookup and construction, but return the alias type to the user.
3. Report error if alias target is not a record type.

**Files:**
- `boot/compiler/checker.tw`
- `boot/tests/suites/checker_suite.tw`

---

## Suggested Execution Order

1. **M1** (range type fix) — ✅ Done. One-line fix in `synth_range_op`.
2. **M2** (Byte/Int promotion) — ✅ Done. Rewrote `synth_arith_op` with explicit type matching; added Byte/Int skip in `synth_cmp_op`.
3. **M5** (defer Never rejection) — ✅ Done. Added Never check in both `check_stmt` and `check_top_level_stmt` Defer arms.
4. **M3** (shift operators) — ✅ Done. Added `Shl`/`Shr` to BinOp, `LtLt` token for `<<`, adjacent-Gt parser detection for `>>` (avoids `>>` generic ambiguity).
5. **M4** (CollectWhile) — ✅ Already worked. Boot's `CollectExpr` with optional condition handles this correctly.
6. **M6** (directional equality) — ✅ Done. Added `try_eq_directional` with speculative synth+check and diagnostic-count rollback.
7. **M8** (type alias record constructor) — ✅ Already worked. Boot's resolver follows aliases correctly.
8. **M7** (call_expected_ret) — ✅ Done. Added `call_expected_ret` field to `InferCtx`, wired through `check_expr` → `synth_call`.

Rationale: fix correctness bugs first (M1, M2, M5), add missing syntax next (M3, M4), then improve inference quality (M6, M7, M8).

---

## Exit Criteria

1. All `boot/tests/suites/checker_suite.tw` and `checker_coverage_suite.tw` pass.
2. New regression tests added for D9–D16.
3. For each D-item above, either:
   - behavior matches stage0, or
   - divergence is explicitly documented as intentional with rationale.
4. Byte/Int arithmetic cross-promotion produces same types as stage0 for all combinations.
5. Range expressions produce `Named(3)` type, not `Vector<Int>`.
