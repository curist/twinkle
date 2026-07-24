# Sound Uniqueness and Mutable Lowering

**Status:** Draft plan area

This folder tracks the from-scratch boot-compiler work for sound uniqueness
analysis and compiler-private mutable lowering.

The plan is now split into four owned tracks so the top-level entry point stays
small:

| Track | Owns | Start here |
|---|---|---|
| Analysis | CFG ownership view, ownership facts, liveness/last-use, summaries, candidate-classification inputs, and proof/debug output. Generated code stays unchanged. | [analysis/README.md](analysis/README.md) |
| Codegen | Mapping proven decisions to today's persistent/in-place/builder hooks, including operation catalogs, decision lookup/fallback plumbing, and one lowering family at a time. | [codegen/README.md](codegen/README.md) |
| Storage Representation | Mandatory performance substrate after the existing-hook proof: private mutation-enabled collection storage, scoped mutable regions, owned-specialized mutable ABI, typed/dense vector targets, and mutable/transient dict storage needed to reach Buffer-class performance. | [storage/README.md](storage/README.md) |
| Migration | Later cleanup after codegen and storage-representation work: compiler-private mutable intrinsics, hook consolidation, split-brain cleanup, and Buffer retirement policy. | [migration/README.md](migration/README.md) |

[architecture.md](architecture.md) remains the umbrella design and canonical scope
source. On any divergence, update `architecture.md` first, then re-derive the
track README(s). The focused track docs own the detailed checklists.

## Current focus

**The analysis track is complete through Phase 6.** Phases 0-6 produce auditable CFG
ownership facts, liveness, summaries, record shell/field and nested-collection
ownership, transport-wrapper / `Result`-payload return-path summaries, owned-entry
recovery, call-site variant selection, per-call-site owned-decision verdicts, and
recursive SCC variant-qualified diagnostic bodies in `twk ir --cfg` — all without
changing generated code. Field-granular codegen seeding has no summary observable and
remains codegen-owned; variant *generation* and in-place emission also remain codegen
work.

The governing rule is **all analysis precision lands before any codegen.** That gate
is satisfied, so the active work is the **codegen track**. Codegen Phases 7A-7D are
done: the surviving mutable hooks are inventoried, the persistent→mutable operation
catalog is verified, and the ANF-keyed decision-table seam is threaded through
backend preparation and emission with centralized persistent fallback. The first
Phase 7E dry-run slice is also done: `twk ir --census --sites` renders update-site
persistent→mutable targets, ownership verdicts, and `would_use` state for
vector/dict/record candidates while emitted code remains persistent.

**Current implementation focus: Codegen Phase 8C first slice is complete (builder
regions — a distinct region-shaped lowering); next are the 8C follow-ups (non-empty
seeds / typed routing / conditional folds, Plans 3–5), then records (8F), function
variants (8G), and record-backed field collections (8H).** Phase 8C Plans 1–2 have
landed: string and vector empty-seed accumulator loops (`acc = ""` / `acc = []`) now
lower end-to-end to builder regions (`builder_from`/`builder_new` → `builder_extend`/
`builder_push` → `builder_freeze`) via an ANF-to-ANF rewrite run at the top of
`link_program` (producing ANF′), driven by `BuilderRegionDecision` records from a
producer with the FU-1 fold-result deadness gate, non-overlap resolution, and
re-folded-accumulator (FU-2) surfacing; `--census --sites` shows a `consumed` column
reflecting actual rewrite application. Vector regions stay boxed (typed routing is
Plan 4). Self-host fixed point holds. The Plan 2 execution plan is archived under
`docs/plans/archive/`. Phases 8A, 8B, 8D, and 8E are complete:
the emitted-code slices select the existing in-place helper for proven owned
collection updates — 8A for straight-line local `Vector` indexed updates, 8B for
loop-carried accumulators in single and nested loops, 8D for owned `Dict.set`
(`dict$set_in_place`), and 8E for owned `Dict.remove` (`dict$remove_in_place`) —
while absent, stale, ambiguous, unsupported, or aliased decisions keep the ordinary
persistent path. All three call-swap families now emit in-place for owned sites. The decision
producer is now family-neutral (`produce_update_call_decisions`, collecting all
supported call-swap families), and `enabled_emit_policy` is the cumulative build
policy. Loop-carried decisions render a `phase8b-loop:<func>:carry L…:site L…:depth N`
proof id. Emittable *vector* sites are the index-assignment sugar form (`xs[i] = v`,
an inline `vector$set_unsafe` caller candidate); explicit `.set_at(...)` calls are
prelude calls and not caller-side candidates — whereas `Dict.set`/`Dict.remove` are
builtins that do surface at the caller. The 8A revisit of the deferred 7E
consumed-vs-ignored decision rendering is complete via the post-prepare `mutable
decisions` audit table; variant-routing dry-runs remain attached to 8G's
ownership-specialized clone routing. The completed 8B/8D/8E execution plans are
archived under `docs/plans/archive/`.

Important scope boundary: the existing-hook slices are the integration proof, not
the whole performance/migration deliverable. The project is not complete until
mutable lowering can keep proven-owned collections in private mutation-enabled
storage across the useful chain, staying low until the latest required publication
boundary before materializing persistent `Vector`/`Dict` values. Typed/unboxed
vectors, dense byte/int regions where appropriate, owned-specialized mutable ABI, and
true mutable/transient dict storage all belong to the storage-representation track,
which happens before migration cleanup and gates retiring `Buffer` as the ordinary
local-update workaround.

## Standing invariants

- **Performance is an end-of-track gate.** During the refactor, judge progress by
  correctness and IR/codegen inspection, not timing claims; run full perf
  comparisons only when the end-to-end path is in place.
- **Codegen stays mechanical.** Backend code consumes ANF-keyed decisions/side
  tables; it never re-proves uniqueness, field ownership, or escape.
- **Persistent fallback is the safety default.** Missing, stale, ambiguous, or
  unsupported mutable decisions emit the ordinary immutable path.
- **Storage representation is mandatory for completion.** Existing hooks may prove
  the seam, but Buffer-class performance requires private mutable storage that can
  preserve typed/unboxed representation and avoid repeated persistent
  materialization inside owned chains.

## Track map

### 1. Analysis track

Framework overview: [analysis/README.md](analysis/README.md); detailed phase ledger: [analysis/phases-0-6-history.md](analysis/phases-0-6-history.md)

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
input facts are now trustworthy. It is intentionally split more finely than a single
codegen milestone: operation catalogs and backend handoff/dry-run decisions (Phase 7)
are in place, and the emitted slices for local (Phase 8A) and loop-carried, single
and nested (Phase 8B) vector indexed updates, plus owned `Dict.set` (Phase 8D) and
`Dict.remove` (Phase 8E), are complete. Current work moves to builder regions
(Phase 8C); later Phase 8 slices broaden to records (8F), ownership-specialized
function variants (8G), and record-backed field collections (8H).

### 3. Storage representation track

Detailed checklist: [storage/README.md](storage/README.md)

This track starts after the existing-hook codegen path proves that sound decisions
can select private mutation safely. It makes storage optimization explicit and
mandatory: scoped mutable regions first, private `MutVec`/`MutDict` or equivalent
storage targets, owned-specialized mutable ABI across helper chains, typed/dense
vector storage where needed, and true mutable/transient HAMT work for dicts. It
runs before migration cleanup, because Buffer retirement depends on storage
performance rather than hook consolidation alone.

### 4. Migration track

Detailed checklist: [migration/README.md](migration/README.md)

This track is not the first codegen or storage implementation. It consolidates
successful existing-hook and private mutable-storage lowering behind compiler-
private intrinsics, removes any remaining ad hoc legality paths, and eventually
evaluates Buffer cleanup.

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
| [storage/README.md](storage/README.md) | Mandatory storage-representation track for private mutation-enabled collection storage, scoped mutable regions, owned-specialized mutable ABI, typed/dense vectors, mutable/transient dict storage, and Buffer-class performance gates. |
| [migration/mutable-intrinsics.md](migration/mutable-intrinsics.md) | Later compiler-private mutable intrinsic family and hook cleanup target. |
| [migration/buffer-cleanup.md](migration/buffer-cleanup.md) | Follow-up policy for retiring Buffer workaround usage. |

## Historical references

- [../archive/boot-uniqueness-deep-ownership.md](../archive/boot-uniqueness-deep-ownership.md)
- [../archive/static-uniqueness-plan.md](../archive/static-uniqueness-plan.md)
- [../archive/awfy-c5-inplace-vector.md](../archive/awfy-c5-inplace-vector.md)
