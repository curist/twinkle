# Codegen Track

**Status:** Ready to start; the full analysis track is complete through Phase 6
(record/field ownership, transport-wrapper / `Result`-payload return-path summaries,
and ownership-specialization decision facts), so codegen consumes a trustworthy fact
set rather than rediscovering ownership. (These "Codegen Phase 7A/…" labels are the
codegen track's own local numbering; see the phase-numbering note in
[../analysis/README.md](../analysis/README.md).)

This track owns the practical bridge from proof facts to emitted code. It should
first reuse today's persistent/in-place/builder mechanisms, not introduce the
future mutable-intrinsic layer. Source code keeps one immutable API such as
`xs.set_at(i, v)`; internally, a proven site may select a mutable target instead
of the persistent target.

[../architecture.md](../architecture.md) is the umbrella design. The analysis
inputs come from [../analysis/README.md](../analysis/README.md); later cleanup
belongs to [../migration/README.md](../migration/README.md).

## Track invariants

- Codegen consumes decisions; it does not re-prove uniqueness, last-use, field
  ownership, or escape.
- Absence, ambiguity, or staleness of a decision emits the persistent operation.
- Each lowering family starts as a narrow vertical slice with inspection output
  before broadening.
- Existing runtime/compiler hooks are implementation targets, not independent
  legality sources.

## Codegen Phase 7A — Operation catalog and dry-run targets

No emitted-code change. This phase answers: “if this candidate is accepted, what
exact existing target would codegen use?”

- [ ] **Inventory current hooks.** Record the existing vector builder,
  vector set, dict in-place, and record-update slots that can be reused first.
  Details: [existing-hooks.md](existing-hooks.md).
- [ ] **Catalog mutable operation families.** For vectors, dicts, builders, and
  record shells, define the persistent fallback, mutable target, source value,
  result binding, and argument/result mapping. Details:
  [operation-catalog.md](operation-catalog.md).
- [ ] **Print dry-run rewrite targets.** Extend inspection output so accepted or
  potential candidates can say `persistent_target -> mutable_target` without
  changing codegen.
- [ ] **Keep unsupported families persistent.** The catalog may name future
  families, but unsupported or unmapped sites must keep the ordinary immutable
  path.

## Codegen Phase 7B — Decision records and handoff contract

No optimized emission yet. This phase makes the analysis→backend seam explicit
and fail-safe.

- [ ] **Define first-cut decision records.** For each accepted candidate, record
  operation family, ANF key, source value, required ownership fact, last-use
  proof, persistent fallback, mutable target, argument mapping, and proof/debug
  id. Details: [handoff-contract.md](handoff-contract.md).
- [ ] **Attach decisions as ANF-keyed side tables.** Codegen-facing data should be
  stable over the optimized ANF artifact that codegen actually consumes.
- [ ] **Define staleness handling.** If an ANF key no longer resolves, resolves to
  the wrong op family, or has ambiguous mapping, the decision is ignored and the
  persistent path is emitted.
- [ ] **Render decisions before using them.** `twk ir`/census output should show
  decisions and proof ids so the first emitted slice is auditable.

## Codegen Phase 7C — Backend lookup and persistent fallback plumbing

Still no optimized emission. This phase wires the backend to consume the side
table while deliberately returning the persistent target for every site.

- [ ] **Thread the decision table to backend/codegen entry points.** Keep the
  default empty table behavior byte-equivalent to today's persistent output.
- [ ] **Validate lookup/fallback paths.** Exercise present, absent, stale, and
  unsupported decisions; every non-live decision must choose persistent fallback.
- [ ] **Add inspection for consumed vs ignored decisions.** Backend debug output
  should distinguish “decision found but dry-run” from “decision absent/stale.”

## Codegen Phase 8A — First vector indexed-update emission

First emitted-code change. Keep the slice intentionally narrow.

- [ ] **Lower one local owned vector `set_at` family through the existing helper.**
  Require explicit `Unique` + last-use decision; no dicts, no builders, no record
  shells, no specialization.
- [ ] **Keep interleaved reads conservative unless facts explicitly certify them.**
  Unsupported borrow shapes fall back to persistent `Vector.set_at`.
- [ ] **Gate with guard programs and WAT/call inspection.** Positive sites should
  show the mutable helper; negative aliasing cases should still call the
  persistent path.

## Codegen Phase 8B — Loop-carried vector updates

- [ ] **Extend vector indexed-update lowering to loop-carried accumulators.**
  Target shapes like `flags = flags.set_at(k, false)` and
  `balls = balls.set_at(j, updated_ball)` where back-edge facts prove ownership.
- [ ] **Render loop proof ids near emitted decisions.** Debug output should name
  the carried local, update site, borrow sites, and accepted/rejected reason.

## Codegen Phase 8C — Existing vector builder lowering

Builder lowering has a different region shape from indexed update and should not
be bundled with it.

- [ ] **Lower existing vector builder regions from facts.** Reuse current builder
  hooks; do not redesign builder/runtime representation yet.
- [ ] **Preserve semantic builder uses.** `collect` and any builder lowering
  required independent of optimization must keep working without an ownership
  decision.

## Codegen Phase 8D — Dict update emission

- [ ] **Lower proven-owned `Dict.set` through existing in-place helpers.** Preserve
  key lookup semantics and old-version observability.
- [ ] **Lower proven-owned `Dict.remove` only after its helper semantics are
  cataloged.** Insertion-order iteration and value sharing must remain correct.
- [ ] **Keep nested value ownership conservative.** Ownership of a dict backing is
  not ownership of reference-typed values stored inside it.

## Codegen Phase 8E — Record shell update emission

- [ ] **Lower simple record shell updates from facts.** Reuse record shells only
  when CFG facts explicitly permit shell reuse.
- [ ] **Keep shell reuse separate from deep field mutation.** A fresh shell around
  shared fields does not prove ownership of vector/dict storage reachable through
  those fields.
- [ ] **Defer field-sensitive and wrapper-backed wins.** `Set<K>` and transported
  `out.ctx`/`out.state` style wins wait for the analysis precision that proves
  them.

## Codegen Phase 8F — Codegen-track verification gate

- [ ] **Run correctness and aliasing guards.** The negative-aliasing suite must
  remain persistent/correct; positive anchors should lower only where proved.
- [ ] **Inspect emitted helper calls.** Use WAT/call inspection to verify chosen
  helper families rather than relying on timing.
- [ ] **Run performance only as an end-of-track signal.** Full AWFY comparisons
  wait until vector/dict/record paths are end-to-end enough to be meaningful.

## Deferrals to migration

| Deferred work | Home |
|---|---|
| Compiler-private `begin`/`read`/`write`/`freeze` intrinsic family | [../migration/mutable-intrinsics.md](../migration/mutable-intrinsics.md) |
| Migrating current hooks behind the intrinsic layer | [../migration/README.md](../migration/README.md) |
| Removing remaining split-brain mutability decisions | [../migration/README.md](../migration/README.md) |
| Buffer retirement | [../migration/buffer-cleanup.md](../migration/buffer-cleanup.md) |
