# Codegen Track

**Status:** Ready to start; the full analysis track is complete through Phase 6
(record/field ownership, transport-wrapper / `Result`-payload return-path summaries,
ownership-specialization decision facts, and recursive SCC variant-qualified
diagnostics), so codegen consumes a trustworthy fact set rather than rediscovering
ownership. (These "Codegen Phase 7A/…" labels are the codegen track's own local
numbering; see the phase-numbering note in [../analysis/README.md](../analysis/README.md).)

This track owns the practical bridge from proof facts to emitted code. It should
first reuse today's persistent/in-place/builder mechanisms, not introduce the
future mutable-intrinsic layer. Source code keeps one immutable API such as
`xs.set_at(i, v)`; internally, a proven site may select a mutable target instead
of the persistent target.

[../architecture.md](../architecture.md) is the umbrella design. The analysis
inputs come from [../analysis/README.md](../analysis/README.md); later cleanup
belongs to [../migration/README.md](../migration/README.md).

## Focused slice docs

| Doc | Purpose |
|---|---|
| [existing-hooks.md](existing-hooks.md) | Current backend/runtime hooks and non-hooks to verify before codegen work. |
| [operation-catalog.md](operation-catalog.md) | Mutable operation families and persistent→mutable target mapping. |
| [handoff-contract.md](handoff-contract.md) | ANF-keyed decision records, stale fallback, and variant-routing records. |
| [vector-lowering.md](vector-lowering.md) | Vector indexed update and vector builder slice notes. |
| [string-lowering.md](string-lowering.md) | String-concat builder-region slice notes. |
| [dict-lowering.md](dict-lowering.md) | Dict set/remove slice notes. |

## Track invariants

- Codegen consumes decisions; it does not re-prove uniqueness, last-use, field
  ownership, or escape.
- Absence, ambiguity, or staleness of a decision emits the persistent operation.
- Each lowering family starts as a narrow vertical slice with inspection output
  before broadening.
- Existing runtime/compiler hooks are implementation targets, not independent
  legality sources.
- Variant-qualified diagnostics never license rewriting the generic function body.
  Codegen must clone/route by exact `VariantId`, with the generic function as the
  persistent fallback.

## Codegen Phase 7A — Existing hook inventory

No emitted-code change. Start with the concrete backend/runtime surface we already
have before designing side tables around it.

- [ ] **Inventory current hooks.** Record the existing vector set, vector/string
  builder, dict in-place, record-update, and function-cloning/routing mechanisms
  that can be reused first. Details: [existing-hooks.md](existing-hooks.md).
- [ ] **Verify hook signatures.** For each hook, record helper/op names, operand
  order, result behavior, monomorphized type restrictions, persistent fallback,
  and WAT/call-inspection signature.
- [ ] **Classify non-hooks.** Keep user-facing scaffolding such as `@std.buffer`
  out of this track; migration owns later cleanup.

## Codegen Phase 7B — Operation catalog

No emitted-code change. This phase answers: “if this candidate is accepted, what
exact existing target would codegen use?”

- [ ] **Catalog mutable operation families.** For vectors, strings, dicts,
  builders, record shells, record-backed field collections, and ownership-specialized
  function variants, define the persistent fallback, mutable target, source value,
  result binding, and argument/result mapping. Details: [operation-catalog.md](operation-catalog.md).
- [ ] **Keep unsupported families persistent.** The catalog may name future
  families, but unsupported or unmapped sites must keep the ordinary immutable
  path.
- [ ] **Name inspection signatures.** Each catalog entry should say what `twk ir`,
  WAT, or call-list evidence proves the mutable target would be selected.

## Codegen Phase 7C — Decision records and handoff contract

No optimized emission yet. This phase makes the analysis→backend seam explicit
and fail-safe.

- [ ] **Define first-cut decision records.** For each accepted candidate, record
  operation family, ANF key, source value, required ownership fact, last-use
  proof, persistent fallback, mutable target, argument mapping, `VariantId` when
  applicable, and proof/debug id. Details: [handoff-contract.md](handoff-contract.md).
- [ ] **Attach decisions as ANF-keyed side tables.** Codegen-facing data should be
  stable over the optimized ANF artifact that codegen actually consumes.
- [ ] **Define staleness handling.** If an ANF key no longer resolves, resolves to
  the wrong op family, or has ambiguous mapping, the decision is ignored and the
  persistent path is emitted.

## Codegen Phase 7D — Backend lookup and persistent fallback plumbing

Still no optimized emission. This phase wires the backend to consume an empty or
ignored side table while deliberately returning the persistent target for every site.

- [ ] **Thread the decision table to backend/codegen entry points.** Keep the
  default empty table behavior byte-equivalent to today's persistent output.
- [ ] **Validate lookup/fallback paths.** Exercise present, absent, stale, and
  unsupported decisions; every non-live decision must choose persistent fallback.
- [ ] **Keep fallback centralized.** Backend code should ask the decision table for
  a target and receive either an exact mutable target or the ordinary persistent
  target, not hand-roll legality checks per family.

## Codegen Phase 7E — Dry-run rendering

No optimized emission yet. This phase proves the seam and inspection story before
any helper or cloned variant is emitted.

- [ ] **Print dry-run rewrite targets.** Extend inspection output so accepted or
  potential candidates can say `persistent_target -> mutable_target` without
  changing codegen.
- [ ] **Render consumed vs ignored decisions.** Backend debug output should
  distinguish “decision found but dry-run” from “decision absent/stale.”
- [ ] **Include variant routing dry-runs.** For Case V-shaped calls, render the
  generic callee, would-be cloned callee, exact `VariantId`, route site, and
  fallback reason.

## Codegen Phase 8A — Local vector indexed-update emission

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

## Codegen Phase 8C — Existing builder-region lowering

Builder lowering has a different region shape from indexed update and should not
be bundled with it. Current boot hooks cover vector builders and string concat
builders; typed vector builder shims (`i64`/`bool`) are implementation details of
the vector family. Details: [vector-lowering.md](vector-lowering.md) and
[string-lowering.md](string-lowering.md).

- [ ] **Lower existing vector builder regions from facts.** Reuse current
  `vector$builder_new/from/push/freeze` hooks; do not redesign builder/runtime
  representation yet.
- [ ] **Lower existing string builder regions from facts.** Reuse current
  `string$builder_from/extend/freeze` hooks for `String.concat` accumulator loops.
- [ ] **Preserve semantic builder uses.** `collect` and any builder lowering
  required independent of optimization must keep working without an ownership
  decision.
- [ ] **Keep unregistered builder helpers out of scope until cataloged.** The
  runtime exports additional helpers such as vector `builder_extend`, but they are
  not currently a first-cut boot builtin/shim target.

## Codegen Phase 8D — Dict set emission

- [ ] **Lower proven-owned `Dict.set` through existing in-place helpers.** Preserve
  key lookup semantics and old-version observability.
- [ ] **Keep nested value ownership conservative.** Ownership of a dict backing is
  not ownership of reference-typed values stored inside it.
- [ ] **Inspect dict helper selection.** Positive sites should show the in-place
  set helper; negative aliasing/old-version cases should still call the persistent
  path.

## Codegen Phase 8E — Dict remove emission

`Dict.remove` uses related machinery but has separate ordering and helper semantics;
do not bundle it with set.

- [ ] **Catalog and verify remove helper semantics first.** Insertion-order
  iteration, missing-key behavior, and old-version observability must remain correct.
- [ ] **Lower proven-owned `Dict.remove` through existing in-place helpers.** Fall
  back to the persistent path for unsupported or stale decisions.
- [ ] **Keep remove inspection distinct from set.** Debug output should make it
  obvious which helper family was selected.

## Codegen Phase 8F — Record shell update emission

- [ ] **Lower simple record shell updates from facts.** Reuse record shells only
  when CFG facts explicitly permit shell reuse.
- [ ] **Keep shell reuse separate from deep field mutation.** A fresh shell around
  shared fields does not prove ownership of vector/dict storage reachable through
  those fields.
- [ ] **Preserve variant qualification.** If shell reuse is proven only in a
  `variant fn ... [unique:...]` diagnostic body, emit it only in the cloned
  ownership-specialized variant, never in the generic function body.

## Codegen Phase 8G — Ownership-specialized function variants and call-site routing

This phase turns Phase 6's printed specialization story into real functions. It is
required for [worked-examples Case V](../analysis/worked-examples.md#case-v--graph_sccvisit-the-whole-compiler-idiom-real):
`graph_scc.visit`'s generic body stays conservative, while `variant fn visit
[unique:p0]` supplies the shell-reuse decisions for owned callers and recursive calls.

- [ ] **Clone functions by exact `VariantId`.** Generate ownership-specialized
  variants only for accepted, reachable keys; keep the original function as the
  persistent/generic fallback.
- [ ] **Route call sites from decision records.** A caller with a live owned
  decision calls the matching clone; absent, stale, over-cap, unsupported, or
  ambiguous decisions call the generic function.
- [ ] **Tie recursive and mutual-recursive calls through the same key.** Inside a
  cloned SCC member, recursive calls that the variant analysis proved reachable
  route to the matching clone/peer clone, not back to the generic summary by
  accident.
- [ ] **Keep variant count capped and inspectable.** Render clone names, source
  `VariantId`, fallback reason, and proof id in `twk ir`/WAT inspection.

## Codegen Phase 8H — Record-backed field collection updates

This phase composes the dict/vector and record-shell slices for the compiler's
common quartet (`record_get` → collection update → `record_update` → `assign`). It
covers `Set<K>` wrappers, transported `out.ctx`/`out.state` records, and Case V's
`cur.indices[...]`, `cur.stack = ...`, and similar field-backed updates.

- [ ] **Lower field-backed dict/vector updates only with explicit field-path
  decisions.** The decision must name the record shell path, projected collection
  path, old-field liveness proof, persistent fallback, and mutable collection target.
- [ ] **Allow shell-only wins without deep field wins.** A cloned `visit` variant
  may reuse the `State` shell while a dict/vector field update remains persistent;
  do not require all-or-nothing lowering.
- [ ] **Preserve sibling and nested-value safety.** Reads of disjoint sibling paths
  are allowed only when licensed; ownership of a dict/vector backing is not
  ownership of reference-typed values stored inside it.

## Codegen Phase 8I — Codegen-track verification gate

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
