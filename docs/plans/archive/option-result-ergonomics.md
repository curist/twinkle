# Option/Result Ergonomics Plan

## Goal

Add ergonomic, explicit bridges from `Option<T>` to `Result<T, E>` and stage
in optional `try` support for `Option`, without regressing current `Result`
behavior.

Primary user-facing target:

```tw
x := try some_option().ok_or("missing x")
```

---

## Current Baseline (2026-03-13)

* `try` currently accepts only `Result<T, E>`:
  * typechecker: `src/types/check.rs` (`ExprKind::Try`)
  * lowerer: `src/ir/lower.rs` (`ExprKind::Try`)
  * spec says "Only for `Result<T,E>`" in `docs/spec.md`.
* There is no `Option.ok_or` / `Option.ok_or_else` in prelude or API docs.
* Prelude method registration for dot syntax depends on
  `builtin_method_alias(type_id)` when prelude exports are registered. `Option`
  / `Result` are not currently mapped there (`src/types/ty.rs`), so adding
  `prelude/*.tw` methods for them needs this wiring.

---

## Scope

In scope:

* `Option.ok_or`
* `Option.ok_or_else`
* (later phase) `try` on `Option<T>` in `Option`-returning contexts
* corresponding updates to language spec and API docs

Out of scope:

* introducing new syntax like `try ... else ...`
* changing existing `Result` `try` semantics
* broad Option/Result combinator expansion beyond this plan

---

## Phases

### Phase 1 — `Option.ok_or` (MVP bridge)

Deliverables:

* Add prelude API:
  * `pub fn ok_or<T, E>(opt: Option<T>, err: E) Result<T, E>`
* Make method callable as:
  * `opt.ok_or(err)`
  * `Option.ok_or(opt, err)`
* Keep `try` semantics unchanged (still `Result` only).

Implementation sketch:

* Add `prelude/option.tw` with pure Twinkle implementation.
* Ensure Option methods are exposed from prelude registration by adding
  `OPTION_TYPE_ID => Some("Option")` in `src/types/ty.rs::builtin_method_alias`.
* Add/extend tests in `boot/tests/suites/api_option_result_suite.tw`.

Acceptance:

* `some.ok_or(err)` returns `.Ok(value)`
* `none.ok_or(err)` returns `.Err(err)`
* `try opt.ok_or("msg")` works cleanly in `Result<_, String>` functions

Documentation updates (required in this phase):

* `docs/API.md`: add `Option.ok_or` signature + examples.
* `docs/spec.md`: add normative Option→Result bridge example in the Result/`try`
  area.

### Phase 2 — `Option.ok_or_else` (lazy bridge)

Deliverables:

* Add prelude API:
  * `pub fn ok_or_else<T, E>(opt: Option<T>, mk_err: fn() E) Result<T, E>`
* Semantics: `mk_err()` is evaluated only for `.None`.

Implementation sketch:

* Implement in `prelude/option.tw` via `case`.
* Add tests proving laziness (e.g. `Cell<Int>` counter increments only on
  `.None` path) in `boot/tests/suites/api_option_result_suite.tw`.

Acceptance:

* `.Some(v).ok_or_else(...)` does not execute closure.
* `.None.ok_or_else(...)` executes closure exactly once.

Documentation updates (required in this phase):

* `docs/API.md`: add `Option.ok_or_else`.
* `docs/spec.md`: clarify eager vs lazy conversion behavior.

### Phase 3 — `try` for `Option` (context-sensitive propagation)

Deliverables:

* Extend `try` to also accept `Option<T>`, with propagation of `.None`.
* Keep existing `Result` behavior unchanged.

Proposed rule:

* `try expr` where `expr: Option<T>` is valid only in functions returning
  `Option<U>` (or equivalent context where early `return .None` is type-valid).
* Desugaring:
  * `.Some(v) => v`
  * `.None => return .None`

Implementation sketch:

* Typechecker changes in `src/types/check.rs` (`ExprKind::Try` branch):
  * accept `Option<T>` when enclosing return type allows `.None` propagation.
  * emit targeted error for invalid contexts.
* Lowering changes in `src/ir/lower.rs` (`ExprKind::Try`):
  * emit match on `OPTION_TYPE_ID` alongside existing `RESULT_TYPE_ID` path.
* Add run/semantic tests for success path, early `None`, and misuse errors.

Acceptance:

* `try` on `Option` works in Option-returning functions.
* `try` on `Option` in Result-returning functions is rejected with clear error
  (until/unless explicit bridge is used, e.g. `.ok_or(...)`).
* Existing `try Result` tests remain green.

Documentation updates (required in this phase):

* `docs/spec.md`: update `try` section from Result-only to dual-mode rules.
* `docs/API.md`: update `Result`/`Option` overview to document `try` behavior.

---

## Testing Plan

Core tests:

* Extend `boot/tests/suites/api_option_result_suite.tw` with:
  * `ok_or` happy/error paths
  * `ok_or_else` lazy evaluation checks
  * chaining with `try`

Compiler/runtime regression checks:

* run existing wasm fixture suites, including:
  * `tests/run/twinkle_typechecker.tw`
* add focused `try Option` fixture/tests when Phase 3 starts.

---

## Risks and Mitigations

* Method registration gap for named types:
  * mitigate via explicit `builtin_method_alias` entry for `Option` (and
    potentially `Result` if Result methods are added later).
* Ambiguous error type inference in `.ok_or(.SomeEnumCase)` style literals:
  * mitigate with diagnostics and allow explicit type annotations.
* Phase 3 complexity (context-sensitive `try Option`):
  * gate behind Phase 1/2 stabilization; keep `.ok_or` as explicit escape hatch.

---

## Exit Criteria

* `Option.ok_or` and `Option.ok_or_else` are shipped, tested, and documented.
* Spec/API docs are updated in the same PRs as behavior changes.
* If Phase 3 is shipped, `try` dual semantics are fully specified and covered by
  regression tests.
