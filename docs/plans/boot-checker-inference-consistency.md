# Boot Checker Inference Consistency Plan

Last updated: 2026-03-28

## Goal

Tighten `boot/compiler/checker.tw` around contextual call inference, closure
annotation reconciliation, record literal validation, and final ambiguity
reporting without rewriting the checker architecture.

This plan is intentionally incremental. The current checker model is good enough
to keep; the main problems are local drift between similar call paths,
incomplete expected-type propagation, and a few diagnostics/validation gaps.

---

## Scope

In scope:

- contextual return-type propagation for calls,
- consistency across direct, module-qualified, method, variant-constructor, and
  general call paths,
- closure return annotation reconciliation in check mode,
- duplicate-field validation for record literals,
- final unsolved-meta diagnostic quality,
- checker-facing documentation for `Never`, alias-normalization boundaries, and
  field-vs-method precedence.

Out of scope:

- redesigning the unifier,
- changing the source-order top-level function inference rule,
- large checker architecture rewrites,
- non-checker runtime or codegen work.

---

## Verified Issues (Current HEAD)

### I1 — `pre_unify_return` policy is ambiguous in code

`pre_unify_return` computes `pre_u := ctx.unify(...)` but returns the incoming
diagnostic vector instead of `pre_u.diags`.

Today this is either:

- an implementation bug if pre-unification is supposed to surface its own
  diagnostics, or
- an unclear implementation of a best-effort constraint-solving step if
  pre-unification is supposed to be silent.

Stage0 uses the second policy explicitly: solve from expected return type when
helpful, but ignore pre-unify mismatch diagnostics and let the outer call/type
check report the real error later. Boot should pick one policy and encode it
clearly rather than looking accidental.

### I2 — Expected return propagation is inconsistent across call forms

Boot already threads `call_expected_ret` into some call sites, but not all:

- direct named calls use it,
- module-qualified calls use it,
- receiver method calls use it,
- qualified variant constructor calls (`TypeName.Variant(args)`) do not,
- general expression calls (`f_expr(args)`) do not.

This makes inference quality depend on surface syntax rather than callable
semantics.

### I3 — Instantiated-call logic is duplicated and already drifting

Named calls, module-qualified calls, and receiver method calls each carry their
own version of:

- signature instantiation,
- optional return pre-unification,
- arity checks,
- argument checking,
- callee-type recording,
- method metadata recording.

That duplication is already responsible for the call-form asymmetries above.

### I4 — `check_closure` drops explicit return annotations under expected function types

When `check_closure` receives an expected function type, it reconciles parameter
annotations with expected parameter types, but it does not reconcile an
explicit closure return annotation with the expected return type before checking
the body.

That weakens user-written annotations in cases like:

```tw
let f: fn(Int) String = fn(x) Int { x }
```

The checker should reject the explicit `Int` annotation against expected
`String`, not silently ignore the explicit annotation path.

### I5 — `check_record_lit` does not reject duplicate fields

The checker validates unknown fields and missing fields, but it does not
diagnose duplicate entries in the same literal unless the parser happens to
reject them earlier.

The checker should diagnose duplicates itself so the semantic layer does not
depend on parser behavior for basic record well-formedness.

### I6 — Final unsolved-meta reporting is noisy and imprecise

The final pass walks every `type_map` entry, zonks it, and emits
`cannot infer type for this expression` whenever the result still contains a
meta.

That is useful, but it currently:

- can emit many duplicates for one root ambiguity,
- reports at every expression node instead of a smaller set of reporting roots,
- falls back to coarse synthetic spans when better span information is missing.

### I7 — A few checker semantics are implemented but not frozen anywhere

The current checker already implies these rules:

- `Never` unifies with any type,
- real record fields win over method-value fallback,
- aliases are expanded at selected checker boundaries rather than universally,
- top-level inference for unannotated functions is source-order sensitive.

These are reasonable choices, but they should be written down explicitly so
later cleanups do not accidentally change them.

---

## Decisions To Freeze Up Front

Before touching implementation details, freeze the following policy decisions:

1. `pre_unify_return` is best-effort and silent, matching stage0.
2. All callable forms should receive the same contextual return-type help when
   they are semantically equivalent call sites.
3. Explicit closure annotations are user intent and must be reconciled, not
   ignored, in check mode.
4. Final ambiguity diagnostics should prefer fewer, higher-signal reports over
   exhaustive per-node duplication.

If any of these decisions changes, this plan should be updated before code
changes land.

---

## Implementation Plan

## Phase 1 — Normalize Call Inference Semantics

### 1.1 Make `pre_unify_return` explicit

Refactor `pre_unify_return` so its behavior is no longer ambiguous:

- if the project wants stage0 parity, rename or comment it as a best-effort
  solver and intentionally discard mismatch diagnostics,
- otherwise thread `pre_u.diags` deliberately and add tests that assert the
  earlier diagnostic behavior.

Recommended direction: keep it silent and explicit, matching stage0.

### 1.2 Extract one helper for instantiated call checking

Create a shared helper for the common pipeline:

1. instantiate signature,
2. optionally pre-unify return type from context,
3. validate arity,
4. check arguments,
5. record callee type and optional method metadata,
6. return instantiated result type.

That helper should be used by:

- direct named calls,
- module-qualified calls,
- receiver method calls,
- qualified variant-constructor calls when they are represented as a known
  callable shape.

### 1.3 Extend contextual return help to the remaining call paths

After the shared helper exists:

- apply contextual return propagation to qualified variant constructors,
- apply contextual return propagation to `synth_call_general`,
- add coverage for higher-order calls so `check_expr(f(x), Expected)` and
  `check_expr(named_call(x), Expected)` do not diverge purely because one callee
  is first-class and the other is syntactically direct.

### 1.4 Preserve or improve diagnostics while normalizing call paths

Call-path unification must not regress:

- arity diagnostics,
- missing-method diagnostics,
- undefined module-qualified target diagnostics,
- method metadata capture used by later lowering.

This phase is successful only if behavior becomes more uniform without turning
specific diagnostics into generic fallback failures.

## Phase 2 — Tighten Local Validation Holes

### 2.1 Reconcile closure return annotations in check mode

In the expected-function branch of `check_closure`:

- if a closure return annotation exists, resolve it,
- unify it with the expected return type,
- then check the body against the reconciled return type.

Add regression coverage for:

- matching explicit annotation + expected type,
- conflicting explicit annotation + expected type,
- no explicit annotation, expected type only.

### 2.2 Add duplicate-field detection in `check_record_lit`

Track seen field names while checking entries and emit a duplicate-field
diagnostic on repeat appearance.

This should compose cleanly with existing unknown-field and missing-field
diagnostics without suppressing the more useful error for each case.

---

## Phase 3 — Improve Final Ambiguity Reporting

### 3.1 Choose reporting roots

Replace unconditional reporting on every `type_map` entry with a smaller set of
high-signal roots, such as:

- `let` initializers,
- block tails,
- call expressions,
- top-level expressions,
- function bodies/returns.

If root selection is too invasive, use a lighter-weight dedupe pass first.

### 3.2 Deduplicate by span or meta set

At minimum, avoid emitting the same ambiguity repeatedly for the same source
location. Better options:

- dedupe by concrete span,
- dedupe by `(span, zonked type)` pair,
- dedupe by the set of remaining meta IDs.

### 3.3 Preserve precise spans where available

Prefer the real expression span recorded earlier in checking. Only synthesize a
fallback span when there is genuinely no tracked source location.

---

## Phase 4 — Freeze Semantics in Docs and Tests

### 4.1 Document checker semantics that already exist

Write down, in the most appropriate design/spec location:

- `Never` unifies with any type,
- field lookup has priority over method-value fallback,
- alias expansion happens at selected checker boundaries,
- unannotated top-level function inference is source-order sensitive.

### 4.2 Add focused regression tests

Add checker tests that lock the intended behavior for:

- direct/module/method/general call inference under expected return types,
- qualified variant-constructor inference,
- closure return annotation conflicts,
- duplicate record fields,
- deduplicated ambiguous-type diagnostics.

---

## Suggested Execution Order

1. Freeze `pre_unify_return` policy.
2. Extract the shared instantiated-call helper.
3. Route qualified variant constructors and general calls through the same
   contextual-return model.
4. Fix closure return annotation reconciliation.
5. Add duplicate-field diagnostics.
6. Reduce ambiguity-reporting noise.
7. Write down the semantic rules and land regression tests.

Rationale: call-path normalization is the highest-leverage change and reduces
the chance of fixing the same inference bug in multiple branches. Validation and
reporting cleanups should follow after the call surface is stable.

---

## Test Matrix

The plan is not complete unless the checker is covered for all of the following:

1. Named generic call inferred from expected return type.
2. Module-qualified generic call inferred from expected return type.
3. Receiver method generic call inferred from expected return type.
4. Qualified variant-constructor call inferred from expected return type.
5. Higher-order/general call inferred from expected return type.
6. Closure with explicit return annotation matching expected function type.
7. Closure with explicit return annotation conflicting with expected function type.
8. Record literal with duplicate field.
9. One ambiguous root producing one high-signal diagnostic instead of a cascade.

---

## Exit Criteria

This plan is complete when all are true:

1. Equivalent call forms receive equivalent contextual inference help.
2. `pre_unify_return` policy is explicit in code and tests.
3. Closure return annotations are never silently ignored in check mode.
4. Record literals reject duplicate fields in the checker.
5. Final ambiguity reporting is materially less noisy on common ambiguous inputs.
6. `Never`, alias-boundary behavior, field-vs-method precedence, and source-order
   inference limitations are written down in docs.
