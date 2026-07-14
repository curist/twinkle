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
  `SummaryTable` input; an empty table degrades to today's conservative behavior.
  - **Threading + guardrail G1.** `analyze_function` carries the Phase 2 freeze
    comment ("later tasks change only `ownership_stage`'s body"). Phase 3 needs the
    read-only table at `transfer_call`, at the far end of the
    `analyze → analyze_function → ownership_stage → run_fixpoint → forward_block →
    transfer_op → transfer_call` chain. Rather than add a bare param to every hop,
    **fold the table into the already-threaded `sem` value** (or a thin analysis-
    context record carrying `sem` + table), so no existing signature changes shape
    and G1's *algorithm* freeze is preserved — the change is a payload extension,
    not a structural edit. Note the two-phase construction: `summarize_function`
    runs against a **table-free** `sem` (it consumes only builtin `CallSemantics`
    and the in-progress SCC summaries it threads itself), then `analyze` runs
    against `sem` + the *final* table. Update the G1 comment to record that Phase 3
    extends the threaded payload (not the frozen structure).
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
//                                                                   ^^^^ local id -> sorted origin PARAM-LOCAL ids
```

`prov` maps a local to the set of **parameter locals** it may be derived from or
reachable through. Storing param *locals* (not indices) keeps publication uniform
(local→local); the boundary classifier converts to positional indices via
`f.params` when building `MayAliasParams`.

## Analysis pipeline

`prune_dead_merge(view)` → `summary.compute(view, b, sem)` → `ownership.analyze(view, b, sem, table)`:

1. **Call graph.** Edges are `ACall(AGlobalFunc(fid), …)` whose `fid` is a user
   function in the linked module. (Builtins go through `CallSemantics`.)
2. **SCC order.** Tarjan SCCs (`graph_scc.tw`), reverse-topological, so a
   callee's summary exists before its callers. **`graph_scc.strongly_connected`
   is String-keyed** (`Vector<String>` nodes, `Dict<String, Vector<String>>`
   edges), so the driver marshals `FuncId.id ↔ String` in both directions. Use a
   fixed, zero-padding-free decimal encoding of `FuncId.id` so node identity is
   canonical, and enumerate `nodes` in ascending `FuncId.id` order so the SCC
   result — and therefore the summary fixpoint order — is byte-stable across
   builds.
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

- **Seed.** Each parameter's local starts `prov = { its own local id }`; all other
  locals start `∅`. Scalar params are included by position (keeps `arg[k] ↔ param
  k`) but never acquire ownership, so they classify `Borrowed`.
- **Propagate.** `AInit`/`AWrapAnyref`/`AUnwrapAnyref`/`AAssign` copy the source's
  `prov`. **Aggregates (`ARecord`/`AVariant`/`AArrayLit`) carry the union of their
  field/element `prov`** — a shell embedding a param origin is *not* independent
  (field-path precision that would recover shell-uniqueness is Phase 6). A fresh
  allocation with no param-origin fields is `∅`. A call result's `prov` follows the
  callee's return effect: `MayAliasParams(S) → ∪ prov(arg_k) for k in S`;
  `OwnedFresh`/`Shared → ∅`.
  - **Operand type info (implementation note).** Restricting the union to
    *ref-typed* fields would be more precise, but `ownership.tw` is deliberately
    type-agnostic today — `Atom` carries no type, and the pass consults no type
    map (only `AnfFunctionDef.op_result_mono`, which is not threaded in). Phase 3
    therefore **unions the `prov` of all operands** (the sound over-approximation),
    not just ref-typed ones. This is harmless: a scalar param's origin flowing into
    an aggregate can only add scalar param indices to a `MayAliasParams` set (whose
    only effect is publishing an unboxed scalar arg — a no-op) and, per decision 8,
    scalar params are **pinned to `Borrowed`/`NoCap` at classification regardless**
    of whether transitive publish touched them. Plumbing `op_result_mono` in to
    recover ref-only precision is a later refinement, not required for soundness.
- **Join.** Positional union of predecessor `prov`, mirroring the ownership join.
- **Publish is transitive over provenance.** `publish_local(L)` sets `own[L] =
  Shared` **and** `own[o] = Shared` for every `o ∈ prov(L)`. This is the general
  soundness rule: whenever a value escapes — `AGlobalSet`, closure capture,
  field-store into a published shell, an unknown-call arg, or the terminator
  publish of a returned value — the parameter locals reachable through it are
  published too. It closes the aggregate hole (`Wrapper.{ xs }` publishes `xs`
  when the wrapper escapes) and the plain move-then-publish hole (`y := move x;
  global_set G = y` publishes `x`).
- **Classify each param `k`** (local `Lk`) over **every block's post-publish exit
  facts** (not just the return block — a param can be published on a branch and be
  dead by the return, and an aggregate-wrapped param is published by the return
  block's *terminator* publish). Two axes:
  - *escape:* `Retained` if `own[Lk] == Shared` in **any** block's exit (published
    anywhere — directly, transitively via an escaping shell, or by the terminator
    publish of a returned value that embeds it); else `Borrowed`.
    (Moved-but-not-published is **not** escape.)
  - *capability:* `Consumed` if the param was moved/invalidated (`valid == false`)
    in any block's exit; else `NoCap`. Recorded, never acted on now.
  Because escape reads the post-publish exit (which includes the terminator
  publish), the branch-published-then-dead case and the returned-wrapper case both
  classify `Retained`.
- **Classify the return** from the **body-only** state at each return block (the
  state *before* the terminator publish — otherwise the returned value always
  looks `Shared`): `MayAliasParams(idx(prov(return_atom)))` if that provenance is
  non-empty (converted to sorted positional indices); else `OwnedFresh` **only when
  the return is a genuinely independent fresh `Unique` value** (`own == Unique` and
  empty `prov`); else `Shared`. Multiple return blocks join by the return lattice.
  An aggregate that embeds a param origin therefore classifies `MayAliasParams`,
  never `OwnedFresh`. (Escape, above, reads the *post-publish* exit; the return
  reads *body-only* — the two axes intentionally read different states.)

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

### Optimization reach — what Phase 3 does and does *not* recover

Stated explicitly because the `MayAliasParams → publish every arg` demotion looks,
in isolation, like it gives back nothing. Verified against
[worked-examples.md](worked-examples.md):

- **The census-dominant wins are *not* recovered in Phase 3, by design.** The
  consume-produce helpers — `set_at` (Case A), `add_type` (Case B), `visit`
  (Case V), i.e. the **171 record + 267 dict in-place sites** — all *return their
  threaded param* (`… ; assign env; return env`). Phase 3 classifies each as
  `p0=retain, ret=alias(p0)` (the return-block terminator publishes the returned
  atom, and its `prov` traces to the param), so the caller arg **and** result are
  demoted to `Shared`. This is the same conservative verdict as today's blanket
  publish boundary — **no regression, but no win either** for these sites.
- **That is sound and intended, not a dead end.** Phase 3 is analysis-only and
  emits zero in-place (Acceptance 9), so no realized optimization is lost. The
  unique consume-produce handoff (`OwnedFromParam(k)`, which keeps the value
  `Unique`) is deliberately absent from the Phase 3 return lattice: recovering it
  needs **ownership specialization** — re-analyzing the callee under a `Unique`-
  entry assumption — which is **Phase 6**
  ([summary-specialization.md](summary-specialization.md), computing-summaries
  steps 1 & 3). Phase 3 computes exactly the conservative "params enter `Unknown`"
  generic summary that Phase 6 specialization takes as input; it does not throw
  away information Phase 6 needs, because Phase 6 re-analyzes rather than reading a
  richer generic summary, and its call-site selection reads the **incoming** arg
  fact (which Phase 3 preserves at the call boundary — the generic demote only
  affects the generic variant's downstream, and these sites rebind the arg
  immediately).
- **Phase 3's actual precision win is narrow but real:** read-only (`Borrowed`)
  args stay `Unique` instead of being published, and `OwnedFresh` results are
  `Unique`. The observable is more precise `--cfg` facts at read-only-helper and
  allocator call sites — the foundation the later phases consume, not the headline
  in-place sites themselves.

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

## Implementation risks / assumptions to validate

Surfaced while grounding this design against the branch; each is a soundness-safe
default with a cheap validation step for the execution plan.

- **`collect_pattern_bindings` returns `Dict<Int, Bool>`, not `Vector<Int>`.** The
  match-arm item must convert its keys to a sorted `Vector<Int>` for
  `CfgBlock.bound`. Also, `build_match` creates each arm block with **empty
  params** *before* `build_expr` populates it, so `bound` is set on the arm entry
  block (`arm_ids[i]`) after creation, not derived from params. Both are
  mechanical; the fixture (Acceptance 8) covers correctness.
- **Positional `arg[k] ↔ param[k]` (decision 8) assumes no ABI-prepended param.**
  Direct `AGlobalFunc` callees are top-level monomorphized functions whose
  `CfgFunction.params` are declaration-order, so the identity should hold — but
  validate against a call site with a receiver/inherent-method desugaring and a
  captured-env case to confirm no implicit slot shifts the index. Indirect/closure
  callees already take the conservative path, so only direct calls matter.
- **Header rendering interleaves with `cfg.render_view`.** `render_view` renders
  the whole view and knows nothing about summaries, while the header line is
  per-function. Either give `render_view` an optional `SummaryTable` (rendered
  before each function's blocks) or have `commands/ir.tw` render function-by-
  function, prefixing `summary.render_header(table, f)`. Keep the summary→cfg
  dependency direction acyclic (summary.tw already depends on ownership/cfg).
- **Summaries are whole-program even for a single `--cfg` render.** A consumed
  callee summary can live anywhere, so `summary.compute` analyzes every function
  (a full liveness + forward fixpoint per function) *in addition to* the final
  `analyze` pass — roughly doubling `--cfg`'s per-function work. Acceptable for a
  debug command; note it so it is not mistaken for a regression, and so it is not
  copied onto a hot path in Phase 4 without caching.

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
  record ownership, or specialization (Phase 6). Consequently a helper that wraps
  a param into a returned/escaping aggregate is treated **conservatively**
  (`param Retained`, `return MayAliasParams`), not optimistically as `OwnedFresh`;
  recovering shell-uniqueness for such transport wrappers is Phase 6.
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
2. **Aggregate-escape soundness** — a wrapper helper `wrap(xs) = Wrapper.{ xs }`
   (plus a variant `.Some(xs)` and an array `[xs]` variant) classifies
   `p0=retain, ret=alias(p0)`; at the caller the arg is demoted to `Shared` and the
   result is **not** `Unique`. Also the plain move-then-publish case
   (`y := <move x>; global_set G = y`) demotes `x` to `Retained`.
3. **Capability is recorded, not acted on** — a callee that moves-but-does-not-
   publish a param (no escape) leaves the caller arg **valid** (not invalidated),
   while the summary header shows `(consumed)`.
4. **Conservatism** — unknown/extern/indirect/`Cell` callees still publish all
   ref args and return `Unknown`.
5. **Recursion** — a mutually-recursive SCC: the fixpoint is stable, terminates
   (within the cap), and yields a sound (conservative-where-needed) summary.
6. **Determinism** — `twk ir --cfg` (headers + facts) byte-identical across two
   builds.
7. **Pruning consistency** — after `prune_dead_merge`,
   `count_edge_arity_mismatches == 0` and reciprocal pred/succ edges hold; a
   provably-dead carried param is gone; ownership/live/valid maps are consistent
   (analysis ran on the pruned view).
8. **Pattern-binding precision** — the G3 fixture: the bound local is killed at
   arm entry (not leaked live-in), still never `Unique`, no trap.
9. **No regression to in-place** — `twk ir --census` still shows **0 in-place**
   (Phase 3 changes no codegen).
10. **Optimizer audit** — a documented grep/reasoning check that no `opt/` pass has
    an independent liveness/ownership/legality path; `opt/README.md` updated and
    consistent with the current pass set.
11. **Full verification** — `make boot-test` green and `make stage2` reaches the
    self-host fixed point (Phase 3 is boot-only and adds no stage0-parity
    construct).

## Deferrals and tracking

| Item | Home |
|---|---|
| "Move ownership-relevant pass queries to CFG facts" | Satisfied (old consumers deleted; CFG facts already single) — evidenced by the optimizer audit (Acceptance 10), not new migration code |
| Caller-binding invalidation / real consumption | Phase 6 |
| Candidate verdicts / decision records | Phase 4 |
| Field-path / return-path / specialization | Phase 6 |
| Extern copying-borrow precision | Phase 8 |

The Phase 3 execution plan marks the delivered README bullets and records the
vacuous-bullet reframing.

## Determinism-sensitive spots (lock with tests)

- **`prov` sets** — sorted `Vector<Int>`, same discipline as `live`; no hash-set
  iteration in an order-sensitive path.
- **Summary fixpoint** — SCC order from Tarjan over a call graph whose nodes are
  enumerated in ascending `FuncId.id` order (via the canonical decimal string
  encoding) and whose edges are enumerated in deterministic ANF traversal order;
  the per-function transfer and the lattice joins are commutative/idempotent, so
  iteration order does not affect the result; the iteration cap is deterministic
  (`members × 4`).
- **`--cfg` header + facts** — params in index order; `SummaryTable` keyed by
  `FuncId.id`, rendered per function in id order. Byte-identical across builds
  (gated by Acceptance 5).
