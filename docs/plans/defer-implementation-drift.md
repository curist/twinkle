# Defer Implementation Drift Plan

Last updated: 2026-03-19

## Goal

Resolve the semantic drift between documented `defer` behavior and current
runtime/lowering behavior, then lock parity across:

- language spec/design docs,
- Core interpreter (`src/interp/eval.rs`),
- ANF defer elimination (`src/opt/defer_elim.rs`),
- run/wasm/opt tests and fixtures.

---

## Decision

Canonical semantics are now fixed:

- `defer` is **block-scoped** (nearest enclosing lexical `{ ... }`).
- execution order is **LIFO** (first-in, last-out).

This plan tracks implementation/docs/test convergence to that model.

---

## Drift Summary

Current mismatch:

1. Docs describe **block-scoped** `defer`:
   - `docs/spec.md` says enclosing-block scope and block-exit triggering.
   - `docs/design/defer.md` says nearest `{ ... }` scope, LIFO on block exit.
2. Existing fixture and observed behavior show **function-scope in `if` blocks**:
   - `tests/run/defer_if.tw` comment expects function-exit timing.
   - Running `twk run tests/run/defer_if.tw` prints:
     - `after if`
     - `inside if`
     - `branch not taken: no defer`
3. Loop behavior currently appears **iteration-scoped**:
   - `tests/run/defer_loop.tw` prints per iteration (`end 0/1/2`).

Net: behavior is effectively hybrid (function-scope for ordinary nested blocks,
iteration-scope for loop body), while docs specify uniformly block-scoped.

---

## Scope

In scope:

- `defer` trigger timing on normal block exit, `return`, `break`, `continue`,
  and trapped exits.
- ordering (LIFO), capture timing (by value), and nested-scope unwinding order.
- parity across interpreter and wasm pipeline.

Out of scope:

- adding new language syntax,
- changing trap model,
- adding resource finalization features beyond current `defer`.

---

## Implementation Plan

## Phase 1 — Freeze Expected Semantics in Tests (Red)

Add/adjust fixtures and harness assertions so semantics are explicit:

- `defer_if` should assert block-exit timing (`inside if` before `after if`).
- add nested-block fixture:
  - inner defer runs when inner block exits, even without `return`.
- add branch non-taken fixture:
  - defer in untaken branch never registers.
- preserve existing loop tests (iteration-end defer behavior).
- add parity tests for both runtimes:
  - interpreter path (`run -i`)
  - wasm path (`run`).

Acceptance for Phase 1:

- at least one test currently fails on existing behavior, proving drift is
  encoded as a regression target.

## Phase 2 — Align Runtime Semantics

### 2.1 Core interpreter (`src/interp/eval.rs`)

Today defer scopes are pushed per function call and per loop iteration.
Implement explicit block-scope push/pop for lexical blocks so `defer` attached
inside an `if`/nested block runs at that block’s exit.

Key constraints:

- keep capture-by-value behavior unchanged.
- keep no-drain-on-trap behavior unchanged.
- preserve LIFO within each lexical scope.
- unwind inner-to-outer on `return`.

### 2.2 ANF defer elimination (`src/opt/defer_elim.rs`)

Adjust elimination so deferred expressions in branch/arm sub-expressions are
attached to the lexical block exit of that branch, not promoted to function
tail behavior.

Key constraints:

- retain current capture snapshot transform.
- maintain loop semantics for `break`/`continue`.
- ensure no `ADefer` survives post-pass.

## Phase 3 — Keep Lowering Invariants Coherent

Audit `src/ir/lower.rs` and `src/ir/lower_anf.rs` to ensure emitted shapes
carry enough scope structure for Phase 2 semantics.

If necessary:

- introduce explicit scope-exit markers in ANF/Core, or
- refine defer-elim context threading to model nested lexical scopes (not only
  fn/loop lists).

## Phase 4 — Documentation + Fixture Sync

After behavior is finalized:

- update any stale fixture comments (`tests/run/defer_if.tw`).
- verify `docs/spec.md` and `docs/design/defer.md` match exact behavior wording.
- add one concise note in `docs/open-questions.md` that defer drift is closed.

---

## Test Matrix (Must Pass for Closure)

1. `if` branch defer timing (taken / not taken).
2. nested block defer timing.
3. loop iteration defer timing.
4. `return` from nested block unwinding order.
5. `break` and `continue` unwind behavior in loop body.
6. trap behavior (no defer drain on trap).
7. capture-by-value for deferred expressions.
8. parity: interpreter output == wasm output for all defer fixtures.

---

## Exit Criteria

This drift is closed when all are true:

1. One canonical defer semantics is documented and approved.
2. Interpreter and wasm paths match that semantics.
3. `tests/run/defer*.tw` comments and outputs reflect the same model.
4. Spec/design docs and implementation tests no longer contradict each other.
