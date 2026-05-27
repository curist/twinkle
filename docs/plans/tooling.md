# Tooling Active TODO Plan

## Goal

Track active, user-facing Twinkle tooling work now that the formatter, boot LSP
baseline, self-hosted compiler, and standalone `twk` workflow are in place.

Historical tooling roadmap context is archived in
[archive/tooling.md](archive/tooling.md).

---

## Current Baseline

Available today:

* `twk fmt <file>` and `twk fmt --check <file>` for canonical source formatting.
* `twk lsp` with diagnostics, hover, go to definition, completion, unused-import
  code actions, and whole-document formatting.
* `twk check`, `twk parse`, `twk ir`, `twk build`, and `twk run` in the boot CLI.
* Twinkle-native boot test suite under `boot/tests/`.

---

## Active Tooling Work Index

| Area | Priority | Status | Details |
|------|----------|--------|---------|
| LSP enhancements | High | Planned | [lsp-enhancements.md](lsp-enhancements.md) |
| Additional LSP code actions | Medium | Planned | [lsp-code-actions.md](lsp-code-actions.md) |
| Whole-project formatter UX | Medium | Planned | This document |
| Linter | Medium | Planned | This document |
| Test runner UX | Medium | Planned | This document |
| Documentation generator | Low | Planned | This document |
| Package/project tooling | Low | Planned | This document |

---

## T1 — Whole-Project Formatter UX

`twk fmt` currently formats explicitly provided files. The next formatter UX
step is project-level discovery.

Planned work:

* Add `twk fmt --all` to discover `.tw` files from the project root.
* Respect obvious ignore locations such as build output directories.
* Keep deterministic traversal and output ordering.
* Report per-file read/parse/write failures while continuing other files.
* Return a non-zero exit when any file needs changes in `--check` mode or any
  formatting failure occurs.

Open questions:

* Whether repeated positional files plus `--all` should be rejected or merged.
* Whether ignore rules should come from `twinkle.toml` or stay hardcoded at
  first.

---

## T2 — Linter

Add `twk lint` for lightweight style and correctness diagnostics that do not
belong in the type checker.

Initial rule candidates:

* Rebinding-without-use patterns that look like mutation mistakes.
* Unreachable code after `return`, `break`, or `error` where syntactically clear.
* Redundant wildcard case arms after exhaustive explicit arms.
* Suspicious shadowing in the same small scope.
* Public declarations without doc comments once the stdlib/project style needs
  it.

Design notes:

* Syntactic lint rules should run after parse only.
* Semantic lint rules can reuse the query pipeline.
* Lint diagnostics should use the same source diagnostic and LSP diagnostic
  rendering paths as compiler diagnostics where practical.

---

## T3 — Test Runner UX

The boot test suite already has Twinkle-native suite infrastructure. The next
step is making it a user-facing tool.

Planned work:

* Add a `twk test` command that discovers project tests.
* Define conventional test file/module discovery.
* Support filtering by suite/test name.
* Produce concise human output and CI-friendly failure output.
* Decide how tests should expose pass/fail APIs in normal user projects.

---

## T4 — Documentation Generator

Add a documentation tool for public module APIs.

Planned work:

* Extract public functions, types, variants, fields, contracts, and doc comments.
* Render Markdown or static HTML from compiler query artifacts.
* Link references between modules where possible.
* Include signatures using the same user-facing type rendering as hover.

This can share much of the symbol extraction needed by LSP document/workspace
symbols.

---

## T5 — Package and Project Tooling

Twinkle currently uses `twinkle.toml` primarily for project-root discovery. A
future package workflow can build on that.

Potential work:

* `twk init` for a minimal project.
* `twk new` for app/library templates.
* Dependency declaration and lockfile design.
* Standard library/package documentation integration.
* Build/test/fmt/check orchestration for packages.

This area should stay low priority until the language and stdlib stabilize.

---

## Cross-Cutting Requirements

* Prefer boot compiler implementations under `boot/` for new tooling behavior.
* Keep CLI behavior deterministic and CI-friendly.
* Reuse query artifacts rather than adding duplicate parser/typechecker paths.
* Preserve UTF-16 position correctness at LSP boundaries.
* Add boot tests for new CLI/LSP behavior.
