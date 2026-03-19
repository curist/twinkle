# Boot Checker ↔ Stage0 Type Checker Parity Plan

**Status:** Draft  
**Date:** 2026-03-19  
**Scope:** `boot/compiler/checker.tw` parity against stage0 `src/types/check.rs` (+ `src/types/patterns.rs` behavior where relevant)  
**Primary tests:** `boot/tests/suites/checker_suite.tw`, `boot/tests/suites/checker_coverage_suite.tw`

---

## Verified Drift (Current HEAD)

### D1 — Assignment target validation is weaker in boot

- **Boot:** `synth_assign_op` only synthesizes both sides and unifies types (`boot/compiler/checker.tw:1262`)
- **Stage0:** validates assignable targets (`Ident`, `FieldAccess`, `Index`) and rejects invalid rebinding contexts (`src/types/check.rs:2963`)

**Impact:** invalid lvalues can pass further than they should; diagnostics are less precise.

### D2 — Module-qualified calls / method dispatch parity gap

- **Boot:** call handling is direct identifier call or generic function-typed callee (`boot/compiler/checker.tw:478`, `boot/compiler/checker.tw:516`); field synthesis is record-field only (`boot/compiler/checker.tw:596`)
- **Stage0:** has dedicated module-call and method-call paths (`src/types/check.rs:1209`, `src/types/check.rs:1365`, `src/types/check.rs:1647`, `src/types/check.rs:2122`)

**Impact:** `module.func(...)`, `receiver.method(...)`, and first-class method values are not stage0-equivalent.

### D3 — Bitwise operator typing differs

- **Boot:** bitwise binary ops fall into generic arithmetic path (`boot/compiler/checker.tw:1246`, `boot/compiler/checker.tw:1290`); unary bit-not accepts any numeric type and returns operand type (`boot/compiler/checker.tw:1341`)
- **Stage0:** bitwise ops require `Int|Byte` and produce `Int` (`src/types/check.rs:1017`, `src/types/check.rs:1181`)

**Impact:** behavioral mismatch and potential downstream lowering/codegen assumptions drift.

### D4 — `for` / `collect` index binding semantics differ

- **Boot:** index binding is always `Int` (`boot/compiler/checker.tw:1162`, `boot/compiler/checker.tw:1218`), dict iteration yields key only (`boot/compiler/checker.tw:1182`)
- **Stage0:** dict second pattern binds value type; indexed `Iterator<T>` loops/collects are rejected (`src/types/check.rs:3187`, `src/types/check.rs:3168`, `src/types/check.rs:3358`, `src/types/check.rs:3335`)

**Impact:** user-visible type behavior mismatch in loop/comprehension bindings.

### D5 — Top-level checking order differs from stage0

- **Boot:** single source-order walk in `check` (`boot/compiler/checker.tw:1701`)
- **Stage0:** Pass 1 top-level statements/lets, Pass 2 functions (`src/types/check.rs:91`, `src/types/check.rs:165`)

**Impact:** functions depending on later top-level lets can typecheck differently than stage0.

### D6 — String interpolation validation is weaker

- **Boot:** checks only method-name existence via resolver method registry (`boot/compiler/checker.tw:1427`, `boot/compiler/checker.tw:1453`)
- **Stage0:** validates callable `to_string` shape and `String` return type (`src/types/check.rs:1452`)

**Impact:** accepts methods that exist but are not interpolation-compatible.

### D7 — Case scrutinee guard is looser

- **Boot:** `synth_case` relies on pattern/exhaustiveness helpers and allows `.None` variant-set fallback without hard error (`boot/compiler/checker.tw:935`, `boot/compiler/checker.tw:1057`)
- **Stage0:** explicit `CaseScrutineeNotSumType` enforcement (`src/types/check.rs:2776`)

**Impact:** non-sum/non-primitive scrutinee cases can produce weaker or delayed diagnostics.

### D8 — Missing expected-type pushdown branches present in stage0

- **Boot:** `check_expr` has no collect-specific check-mode branch and no contextual `Int`→`Byte` literal narrowing (`boot/compiler/checker.tw:764`)
- **Stage0:** has `check_collect` branch and Byte narrowing (`src/types/check.rs:699`, `src/types/check.rs:849`, `src/types/check.rs:3227`)

**Impact:** inference quality and diagnostics differ in common annotated contexts.

---

## Implementation Plan

### M1 — Assignment Semantics Parity

1. Replace `synth_assign_op` with lvalue-target validation matching stage0 categories.
2. Add explicit diagnostics for unsupported assignment targets.
3. Ensure rebinding rules align with existing `check_let` behavior.

**Files:**
- `boot/compiler/checker.tw`
- `boot/tests/suites/checker_suite.tw`

### M2 — Call/Method Dispatch Parity

1. Add call path split for module-qualified calls and receiver-method calls.
2. Add method value-reference synthesis (`x.method` as function value), where grammar allows.
3. Keep fallback to existing function-type-call synthesis only when callee shape is not module-qualified or method-qualified syntax.
4. For recognized module/method-qualified syntax, do not silently fallback on lookup/arity/type errors; emit module/method-specific diagnostics.

**Files:**
- `boot/compiler/checker.tw`
- `boot/compiler/resolver.tw` (only if additional lookup helpers are required)
- `boot/tests/suites/checker_suite.tw`

### M3 — Bitwise Typing Normalization

1. Add dedicated bitwise binary handling (`&`, `|`, `^`) with `Int|Byte -> Int`.
2. Tighten unary `~` to `Int|Byte -> Int`.
3. Ensure diagnostics match stage0 intent (operand domain errors, not generic arithmetic errors).

**Files:**
- `boot/compiler/checker.tw`
- `boot/tests/suites/checker_suite.tw`

### M4 — Loop/Collect Binding Semantics

1. Extend iterable analysis to distinguish element and secondary binding semantics.
2. For `Dict<K,V>`, bind primary to `K`, secondary to `V`.
3. Reject indexed `Iterator<T>` forms with explicit diagnostics.

**Files:**
- `boot/compiler/checker.tw`
- `boot/tests/suites/checker_suite.tw`
- `boot/tests/suites/checker_coverage_suite.tw`

### M5 — Top-Level Pass Ordering

1. Refactor `check()` into stage0-like two-pass order:
   - Pass 1: top-level statements/lets
   - Pass 2: function bodies
2. Preserve current partial-type-map-on-error behavior.
3. Document and validate expected diagnostic-order shifts from pass reordering (same error class/content, potentially different emission order).

**Files:**
- `boot/compiler/checker.tw`
- `boot/tests/suites/checker_suite.tw`

### M6 — Interpolation Contract Validation

1. Replace boolean `is_interpolatable` shortcut with signature-aware validation:
   - method exists
   - callable shape is receiver-only
   - return type is `String`
2. Keep primitive fast-path acceptance (`Int`, `Float`, `Bool`, `Byte`, `String`).

**Files:**
- `boot/compiler/checker.tw`
- `boot/tests/suites/checker_suite.tw`

### M7 — Case/Check-Mode Pushdown Completeness

1. Add explicit case scrutinee validation (sum or allowed primitive match targets).
2. Add `check_expr` branch for collect expressions when expected is `Vector<T>`.
3. Add contextual `Int` literal narrowing to `Byte` in check mode.

**Files:**
- `boot/compiler/checker.tw`
- `boot/tests/suites/checker_suite.tw`

---

## Suggested Execution Order

1. M5 (top-level pass ordering)
2. M1 (assignment semantics)
3. M3 (bitwise typing)
4. M4 (loop/collect bindings)
5. M7 (case + check-mode pushdown)
6. M6 (interpolation contract)
7. M2 (call/method dispatch parity)

Rationale: complete low-coupling semantic fixes first, then land call/method dispatch last because it has the highest interaction surface.

---

## Exit Criteria

1. All `boot/tests/suites/checker_suite.tw` and `checker_coverage_suite.tw` pass.
2. New parity regression tests added for D1–D8.
3. For each D-item above, either:
   - behavior is matched to stage0, or
   - divergence is explicitly documented as intentional with rationale.
4. Diagnostic parity is verified for touched paths: error kind/message intent matches stage0 behavior (allowing benign wording/order differences where documented).
