# LSP Find References — Done

Implemented `textDocument/references` for the Twinkle LSP server.

## What was built

* `boot/lib/lsp/params.tw` — `ReferenceParams` / `decode_references`
* `boot/compiler/query/references.tw` — core query: symbol identification,
  local and workspace reference collection with shadowing support
* `boot/lib/lsp/references.tw` — response adapter (ReferenceLocation → LSP JSON)
* `boot/lib/lsp/server_core.tw` — `handle_references` handler, `referencesProvider` capability

## Symbol kinds supported

Functions, top-level bindings, local bindings, parameters, types, variants,
record fields, and imports — both same-module and cross-module.

## Key design decisions

* **SymbolId enum** — `Local(name, span)`, `Func(mod, name)`, `TypeDef(mod, name)`,
  `Variant(mod, type, name)`, `Field(mod, type, name)`. Reuses go-to-definition
  for cursor-to-identity resolution.
* **RefCtx record** — bundles `uri`, `canonical_path`, `env`, `typed` to thread
  through recursive AST walkers.
* **Identity matching** — `matches_func` / `matches_type` / `matches_type_id`
  check origin-based matching first, then fall back to local (no-origin) matching
  using canonical module paths.
* **Cross-module coverage** — `handle_references` snapshots all open documents
  to populate the cache, same approach as workspace symbols.
* **Shadowing** — local reference collection stops at rebinding boundaries.

## Tests

15 tests in `boot/tests/suites/lsp_references_suite.tw` covering local variables,
functions, types, variants, fields, shadowing, `includeDeclaration`, capability
advertisement, cross-module functions, cross-module types, qualified type paths,
and record construction field references.
