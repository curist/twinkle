# Self-Hosting Status Tracker

Last updated: 2026-04-10

## Purpose

Track implementation progress for the self-hosted compiler plan in
[self-hosting.md](self-hosting.md) with a lightweight, update-friendly status
snapshot.

---

## Status Key

- `Planned`: design exists, implementation not started
- `In Progress`: active implementation work
- `Blocked`: waiting on prerequisite decision/work
- `Done`: landed and validated at plan scope

---

## Phase Snapshot

| Phase | Scope | Status | Notes |
|------|-------|--------|-------|
| A | Frontend (lexer/parser/resolver/checker) | Done | Lexer, parser, resolver, type checker landed. Remaining checker work is parity/hardening follow-up rather than missing core pipeline stages. |
| B | Core IR lowering + monomorphization | Done | [archive/boot-core-ir.md](archive/boot-core-ir.md) — core IR, lowering, and monomorphization are in place. |
| C | ANF lowering + optimization | Done | [archive/boot-anf-lowering.md](archive/boot-anf-lowering.md) — ANF lowering, optimization passes, liveness, uniqueness rewrite, and defer elimination landed. |
| D | Codegen + linker | In Progress | The full boot codegen pipeline exists and the boot compiler can compile itself to Wasm. The erased-boundary physical typing cleanup is complete for its current scope; the remaining self-hosted backend correctness gaps are now centered on representation parity in sum match/pattern lowering and on iterator/runtime follow-up work. |
| E | Integration + self-hosting loop | In Progress | Multi-module/self-host execution works far enough to drive backend debugging. The remaining work is no longer foundation-library setup; it is closing the self-hosted Wasm correctness gap. |

---

## Active Subplans

Current backend execution order:

1. [boot-selfhosted-wasm-repr-parity.md](boot-selfhosted-wasm-repr-parity.md)
   tracks the active self-hosting blocker categories and overall sequencing.
2. [boot-backend-physical-typing.md](boot-backend-physical-typing.md)
   is complete for the current erased-boundary stabilization scope and now acts
   as the record of that cleanup.
3. Current active work returns to
   [boot-selfhosted-wasm-repr-parity.md](boot-selfhosted-wasm-repr-parity.md),
   with sum match/pattern lowering now the leading blocker.
4. [backend-anyref-elimination.md](backend-anyref-elimination.md)
   remains the longer-term architecture plan after the current self-hosted
   backend stops advancing by validator mismatch cleanup.

| Area | Status | Plan |
|------|--------|------|
| Multi-module integration | In Progress | [boot-multi-module.md](boot-multi-module.md) |
| Self-hosted Wasm repr parity | In Progress | [boot-selfhosted-wasm-repr-parity.md](boot-selfhosted-wasm-repr-parity.md) |
| Backend physical typing | Done | [boot-backend-physical-typing.md](boot-backend-physical-typing.md) |
| Iterator codegen correctness | In Progress | [boot-iterator-codegen-parity.md](boot-iterator-codegen-parity.md) |
| Uniqueness mono sync | In Progress | [boot-uniqueness-mono-sync.md](boot-uniqueness-mono-sync.md) |
| Checker inference consistency | In Progress | [boot-checker-inference-consistency.md](boot-checker-inference-consistency.md) |
| Boot Wasm serializer | Planned | [boot-wasm-binary-serializer.md](boot-wasm-binary-serializer.md) |
| Historical completed milestones | Done | See [archive/README.md](archive/README.md) for the earlier parser/resolver/checker/core/ANF/codegen milestone plans |

---

## Update Policy

When milestones land:

1. Update the relevant phase status and notes in this file.
2. Link the concrete PR/commit or test evidence in notes.
3. Keep [self-hosting.md](self-hosting.md) as architecture/design source of
   truth; keep this file as execution-status source of truth.
