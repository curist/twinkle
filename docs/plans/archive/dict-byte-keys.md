# Dict Byte Keys Plan

## Goal

Allow `Byte` as a first-class dictionary key type:

* from: `Dict<Int, V>` and `Dict<String, V>`
* to: `Dict<Int, V>`, `Dict<String, V>`, and `Dict<Byte, V>`

This is a focused ergonomics/runtime-alignment change, not a general expansion
to arbitrary key types.

---

## Why This Change

`Byte` keys are a natural fit for common tasks:

* byte frequency tables
* byte-to-value lookup maps
* UTF-8 / binary processing pipelines

Current runtime behavior already supports structural key equality for generic
key values; the primary blocker is type-system key restrictions.

---

## Current Baseline (2026-03-14)

* `Dict<K, V>` key type validation currently allows only `Int` and `String`.
* API docs state: "Keys must be `Int` or `String`."
* Interpreter/Wasm dictionary equality paths are generic enough to handle byte
  keys, so this is mostly a typechecker + docs + coverage change.

---

## Scope

In scope:

* type-system acceptance of `Byte` in `Dict<Byte, V>`
* interpreter and Wasm behavior parity validation
* docs and tests updates

Out of scope:

* allowing arbitrary key types
* implicit coercions between `Byte` and `Int` key spaces
* dict representation changes (HAMT/runtime redesign)

---

## Design Rules

* `Dict<Byte, V>` is a distinct type from `Dict<Int, V>`.
* No implicit widening/narrowing for dict key types.
* Existing `Int`/`String` dict behavior remains unchanged.

---

## Delivery Plan

### Milestone 1 — Type System Gate

Implementation sketch:

* update dict-key validation in type resolution/checking to include `Byte`
* ensure diagnostics for invalid key types remain clear and specific

Likely files:

* `src/types/env.rs`
* `src/types/error.rs` (only if message text requires refinement)

Acceptance:

* `Dict<Byte, V>` typechecks
* `Dict<Bool, V>` and other invalid key types still fail

### Milestone 2 — Runtime/Backend Verification

Implementation sketch:

* verify no runtime path assumes only `Int`/`String` keys
* add explicit behavioral tests for byte-key dict operations:
  - set/get/has/remove/len
  - iteration over keys
  - update/overwrite same byte key

Likely files:

* `boot/tests/suites/api_dict_suite.tw`
* `tests/run/dict_methods.tw` (if additional run-level fixture is needed)

Acceptance:

* interpreter and Wasm pass byte-key dict tests
* no regressions for existing dict tests

### Milestone 3 — Docs + Examples

Implementation sketch:

* update API reference key constraints (`Int | String | Byte`)
* add a small byte-frequency style example

Likely files:

* `docs/API.md`
* `examples/` (new or updated example)

Acceptance:

* docs accurately describe key constraints and semantics
* at least one example uses `Dict<Byte, _>`

---

## Test Plan

Boot suite:

* `Dict<Byte, Int>` creation and mutation
* safe lookup and existence checks on byte keys
* overwrite behavior with identical byte key
* no accidental cross-type key mixing (`Byte` keys not interchangeable with `Int`)

Negative typecheck coverage:

* retain failure cases for unsupported dict keys (`Bool`, `Float`, etc.)

Backend parity:

* run boot suites in interpreter and Wasm

---

## Risks and Mitigations

* Risk: hidden assumptions in dict lowering/runtime about key kinds.
  - Mitigation: add explicit byte-key tests in both backends.
* Risk: ambiguity around `Byte` vs `Int` key equivalence.
  - Mitigation: document no implicit key coercion and test distinct key spaces.
* Risk: scope creep to arbitrary key support.
  - Mitigation: keep key whitelist explicit (`Int | String | Byte`).

---

## Exit Criteria

This plan is complete when:

1. `Dict<Byte, V>` typechecks and behaves correctly.
2. Boot dict coverage includes byte-key operations.
3. Interpreter and Wasm suites pass with byte-key tests.
4. `docs/API.md` reflects the new key whitelist.
