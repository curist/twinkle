# Minimal API Ergonomics Plan

## Goal

Add a small, high-leverage API surface that improves everyday Twinkle ergonomics
without growing into "me too" stdlib bloat:

1. `Vector.sort_by(cmp)`
2. Lazy iterator combinators: `Iterator.map/filter/take`
3. `Option/Result` composition helpers: `map/and_then`

## Why This Scope

- `Vector.sort_by` is hard to emulate ergonomically and is broadly useful.
- Lazy iterator combinators avoid forced materialization (`to_vector`) and allow
  streaming pipelines.
- `Option/Result` combinators improve expression-level composition around `try`.

Explicitly out of scope in this plan: broad convenience wrappers (`is_empty`,
`entries`, etc.) unless real usage data shows repetition.

## Current Baseline (2026-03-14)

- `Vector<T>` has map/filter/fold/find/any/all/contains/reverse/join, but no sort.
- `Iterator<T>` has `next`, `unfold`, and `to_vector`, but no lazy combinators.
- `Option<T>` has `ok_or`, `ok_or_else`, `transpose`.
- `Result<T,E>` has `transpose`.

## API Additions

### A. Vector sorting

```tw
pub fn sort_by<T>(xs: Vector<T>, cmp: fn(T, T) Int) Vector<T>
```

Semantics:

- `cmp(a, b) < 0` => `a` comes before `b`
- `cmp(a, b) == 0` => equal ordering
- `cmp(a, b) > 0` => `a` comes after `b`
- Pure function: returns a new vector, does not mutate input.
- Deterministic output for identical input and comparator.

### B. Lazy iterator combinators

```tw
pub fn map<T, U>(it: Iterator<T>, f: fn(T) U) Iterator<U>
pub fn filter<T>(it: Iterator<T>, pred: fn(T) Bool) Iterator<T>
pub fn take<T>(it: Iterator<T>, n: Int) Iterator<T>
```

Semantics:

- These are lazy adapters; they should not traverse until consumed.
- `take(it, n)` with `n <= 0` yields an empty iterator.
- Combinators should preserve iterator persistence behavior.

### C. Option/Result composition

```tw
pub fn map<T, U>(opt: Option<T>, f: fn(T) U) Option<U>
pub fn and_then<T, U>(opt: Option<T>, f: fn(T) Option<U>) Option<U>

pub fn map<T, U, E>(res: Result<T, E>, f: fn(T) U) Result<U, E>
pub fn and_then<T, U, E>(res: Result<T, E>, f: fn(T) Result<U, E>) Result<U, E>
```

Semantics:

- `map` transforms only success/present payload.
- `and_then` chains fallible steps without nested wrappers.
- Error/none path passes through unchanged.

## Delivery Plan

### Milestone 1 — `Vector.sort_by`

Implementation sketch:

- Add implementation in `prelude/vector.tw`.
- Add signature stub in `prelude/signatures/vector.tw`.
- Register method for both dot call and qualified call:
  - `xs.sort_by(cmp)`
  - `Vector.sort_by(xs, cmp)`

Tests:

- Extend `boot/tests/suites/api_vector_suite.tw`:
  - basic ascending/descending sort
  - duplicate keys
  - empty/singleton vectors
  - input vector remains unchanged

Docs:

- Add to `docs/API.md` under `Vector<T>`.

### Milestone 2 — Lazy iterator combinators

Implementation sketch:

- Add in `prelude/iterator.tw` using `Iterator.unfold` composition.
- Add signatures in `prelude/signatures/iterator.tw`.

Tests:

- Extend `boot/tests/suites/api_range_iterator_suite.tw`:
  - `map` transforms values lazily
  - `filter` keeps matching values
  - `take` limits traversal
  - composed pipelines (`map` + `filter` + `take`)
  - parity checks versus collect/materialized equivalents

Docs:

- Add methods to `docs/API.md` `Iterator<T>` section with lazy behavior note.

### Milestone 3 — Option/Result `map/and_then` (can be deferred)

Implementation sketch:

- Add Option helpers in `prelude/option.tw`.
- Add Result helpers in `prelude/result.tw`.
- Add method aliases so dot/qualified forms both work.

Tests:

- Extend `boot/tests/suites/api_option_result_suite.tw`:
  - success/present path mapping
  - pass-through for `.None` / `.Err`
  - chain composition without nesting
  - interop with `try` where relevant

Docs:

- Add method entries and short examples in `docs/API.md`.

## Risks and Mitigations

- Comparator misuse in `sort_by` (inconsistent ordering) can produce surprising
  results:
  - Mitigation: document comparator contract clearly in API docs.
- Iterator laziness bugs can accidentally force traversal:
  - Mitigation: add tests that validate bounded consumption with `take`.
- API growth pressure:
  - Mitigation: keep this plan intentionally minimal and usage-driven.

## Exit Criteria

This plan is complete when:

1. All three API groups are implemented (or Milestone 3 is explicitly deferred).
2. Related boot suites are updated and passing in interpreter and Wasm backends.
3. `docs/API.md` includes all new signatures and semantics notes.
