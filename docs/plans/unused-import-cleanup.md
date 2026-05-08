# Unused Import Cleanup — Shared Lib + LSP Code Action

**Status:** Planned

## Context

We added unused import detection to the boot compiler (`boot/compiler/unused_imports.tw`).
It currently produces `Diagnostic` warnings with spans, consumed by both the CLI
build path (`module_compiler.tw`) and the LSP diagnostics path (`query/analyze.tw`).

The next step is to make it actionable: a shared library that produces **edit
operations** (not just warnings), so both the LSP (`textDocument/codeAction`) and
a future CLI fix command can remove unused imports automatically.

## Current State

| File | Role |
|------|------|
| `boot/compiler/unused_imports.tw` | Detects unused imports, returns `Vector<Diagnostic>` |
| `boot/compiler/module_compiler.tw` | Prints warnings to stderr during builds (runs detection separately) |
| `boot/compiler/query/analyze.tw` | Runs detection, feeds warnings into LSP diagnostics |
| `boot/lib/lsp/server_core.tw` | LSP server — hover + definition only, no code actions |
| `boot/lib/lsp/diagnostics.tw` | Publishes diagnostics with severity to LSP client |

## Design Decisions

### Comma-safe edits via statement-level reconstruction

Surgically removing a single item from `use foo.{A, B, C}` requires handling
commas and whitespace — getting this wrong produces `use foo.{A, , C}`. Instead
of tracking comma spans, we operate at the **statement level**:

- **All items unused** → delete the entire `use` line
- **Some items unused** → emit a replacement `TextEdit` that rewrites the whole
  `use` statement with only the used items retained (e.g., `use foo.{A, C}`)

This avoids overlapping edits and comma surgery entirely. The `unused_imports`
module already has both the `UseDecl` (with all items) and the used-name set,
so reconstructing the filtered statement is straightforward.

### Roundtrip edit info via `diagnostic.data`

The LSP spec allows a `data` field on published diagnostics that the client
sends back verbatim in code action requests. We attach removal info (the
`use_span` and reconstructed text) as JSON in `diagnostic.data`. This means:

- **No extra state** — the code action handler reads everything from the
  incoming request's `context.diagnostics`, no need to store
  `UnusedImportResult` in `State` or thread it through analysis types
- **No correlation logic** — no matching diagnostics by span to find the
  corresponding edit

### Single canonical detection site

Currently `check_unused_imports` runs in both `analyze.tw` (LSP path) and
`module_compiler.tw` (CLI build path). We keep `analyze.tw` as the canonical
call site and remove the duplicate loop from `module_compiler.tw`. The CLI
build path reads warnings from the analysis result's diagnostics instead.

## Plan

### 1. Extend `unused_imports.tw` — return edit info

Add types for structured edit data alongside diagnostics:

```twinkle
pub type ImportEdit = .{
  /// Byte span of the entire `use` statement (including trailing newline)
  use_span: span.Span,
  /// Replacement text: "" to delete, or reconstructed `use` with unused items removed
  replacement: String,
}

pub type UnusedImportDiag = .{
  diagnostic: Diagnostic,
  edit: ImportEdit,
}

pub type UnusedImportResult = .{
  items: Vector<UnusedImportDiag>,
}
```

Change `check_unused_imports(module) -> UnusedImportResult`.

Each `UnusedImportDiag` pairs a warning diagnostic (for display) with an
`ImportEdit` (for removal). The edit always covers the full `use` statement:

- **Full import unused** (`use foo.bar`): `replacement = ""`
- **All selective items unused** (`use foo.{A, B}`): `replacement = ""`
- **Some selective items unused** (`use foo.{A, B, C}` with `B` unused):
  `replacement = "use foo.{A, C}\n"` — reconstructed from the AST

The reconstruction function (`reconstruct_use_decl`) rebuilds the `use`
statement text from the `UseDecl` AST node, filtering out unused items.

### 2. Attach edit info to published diagnostics via `data`

In `boot/lib/lsp/diagnostics.tw`, when building diagnostic JSON, include the
`data` field with the edit payload:

```json
{
  "range": ...,
  "severity": 2,
  "source": "twinkle",
  "message": "unused import: B",
  "data": {
    "kind": "unused_import",
    "use_start": 0,
    "use_end": 25,
    "replacement": "use foo.{A, C}\n"
  }
}
```

The existing `diagnostic_to_json` function takes `query_diagnostics.Diagnostic`.
We extend that type (or add a parallel path) to carry optional `data: Json?`.

**Files**: `boot/lib/lsp/diagnostics.tw`, `boot/compiler/query/diagnostics.tw`

### 3. Add LSP code action support

#### 3a. Code action JSON builders — `boot/lib/lsp/code_action.tw` (new)

Helpers to build LSP JSON for:
- `TextEdit` (range + newText)
- `WorkspaceEdit` (changes: `{ uri: [TextEdit] }`)
- `CodeAction` (title, kind=`"quickfix"`, edit, diagnostics)

Follows the existing pattern of `diagnostics.tw`, `hover.tw`, `definition.tw`.

#### 3b. Advertise capability — `boot/lib/lsp/server_core.tw`

Add `"codeActionProvider": true` to the initialize response capabilities.

#### 3c. Handle `textDocument/codeAction` — `boot/lib/lsp/server_core.tw`

New handler that:
1. Decodes request params (textDocument, range, context)
2. Iterates `context.diagnostics`, filters for those with
   `data.kind == "unused_import"`
3. For each, builds a `CodeAction` with a `WorkspaceEdit` using the
   `use_start`/`use_end`/`replacement` from `data`
4. When multiple unused imports share the same `use_span`, dedup into a
   single edit (the reconstructed statement already accounts for all
   unused items in that statement)
5. When >1 unused import exists, also offer a "Remove all unused imports"
   bulk action that combines all edits

#### 3d. Param decoder — `boot/lib/lsp/params.tw`

Add `decode_code_action` to extract `textDocument`, `range`, and
`context.diagnostics` (including `data` fields) from the request params.

### 4. Consolidate CLI build path

Remove the separate `check_unused_imports` loop from `module_compiler.tw`.
Instead, after `analyze_module` succeeds, read import warnings from
`a_result.diagnostics` and print them. The analysis path in `analyze.tw`
already runs detection and includes warnings in its diagnostics.

**File**: `boot/compiler/module_compiler.tw`

### 5. Update analysis path

In `query/analyze.tw`, update the `check_unused_imports` call to use the new
`UnusedImportResult` return type. Extract `.items[].diagnostic` for the
diagnostic list. For the LSP path, also propagate the `ImportEdit` data so
it can be attached to published diagnostics (via a new optional field on
`AnalysisDiag`).

**File**: `boot/compiler/query/analyze.tw`

## Files to modify

| File | Change |
|------|--------|
| `boot/compiler/unused_imports.tw` | New types, return `UnusedImportResult`, add `reconstruct_use_decl` |
| `boot/lib/lsp/code_action.tw` | **New** — JSON builders for CodeAction/TextEdit/WorkspaceEdit |
| `boot/lib/lsp/server_core.tw` | Add codeAction capability + handler |
| `boot/lib/lsp/params.tw` | Add `decode_code_action` param decoder |
| `boot/lib/lsp/diagnostics.tw` | Attach `data` field to diagnostic JSON |
| `boot/compiler/query/analyze.tw` | Use new result type, propagate edit data |
| `boot/compiler/query/diagnostics.tw` | Add optional `data` field to `Diagnostic`/`AnalysisDiag` |
| `boot/compiler/module_compiler.tw` | Remove duplicate detection loop, read from analysis result |

## Verification

1. `make bundle-cli` — self-host loop passes
2. `target/twk build /tmp/test_unused.tw` — still prints warnings correctly
3. Open a `.tw` file with unused imports in an editor with LSP:
   - Yellow squiggles appear on unused imports
   - Quick fix action "Remove unused import 'X'" appears
   - Applying the action removes the import line or rewrites the item list
   - "Remove all unused imports" appears when multiple exist
4. Edge cases to test:
   - `use foo.{A}` where `A` unused → removes whole line
   - `use foo.{A, B, C}` where `B` unused → rewrites to `use foo.{A, C}`
   - `use foo.{A, B, C}` where all unused → removes whole line
   - `use foo.bar` where `bar` unused → removes whole line
   - `use foo.bar as baz` where `baz` unused → removes whole line
