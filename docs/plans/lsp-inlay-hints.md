# LSP Inlay Hints Plan — **Done**

## Goal

Implement `textDocument/inlayHint` for lightweight inferred information in the
editor.

---

## Scope

Implemented:

* Inferred type hints for local `:=` bindings when the type is not obvious
  (skips literal ints, floats, strings, bools).
* Parameter name hints at call sites for multi-arg functions (skips when the
  argument variable already matches the parameter name).

Deferred to future work:

* Return type hints for functions without explicit return annotations.
* Hints for every expression.
* Interactive hint resolve commands.
* User configuration plumbing.

---

## Design

Uses the typed semantic snapshot to locate inferred types and function
signatures. Walks the AST within the requested byte range, emitting conservative
hints to avoid visual noise.

Rules:

* Show `: Type` after local binding names only when the binding uses `:=` and
  the initializer is not a literal with an obvious primitive type.
* Show parameter name hints for positional arguments when a function has named
  parameters and the argument is not already a named/local variable matching the
  parameter. Single simple-arg calls are suppressed to reduce noise.

---

## Implementation

1. `boot/lib/lsp/params.tw` — `InlayHintParams` type and `decode_inlay_hint`.
2. `boot/compiler/query/inlay_hints.tw` — AST walker producing `InlayHint`
   values (type hints and parameter hints) within a byte range.
3. `boot/lib/lsp/inlay_hint.tw` — JSON response adapter with LSP kind codes
   (1=Type, 2=Parameter) and padding flags.
4. `boot/lib/lsp/server_core.tw` — `inlayHintProvider: true` capability,
   `textDocument/inlayHint` dispatch and handler.
5. `boot/tests/suites/lsp_inlay_hint_suite.tw` — tests covering type hints,
   obvious-literal suppression, annotated-binding suppression, parameter hints,
   param-name-matching suppression, unknown documents, and capability
   advertisement.

---

## Test Plan — Covered

* Inferred local binding hints display stable type strings.
* Explicitly annotated bindings do not get duplicate type hints.
* Obvious literals (int, float, string, bool) are suppressed.
* Parameter hints appear at multi-arg call sites.
* Parameter hints suppressed when arg matches param name.
* Unknown or untyped documents return an empty result.
* Capability advertised in initialize response.
