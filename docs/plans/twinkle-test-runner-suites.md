# Twinkle Test Runner Suite Authoring Plan

Plan for expanding and systematizing `.tw` test suites under `boot/tests/` using the Twinkle-native test runner.

---

## Goal

Create a repeatable authoring workflow so new language/API behavior can be validated in Twinkle suites first, with deterministic output and easy filtering.

---

## Current Baseline

- Runner API exists in `boot/tests/runner.tw`:
  - `runner.suite(name)` / `.test(name, fn() Result<Void, String>)`
  - `runner.run_all([...])`
  - `TWK_TEST_FILTER` substring filtering (`test` or `suite::test`)
  - `NO_COLOR` support
- Assertions exist in `boot/tests/assert.tw` and return `Result<Void, String>`.
- Active suites:
  - `assert_helpers_suite.tw`
  - `semantic_suite.tw`
- Verified commands:
  - `cargo run -- run -i boot/tests/main.tw`
  - `cargo run -- run boot/tests/main.tw`
  - `TWK_TEST_FILTER='short-circuit' NO_COLOR=1 cargo run -- run -i boot/tests/main.tw`

---

## Scope and Non-Goals

Scope:
- Authoring and organizing Twinkle-runner suites (`boot/tests/suites/*.tw`).
- Mapping suite coverage to `docs/spec.md` and `docs/API.md`.
- Defining naming, assertion, and execution conventions.

Non-goals:
- Replacing Rust-side parser/typechecker golden tests.
- Compile-fail coverage (keep in Rust harness; Twinkle runner is runtime/pass coverage).
- Redesigning the runner output format.

---

## Suite Authoring Contract

1. Each suite file exports exactly one `pub fn suite() runner.Suite`.
2. Each test callback returns `Result<Void, String>`.
3. Use `try assert.*(...)` for checks and end with `.Ok({})`.
4. Keep test data local and deterministic (no random/time/process spawning).
5. Name tests by behavior, not implementation detail, so `TWK_TEST_FILTER` stays useful.
6. Prefer one semantic claim per test; split multi-claim flows into multiple tests unless setup is expensive.
7. Use helper functions inside the suite file for repeated setup/transforms.

---

## Coverage Map (Spec/API → Suites)

| Source | Behavior to lock down | Planned suite |
|-------|------------------------|---------------|
| Spec §7.3–§7.7 | rebinding, aliasing, closure capture, loop capture | `semantic_closure_rebinding_suite.tw` |
| Spec §12, §18 | control flow, early return, `try` propagation | `semantic_control_flow_suite.tw` |
| API: `Option`, `Result` | construction, matching, helper flows | `api_option_result_suite.tw` |
| API: `Cell` | explicit shared mutable state semantics | `api_cell_suite.tw` |
| API: `Vector` + Spec §14 | immutable ops, safe/unsafe indexing behavior | `api_vector_suite.tw` |
| API: `String` + Spec §11/§15 | interpolation, slicing/index semantics, predicates | `api_string_suite.tw` |
| API: `Dict` + Spec §17 | key constraints behavior, lookup/update iteration | `api_dict_suite.tw` |
| API: `Range`, `Iterator` + Spec §12/§13/§16 | loop/collect iteration behavior | `api_range_iterator_suite.tw` |
| API: `@std.path` | pure path transforms | `stdlib_path_suite.tw` |
| API: `@std.fs`, `@std.proc` | host integration contracts (deterministic subset) | `stdlib_host_suite.tw` |

Note: keep `semantic_suite.tw` as a smoke suite; move deeper edge coverage into dedicated domain suites.

---

## Rollout Plan

### Phase 1: Foundation and conventions

- [ ] Add `boot/tests/suites/README.md` with suite contract and template.
- [ ] Normalize current suite naming style (`<domain> <behavior>`).
- [ ] Add missing assertion helpers only when duplicated logic appears in 3+ tests.

### Phase 2: Semantic core expansion (spec-driven)

- [ ] Add `semantic_closure_rebinding_suite.tw`.
- [ ] Add cases for closure non-rebinding error boundaries represented as runtime-adjacent behavior where possible.
- [ ] Add control-flow edges (nested `case`, `for` with `break`/`continue`, `return` through helpers).

### Phase 3: API surface expansion (API.md-driven)

- [ ] Add Option/Result suite (including `try` usage patterns).
- [ ] Add Vector/String/Dict suites with both normal and edge behavior.
- [ ] Add Range/Iterator suite for `for` and `collect` combinations.

### Phase 4: Stdlib and host-boundary coverage

- [ ] Add path suite (`@std.path`) with normalization/join corner cases.
- [ ] Add host suite (`@std.fs`, `@std.proc`) with deterministic assertions only.
- [ ] Avoid environment-dependent expectations (assert shape, not machine-specific values).

### Phase 5: Execution matrix and CI hooks

- [ ] Run all suites in interpreter mode (`-i`) and Wasm mode.
- [ ] Add CI command pair for both modes.
- [ ] Keep filterability by ensuring unique test names across suites.

---

## Authoring Template

```tw
use tests.runner
use tests.assert

pub fn suite() runner.Suite {
  runner.suite("vector api")
    .test("push returns new vector", fn() {
      xs: Vector<Int> = [1]
      ys := xs.push(2)
      try assert.int_eq(xs.len(), 1)
      try assert.int_eq(ys.len(), 2)
      .Ok({})
    })
}
```

And register it in `boot/tests/main.tw`:

```tw
use tests.suites.vector_api_suite

runner.run_all([
  // ...
  vector_api_suite.suite(),
])
```

---

## Definition of Done

- [ ] Each high-value spec/API area above has at least one dedicated suite.
- [ ] Full run passes in both backends:
  - `cargo run -- run -i boot/tests/main.tw`
  - `cargo run -- run boot/tests/main.tw`
- [ ] Filtered run works predictably for suite/test names.
- [ ] New suite additions require only: file + `use` + `run_all([...])` registration.
