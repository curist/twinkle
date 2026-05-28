# LSP Folding Ranges Plan

## Goal

Implement `textDocument/foldingRange` so editors can fold Twinkle declarations,
blocks, imports, and comments.

---

## Scope

In scope:

* Function bodies.
* Type declarations with multiline record fields or variants.
* Extern blocks as formatted/grouped by source structure.
* `case`, `if`, `for`, `cond`, closure, record, vector, and dict literal blocks
  when multiline.
* Consecutive import groups and comment blocks where spans are available.

Out of scope for the first pass:

* Folding ranges that depend on formatter output rather than source spans.
* Region pragmas.

---

## Design

Folding ranges can be produced from AST spans plus token trivia. Only return
ranges spanning more than one line. Prefer folding the interior of delimiters so
editors keep the opening line visible.

Use LSP folding range kinds where appropriate:

* `imports` for import blocks
* `comment` for comment groups
* omit kind for code blocks

---

## Implementation Steps

1. Add `FoldingRangeParams` decoding.
2. Add an AST/trivia folding collector query.
3. Add JSON response helpers under `boot/lib/lsp/folding_range.tw`.
4. Advertise `foldingRangeProvider: true`.
5. Handle `textDocument/foldingRange` in `server_core.tw`.
6. Add tests covering declarations, nested blocks, imports, comments, and
   single-line constructs.

---

## Test Plan

* Multiline functions and types produce ranges.
* Single-line declarations do not produce ranges.
* Consecutive imports can fold as an imports range.
* Consecutive comments can fold as a comment range.
* Nested ranges are stable and sorted.

---

## Exit Criteria

Editors can fold common multiline Twinkle constructs without incorrect or noisy
single-line ranges.
