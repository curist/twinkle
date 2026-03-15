# Naming Case Enforcement Plan

## Goal

Align compiler behavior with Twinkle's documented naming rules:

* types and variants: `PascalCase`
* functions, values, locals, fields, module aliases: lowercase-initial

This plan makes those rules hard language constraints (not lint), matching the
spec direction and preserving parser disambiguation guarantees.

## Current Baseline (2026-03-15)

Docs currently state parser-level enforcement:

* [spec naming section](../spec.md)
* [grammar notes for `LowerIdent`](../grammar.ebnf)

Implementation reality is looser for declaration/binding names:

* uppercase function names are accepted
* uppercase value/local bindings are accepted
* uppercase module-level "constant-style" bindings are accepted

Known in-repo example using uppercase values:

* [module_globals.tw](../../tests/run/module_globals.tw)

## Scope

In scope:

* enforce initial-case constraints at declaration/binding/import sites
* keep existing constructor/field parser disambiguation behavior
* update tests/docs to match enforced behavior

Out of scope:

* introducing a `const` keyword
* supporting `Math.PI` style value constants
* changing expression-level constructor grammar (`.Variant`, `Type.Variant`)
* full snake_case linting (this plan enforces initial-case only)

## Policy

1. `TypeDecl` names must start uppercase.
2. Variant names remain uppercase (already enforced in variant literals).
3. Function names must start lowercase.
4. Value binding names (`let`/module `pub` values) must start lowercase.
5. Pattern binding identifiers must start lowercase (except `_` wildcard).
6. Record field names must start lowercase.
7. Imported module alias names (`use ... as alias`) must start lowercase.
8. Destructured import value aliases/names (when feature lands) must start
   lowercase; imported type aliases/names must start uppercase.

## Design

### 1. Parser-level checks for declaration/binding positions

Add targeted parse errors for case violations in these positions:

* type declarations (`type Name = ...`)
* function declarations (`fn name(...)`)
* let/binding patterns in declaration contexts
* record field declarations
* module alias in `use ... as alias`

This keeps errors early and consistent with current parser-driven naming model.

### 2. Keep expression parsing rules load-bearing

No relaxation of existing case-sensitive expression decisions:

* `.Upper` constructor forms
* `.lower` field/method forms
* constructor-in-postfix rejection for disallowed value paths

These rules remain key for parse-time disambiguation and clear diagnostics.

### 3. Diagnostics

Add explicit parse errors with actionable wording, e.g.:

* `"function name 'Parse' must start with a lowercase letter"`
* `"value binding 'PI' must start with a lowercase letter"`
* `"type name 'point' must start with an uppercase letter"`

## Implementation Tasks

1. Add parse error variants for declaration/binding case mismatches.
2. Wire checks into parser declaration/binding parsing paths.
3. Add/update parser unit tests for each rejected form.
4. Update integration fixtures that intentionally used uppercase value bindings.
5. Ensure spec/grammar wording matches exact enforced behavior.

## Test Plan

Parser tests (new/updated):

1. reject uppercase function declaration names.
2. reject uppercase let binding names.
3. reject lowercase type declaration names.
4. reject uppercase record field names.
5. reject uppercase module alias names.
6. continue accepting uppercase type and variant names.

Integration tests:

1. migrate `tests/run/module_globals.tw` to lowercase bindings.
2. confirm no runtime behavior change after rename.
3. verify existing constructor/field parsing snapshots remain stable.

## Migration Notes

Expected repo-facing migration is small:

* rename uppercase value bindings in fixtures/examples to lowercase (currently
  known: `PI`, `GREETING` in `module_globals.tw`).

User-facing migration path:

1. rename value/function/module-alias identifiers to lowercase-initial.
2. keep types/variants PascalCase.
3. if visual constant emphasis is desired, reserve that for future explicit
   language features (`const`) rather than case exceptions.

## Risks and Mitigations

Risk: users expect uppercase constants (`PI`) from other languages.

Mitigation:

* explicit diagnostics and docs rationale: Twinkle immutability means all
  bindings are constant-by-value semantics already.

Risk: drift between parser and tree-sitter highlighting for new diagnostics.

Mitigation:

* no syntax shape change is required here, but keep tree-sitter tests green and
  update highlighting tests if identifier classification assumptions exist.

## Exit Criteria

1. Compiler enforces documented initial-case rules at declaration/binding sites.
2. Docs/spec/grammar and implementation are consistent.
3. Existing parser disambiguation behavior for constructors/fields is preserved.
4. Test suite passes with migrated fixtures.
