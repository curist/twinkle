# LSP Incremental Sync — Done

## Goal

Move from full document sync to incremental text synchronization for better
editor performance with larger Twinkle files.

---

## What was built

* `boot/lib/lsp/params.tw` — `TextDocumentContentChangeEvent` extended with
  optional `range: TextRange?`; new `TextRange` type and `range_decoder()`.
* `boot/lib/lsp/document_store.tw` — `change_incremental()` splices text at
  byte offsets and rebuilds the line index.
* `boot/lib/lsp/server_core.tw` — `textDocumentSync` advertised as `2`
  (incremental); `handle_did_change` iterates content changes, applying
  range-based edits via UTF-16→byte offset conversion or full-text replacement.

## Design decisions

* Byte offset conversion is done in `handle_did_change` (server_core) using the
  existing `position_offset` helper, keeping `document_store` transport-agnostic.
* Multiple content changes in a single notification are applied sequentially,
  rebuilding the line index after each splice so subsequent ranges resolve
  correctly against updated text.
* Invalid ranges (out-of-bounds or missing document) are silently skipped to
  avoid corrupting stored text.
* Full-text changes (no `range` field) continue to work as before — clients
  that send full text are unaffected.

## Tests

* `lsp_document_store_suite` — incremental insert, delete, cross-line replace,
  invalid range safety, unknown document safety.
* `lsp_server_core_suite` — incremental didChange via protocol, multiple
  sequential incremental changes, capability advertisement updated to `2`.
