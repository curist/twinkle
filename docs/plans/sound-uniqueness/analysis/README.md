# Analysis Track

**Status:** Phase 0-2 done; Phase 3 next/current.

This track owns the proof-producing half of sound uniqueness: CFG structure,
ownership facts, liveness/last-use, summaries, candidate-classification inputs,
and debug output. It does **not** change emitted code. The codegen track turns these
facts into ANF-keyed lowering decisions.

[../architecture.md](../architecture.md) is the canonical scope/design source.
This README is the analysis worklist derived from it.

## Track invariants

- Generated code stays unchanged throughout this track.
- ANF remains authoritative; CFG is a derived analysis/debug view over optimized
  ANF.
- Facts are the only source of mutability legality. Later codegen consumes them
  mechanically.
- Missing proof means `Unknown` or `Shared`, never speculative mutation.

## Phase 0 — Baseline and safety rails *(architecture: Precondition)*

- [x] **Define correctness guard programs.** Behavioral negative-aliasing suite
  `uniqueness_guard_suite.tw` covers slice/concat/view sharing, record fields,
  variants, dict values, nested collections, closure capture, task/channel
  publication, module-global publication, retaining callees, `Cell`, and `try`,
  plus positive anchors. Details: [phase0-baseline.md](phase0-baseline.md).
- [x] **Define inspection workflow.** First debug surface is `twk ir --census`,
  with `--sites` for per-site detail. Ownership facts and CFG output arrive in
  later phases. Details: [phase0-baseline.md](phase0-baseline.md),
  [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [x] **Reconcile the COW census ceiling.** The Rust-side census remains a
  stage0 reference distribution, not this project's regression gate. Details:
  [worked-examples.md](worked-examples.md).
- [x] **Stand up a boot-side ownership census harness.** Deterministic
  candidate-op/in-place counter over optimized ANF, exposed through
  `twk ir --census`; current all-COW floor is the zero baseline. Details:
  [phase0-baseline.md](phase0-baseline.md).

## Phase 1 — CFG ownership view, no codegen changes *(architecture: 1A-view)*

- [x] **Build CFG ownership view over ANF.** `compiler/cfg.tw` derives a
  deterministic view from `artifacts.opt`, with mappings back to optimized ANF
  let-result locals. Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [x] **Add SSA-style block parameters for carried values.** Branch/match/loop
  joins carry sorted params; ownership facts stay in separate maps.
- [x] **Represent breaks and loop exits.** `break`/`continue` are terminators
  wired by a post-pass; value break remains defensive scaffolding.
- [x] **Print the structural CFG.** `twk ir --cfg` shows blocks, carried params,
  terminators, predecessor/successor edges, and per-block ANF mapping.

## Phase 2 — Minimal ownership facts, still no codegen changes *(architecture: 1A-facts)*

- [x] **Design the sound ownership analysis direction.** Full design covers
  affine ownership, transport wrappers, path-sensitive summaries, and later
  specialization; the first executable implementation is smaller. Details:
  [fact-lattice.md](fact-lattice.md),
  [summary-specialization.md](summary-specialization.md),
  [sound-analysis.md](sound-analysis.md).
- [x] **Implement the first ownership domain.** Track `Unique`, `Shared`, and
  `Unknown`, with binding validity, liveness, and last-use as separate facts.
  Details: [phase2-design.md](phase2-design.md).
- [x] **Populate per-predecessor join and back-edge fact transfers.** Real
  per-edge phi atoms and live-in positional joins distinguish forwarding from
  rebinding.
- [x] **Model conservative publication and aliasing.** Known aliases and
  publication sinks demote to `Shared`; missing proof stays `Unknown`.
- [x] **Handle loop-carried ownership in the simple domain.** Unique values can
  cross back-edges only when continuing paths preserve the invariant.
- [x] **Catalog later precision needs without implementing them yet.** Shell/field
  ownership, transport wrappers, nested collections, closure recovery, and
  concurrency precision remain in focused docs.

## Phase 3 — Shared optimizer facts and minimal summaries *(architecture: 1B + first summaries)*

Analysis side only. This phase makes ownership a shared primitive that existing
optimizer decisions can later consume; it does not introduce ownership-specialized
variants, decision records, or codegen changes.

- [ ] **Move ownership-relevant pass queries to CFG facts.** Liveness, joins,
  back-edges, and publication should have one shared source of truth. Phase 3's
  design notes that the old ownership-consuming passes were removed, so this is
  primarily an audit and documentation step; candidate verdicts and decision
  records remain in the codegen track. Details: [phase3-design.md](phase3-design.md).
- [ ] **Dead-merge block-param pruning using Phase 2 liveness facts.** Drop
  join/loop carried params that are dead across the boundary.
- [ ] **Match-arm pattern-binding precision.** Carry pattern-bound locals onto arm
  blocks so they are killed at block entry.
- [ ] **Decide which local peepholes stay ANF-local.** Dead-let/copy-prop/
  const-fold/branch simplification may remain ANF-local while they do not depend
  on ownership/control-flow facts.
- [ ] **Compute minimal function summaries.** Start with consumes parameter,
  retains parameter, returns fresh value, and returns alias; use these to avoid
  treating every known helper as an unknown publication boundary. Details:
  [summary-specialization.md](summary-specialization.md),
  [phase3-design.md](phase3-design.md).

## Analysis deferrals

| Deferred work | Home |
|---|---|
| Candidate verdicts and ANF-keyed codegen decisions | [../codegen/README.md](../codegen/README.md) |
| Existing-hook codegen lowering | [../codegen/README.md](../codegen/README.md) |
| Record/field ownership, return-path transport wrappers, and locally handled Result payload paths | Later analysis precision; see [records-fields.md](records-fields.md) and [summary-specialization.md](summary-specialization.md) |
| Bounded ownership-specialized variants and SCC/variant interaction | Later analysis precision; see [summary-specialization.md](summary-specialization.md) |
| Compiler-private mutable intrinsic family | [../migration/README.md](../migration/README.md) |
| Extern copying-borrow precision | Follow-up; see [concurrency-publication.md](concurrency-publication.md) |
| Non-escaping closure recovery | Follow-up; see [closure-capture.md](closure-capture.md) |
| Advanced concurrency copy/share refinement | Follow-up; see [concurrency-publication.md](concurrency-publication.md) |
