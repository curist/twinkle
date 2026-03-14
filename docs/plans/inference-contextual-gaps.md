# Contextual Inference Gaps Plan

## Goal

Close two user-facing type-inference gaps where contextual information exists
but is not applied strongly enough:

1. shorthand variant literals in generic argument positions
2. unannotated closure parameters in generic callback positions

Examples to support:

```tw
// should infer ParseError for E in ok_or<T, E>
def := try find_by_long(active_args, key).ok_or(.UnknownFlag("--${key}"))

// should infer a: arg.Arg from find<A>(xs: Vector<A>, f: fn(A) Bool)
args.find(fn(a) { a.name == name })
```

---

## Current Baseline (2026-03-14)

Observed behavior:

* `ok_or(.UnknownFlag(...))` fails unless constructor is type-qualified
  (`ParseError.UnknownFlag(...)`).
* `find(fn(a) { ... })` may fail without explicit parameter annotation
  (`fn(a: arg.Arg) Bool { ... }`) in some generic-call contexts.

Current relevant implementation:

* call checking/inference: `src/types/check.rs` (`synth_call`, `check_expr`)
* variant shorthand behavior:
  * `synth_variant_lit` intentionally errors without concrete context
  * `check_variant_lit` expects a concrete sum type
* closure checking depends on expected function type propagation in `check_expr`
* practical reproducer: `boot/lib/argparse/app.tw`

---

## Scope

In scope:

* improve contextual inference for `.Variant(...)` in generic argument positions
* improve contextual inference for unannotated closure params in generic callback
  positions
* add regression tests for both success and ambiguity/failure behavior

Out of scope:

* higher-rank polymorphism
* trait/constraint-style overloading
* broad type-system redesign

---

## Design Direction

### A. Variant shorthand against expected metavariables

Problem:

* in calls like `ok_or`, the expected arg type can be a metavariable (`?E`) at
  check time; shorthand variant checking currently requires a concrete `Named`
  sum type.

Direction:

* when checking `.Variant(...)` against expected `MetaVar(m)`:
  * enumerate candidate sum types containing `Variant` with matching arity
  * if exactly one candidate is viable, solve `m` to that sum type and proceed
    with regular `check_variant_lit`
  * if none: keep current mismatch/no-such-variant style error
  * if multiple: emit an ambiguity diagnostic with guidance to qualify variant
    (`TypeName.Variant(...)`) or annotate

### B. Closure param inference in generic callback calls

Problem:

* callback expected type may still contain unresolved metas; unannotated
  closure params then remain too unconstrained and field access fails.

Direction:

* in closure checking against expected function type:
  * aggressively zonk/resolve expected param types before binding closure params
  * if expected param includes solvable metas, bind them from closure body usage
    via normal unification
  * ensure method-call argument checking passes expected callback types that are
    sufficiently instantiated before checking closure bodies

---

## Milestones

### I1 — Red Tests for Both Gaps

Add failing tests that characterize current behavior:

* `ok_or(.UnknownFlag(...))` in `boot/lib/argparse/app.tw`-like shape
* `Vector.find(fn(a) { a.name == ... })` with unannotated `a`

Likely files:

* `tests/typecheck/pass/*` and `tests/typecheck/fail/*`
* targeted unit tests in `src/types/check.rs` if useful

Acceptance:

* tests fail before implementation and encode intended behavior

### I2 — Variant Contextual Inference

Implement metavariable-aware variant checking in argument contexts.

Likely files:

* `src/types/check.rs`
* possibly `src/types/error.rs` for ambiguity diagnostics

Acceptance:

* shorthand variant in `ok_or`-style calls infers expected error sum type
* ambiguous shorthand variant cases produce clear diagnostics

### I3 — Closure Param Contextual Inference

Improve generic callback contextual typing for unannotated closure params.

Likely files:

* `src/types/check.rs` (`synth_call`, closure branch in `check_expr`)

Acceptance:

* `find(fn(a) { ... })` works without param annotation when callback type is
  contextually available
* existing annotated and non-generic closure behavior remains unchanged

### I4 — Diagnostics and Regression Hardening

Add/adjust diagnostics and broad regression coverage.

Coverage:

* positive:
  * `ok_or(.ErrVariant(...))` style inference
  * unannotated callback closures in method and qualified-call forms
* negative:
  * ambiguous shorthand variant name across multiple sum types
  * truly unconstrained closure param still requests annotation

Acceptance:

* failures are actionable; no misleading `Expected: Int / Actual: ?N` class
  messages for these cases

---

## Risks and Mitigations

* Risk: over-eager variant candidate selection can infer wrong type.
  Mitigation: require unique viable candidate; otherwise emit ambiguity error.
* Risk: closure inference changes may regress existing generic-call checking.
  Mitigation: add focused and broad regression tests; keep changes localized to
  callback/closure paths.
* Risk: more metas remain unsolved at statement boundaries.
  Mitigation: preserve existing zonk/occurs-check paths and add targeted asserts
  in tests.

---

## Exit Criteria

1. `ok_or(.Variant(...))`-style calls type-check when context uniquely determines
   the sum type.
2. `find(fn(x) { ... })`-style unannotated closures type-check when callback
   type is contextually known.
3. Ambiguous or unconstrained cases produce explicit diagnostics with clear fix
   suggestions.
4. New regression tests cover both success and failure modes.
