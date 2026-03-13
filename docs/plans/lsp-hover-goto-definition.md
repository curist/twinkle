# LSP Phase 1 Plan — Hover + Go-To-Definition

## Goal

Ship a first usable `twk lsp` with:

1. **Hover**: show inferred type at cursor.
2. **Go-to-definition**: jump to declaration for symbol under cursor.

This plan is intentionally scoped to the existing compiler architecture (query stages + module orchestrator + source-map compilation), and avoids lower/link in the interactive path.

---

## Scope

**In scope (Phase 1):**

* `textDocument/hover`
* `textDocument/definition`
* open/change/close tracking for unsaved buffers
* definition coverage for:
  * local variables and function parameters
  * top-level functions/types/`let` bindings
  * imported module-qualified symbols (`alias.name`)
  * method calls where a concrete target function can be resolved

**Out of scope (deferred):**

* completion, rename, references, code actions
* doc-comment hover rendering (needs trivia/doc-comment pipeline)
* robust behavior on heavily broken syntax (full parser recovery)
* multi-root workspace protocol features

---

## Current Baseline (Repo Reality)

* There is no `lsp` module and no `twk lsp` subcommand yet.
* The query pipeline is already decomposed and cache-backed:
  * parse/resolve/typecheck/lower stages and cache keys in `src/query/*`
  * reverse-dependent invalidation exists via `DependencyGraph`.
* Type checking already records `ExprId -> MonoType` (`TypeMap::expr_types`), which is sufficient for hover once we can map cursor -> `ExprId`.
* `compile_entry_from_source_map[_with_trace]` already supports in-memory source maps and deterministic dependency traversal, which is a strong fit for LSP document state.
* There is currently no first-class reference/definition index:
  * `ResolvedModule` does not expose declaration spans.
  * `ModuleExports` does not carry declaration spans.
  * `TypeMap` has `method_calls`/`generic_instantiations` slots that are currently unused.
* Span model is byte-offset based (`Span { start, end }`), while LSP uses zero-based UTF-16 line/character positions. Conversion helpers do not exist yet.

---

## Architecture Direction

### 1) Add an analysis-only workspace API

Introduce a public API that compiles from source-map through **parse+resolve+typecheck only** and returns per-module artifacts needed for editor features.

Target shape (exact naming can vary):

```rust
pub struct WorkspaceAnalysis {
    pub entry_path: PathBuf,
    pub modules: HashMap<PathBuf, AnalyzedModule>,
    pub diagnostics: HashMap<PathBuf, Vec<QueryDiagnostic>>,
}

pub struct AnalyzedModule {
    pub ast: SourceFile,
    pub file_registry: FileRegistry,
    pub typed: TypedModule,
    pub imports: Vec<AnalyzedImport>, // alias -> canonical path
}
```

Design requirement: LSP should never trigger lower/link for hover/definition.

### 2) Build a semantic index layer on top of analysis artifacts

Add a lightweight, query-time index:

* `ExprSpanIndex`: `(Span, ExprId)` entries to map cursor offset -> smallest containing expression.
* `DefinitionIndex`: declaration sites with `(path, span, symbol kind, name)`.
* `ReferenceIndex` (or on-demand resolver): symbol use site -> definition target.

This index can be built per analyzed module and cached by module hash.

### 3) Implement a thin LSP session host

`twk lsp` session responsibilities:

* maintain open-buffer overlays (`PathBuf -> String`)
* materialize workspace source map (disk + overlays)
* on request, analyze relevant module entry and query semantic index
* return LSP responses (`Hover`, `Location`)

Keep protocol transport separate from semantic logic so tests can call semantic APIs directly.

---

## Milestones

### M1 — Analysis Artifacts for Tooling

**Code changes:**

* Add analysis-oriented API in module/query layer (source-map friendly, no lowering).
* Capture per-module typed artifacts and import alias mapping.
* Normalize diagnostics as `QueryDiagnostic` keyed by module path.

**Likely files:**

* `src/module/mod.rs`
* `src/module/context.rs`
* `src/module/stage_runner.rs`
* `src/query/api.rs`

**Acceptance:**

* Existing compile/check behavior unchanged.
* New tests prove multi-module source-map analysis returns typed artifacts and diagnostics without lowering.

### M2 — Position/Span Conversion + Expression Lookup

**Code changes:**

* Add UTF-16 <-> byte-offset conversion helpers.
* Add cursor-to-`ExprId` resolver using smallest-containing-span strategy.

**Likely files:**

* `src/lsp/position.rs` (new)
* `src/lsp/index.rs` (new)
* optional helper in `src/syntax/span.rs` if shared

**Acceptance:**

* Unit tests for ASCII and multibyte Unicode positions.
* Deterministic `ExprId` lookup under nested expressions.

### M3 — Hover Core

**Code changes:**

* `hover_at(path, position)`:
  * convert position -> byte offset
  * find `ExprId`
  * read type from `TypedModule.type_map`
  * format with `MonoType::format_with_names(&typed.type_env)`

**Likely files:**

* `src/lsp/hover.rs` (new)
* `src/lsp/mod.rs` (new)

**Acceptance:**

* Hover returns expected types for:
  * local identifiers
  * call expressions
  * method calls
  * literals and field/index expressions
* Hover returns `null` when cursor is not on an expression.

### M4 — Definition Index + Go-To-Definition Core

**Code changes:**

* Build declaration maps for:
  * local bindings (params + `let` patterns) with lexical scopes
  * top-level declarations
  * import-qualified symbols
* Resolve cursor symbol to declaration location:
  * same-file local/top-level
  * cross-module via import alias map
  * method target resolution via typed receiver + method table
* Extend artifacts (if needed) to carry declaration spans for exported symbols.

**Likely files:**

* `src/lsp/definition.rs` (new)
* `src/lsp/index.rs` (new)
* `src/module/artifacts.rs` and/or `src/module/context.rs` (span metadata if needed)
* `src/types/resolve.rs` (if exposing declaration span tables is cleaner here)

**Acceptance:**

* Definition jumps work for:
  * local variable and parameter references
  * top-level `fn`, `type`, `let`
  * `alias.name` cross-module references
  * method call sites with resolvable targets
* Builtin/intrinsic symbols without source declarations return no location (not an error).

### M5 — LSP Protocol Endpoint (`twk lsp`)

**Code changes:**

* Add CLI subcommand and stdio JSON-RPC loop.
* Implement minimal methods:
  * `initialize`, `initialized`, `shutdown`, `exit`
  * `textDocument/didOpen`, `didChange`, `didClose`
  * `textDocument/hover`
  * `textDocument/definition`

**Likely files:**

* `src/main.rs`
* `src/cli/mod.rs`
* `src/cli/lsp.rs` (new)
* `src/lsp/session.rs` (new)

**Acceptance:**

* Basic editor integration works in end-to-end smoke test.
* No panic on malformed/unsupported requests; return LSP-compatible errors/nulls.

---

## Test Plan

### Unit tests

* UTF-16 position conversion edge cases.
* Expr span indexing behavior (smallest-containing rule).
* Lexical scope resolution for locals/shadowing.

### Integration tests (source-map based)

* Hover and definition within one module.
* Cross-module definition via imports.
* Method call definition to prelude/user methods.
* Unsaved buffer change updates hover/definition without disk writes.

### Protocol smoke tests

* Spin up `twk lsp`, send initialize/open/hover/definition/shutdown sequence over stdio.

---

## Performance Targets (Phase 1)

* Warm hover/definition request median: **< 50 ms** on medium test project.
* Cold request after edit: **< 250 ms** for entry module with small dependency graph.
* No lower/link in hot path.

---

## Risks and Mitigations

* **UTF-16 mismatch bugs**: isolate conversion logic and test heavily with Unicode fixtures.
* **Scope-resolution drift from type checker**: keep local binding resolver simple and backed by focused tests; only use typed artifacts where necessary.
* **Cross-module symbol ambiguity**: key indices by canonical path + symbol kind + name.
* **Parser hard-fail behavior**: for Phase 1, return empty hover/definition on parse failure and preserve previous successful analysis for unaffected files.

---

## Follow-Ups After Phase 1

* Publish diagnostics on change/save and add completion via [lsp-diagnostics-completion.md](lsp-diagnostics-completion.md).
* Add parser recovery and doc-comment trivia support for richer editor UX.
