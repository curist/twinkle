# LSP Completion Follow-Up Plan

## Goal

Continue LSP completion work after the archived Phase 2 mixed plan
([archive/lsp-diagnostics-completion.md](archive/lsp-diagnostics-completion.md)).

Primary target: make `textDocument/completion` reliable during active, partially
broken edits without regressing current diagnostics behavior.

---

## Current Baseline (2026-03-13)

Implemented:

* `textDocument/completion` request handling exists in `twk lsp`.
* Semantic completion candidates exist for locals/module/import/member contexts.
* Completion can include detail/documentation payloads.

Current known gap:

* completion depends on successfully analyzed current module state.
* when the current snapshot fails parse/resolve/typecheck, completion may return
  an empty list even while diagnostics are still published.

This is most visible during transient edit states (half-written delimiters,
partial dot-chains, incomplete match/case edits, mid-rename states).

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

### C3 — Broken-Edit Reliability

* keep serving completion from last successful semantic state when latest
  snapshot fails analysis
* continue publishing diagnostics from latest snapshot
* keep fallback behavior bounded to completion path (no protocol surprises)

Likely files:

* `src/lsp/session.rs`
* `src/lsp/completion.rs`
* `src/cli/lsp.rs`

Acceptance:

* incomplete local edits do not collapse completion to empty when prior
  successful semantics are available
* behavior is deterministic and covered by tests

### C4 — Completion Protocol Coverage

* add protocol smoke tests with multiple cursor positions in one document
* include partial/broken edit states and verify labels remain available
* assert protocol payload shape (`CompletionItem`) for core cases

Likely files:

* `src/cli/lsp.rs`
* `tests/lsp_completion_test.rs`

Acceptance:

* test suite reproduces and guards known noisy editor states
* regressions in completion availability are caught in CI

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
