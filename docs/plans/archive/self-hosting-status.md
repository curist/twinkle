# Self-Hosting Status Tracker

Last updated: 2026-04-13

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
| D | Codegen + linker | Done | Full boot codegen pipeline complete. Erased-boundary physical typing, sum match/pattern lowering, iterator/runtime, and repr parity all resolved. `tools/selfhost_loop.sh boot/main.tw` reaches fixed point (stage0→stage1→stage2→stage3 identical output). |
| E | Integration + self-hosting loop | Done | Multi-module compilation, CLI, and self-hosting loop all complete. Fixed-point self-hosting achieved 2026-04-13. |

---

## Post-Fixed-Point Subplans

The self-hosting loop is complete. Remaining work is hardening, cleanup, and
longer-term architecture.

| Area | Status | Plan |
|------|--------|------|
| Uniqueness mono sync | In Progress | [boot-uniqueness-mono-sync.md](boot-uniqueness-mono-sync.md) |
| Checker inference consistency | In Progress | [boot-checker-inference-consistency.md](boot-checker-inference-consistency.md) |
| Nested variant pattern lowering | In Progress | [boot-nested-variant-pattern-lowering.md](boot-nested-variant-pattern-lowering.md) |
| anyref elimination | Planned | [backend-anyref-elimination.md](backend-anyref-elimination.md) |
| Boot Wasm serializer | Planned | [boot-wasm-binary-serializer.md](boot-wasm-binary-serializer.md) |
| Historical completed milestones | Done | See [archive/README.md](archive/README.md) for earlier milestone plans and archived self-hosting blocker plans |

---

## Update Policy

When milestones land:

1. Update the relevant phase status and notes in this file.
2. Link the concrete PR/commit or test evidence in notes.
3. Keep [self-hosting.md](self-hosting.md) as architecture/design source of
   truth; keep this file as execution-status source of truth.
