# Self-Hosting Status Tracker

Last updated: 2026-03-26

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
| C | ANF lowering + optimization | Done | [archive/boot-anf-lowering.md](archive/boot-anf-lowering.md) — All 12 milestones complete: ANF IR types, Core→ANF lowering, analysis utilities, dead-let/copy-prop/const-fold/branch-simp, pipeline, liveness, uniqueness rewrite, defer elimination, integration tests |
| D | Codegen + linker | Done | [archive/boot-codegen.md](archive/boot-codegen.md) — full pipeline landed (plan_wasm_types → insert_boundaries → emit_module → link → emit_wat), 45/45 Wasmtime regression matrix green, match consolidation and boundary fixes complete. Closure-with-captures remains a known limitation. |
| E | Integration + self-hosting loop | Planned | Depends on A-D milestones and `boot` module/graph/query libs. |

---

## Active Subplans

| Area | Status | Plan |
|------|--------|------|
| Frontend gap closure | Done | [archive/boot-parser-gap-closure.md](archive/boot-parser-gap-closure.md) |
| Resolver fixes | Done | [archive/boot-resolver-fixes.md](archive/boot-resolver-fixes.md) |
| Type checker | Done | [archive/boot-type-checker.md](archive/boot-type-checker.md) — M1–M9 done |
| Resolver method registry | Done | [archive/boot-resolver-method-registry.md](archive/boot-resolver-method-registry.md) — M1–M4 done, M5 deferred to multi-module |
| Resolver hardening | Done | [archive/boot-resolver-hardening.md](archive/boot-resolver-hardening.md) |
| Frontend fixes | Done | [archive/boot-frontend-fixes.md](archive/boot-frontend-fixes.md) — correctness, completeness, refactoring, test coverage |
| Core IR & lowering | Done | [archive/boot-core-ir.md](archive/boot-core-ir.md) — Core IR types, AST→Core IR lowering, monomorphization |
| Snapshot testing | Done | [archive/boot-snapshot-testing.md](archive/boot-snapshot-testing.md) — `.boot.expected` files for parser diagnostics |
| ANF lowering + optimization | Done | [archive/boot-anf-lowering.md](archive/boot-anf-lowering.md) — M1–M12 complete |
| Codegen + linker | Done | [archive/boot-codegen.md](archive/boot-codegen.md), [archive/boot-codegen-followup.md](archive/boot-codegen-followup.md) |
| Deferred foundation libs (`module`, `graph`, `query`) | Planned | [boot-foundation-libs.md](boot-foundation-libs.md) |

---

## Update Policy

When milestones land:

1. Update the relevant phase status and notes in this file.
2. Link the concrete PR/commit or test evidence in notes.
3. Keep [self-hosting.md](self-hosting.md) as architecture/design source of
   truth; keep this file as execution-status source of truth.
