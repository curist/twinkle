# Iterator `to_vector` Plan

## Goal

Add an inherent `Iterator` API for materialization:

* `Iterator.to_vector<T>(it: Iterator<T>) Vector<T>`
* `it.to_vector()`

This should be the canonical method-form equivalent of:

```tw
collect x in it { x }
```

## Current State

* `Iterator` exposes `next` and `unfold` intrinsics (`prelude/signatures/iterator.tw`).
* Users can materialize iterators only through `collect` syntax.
* There is no direct `Iterator` method for conversion to `Vector`.

## Target State

* `to_vector` is available as both `Iterator.to_vector(it)` and `it.to_vector()`.
* Semantics are identical to `collect x in it { x }`.
* No runtime/intrinsic behavior changes are required for this API.

## Non-Goals

* Introducing lazy caching or replayable iterators.
* Changing `Iterator.next` or `Iterator.unfold` semantics.
* Adding a separate `from_vector` API in this plan.

## Proposed Design

Implement as a prelude method in a new `prelude/iterator.tw` module:

```tw
pub fn to_vector<T>(it: Iterator<T>) Vector<T> {
  collect x in it { x }
}
```

Rationale:

* Keeps implementation simple and backend-neutral.
* Reuses existing `collect` lowering and optimizer paths.
* Preserves a single source of truth for iterator materialization behavior.

## Implementation Tasks

### Task A: Prelude API

* Add `prelude/iterator.tw` with `pub fn to_vector<T>(it: Iterator<T>) Vector<T>`.
* Keep function generic and side-effect free.

### Task B: Method Resolution Surface

* Confirm prelude module auto-load path includes the new file.
* Confirm inherent method registration works via existing resolver logic (first parameter `Iterator<T>`).

### Task C: Tests

* Add run fixture covering:
  * empty iterator
  * finite iterator from `Iterator.unfold`
  * method and module-qualified call forms
* Add/extend Twinkle suite coverage under `boot/tests/suites/api_range_iterator_suite.tw`.

### Task D: Docs

* Update `docs/spec.md` and `docs/API.md` with `Iterator.to_vector`.
* Document that infinite iterators will not terminate and materialization is `O(n)` memory.

## Validation

* Interpreter and Wasm outputs match for all new fixtures.
* Existing iterator and collect tests remain green.
* `it.to_vector()` and `collect x in it { x }` produce identical outputs in parity tests.

## Risks

* Users may accidentally materialize very large or infinite iterators.
* API discoverability overlap with `collect` may cause style inconsistency unless docs give guidance.

## Rollout

1. Land prelude API and tests.
2. Update docs/spec.
3. Optionally migrate selected fixtures/docs from `collect` to `to_vector` examples for discoverability.
