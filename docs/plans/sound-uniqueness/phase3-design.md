# Phase 3 Implementation Design — Shared Ownership Facts + Minimal Summaries

**Status:** Draft design (feeds the Phase 3 execution plan)

This is the implementation design for **Phase 3** of the sound-uniqueness track:
give the Phase 2 intraprocedural ownership analysis a first layer of
**interprocedural function summaries** so a call to a known helper stops being a
blanket publication boundary, plus two small facts-precision items. It remains
**analysis-only** — no codegen, decision records, mutable intrinsics, or in-place
lowering (those are Phases 4–5+).

Canonical semantics: [summary-specialization.md](summary-specialization.md) (the
summary lattice and SCC fixpoint), [fact-lattice.md](fact-lattice.md), and
[cfg-ownership-ir.md](cfg-ownership-ir.md). It implements the Phase 3 bullets in
[README.md](README.md). On any genuine conflict the canonical docs win and this
doc is corrected.

## Current state (verified against the branch)

Established before designing, because it reframes the phase:

- **In-place lowering is entirely removed on this branch.** Commit `5d5ac090`
  deleted `opt/uniqueness.tw`, `opt/liveness.tw`, `opt/builder_region.tw`,
  `opt/loop_builder.tw` and gutted `opt/analysis.tw`; `opt/pipeline.tw` now runs
  only defer-elim + the general peepholes. `twk ir --census` reports 0 in-place.
  Everything is persistent — an intentional, temporary regression while the sound
  engine is rebuilt.
- **Phase 2 `ownership.tw` facts are render-only.** Nothing imports `cfg`/
  `ownership` except `commands/ir.tw` (`--cfg`). No optimizer pass consumes them.
- **Surviving peepholes** (`dead_let`, `copy_prop`, `const_fold`, `branch_simp`,
  `defer_elim`) are ANF-local and do not consult ownership/control-flow facts.
- **`opt/README.md` is stale** — it still describes the deleted
  `uniqueness.tw`/`liveness.tw`/builder-region passes.
- **`artifacts.opt` is the fully-linked, monomorphized whole-program module**
  (census and `--cfg` already run on it), so the call graph is whole-program and
  there is **no cross-module summary concern**.
- **`graph_scc.tw` (Tarjan SCC) already exists** and is reusable for call-graph
  ordering.
- **`ForwardState = .{ own, valid }`** — no param-provenance today. Builtin
  `CallSemantics` (effect + `cow_base_arg` + `in_place_equivalent`) are consumed by
  `transfer_call`; **user-function summaries do not exist.**

**Reframing of README bullet 1** ("move ownership-relevant pass queries to CFG
facts … one shared source of truth"): the old ownership-consuming passes were
deleted in the rebuild, so there is **nothing to migrate** — the CFG facts are
already the single source. Phase 3 does **not** silently narrow the 1B intent: it
keeps candidate verdicts / decision records where the README already puts them
(**Phase 4**), and it *proves* the "single source" claim with an explicit
optimizer audit (see Acceptance). The substantive Phase 3 work is summaries + the
two precision items + doc/audit hygiene.

## Scope

**In scope (Phase 3):**

- **Minimal function summaries**, whole-program, bottom-up over call-graph SCCs.
  Two independent axes per the review (see Data model):
  - **caller-visible escape** — `Borrowed` vs `Retained`; the only axis
    `transfer_call` acts on;
  - **future mutable capability** — `Consumed` (recorded for Phases 4–6, **not**
    acted on now);
  - **return provenance** — `OwnedFresh` / `MayAliasParams(set)` / `Shared`.
- **Consume the escape + return axes in `transfer_call`** so borrowed args stay
  `Unique`, fresh returns become `Unique`, retained args and aliased-return
  origins demote to `Shared`. Primary observable: **more precise `twk ir --cfg`
  facts.**
- **Dead-merge block-param pruning (minimal, pre-analysis).** Drop only join/loop
  carried params that are dead across the boundary, realign edge args, then run
  the full analysis on the pruned view (no stale facts).
- **Match-arm pattern-binding precision.** Kill pattern-bound locals at arm entry
  — retires the Phase 2 G3 over-approximation.
- **Doc + audit hygiene.** Rewrite the stale `opt/README.md` to reflect the
  current passes and record the ANF-local peephole decision; audit the optimizer
  to prove no pass retains independent ownership/liveness/legality logic.
- **Fold summary display into `--cfg`** (a per-function header line).

**Out of scope (deferred):**

- **Caller-binding invalidation / real consumption of args.** Only a later
  specialized mutable variant selected with an ownership key + last-use proof may
  invalidate a caller binding → **Phase 6** ([summary-specialization.md](summary-specialization.md)).
  Phase 3 records `Consumed` as a capability but never invalidates a caller arg.
- **Candidate verdicts and decision records** → **Phase 4** (README as written).
- **Field-path / return-path summaries** (`out.ctx`, `Ok[0].state`),
  field-sensitive record ownership, and bounded ownership specialization →
  **Phase 6**.
- **Any codegen or in-place lowering** → **Phase 4/5**.
- **Extern copying-borrow precision** → **Phase 8** (externs stay conservative).

Guiding rule (unchanged): **soundness before coverage.** A summary is consumed to
be *less* conservative than "publish everything", so any unknown, extern,
indirect, or in-progress-recursive callee keeps the conservative default (params
`Retained`, return `Shared`).

## Locked decisions

| # | Decision | Choice |
|---|---|---|
| 1 | How summaries are computed | Provenance-augmented forward pass — fold a per-local origin-param map into `ForwardState`; reuse the Phase 2 engine |
| 2 | Summary schema | **Two axes**: caller-visible escape (`Borrowed`/`Retained`) that `transfer_call` acts on, and recorded capability (`Consumed`) that it does not; return provenance is a **param set** |
| 3 | Caller consumption | `transfer_call` acts on escape + return only; **never invalidates a caller binding** (that needs specialization, Phase 6) |
| 4 | Interprocedural order | Whole-program call graph; Tarjan SCCs bottom-up; conservative seed; iterate to a fixpoint or a cap |
| 5 | Inspection surface | Folded into `--cfg` header line; **no** new command |
| 6 | Module layout | Summary *types* + provenance + classification + consumption in `ownership.tw` (acyclic); interprocedural driver + header rendering in new `summary.tw` |
| 7 | Dead-merge pruning | Minimal **pre-analysis** view transform (dead-across-boundary carried params only); analysis re-runs on the pruned view |
| 8 | Param indexing | Summaries index **all** params positionally (scalars included), so param index == call-argument index; scalar params are always `Borrowed`/no-cap |

## Module layout

- **`ownership.tw`** (extend) — owns the summary *types*, the `prov` field on
  `ForwardState`, per-function effect **classification**, and **call-site
  consumption** in `transfer_call`. Types live here (mirroring `cfg` owning
  `BlockFacts`) so `summary.tw → ownership.tw` is acyclic. `analyze` gains a
  `SummaryTable` param; an empty table degrades to today's conservative behavior.
- **`summary.tw`** (new) — the **interprocedural driver**: extract the
  whole-program call graph, order via `graph_scc.tw`, run the bottom-up fixpoint
  (calling an `ownership.summarize_function`-style entry), return the final
  `SummaryTable`, and render the per-function header.
- **`commands/ir.tw`** — `--cfg` becomes
  `build_view → prune_dead_merge → summary.compute → ownership.analyze(…, table) → render`.

### Data model (in `ownership.tw`)

```tw
// Caller-visible escape — the ONLY axis transfer_call acts on. 2-point lattice:
// Borrowed ⊑ Retained, with Retained the conservative top.
pub type EscapeEffect = { Borrowed, Retained }

// Future in-place capability — recorded for Phases 4-6; NOT acted on in Phase 3.
// Conservative default NoCap (never claim consumption we cannot prove).
pub type ParamCapability = { NoCap, Consumed }

pub type ParamSummary = .{ escape: EscapeEffect, capability: ParamCapability }

// Return provenance. Lattice: OwnedFresh ⊑ MayAliasParams(S) ⊑ Shared, where
// MayAliasParams(S1) ⊑ MayAliasParams(S2) iff S1 ⊆ S2. Conservative top = Shared.
pub type ReturnEffect = { OwnedFresh, MayAliasParams(Vector<Int>), Shared }

pub type Summary = .{ params: Vector<ParamSummary>, ret: ReturnEffect }
pub type SummaryTable = .{ by_func: Dict<Int, Summary> }
```

`ForwardState` gains one field:

```tw
type ForwardState = .{ own: Dict<Int, Int>, valid: Dict<Int, Bool>, prov: Dict<Int, Vector<Int>> }
//                                                                   ^^^^ local id -> sorted origin param indices
```

## Analysis pipeline

`prune_dead_merge(view)` → `summary.compute(view, b, sem)` → `ownership.analyze(view, b, sem, table)`:

1. **Call graph.** Edges are `ACall(AGlobalFunc(fid), …)` whose `fid` is a user
   function in the linked module. (Builtins go through `CallSemantics`.)
2. **SCC order.** Tarjan SCCs (`graph_scc.tw`), reverse-topological, so a
   callee's summary exists before its callers.
3. **Per-SCC fixpoint.** Seed each member conservatively (params `Retained`,
   return `Shared`; capability `NoCap`), so the first pass is sound before callees
   are known. Recompute each member's summary from its body using the current
   table and **replace** it. Because callee summaries only move **down** the
   lattice (toward `Borrowed`/`OwnedFresh`) and the per-function transfer is
   monotone, a member's recomputed summary never regresses; the finite lattice
   guarantees convergence. Cap the per-SCC iterations at `members × 4`; on cap,
   keep the current (still-sound) summaries. Singleton non-recursive SCCs (the
   common case) need exactly one pass.
4. **`analyze`.** Runs the Phase 2 pipeline with the final table; `transfer_call`
   consumes it. Produces the `--cfg` facts.

`summarize_function(f, table, b, sem)` runs the provenance-augmented forward pass
over `f`'s blocks (consuming `table` at its own call sites) and classifies `f`'s
effects at the boundary.

### Provenance semantics

- **Seed.** Each parameter's local starts `prov = { its positional index }`; all
  other locals start `∅`. Scalar params are included by position (keeps
  `arg[k] ↔ param k`) but never acquire ownership, so they classify `Borrowed`.
- **Propagate.** `AInit`/`AWrapAnyref`/`AUnwrapAnyref`/`AAssign` copy the source's
  `prov`. Fresh allocations and aggregates (`ARecord`/`AVariant`/`AArrayLit`)
  produce `∅` (nested-field provenance is Phase 6). A call result's `prov` follows
  the callee's return effect: `MayAliasParams(S) → ∪ prov(arg_k) for k in S`;
  `OwnedFresh`/`Shared → ∅`.
- **Join.** Positional union of predecessor `prov`, mirroring the ownership join.
- **Classify each param `k`** from its exit facts (two independent axes):
  - *escape:* `Retained` if param k's local is published (`own == Shared` at the
    exit meet); else `Borrowed`. (Being moved-but-not-published is **not** escape.)
  - *capability:* `Consumed` if the pass moved/invalidated it (`valid == false` at
    exit, or a consuming op on it); else `NoCap`. Recorded, never acted on now.
- **Classify the return:** `MayAliasParams(prov(return_atom))` if that provenance
  is non-empty (the full set, sorted); else `OwnedFresh` if `own == Unique`; else
  `Shared`.

### Consumption in `transfer_call` (Phase 3)

For a **direct callee with a known summary** (replaces the blanket publish bucket):

| Summary element | Effect at the call site |
|---|---|
| return `OwnedFresh` | result `← Unique` |
| return `MayAliasParams(S)` | publish **every** `arg_k` for `k ∈ S` (`→ Shared`) and result `← Shared` |
| return `Shared` | result `← Unknown` |
| param escape `Borrowed` | arg unchanged (**the win — borrowed args stay `Unique`**) |
| param escape `Retained` | publish arg `→ Shared` |
| param capability `Consumed` | **no caller-binding effect in Phase 3** (recorded only; consumption/invalidation is Phase 6) |

Builtins still resolve via `CallSemantics`. **Unknown Twinkle call without a
summary, extern, indirect/closure callee, or `Cell` op → keep the conservative
publish-all-ref-args bucket.**

## Precision items

- **Dead-merge param pruning (minimal, pre-analysis).** `prune_dead_merge(view)`
  computes liveness only, then for each join/loop block drops any carried param
  **not** in that block's live-in set and removes the positionally-aligned atom
  from every predecessor edge and matching terminator payload. Removing a
  *dead* param cannot change any other local's liveness, so the result is
  self-consistent; the full ownership/summary analysis then runs fresh on the
  pruned view (**no stale fact maps**). Guarded by an arity-reciprocity check
  (`count_edge_arity_mismatches` == 0) and a fixture. (Render-only filtering is
  the even-more-minimal fallback if the transform proves fiddly.)
- **Match-arm pattern-binding precision (retires G3).** `build_match` records
  `anf_analysis.collect_pattern_bindings(arm.pattern)` on a new
  `CfgBlock.bound: Vector<Int>` (empty elsewhere). `scan_block_backward` kills
  `bound` at block entry; the forward pass seeds those locals `Unknown` (a
  destructured payload is a borrow from the scrutinee). The G3 fixture flips from
  "sound-but-imprecise" (leaked live-in) to precise.
- **Doc + audit hygiene.** Rewrite `opt/README.md` to describe the *current*
  passes (drop the deleted `uniqueness`/`liveness`/builder-region descriptions)
  and record that `dead_let`/`copy_prop`/`const_fold`/`branch_simp` stay ANF-local
  because they do not consult ownership/CF facts. Audit (grep + reasoning) that no
  optimizer pass retains independent ownership/liveness/legality logic — the
  evidence backing the "single source of truth" claim.

## Rendering

`--cfg` gains a per-function header line, e.g.:

```
fn f [FuncId(295)]  summary: p0=borrow p1=retain(consumed) ret=alias(p1)
```

Deterministic: params in index order; `ret=fresh` / `ret=alias(pA,pB)` /
`ret=shared`; the `(consumed)` capability tag appears only when set. Improved
block facts (borrowed-helper call sites now `Unique`) are the real signal.

## Non-goals

- No caller-binding invalidation / real consumption (Phase 6).
- No candidate verdicts or decision records (Phase 4).
- No field-path / return-path summaries, transport wrappers, field-sensitive
  record ownership, or specialization (Phase 6).
- No codegen or in-place emission (Phase 4/5).
- No extern copying-borrow precision (Phase 8).
- No change to the surviving ANF-local peepholes beyond the doc decision.

## Acceptance criteria

Concrete gates for the execution plan (all via the boot suite unless noted):

1. **Escape/return facts** — hand-built multi-function fixtures: callee
   returns-fresh ⇒ caller result `Unique`; borrows param ⇒ caller arg stays
   `Unique`; publishes param ⇒ caller arg `Shared`; returns-alias-of-param ⇒
   result `Shared` **and the origin arg** `Shared`; multi-origin return ⇒ **all**
   origin args `Shared`.
2. **Capability is recorded, not acted on** — a callee that moves-but-does-not-
   publish a param leaves the caller arg **valid** (not invalidated), while the
   summary header shows `(consumed)`.
3. **Conservatism** — unknown/extern/indirect/`Cell` callees still publish all
   ref args and return `Unknown`.
4. **Recursion** — a mutually-recursive SCC: the fixpoint is stable, terminates
   (within the cap), and yields a sound (conservative-where-needed) summary.
5. **Determinism** — `twk ir --cfg` (headers + facts) byte-identical across two
   builds.
6. **Pruning consistency** — after `prune_dead_merge`,
   `count_edge_arity_mismatches == 0` and reciprocal pred/succ edges hold; a
   provably-dead carried param is gone; ownership/live/valid maps are consistent
   (analysis ran on the pruned view).
7. **Pattern-binding precision** — the G3 fixture: the bound local is killed at
   arm entry (not leaked live-in), still never `Unique`, no trap.
8. **No regression to in-place** — `twk ir --census` still shows **0 in-place**
   (Phase 3 changes no codegen).
9. **Optimizer audit** — a documented grep/reasoning check that no `opt/` pass has
   an independent liveness/ownership/legality path; `opt/README.md` updated and
   consistent with the current pass set.
10. **Full verification** — `make boot-test` green and `make stage2` reaches the
    self-host fixed point (Phase 3 is boot-only and adds no stage0-parity
    construct).

## Deferrals and tracking

| Item | Home |
|---|---|
| "Move ownership-relevant pass queries to CFG facts" | Satisfied (old consumers deleted; CFG facts already single) — evidenced by the optimizer audit (Acceptance 9), not new migration code |
| Caller-binding invalidation / real consumption | Phase 6 |
| Candidate verdicts / decision records | Phase 4 |
| Field-path / return-path / specialization | Phase 6 |
| Extern copying-borrow precision | Phase 8 |

The Phase 3 execution plan marks the delivered README bullets and records the
vacuous-bullet reframing.

## Determinism-sensitive spots (lock with tests)

- **`prov` sets** — sorted `Vector<Int>`, same discipline as `live`; no hash-set
  iteration in an order-sensitive path.
- **Summary fixpoint** — SCC order from Tarjan over a call graph whose edges are
  enumerated in deterministic ANF traversal order; the per-function transfer and
  the lattice joins are commutative/idempotent, so iteration order does not affect
  the result; the iteration cap is deterministic (`members × 4`).
- **`--cfg` header + facts** — params in index order; `SummaryTable` keyed by
  `FuncId.id`, rendered per function in id order. Byte-identical across builds
  (gated by Acceptance 5).
