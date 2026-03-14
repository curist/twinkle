# Twinkle Documentation

## Language

- [spec.md](spec.md) — Language specification (canonical reference)
- [grammar.ebnf](grammar.ebnf) — Formal grammar

## Design

Language design notes, rationale, and open questions.

- [module.md](design/module.md) — Module system design (imports, resolution, aliasing, re-exports)
- [compiler-architecture.md](design/compiler-architecture.md) — Compiler pipeline, architecture principles, and repository layout
- [records.md](design/records.md) — Nominal record types
- [traits.md](design/traits.md) — Why no traits; records-of-functions instead
- [iterator.md](design/iterator.md) — Iterator design
- [defer.md](design/defer.md) — `defer` semantics
- [immutability.md](design/immutability.md) — Immutability and explicit state
- [stdlib.md](design/stdlib.md) — Standard library API design

## Open Questions

- [open-questions.md](open-questions.md) — Unresolved design concerns and discussion items

## Compiler Internals

Implementation details for compiler and runtime contributors.

- [ir.md](internals/ir.md) — Core IR and ANF IR specification
- [host-abi.md](internals/host-abi.md) — Host import contract (what Wasmtime/browser/Node.js must provide)
- [persistent-runtime.md](internals/persistent-runtime.md) — Backend-agnostic runtime surface for immutable data operations
- [query-pipeline.md](internals/query-pipeline.md) — Refactoring the pipeline into pure per-stage functions (for LSP, testing, self-hosting)
- [tooling.md](internals/tooling.md) — Design for formatter, linter, and LSP (`twk fmt`, `twk lint`, etc.)
- [test-plan.md](internals/test-plan.md) — Testing methodology across all compiler stages and runtimes

## Plans

Implementation plans and stage breakdowns — see [plans/README.md](plans/README.md).
