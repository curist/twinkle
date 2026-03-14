# Record Literal Field Punning Plan

## Goal

Add record field punning syntax so repeated `name: name` pairs can be written
as `name` in record literals.

Examples:

```tw
// before
user := User.{ name: name, age: age }
v: Vec2 = .{ x: x, y: y }

// after
user := User.{ name, age }
v: Vec2 = .{ x, y }
```

This is purely a surface-syntax ergonomics feature. Core typing and runtime
behavior should remain unchanged.

---

## Current Baseline (2026-03-14)

Current parser behavior requires `Identifier ":" Expr` for every record field.

Current accepted forms:

* anonymous record literal: `.{ x: 1, y: 2 }`
* named record literal: `Point.{ x: 1, y: 2 }`

Current implementation points:

* parser record literals: `src/syntax/parser.rs` (`parse_record_literal`)
* AST representation: `ExprKind::RecordLit { name, fields }` in `src/syntax/ast.rs`
* tree-sitter grammar: `tree-sitter-twinkle/grammar.js` (`record_field`)
* type checking and lowering consume `(field_name, Expr)` pairs unchanged

---

## Scope

In scope:

* record literal field punning in both anonymous and named record literals
* mixed forms in one literal (`.{ x, y: 2 }`)
* parser, `tree-sitter-twinkle/grammar.js`, tests, and language docs updates
* LSP go-to-definition behavior validation for shorthand fields

Out of scope:

* pattern punning/destructuring shorthand
* import/destructuring shorthand
* any type-system or IR/runtime representation changes

---

## Proposed Syntax and Semantics

Grammar change (conceptual):

* from: `RecordField = Identifier ":" Expr`
* to: `RecordField = Identifier [ ":" Expr ]`

Semantic rule:

* `.{ name }` desugars to `.{ name: name }`
* `Point.{ x, y: value }` desugars to `Point.{ x: x, y: value }`

Name resolution/type checking:

* shorthand value identifier is resolved like any normal expression identifier
* unresolved shorthand names produce existing undefined-variable diagnostics
* record field presence/type checks are unchanged

---

## Implementation Plan

### P1 — Parser Support

Files:

* `src/syntax/parser.rs`

Changes:

* In `parse_record_literal`, after reading field name:
  * if `:` exists, parse explicit value expression (current behavior)
  * otherwise synthesize value expression `ExprKind::Ident(field_name)` using
    the field token span
* keep existing comma/trailing-comma behavior
* allow mixed explicit and shorthand fields in one literal

Acceptance:

* parser accepts `.{ x }`, `.{ x, y }`, `Point.{ x, y: 1 }`
* parser output remains `ExprKind::RecordLit { fields: Vec<(String, Expr)> }`

### P2 — Tree-sitter Grammar Support

Files:

* `tree-sitter-twinkle/grammar.js`
* regenerated parser artifacts under `tree-sitter-twinkle/src/` (if required by workflow)
* highlight/corpus tests as needed

Changes:

* update `record_field` rule to make `':' value` optional
* ensure highlight/corpus expectations still classify record field names correctly

Acceptance:

* syntax tree distinguishes field name and optional explicit value
* grammar tests pass for both explicit and shorthand forms
* no highlight regression for `record_field` names

### P3 — Tests

Files (likely):

* `src/syntax/parser.rs` parser tests
* `tests/run/` new or extended runtime/compile tests
* `tests/typecheck/` pass/fail fixtures for shorthand edge cases
* `tests/lsp_definition_test.rs` go-to-definition coverage for shorthand

Coverage targets:

* parse success:
  * anonymous shorthand
  * named shorthand
  * mixed shorthand + explicit value
  * trailing commas
* typecheck success:
  * inferred/annotated contexts with shorthand
* typecheck failure:
  * shorthand name not bound (`.{ missing }`)
* LSP definition:
  * cursor on shorthand field value token in `.{ x }` resolves to binding of `x`
* regression:
  * explicit `name: expr` behavior unchanged

### P4 — Documentation

Files:

* `docs/spec.md`
* `docs/grammar.ebnf`
* optional: `docs/design/records.md` (if it mirrors literal syntax examples)

Changes:

* add shorthand examples and desugaring note
* update formal grammar in EBNF docs

Acceptance:

* spec and grammar show both explicit and shorthand forms
* examples are consistent with parser behavior

### P5 — LSP / Tooling Validation

Rationale:

* go-to-definition currently resolves identifier references from expression spans.
  Shorthand should work if parser desugaring creates an `ExprKind::Ident` whose
  span matches the shorthand token.

Files:

* `src/lsp/definition.rs` (only if behavior needs adjustment)
* `tests/lsp_definition_test.rs`

Checks:

* add a definition test for shorthand in both anonymous and named record literals
* verify cursor on shorthand token resolves to the local/parameter binding
* if resolution fails, adjust expression span selection logic to prioritize the
  shorthand identifier expression at cursor offset

Acceptance:

* go-to-definition on shorthand fields behaves like `name: name` explicit form
* no regression for existing definition scenarios

---

## Risks and Mitigations

* Risk: parser ambiguity or reduced readability in mixed literals.
  Mitigation: limit shorthand to omitted value only; keep field-name token
  requirements unchanged.
* Risk: users may assume destructuring shorthand exists in patterns.
  Mitigation: explicitly document that this change applies only to record
  literals.
* Risk: tooling mismatch (compiler parser vs tree-sitter parser).
  Mitigation: update both in the same change and include corpus/highlight tests.
* Risk: shorthand token may not map cleanly to identifier-expression span for LSP.
  Mitigation: add dedicated definition tests and adjust LSP span selection only if needed.

---

## Exit Criteria

1. `name: name` shorthand compiles in named and anonymous record literals.
2. Existing explicit record literal syntax remains fully compatible.
3. Compiler parser tests and tree-sitter tests cover shorthand and mixed forms.
4. LSP go-to-definition works on shorthand field tokens (or is explicitly fixed).
5. `docs/spec.md` and `docs/grammar.ebnf` document the final syntax.
