# Sound Uniqueness and Mutable Lowering

**Status:** Draft plan area

This folder tracks the from-scratch boot-compiler work for sound uniqueness
analysis and compiler-private mutable lowering.

The plan is now split into three owned tracks so the top-level entry point stays
small:

| Track | Owns | Start here |
|---|---|---|
| Analysis | CFG ownership view, ownership facts, liveness/last-use, summaries, candidate-classification inputs, and proof/debug output. Generated code stays unchanged. | [analysis/README.md](analysis/README.md) |
| Codegen | Mapping proven decisions to today's persistent/in-place/builder hooks, including operation catalogs, decision lookup/fallback plumbing, and one lowering family at a time. | [codegen/README.md](codegen/README.md) |
| Migration | Later cleanup after existing-hook lowering works: compiler-private mutable intrinsics, hook consolidation, split-brain cleanup, and Buffer retirement policy. | [migration/README.md](migration/README.md) |

[architecture.md](architecture.md) remains the umbrella design and canonical scope
source. On any divergence, update `architecture.md` first, then re-derive the
track README(s). The focused track docs own the detailed checklists.

## Current focus

**The analysis track is complete through Phase 6 (2026-07-19; recursive-summary
diagnostics closed afterward).** Phases 0-6 produce auditable CFG ownership facts,
liveness, summaries, record shell/field and nested-collection ownership,
transport-wrapper / `Result`-payload return-path summaries, owned-entry recovery,
call-site variant selection, per-call-site owned-decision verdicts, and recursive
SCC variant-qualified diagnostic bodies in `twk ir --cfg` — all without changing
generated code. Field-granular codegen seeding has no summary observable and remains
codegen-owned; variant *generation* and in-place emission also remain codegen work.

The governing rule is **all analysis precision lands before any codegen.** That gate
is now satisfied, so the current focus is the **codegen track**. Phases 7A/7B are
done (2026-07-20): the surviving mutable hooks are inventoried and the
persistent→mutable operation catalog is verified against `main`. Key finding —
vector/dict in-place helpers, builder families, and the record `can_reuse` slot
survive from the previous COW era, but the ownership-driven *rewrite pass* that
selected them was removed. Semantic builder lowering such as `collect` still emits
builders; no ownership decision currently selects vector/dict in-place helpers,
record `can_reuse=true`, or optimizer-selected builder regions. The codegen track
re-drives those surviving hooks from the new sound facts. **Next: Phase 7C** —
ANF-keyed decision records + a centralized selector/handoff layer, then narrow
emitted slices.

## Standing invariants

- **Performance is an end-of-track gate.** During the refactor, judge progress by
  correctness and IR/codegen inspection, not timing claims; run full perf
  comparisons only when the end-to-end path is in place.
- **Codegen stays mechanical.** Backend code consumes ANF-keyed decisions/side
  tables; it never re-proves uniqueness, field ownership, or escape.
- **Persistent fallback is the safety default.** Missing, stale, ambiguous, or
  unsupported mutable decisions emit the ordinary immutable path.

## Track map

### 1. Analysis track

Detailed checklist: [analysis/README.md](analysis/README.md)

Analysis phases (architecture 1A-1E; all analysis precision lands before any codegen):

- Phase 0: baseline and safety rails — done.
- Phase 1: structural CFG ownership view — done.
- Phase 2: minimal ownership facts — done.
- Phase 3: shared optimizer facts and minimal summaries — done.
- Phase 4: record shell/field and nested-collection ownership (1C) — done.
- Phase 5: transport-wrapper and `Result`-payload return-path summaries (1D) — **done**
  (tag-aware variant meet; one param-threaded deferral to Phase 6). Plan archived:
  [../archive/sound-uniqueness-phase5-plan.md](../archive/sound-uniqueness-phase5-plan.md).
- Phase 6: ownership-specialization decision facts (1E) — **done**
  (2a/2b `in_place_paths`, 3 owned-entry re-analysis, 4a whole-return move, 4c-core
  `select_variant`, completion decision verdict rendering, and recursive SCC
  variant-qualified diagnostic bodies in `twk ir --cfg`). Field-granular codegen
  seeding and emitted variant cloning remain codegen-owned. Dated execution plans
  are archived under `docs/plans/archive/`.

### 2. Codegen track

Detailed checklist: [codegen/README.md](codegen/README.md)

This track starts after the completed Phase 6 (1E) specialization decisions; its
input facts are now trustworthy. It is
intentionally split more finely than a single codegen milestone: first operation
catalogs and dry-run decisions (Phase 7), then narrow emitted slices for vectors,
builders, dicts, and records (Phase 8).

### 3. Migration track

Detailed checklist: [migration/README.md](migration/README.md)

This track is not the first codegen implementation. It consolidates successful
existing-hook lowering behind compiler-private mutable intrinsics, removes any
remaining ad hoc legality paths, and eventually evaluates Buffer cleanup.

## Focused docs

| Doc | Purpose |
|---|---|
| [architecture.md](architecture.md) | Umbrella architecture, phases, mutable intrinsics, specialization, testing policy. |
| [analysis/design-rationale.md](analysis/design-rationale.md) | Why static + annotation-free + no-runtime-RC: the Wasm-GC-vs-refcount reason we can't copy Koka/Roc/Lean, the annotation-free/zero-overhead/coverage tradeoff triangle, and what immutable value semantics buys. |
| [analysis/worked-examples.md](analysis/worked-examples.md) | Real boot ANF dumps, op→ownership-event table, and census baseline. |
| [analysis/fact-lattice.md](analysis/fact-lattice.md) | Semantic core: first ownership domain, transfer function, binding-validity split, and later precision layers. |
| [analysis/summary-specialization.md](analysis/summary-specialization.md) | Interprocedural summaries, later path-sensitive summaries, and ownership specialization. |
| [analysis/cfg-ownership-ir.md](analysis/cfg-ownership-ir.md) | CFG ownership view over ANF with SSA-style block parameters and codegen-ready decision concepts. |
| [analysis/sound-analysis.md](analysis/sound-analysis.md) | Required coverage matrix for positive and negative ownership patterns. |
| [analysis/records-fields.md](analysis/records-fields.md) | Record shell vs deep field ownership, wrapper projection, and nested collection ownership. |
| [codegen/README.md](codegen/README.md) | Codegen track checklist and sequencing. |
| [codegen/handoff-contract.md](codegen/handoff-contract.md) | Decision-record handoff, stale fallback, and backend lookup contract. |
| [codegen/operation-catalog.md](codegen/operation-catalog.md) | Mutable operation families and persistent→mutable rewrite targets. |
| [codegen/existing-hooks.md](codegen/existing-hooks.md) | Inventory of current hooks reused by first codegen slices. |
| [codegen/vector-lowering.md](codegen/vector-lowering.md) | Vector indexed update and vector builder slice notes. |
| [codegen/string-lowering.md](codegen/string-lowering.md) | String-concat builder-region slice notes. |
| [codegen/dict-lowering.md](codegen/dict-lowering.md) | Dict set/remove slice notes. |
| [migration/mutable-intrinsics.md](migration/mutable-intrinsics.md) | Later compiler-private mutable intrinsic family and hook cleanup target. |
| [migration/buffer-cleanup.md](migration/buffer-cleanup.md) | Follow-up policy for retiring Buffer workaround usage. |

## Historical references

- [../archive/boot-uniqueness-deep-ownership.md](../archive/boot-uniqueness-deep-ownership.md)
- [../archive/static-uniqueness-plan.md](../archive/static-uniqueness-plan.md)
- [../archive/awfy-c5-inplace-vector.md](../archive/awfy-c5-inplace-vector.md)
