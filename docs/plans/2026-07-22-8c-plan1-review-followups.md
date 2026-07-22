# 8C Plan 1 — Review Findings & Follow-ups

**Date:** 2026-07-22. **Branch:** `sound-uniqueness-8c-builder-region`.

Disposition of the deep-review findings on Plan 1 (the structural `linearly_folded` builder-region
fact + inspection). The review verdict was **sound & correct, merge-ready**; these are the concrete
items worth acting on. Parent design: `docs/plans/sound-uniqueness/codegen/builder-region-design.md`.

## Done on this branch

| # | Finding | Fix | Commit |
|---|---|---|---|
| **F1** | Range/index loops (`for i in range(n) { acc = acc.concat(…) }`) — the most common string-building shape — never certified: `loop_main_arm` bailed on range's step-sign `AIf` before reaching the break-dispatch. | `loop_main_arm` now skips a non-break-dispatch `AIf` that doesn't reference `acc` (recurses) and bails only if it does. Added 4 adversarial tests (range string + vector certify; interior-read + conditional-fold in a range loop reject). | `27ecbfad` |
| **R1** | `acc: Vector<RegionCandidate>` param in `detect_in_expr`/`detect_in_op` shadowed the accumulator-`LocalId` meaning of `acc`. | Renamed → `cands`. | `27ecbfad` |
| **F2** | A malformed (non-self-concat) fold reported a "self-concat / … or malformed" reason, misleading the census. | Split via `is_self_concat`: distinct self-concat vs malformed-tail reasons. | `27ecbfad` |

## Confirmed non-issues (no action)

- **A2 "dead branch"** — the `_ => "malformed fold tail"` arm in `scan_main_arm` is a *mandatory*
  `case` exhaustiveness arm (`body: AnfExpr` can't be statically narrowed to `Let`), not removable
  dead code. Leave as-is.

## Pending — actionable follow-ups

### FU-1 (was F3) — Plan 2 must add a fold-result liveness gate *(soundness gate; blocks Plan 2 emission)*

**What.** Plan 1 certification implicitly assumes the fold-call result temp is dead after the
`acc = result` reassign. Today that holds *only* because `fold_chunk` matches a **direct**
`AAssign(acc, ALocal result)` where `result` is a synthetic single-use temp (a named/live temp gets
an intervening `AInit` node and is rejected — verified). This rests on the optimizer never
copy-propagating a live temp into the direct reassign position.

**Risk.** If a future opt pass produced a *live* fold-result local in that position, Plan 2's rewrite
(which neutralizes the reassign to `AInit(.ALitVoid)` and replaces the fold with a void
`builder_push`) would drop a value that is still read → **miscompile**.

**Action (Plan 2, in the rewrite's structural validation, before neutralizing any fold):** add a
cheap last-use / single-use check that the fold-result local has **no use other than the reassign**.
Reject the region (persistent fallback) if it does. This makes the deadness assumption explicit
rather than trusted.

**Acceptance.** A fixture where the fold result is (synthetically) read after the reassign is not
rewritten; the normal case still is; the check is in Plan 2's validation path, not codegen re-proof.

**Where.** Fold into `builder-region-design.md` Component 3 ("Concrete ANF rewrite shape") as a
pre-neutralization guard.

### FU-2 (was review item b) — Surface a second fold over the same accumulator local *(Plan 2)*

**What.** `acc := ""; for … { acc = acc.concat(…) }; use(acc); for … { acc = acc.concat(…) }` —
the second loop's region is not detected at all (not even as a rejected candidate), because
`seed_family` only fires on a fresh `AInit` empty-seed binding and the second loop reuses the
already-bound `acc`.

**Severity.** Conservative and **sound** (no false certification; Plan 2 only ever touches the first
loop's boundary). It is an inspection-completeness gap, not a correctness bug.

**Action (Plan 2, alongside the non-overlap machinery):** when the producer builds
`BuilderRegionDecision` records with `BuilderRegionKey` + non-overlap handling, also recognize a
re-folded accumulator whose value re-enters a subsequent loop as its own region (or explicitly
render it as a rejected candidate with a "re-used accumulator, not a fresh seed" reason). Keyed
distinctly by seed/loop site so it can't collide with the first region.

**Acceptance.** The two-loop-same-`acc` fixture surfaces two candidates (or one certified + one
rejected-with-reason), never a silent drop.

### FU-3 (was R2) — Unify the ANF-walking functions *(refactor; deferred)*

**What.** `detect_in_expr`/`detect_in_op`, `references_fold`/`op_fold_ref`, `collect_fold_sites`, and
`op_references_deep` all walk ANF and share the "recurse into `AIf`/`AMatch`/`ALoop`/`ADefer`
children" shape, differing mainly in return type (Bool vs threaded `Vector`).

**Why deferred.** Unifying needs a generic ANF children-fold combinator. The MEMORY-noted
`core_fold.tw` is **Core-IR-only and unbuilt**; there is no ANF equivalent. Building one is its own
piece of work.

**Constraint (must preserve soundness).** `op_references_deep`'s **no-wildcard, per-variant
exhaustive** enumeration is deliberate — a new `AnfOp` variant must force a compile error there. Any
combinator must keep that exhaustiveness property (no `_ =>` default that silently passes an
unclassified op). If a combinator can't guarantee this, `op_references_deep` stays hand-written.

**Action.** Deferred. Revisit if/when an ANF `fold_children` combinator is built (candidate: pair it
with the Core-IR `core_fold.tw` effort). Not scheduled.

## Summary

- **Ship now:** Plan 1 with F1/R1/F2 applied (this branch).
- **Plan 2 must-do:** FU-1 (liveness gate — soundness) and FU-2 (second-fold surfacing — via the
  non-overlap machinery). Add both to `builder-region-design.md` when Plan 2 is written.
- **Unscheduled refactor:** FU-3 (ANF walker combinator), gated on an exhaustiveness-preserving
  `fold_children`.
