# LSP Inlay Hints Plan

## Goal

Implement `textDocument/inlayHint` for lightweight inferred information in the
editor.

---

## Scope

In scope:

* Inferred type hints for local `:=` bindings when the type is not obvious.
* Optional return type hints for public or top-level functions without explicit
  return annotations.
* Parameter name hints at call sites where arguments are not self-explanatory.

Out of scope for the first pass:

* Hints for every expression.
* Interactive hint resolve commands.
* User configuration plumbing; start with conservative defaults.

---

## Design

Use the typed semantic snapshot to locate inferred types and function
signatures. Emit conservative hints to avoid visual noise.

Potential rules:

* Show `: Type` after local binding names only when the binding uses `:=` and
  the initializer is not a literal with an obvious primitive type.
* Show `: Type` for top-level bindings if they become part of the module API.
* Show parameter name hints for positional arguments when a function has named
  parameters and the argument is not already a named/local variable matching the
  parameter.

---

## Implementation Steps

1. Add `InlayHintParams` decoding, including the requested range.
2. Add typed AST/range query for candidate hints.
3. Add type/signature rendering helpers or reuse hover rendering.
4. Add JSON response helpers under `boot/lib/lsp/inlay_hint.tw`.
5. Advertise `inlayHintProvider: true`.
6. Handle `textDocument/inlayHint` in `server_core.tw`.
7. Add tests for type hints, parameter hints, and range filtering.

---

## Test Plan

* Inferred local binding hints display stable type strings.
* Explicitly annotated bindings do not get duplicate type hints.
* Parameter hints appear at the correct argument positions.
* Requested LSP range filters returned hints.
* Unknown or untyped documents return an empty result.

---

## Exit Criteria

Editors can show useful inferred type and parameter hints while avoiding noisy
or redundant hints in common Twinkle code.
