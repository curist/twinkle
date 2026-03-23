# Later Stages — Tooling & Ecosystem

> **Full design:** See [docs/internals/tooling.md](../internals/tooling.md).
>
> **Phase 1 LSP execution plan (completed):** See [archive/lsp-hover-goto-definition.md](archive/lsp-hover-goto-definition.md).
>
> **LSP completion follow-up plan:** See [lsp-completion.md](lsp-completion.md).  
> **Archived LSP watched files plan:** See [archive/lsp-file-watching.md](archive/lsp-file-watching.md).  
> **Archived Phase 2 mixed plan:** See [archive/lsp-diagnostics-completion.md](archive/lsp-diagnostics-completion.md).

**Prerequisites before tooling:**

* **Lossless lexer**: comments preserved as trivia tokens (required by formatter and LSP).
* **Parser error recovery**: partial AST on syntax errors (required by LSP; nice-to-have for formatter).

**Practical goals for "easy tooling":**

* **Whole-program formatting is easy to run**
  * `twk fmt --all` discovers project files from root and formats all `.tw` files deterministically;
  * formatting is idempotent (`fmt` twice yields no diff);
  * on syntax error, command reports file + span and continues other files (non-zero exit at end).

* **Incremental LSP is easy to keep fast**
  * on file change, re-run parse/resolve/typecheck only for the changed file and affected reverse-dependents;
  * unchanged modules are served from stage caches;
  * diagnostics/hover/go-to-definition read query artifacts directly (no full lower/link requirement).

**Milestones to reach those goals:**

* **T1 — Formatter core**
  * finish lossless lexer trivia model and formatter AST printer;
  * add `twk fmt <file>` with golden tests + idempotence tests.

* **T2 — Whole-project formatter UX**
  * add `twk fmt --all` file discovery, include/exclude rules, and stable output ordering;
  * add CI-friendly exit codes and summary reporting.

* **T3 — Incremental diagnostics for LSP**
  * complete Stage 6b query-cache work;
  * add dependency graph invalidation + reverse-dependency tracking;
  * expose diagnostics query endpoint reused by CLI and LSP host.

* **T4 — Interactive LSP features**
  * add hover / go-to-definition / completion on top of cached query artifacts;
  * add latency benchmarks with warm cache and edit-loop workloads.

**Tooling implementation map (files):**

* **Formatter core (`T1`)**
  * `src/syntax/lexer.rs`, `src/syntax/tokens.rs`:
    * add trivia/comment preservation model required by formatter.
  * `src/syntax/parser.rs`:
    * ensure parser exposes token/trivia links needed by formatting decisions.
  * `src/syntax/pretty.rs`:
    * implement canonical formatter printer.
  * `src/cli/mod.rs`, new `src/cli/fmt.rs`, `src/main.rs`:
    * wire `twk fmt <file>` and `--check`.
  * Tests: add formatter golden/idempotence suite under `tests/` (new formatter tests file + fixtures).

* **Whole-project formatting UX (`T2`)**
  * New module recommended: `src/cli/project_files.rs` (shared project file discovery).
  * `src/cli/fmt.rs`:
    * implement `--all`, deterministic file ordering, partial-failure reporting, CI exit codes.

* **Incremental diagnostics (`T3`)**
  * Build on query cache modules from Stage 6b Step C.
  * New module recommended: `src/diagnostics/mod.rs` for normalized diagnostic structures.
  * `src/cli/check.rs`:
    * consume diagnostics query API (no full lower/link unless requested).

* **LSP (`T4`)**
  * New module recommended: `src/lsp/mod.rs` (or standalone crate later).
  * Integrate with query API from Stage 6b Step D; keep lower/link out of hot path.
  * Add edit-loop latency benchmark harness (new `benches/lsp_latency.rs`).

**Planned tools** (all as `twk` subcommands initially, rewritten in Twinkle post-self-hosting):

* **`twk fmt`**: canonical formatter. Only needs parse stage + lossless lexer. No config; one official style.
* **`twk lint`**: linter with syntactic rules (parse only) and semantic rules (parse + typecheck). Key rule: warn on rebinding-without-use (the "looks like mutation but isn't" trap).
* **`twk lsp`**: language server (LSP protocol). Needs query-friendly pipeline + lossless lexer + error recovery. Initial features: diagnostics, hover types, go-to-definition, completion.
* **Standard library** in Twinkle (collections, JSON, I/O via WASI).
* **Package manager**, **test runner**, **doc generator**, **build system**.

These tools are separate concerns and plug into the pipeline via the existing compiler API (parse, typecheck, IR, codegen).
