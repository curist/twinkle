# LSP Document Symbols Plan

## Goal

Implement `textDocument/documentSymbol` so editors can show a file outline and
quickly navigate top-level Twinkle declarations.

---

## Scope

In scope:

* Top-level `use`, `type`, `fn`, `extern type`, and `extern fn` symbols.
* Top-level executable statements as optional anonymous entries only if useful.
* Record fields and enum variants as children of type declarations.
* Function parameters and local bindings are out of scope for the first pass.

---

## Design

Add a query helper that walks the parsed module AST and emits symbol records:

* name
* kind
* full range
* selection range
* optional children

Expose these through a small LSP adapter that maps Twinkle symbol kinds to LSP
`SymbolKind` integers.

Suggested kind mapping:

* module/import: `Module` or `Namespace`
* function/extern function: `Function`
* type declarations: `Struct`, `Enum`, or `TypeParameter` depending on def
* record fields: `Field`
* enum variants: `EnumMember`

---

## Implementation Steps

1. Add `DocumentSymbolParams` decoding in `boot/lib/lsp/params.tw`.
2. Add `boot/compiler/query/document_symbols.tw` for AST-to-symbol extraction.
3. Add `boot/lib/lsp/document_symbol.tw` for JSON response construction.
4. Advertise `documentSymbolProvider: true` from `initialize`.
5. Handle `textDocument/documentSymbol` in `server_core.tw`.
6. Add tests in a new `lsp_document_symbol_suite.tw`.

---

## Test Plan

* Top-level functions and types appear in source order.
* Record fields are children of record types.
* Sum variants are children of enum/sum types.
* Selection ranges point at names, not whole declarations.
* Multibyte text before a declaration still maps ranges correctly.
* Unknown documents return an empty result.

---

## Exit Criteria

Editors can display a useful outline for normal Twinkle source files, including
nested record fields and variants under their parent types.
