# Option/Result Transpose Plan

## Goal

Add ergonomic, explicit wrapper-order conversion helpers for nested
`Option`/`Result` values, without changing existing `try` behavior.

Primary user-facing target:

```tw
value_opt := try opt_res.transpose()
```

where `opt_res: Option<Result<T, E>>` and the transposed value is
`Result<Option<T>, E>`.

---

## Current Baseline (2026-03-13)

* `try` currently unwraps only `Result<T, E>`:
  * typechecker: `src/types/check.rs` (`ExprKind::Try`)
  * lowerer: `src/ir/lower.rs` (`ExprKind::Try`)
  * spec states Result-only `try` behavior in `docs/spec.md`.
* There is no `Option.transpose` / `Result.transpose` in prelude or API docs.
* Dot-method prelude registration depends on
  `src/types/ty.rs::builtin_method_alias(type_id)`; `Option`/`Result` method
  alias coverage must exist for these APIs to be callable as methods.

---

## Scope

In scope:

* `Option.transpose` for `Option<Result<T, E>> -> Result<Option<T>, E>`
* `Result.transpose` for `Result<Option<T>, E> -> Option<Result<T, E>>`
* method-call and namespaced-call forms for both (`x.transpose(...)` and
  `Type.transpose(...)`)
* corresponding updates to API/spec docs and focused tests

Out of scope:

* generic wrapper-swapping abstractions
* trait-based carrier conversion
* new `try` behavior or syntax
* broad Option/Result combinator expansion beyond `transpose`

---

## Phases

### Phase 1 — `Option.transpose` (high-value direction)

Deliverables:

* Add prelude API:
  * `pub fn transpose<T, E>(opt: Option<Result<T, E>>) Result<Option<T>, E>`
* Make method callable as:
  * `opt.transpose()`
  * `Option.transpose(opt)`
* Keep `try` semantics unchanged (Result-only unwrap).

Semantics:

* `.None => .Ok(.None)`
* `.Some(.Ok(v)) => .Ok(.Some(v))`
* `.Some(.Err(e)) => .Err(e)`

Implementation sketch:

* Add/extend `prelude/option.tw` with a pure Twinkle `case` implementation.
* Ensure `Option` method alias wiring exists in
  `src/types/ty.rs::builtin_method_alias`.
* Add/extend tests in `boot/tests/suites/api_option_result_suite.tw`.

Acceptance:

* All semantic cases above evaluate correctly.
* `try opt_res.transpose()` works cleanly in `Result<_, E>` functions, yielding
  `Option<T>` on success.

Documentation updates (required in this phase):

* `docs/API.md`: add `Option.transpose` signature + examples.
* `docs/spec.md`: add normative `Option<Result<T,E>> -> Result<Option<T>,E>`
  conversion behavior.

### Phase 2 — `Result.transpose` (symmetry and inverse direction)

Deliverables:

* Add prelude API:
  * `pub fn transpose<T, E>(res: Result<Option<T>, E>) Option<Result<T, E>>`
* Make method callable as:
  * `res.transpose()`
  * `Result.transpose(res)`
* Preserve existing `try` behavior (still unwraps only outer `Result`).

Semantics:

* `.Err(e) => .Some(.Err(e))`
* `.Ok(.None) => .None`
* `.Ok(.Some(v)) => .Some(.Ok(v))`

Implementation sketch:

* Add/extend `prelude/result.tw` with a pure Twinkle `case` implementation.
* Ensure `Result` method alias wiring exists in
  `src/types/ty.rs::builtin_method_alias`.
* Extend `boot/tests/suites/api_option_result_suite.tw`.

Acceptance:

* All semantic cases above evaluate correctly.
* Roundtrip checks pass for representative values:
  * `x.transpose().transpose() == x` for both supported nested shapes.

Documentation updates (required in this phase):

* `docs/API.md`: add `Result.transpose` signature + examples.
* `docs/spec.md`: document the inverse relationship of the two transpose
  operations.

---

## Testing Plan

Core tests:

* Extend `boot/tests/suites/api_option_result_suite.tw` with:
  * direct-case coverage for `Option.transpose`
  * direct-case coverage for `Result.transpose`
  * roundtrip/inverse checks
  * `try` composition examples (Result unwrap only)

Compiler/runtime regression checks:

* run existing wasm fixture suites, including:
  * `tests/run/twinkle_typechecker.tw`
* ensure existing `try Result` behavior remains unchanged.

---

## Risks and Mitigations

* Method registration gap for named types:
  * mitigate via explicit `builtin_method_alias` entries for both `Option` and
    `Result`.
* Nested literal/type inference ambiguity (`.Err(...)`, `.Some(...)`) in
  unannotated contexts:
  * mitigate with targeted diagnostics and test coverage for annotated examples.
* User confusion about `try` after transpose:
  * mitigate with docs that explicitly state `try` unwraps only outer `Result`.

---

## Exit Criteria

* `Option.transpose` and `Result.transpose` are shipped, tested, and documented.
* Spec/API docs are updated in the same PRs as behavior changes.
* Existing Result `try` behavior remains unchanged and regression-tested.
