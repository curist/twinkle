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

### Builtin & Inherent Method Docs

`///` only works for `.tw` source files. Builtins and Rust-implemented inherent methods
have no `.tw` source to attach comments to, so they need a separate mechanism.

**Three categories need coverage:**

1. **Pure builtins** — `println`, `error`, `range`, `Cell.new`, etc. Registered directly
   in `ValueEnv::new()` / `TypeEnv::new()` (Rust code). No `.tw` source at all.

2. **Intrinsic methods** — `String.len`, `String.slice`, `Vector.push`, etc. Signatures
   loaded from embedded `prelude/signatures/*.tw` files via `signatures.rs`, but bodies
   are stubs. The `.tw` files could carry `///` docs, but the signatures are parsed by
   a special pipeline that doesn't preserve comments.

3. **Prelude methods** — functions in `prelude/*.tw` (e.g., `string.tw`, `vector.tw`).
   These are real `.tw` source and *can* carry `///` docs once the parser supports it.
   However, they are auto-imported under internal `__prelude_*` aliases and their
   functions are re-registered under canonical builtin aliases (e.g., `Vector.map`),
   so doc lookup must follow that indirection.

**Approach: `doc` field on `FunctionSignature`**

Add `doc: Option<String>` to `FunctionSignature` in `src/types/ty.rs`. Populate it from:

* **Rust code** — hard-coded doc strings passed alongside builtin registration in
  `ValueEnv::new()` and `TypeEnv::new()`.
* **`///` in `.tw` source** — once parser support lands, extract docs during parsing and
  thread them through resolve → `FunctionSignature`. This covers both prelude `.tw` files
  and user-defined functions.

This single field is then available to hover and completion without separate lookup tables.

---

## Scope

**In scope:**

* publish diagnostics on `didOpen`/`didChange`/`didSave` (or immediate on change for now)  ✅ D1+D2 done
* clear diagnostics on `didClose`  ✅ D2 done
* completion in common contexts:
  * local identifiers in scope
  * top-level values/functions/types
  * import-qualified names (`alias.<...>`)
  * method names after `expr.`
* attach basic type/signature/doc detail to completion items when available
* doc strings for builtins and inherent methods via `FunctionSignature.doc`

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

### D1 — Diagnostics API + LSP Mapping  ✅

* `src/lsp/diagnostics.rs` — `QueryDiagnostic` → `LspDiagnostic` conversion (UTF-16 ranges, `E_`/`W_` severity).
* `src/module/mod.rs` — resilient analysis pipeline: errors recorded as structured diagnostics instead of aborting.
* `WorkspaceAnalysis.file_registries` — fallback registries for modules that failed resolve/typecheck.

### D2 — Publish Diagnostics Loop  ✅

* `src/lsp/session.rs` — `diagnostics()` and `all_diagnostics()` methods.
* `src/cli/lsp.rs` — `didOpen`/`didChange` push `publishDiagnostics`; `didClose` clears.
* Tests: `tests/lsp_diagnostics_test.rs` (7 tests).

### B1 — Builtin Doc Strings  ✅

* `src/types/ty.rs` — `FunctionSignature.doc: Option<String>` field added.
* `src/intrinsics/signatures.rs` — `builtin_doc()` table populates docs for all user-facing
  intrinsics (Int, Float, Bool, String, Byte, Vector, Dict, Cell, Range, Iterator).
* `src/lsp/mod.rs` — hover appends doc below type signature for both expression identifiers
  and method/qualified call targets; `builtin_value_doc()` covers `println`/`print`/`error`/etc.
* `src/module/context.rs`, `src/module/mod.rs` — propagate `doc` through FunctionSignature clones.
* Tests: `tests/lsp_hover_test.rs` — `hover_on_builtin_function_shows_doc_string`,
  `hover_on_method_call_shows_doc_string`.

### C1 — Completion Core (No Docs Yet)  ✅

**Status (2026-03-13):**

Implemented:

* `src/lsp/completion.rs` added with semantic completion query + context classifier (`Identifier` vs `Dot`).
* Candidate gathering implemented for:
  * lexical locals/params (scope walk)
  * module top-level functions/values
  * import aliases
  * module exports after `alias.`
  * type members after `expr.` (record fields + methods via `TypeEnv`)
  * builtins, type names, sum variants, keywords
* `src/lsp/session.rs` exposes `completion(entry, module, position)`.
* `src/lsp/mod.rs` exports new completion module.
* `src/types/env.rs` now exposes iteration helpers used by completion:
  * `TypeEnv::methods_for_type`
  * `TypeEnv::all_type_names`
  * `ValueEnv::all_functions` / `all_values` / `all_builtins`
* `src/cli/lsp.rs` now:
  * advertises `completionProvider` in `initialize` capabilities
  * handles `textDocument/completion`
  * maps completion results to protocol `CompletionItem` JSON
* Tests added:
  * `tests/lsp_completion_test.rs` (locals, import alias, method completion)
  * `src/cli/lsp.rs` unit test for protocol request/response completion path

**Code changes (target state):**

* Context classification + candidate gathering from lexical/module/import/method sources.

**Likely files:**

* `src/lsp/completion.rs` (new)
* `src/lsp/index.rs`
* `src/lsp/session.rs`

**Acceptance:**

* completion returns stable candidates in the core contexts listed above.
* completion is reachable through LSP (`textDocument/completion`).
* completion behavior is covered by automated tests.

### C2 — Simple Doc Comments (`///`) + Completion/Hover Detail  ✅

**Status (2026-03-13):**

Implemented:

* `///` parsing/attachment for top-level declarations (`fn`, `type`, `pub let`) via a
  post-parse source pass in `src/syntax/mod.rs` (strict contiguous block, blank-line break).
* AST doc storage:
  * `FunctionDecl.doc: Option<String>`
  * `TypeDecl.doc: Option<String>`
  * `Stmt::Let { doc: Option<String>, ... }` (used for `pub let`)
* Resolver now threads source docs into `FunctionSignature.doc` (`src/types/resolve.rs`).
* Completion items now carry `documentation` text in addition to `detail`
  (`src/lsp/completion.rs` + protocol mapping in `src/cli/lsp.rs`).
* Hover now shows source `///` docs automatically through existing `FunctionSignature.doc`
  lookup path (`src/lsp/mod.rs` behavior unchanged, data path now populated).

Notes:

* This implementation intentionally does not add lexer-level doc trivia tokens yet.
  It uses the current parser + source-text attachment pass to keep scope small and
  land user-visible value quickly.

**Code changes:**

* Preserve and attach `///` docs to declarations in the parser.
* Thread parsed docs into `FunctionSignature.doc` during resolve/typecheck (same field used by B1 for builtins).
* Surface docs in completion item documentation and hover supplementary text.

**Likely files:**

* `src/syntax/mod.rs`
* `src/syntax/parser.rs`
* `src/syntax/ast.rs`
* `src/types/resolve.rs`
* `src/lsp/completion.rs`
* `src/cli/lsp.rs`

**Acceptance:**

* `///` comments appear for documented declarations in completion/hover.
* Non-doc comments remain ignored for docs.
* Prelude `.tw` files with `///` docs surface through hover/completion.

---

## Test Plan

### Unit tests

* `///` attachment rules (blank-line break, contiguous blocks).  ✅
* diagnostics range conversion (including Unicode).  ✅
* completion context classifier.  (pending, not isolated yet)
* builtin doc strings populated on `FunctionSignature`.  ✅

### Integration tests

* publish diagnostics after invalid edit, then clear after fix.  ✅
* completion for locals/import aliases/methods.  ✅
* completion docs populated from `///`.  ✅
* hover shows doc string for builtins (`println`, `String.len`, etc.).  ✅
* hover shows doc strings from user `///` declarations.  ✅

### Protocol smoke tests

* initialize -> open -> change -> publish diagnostics.
* completion request in several positions with expected labels.  (partial: request/response path covered by unit test)

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

---

## Next Slice

1. Execute optional hardening plan below (`DocComment` trivia/tokens in lexer+parser).
2. Add protocol smoke tests that issue completion requests at multiple cursor positions and assert
   `documentation` payloads directly.

---

## Optional Hardening Plan — Doc Trivia Model

### Goal

Replace the current source-text post-pass doc attachment with a lexer/parser-driven model so
doc comments become stable syntax artifacts that are reusable by formatter/lossless tooling.

### Scope

In scope:

* `DocComment` token/trivia support for `///`.
* Parser-side doc attachment for top-level `fn` / `type` / `pub let`.
* Removal of source-text attachment logic from `src/syntax/mod.rs`.
* Behavior parity with current rules (contiguous-only, blank-line break, ignore non-doc comments).

Out of scope:

* Block doc syntax (`/** ... */`).
* Doc comments on non-top-level declarations.
* Markdown rendering/formatting semantics for docs.

### Milestones

#### H1 — Lexer/Token plumbing

* Extend lexer/token model to preserve `///` as doc-comment trivia (or explicit `DocComment` token).
* Preserve enough line-boundary information to enforce blank-line break semantics in parser.

Likely files:

* `src/syntax/lexer.rs`
* `src/syntax/tokens.rs`

Acceptance:

* `///` is recoverable by parser without scanning raw source text.
* Existing non-doc comment behavior is unchanged.

#### H2 — Parser attachment

* Attach doc blocks in parser right before consuming top-level declarations.
* Keep strict attachment semantics:
  * contiguous `///` lines only
  * blank line breaks block
  * non-doc comment breaks/ignores doc block

Likely files:

* `src/syntax/parser.rs`
* `src/syntax/ast.rs`

Acceptance:

* AST `doc` fields are populated with identical results to current C2 behavior.
* `src/syntax/mod.rs` no longer needs source-text doc attachment pass.

#### H3 — Cleanup + regression hardening

* Delete/retire now-redundant source-pass helpers from `src/syntax/mod.rs`.
* Add targeted parser/lexer tests for doc-tokenization and attachment rules.
* Re-run LSP completion/hover doc tests to ensure no behavior regressions.

Likely files:

* `src/syntax/mod.rs`
* `src/syntax/lexer.rs`
* `src/syntax/parser.rs`
* `tests/lsp_completion_test.rs`
* `tests/lsp_hover_test.rs`

Acceptance:

* No user-visible behavior change in hover/completion docs.
* Implementation no longer depends on byte/char offset conversion for doc attachment.
