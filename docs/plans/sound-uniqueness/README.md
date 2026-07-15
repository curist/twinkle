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

The analysis track's first proof-producing milestone is complete: Phases 0-3 now
produce auditable CFG ownership facts, liveness, and minimal summaries without
changing generated code. The next implementation focus is the **codegen track**:
ANF-keyed decision handoff/fallback plumbing, then narrow emitted slices for the
existing mutable hooks.

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

- Phase 0: baseline and safety rails — done.
- Phase 1: structural CFG ownership view — done.
- Phase 2: minimal ownership facts — done.
- Phase 3: shared optimizer facts and minimal summaries — done.

### 2. Codegen track

Detailed checklist: [codegen/README.md](codegen/README.md)

This track starts after the analysis facts are trustworthy enough to drive
candidate decisions. It is intentionally split more finely than the old Phase 4/5:
first operation catalogs and dry-run decisions, then backend lookup/fallback
plumbing, then narrow emitted slices for vectors, builders, dicts, and records.

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
| [migration/mutable-intrinsics.md](migration/mutable-intrinsics.md) | Later compiler-private mutable intrinsic family and hook cleanup target. |
| [migration/buffer-cleanup.md](migration/buffer-cleanup.md) | Follow-up policy for retiring Buffer workaround usage. |

## Historical references

- [../archive/boot-uniqueness-deep-ownership.md](../archive/boot-uniqueness-deep-ownership.md)
- [../archive/static-uniqueness-plan.md](../archive/static-uniqueness-plan.md)
- [../archive/awfy-c5-inplace-vector.md](../archive/awfy-c5-inplace-vector.md)
