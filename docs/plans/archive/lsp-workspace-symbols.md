# LSP Workspace Symbols Plan

## Goal

Implement `workspace/symbol` so users can search functions, types, variants,
and other exported symbols across a Twinkle project.

---

## Scope

In scope:

* Project-local modules reachable from open documents and project root.
* Exported functions and types.
* Variants and record fields if the result remains useful.
* Fuzzy or case-insensitive query matching.

Out of scope for the first pass:

* External package indexes.
* Partial results / streaming work-done progress.
* Workspace symbol resolve.

---

## Design

Reuse document-symbol extraction over all known project modules. Results should
include the symbol name, kind, container/module name, and location.

Candidate module sources:

* Current semantic workspace graph for open documents.
* Project modules discovered by the existing module loader.
* Open overlay documents should override disk contents.

---

## Implementation Steps

1. Add `WorkspaceSymbolParams` decoding in `params.tw`.
2. Add a query module that gathers project symbols from parsed modules.
3. Add a matcher for simple substring/fuzzy matching.
4. Add JSON response helpers under `boot/lib/lsp/workspace_symbol.tw`.
5. Advertise `workspaceSymbolProvider: true`.
6. Handle `workspace/symbol` in `server_core.tw`.
7. Add tests using multiple open documents in the same temporary project.

---

## Test Plan

* Query finds exported symbols from another module.
* Query respects open-document overlay text.
* Empty query either returns a bounded useful set or an empty result.
* Results contain correct URIs, ranges, and container names.
* Unknown or invalid workspace state returns an empty result.

---

## Exit Criteria

Editor “go to symbol in workspace” can find project-local exported functions,
types, and variants with correct navigation locations.
