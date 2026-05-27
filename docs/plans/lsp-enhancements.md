# LSP Enhancement Plan

## Goal

Track the next wave of `twk lsp` features beyond the current baseline. The
focus is editor-visible language intelligence that can be built on top of the
existing parser, query pipeline, document store, and semantic snapshot support.

---

## Current Baseline

Implemented today:

* `initialize`, `shutdown`, `exit`
* full document sync (`textDocumentSync: 1`)
* diagnostics publishing
* hover
* go to definition
* completion
* code actions for unused imports
* whole-document formatting (`textDocument/formatting`)

Range formatting and on-type formatting are intentionally out of scope for this
plan; whole-document format-on-save is good enough for now.

---

## Active LSP Plan Index

| Feature | LSP method/capability | Priority | Status | Details |
|---------|------------------------|----------|--------|---------|
| Document symbols | `textDocument/documentSymbol` | High | Planned | [lsp-document-symbols.md](lsp-document-symbols.md) |
| Find references | `textDocument/references` | High | Planned | [lsp-references.md](lsp-references.md) |
| Rename | `textDocument/rename`, `textDocument/prepareRename` | High | Planned | [lsp-rename.md](lsp-rename.md) |
| Signature help | `textDocument/signatureHelp` | High | Planned | [lsp-signature-help.md](lsp-signature-help.md) |
| Semantic tokens | `textDocument/semanticTokens/full` | Medium | Planned | [lsp-semantic-tokens.md](lsp-semantic-tokens.md) |
| Workspace symbols | `workspace/symbol` | Medium | Planned | [lsp-workspace-symbols.md](lsp-workspace-symbols.md) |
| Document highlight | `textDocument/documentHighlight` | Medium | Planned | [lsp-document-highlight.md](lsp-document-highlight.md) |
| Inlay hints | `textDocument/inlayHint` | Medium | Planned | [lsp-inlay-hints.md](lsp-inlay-hints.md) |
| Folding ranges | `textDocument/foldingRange` | Low | Planned | [lsp-folding-ranges.md](lsp-folding-ranges.md) |
| Incremental sync | `textDocumentSync: 2` | Low | Planned | [lsp-incremental-sync.md](lsp-incremental-sync.md) |

Existing related plan:

* [lsp-code-actions.md](lsp-code-actions.md) tracks additional quick fixes and
  source actions.

---

## Suggested Implementation Order

1. Document symbols: simple AST walk, high editor value, good foundation for
   workspace symbols.
2. Find references: establishes symbol identity and use-site collection needed
   for rename and highlights.
3. Rename: build on references with scope-aware edit generation.
4. Signature help: reuses type/signature rendering and call-site analysis.
5. Semantic tokens: improve syntax highlighting with compiler knowledge.
6. Inlay hints: useful once type/signature lookup helpers are stable.
7. Workspace symbols, document highlight, folding ranges, incremental sync as
   follow-up quality-of-life improvements.

---

## Shared Architecture Notes

Most features should follow the same shape:

1. Decode LSP params in `boot/lib/lsp/params.tw`.
2. Add a protocol adapter module under `boot/lib/lsp/` when JSON construction is
   non-trivial.
3. Add or extend query modules under `boot/compiler/query/` for compiler-facing
   logic.
4. Wire the request in `boot/lib/lsp/server_core.tw` and advertise the matching
   capability from `initialize`.
5. Add protocol-level tests under `boot/tests/suites/`.

Prefer query-layer implementations that work against a `SemanticSnapshot` so
features can reuse cached parsing, resolving, and type checking.

---

## Cross-Cutting Requirements

* Use UTF-16 LSP positions, matching the existing `positionEncoding`.
* Return empty/null responses rather than crashing when a document is missing,
  params fail to decode, or semantic analysis is unavailable.
* Prefer stale resolved/typed cache fallbacks where safe, matching completion.
* Keep edits minimal and deterministic.
* Add tests for multibyte text when ranges/positions are involved.
