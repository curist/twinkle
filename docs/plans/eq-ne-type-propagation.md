# `==` / `!=` Type-Propagation Plan

## Goal

Make equality checks bidirectional enough that contextual types flow across
operands, so expressions like `kind == .Use` type-check when `kind` already has
a known sum type.

Primary target:

- `==` and `!=` in `src/types/check.rs` (`synth_binary`).

Out of scope for this plan:

- changing `< <= > >=` behavior
- changing variant-literal rules outside contextual positions
- broad rework of bidirectional inference across all binary operators

---

## Current Baseline

In `synth_binary`, comparison operators currently do:

1. `left_ty = synth_expr(left)?`
2. `right_ty = synth_expr(right)?`
3. `unify(left_ty, right_ty, right.span)?`

This fails for shorthand variants on either side because `.Variant(...)` is
rejected in synthesis mode (`synth_variant_lit` emits
`"variant literals without type context"`).

So:

- `kind == .Use` fails today even if `kind: TokenKind`.
- `.Use == kind` also fails for the same reason.

---

## Desired Semantics

For `==` and `!=`:

1. If one side synthesizes to a concrete/known type, check the other side
   against that type first.
2. If that directional check fails, try the opposite direction.
3. If neither directional check works, fall back to current synth+synth+unify
   behavior for final diagnostics.

This keeps the existing equality type rule ("both operands must unify") while
using available context to type-check context-dependent literals.

---

## Implementation Plan

### P1 — Add Speculative Type-Check Helper

Add a small internal helper in `TypeChecker` to run a tentative check/synth
attempt without committing diagnostics/state when it fails.

Reason: directional attempts can fail transiently and should not emit duplicate
or misleading errors before fallback.

State to roll back on failed attempt:

- `errors` length
- `meta_subst`
- `next_meta`

`TypeMap` writes are typically overwritten later, but this plan should verify
whether failed speculative paths leak problematic entries; if so, roll back
`type_map` deltas too.

### P2 — Refactor Eq/Ne Arm

In `synth_binary`:

- split `BinOp::Eq | BinOp::Ne` from other comparisons
- implement directional algorithm:
  1. try `left = synth`, then `check(right, left_ty)`
  2. try `right = synth`, then `check(left, right_ty)`
  3. fallback: current synth+synth+unify path
- return `MonoType::Bool` on success (same as today)

Keep `< <= > >=` on current behavior for now.

### P3 — Regression Tests

Add focused tests in `tests/typecheck/pass` and `tests/typecheck/fail`:

Pass cases:

- `kind == .Use` where `kind: TokenKind`
- `.Use == kind` where `kind: TokenKind`
- `x == .Some(1)` where `x: Option<Int>`

Fail cases:

- both sides context-free shorthand variants:
  `.Some(1) == .Some(2)` (still requires external context)
- wrong contextual payload:
  `x: Option<Int>; x == .Some("x")`
- unknown variant for contextual sum type

Also add at least one assertion-oriented Rust test in
`tests/typecheck_test.rs` for diagnostic stability (single clear root error,
not duplicated cascades).

---

## Edge Cases to Decide Explicitly

1. If both directional attempts fail, should we keep only fallback diagnostics
   or merge directional notes?
   Recommendation: keep fallback diagnostics only (clearer, less noisy).

2. Should equality allow context propagation into anonymous records too
   (`rec == .{ ... }`)?
   Recommendation: yes, as a natural consequence of `check_expr` against known
   `left_ty`.

3. Should this behavior later extend to `< <= > >=`?
   Recommendation: defer; those operators often imply additional constraints
   beyond "same type".

---

## Risks and Mitigations

- Risk: speculative attempts mutate unification state.
  Mitigation: explicit rollback guard around tentative attempts.

- Risk: duplicate diagnostics from multiple attempts.
  Mitigation: rollback failed-attempt diagnostics and emit only final-path
  diagnostics.

- Risk: unintentionally accepting ambiguous equality forms.
  Mitigation: keep fallback requiring successful synth/unify; add explicit fail
  tests for both-sides-context-free literals.

---

## Exit Criteria

The plan is complete when:

1. `kind == .Use` and `.Use == kind` both type-check with known `TokenKind`.
2. Existing fail case `variant_shorthand_no_context.tw` still fails.
3. Typecheck test suite remains green with new coverage for directional
   equality propagation.
