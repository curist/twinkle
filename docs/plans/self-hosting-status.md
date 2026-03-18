# Self-Hosting Status Tracker

Last updated: 2026-03-18

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
| A | Frontend (lexer/parser/resolver/checker) | In Progress | Lexer done. Parser done. Resolver functional with arity checks, topo-sorted type resolution, circular alias detection, and full error collection. Type checker M1–M9 done; method registry M1–M4 done; frontend fixes plan complete (correctness, completeness, refactoring, test coverage). |
| B | Core IR lowering + monomorphization | Planned | No committed self-hosted Core IR pipeline yet. |
| C | ANF lowering + optimization | Planned | No committed self-hosted ANF/opt pipeline yet. |
| D | Codegen + linker | Planned | Representation/layout redesign is documented in [self-hosting.md](self-hosting.md). |
| E | Integration + self-hosting loop | Planned | Depends on A-D milestones and `boot` module/graph/query libs. |

---

## Active Subplans

| Area | Status | Plan |
|------|--------|------|
| Frontend gap closure | Planned | [boot-parser-gap-closure.md](boot-parser-gap-closure.md) |
| Resolver fixes | Done | [archive/boot-resolver-fixes.md](archive/boot-resolver-fixes.md) |
| Type checker | In Progress | [boot-type-checker.md](boot-type-checker.md) — M1–M9 done |
| Resolver method registry | In Progress | [boot-resolver-method-registry.md](boot-resolver-method-registry.md) — M1–M4 done, M5 (method call checking) remaining |
| Frontend fixes | Done | [archive/boot-frontend-fixes.md](archive/boot-frontend-fixes.md) — correctness, completeness, refactoring, test coverage |
| Deferred foundation libs (`module`, `graph`, `query`) | Planned | [boot-foundation-libs.md](boot-foundation-libs.md) |

---

## Unaddressed Plans

| Plan | Current State |
|------|---------------|
| [boot-parser-gap-closure.md](boot-parser-gap-closure.md) | Defined, but milestone execution has not started. |

---

## Update Policy

When milestones land:

1. Update the relevant phase status and notes in this file.
2. Link the concrete PR/commit or test evidence in notes.
3. Keep [self-hosting.md](self-hosting.md) as architecture/design source of
   truth; keep this file as execution-status source of truth.
