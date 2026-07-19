# Analysis Track

**Status:** Phases 0-6 done (2026-07-19). Phase 6 closed the Phase 5
param-threaded deferral and now prints the per-call-site owned specialization
decision in `twk ir --cfg`; generated code remains unchanged and the census stays
0 in-place. Two design items were **re-scoped to codegen** by spikes:
field-granular `field_own` seeding (Stage "4b") has no summary observable, and the
variant memo + SCC fixpoint (D12) re-analyzes a specialized *body* only for
emission, not for the analysis decision. All analysis precision lands *before* any
codegen, per [../architecture.md](../architecture.md)'s governing rule that all
analysis precision precedes codegen.

This track owns the proof-producing half of sound uniqueness: CFG structure,
ownership facts, liveness/last-use, summaries, candidate-classification inputs,
and debug output. It does **not** change emitted code. The codegen track turns these
facts into ANF-keyed lowering decisions.

[../architecture.md](../architecture.md) is the canonical scope/design source.
This README is the analysis worklist derived from it.

> **Phase numbering.** The whole plan uses one monotonic integer scheme where the
> number encodes execution order: **Phases 0-6 = analysis** (0-3 foundation, 4-6
> the precision below), **Phases 7-8 = codegen** (decisions/handoff, then
> emission — see [../codegen/README.md](../codegen/README.md)), and **Phases 9-10 =
> migration** (mutable intrinsics, then follow-up precision + Buffer retirement —
> see [../migration/README.md](../migration/README.md)). The tag on each heading
> (e.g. *architecture: 1C*) maps the integer to architecture.md's letter scheme,
> where **Phase 1 = all analysis (1A-1E)** and **Phase 2 = all codegen (2A+)**. All
> analysis (0-6) precedes all codegen (7-8).

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
  cross single-loop back-edges when continuing paths preserve the invariant. Nested
  loop-carried values that are mutated in an inner loop remain a documented
  follow-up; see the analysis deferrals below.
- [x] **Catalog later precision needs without implementing them yet.** Shell/field
  ownership, transport wrappers, nested collections, closure recovery, and
  concurrency precision remain in focused docs.

## Phase 3 — Shared optimizer facts and minimal summaries *(architecture: 1B + first summaries)*

Analysis side only. This phase makes ownership a shared primitive that existing
optimizer decisions can later consume; it does not introduce ownership-specialized
variants, decision records, or codegen changes.

- [x] **Move ownership-relevant pass queries to CFG facts.** Liveness, joins,
  back-edges, and publication should have one shared source of truth. Phase 3's
  design notes that the old ownership-consuming passes were removed, so this is
  primarily an audit and documentation step; candidate verdicts and decision
  records remain in the codegen track. Done — CFG facts already the single source
  (old consumers deleted in the rebuild); optimizer audited (Task 8). Details:
  [phase3-design.md](phase3-design.md).
- [x] **Dead-merge block-param pruning using Phase 2 liveness facts.** Drop
  join/loop carried params that are dead across the boundary.
- [x] **Match-arm pattern-binding precision.** Carry pattern-bound locals onto arm
  blocks so they are killed at block entry.
- [x] **Decide which local peepholes stay ANF-local.** Dead-let/copy-prop/
  const-fold/branch simplification may remain ANF-local while they do not depend
  on ownership/control-flow facts. Done — the peephole decision is recorded in
  [boot/compiler/opt/README.md](../../../../boot/compiler/opt/README.md).
- [x] **Compute minimal function summaries.** Start with consumes parameter,
  retains parameter, returns fresh value, and returns alias; use these to avoid
  treating every known helper as an unknown publication boundary. Details:
  [summary-specialization.md](summary-specialization.md),
  [phase3-design.md](phase3-design.md).

## Phase 4 — Record shell/field and nested-collection ownership *(architecture: 1C)*

The minimal domain (Phases 1-2) treats a fresh record/variant/array shell as
`Unique` and makes no claim about its contents, so the compiler's characteristic
idiom — unique record shells over dict/vector fields — still classifies as blanket
publication. This phase adds the field-sensitive layer. Still no codegen changes.
Canonical semantics: [records-fields.md](records-fields.md); implementation
design: [phase4-design.md](phase4-design.md).

- [x] **Separate shell reuse from field-backing ownership.** A record update
  carries two independent questions: shell reuse (needs the shell `Unique`) and
  field-backing in-place (needs the field's collection deeply `Unique` with no
  live alias on the old field value). A fresh shell around shared fields is not
  deep ownership.
- [x] **Model nested-collection ownership** (`Vector<Vector<T>>`,
  `Dict<K, Vector<V>>`) with the same shell-vs-deep split: an owned outer backing
  does not imply owned inner backing.
- [x] **Print the field verdicts.** Per record-update / field-projection /
  nested-write site: shell-owned, deeply-owned field, projected owned field, or
  the rejection reason (outer owned but inner shared, nested publication,
  insufficient deep ownership).

Exit: intraprocedural `advance`/`push_scope`-shaped fixtures and inlined
Case-V-shaped `Vector<Vector>` / dict-valued field updates classify with correct
shell-vs-deep verdicts; the real cross-function/recursive worked examples remain
Phase 5-6 coverage; generated code unchanged.

## Phase 5 — Transport-wrapper and `Result`-payload return-path summaries *(architecture: 1D)*

Boot threads context/state through small product records
(`SynthOut`/`CheckOut`/`ExprOut`/`FreshResult`/…) and their `Result`-wrapped
forms. Without return-path precision these look like aggregate publication and the
analysis drops to persistent across checker/lowering/resolver/query analysis. This
phase adds return-path summaries. Still no codegen changes. Canonical semantics:
[summary-specialization.md](summary-specialization.md); implementation design:
[phase5-design.md](phase5-design.md).

- [x] **Return-path summaries keyed by field and variant-payload paths.**
  `returns[.ctx]`/`[.state]`/`[.env] = OwnedFromParam(k)`, plus variant paths such
  as `Ok[0].state` / `Err[0].state`. (`Summary.ret_paths`; `RetVia.Direct|Variant`;
  three-way `classify_path_own` with a `field_own` `OwnedFresh` gate.)
- [x] **Field-projection move.** `ctx = out.ctx` / `state = out.state` transfers
  the field's ownership when that path is dead through `out` afterward; reading
  sibling result fields does not block it, but publishing / returning / storing /
  re-reading the transported path does. (Bounded transport recognizer, Task 11.)
- [x] **Path-aware liveness.** Answer whether a returned field/payload path — not
  just the wrapper local — remains observable. (Sibling-read licensing + live-out
  gate in the transport recognizer.)
- [x] **Handled-`Result` arm joins.** Merge transported payload facts like ordinary
  record-field facts; `return` / `try` early-return arms are leaf function-exit
  transfers, while value-carrying `break` stays a publication/region-exit edge to
  its loop successor (Case T). (Match-arm `seed_payload_binding`, Task 13.)

Exit: Case W (fresh-local record transport) classifies as an ownership-preserving
handoff instead of aggregate publication; generated code unchanged (census 0
in-place). The tag-aware return-site meet was hardened in-phase (commit `2d23394c`),
so any sum-typed transport keeps its payload paths — a real two-tag `Result` keeps
both arms, and `Option` keeps `Some[0].*` across a payload-less `.None` return.
**One documented
precision gap remains (sound under-approximation, deferred to the Phase 6 design
pass — see the archived plan [archive/phase5-plan.md](../../archive/sound-uniqueness-phase5-plan.md) and
the Phase 6 section below):** the caller-recovery gate fires only for **fresh unique
locals, not params**, so param-threaded state (the common idiom) — and Case R with a
param scrutinee — is not yet recovered. It folds into Phase 6 parameter-ownership;
loosening the gate earlier would be unsound. The full implementation record for
Phase 5 is [archive/phase5-plan.md](../../archive/sound-uniqueness-phase5-plan.md).

## Phase 6 — Ownership-specialization decision facts *(architecture: 1E)*

The final analysis phase produces the specialization **decisions**, not the
variants. Generating cloned variants is codegen (architecture Phase 2A); this
phase only proves and prints what those variants must be, so the specialization
story is verifiable before any code is emitted. Canonical semantics:
[summary-specialization.md](summary-specialization.md).

> **Inherits the one Phase 5 deferral — param-threaded transport recovery.** Phase 5's
> caller-recovery gate accepts an `OwnedFromParam(k)` return path only when the
> argument is a *fresh unique local*; a **parameter** argument enters the generic
> analysis as `Unknown` (a param may be shared at some caller), so the gate rejects it
> and publish-on-fail publishes it. This is the sound behavior for a phase with no
> per-call-site specialization. The owned-parameter preconditions in the first bullet
> below are exactly what lifts it: once a call site is proven to pass an owned argument
> (and an owned-specialized callee variant is selected), param-threaded state — and
> Case R with a param scrutinee — can be recovered under that precondition. Guarded by
> the `param-threaded state is NOT recovered` test in `cfg_return_paths_suite.tw`; do
> not loosen the Phase 5 gate without the precondition.

- [x] **Per-function preconditions/postconditions.** Whether each param is consumed
  / borrowed / published / returned (`ParamSummary.base_role` + `in_place_paths` +
  `flows_to_return`, Stages 2a/2b); whether the return is owned / persistent /
  published (`ret`/`ret_paths`, Phase 5 + 4a's whole-return move). Owned-entry
  re-analysis (`summarize_variant`, Stage 3) recovers param-threaded state under a
  proven precondition; the Phase 5 gate is unchanged.
- [x] **Per-call-site variant selection (logic).** `ownership.select_variant(func_id,
  callee_summary, arg_unique)` (Stage 4c-core): which callers pass proven-owned args
  (owned key) and which stay generic. Reads the callee's generic summary — exact for
  a direct-mutator callee (`add_type`, `Consumed` generically), which covers the
  flagship Case B∩C; a param-threaded/recursive callee needs its owned summary / the
  SCC fixpoint (re-scoped to codegen), with the recording pass sound (under-approximate
  to generic) without them.
- [x] **Decision recording pass + rendering.** Walk call sites and render the
  selected owned variant for direct user calls whose arguments are proven owned;
  generic decisions intentionally print no verdict in this slice to keep `--cfg`
  readable. Decisions only — no cloned variants emitted.
- [~] **SCC-granularity variant fixpoint + demand-driven memo + cap (D12)** —
  **re-scoped to codegen.** These re-analyze a specialized *body* (needed only when
  codegen emits one, incl. recursive Case V); the analysis *decision* is computed from
  the generic summary and does not need them. `ConsumedPaths` + the D6 disjoint-sibling
  read-rule + field-granular seeding ride to codegen with them. See the Stage-4b/4c
  implementation notes in [phase6-design.md](phase6-design.md).

Exit: the B∩C "one callee, two caller shapes" example prints a verifiable
per-call-site specialization decision (owned-specialized vs generic, with the
licensing proof) in `twk ir --cfg`; generated code unchanged. (Recursive Case V and
partial mixed-ownership keys are exercised once codegen re-analyzes specialized
bodies.)

## Analysis deferrals

These are **not** analysis-track work that gates codegen. Codegen-owned items live
in the codegen track; the follow-ups are conservative-by-default (sound without
them) and are refined *after* the first codegen, per architecture.md.

| Deferred work | Home |
|---|---|
| Candidate verdicts and ANF-keyed codegen decisions | [../codegen/README.md](../codegen/README.md) |
| Existing-hook codegen lowering | [../codegen/README.md](../codegen/README.md) |
| Ownership-specialized variant *generation* (the 1E decisions are analysis; cloning is codegen) | [../codegen/README.md](../codegen/README.md) |
| Compiler-private mutable intrinsic family | [../migration/README.md](../migration/README.md) |
| Extern copying-borrow precision | Post-codegen follow-up; see [concurrency-publication.md](concurrency-publication.md) |
| Non-escaping closure recovery | Post-codegen follow-up; see [closure-capture.md](closure-capture.md) |
| Advanced concurrency copy/share refinement | Post-codegen follow-up; see [concurrency-publication.md](concurrency-publication.md) |
| General per-path liveness beyond the Phase 5 transport-wrapper shape | Post-codegen follow-up (conservative-by-default; sound without it); see [phase5-design.md](phase5-design.md) |
| Return paths deeper than one field under a record / variant payload | Post-codegen follow-up (sound under-claim without it, bounded by a deeper `PathKey` codec); see [phase5-design.md](phase5-design.md) |
