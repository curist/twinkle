# Deterministic WAT Output Plan

## Goal

Eliminate non-deterministic ordering in WAT output so that build pipeline
snapshot tests pass reliably across runs.

---

## Symptom

`build_snapshot_hello`, `build_snapshot_arithmetic`, and `build_snapshot_records`
intermittently fail when run as part of the full test suite (`cargo test`). The
diff shows `$user__UserRecord_N` type definitions with swapped field types
between runs — e.g. `UserRecord_8` alternates between having a closure field and
an i64 field.

This is a pre-existing issue (reproduces on `main` before any new changes).

---

## Root Cause

### Primary: unstable `TypeId` assignment during type resolution

In `src/types/resolve.rs`, declarations are collected in `HashMap`s and then
iterated to assign placeholder `TypeId`s (`TypeEnv::add_type`). Because HashMap
iteration order is intentionally non-deterministic, user-declared type names can
receive different numeric `TypeId`s between runs.

WAT record type symbols are numeric (`UserRecord_<TypeId>`), so changing
`TypeId` assignment changes which logical record appears under a given
`UserRecord_N` name, producing snapshot diffs that look like field-type swaps.

### Secondary: codegen hash maps worth auditing, but not primary for this symptom

`concrete_func_sigs` in codegen is HashMap-backed, but typed closure/type
registries already normalize emission through `BTreeMap`s. This path should be
kept as a follow-up audit target, not the first-line fix for the observed
`UserRecord_N` instability.

---

## Scope

In scope:

* Make WAT output deterministic for identical source input
* Fix existing snapshot test flakiness
* Stabilize resolver declaration processing order

Out of scope:

* Performance optimization of the codegen pipeline
* Changing the monomorphization algorithm
* Broad HashMap->BTreeMap churn in codegen unless needed after resolver fix

---

## Proposed Fix

Stabilize resolver ordering by preserving source declaration order end-to-end.

1. **Track source order during declaration collection**
   * Add `type_decl_order: Vec<String>` and `function_decl_order: Vec<String>`
     in `Resolver`.
   * Append names when declarations are collected.

2. **Use stored order for resolution passes**
   * Build ordered declaration vectors from the order lists.
   * Assign placeholder `TypeId`s from that ordered vector.
   * Resolve function signatures from ordered function declarations.

3. **Follow-up audit (only if needed)**
   * If flakiness remains, audit codegen HashMap iteration sites and sort before
     emission where required.

### Likely files

* `src/types/resolve.rs` — `Resolver` fields and ordered iteration
* `tests/build_pipeline_snapshot_test.rs` — validation target (no code changes required)

---

## Delivery Plan

### Milestone 1 — Resolver ordering stabilization

* Implement declaration-order tracking in `Resolver`
* Use declaration order for placeholder `TypeId` assignment
* Use declaration order for function signature registration
* Verify: `cargo test --test build_pipeline_snapshot_test` passes consistently
  across 10+ consecutive runs

### Milestone 2 — Snapshot refresh (only if canonical output changed)

* If snapshots differ after deterministic fix, run:
  `UPDATE_SNAPSHOTS=1 cargo test --test build_pipeline_snapshot_test`
* Re-run without `UPDATE_SNAPSHOTS` to confirm stability

### Milestone 3 — Broader regression sanity

* Run full `cargo test`
* If failures indicate additional ordering nondeterminism, add targeted
  ordering fixes with focused tests

---

## Test Plan

* Run `cargo test --test build_pipeline_snapshot_test` in a loop (10+ runs) to
  confirm determinism
* Compare repeated generated WAT outputs for byte-identical results
* Run full `cargo test` to catch regressions

---

## Risks and Mitigations

* Risk: changing type declaration order semantics could affect existing `TypeId`
  expectations in snapshots.
  - Mitigation: refresh snapshots once after fix and keep order rule explicit
    (source order).
* Risk: residual nondeterminism from other HashMap-backed paths.
  - Mitigation: follow-up audit for emitter iteration points if flakiness
    persists.

---

## Exit Criteria

This plan is complete when:

1. `cargo test --test build_pipeline_snapshot_test` passes across repeated runs
   with no snapshot flakiness.
2. WAT output is deterministic for identical source input.
3. Full `cargo test` passes (or remaining failures are unrelated and documented).
