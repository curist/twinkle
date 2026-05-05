# Boot Backend Verifier Refactor

## Goal

Split backend verification into smaller invariant groups and make the cost of
verification easier to control.

The verifier should continue catching invalid prepared backend IR with readable
errors, but the implementation should be easier to maintain and less expensive
for routine builds.

---

## Motivation

`boot/compiler/backend/verify.tw` is intentionally strict and currently performs
several kinds of validation in one large file. Some checks are cheap structural
invariants; others walk expression trees or recompute facts already made
explicit by `PreparedModule`.

Separating these concerns will make backend evolution safer and creates a path
to configurable verification levels.

---

## Non-Goals

* No weakening of default development verification during the initial split
* No change to Prepared IR semantics
* No change to Wasm emission behavior
* No removal of verifier tests

---

## Target Shape

Possible module split:

```text
boot/compiler/backend/verify.tw              # public entrypoint
boot/compiler/backend/verify_slots.tw        # slot membership and slot tables
boot/compiler/backend/verify_repr.tw         # repr/wasm type consistency
boot/compiler/backend/verify_expr.tw         # expression/control-flow walk
boot/compiler/backend/verify_calls.tw        # callable target invariants
```

Possible verification levels:

```tw
type VerifyLevel = { Basic, Full }
```

`Basic` should contain cheap invariants that are worth keeping always-on.
`Full` should preserve today's stricter development checks.

---

## Work Plan

### Phase 1: Mechanical split

- [ ] Move slot membership checks into a focused module.
- [ ] Move expression walk checks into a focused module.
- [ ] Move repr/type consistency helpers into a focused module.
- [ ] Keep public API and behavior unchanged.

### Phase 2: Share verifier helpers

- [ ] Reuse shared Wasm type equality helpers once available.
- [ ] Reuse backend fact helpers once available.
- [ ] Standardize error formatting helpers across verifier modules.

### Phase 3: Introduce verification levels

- [ ] Define `VerifyLevel` and default to current full behavior.
- [ ] Decide how build entrypoints select basic vs full verification.
- [ ] Keep tests running full verification unless a test specifically targets
      level selection.

### Phase 4: Avoid unnecessary recomputation

- [ ] Identify facts recomputed by the verifier that already exist in
      `PreparedModule` or the Wasm plan.
- [ ] Replace recomputation with prepared facts where that keeps the invariant
      equally strong.
- [ ] Keep cross-checks when they intentionally validate consistency between two
      representations.

---

## Validation

- [ ] Backend verifier suite
- [ ] Backend prepare suite
- [ ] Codegen integration suite
- [ ] Boot self-build with verification enabled
- [ ] Build path using the chosen non-full verification mode, if introduced

---

## Risks

* Splitting the verifier can accidentally hide shared assumptions.
* A basic verification mode must not become a way for invalid IR to reach Wasm
  emission unnoticed in normal development.
* Removing recomputation too aggressively can reduce the verifier's value as an
  independent consistency check.
