# Self-Hosting Status Tracker

Last updated: 2026-03-21

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
| A | Frontend (lexer/parser/resolver/checker) | Done | Lexer, parser, resolver, type checker (M1–M9) all complete. Method registry M1–M4 done; M5 deferred to multi-module. Snapshot testing for diagnostics. |
| B | Core IR lowering + monomorphization | Done | [archive/boot-core-ir.md](archive/boot-core-ir.md) — IR types, lowering (all expr/stmt forms), monomorphization. All gaps and discrepancies resolved. |
| C | ANF lowering + optimization | Planned | No committed self-hosted ANF/opt pipeline yet. |
| D | Codegen + linker | Planned | Representation/layout redesign is documented in [self-hosting.md](self-hosting.md). |
| E | Integration + self-hosting loop | Planned | Depends on A-D milestones and `boot` module/graph/query libs. |

---

## Active Subplans

| Area | Status | Plan |
|------|--------|------|
| Frontend gap closure | Done | [archive/boot-parser-gap-closure.md](archive/boot-parser-gap-closure.md) |
| Resolver fixes | Done | [archive/boot-resolver-fixes.md](archive/boot-resolver-fixes.md) |
| Type checker | In Progress | [boot-type-checker.md](boot-type-checker.md) — M1–M9 done |
| Resolver method registry | In Progress | [boot-resolver-method-registry.md](boot-resolver-method-registry.md) — M1–M4 done, M5 (method call checking) remaining |
| Frontend fixes | Done | [archive/boot-frontend-fixes.md](archive/boot-frontend-fixes.md) — correctness, completeness, refactoring, test coverage |
| Core IR & lowering | Done | [archive/boot-core-ir.md](archive/boot-core-ir.md) — Core IR types, AST→Core IR lowering, monomorphization |
| Snapshot testing | Done | [archive/boot-snapshot-testing.md](archive/boot-snapshot-testing.md) — `.boot.expected` files for parser diagnostics |
| Deferred foundation libs (`module`, `graph`, `query`) | Planned | [boot-foundation-libs.md](boot-foundation-libs.md) |

---

## Update Policy

When milestones land:

1. Update the relevant phase status and notes in this file.
2. Link the concrete PR/commit or test evidence in notes.
3. Keep [self-hosting.md](self-hosting.md) as architecture/design source of
   truth; keep this file as execution-status source of truth.
