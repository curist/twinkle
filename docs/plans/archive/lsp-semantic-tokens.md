# LSP Semantic Tokens Plan

## Goal

Implement semantic highlighting with `textDocument/semanticTokens/full` so
editors can distinguish Twinkle symbols using compiler knowledge rather than
syntax alone.

---

## Scope

In scope:

* Types, type parameters, functions, parameters, locals, fields, variants,
  modules/import aliases, builtins, and keywords where useful.
* Full-document semantic tokens.
* Token modifiers such as declaration, readonly, documentation, builtin, and
  deprecated if applicable.

Out of scope for the first pass:

* Delta semantic tokens.
* Workspace-wide refresh requests.
* Theme-specific color choices.

---

## Design

Semantic token generation should combine parsed AST spans with resolved/type
information. The server advertises a fixed legend of token types and modifiers.
Responses use LSP's relative integer encoding.

Proposed token types:

* `namespace`
* `type`
* `typeParameter`
* `function`
* `method`
* `parameter`
* `variable`
* `property`
* `enumMember`
* `keyword`
* `string`
* `number`

Proposed modifiers:

* `declaration`
* `definition`
* `readonly`
* `defaultLibrary`

---

## Implementation Steps

1. Add semantic token params decoding.
2. Add token legend JSON in a new `boot/lib/lsp/semantic_tokens.tw` adapter.
3. Add a query module that walks AST/resolved info and emits absolute token
   ranges with token type/modifier ids.
4. Sort tokens, drop overlaps if needed, and encode relative deltas.
5. Advertise `semanticTokensProvider.full: true`.
6. Handle `textDocument/semanticTokens/full`.
7. Add tests for token kind and delta encoding.

---

## Test Plan

* Function declarations and call sites receive function tokens.
* Type declarations and type annotations receive type tokens.
* Parameters and locals are distinguished.
* Record fields and variants are distinguished.
* Builtins/prelude symbols receive `defaultLibrary` where available.
* Multibyte text before tokens produces correct UTF-16 positions.

---

## Exit Criteria

Editors with semantic-token support can provide meaning-aware highlighting for
Twinkle code without relying solely on tree-sitter scopes.
