# Boot Phase File Decomposition

## Goal

Decompose the largest boot compiler phase files into focused implementation
modules after shared helpers have been extracted.

This plan is about internal maintainability. It is not a broad path/layout
rename plan; that is tracked separately by the boot compiler layout
reorganization plan.

---

## Motivation

Several phase files contain many distinct responsibilities in one module:

* parser grammar routines and recovery
* checker unification, annotation resolution, expression checking, statements,
  patterns, exhaustiveness, and interpolation
* Core lowering for calls, records, variants, control flow, closures, `try`, and
  comprehensions
* Wasm emission for calls, ABI shims, records, variants, control flow, helpers,
  globals, and runtime bridges

Large files make targeted review difficult and encourage local duplication.

---

## Non-Goals

* No behavior changes
* No user-visible diagnostic changes unless unavoidable
* No file moves that conflict with `boot-compiler-layout-reorg.md`
* No parser/checker architecture redesign
* No broad helper extraction that belongs in the shared helper plans

---

## Suggested Decomposition

Exact names should follow the final compiler layout, but the concern boundaries
should remain stable.

### Checker

```text
checker/unify.tw
checker/annotations.tw
checker/patterns.tw
checker/expr.tw
checker/stmts.tw
checker/exhaustiveness.tw
checker/interpolation.tw
```

### Core lowering

```text
lower_core/calls.tw
lower_core/records.tw
lower_core/variants.tw
lower_core/patterns.tw
lower_core/control_flow.tw
lower_core/closures.tw
lower_core/collect.tw
```

### Wasm emission

```text
emit/calls.tw
emit/runtime_abi.tw
emit/records.tw
emit/variants.tw
emit/control_flow.tw
emit/closures.tw
emit/helpers.tw
emit/module_globals.tw
```

### Parser

Parser decomposition should be more conservative because grammar routines are
heavily coupled by cursor/recovery behavior. Prefer extracting only clearly
separable helpers first:

```text
parser/types.tw
parser/patterns.tw
parser/strings.tw
parser/recovery.tw
```

---

## Work Plan

### Phase 1: Precondition cleanup

- [ ] Complete or partially complete shared type helper extraction.
- [ ] Complete or partially complete Wasm type-ordering extraction for codegen.
- [ ] Decide whether this plan lands before or after the broader layout reorg.

### Phase 2: Decompose checker

- [ ] Extract unification and substitution-adjacent code first.
- [ ] Extract pattern and exhaustiveness checking.
- [ ] Extract expression and statement checking only after smaller helpers are
      stable.
- [ ] Keep `checker.tw` as the public entrypoint.

### Phase 3: Decompose Core lowering

- [ ] Extract pattern lowering and variant lowering.
- [ ] Extract call/method/contract lowering.
- [ ] Extract control-flow lowering.
- [ ] Keep `lower_core.tw` as the public entrypoint.

### Phase 4: Decompose Wasm emission

- [ ] Extract runtime ABI shims first, because they are a distinct concern.
- [ ] Extract call emission.
- [ ] Extract records/variants/control-flow helpers.
- [ ] Keep `emit.tw` as the public entrypoint until the broader layout plan says
      otherwise.

### Phase 5: Consider parser extraction last

- [ ] Extract string literal/interpolation parsing if low-risk.
- [ ] Extract type-expression parsing if imports remain simple.
- [ ] Avoid splitting recovery logic until parser tests are strong enough to
      catch subtle cursor-position regressions.

---

## Validation

- [ ] Parser suite after any parser split
- [ ] Checker suites after checker split
- [ ] Lowering suites after lowering split
- [ ] Codegen suites after emitter split
- [ ] Full boot test suite after each phase

---

## Risks

* Twinkle module imports may create cycles if helpers are split too eagerly.
* Parser cursor/recovery behavior is easy to perturb with mechanical moves.
* Public entrypoints should remain stable during decomposition to avoid test
  churn.
