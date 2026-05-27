# LSP Incremental Sync Plan

## Goal

Move from full document sync to incremental text synchronization when it becomes
worthwhile for larger Twinkle files and projects.

---

## Current Baseline

The server advertises `textDocumentSync: 1` and expects full document text in
`textDocument/didChange`. This is simple, robust, and good enough for typical
files today.

---

## Scope

In scope:

* Advertise incremental sync with `textDocumentSync: 2` or an equivalent options
  object.
* Decode `TextDocumentContentChangeEvent` with optional range/rangeLength.
* Apply UTF-16 LSP ranges to the stored document text.
* Continue accepting full-text changes for clients that send them.

Out of scope for the first pass:

* Incremental compiler analysis. This plan only changes transport/document
  storage; compiler queries may still reparse whole documents.

---

## Design

Extend `document_store.change_full_text` with an incremental update path:

1. Convert the change range from UTF-16 positions to byte offsets using the
   current document's `LineIndex`.
2. Replace the byte slice with the incoming text.
3. Rebuild the line index for the updated document.
4. Apply multiple content changes in order.

If any range is invalid, ignore the change or fall back to a safe error path
without corrupting stored text.

---

## Implementation Steps

1. Extend params decoding for optional `range` and `rangeLength`.
2. Add `document_store.change_incremental`.
3. Update `handle_did_change` to process both full and incremental changes.
4. Advertise incremental sync.
5. Add tests for single-line, multiline, insertion, deletion, replacement, and
   multibyte edits.

---

## Test Plan

* Full-text changes still work.
* Incremental insertion updates text and line index.
* Incremental deletion across lines updates text and line index.
* Multibyte UTF-16 positions map to correct byte offsets.
* Invalid ranges do not corrupt document state.

---

## Exit Criteria

The LSP server can safely accept incremental document changes from editors while
preserving the current full-sync behavior as a compatibility path.
