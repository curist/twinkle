# Order + Default Comparator Plan

## Goal

Align Twinkle comparator ergonomics with the Gleam-style model:

1. Introduce an explicit `Order` union.
2. Provide default comparator functions on core primitive types:
   - `Int.compare`
   - `Float.compare`
   - `String.compare`
   - `Byte.compare`
3. Migrate `Vector.sort_by` to consume `Order` instead of `Int`.

This plan is intentionally **breaking** (no backward-compat shims), since the
language is currently single-user and we prefer a clean long-term API.

---

## Why This Change

Current `sort_by` requires `fn(T, T) Int` and often encourages patterns like
`a - b`, which are less explicit and can hide comparator contract mistakes.

An `Order` return type:

* makes comparator intent explicit (`Lt`/`Eq`/`Gt`)
* composes better with default primitive comparators
* matches the record-function style direction already used in Twinkle

---

## Current Baseline (2026-03-14)

* `Vector.sort_by` signature is:
  `fn<T>(xs: Vector<T>, cmp: fn(T, T) Int) Vector<T>`
* No `Order` union exists in prelude/builtins.
* No default primitive comparator functions exist.
* Existing sort call sites in tests/examples currently use int-delta comparators
  (`a - b`, `b - a`).

---

## Target API

```tw
pub type Order = { Lt, Eq, Gt }

pub fn compare(a: Int, b: Int) Order
pub fn compare(a: Float, b: Float) Order
pub fn compare(a: String, b: String) Order
pub fn compare(a: Byte, b: Byte) Order

pub fn sort_by<T>(xs: Vector<T>, cmp: fn(T, T) Order) Vector<T>
```

Usage shape:

```tw
xs.sort_by(Int.compare)
names.sort_by(String.compare)
```

Optional follow-up (not required for this plan):

* `Order.reverse`
* `Order.break_tie` / `Order.lazy_break_tie`

---

## Semantics

### `Order`

* `Order.Lt`: first value is less than second
* `Order.Eq`: equal
* `Order.Gt`: first value is greater than second

### Primitive `compare`

* `Int.compare(a, b)`:
  `< => Lt`, `== => Eq`, `> => Gt`
* `Byte.compare(a, b)`:
  same as int comparison over byte numeric value
* `String.compare(a, b)`:
  lexicographic byte-order comparison (consistent with existing string ordering)
* `Float.compare(a, b)`:
  must be deterministic and documented clearly, including NaN behavior

### `Vector.sort_by`

* Consumes `fn(T, T) Order`
* Pure and deterministic for identical input + comparator
* No mutation of input vector

---

## Delivery Plan

### Milestone 1 — Add `Order` + Primitive `compare`

Implementation sketch:

* add `Order` union (prelude-level type)
* add `compare` functions to:
  - `prelude/int.tw`
  - `prelude/float.tw`
  - `prelude/string.tw`
  - `prelude/byte.tw` (new prelude module if needed)
* ensure both dot and qualified-call forms work:
  - `a.compare(b)`
  - `Int.compare(a, b)` (and peers)

Tests:

* add/extend boot suite coverage for all four compare functions
* include simple qualified-call checks
* include deterministic `Float.compare` behavior checks for chosen NaN policy

Docs:

* add `Order` section + compare methods in `docs/API.md`

### Milestone 2 — Migrate `Vector.sort_by` to `Order`

Implementation sketch:

* update `prelude/vector.tw` signature and implementation:
  - from `fn(T, T) Int` to `fn(T, T) Order`
* update all in-repo call sites to pass `*.compare` or explicit `Order`-returning lambdas

Tests:

* update `boot/tests/suites/api_vector_suite.tw` sort tests
* keep immutability/duplicates/edge-case coverage

Docs:

* update `Vector.sort_by` signature and comparator contract in `docs/API.md`

### Milestone 3 — Cleanup + Example Refresh

Implementation sketch:

* remove remaining int-delta comparator examples from tests/examples
* add positive examples for:
  - `nums.sort_by(Int.compare)`
  - `bytes.sort_by(Byte.compare)`
  - `words.sort_by(String.compare)`

Tests:

* full boot suite passes in interpreter and Wasm
* targeted run fixture(s) for comparator ergonomics

Docs:

* refresh examples in `examples/` to use `compare` by default

---

## Breaking Changes

This plan intentionally introduces a breaking change:

* old: `Vector.sort_by(fn(T, T) Int)`
* new: `Vector.sort_by(fn(T, T) Order)`

No compatibility adapter is planned in this phase.

---

## Risks and Mitigations

* Float ordering edge cases (NaN/-0.0):
  - Mitigation: choose and document one deterministic policy; lock with tests.
* Broad migration churn from changed `sort_by` signature:
  - Mitigation: one-shot repo-wide update and CI pass in same PR.
* Scope creep into advanced comparator combinators:
  - Mitigation: keep `Order.reverse/break_tie` explicitly out of this plan.

---

## Exit Criteria

This plan is complete when:

1. `Order` exists and is documented.
2. `Int/Float/String/Byte.compare` exist and are covered by tests.
3. `Vector.sort_by` consumes `fn(T, T) Order`.
4. In-repo call sites and examples are migrated to the new comparator style.
5. Boot suites pass in interpreter and Wasm backends.
