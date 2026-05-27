# LSP Completion Follow-Up Plan

## Goal

Continue LSP completion work after the archived Phase 2 mixed plan
([archive/lsp-diagnostics-completion.md](archive/lsp-diagnostics-completion.md)).

Primary target: make `textDocument/completion` reliable during active, partially
broken edits without regressing current diagnostics behavior.

---

## Current Baseline (2026-05-10)

Implemented:

* `textDocument/completion` request handling in boot compiler LSP.
* Cursor-hole token injection (`lex_with_cursor`) for context classification.
* CursorHole as syntactic fence in 6 parser sites.
* Stale-cache fallback: `completion_snapshot` serves last successful typed state
  when current analysis fails (broken edits).
* Member completion: fields and methods on receiver type via AST walk at
  `dot_offset - 1` (handles stale AST correctly, including chained access).
* Variant completion in `case`: detects enclosing case scrutinee type, suggests
  only that type's variants, and excludes already-matched arms (exhaustive aid).
* General completion: keywords, functions, types, module-scope values with
  prefix filtering.
* Import completion: known extern namespaces.
* Protocol smoke tests: 8 tests covering member, variant (with exhaustive
  filtering), and keyword completion via didOpen → didChange → completion flow.

Current known gaps:

* Function-local variable completions not available (checker locals not persisted).
* Cursor-hole re-parsing not wired into member completion receiver resolution
  (uses stale AST walk; breaks for `foo.|\nbar()` where stale AST merges the
  expressions).
* Import completion only shows extern namespaces, not filesystem module discovery.

---

## Scope

In scope:

* completion reliability under broken/incomplete edit snapshots
* deterministic candidate behavior in identifier and member-access contexts
* protocol-level completion smoke tests across multiple cursor positions

Out of scope:

* snippets/import auto-insertion
* advanced ranking/ML scoring
* non-completion LSP features

---

## Milestones

### C3 — Broken-Edit Reliability ✅

* [x] keep serving completion from last successful semantic state when latest
  snapshot fails analysis (`completion_snapshot` stale-cache fallback)
* [x] continue publishing diagnostics from latest snapshot
* [x] keep fallback behavior bounded to completion path (no protocol surprises)
* [x] fix cursor-hole injection boundary (`>=` → `>` in `lex_with_cursor`)
* [x] fix receiver lookup in stale AST (search at `dot_offset - 1`)
* [x] exhaustive case variant completion (detect scrutinee type, filter matched arms)

Implemented in:

* `boot/compiler/lexer.tw` — `lex_with_cursor`
* `boot/compiler/query/completion.tw` — context classification + candidate gathering
* `boot/lib/lsp/completion.tw` — CompletionItem JSON formatting
* `boot/lib/lsp/server_core.tw` — capability, handler, `completion_snapshot`

### C4 — Completion Protocol Coverage ✅

* [x] 8 protocol smoke tests covering member, variant, exhaustive, and keyword completion
* [x] tests simulate real editor flow (didOpen → didChange → completion)
* [x] assert completion labels for core cases

Implemented in:

* `boot/tests/suites/lsp_completion_suite.tw`

### C5 — Remaining Work

* [x] Function-local variable completions (AST walk for params + let-bindings)
* [x] Wire cursor-hole re-parsing into `member_completions` for correct
  receiver type in cross-line cases (`foo.|\nbar()`)
* [x] Import completion: filesystem module discovery (stdlib via core_lib,
  project modules via filesystem, `@std` suggestion at top level)
* [ ] Snippets / import auto-insertion (out of scope for this plan)

---

## Test Plan

Unit tests:

* context classifier behavior for partial identifier/member edits
* fallback candidate selection with stale-vs-latest snapshot combinations

Integration/protocol tests:

* initialize -> open -> completion (baseline)
* open -> broken change -> completion (fallback path)
* fix change -> completion (latest semantic path restored)

---

## Risks and Mitigations

* stale semantic mismatch:
  keep completion fallback read-only and continue latest diagnostics publishing
* candidate over-noise:
  keep deterministic ordering and lightweight prefix filtering
* hidden regressions:
  enforce protocol smoke coverage for multi-position completion requests

---

## Exit Criteria

* completion remains available during common transient broken edits
* completion protocol behavior is validated by dedicated smoke tests
* diagnostics path remains unchanged by completion hardening
