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
ownership specialization (the two-variant `add_type` case).

## Trackable action order

### Phase 0 — Baseline and safety rails

- [ ] **Define correctness guard programs.** Add/collect negative aliasing cases
  for slice/concat sharing, record fields, variants, array literals, nested
  collections, closure capture, task/fiber capture, channel send, globals, and
  unknown calls. Details: [architecture.md](architecture.md).
- [ ] **Define inspection workflow.** Decide the initial `twk ir` debug surface
  for ownership facts and CFG view output. The exact flag names can change.
  Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Keep performance as an end-of-track gate.** During the refactor, use
  correctness and IR/codegen inspection rather than timing claims. Details:
  [architecture.md](architecture.md).
- [x] **Reconcile the COW census ceiling.** Done 2026-07-12: re-baselined
  `tests/cow_analysis.rs` 1696 → 2000 (boot source growth, not a regression).
  Surfaced that the total is non-deterministic (stage0 optimizer jitter ~1962–1970)
  and that it measures **stage0**, not the new boot analysis — so it's a reference
  distribution, not this project's regression gate. Details:
  [worked-examples.md](worked-examples.md).
- [ ] **Add a boot-side ownership census.** The new boot analysis needs its own
  deterministic in-place/COW count in the boot pipeline to serve as the actual
  Precondition baseline gate.

### Phase 1 — CFG ownership view, no codegen changes

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

### Phase 2 — Sound analysis facts, still no codegen changes

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
- [ ] **Handle record shell and field ownership.** Separate record-shell reuse
  from deep ownership of fields; support field projections and wrapper records.
  Details: [sound-analysis.md](sound-analysis.md).
- [ ] **Handle nested collection ownership conservatively.** Distinguish owned
  outer collections from unknown/shared inner collections. Details:
  [sound-analysis.md](sound-analysis.md).
- [ ] **Handle closure capture conservatively.** Treat escaping/unknown captures
  as publication; leave non-escaping recovery for later. Details:
  [closure-capture.md](closure-capture.md).
- [ ] **Handle concurrency publication.** Treat Task/fiber captures and
  `Channel<T>` sends as publication sinks; model cross-worker copy/share
  separately. Details: [concurrency-publication.md](concurrency-publication.md).

### Phase 3 — Shared optimizer facts and summaries

- [ ] **Move ownership-relevant pass queries to CFG facts.** Liveness, joins,
  back-edges, publication, and candidate verdicts should have one shared source
  of truth. Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Keep local peepholes where safe.** Dead-let/copy-prop/const-fold/branch
  simplification can remain ANF-local temporarily when they do not depend on
  ownership/control-flow facts. Details: [architecture.md](architecture.md).
- [ ] **Add function summaries.** Summarize parameter ownership requirements,
  consumed/borrowed/published params, return ownership, and call-site
  compatibility. Details: [architecture.md](architecture.md).
- [ ] **Make specialization deterministic and capped.** Account for type
  monomorph clones multiplied by ownership-shape variants. Details:
  [architecture.md](architecture.md).

### Phase 4 — Codegen-ready decisions, no backend proof

- [ ] **Define mutable intrinsic decision records.** For each accepted region,
  record operation family, begin/thaw, reads, writes, freeze/publish, fallback,
  and proof/debug id. Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Represent record/field codegen decisions.** Decide record shell reuse,
  field projection transfer/borrow, shared-field fallback, and `Set<K>` wrapper
  projection. Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [ ] **Keep codegen mechanical.** Backend code consumes ANF-keyed annotations or
  side tables; it must not re-prove uniqueness, field ownership, or escape.
  Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).

### Phase 5 — First mutable lowering wins

- [ ] **Lower owned vector `set_at`.** Target ordinary AWFY `sieve` and the vector
  update part of `bounce`; support interleaved non-escaping reads. Details:
  [architecture.md](architecture.md).
- [ ] **Lower vector append/build regions.** Unify hand-written accumulators and
  existing builder-like shapes behind internal mutable-region decisions. Details:
  [architecture.md](architecture.md).
- [ ] **Lower owned dict regions.** Target `Dict.set`/`Dict.remove` with old-value
  observability as the main blocker. Details: [architecture.md](architecture.md).
- [ ] **Lower record shell/field cases.** Reuse record shells and project owned
  fields only when the CFG facts explicitly permit it. Details:
  [architecture.md](architecture.md).

### Phase 6 — Interprocedural ownership specialization

- [ ] **Clone ownership-specialized function variants on demand.** Keep ordinary
  immutable variants for shared/unknown callers. Details: [architecture.md](architecture.md).
- [ ] **Select variants statically at call sites.** No runtime uniqueness tests or
  dynamic dispatch for proof. Details: [architecture.md](architecture.md).
- [ ] **Enforce variant caps and fallback.** Avoid combinatorial blowup across
  type monomorphization and ownership shapes. Details: [architecture.md](architecture.md).

### Phase 7 — Cleanup and end-of-track verification

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
| [worked-examples.md](worked-examples.md) | Real boot ANF dumps (Cases A/B/C/V/T), the op→ownership-event table, and the stage0 census baseline. The design anchor every rule is validated against. |
| [fact-lattice.md](fact-lattice.md) | Semantic core: the ownership lattice, per-`AnfOp` transfer function, the `AInit` move/alias hinge, and control-flow merges. |
| [summary-specialization.md](summary-specialization.md) | Interprocedural layer: function-summary schema, SCC-ordered computation, and field-path-granular call-site variant selection. |
| [cfg-ownership-ir.md](cfg-ownership-ir.md) | CFG ownership view over ANF with SSA-style block parameters for carried values; optimizer analysis consumes this shared control-flow view. |
| [sound-analysis.md](sound-analysis.md) | Required-coverage matrix (positive/negative patterns per vector/dict/record/nested/caller-shape). The algorithm lives in fact-lattice.md + summary-specialization.md. |
| [closure-capture.md](closure-capture.md) | Closure capture as a publication sink, plus future recoverable non-escaping/inlined/summarized cases. |
| [concurrency-publication.md](concurrency-publication.md) | Task/fiber capture, `Channel<T>` sends, and cross-worker copy-vs-share distinctions. |
| [buffer-cleanup.md](buffer-cleanup.md) | Follow-up policy for retiring Buffer workaround usage after ordinary immutable code reaches private mutable lowering. |

## Historical references

- [../archive/boot-uniqueness-deep-ownership.md](../archive/boot-uniqueness-deep-ownership.md)
- [../archive/static-uniqueness-plan.md](../archive/static-uniqueness-plan.md)
- [../archive/awfy-c5-inplace-vector.md](../archive/awfy-c5-inplace-vector.md)
