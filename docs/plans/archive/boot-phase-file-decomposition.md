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
emit/arrays.tw
emit/context.tw
emit/coercions.tw
emit/layout_helpers.tw
emit/match.tw
emit/anyref.tw
emit/bridge_funcs.tw
emit/helper_collectors.tw
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

- [x] Complete or partially complete shared type helper extraction.
- [x] Complete or partially complete Wasm type-ordering extraction for codegen.
- [x] Decide whether this plan lands before or after the broader layout reorg.

Progress note: initial lowerer decomposition is landing before the broader layout
reorg, using `boot/compiler/lower_core/` helper modules while keeping
`lower_core.tw` as the public entrypoint.

### Phase 2: Decompose checker

- [ ] Extract unification and substitution-adjacent code first.
- [ ] Extract pattern and exhaustiveness checking.
- [ ] Extract expression and statement checking only after smaller helpers are
      stable.
- [ ] Keep `checker.tw` as the public entrypoint.

Progress note: an initial checker context/unification extraction passed the
normal boot build and test suite but failed the self-hosted stage loop with an
illegal-cast trap. That slice was backed out; checker decomposition should resume
only with `make stage2` or `make bundle-cli` validation in addition to the normal
boot tests.

### Phase 3: Decompose Core lowering

- [x] Extract shared Core expression/type helper modules.
- [x] Extract operator mapping and unit-variant comparison helpers.
- [x] Extract closure free-variable collection helpers.
- [x] Extract pattern lowering and variant lowering.
- [x] Extract call/method/contract lowering.
- [x] Extract collect and for-loop lowering.
- [x] Extract focused control-flow helpers.
- [x] Extract binary/unary operator lowering.
- [x] Extract statement-chain lowering.
- [x] Extract function lowering.

Progress note: `LowerCtx` and its context-local helpers now live in
`lower_core/context.tw`. Call sites in `lower_core.tw` use direct function calls
for lowering routines so future modules can import `LowerCtx` without relying on
inherent methods defined in the entrypoint module. Method target-name resolution
is in `lower_core/calls.tw`, qualified type-name extraction is in
`lower_core/helpers.tw`, runtime-equality classification is in
`lower_core/operators.tw`, iteration type helpers are in `lower_core/types.tw`,
and collect builder call constructors are in `lower_core/collect_helpers.tw`.
Pattern lowering now lives in `lower_core/patterns.tw`, variant/case/array/index
lowering lives in `lower_core/variants.tw`, record construction and field access
live in `lower_core/records.tw`, call/method/contract lowering lives in
`lower_core/calls.tw`, closure lowering and free-variable collection live in
`lower_core/closures.tw`, collect and for-loop lowering live in
`lower_core/iteration.tw`, string interpolation lowering lives in
`lower_core/strings.tw`, lvalue assignment lowering lives in
`lower_core/lvalues.tw`, `if` and `try` lowering plus the indexed-loop
`continue` rewriter live in `lower_core/control_flow.tw`, binary/unary lowering
and runtime equality call construction live with the operator helpers,
statement-chain lowering lives in `lower_core/statements.tw`, and function
lowering lives in `lower_core/functions.tw`. `lower_core.tw` is now expression
dispatch plus module entrypoint glue.
- [x] Keep `lower_core.tw` as the public entrypoint.

### Phase 4: Decompose Wasm emission

- [x] Extract runtime ABI shims first, because they are a distinct concern.
- [x] Extract call emission.
- [x] Extract no-context anyref, bridge-function, and helper-collection code.
- [x] Extract shared context, layout, and coercion helpers needed by records,
      variants, and control flow.
- [x] Extract record construction/access/update emission.
- [x] Extract variant literal emission.
- [x] Extract vector literal and index-operation emission.
- [x] Extract if/loop control-flow emission.
- [x] Extract match arm-chain emission.
- [x] Extract closure/trampoline emission dispatch.
- [x] Keep `emit.tw` as the public entrypoint until the broader layout plan says
      otherwise.

Progress note: host/vector-builder ABI classification and shim instruction
builders now live in `codegen/emit/runtime_abi.tw`. Module-global emission lives
in `codegen/emit/module_globals.tw`, with default-value instruction construction
in `codegen/emit/defaults.tw`. String-pool globals/getters/data-segment emission
lives in `codegen/emit/string_pool.tw`, and shared emission helpers for names,
divergence/trivial-pattern analysis, sum/variant layout lookup, variant field
monotype resolution, and condition composition live in
`codegen/emit/helpers.tw`. Anyref boxing/unboxing and erased-container egress
live in `codegen/emit/anyref.tw`; runtime bridge function emission for typed sum
conversion, host read-file results, option conversion, and iterator-next helpers
lives in `codegen/emit/bridge_funcs.tw`; discovery of required bridge helpers
lives in `codegen/emit/helper_collectors.tw`. `EmitCtx`, local/function lookup,
layout helpers, and stack coercions now live in `codegen/emit/context.tw`,
`codegen/emit/layout_helpers.tw`, and `codegen/emit/coercions.tw`, giving
`EmitCtx`-dependent slices a cycle-free base. Direct/runtime call emission lives
in `codegen/emit/calls.tw`; `emit.tw` keeps a small wrapper to pass callbacks for
atom, intrinsic, and closure-specific emission that still belongs to the main
emitter. Record construction/access/update emission lives in
`codegen/emit/records.tw`, variant literal emission lives in
`codegen/emit/variants.tw`, vector literal and index-operation emission live in
`codegen/emit/arrays.tw`, if/loop emission plus result-store helpers live in
`codegen/emit/control_flow.tw`, match arm-chain emission lives in
`codegen/emit/match.tw`, and closure/trampoline emission lives in
`codegen/emit/closures.tw`. The public codegen entrypoint remains unchanged.
These slices were validated with the normal boot tests and the self-hosted stage
loop.

### Phase 5: Consider parser extraction last

- [x] Extract numeric literal parsing helpers as a low-risk parser split.
- [ ] Extract string literal/interpolation parsing if low-risk.
- [ ] Extract type-expression parsing if imports remain simple.
- [x] Avoid splitting recovery logic until parser tests are strong enough to
      catch subtle cursor-position regressions.

---

## Validation

- [x] Parser suite after any parser split
- [ ] Checker suites after checker split
- [x] Lowering suites after lowering split
- [x] Codegen suites after emitter split
- [x] Full boot test suite after each phase

---

## Risks

* Twinkle module imports may create cycles if helpers are split too eagerly.
* Parser cursor/recovery behavior is easy to perturb with mechanical moves.
* Public entrypoints should remain stable during decomposition to avoid test
  churn.
