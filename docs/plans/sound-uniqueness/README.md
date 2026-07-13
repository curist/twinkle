# Sound Uniqueness and Mutable Lowering

**Status:** Draft plan area

This folder tracks the from-scratch boot-compiler work for sound uniqueness
analysis and compiler-private mutable lowering.

Start with [architecture.md](architecture.md) for the full design. This README is
the trackable action order; focused docs hold details for the larger risk areas.
[worked-examples.md](worked-examples.md) grounds the design in real boot-compiler
ANF shapes (the op→event mapping, annotated cases, and the census baseline).
[fact-lattice.md](fact-lattice.md) is the semantic core: the first
`Unique`/`Shared`/`Unknown` ownership domain, per-op transfer rules, and
control-flow merges — validated against those examples, with later precision
layers noted separately. [summary-specialization.md](summary-specialization.md) is
the interprocedural layer: minimal first summaries, then the later path-sensitive
summary/specialization target. The codegen half first consumes facts through
existing persistent/in-place/builder hooks; the later compiler-private
mutable-intrinsic contract and record/field lowering are sketched in
[mutable-intrinsics.md](mutable-intrinsics.md) and [records-fields.md](records-fields.md).

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

- [x] **Define correctness guard programs.** Done 2026-07-13: a behavioral
  negative-aliasing suite (`uniqueness_guard_suite.tw`): one test per required
  negative (slice/concat/view sharing, record fields, variants, dict values,
  nested collections, closure capture, task/channel publication, module-global
  publication, retaining callee, Cell, `try`), each asserting the observable
  persistent result and cross-referenced to its worked-example case + fact-lattice
  rule, plus the Case B/V positive anchors. Details:
  [phase0-baseline.md](phase0-baseline.md).
- [x] **Define inspection workflow.** Done 2026-07-13: the first debug surface is a
  `twk ir --census` flag (population table of candidate op-families + in-place
  counts, `--sites` for per-site detail); the ownership-facts / CFG view is Phase 1.
  Details:
  [phase0-baseline.md](phase0-baseline.md), [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [x] **Reconcile the COW census ceiling.** Done 2026-07-12: re-baselined
  `tests/cow_analysis.rs` 1696 → 2000 (boot source growth, not a regression).
  Surfaced that the total is non-deterministic (stage0 optimizer jitter ~1962–1970)
  and that it measures **stage0**, not the new boot analysis — so it's a reference
  distribution, not this project's regression gate. Details:
  [worked-examples.md](worked-examples.md).
- [x] **Stand up a boot-side ownership census harness.** Done 2026-07-13: a
  deterministic candidate-op/in-place counter over the boot pipeline's optimized
  ANF (`artifacts.opt`), shared as a reusable `census.tw` function (detection rides
  `OptimizerSemantics`) behind the `twk ir --census` flag and an asserted fixture
  gate (`uniqueness_census_suite.tw`), with `boot/main.tw` as a loose wide
  reference. On this branch it reads the current **all-COW floor** (the old passes
  were removed), which *is* the zero baseline. It becomes the discriminating
  regression gate as Phase 5 codegen starts converting sites. Distinct from
  `tests/cow_analysis.rs`, which measures stage0. Details:
  [phase0-baseline.md](phase0-baseline.md).

### Phase 1 — CFG ownership view, no codegen changes *(architecture: 1A)*

- [ ] **Build CFG ownership view over ANF.** ANF remains authoritative; the CFG is
  a derived analysis view with deterministic block ids, successors, and mappings
  back to ANF lets/ops. Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Add SSA-style block parameters for carried values.** Use block parameters
  for values crossing joins/back-edges; keep ownership facts as separate maps.
  Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Represent value-carrying breaks.** Treat `break value` as both a control
  edge and a possible publication/region-exit edge; explicit freeze handling is a
  later mutable-region concern. Details:
  [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Print the structural CFG.** `twk ir --cfg` shows blocks, carried block
  parameters, terminators (including value-carrying break edges), and per-block
  ANF mapping, with the entry/exit fact maps shown empty. Populated ownership
  facts, candidates, accepted/rejected reasons, and proof ids arrive with Phase 2.
  Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).

### Phase 2 — Minimal ownership facts, still no codegen changes *(architecture: 1A facts)*

- [x] **Design the sound ownership analysis direction.** The full design covers
  affine ownership, transport wrappers, path-sensitive summaries, and later
  specialization. The first implementation is intentionally smaller. Details:
  [fact-lattice.md](fact-lattice.md), [summary-specialization.md](summary-specialization.md),
  [sound-analysis.md](sound-analysis.md).
- [ ] **Implement the first ownership domain.** Track only `Unique`, `Shared`, and
  `Unknown` as ownership facts, populating the empty entry/exit fact maps the
  Phase 1 structural view reserved and surfacing them in the `twk ir` CFG output.
  Keep ownership facts separate from block parameters, keep binding validity,
  last-use, and liveness as separate CFG facts, and do not model `Moved` as an
  ownership lattice element.
  Details: [fact-lattice.md](fact-lattice.md), [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Populate per-predecessor join and back-edge fact transfers.** Interpret
  the Phase 1 structural join params and predecessor edges: for partial branch
  rebinds, distinguish arms that rebound a carried `LocalId` from arms that
  forward the incoming value; for loop back-edges, distinguish preserved carried
  locals from updated carried locals. Phase 1 edge args are only arity
  placeholders — Phase 2 is where forwarding/rebinding semantics become facts.
  Details: [fact-lattice.md](fact-lattice.md), [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Model conservative publication and aliasing.** Known aliases and
  publication sinks demote to `Shared`; missing proof stays `Unknown`. Keep
  record shapes, deep field ownership, and transport-wrapper paths out of the
  first executable domain. Details: [sound-analysis.md](sound-analysis.md).
- [ ] **Handle loop-carried ownership in the simple domain.** Prove `Unique` can
  cross back-edges only when every continuing path preserves the invariant and
  reads are non-escaping. Details: [sound-analysis.md](sound-analysis.md).
- [ ] **Catalog later precision needs without implementing them yet.** Record
  shell/field ownership, transport wrappers (`out.ctx`, `out.state`,
  `Ok[0].state`), nested collections, closure recovery, and concurrency sinks stay
  in the coverage docs until the core engine is stable. Details:
  [records-fields.md](records-fields.md), [summary-specialization.md](summary-specialization.md),
  [concurrency-publication.md](concurrency-publication.md).

### Phase 3 — Shared optimizer facts and minimal summaries *(architecture: 1B + first summaries)*

Analysis side only. This phase makes ownership a shared primitive that existing
optimizer decisions can consume; it does not introduce ownership-specialized
variants yet.

- [ ] **Move ownership-relevant pass queries to CFG facts.** Liveness, joins,
  back-edges, publication, and candidate verdicts should have one shared source
  of truth. Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Decide which local peepholes stay ANF-local.** Dead-let/copy-prop/
  const-fold/branch simplification may remain ANF-local while they do not depend
  on ownership/control-flow facts; the rest move onto CFG facts. Details:
  [architecture.md](architecture.md).
- [ ] **Compute minimal function summaries.** Start with only: consumes parameter,
  retains parameter, returns fresh value, returns alias. Use these to avoid
  treating every known helper as an unknown publication boundary. Delay
  access-path summaries, return-path transport wrappers, and ownership
  specialization. Details: [summary-specialization.md](summary-specialization.md).

### Phase 4 — Existing lowering decisions, no backend proof *(architecture: 2A prep)*

- [ ] **Feed existing runtime/codegen hooks.** Convert ownership facts into
  decisions among today's persistent operation, existing in-place helper, and
  existing builder lowering. Do not introduce `begin_mutable`/`freeze` or a new
  runtime representation in this phase. Details: [mutable-intrinsics.md](mutable-intrinsics.md).
- [ ] **Define decision records for current hooks.** For each accepted candidate,
  record operation family, source value, required `Unique` fact, last-use proof,
  fallback persistent operation, and proof/debug id. Details:
  [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Keep codegen mechanical.** Backend code consumes decisions; absence or
  stale/ambiguous decisions fall back to the persistent path. Details:
  [mutable-intrinsics.md](mutable-intrinsics.md).

### Phase 5 — First mutable lowering through existing hooks *(architecture: 2A + 2B + 2C)*

Emission consumes the CFG ownership facts and decision records from Phases 2–4.
The goal is correctness and inspectability first, using existing runtime
primitives.

- [ ] **Lower local owned vector `set_at` and dict updates where the simple domain
  proves `Unique`.** Support interleaved non-escaping reads only when the facts are
  explicit; otherwise fall back to persistent operations. Details:
  [mutable-intrinsics.md](mutable-intrinsics.md).
- [ ] **Lower existing vector builder regions from facts.** Reuse the current
  builder hooks; do not redesign builder/runtime representation yet. Details:
  [mutable-intrinsics.md](mutable-intrinsics.md).
- [ ] **Lower simple record shell updates from facts.** Reuse record shells only
  when the CFG facts explicitly permit it. Field-sensitive/deep record ownership
  remains a later precision step. Details: [records-fields.md](records-fields.md).

### Phase 6 — Add path precision and bounded specialization *(architecture: 2A scaling)*

Builds on the stable core engine. This is where the plan grows toward the full
record-threading and helper-transport story.

- [ ] **Add field-path and return-path summaries.** Support transported fields
  (`out.ctx`, `out.state`, `Ok[0].state`) and field-sensitive record ownership.
  Details: [summary-specialization.md](summary-specialization.md), [records-fields.md](records-fields.md).
- [ ] **Add bounded ownership specialization.** Multiple `(param, field-path)`
  owned reqs per callee (e.g. shell-only vs shell+`.types`), with deterministic
  caps and persistent fallback. Details: [summary-specialization.md](summary-specialization.md).
- [ ] **Enforce variant caps and fallback.** Bound variants per
  `(mono-instance, func)`; overflow falls back to the generic persistent variant.
  Details: [summary-specialization.md](summary-specialization.md).
- [ ] **Resolve the SCC-fixpoint / variant interaction.** Recursive callees reuse
  in-progress variants; a variant's re-analysis must not destabilize a summary a
  sibling variant depends on. Details: [summary-specialization.md](summary-specialization.md).

### Phase 7 — Mutable-intrinsic migration and hook cleanup *(architecture: 2D)*

This phase handles the cleanup explicitly deferred by the first implementation's
"reuse existing hooks" rule. It should happen only after Phases 4–6 have proven
that ownership facts can drive today's persistent/in-place/builder choices
correctly.

- [ ] **Define the compiler-private intrinsic family.** Finalize the internal
  operations (`begin`/`read`/`write`/`append`/`remove`/`freeze`), operand encoding,
  ANF annotation vs side-table representation, and proof/debug ids. Details:
  [mutable-intrinsics.md](mutable-intrinsics.md), [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Migrate existing hooks behind the intrinsic layer.** Route
  `vector$builder_*`, vector set helpers, dict in-place helpers, and
  `ARecordUpdate.in_place` through the shared decision/intrinsic interface while
  preserving their current runtime implementations where possible. Details:
  [mutable-intrinsics.md](mutable-intrinsics.md).
- [ ] **Remove split-brain mutability decisions.** Delete or disable any ad hoc
  recognizer/legality path that can independently decide mutation. After this
  phase, ownership facts are the only legality source; hooks are implementation
  targets only. Details: [architecture.md](architecture.md).
- [ ] **Preserve non-optimizer builder uses.** `collect` and any semantic builder
  lowering that is required independent of optimization must keep working; the
  cleanup targets builder use as an optimizer rewrite target, not the runtime
  mechanism itself. Details: [mutable-intrinsics.md](mutable-intrinsics.md).
- [ ] **Update inspection output.** `twk ir` / census output should show both the
  ownership proof and the final intrinsic-or-hook lowering chosen, so migrations
  remain auditable. Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).

### Phase 8 — Optional precision recovery and end-of-track verification *(architecture: Follow-up)*

This phase handles anticipated work that is intentionally not on the critical path
for the first correct ownership engine.

- [ ] **Evaluate non-escaping closure recovery.** Keep escaping/unknown captures as
  publication sinks by default; add non-escaping/inlined closure recovery only if
  real workloads justify it. Details: [closure-capture.md](closure-capture.md).
- [ ] **Evaluate advanced concurrency distinctions.** Keep Task/fiber capture and
  Channel sends conservative; refine serialized-copy vs shared-transfer cases only
  when the runtime contract is explicit. Details:
  [concurrency-publication.md](concurrency-publication.md).
- [ ] **Run end-of-track performance gates.** Compare ordinary AWFY variants to
  current `*_mut` workaround ceilings from the same machine/session. Details:
  [architecture.md](architecture.md).
- [ ] **Retire Buffer workaround usage when justified.** Only after ordinary code
  reaches the target class and `Vector<Byte>` covers crypto needs. Details:
  [buffer-cleanup.md](buffer-cleanup.md).

## Future-work ledger

Any design text that says "later", "future", or "cleanup" should map to one of
these phases:

| Deferred work | Phase |
|---|---|
| Record/field ownership, return-path transport wrappers, and locally handled Result payload paths | Phase 6 |
| Bounded ownership-specialized variants and SCC/variant interaction | Phase 6 |
| Compiler-private mutable intrinsic family | Phase 7 |
| Migration of existing builder/in-place hooks behind shared internals | Phase 7 |
| Removal of ad hoc mutability legality paths | Phase 7 |
| Non-escaping closure recovery | Phase 8, optional based on workload evidence |
| Advanced concurrency copy/share refinement | Phase 8, optional based on runtime contract |
| Buffer retirement | Phase 8, after performance parity is demonstrated |

## Focused docs

| Doc | Purpose |
|---|---|
| [architecture.md](architecture.md) | Umbrella architecture, phases, mutable intrinsics, specialization, testing policy. |
| [phase0-baseline.md](phase0-baseline.md) | Phase 0 safety rails: the negative-aliasing guard suite, the reusable census + `twk ir --census` flag, and the fixture gate vs wide reference. Both rails are latent now and gain signal at Phase 5. |
| [design-rationale.md](design-rationale.md) | Why static + annotation-free + no-runtime-RC: the Wasm-GC-vs-refcount reason we can't copy Koka/Roc/Lean, the annotation-free/zero-overhead/coverage tradeoff triangle, and what immutable value semantics buys (may-alias-and-write → ownership+liveness). |
| [worked-examples.md](worked-examples.md) | Real boot ANF dumps (Cases A/B/C/V/T), the op→ownership-event table, and the stage0 census baseline. The design anchor every rule is validated against. |
| [fact-lattice.md](fact-lattice.md) | Semantic core: the first `Unique`/`Shared`/`Unknown` domain, per-`AnfOp` transfer function, `AInit` move/alias hinge, binding-validity split, and later precision layers. |
| [summary-specialization.md](summary-specialization.md) | Interprocedural layer: minimal first summaries, then SCC-ordered path-sensitive summaries and field-path-granular call-site variant selection. |
| [cfg-ownership-ir.md](cfg-ownership-ir.md) | CFG ownership view over ANF with SSA-style block parameters for carried values; optimizer analysis consumes this shared control-flow view. |
| [sound-analysis.md](sound-analysis.md) | Required-coverage matrix (positive/negative patterns per vector/dict/record/nested/caller-shape). The algorithm lives in fact-lattice.md + summary-specialization.md. |
| [closure-capture.md](closure-capture.md) | Closure capture as a publication sink, plus future recoverable non-escaping/inlined/summarized cases. |
| [concurrency-publication.md](concurrency-publication.md) | Task/fiber capture, `Channel<T>` sends, synchronous extern/FFI (copying boundary), and cross-worker copy-vs-share distinctions. |
| [records-fields.md](records-fields.md) | Skeleton: record shell-vs-deep field ownership, the two-decisions-per-quartet split, `Set<K>` wrapper projection, and nested collection ownership. |
| [mutable-intrinsics.md](mutable-intrinsics.md) | Codegen handoff: first select existing persistent/in-place/builder hooks from ownership facts; later migrate toward compiler-private mutable-collection intrinsics. |
| [buffer-cleanup.md](buffer-cleanup.md) | Follow-up policy for retiring Buffer workaround usage after ordinary immutable code reaches private mutable lowering. |

## Historical references

- [../archive/boot-uniqueness-deep-ownership.md](../archive/boot-uniqueness-deep-ownership.md)
- [../archive/static-uniqueness-plan.md](../archive/static-uniqueness-plan.md)
- [../archive/awfy-c5-inplace-vector.md](../archive/awfy-c5-inplace-vector.md)
