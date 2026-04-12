# Boot nested variant-pattern lowering safety

## Context

While cleaning up hot-path lookups in boot codegen, a seemingly harmless rewrite
of `boot/compiler/codegen/emit.tw` introduced a stage2 runtime trap:

- stage0 could still build `boot/main.tw`
- the stage1-produced boot compiler could still build stage2
- but running the resulting stage2 Wasm binary trapped with `unreachable`
  even on `--help`

The regression came from rewriting code like this:

```tw
case env.lookup_type_def(tid) {
  .Some(d) => ...
  .None => ...
}
```

into a more compact nested variant-pattern form:

```tw
case env.lookup_type_def(tid) {
  .Some(.Sum(_, _, _)) => ...
  .Some(.Alias(_, type_params, target)) => ...
  _ => ...
}
```

In particular, the nested shape inside `can_match_variant_pattern` appears to be
what pushed the self-hosted compiler into the bad state.

The code has since been restored to a safer two-step form so self-hosting works
again, but the underlying compiler/runtime issue remains unexplained.

## Temporary rule

Until this bug is understood, do **not** rewrite production boot compiler code
back to compact nested variant-pattern matches.

Avoid shapes like:

```tw
case some_option {
  .Some(.Variant(...)) => ...,
  _ => ...,
}
```

Prefer the currently-safe two-step form:

```tw
def := case env.lookup_type_def(tid) {
  .Some(d) => d,
  .None => { return false },
}
case def {
  .Sum(_, _, _) => true,
  .Alias(_, type_params, target) =>
    can_match_variant_pattern(subst_type_params(target, type_params, args), env),
  _ => false,
}
```

This should be treated as a temporary coding rule for boot compiler sources
until a reduced repro exists and the self-host loop is green with the compact
form restored.

## Problem statement

The boot pipeline currently appears to have a semantic or lowering bug around at
least some **nested variant patterns**, especially when they appear in internal
compiler code that is itself self-hosted and optimized.

This is dangerous because:

- the source-level rewrite looks semantics-preserving
- ordinary stage0 tests did not catch it
- the failure only showed up in later self-host stages
- it can silently discourage reasonable refactors toward clearer pattern code

For this bug class, passing unit-style or stage0-only coverage is not enough.
Any attempted fix must also pass the self-host loop.

We need to determine whether the root cause is:

- parser / AST shape for nested patterns
- resolver / checker typing of nested variant patterns
- lower_core / lower_anf lowering of nested patterns
- optimizer interaction with nested match structure
- codegen for nested variant destructuring
- or a runtime/layout mismatch for specific nested pattern forms

## Goal

Make nested variant-pattern matching semantically reliable in the boot compiler,
or clearly document the currently unsupported subset and enforce it explicitly.

The preferred end state is full correctness for nested variant patterns of the
kind used above.

## Non-goals

This plan does not aim to:

- redesign pattern matching syntax
- broaden match ergonomics beyond current semantics
- optimize pattern lowering for speed first
- replace the current match lowering architecture wholesale

Correctness and diagnosability come first.

## Known failing shape

A concrete suspicious shape is:

```tw
case env.lookup_type_def(tid) {
  .Some(.Sum(_, _, _)) => true,
  .Some(.Alias(_, type_params, target)) =>
    can_match_variant_pattern(subst_type_params(target, type_params, args), env),
  _ => false,
}
```

The equivalent two-step form does not trigger the trap:

```tw
opt := env.lookup_type_def(tid)
case opt {
  .Some(def) => {
    case def {
      .Sum(_, _, _) => true,
      .Alias(_, type_params, target) => ...,
      _ => false,
    }
  },
  .None => false,
}
```

This strongly suggests the problem is not the semantic intent but the compiled
handling of nested variant patterns.

## Current investigation status

Status at the moment:

- production boot code still uses the safer two-step workaround
- `tools/selfhost_loop.sh boot/main.tw` remains the primary regression check
- a reduced nested-pattern repro exists and passes through the self-host path
- a more faithful paired repro also exists in both nested and two-step forms
- we have **not** reproduced the original stage2 trap in isolation yet

Artifacts added during investigation:

- `boot/repros/nested_variant_pattern_repro.tw`
- `boot/repros/nested_variant_pattern_faithful_nested.tw`
- `boot/repros/nested_variant_pattern_faithful_two_step.tw`
- `tools/selfhost_nested_pattern_repro.sh`
- `tools/selfhost_compare_nested_pattern_ir.sh`

What the paired self-host repros established:

- the nested and two-step source forms both succeed when isolated
- the self-hosted compiler preserves them as **different optimized IR shapes**
- the two-step form lowers to an outer `Option` unwrap match followed by a
  second match on `ResolvedTypeDef`
- the compact nested form remains a single match with nested variant arms

So the current evidence still points to a downstream lowering/backend issue, but
it appears to require a more specific surrounding context than the reduced
repros currently capture.

## Investigation plan

### 1. Add a minimal reduced repro

Status: **partially done**.

Focused repros now exist for:

- small nested `Option<Result<...>>` matching
- a more faithful `ResolvedEnv.lookup_type_def` / alias-recursive helper shape
- both compact nested and equivalent two-step forms

These repros pass through the self-host path today, so they are useful for
comparison and future regression checks, but they do **not** yet trigger the
original stage2 trap.

Follow-up still needed:

- find the missing ingredient that makes the real compiler path fail
- decide whether that ingredient is optimizer-sensitive, module-size-sensitive,
  or specific to the original boot codegen context

### 2. Compare nested vs two-step lowering output

Status: **started**.

Inspect IR for both forms:

- nested pattern form
- equivalent two-step form

Compare at each stage if possible:

- parsed AST
- resolved/checker output if useful
- core IR
- ANF
- final emitted Wasm/WAT shape

In particular, compare:

- arm ordering
- scrutinee sharing vs re-evaluation
- payload projection order
- temporary binding lifetime
- branch `unreachable` placement

The key question:

> where do the two semantically equivalent forms stop being equivalent?

Current finding: they already differ in self-hosted optimized IR, so there is no
implicit normalization today. That makes it plausible that later backend stages
see meaningfully different control-flow and binding structure.

## 3. Check optimizer interaction

Status: **partially done**.

Because the failure only surfaced in self-hosted execution, verify whether any
optimizer pass changes the nested-pattern form differently from the two-step
form.

Likely suspects:

- branch simplification
- uniqueness / ownership rewrite side effects on temporary locals
- dead-let / copy-prop around match-bound temps

This step should explicitly compare optimized ANF for both forms.

The current paired repro tooling already demonstrates that optimized IR differs
between nested and two-step forms; the remaining work is to determine whether
that difference is benign or the first point where the later stage2 failure is
seeded.

## 4. Audit pattern-lowering assumptions

Read the pattern-lowering path for nested variants and confirm invariants around:

- scrutinee temp creation
- tag checks vs payload extraction ordering
- wildcard handling inside nested variant branches
- alias / sum / option/result special cases
- branch fallthrough and `unreachable` placement

Likely files:

- `boot/compiler/lower_core.tw`
- `boot/compiler/lower_anf.tw`
- `boot/compiler/codegen/emit.tw`
- related pattern helpers in checker and backend code

The original trigger lived in `boot/compiler/codegen/emit.tw`, in
`can_match_variant_pattern`, so that helper should stay in scope during the
investigation even if the real bug is elsewhere.

## 5. Decide final fix shape

Depending on the root cause, choose one of:

### Option A: fix lowering/codegen

If nested variant patterns are intended and mostly work, repair the actual bug
and keep the compact syntax valid.

### Option B: normalize nested patterns early

Desugar nested variant patterns into explicit staged matches before the risky
lowering path.

This may be the safest short-to-medium-term fix if the direct nested lowering is
fragile.

### Option C: reject unsupported nested forms explicitly

If the boot compiler cannot yet support a specific nested subset reliably,
reject it with a targeted diagnostic rather than compiling unsoundly.

This is the least desirable end state, but still better than stage2 traps.

## Recommended direction

Prefer **Option B or A**:

- if a localized lowering fix is clear, take Option A
- otherwise, normalize nested patterns into simpler equivalent matches early so
  downstream stages only handle the already-proven shape

The important property is that source refactors between the nested and two-step
forms must not change runtime behavior.

## Tests to add

### A. Focused nested variant-pattern regression

Small reduced repro covering:

- `.Some(.Variant(...))`
- nested wildcard + nested binder combinations
- nested positive and fallback arms

### B. Self-host regression

Exercise the actual codegen/helper shape through:

```bash
tools/selfhost_loop.sh boot/main.tw
```

and treat any stage2/stage3 trap as a failure.

This validation is required even when:

- the reduced repro passes
- stage0 tests pass
- optimizer-focused tests pass

### C. Shape equivalence test

Where practical, assert that nested and two-step forms produce equivalent
observable behavior through the boot pipeline.

## Success criteria

This plan is complete when:

- nested variant-pattern handling no longer causes stage2 runtime traps
- the original compact form is either supported correctly or rejected clearly
- a focused regression test prevents future refactors from reintroducing the bug
- self-hosting remains stable after reintroducing the cleaner source form
- the fix has been validated through `tools/selfhost_loop.sh boot/main.tw`

## Recommended pause point

The investigation is in a better state than before, but the bug is still not
minimized enough to safely attempt a production fix.

For now, keep the current workaround in production code, keep using the
self-host loop as the main regression signal, and resume only when there is time
to inspect later backend stages in more detail.

## Open questions

### Is the problem specific to nested variant patterns, or nested pattern matching in general?

The current evidence points at nested variant patterns, but the real issue could
be broader:

- nested tuple/record-like destructuring if introduced later
- nested `Option`/`Result` special-case lowering
- nested payload extraction combined with alias expansion

### Is the trap caused before Wasm emission?

It may be that the bad shape is already visible in optimized ANF or backend
prepared IR, and Wasm simply makes it fatal. The investigation should not assume
codegen is the first broken stage.

### Should nested patterns be normalized unconditionally?

Even if direct nested lowering can be fixed, a normalization step might still be
worth it if it simplifies downstream invariants and improves self-host
robustness.

