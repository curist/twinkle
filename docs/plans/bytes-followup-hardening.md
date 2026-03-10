# Byte Semantics Follow-up Hardening

**Status:** In progress (Phase 1 complete, Phase 2 active)  
**Last updated:** 2026-03-10

## Goal

Consolidate and harden the byte-first string/`Byte` work so behavior is stable across
interpreter and Wasm backends, while reducing long-term maintenance risk in type inference
and codegen (especially `Iterator.unfold` callback flows).

This plan is a follow-up to [string-unicode-semantics.md](./string-unicode-semantics.md).

---

## Why this plan exists

The branch landed major semantics successfully, but we now have follow-up risks:

1. **Index-width truncation in some byte APIs**  
   Some Wasm paths still narrow `Int` (`i64`) to `i32` before bounds checks.
2. **Intrinsic typing/ABI policy is duplicated**  
   Typechecker/lowering/codegen/intrinsics each encode parts of the same contract.
3. **`Byte` arithmetic promotion is not fully implemented**  
   The plan says `Byte` arithmetic promotes to `Int`, but current typing/lowering paths
   still assume old numeric operator buckets.
4. **`Iterator.unfold` callback handling uses fallback recovery**  
   The current `resolve_unfold_step_types` fallback is pragmatic and correct, but points to
   missing unified op-result typing metadata.
5. **Prelude-ID migration is ad hoc**  
   Compatibility around removed/replaced prelude IDs is handled by local exceptions.

---

## Current confirmed gaps

### A. Remaining i64->i32 wraparound hazards

`String.slice` and `String.from_code_point` are now fixed, but similar risk remains in:

* string indexing (`s[i]`) lowering
* `String.get`
* `String.char_code_at`

Target files:

* [src/codegen/emit.rs](../../src/codegen/emit.rs)
* [src/interp/eval.rs](../../src/interp/eval.rs) (oracle behavior reference)
* [tests/run](../../tests/run)

### B. Distributed intrinsic contracts

The same intrinsic contract is spread across:

* typechecker builtin call logic
* prelude function tables / IDs
* codegen intrinsic return-valtype inference
* interpreter intrinsic behavior

Target files:

* [src/types/check.rs](../../src/types/check.rs)
* [src/types/env.rs](../../src/types/env.rs)
* [src/ir/lower.rs](../../src/ir/lower.rs)
* [src/codegen/ctx.rs](../../src/codegen/ctx.rs)
* [src/codegen/prelude.rs](../../src/codegen/prelude.rs)
* [src/codegen/emit.rs](../../src/codegen/emit.rs)
* [src/interp/eval.rs](../../src/interp/eval.rs)

### C. `Byte` arithmetic promotion mismatch

Language direction says:

* `Byte + Byte -> Int`
* `Byte + Int -> Int`, etc.

Current operator typing and ANF op-kind mapping still center on `Int|Float|Bool|String`.

Target files:

* [src/types/check.rs](../../src/types/check.rs)
* [src/ir/anf.rs](../../src/ir/anf.rs)
* [src/ir/lower_anf.rs](../../src/ir/lower_anf.rs)
* [src/codegen/emit.rs](../../src/codegen/emit.rs)
* [src/interp/eval.rs](../../src/interp/eval.rs)

### D. `Iterator.unfold` callback inference resilience

The fallback in `resolve_unfold_step_types` fixes real cases, but relies on current-function
return-type context when direct atom inference fails.

This is acceptable short-term, but long-term should be replaced with explicit op result typing
metadata so typed-unfold specialization does not depend on contextual recovery.

Target files:

* [src/codegen/ctx.rs](../../src/codegen/ctx.rs)
* [src/codegen/emit.rs](../../src/codegen/emit.rs)
* [src/ir/lower_anf.rs](../../src/ir/lower_anf.rs)

### E. Prelude-ID compatibility policy

`String.substring` -> `String.slice` currently uses fixed ID compatibility handling in tests
and lookup tables. We should define a clear policy for deprecations and reserved IDs.

Target files:

* [src/ir/lower.rs](../../src/ir/lower.rs)
* [src/module/context.rs](../../src/module/context.rs)
* [src/codegen/prelude.rs](../../src/codegen/prelude.rs)

---

## Phased execution

### Phase 1: Correctness hardening for all byte index/code-point APIs

**Work**

* Audit all string byte-index and code-point intrinsics for i64-range correctness.
* Add shared codegen helper(s) for index normalization/checks before narrowing to i32.
* Add regression fixtures for each API with large-index/large-int inputs.

**Exit criteria**

* Interpreter/Wasm parity holds for all added edge-case fixtures.
* Differential tests cover these cases.

### Phase 2: Central intrinsic contract registry (single source of truth)

**Work**

* Introduce a central intrinsic spec table describing:
  * Twinkle name
  * parameter/return `MonoType` contract
  * ABI-level valtypes where needed
  * runtime vs intrinsic backend dispatch
* Migrate consumers incrementally (typecheck, lowering, codegen result inference).

**Progress notes (2026-03-10)**

* Phase 1 completed: string indexing, `String.get`, `String.char_code_at`, and
  `String.from_char_code` now guard in i64-domain before i32 narrowing; large-index
  and large-int regressions are covered in run/wasm/differential fixtures.
* Kickoff landed: introduced a shared intrinsic contract module and began wiring
  type-env builtin signatures and codegen intrinsic result typing to consume it.
* Phase 2 expanded: generic/container intrinsic contracts are now represented in
  the same registry; emit-time intrinsic result typing no longer needs a generic
  compatibility fallback table.
* Interpreter builtin dispatch now consumes the shared contract registry for
  covered intrinsic arity checks.

**Exit criteria**

* No new intrinsic requires editing multiple disconnected signature sources.
* Existing intrinsic tests still pass unchanged.

### Phase 3: Complete `Byte` arithmetic promotion

**Work**

* Extend binary operator typing rules to include `Byte` promotion matrix.
* Update ANF operand-kind modeling (or add explicit promotion lowering prior to ANF binop emit).
* Ensure interpreter and Wasm agree on resulting type/behavior.

**Exit criteria**

* `Byte` arithmetic examples in spec/plan compile and run.
* Promotion behavior is tested in both run and differential suites.

### Phase 4: Remove `Iterator.unfold` fallback by construction

**Objective**

Eliminate fallback-based recovery in `resolve_unfold_step_types` and make typed unfold-step
selection deterministic from explicit metadata.

**Why this is a major task**

This touches type propagation, ANF lowering, codegen context, and backend invariants. A safe
rollout needs intermediate checkpoints to keep backend behavior stable.

#### Phase 4a: Add explicit ANF op-result `MonoType` metadata

**Work**

* Introduce explicit result-mono metadata for let-bound ANF ops used during emit-time typing.
* Ensure monomorphized concrete result types are attached before backend emission.
* Keep current inference paths as compatibility fallback while metadata is being populated.

**Exit criteria**

* For unfold-related let-bindings, emit-time type lookup succeeds from metadata in all existing tests.
* No behavior change in interpreter or Wasm outputs.

#### Phase 4b: Make emit context consume metadata first

**Work**

* Update `EmitCtx` result-type lookup to prefer explicit op-result metadata over local heuristics.
* Limit heuristic/inferred paths to diagnostics or temporary compatibility only.
* Add debug assertions when a required unfold-related op result is missing metadata.

**Exit criteria**

* Typed unfold-step selection does not require current-function return type for covered paths.
* New tests prove callback-heavy `Iterator.unfold` programs remain typed/specialized correctly.

#### Phase 4c: Remove fallback from `resolve_unfold_step_types`

**Work**

* Delete contextual fallback to function return type in `resolve_unfold_step_types`.
* Convert fallback-missing cases into compile-time invariant failures with actionable diagnostics.
* Keep the function narrow: resolve from explicit arg/op metadata only.

**Exit criteria**

* `resolve_unfold_step_types` has no function-return fallback logic.
* All run, wasm, and differential tests pass without restoring fallback behavior.

#### Phase 4d: Invariant enforcement pass

**Work**

* Add a validation pass before Wasm emission that checks required op/local mono metadata exists.
* Fail fast with clear diagnostics when unfold-specialization prerequisites are missing.
* Document the invariant in backend internals docs.

**Exit criteria**

* Missing unfold typing metadata cannot silently degrade to inferred/fallback behavior.
* Failures are deterministic and easy to triage.

### Phase 5: Prelude ID lifecycle policy

**Work**

* Define rules for:
  * reserved/deprecated IDs
  * alias windows
  * removal procedure
* Encode policy in prelude map tests to avoid ad hoc skips.

**Exit criteria**

* No manual one-off skip logic for retired IDs without documented policy.
* Migration path is clear for future stdlib/intrinsic renames.

---

## Test strategy additions

Add focused fixture groups:

* `tests/run/string_large_index_semantics.tw`
* `tests/run/traps/string_large_index_traps.tw`
* `tests/run/byte_arithmetic_promotion.tw`
* `tests/run/iterator_unfold_callback_typing.tw`
* `tests/run/iterator_unfold_rebind_callback_typing.tw`
* `tests/run/iterator_unfold_nested_match_typing.tw`

And ensure coverage in:

* [tests/run_test.rs](../../tests/run_test.rs)
* [tests/run_wasm_test.rs](../../tests/run_wasm_test.rs)
* [tests/differential_test.rs](../../tests/differential_test.rs)

---

## Out of scope

* Grapheme cluster semantics.
* Unicode normalization/casefold/collation work.
* Large runtime representation redesign beyond what is needed for the above hardening.

---

## Definition of done for fallback removal

Fallback removal is complete only when all of the following are true:

* `resolve_unfold_step_types` has no contextual return-type fallback path.
* Emit-time unfold-step typing is sourced from explicit metadata, not ad hoc inference.
* A pre-emit validation pass enforces required typing invariants.
* Callback-heavy unfold fixtures pass in interpreter, Wasm, and differential suites.
