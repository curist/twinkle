# LSP Rename Plan

> **Status: shipped (boot).** `prepareRename`/`rename` are implemented in
> `compiler/query/rename.tw` and `lib/lsp/rename.tw`, advertised as
> `renameProvider { prepareProvider: true }`. Renamability is gated on a
> user-defined declaration (builtins/stdlib refused), names are validated against
> the parser's conventions, and edits cover references plus the declaration
> across project-local modules. The out-of-scope items below (file/module path
> renames, cross-package rename) remain future work.

## Goal

Implement safe symbol rename through `textDocument/prepareRename` and
`textDocument/rename`.

---

## Scope

In scope:

* Local variables, parameters, top-level functions/bindings, user-defined types,
  variants, record fields, import aliases, and destructured import names.
* Workspace edits across project-local modules.
* Validation of Twinkle naming conventions before edits are returned.

Out of scope for the first pass:

* File/module path renames.
* Cross-package rename.
* Rename of builtins or standard-library symbols.

---

## Design

Rename should build directly on the symbol identity and reference collection
from [lsp-references.md](lsp-references.md). `prepareRename` verifies that the
cursor is on a renamable symbol and returns the editable name range. `rename`
validates the new name, finds all references, and returns a `WorkspaceEdit`.

Rename depends on the project-configuration work in [tooling.md](tooling.md): the
LSP needs a stable project graph before it can promise complete workspace edits.
For configured projects, reference collection should analyze the union of
`[project].entries` and `[test].entries` reachable modules, with open documents
as overlays. For projects without configured entries, keep the current open-doc
fallback and avoid advertising rename as complete-project safe.

Validation rules:

* Values, functions, fields, modules: lowercase/snake-case identifier start.
* Types and variants: uppercase/PascalCase identifier start.
* Reject keywords and invalid lexer tokens.
* Reject builtins, stdlib definitions, and unresolved/error symbols.

---

## Implementation Steps

0. Land the project-config/LSP workspace-root prerequisite from
   [tooling.md](tooling.md), including configured `[project].entries` and
   `[test].entries` as workspace roots.
1. Add `PrepareRenameParams` and `RenameParams` decoders.
2. Add identifier validation helpers, ideally shared with parser naming rules.
3. Implement `prepareRename` using symbol-at-position.
4. Implement `rename` using reference collection and workspace edit builders.
5. Advertise `renameProvider` with prepare support only when workspace roots are
   known well enough for deterministic edits, or document the fallback limits.
6. Add tests covering successful and rejected renames.

---

## Test Plan

* Local rename respects shadowing.
* Function rename updates declaration, calls, method references where supported,
  and imports.
* Type rename updates declaration, annotations, constructors, and imports.
* Variant rename updates declaration, expressions, and patterns.
* Field rename is record-type-aware.
* Invalid names are rejected with an LSP error response.
* Builtin and stdlib symbols are not renamable.

---

## Exit Criteria

Rename returns deterministic workspace edits for common user-defined symbols and
refuses unsafe or invalid renames before modifying editor buffers.
