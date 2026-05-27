# LSP Signature Help Plan

## Goal

Implement `textDocument/signatureHelp` so editors can show function and method
parameter information while users type calls.

---

## Scope

In scope:

* Function calls: `foo(a, b)`.
* Module-qualified calls: `module.foo(a)`.
* Method-call sugar: `value.method(a)` with receiver-aware signatures.
* Constructor and variant calls where signatures are available.
* Active parameter selection based on cursor position.

* Incomplete code while typing: source-scanning fallback when AST is unavailable.
* Fallback from failed AST resolution to source-scan (e.g. stale typed env).

Out of scope:

* Overload handling; Twinkle has no overloads, so one signature is expected.

---

## Design

At a cursor position, find the innermost call expression whose argument list
contains the cursor. Resolve the callee to a function/constructor signature,
render label and parameter labels, and compute `activeParameter` from comma
positions.

Method calls should display user-facing receiver style when possible, while
still using the underlying function signature for types.

Trigger characters:

* `(`
* `,`

Retrigger characters:

* `,`
* `)` may be useful but is optional.

---

## Implementation Steps

1. Add `SignatureHelpParams` decoding.
2. Add query helper to locate the active call at a byte offset.
3. Reuse or extend signature rendering from hover/completion.
4. Add `boot/lib/lsp/signature_help.tw` for response JSON.
5. Advertise `signatureHelpProvider` with trigger characters.
6. Handle `textDocument/signatureHelp` in `server_core.tw`.
7. Add tests for function, qualified function, method, and nested calls.

---

## Test Plan

* Cursor after `(` reports active parameter 0.
* Cursor after first comma reports active parameter 1.
* Nested calls choose the innermost containing call.
* Method calls include the expected receiver-adjusted labels.
* Unknown or unresolved callees return null.
* Multibyte text before the call maps cursor positions correctly.
* Incomplete code (no closing paren) triggers source-scan fallback.
* Incomplete code with commas tracks active parameter correctly.
* Incomplete nested calls resolve the innermost function.
* Incomplete module-qualified calls resolve via qualified name lookup.
* Complete calls that fail typed resolution fall back to source scan.

---

## Exit Criteria

Editors show accurate call signatures and active parameters for normal Twinkle
function, module-qualified, and method-call syntax — including while the user is
actively typing incomplete code.
