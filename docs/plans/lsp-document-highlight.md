# LSP Document Highlight Plan

## Goal

Implement `textDocument/documentHighlight` so editors can highlight all uses of
the symbol under the cursor within the current document.

---

## Scope

In scope:

* Same symbol kinds as find references, limited to the current document.
* Read/write distinction where the AST can identify rebinding targets.

Out of scope for the first pass:

* Cross-file highlights; use find references for that.
* Highlighting all textual matches when semantic resolution fails.

---

## Design

Document highlight should reuse the symbol-at-position and reference collection
from [lsp-references.md](lsp-references.md), filtering results to the current
URI. Each result maps to `DocumentHighlight` with kind:

* `Text` for neutral references
* `Read` for expression/type uses
* `Write` for declaration and rebinding targets where known

---

## Implementation Steps

1. Add `DocumentHighlightParams` decoding.
2. Add a helper that collects current-document references for the symbol under
   cursor.
3. Add JSON response helpers under `boot/lib/lsp/document_highlight.tw`.
4. Advertise `documentHighlightProvider: true`.
5. Handle `textDocument/documentHighlight` in `server_core.tw`.
6. Add tests for locals, top-level functions, types, fields, and shadowing.

---

## Test Plan

* Local highlights respect lexical scope and shadowing.
* Top-level declarations and references are highlighted together.
* Record field highlights are type-aware.
* Cursor on whitespace or unknown document returns an empty result.
* Rebinding/write sites use `Write` when detectable.

---

## Exit Criteria

Editors highlight semantically matching occurrences of the cursor symbol within
the current Twinkle document.
