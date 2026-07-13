# Sound Uniqueness and Mutable Lowering

**Status:** Draft plan area

This folder tracks the from-scratch boot-compiler work for sound uniqueness
analysis and compiler-private mutable lowering.

Start with [architecture.md](architecture.md) for the full design. This README is
the trackable action order; focused docs hold details for the larger risk areas.
[worked-examples.md](worked-examples.md) grounds the design in real boot-compiler
ANF shapes (the op→event mapping, annotated cases, and the census baseline).
[fact-lattice.md](fact-lattice.md) is the semantic core: the ownership lattice,
per-op transfer rules, and control-flow merges — validated against those examples.
[summary-specialization.md](summary-specialization.md) is the interprocedural
layer: the function-summary schema, SCC-ordered computation, and call-site
ownership specialization (the two-variant `add_type` case). The codegen half —
the compiler-private mutable-intrinsic contract and record/field lowering — is
sketched in [mutable-intrinsics.md](mutable-intrinsics.md) and
[records-fields.md](records-fields.md) (skeletons, filled in before lowering
begins).

## Trackable action order

These phases are a **finer pipeline-layer cut** (view → facts → shared facts →
decisions → emit → scale → cleanup) of the milestone plan in
[architecture.md](architecture.md), which cuts by codegen milestone
(Precondition / 1A / 1B / 2A–2D / Follow-up). They do not map 1:1; each header
notes the architecture milestone(s) it corresponds to so the two can be
cross-read.

**[architecture.md](architecture.md) is the canonical scope/design source; this
list is the derived tracking checklist.** On any divergence architecture wins, and
a re-scope should change architecture.md first, then re-derive the affected phases
here — so the two orderings are kept from drifting apart by hand.

**Standing invariants** (hold across every phase — not one-time tasks):

- **Performance is an end-of-track gate.** During the refactor, judge progress by
  correctness and IR/codegen inspection, not timing claims; run full perf
  comparisons only when the end-to-end path is in place.
- **Codegen stays mechanical.** Backend code consumes ANF-keyed decisions/side
  tables; it never re-proves uniqueness, field ownership, or escape.

### Phase 0 — Baseline and safety rails *(architecture: Precondition)*

- [ ] **Define correctness guard programs.** A behavioral negative-aliasing suite
  (`uniqueness_guard_suite.tw`): one test per required negative (slice/concat/view
  sharing, record fields, variants, nested collections, closure capture,
  task/channel publication, globals, unknown calls, Cell, `try`), each asserting the
  observable persistent result and cross-referenced to its worked-example case +
  fact-lattice rule, plus the Case B/V positive anchors. Details:
  [phase0-baseline.md](phase0-baseline.md).
- [ ] **Define inspection workflow.** The first debug surface is a `twk ir
  --census` flag (population table of candidate op-families + in-place counts,
  `--sites` for per-site detail); the ownership-facts / CFG view is Phase 1. Details:
  [phase0-baseline.md](phase0-baseline.md), [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [x] **Reconcile the COW census ceiling.** Done 2026-07-12: re-baselined
  `tests/cow_analysis.rs` 1696 → 2000 (boot source growth, not a regression).
  Surfaced that the total is non-deterministic (stage0 optimizer jitter ~1962–1970)
  and that it measures **stage0**, not the new boot analysis — so it's a reference
  distribution, not this project's regression gate. Details:
  [worked-examples.md](worked-examples.md).
- [ ] **Stand up a boot-side ownership census harness.** A deterministic
  in-place/COW counter over the boot pipeline's optimized ANF (`artifacts.opt`),
  shared as a reusable `census.tw` function behind the `twk ir --census` flag and
  an asserted fixture gate (`uniqueness_census_suite.tw`), with `boot/main.tw` as a
  loose wide reference. Buildable now: on this branch it reads the current
  **all-COW floor** (the old passes were removed), which *is* the zero baseline. It
  becomes the discriminating regression gate as Phase 5 codegen starts converting
  sites. Distinct from `tests/cow_analysis.rs`, which measures stage0. Details:
  [phase0-baseline.md](phase0-baseline.md).

### Phase 1 — CFG ownership view, no codegen changes *(architecture: 1A)*

- [ ] **Build CFG ownership view over ANF.** ANF remains authoritative; the CFG is
  a derived analysis view with deterministic block ids, successors, and mappings
  back to ANF lets/ops. Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Add SSA-style block parameters for carried values.** Use block parameters
  for values crossing joins/back-edges; keep ownership facts as separate maps.
  Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Represent value-carrying breaks.** Treat `break value` as both a control
  edge and a possible publication/freeze/region-exit edge. Details:
  [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Print CFG and ownership facts.** `twk ir` should show blocks, carried
  values, entry/exit facts, candidates, accepted/rejected reasons, and proof ids.
  Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).

### Phase 2 — Sound analysis facts, still no codegen changes *(architecture: 1A facts)*

- [x] **Design the sound ownership analysis.** Affine lattice + per-`AnfOp`
  transfer function + SCC-ordered summaries. Design in
  [fact-lattice.md](fact-lattice.md) and
  [summary-specialization.md](summary-specialization.md); required coverage in
  [sound-analysis.md](sound-analysis.md).
- [ ] **Implement local ownership/borrow/publication facts.** Track owned,
  mutable-region, borrowed, published, unknown, and persistent states. Details:
  [sound-analysis.md](sound-analysis.md).
- [ ] **Handle loop-carried ownership.** Prove owned handles can cross back-edges
  only when every path preserves the invariant and reads are non-escaping borrows.
  Details: [sound-analysis.md](sound-analysis.md).
- [ ] **Handle record shell, field ownership, and transport wrappers.** Separate
  record-shell reuse from deep ownership of fields; support field projections,
  `.{ ..., ctx/state/env }` return-path ownership, locally handled Result payload
  paths, and wrapper records. Details: [records-fields.md](records-fields.md),
  [sound-analysis.md](sound-analysis.md).
- [ ] **Handle nested collection ownership conservatively.** Distinguish owned
  outer collections from unknown/shared inner collections. Details:
  [records-fields.md](records-fields.md), [sound-analysis.md](sound-analysis.md).
- [ ] **Handle closure capture conservatively.** Treat escaping/unknown captures
  as publication; leave non-escaping recovery for later. Details:
  [closure-capture.md](closure-capture.md).
- [ ] **Handle concurrency publication.** Treat Task/fiber captures and
  `Channel<T>` sends as publication sinks; model cross-worker copy/share
  separately. Details: [concurrency-publication.md](concurrency-publication.md).

### Phase 3 — Shared optimizer facts and summaries *(architecture: 1B + 2A summary analysis)*

Analysis side only — this phase *computes* summaries and the specialization
policy; variant emission and call-site selection are Phase 5/6.

- [ ] **Move ownership-relevant pass queries to CFG facts.** Liveness, joins,
  back-edges, publication, and candidate verdicts should have one shared source
  of truth. Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Decide which local peepholes stay ANF-local.** Dead-let/copy-prop/
  const-fold/branch simplification may remain ANF-local while they do not depend
  on ownership/control-flow facts; the rest move onto CFG facts. Details:
  [architecture.md](architecture.md).
- [ ] **Compute function summaries.** Summarize parameter ownership requirements,
  consumed/borrowed/published params, return-path ownership (including `out.ctx`,
  `out.state`, and `Ok[0].state` transport wrappers), and call-site
  compatibility, SCC-ordered. Details: [summary-specialization.md](summary-specialization.md).
- [ ] **Define the specialization key and cap policy.** Deterministic,
  order-independent `(param, field-path)` keys and per-`(mono-instance, func)`
  variant caps, accounting for type-monomorph clones × ownership-shape variants.
  This fixes the *policy*; emission is Phase 5/6. Details:
  [summary-specialization.md](summary-specialization.md).

### Phase 4 — Codegen-ready decisions, no backend proof *(architecture: 2A prep)*

- [ ] **Define the mutable-collection intrinsic family.** The emitted codegen
  contract: begin/thaw, read, write, append/extend, remove, freeze/publish —
  one shape shared by vector, dict, and record-shell lowering; not a prelude/
  `@std` API. Details: [mutable-intrinsics.md](mutable-intrinsics.md).
- [ ] **Define mutable intrinsic decision records.** For each accepted region,
  record operation family, begin/thaw, reads, writes, freeze/publish, fallback,
  and proof/debug id. Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Represent record/field codegen decisions.** Decide record shell reuse,
  field projection transfer/borrow, transport-wrapper `.ctx`/`.state` and
  `Ok[0].state` moves, shared-field fallback, and `Set<K>` wrapper projection.
  Details: [records-fields.md](records-fields.md), [cfg-ownership-ir.md](cfg-ownership-ir.md).

### Phase 5 — First mutable lowering wins *(architecture: 2A + 2B + 2C)*

The first wins are **interprocedural by construction**, not intraprocedural:
sieve updates through the `set_at` wrapper and `build_env` through `add_type`
(worked-examples Cases A/B). So a *minimal* two-variant call-site specialization
is on this phase's critical path — Phase 6 only *scales* it. Emission consumes the
summaries computed in Phase 3.

- [ ] **Emit minimal owned/generic call-site specialization.** From Phase 3
  summaries, materialize two variants of a consuming callee (owned-specialized +
  persistent) and select statically at the call site — enough for thin wrappers
  (`set_at`) and single-field records (`add_type`). No runtime uniqueness test.
  Details: [summary-specialization.md](summary-specialization.md).
- [ ] **Lower owned vector `set_at`.** Target ordinary AWFY `sieve` and the vector
  update part of `bounce`; support interleaved non-escaping reads. Details:
  [mutable-intrinsics.md](mutable-intrinsics.md).
- [ ] **Lower vector append/build regions.** Unify hand-written accumulators and
  existing builder-like shapes behind internal mutable-region decisions. Details:
  [mutable-intrinsics.md](mutable-intrinsics.md).
- [ ] **Lower owned dict regions.** Target `Dict.set`/`Dict.remove` with old-value
  observability as the main blocker. Details: [mutable-intrinsics.md](mutable-intrinsics.md).
- [ ] **Lower record shell/field cases.** Reuse record shells and project owned
  fields only when the CFG facts explicitly permit it. Details:
  [records-fields.md](records-fields.md), [mutable-intrinsics.md](mutable-intrinsics.md).

### Phase 6 — Scale and cap interprocedural specialization *(architecture: 2A scaling)*

Builds on Phase 5's minimal owned/generic split; here it grows to multi-variant,
field-path granularity, and stays bounded.

- [ ] **Extend to field-path-granular variants.** Multiple `(param, field-path)`
  owned reqs per callee (e.g. shell-only vs shell+`.types`), beyond the single
  owned/generic split. Details: [summary-specialization.md](summary-specialization.md).
- [ ] **Enforce variant caps and fallback.** Bound variants per
  `(mono-instance, func)`; overflow falls back to the generic persistent variant.
  Details: [summary-specialization.md](summary-specialization.md).
- [ ] **Resolve the SCC-fixpoint / variant interaction.** Recursive callees reuse
  in-progress variants; a variant's re-analysis must not destabilize a summary a
  sibling variant depends on. Details: [summary-specialization.md](summary-specialization.md).

### Phase 7 — Cleanup and end-of-track verification *(architecture: 2D + Follow-up)*

- [ ] **Migrate ad hoc transient hooks behind shared internals.** Route existing
  builder/in-place helpers through the common mutable-region model where useful.
  Details: [architecture.md](architecture.md).
- [ ] **Run end-of-track performance gates.** Compare ordinary AWFY variants to
  current `*_mut` workaround ceilings from the same machine/session. Details:
  [architecture.md](architecture.md).
- [ ] **Retire Buffer workaround usage when justified.** Only after ordinary code
  reaches the target class and `Vector<Byte>` covers crypto needs. Details:
  [buffer-cleanup.md](buffer-cleanup.md).

## Focused docs

| Doc | Purpose |
|---|---|
| [architecture.md](architecture.md) | Umbrella architecture, phases, mutable intrinsics, specialization, testing policy. |
| [phase0-baseline.md](phase0-baseline.md) | Phase 0 safety rails: the negative-aliasing guard suite, the reusable census + `twk ir --census` flag, and the fixture gate vs wide reference. Both rails are latent now and gain signal at Phase 5. |
| [design-rationale.md](design-rationale.md) | Why static + annotation-free + no-runtime-RC: the Wasm-GC-vs-refcount reason we can't copy Koka/Roc/Lean, the annotation-free/zero-overhead/coverage tradeoff triangle, and what immutable value semantics buys (may-alias-and-write → ownership+liveness). |
| [worked-examples.md](worked-examples.md) | Real boot ANF dumps (Cases A/B/C/V/T), the op→ownership-event table, and the stage0 census baseline. The design anchor every rule is validated against. |
| [fact-lattice.md](fact-lattice.md) | Semantic core: the ownership lattice, per-`AnfOp` transfer function, the `AInit` move/alias hinge, and control-flow merges. |
| [summary-specialization.md](summary-specialization.md) | Interprocedural layer: function-summary schema, SCC-ordered computation, and field-path-granular call-site variant selection. |
| [cfg-ownership-ir.md](cfg-ownership-ir.md) | CFG ownership view over ANF with SSA-style block parameters for carried values; optimizer analysis consumes this shared control-flow view. |
| [sound-analysis.md](sound-analysis.md) | Required-coverage matrix (positive/negative patterns per vector/dict/record/nested/caller-shape). The algorithm lives in fact-lattice.md + summary-specialization.md. |
| [closure-capture.md](closure-capture.md) | Closure capture as a publication sink, plus future recoverable non-escaping/inlined/summarized cases. |
| [concurrency-publication.md](concurrency-publication.md) | Task/fiber capture, `Channel<T>` sends, synchronous extern/FFI (copying boundary), and cross-worker copy-vs-share distinctions. |
| [records-fields.md](records-fields.md) | Skeleton: record shell-vs-deep field ownership, the two-decisions-per-quartet split, `Set<K>` wrapper projection, and nested collection ownership. |
| [mutable-intrinsics.md](mutable-intrinsics.md) | Skeleton: the compiler-private mutable-collection intrinsic family (begin/read/write/append/remove/freeze) and vector/dict/freeze lowering — the codegen contract. |
| [buffer-cleanup.md](buffer-cleanup.md) | Follow-up policy for retiring Buffer workaround usage after ordinary immutable code reaches private mutable lowering. |

## Historical references

- [../archive/boot-uniqueness-deep-ownership.md](../archive/boot-uniqueness-deep-ownership.md)
- [../archive/static-uniqueness-plan.md](../archive/static-uniqueness-plan.md)
- [../archive/awfy-c5-inplace-vector.md](../archive/awfy-c5-inplace-vector.md)
