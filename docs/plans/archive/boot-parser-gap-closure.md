# Boot Parser Gap-Closure Plan

## Goal

Move the current bootstrap parser from an item-outline scaffold to a true
error-recovering frontend parser aligned with the self-hosting plan:

- richer token coverage
- real AST nodes for type/function/statement bodies
- robust boundary detection and recovery

This plan covers the concrete gaps observed in the current `boot/compiler/*`
implementation.

---

## Current Gaps

### G1 — Token/Lexer Coverage Is Incomplete

Missing or partial support:

- multi-char operators: `==`, `!=`, `<=`, `>=`, `&&`, `||`, `..`
- float literals (`3.14`)
- broader keyword set (`if`, `else`, `case`, `for`, `in`, `collect`,
  `return`, `break`, `continue`, `true`, `false`, `try`, `and`, `or`, etc.)
- string interpolation tokenization (`"${...}"` segments and boundaries)

### G2 — AST Is Outline-Level, Not Recursive

Current AST stores:

- function body as `Span` only
- return type as opaque `String?`
- type declarations without parsed bodies (record/sum/alias detail absent)
- top-level statements as raw text

This blocks downstream resolver/typechecker/lsp work that depends on structured
expression/type trees.

### G3 — Top-Level Boundary Heuristic Is Unsound

`consume_to_item_boundary` currently splits at newline when depth is zero. This
can incorrectly split multiline expressions/statements (`x +` newline `y`).

### G4 — `find_function_body_open` Helper Is Narrow

The helper currently special-cases `.{ ... }` to avoid confusing record-type
syntax with function body opening braces, but does not model general type syntax
forms. It is currently safe enough for `parse_function` only, but fragile if
reused elsewhere.

### G5 — `parse_type` Is Non-Structural

`parse_type` captures only declaration header (`pub type Name`) and skips the
rest heuristically; no type parameters, no definition-kind parse, no
field/variant capture.

### G6 — `join_token_text` Is Lossy

Joining tokens with normalized spaces is acceptable for temporary debug output
but unsuitable for semantic parsing (types/exprs cannot remain as normalized
strings).

### G7 — Import Parsing Lacks Destructuring Form

Need support for stage0-compatible destructuring imports:

- `use foo.bar.{x, y}`
- mixed value/type imports and aliases inside braces

---

## Scope

In scope:

- lexer/token parity needed for parser phase
- structural AST expansion for type/function/statement/expression parsing
- reliable boundary/recovery logic
- import destructuring parsing
- targeted parser tests for each gap

Out of scope:

- resolver/typechecker semantics changes
- codegen/runtime changes
- full LSP index implementation

---

## Milestones

### M1 — Lexer/Token Foundation

Deliverables:

- add missing token kinds
- implement multi-char operator lexing
- add float literal scanning
- add missing keyword recognition
- introduce interpolation-aware string tokenization

Acceptance:

- parser no longer needs placeholder `String`/`Stmt` token-text hacks for new
  syntax forms
- lexer test suite includes positive + malformed cases for each new token class

### M2 — Structural AST for Types and Functions

Deliverables:

- parse full `TypeDecl`:
  - optional `pub`
  - name + type params
  - definition kind: record/sum/alias
- parse `FunctionDecl` with:
  - typed parameter nodes
  - parsed return type AST
  - parsed block/expression body tree (not span-only)

Acceptance:

- no `return_type: String?` / body-span-only representation for parsed function
  declarations
- no heuristic skipping of type bodies

### M3 — Expression/Statement Parser Core

Deliverables:

- precedence-based expression parser
- statement parsing for `let`, `if`, `case`, loops, `return`, `break`,
  `continue`, `defer`, expression statements
- top-level statement parsing via structure, not raw `StmtItem.text`

Acceptance:

- multiline expressions parse as single statements where valid
- `consume_to_item_boundary` no longer acts as primary statement parser

### M4 — Recovery and Boundary Hardening

Deliverables:

- improve synchronization points (`newline`, `;`, `}`, top-level starters) with
  context awareness
- restrict `find_function_body_open` usage or replace with grammar-driven parse
- add comments/docs calling out helper assumptions where retained

Acceptance:

- malformed constructs yield localized diagnostics without cascading whole-file
  collapse
- helper behavior is explicit and tested

### M5 — Import Destructuring Support

Deliverables:

- parse `use foo.bar.{...}` item lists
- support value imports, `type` imports, aliases in item lists
- validate/diagnose invalid combinations cleanly

Acceptance:

- parity with stage0 import forms used by existing code/tests

---

## Test Plan

Add/expand parser tests to cover:

1. tokenization parity for newly added operators/keywords/literals/interpolation
2. multiline statement continuity (`x +` newline `y`)
3. full type declaration shapes (record/sum/alias with generics)
4. function declarations with non-trivial return types and bodies
5. import destructuring forms and error recovery
6. malformed inputs with expected diagnostic counts/locations

---

## Risks and Mitigations

- Risk: feature-by-feature lexer expansion drifts from parser expectations.
  Mitigation: land tokens and parser productions in small paired slices.

- Risk: recovery logic becomes ad hoc and brittle.
  Mitigation: define explicit synchronization strategy and test it directly.

- Risk: staged AST migrations break downstream boot code.
  Mitigation: migrate through additive AST variants and remove placeholders only
  after call sites switch.

---

## Exit Criteria

This plan is complete when:

1. parser emits structured AST (no span-only function bodies or raw stmt text
   placeholders for core forms),
2. top-level multiline statements are no longer split by newline heuristics,
3. `type` declarations and destructuring imports parse structurally,
4. lexer/parser tests cover the listed gaps and pass consistently.
