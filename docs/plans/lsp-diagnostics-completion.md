# LSP Phase 2 Plan — Diagnostics + Completion

## Goal

Extend `twk lsp` after Phase 1 (hover + definition) with:

1. **Published diagnostics** (`textDocument/publishDiagnostics`).
2. **Code completion** (`textDocument/completion`).

This phase also defines a minimal, explicit doc-comment convention to support richer hover/completion detail.

---

## Minimal Doc-Comment Convention (Simple First)

To keep scope small, adopt one form only:

* `///` line comments

Examples:

```tw
/// Adds two integers.
pub fn add(x: Int, y: Int) Int {
  x + y
}

/// RGB color.
type Color = .{ r: Int, g: Int, b: Int }
```

Attachment rules (Phase 2):

1. A contiguous `///` block attaches to the **next top-level declaration** (`fn`, `type`, or `pub let`).
2. A blank line breaks the block.
3. Non-doc comments (`//`) are ignored for docs.
4. No block-doc syntax (`/** */`) in Phase 2.

Storage shape (suggested):

* Add `doc: Option<String>` on top-level declaration AST nodes (or an equivalent side table keyed by declaration span).

---

## Scope

**In scope:**

* publish diagnostics on `didOpen`/`didChange`/`didSave` (or immediate on change for now)
* clear diagnostics on `didClose`
* completion in common contexts:
  * local identifiers in scope
  * top-level values/functions/types
  * import-qualified names (`alias.<...>`)
  * method names after `expr.`
* attach basic type/signature/doc detail to completion items when available

**Out of scope:**

* snippets with complex edits
* semantic token coloring
* advanced ranking/ML scoring
* rename/refactor APIs

---

## Current Baseline

* Structured query diagnostics already exist as `QueryDiagnostic` in `src/query/api.rs`.
* Module compilation with in-memory source map and deterministic dependency order is available.
* No parser recovery yet; parse errors still hard-fail.
* Comments are not preserved as doc trivia in a tool-friendly way yet.

---

## Architecture Direction

### 1) Diagnostics pipeline

Add an analysis API that returns diagnostics per canonical module path, including parse/resolve/typecheck failures for editor display.

Key behavior:

* convert internal spans to LSP ranges (UTF-16 correct)
* map error code prefixes to LSP severities (`Error`, later `Warning` for lints)
* keep last-successful semantic state for completion/hover fallback when an edit introduces parse failure

### 2) Completion engine

Implement completion as semantic query over analyzed module + cursor context:

* classify context from AST/span/token neighborhood:
  * value position
  * member access after dot
  * type position (optional in this phase)
* gather candidates from layered scopes:
  1. lexical locals/params
  2. current module top-level declarations
  3. imported module exports via alias
  4. methods from `TypeEnv` for receiver type
* include `label`, `kind`, optional `detail` (type/signature), and optional docs (from `///`).

### 3) Doc-comment plumbing

Preserve doc comments through lex/parse and expose them in query artifacts used by hover/completion:

* lexer emits doc-comment trivia
* parser attaches docs to declaration nodes
* analysis exports docs in declaration metadata

---

## Milestones

### D1 — Diagnostics API + LSP Mapping

**Code changes:**

* Add `analyze_workspace_with_diagnostics(...)` result shape (or extend Phase 1 analysis API).
* Implement internal diagnostic -> LSP diagnostic mapping helpers.

**Likely files:**

* `src/query/api.rs`
* `src/module/mod.rs`
* `src/lsp/diagnostics.rs` (new)
* `src/lsp/position.rs`

**Acceptance:**

* diagnostics available per module path with stable ordering and codes.

### D2 — Publish Diagnostics Loop

**Code changes:**

* On `didOpen`/`didChange`: re-analyze impacted graph and push diagnostics.
* On `didClose`: clear diagnostics for that URI.

**Likely files:**

* `src/lsp/session.rs`
* `src/cli/lsp.rs`

**Acceptance:**

* editor shows and clears errors correctly across edits and close events.

### C1 — Completion Core (No Docs Yet)

**Code changes:**

* Context classification + candidate gathering from lexical/module/import/method sources.

**Likely files:**

* `src/lsp/completion.rs` (new)
* `src/lsp/index.rs`
* `src/lsp/session.rs`

**Acceptance:**

* completion returns stable candidates in the core contexts listed above.

### C2 — Simple Doc Comments (`///`) + Completion/Hover Detail

**Code changes:**

* Preserve and attach `///` docs to declarations.
* Surface docs in completion item documentation and hover supplementary text.

**Likely files:**

* `src/syntax/lexer.rs`
* `src/syntax/tokens.rs`
* `src/syntax/parser.rs`
* `src/syntax/ast.rs`
* `src/lsp/completion.rs`
* `src/lsp/hover.rs`

**Acceptance:**

* `///` comments appear for documented declarations in completion/hover.
* non-doc comments remain ignored for docs.

---

## Test Plan

### Unit tests

* `///` attachment rules (blank-line break, contiguous blocks).
* diagnostics range conversion (including Unicode).
* completion context classifier.

### Integration tests

* publish diagnostics after invalid edit, then clear after fix.
* completion for locals/import aliases/methods.
* completion docs populated from `///`.

### Protocol smoke tests

* initialize -> open -> change -> publish diagnostics.
* completion request in several positions with expected labels.

---

## Risks and Mitigations

* **Parser hard-fail on broken code**: keep fallback to last-good semantic index; still publish current parse diagnostics.
* **Too many completion candidates**: start deterministic and simple; add lightweight prefix filtering + ranking.
* **Doc attachment ambiguity**: codify strict `///` next-declaration rule and test it.

---

## Non-Goals for This Phase

* formatter/linter integration into diagnostics stream
* block doc syntax (`/** */`)
* completion snippets with import auto-insertion
