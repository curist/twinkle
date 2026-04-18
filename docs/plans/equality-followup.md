# Equality Follow-Up Plan

Last updated: 2026-04-18

## Goal

Finish the current equality work so Twinkle has a coherent and documented value-
equality story for collection-like values without accidentally turning record
comparison into structural equality.

Primary follow-up targets:

- typed sum structural equality in the boot/runtime path,
- broader positive and negative collection equality coverage,
- explicit tests for the intended boundary between value equality and reference
  equality.

This plan is intentionally a follow-up plan, not a full redesign of equality.

---

## Current Baseline

Recent boot work established the following behavior:

- `Vector<T>` uses runtime structural equality,
- `Dict<K, V>` uses runtime structural equality,
- records still use reference equality,
- primitive equality remains on the direct primitive operator path,
- boot lowers non-primitive `==` / `!=` through runtime equality instead of
  panicking in `op_kind_from`.

Known gap still observed after that change:

- typed sums are not yet uniformly compared structurally in all nested cases,
  especially when collection equality must recurse through `Option`, `Result`,
  or user-defined sum payloads.

Concrete example seen during investigation:

- `Vector<Option<Vector<Int>>>` equality still produced the wrong result.

---

## Intended Semantics

### Value equality

These should compare by value:

- primitives,
- `String`,
- `Vector<T>`, recursively,
- `Dict<K, V>`, by key/value content rather than insertion order,
- typed sums (`Option`, `Result`, user sums), recursively through payloads.

### Reference equality

These should remain reference-equal:

- records.

That means:

- two separately allocated records with identical fields are **not** equal,
- the same record reference compared with itself is equal,
- collection equality may recurse into record payloads, but record payloads
  themselves should still compare by identity.

---

## Non-Goals

This plan does not attempt to:

- make records structural,
- introduce user-defined equality customization,
- add traits/typeclasses/protocol-based equality,
- redesign the primitive equality lowering pipeline,
- unify boot and stage0 naming/model cleanup for builtin exposure.

---

## Problem Breakdown

### E1 — Typed sums are not fully covered by runtime structural equality

The current runtime equality path recognizes `Variant` and some runtime shapes,
but boot now emits typed sum structs for `Option`, `Result`, and user sums.
Those typed sum values need a structural comparison path that works on their
actual runtime representation, not only on erased/bridge forms.

Questions to answer during implementation:

- Are all sum values reaching runtime equality as typed sum structs?
- Which exact Wasm layouts are emitted for:
  - `Option<T>`
  - `Result<T, E>`
  - user-defined sums
- Can equality dispatch detect those layouts directly, or does boot need helper
  calls generated from type layouts?

### E2 — Equality recursion needs explicit coverage across nesting boundaries

The current tests establish only the basic `Vector` and `Dict` cases. We still
need confidence for combinations like:

- `Vector<Option<Int>>`
- `Vector<Option<Vector<Int>>>`
- `Dict<String, Option<Int>>`
- `Dict<String, Vector<Result<Int, String>>>`
- user sum payloads containing vectors/dicts
- vectors/dicts containing records, where the recursive comparison should stop
  at record identity.

### E3 — Negative cases are underspecified

We need targeted tests proving that equality returns false for the right reason:

- different vector lengths,
- different vector element values,
- missing dict key,
- same dict keys with different values,
- same dict contents inserted in different order still equal,
- same-shape records still unequal when separately allocated,
- sum values with different variants are unequal,
- same variant but different payload is unequal.

---

## Implementation Plan

### P1 — Inventory the actual sum runtime representation

Before changing equality again, inspect the current boot backend output and
runtime type layouts for:

- `Option<T>`
- `Result<T, E>`
- one user-defined sum with payloads

Verify whether runtime equality currently sees:

- typed sum structs directly,
- `rt_types__Variant`,
- or both depending on path.

Deliverable:

- short notes in this plan or a linked investigation comment describing the
  actual runtime shapes and the exact mismatch causing the nested-sum failure.

### P2 — Add a dedicated typed-sum structural equality path

Implement structural equality for typed sums in the runtime equality module.

Expected behavior:

- compare variant/tag first,
- compare payload fields in order,
- recurse through runtime equality for each payload field.

Constraints:

- preserve record identity semantics when a payload field is a record,
- do not rely on erased-sum fallback unless that is the actual emitted shape,
- keep `Option`/`Result` and user-defined sums on the same semantic rule.

Possible implementation directions:

1. direct dispatch on typed sum struct layouts,
2. generated helper(s) per sum layout,
3. bridge through an existing typed-sum-to-variant helper if that is already a
   stable runtime boundary.

The implementation should prefer whichever route matches the emitted layout most
naturally and avoids duplicate shape knowledge.

### P3 — Add focused regression tests for typed-sum equality

Add boot tests covering at least:

#### Positive

- `Option<Int>` equality
- `Result<Int, String>` equality
- user sum same variant same payload
- `Vector<Option<Vector<Int>>>` equality
- `Dict<String, Result<Vector<Int>, String>>` equality

#### Negative

- `Option.Some(1) != Option.Some(2)`
- `Option.Some(1) != Option.None`
- `Result.Ok(1) != Result.Err("x")`
- user sum same payload type but different variant
- user sum same variant but different payload

### P4 — Expand collection equality coverage systematically

Add broader positive/negative tests for:

#### Vector

- equal flat vectors
- unequal length
- unequal element
- nested vector equality
- vectors containing sums
- vectors containing records demonstrating identity semantics

#### Dict

- equal dicts with same insertion order
- equal dicts with different insertion order
- missing key
- same keys with different values
- nested dict/vector/sum combinations
- dict values containing records demonstrating identity semantics

### P5 — Add boundary tests for record identity under recursive equality

Make the record rule impossible to regress accidentally.

Required tests:

- top-level record equality is identity-only,
- vectors containing the same record reference compare equal,
- vectors containing separately allocated equal-shaped records compare unequal,
- dict values containing the same record reference compare equal,
- dict values containing separately allocated equal-shaped records compare unequal.

### P6 — Add a direct repro suite for small equality programs

Create a small suite or fixture-style coverage specifically for equality with
short source snippets mirroring the kind of manual `/tmp/eq.tw` checks used
while debugging.

This suite should favor clarity over breadth and should include:

- one vector structural-equality repro,
- one dict order-insensitive equality repro,
- one typed-sum nested collection repro,
- one record identity repro.

---

## Suggested Test Placement

Likely homes:

- `boot/tests/suites/semantic_suite.tw` for user-visible semantic behavior,
- `boot/tests/suites/runtime_suite.tw` for runtime equality dispatch/layout
  assertions,
- a new dedicated suite if the matrix grows enough that semantic tests become
  hard to scan.

Guideline:

- semantic tests should prove visible language behavior,
- runtime tests should prove the runtime module contains the expected helper
  calls/imports/dispatch structure.

---

## Risks

### R1 — Sum equality may accidentally erase record identity

If typed sum comparison blindly deep-compares every field shape, record payloads
could become structural by accident.

Mitigation:

- keep all payload comparison routed through the main runtime equality function,
  whose fallback for records remains identity.

### R2 — Backend layout assumptions may drift

If equality hard-codes one sum representation while the backend emits another,
we will get fragile behavior.

Mitigation:

- complete P1 first,
- prefer helper generation or existing layout queries over duplicated shape
  knowledge where possible.

### R3 — Test matrix can become noisy and redundant

It is easy to add many equality tests that all cover the same code path.

Mitigation:

- keep each test named around one semantic distinction,
- organize by positive/negative and by data shape,
- avoid repeating the same flat-vector case in multiple suites.

---

## Exit Criteria

This plan is complete when all of the following are true:

- nested typed-sum collection equality works correctly,
- `Option`, `Result`, and user-defined sum payload equality is covered,
- vector and dict equality have explicit positive and negative regression cases,
- record identity semantics are locked down by tests,
- there is at least one small repro-style suite preventing a return to the
  original `eq.tw` debugging workflow.
