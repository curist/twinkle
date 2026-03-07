# Twinkle Documentation

## Language

- [spec.md](spec.md) — Language specification (canonical reference)
- [grammar.ebnf](grammar.ebnf) — Formal grammar

## Design

Language design notes, rationale, and open questions.

- [module.md](design/module.md) — Module system design decisions (D-001 through D-009+)
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
- [host-abi.md](internals/host-abi.md) — Wasm host ABI reference
- [persistent-runtime.md](internals/persistent-runtime.md) — Runtime abstraction layer
- [query-pipeline.md](internals/query-pipeline.md) — Pipeline architecture
- [tooling.md](internals/tooling.md) — Formatter, linter, LSP internals
- [test-plan.md](internals/test-plan.md) — Testing strategy

## Plans

Implementation plans and stage breakdowns — see [plans/README.md](plans/README.md).