# Backend Pipeline Alignment

**Goal:** Align the actual compiler/backend pipeline with
[../internals/monomorphization.md](../internals/monomorphization.md), so all backend-facing
paths operate on monomorphized Core IR before ANF lowering and Wasm-oriented optimization.

This plan exists because the architecture direction is now clear, but the implementation is
still inconsistent across entrypoints.

---

## Why This Plan Exists

[../internals/monomorphization.md](../internals/monomorphization.md) establishes the intended
pipeline:

```text
parse
→ resolve
→ typecheck
→ lower to Core IR
→ monomorphize
→ lower to ANF
→ optimize
→ emit Wasm IR
→ link / encode
```

That is already effectively true for the Wasm build path, but it is not yet true for every
tooling/debug path that exposes ANF or optimized ANF.

As long as those paths disagree about whether ANF is pre- or post-monomorphization, it is too
easy to:

* reason about the wrong IR shape
* add backend optimizations against non-canonical input
* keep patching Wasm erasure issues before the pipeline contract is fully stable

This plan makes the pipeline contract explicit and brings the implementation in line with it.

## Status

Completed on March 7, 2026.

---

## Current State

Aligned with the intended backend pipeline:

* `twk build`
* `twk run` (default Wasm mode)
* `twk lower-anf`
* `twk opt`
* backend-facing ANF/opt tests and snapshots
* Wasm-oriented build/test helpers that go through the shared backend pipeline

Intentional exception:

* `twk run -i` (interpreter mode) remains on linked Core IR rather than the backend ANF pipeline

The earlier mismatch was that some CLI/debug surfaces showed ANF lowered directly from linked
Core IR rather than from monomorphized Core IR. That mismatch is now removed from the
backend-oriented paths.

---

## Intended Outcome

After this plan:

* backend-facing ANF is always lowered from monomorphized Core IR
* `lower-anf` and `opt` reflect the same pipeline contract as `build` / `run`
* tests and snapshots that inspect ANF or optimized ANF do so against the canonical
  monomorphized input
* it is clear which paths are backend-oriented and which paths intentionally remain
  interpreter/Core-oriented

---

## Scope

In scope:

* aligning backend-oriented CLI entrypoints to monomorphize before ANF lowering
* updating tests, snapshots, and docs that assume the old pre-monomorphization ANF shape
* deciding and documenting whether interpreter-facing paths are exceptions

Out of scope:

* changing the meaning of monomorphization itself
* Wasm layout specialization work
* `Anyref` reduction work beyond what is required to stabilize the pipeline contract

---

## Work Items

### 1. Align `lower-anf` ✅

Update `twk lower-anf` so it reflects the intended backend pipeline:

* compile to Core IR
* monomorphize
* lower to ANF

Questions to settle:

* should `lower-anf` always show monomorphized ANF
* or should it gain an explicit flag if the pre-monomorphization view is still useful

Recommended default:

* make monomorphized ANF the default

### 2. Align `opt` ✅

Update `twk opt` so optimization is shown on top of monomorphized ANF, matching the actual
Wasm build path.

With `--show-original`, the “original ANF” should mean:

* original post-monomorphization ANF

not:

* ANF before monomorphization

### 3. Audit ANF-facing tests and snapshots ✅

Update tests that currently inspect pre-monomorphization ANF or optimized ANF shapes.

Likely areas:

* `tests/anf_test.rs`
* `tests/opt_test.rs`
* snapshot-style tests that inspect build/opt output

The important rule is that backend-facing ANF tests should validate the canonical
monomorphized pipeline, not an older intermediate pipeline.

### 4. Decide interpreter-path policy ✅

Decide whether interpreter paths are intentionally exempt.

* interpreter continues to consume linked Core IR directly
* backend-oriented paths consume monomorphized Core IR before ANF/codegen
* this split is now documented explicitly rather than treated as an accidental inconsistency

### 5. Re-baseline downstream plans ✅

Once this pipeline is aligned, downstream backend plans should assume it.

Downstream backend plans may now assume monomorphized ANF as the canonical input.

---

## Exit Criteria

This plan is done when:

* `twk build` and default `twk run` still use the monomorphized backend pipeline
* `twk lower-anf` lowers from monomorphized Core IR
* `twk opt` optimizes monomorphized ANF
* backend-facing ANF/opt tests and snapshots are updated to match
* any intentional interpreter exception is documented explicitly
* downstream Wasm backend plans can safely assume monomorphized ANF as the canonical input

---

## Follow-On

After this plan completes, the remaining work in
[wasm-type-erasure-reduction.md](wasm-type-erasure-reduction.md) should resume on top of the
aligned pipeline.
