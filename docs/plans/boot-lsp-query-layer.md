# Boot LSP Query Layer Plan

## Goal

Evolve the boot compiler query layer into the shared foundation for LSP
workspace diagnostics and later editor features.

The immediate target is to move from single open-document diagnostics to
workspace-aware module analysis that can use open editor buffers, read unopened
dependencies from disk, cache pure compiler stages, and report diagnostics for
all affected modules.

---

## Current Baseline

Implemented groundwork:

- LSP framed stdio transport and JSON-RPC message handling.
- LSP document lifecycle handling for open/change/close.
- In-memory document store with source identities and overlay export.
- Pure diagnostics query for one in-memory document.
- Workspace diagnostics query that resolves imports, checks dependencies in
  semantic order, uses open-buffer overlays, and reports dependency failures as
  diagnostics.
- Query cache and stage runner foundation for parse/resolve/typecheck/lower.
- Dependency graph storage with reverse-dependency invalidation helpers.
- Boot module compiler partially uses query stages.
- LSP publishes workspace diagnostics for open documents on lifecycle events.
- Framed stdio smoke coverage exists for valid diagnostics, invalid diagnostics,
  workspace import diagnostics, dependency edits affecting importers, clearing
  diagnostics, and shutdown.

Current limitation:

- LSP workspace publishing is conservative: it rechecks open documents as roots
  instead of using a precise affected-module set.
- Diagnostics for unopened files are kept internal until those files open.
- Cache invalidation tracks source changes and reverse dependencies, but LSP
  publishing still rechecks all open roots instead of scheduling only the
  affected open modules.

---

## Target Architecture

The query layer should provide a small, explicit workspace API that higher-level
LSP features can reuse:

```tw
workspace_snapshot(open_documents, entry_uri)
  -> source graph + overlays + canonical module identities

workspace_diagnostics(snapshot, changed_uri)
  -> updated cache + diagnostics by uri

semantic_snapshot(snapshot, uri)
  -> parsed/resolved/typed artifacts for hover/definition/completion
```

Key ideas:

- **Canonical module identity**: every source file has a stable normalized path
  and URI representation.
- **Document overlay**: open editor buffers take precedence over disk contents.
- **Dependency planning**: imports are discovered and checked in dependency order.
- **Stage reuse**: parse, resolve, and typecheck artifacts are reused when source
  and dependency inputs are unchanged.
- **Affected diagnostics**: a changed module can cause diagnostics in itself and
  in modules that depend on it.
- **No lower/link/codegen for diagnostics**: LSP diagnostics stop after semantic
  analysis unless a future feature explicitly needs later stages.

---

## Work Plan

### Phase 1 — Path, URI, and Source Overlay

Purpose: give the query layer a reliable way to locate source text from either
open documents or disk.

Checklist:

- [x] Add URI/path conversion helpers for `file://` documents.
- [x] Normalize/canonicalize paths used as query keys.
- [x] Define a source identity type shared by LSP and compiler queries.
- [x] Add an overlay abstraction that checks open documents before disk.
- [x] Preserve document versions for diagnostics published from open buffers.
- [x] Add tests for URI decoding, path normalization, and overlay precedence.

Likely files:

- `boot/lib/lsp/params.tw`
- `boot/lib/lsp/document_store.tw`
- `boot/lib/source/*`
- `boot/compiler/query/*`

Acceptance:

- A query can ask for source by canonical module/path and receive the current
  open-buffer text when present, otherwise disk text.
- LSP diagnostics continue to publish against the original document URI.

---

### Phase 2 — Dependency Planning for Diagnostics

Purpose: analyze a document with the same import semantics as the boot compiler,
without lowering or linking.

Checklist:

- [x] Reuse or extract module import discovery from the existing compiler path.
- [x] Resolve relative, project, and stdlib imports from the LSP entry file.
- [x] Build a dependency plan in semantic-check order.
- [x] Detect and report missing imports through diagnostics.
- [x] Detect cycles and report useful diagnostics without crashing the server.
- [x] Add tests for same-directory imports, relative imports, stdlib imports,
  missing imports, and cycles.

Likely files:

- `boot/compiler/module_compiler.tw`
- `boot/compiler/query/diagnostics.tw`
- `boot/lib/graph/dependency.tw`
- `boot/lib/source/registry.tw`

Acceptance:

- Opening a file that imports another module checks the dependency first and uses
  its exports when checking the opened file.
- Import failures appear as LSP diagnostics instead of transport/server errors.

---

### Phase 3 — Workspace Diagnostics Result Shape

Purpose: return diagnostics for every module affected by a change, not just the
edited document.

Checklist:

- [x] Change diagnostics query output from a single diagnostics list to
  diagnostics grouped by source identity/URI.
- [x] Publish diagnostics for all open affected documents.
- [x] Decide how to handle diagnostics for unopened files in LSP:
  - [ ] publish them when a URI is known, or
  - [x] keep them internal until the file opens.
- [x] Clear stale diagnostics when a file becomes clean or is removed from the
  affected set.
- [x] Add tests for dependency errors surfacing in importers and stale diagnostic
  clearing.

Likely files:

- `boot/compiler/query/diagnostics.tw`
- `boot/lib/lsp/diagnostics.tw`
- `boot/lib/lsp/server_core.tw`

Acceptance:

- Changing an exported type/function in a dependency can update diagnostics in
  open importers.
- Fixing a dependency clears stale diagnostics in affected open modules.

---

### Phase 4 — Query Cache Invalidation

Purpose: make repeated LSP checks cheap and predictable while keeping behavior
correct.

Checklist:

- [x] Define cache keys that include source content and relevant dependency
  semantic fingerprints.
- [x] Invalidate parse artifacts when source text changes.
- [x] Invalidate resolve/typecheck artifacts when dependency semantic inputs
  change.
- [x] Add reverse-dependency graph helpers for affected-module discovery.
- [x] Use reverse dependencies from workspace diagnostics cache invalidation.
- [x] Keep cache updates explicit in query outputs.
- [x] Add tests showing unaffected modules reuse cached stages.

Likely files:

- `boot/compiler/query/cache.tw`
- `boot/compiler/query/stage_runner.tw`
- `boot/compiler/query/diagnostics.tw`
- `boot/lib/query/keys.tw`
- `boot/lib/graph/dependency.tw`

Acceptance:

- Editing one file recomputes only the changed module and modules whose semantic
  inputs changed.
- Reverting a file or making no-op changes does not produce stale diagnostics.

---

### Phase 5 — LSP Integration Hardening

Purpose: make the workspace diagnostics query reliable under editor behavior.

Checklist:

- [x] Keep malformed document notifications ignored or converted to protocol
  errors without breaking server state.
- [x] Ensure analysis failures become diagnostics, not LSP process exits.
- [x] Support repeated open/change/close cycles for the same URI.
- [x] Add framed LSP integration coverage for imported modules.
- [x] Add framed LSP integration coverage for dependency edits affecting an
  already-open importer.
- [x] Document the recommended smoke command in developer docs or Makefile help.

Likely files:

- `boot/lib/lsp/server_core.tw`
- `boot/lib/lsp/diagnostics.tw`
- `tools/lsp_smoke.mjs`
- `Makefile`

Acceptance:

- The server remains alive across ordinary partial-edit states and module import
  failures.
- The smoke test covers workspace-aware diagnostics behavior through framed
  stdio, not just pure unit helpers.

---

### Phase 6 — Semantic Snapshots for Editor Features

Purpose: expose reusable analysis artifacts for hover, go-to-definition,
document symbols, and completion.

Checklist:

- [ ] Define a typed semantic snapshot returned by the query layer.
- [ ] Preserve source spans and symbol identities needed by editor features.
- [ ] Add lookup helpers for symbol-at-position and enclosing syntax context.
- [ ] Decide fallback behavior when the latest snapshot has parse/resolve/type
  errors.
- [ ] Add tests for semantic snapshot stability across edits.

Likely future LSP features:

- [ ] Hover.
- [ ] Go-to-definition.
- [ ] Document symbols.
- [ ] Completion.

Acceptance:

- Editor features can consume query artifacts directly without re-running their
  own compiler pipelines.
- Diagnostics and feature queries share the same source overlay and cache.

---

## Testing Strategy

Unit tests:

- URI/path normalization.
- Overlay source lookup.
- Dependency planning.
- Cache key/invalidation behavior.
- Diagnostics grouping and stale clearing.

Boot integration tests:

- Query diagnostics over imported modules.
- Open-buffer dependency overrides disk content.
- Missing imports and cycles produce diagnostics.

Framed LSP smoke tests:

- Open valid document -> empty diagnostics.
- Change to invalid text -> diagnostics.
- Close document -> empty diagnostics.
- Open importer and dependency -> dependency edit updates importer diagnostics.
- Fix dependency -> stale importer diagnostics clear.

Validation commands:

```bash
make lsp-smoke
tools/boot-test-fast.sh
tools/selfhost_loop.sh boot/main.tw
```

---

## Risks and Mitigations

- **URI/path mismatches causing duplicate cache entries**: use one canonical
  source identity everywhere and keep URI only for protocol output.
- **Open-buffer and disk divergence**: centralize source lookup behind the
  overlay abstraction.
- **Over-invalidation**: start conservative, then refine with dependency export
  fingerprints once correctness is solid.
- **Partial edits causing noisy failures**: convert compiler failures into
  diagnostics and keep the LSP process alive.
- **Feature-specific reanalysis**: expose semantic snapshots early so hover,
  definition, symbols, and completion do not grow parallel pipelines.

---

## Exit Criteria

This plan is complete when:

- LSP diagnostics are workspace-aware and import-aware.
- Open documents override disk contents for all modules in a diagnostics run.
- Query cache invalidation handles changed modules and affected importers.
- Diagnostics are published and cleared for affected open documents.
- Framed stdio integration tests cover workspace diagnostics behavior.
- The query layer exposes semantic artifacts suitable for the first editor
  feature implementation.
