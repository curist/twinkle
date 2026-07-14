# Phase 3 Implementation Design — Shared Ownership Facts + Minimal Summaries

**Status:** Draft design (feeds the Phase 3 execution plan)

This is the implementation design for **Phase 3** of the sound-uniqueness track:
give the Phase 2 intraprocedural ownership analysis a first layer of
**interprocedural function summaries** so a call to a known helper stops being a
blanket publication boundary, plus two small facts-precision items. It remains
**analysis-only** — no codegen or in-place lowering (that is Phases 4–5).

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
- **`artifacts.opt` is the fully-linked, monomorphized whole-program module**
  (census and `--cfg` already run on it), so the call graph is whole-program and
  there is **no cross-module summary concern**.
- **`graph_scc.tw` (Tarjan SCC) already exists** (used for module groups) and is
  reusable for call-graph ordering.
- **`ForwardState = .{ own, valid }`** — no param-provenance today. Builtin
  `CallSemantics` (effect + `cow_base_arg` + `in_place_equivalent`) exist and are
  consumed by `transfer_call`; **user-function summaries do not exist.**

**Reframing of README bullet 1** ("move ownership-relevant pass queries to CFG
facts … one shared source of truth"): it was written assuming the old
ownership-consuming passes still existed. They were deleted in the rebuild, so
there is **nothing to migrate** — the CFG facts are already the single source.
Combined with bullet 4 (peepholes stay ANF-local because they don't depend on
ownership), this bullet reduces to a decision record, not code. The substantive
Phase 3 work is summaries + the two precision items.

## Scope

**In scope (Phase 3):**

- **Minimal function summaries**, whole-program, computed bottom-up over
  call-graph SCCs: per parameter `Borrowed` / `Retained` / `Consumed`; per return
  `OwnedFresh` / `OwnedFromParam(k)` / `Shared`.
- **Consume summaries in `transfer_call`** so borrowed args stay `Unique`, fresh
  returns become `Unique`, consumed args are moved, retained args are published.
  The primary observable is **more precise `twk ir --cfg` facts.**
- **Dead-merge block-param pruning (minimal).** Drop only join/loop carried
  params that are dead across the boundary (`BlockFacts.live`), realigning edge
  args positionally.
- **Match-arm pattern-binding precision.** Carry
  `collect_pattern_bindings(arm.pattern)` onto arm blocks so pattern-bound locals
  are killed at block entry — retires the Phase 2 G3 over-approximation.
- **Record the ANF-local peephole decision** in `opt/README.md`.
- **Fold summary display into `--cfg`** (a per-function header line).

**Out of scope (deferred):**

- Field-path / return-path summaries (`out.ctx`, `out.state`, `Ok[0].state`) and
  field-sensitive record ownership → **Phase 6**
  ([summary-specialization.md](summary-specialization.md),
  [records-fields.md](records-fields.md)).
- Bounded ownership specialization / per-callee variants → **Phase 6**.
- Any codegen, decision records, or in-place lowering → **Phase 4/5**.
- Extern copying-borrow precision → **Phase 8** (externs stay conservative).

Guiding rule (unchanged from Phase 2): **soundness before coverage.** A summary is
consumed to be *less* conservative than "publish everything", so any unknown,
extern, indirect, or in-progress-recursive callee keeps the conservative default
(all params `Retained`, return `Shared`).

## Locked decisions

| # | Decision | Choice |
|---|---|---|
| 1 | How summaries are computed | Provenance-augmented forward pass — fold a per-local origin-param map into `ForwardState`; reuse the Phase 2 engine (one source of truth) |
| 2 | Interprocedural order | Whole-program call graph over linked ANF; Tarjan SCCs (`graph_scc.tw`) bottom-up; recursion iterated to a fixpoint with a conservative seed |
| 3 | Consumption | Summaries feed `transfer_call`; primary observable is improved `--cfg` facts |
| 4 | Inspection surface | Folded into `--cfg` as a per-function header line; **no** new command |
| 5 | Module layout | Summary *types* + provenance + classification + consumption live in `ownership.tw` (no import cycle); the interprocedural driver + header rendering live in a new `summary.tw` |
| 6 | Dead-merge pruning | Kept **minimal**: only carried params dead across the boundary; a focused view rewrite, one fixture |

## Module layout

- **`ownership.tw`** (extend) — owns `ParamEffect`, `ReturnEffect`, `Summary`,
  `SummaryTable`; the provenance field on `ForwardState`; per-function effect
  **classification**; and **call-site consumption** in `transfer_call`. Types live
  here (mirroring how `cfg` owns `BlockFacts`) so `summary.tw → ownership.tw` is
  acyclic. `analyze` gains a `SummaryTable` parameter; an empty table degrades to
  today's conservative behavior.
- **`summary.tw`** (new) — owns the **interprocedural driver**: extract the
  whole-program call graph, order it via `graph_scc.tw`, run the bottom-up
  fixpoint (calling an `ownership.summarize_function`-style entry), return the
  final `SummaryTable`, and render the per-function summary header.
- **`commands/ir.tw`** — `--cfg` becomes
  `build_view → summary.compute → ownership.analyze(…, table) → render`.

### Data model (in `ownership.tw`)

```tw
pub type ParamEffect = { Borrowed, Retained, Consumed }
pub type ReturnEffect = { OwnedFresh, OwnedFromParam(Int), Shared }
pub type Summary = .{ params: Vector<ParamEffect>, ret: ReturnEffect }
// keyed by FuncId.id
pub type SummaryTable = .{ by_func: Dict<Int, Summary> }
```

`ForwardState` gains one field:

```tw
type ForwardState = .{ own: Dict<Int, Int>, valid: Dict<Int, Bool>, prov: Dict<Int, Vector<Int>> }
//                                                                   ^^^^ local id -> sorted origin param indices
```

## Analysis pipeline

`summary.compute(view, b, sem) SummaryTable`, then `ownership.analyze(view, b,
sem, table)`:

1. **Call graph.** Edges are `ACall(AGlobalFunc(fid), …)` whose `fid` is a user
   function in the linked module. (Builtins are handled by `CallSemantics`, not
   summaries.)
2. **SCC order.** Tarjan SCCs, processed reverse-topologically so a callee's
   summary exists before its callers.
3. **Per-SCC fixpoint.** Seed every member conservatively (all params `Retained`,
   return `Shared` — today's `.None` bucket), so the first pass is sound even
   before callees are known. Recompute each member's summary from its body using
   the current table; repeat until the SCC's summaries stop changing. A recursive
   call reads the current summary, which only **refines** (never regresses) across
   passes, so over the finite lattice the iteration converges. Singleton SCCs
   (the common case) need exactly one pass.
4. **`analyze`.** Runs the Phase 2 pipeline once more with the final table;
   `transfer_call` consumes it (section below). Produces the `--cfg` facts.

`summarize_function(f, table, b, sem)` runs the provenance-augmented forward pass
over `f`'s blocks (consuming `table` at its own call sites) and reads the boundary
to classify `f`'s effects.

### Provenance semantics

- **Seed.** Each function parameter's local starts `prov = { its index }`. All
  other locals start `∅`.
- **Propagate.** `AInit`/`AWrapAnyref`/`AUnwrapAnyref` copy the source's `prov`.
  Aggregates (`ARecord`/`AVariant`/`AArrayLit`) and fresh allocations
  (`Allocate` calls) produce `∅` (a fresh shell is not param-derived — nested
  field provenance is Phase 6). `AAssign` copies the RHS `prov`. A call result's
  `prov` follows its callee summary: `OwnedFromParam(k) → prov(arg_k)`,
  `OwnedFresh → ∅`, `Shared → ∅`.
- **Join.** Positional union of predecessor `prov`, mirroring the ownership join
  (params via edge args, live-through non-param locals by same id).
- **Classify each param `k`** from its local's two exit facts, in this
  precedence (each outcome is the conservative-safe reading of what the consumer
  will do with the arg):
  1. `valid[k] == false` (moved/invalidated on some path) → **`Consumed`** — the
     consumer marks its arg may-invalid, matching `binding_valid`'s may-moved
     meaning.
  2. else `own[k] == Shared` (published/aliased, still valid) → **`Retained`** —
     the consumer publishes its arg.
  3. else (untouched or read-only; a param enters `Unknown` and only a
     publish/move changes that) → **`Borrowed`** — the consumer leaves its arg
     unchanged.
- **Classify the return** in this precedence: `OwnedFromParam(k)` if the return
  atom's `prov` is non-empty (smallest such `k`, deterministic; the consumer
  treats any `OwnedFromParam` as an alias so the choice is correctness-neutral);
  else `OwnedFresh` if the return's `own == Unique`; else `Shared`.

### Consumption in `transfer_call`

For a **direct callee with a known summary** (replaces the blanket publish
bucket):

| Summary element | Effect at the call site |
|---|---|
| return `OwnedFresh` | result `← Unique` |
| return `OwnedFromParam(k)` | result aliases `arg_k` (both `Shared`, existing alias hinge) |
| return `Shared` | result `← Unknown` |
| param `Borrowed` | arg unchanged (**the key win — borrowed args stay `Unique`**) |
| param `Retained` | publish arg `→ Shared` |
| param `Consumed` | move arg (invalidate if last-use, like the `AInit` move) |

Builtins still resolve via `CallSemantics` (unchanged). **Unknown Twinkle call
without a summary, extern, indirect/closure callee, or `Cell` op → keep the
conservative publish-all-ref-args bucket.** The Phase 2 soundness guard still
holds and is extended with a summary-consumption direction.

## Precision items

- **Dead-merge param pruning (minimal).** After analysis, for each join/loop
  block, drop any carried param that is **not** in that block's `entry.live`
  (dead across the boundary) and remove the positionally-aligned atom from every
  predecessor edge and the matching terminator payload. Scope is deliberately
  narrow: only clearly-dead carried params, no general SSA cleanup. Observable as
  fewer params in `--cfg`; guarded by one fixture and the existing
  `count_edge_arity_mismatches` reciprocity check.
- **Match-arm pattern-binding precision (retires G3).** `build_match` records
  `anf_analysis.collect_pattern_bindings(arm.pattern)` on a new
  `CfgBlock.bound: Vector<Int>` (empty elsewhere). `scan_block_backward` kills
  `bound` at block entry; the forward pass seeds those locals `Unknown` (a
  destructured payload is a borrow from the scrutinee). The Phase 2 G3 fixture
  flips from "sound-but-imprecise" (leaked live-in) to precise.
- **Peephole decision (no code).** Record in `opt/README.md` that
  `dead_let`/`copy_prop`/`const_fold`/`branch_simp` stay ANF-local because they do
  not consult ownership or control-flow facts; only ownership-dependent work
  consumes the CFG facts.

## Rendering

`--cfg` gains a per-function header line rendering the function's summary, e.g.:

```
fn f [FuncId(295)]  summary: p0=borrow p1=retain ret=fresh
```

Deterministic: params in index order; `ret=from(p_k)` for `OwnedFromParam(k)`.
The improved block facts (borrowed-helper call sites now `Unique`) are the real
signal; the header is inspection sugar.

## Testing

TDD, mirroring Phase 2's `cfg_ownership_facts_suite`. Extend `module_of` to build
**multi-function** `AnfModule`s (several `AnfFunctionDef`s with a caller invoking
a callee via `ACall(AGlobalFunc(callee_id), …)`). One fixture per rule, each
cross-referenced to [summary-specialization.md](summary-specialization.md):

- **borrow** — callee reads a param and returns fresh ⇒ caller's arg stays
  `Unique`, call result `Unique`.
- **retain** — callee publishes a param (e.g. `global_set`) ⇒ caller's arg
  `Shared`.
- **consume** — callee consumes a param (`Update` on it, last-use) ⇒ caller's arg
  invalidated.
- **returns-alias** — callee returns a param ⇒ result aliases that arg.
- **recursion (SCC)** — a mutually-recursive pair: the fixpoint is stable and
  terminates, and the summary is the conservative-but-correct one.
- **dead-merge pruning** — a join with a provably-dead carried param loses it,
  edge arity stays consistent.
- **pattern-binding precision** — the G3 case: the bound local is killed at arm
  entry (not leaked live-in), still never `Unique`, no trap.
- **`--cfg` summary header render + determinism** (byte-identical across two
  builds).
- **soundness guard (extended)** — unknown/extern/indirect callees still publish;
  a summarized borrow does not.
- **real-program smoke** — analyze a linked program with a real helper without
  trapping.

## Non-goals

- No field-path / return-path summaries, transport wrappers, or field-sensitive
  record ownership (Phase 6).
- No ownership specialization or per-callee variants (Phase 6).
- No codegen, decision records, or in-place emission (Phase 4/5).
- No extern copying-borrow precision (Phase 8).
- No change to the surviving ANF-local peepholes beyond the decision record.

## Deferrals and tracking

The README Phase 3 bullets are delivered by this design except the two items that
are explicitly reframed/deferred:

| Item | Home |
|---|---|
| "Move ownership-relevant pass queries to CFG facts" | Satisfied vacuously (old consumers deleted; CFG facts already the single source) — recorded as the peephole decision, not code |
| Field-path / return-path / specialization | Phase 6 (unchanged) |
| Extern copying-borrow precision | Phase 8 (unchanged) |

The Phase 3 execution plan marks the delivered README bullets and notes the
vacuous-bullet reframing.

## Determinism-sensitive spots (lock with tests)

- **`prov` sets** — sorted `Vector<Int>`, same discipline as `live`; never a
  hash-set iteration in an order-sensitive path.
- **Summary fixpoint** — SCC order from Tarjan over a call graph whose edges are
  enumerated in deterministic ANF traversal order; the per-SCC meet is
  commutative/idempotent over the finite lattice, so iteration order does not
  affect the result.
- **`--cfg` header** — params in index order; `SummaryTable` lookups keyed by
  `FuncId.id`, rendered per function in block/function id order. Byte-identical
  across builds (gated by the determinism test).
