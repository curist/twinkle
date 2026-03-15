# Qualified Variant Constructor Paths Plan

## Goal

Allow qualified constructor paths in expression position:

* `Type.Variant`
* `Type.Variant(args...)`
* `module.Type.Variant`
* `module.Type.Variant(args...)`

while preserving the existing rejection of value-postfix constructor syntax:

* `x.Variant` (where `x` is a value expression) stays invalid.

## Current Baseline (2026-03-15)

* Parser accepts `Type.Variant` in expression position.
* Parser rejects `module.Type.Variant` and `module.Type.Variant(args)` with
  `ConstructorInPostfix`.
* Type checking and lowering currently detect constructor forms mainly when the
  base is a single `Ident` (e.g. `Type.Variant`), not a dotted type path.

This creates a mismatch with documented constructor ergonomics and with existing
qualified type alias registration in `TypeEnv`.

## Scope

In scope:

* Expression-position constructor access/calls for dotted type paths.
* Preserve explicit parse-time rejection of value-postfix constructors.
* Keep constructor resolution static and type-directed.
* Add parser/typecheck/lowering and regression tests.

Out of scope:

* Enabling constructor lookup from runtime values.
* Changing variant naming rules (still PascalCase).
* Broad syntax changes beyond constructor-path handling.

## Desired Semantics

1. `Type.Variant` and `module.Type.Variant` are constructor references.
2. `Type.Variant(args)` and `module.Type.Variant(args)` construct variants.
3. `x.Variant` remains a parse error (constructor cannot be selected from a
   value expression).
4. Existing lowercase postfix behavior is unchanged (`x.field`, `x.method()`).

## Design

### 1. Parser: refine postfix constructor guard

Today, terminal postfix `.Upper` is rejected unconditionally. Replace this with
base-sensitive logic:

* Continue rejecting terminal `.Upper` when base is value-like.
* Allow terminal `.Upper` when base is syntactically a potential type path
  (identifier/field-access chain that includes a type segment).

Practical heuristic for stage0 parser:

* Permit terminal `.Upper` when the base is an identifier/field-access chain
  and has at least one PascalCase segment.
* Keep rejection for chains that are purely lowercase segments (`x.Variant`,
  `foo.bar.Variant`).

This keeps current ergonomics (`Type.Variant`, `mod.Type.Variant`) without
opening value-postfix constructors.

### 2. Shared extraction for dotted type names

Introduce one helper that converts `ExprKind::Ident`/`ExprKind::FieldAccess`
chains into a dotted name string, and reuse it in both:

* type checker constructor detection
* lowerer constructor lowering

This avoids one-off handling that only matches `Ident`.

### 3. Type checker updates

For `ExprKind::FieldAccess` and field-access `Call` callee handling:

* Attempt constructor detection using extracted dotted base path + terminal
  field as variant name.
* Resolve base dotted path via `TypeEnv::lookup_type`.
* If resolved and variant exists, synthesize constructor value/function as today
  (arity and payload checks unchanged).
* Otherwise, fall back to existing module/value field/method logic.

### 4. Lowerer updates

Mirror type checker logic:

* For field-access expressions and field-access calls, recognize constructor
  forms where base is a dotted type path.
* Lower to existing `CoreExprKind::Variant` representation.
* Preserve current fallback paths for module function calls, method calls,
  record field access, and value references.

### 5. Diagnostics

Keep `ConstructorInPostfix` as a parse-time diagnostic for disallowed forms.
Optionally tighten wording to clarify value-vs-type distinction.

## Tests

Parser tests:

* Pass: `Type.Variant`, `Type.Variant(1)`.
* Pass: `mod.Type.Variant`, `mod.Type.Variant(1)`.
* Fail: `x.Variant` with `ConstructorInPostfix`.

Typecheck/lower/run coverage:

* Cross-module fixture using `mod.Type.Variant` and
  `mod.Type.Variant(payload)` in expression position.
* Zero-arg and payload variants.
* Ensure existing `Type.Variant` behavior remains unchanged.

Regression coverage:

* Keep/extend parser error fixture for value-postfix constructor rejection.

## Risks and Mitigations

Risk: Heuristic admits odd lowercase-prefix chains that are not true module/type
paths.

Mitigation:

* Parser only decides syntactic admissibility.
* Type checker still validates against `TypeEnv`; invalid paths fail with normal
  typed errors.
* Add targeted parser/typecheck regressions for borderline chains.

Risk: Resolution-order regressions between module-qualified function calls and
constructor calls.

Mitigation:

* Keep constructor detection narrow (must resolve as type + known variant).
* Preserve existing module-call fast path when constructor detection does not
  match.

## Implementation Steps

1. Add parser helper for constructor-eligible postfix base and update terminal
   `.Upper` guard.
2. Add shared dotted-name extractor for expression chains.
3. Update type checker constructor detection for both field-access synthesis and
   field-access calls.
4. Update lowerer constructor lowering for dotted type bases.
5. Add parser + integration tests and run full test suite.

## Exit Criteria

* `module.Type.Variant` and `module.Type.Variant(args)` parse, typecheck, and
  lower correctly.
* `x.Variant` remains rejected at parse time.
* Existing constructor, field, and method behaviors are unchanged in regression
  tests.
