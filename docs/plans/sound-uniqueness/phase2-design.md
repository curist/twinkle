# Phase 2 Implementation Design — Minimal Ownership Facts

**Status:** Draft design (feeds the Phase 2 execution plan)

This is the implementation design for **Phase 2** of the sound-uniqueness track:
populate the empty entry/exit fact maps that the [Phase 1 structural CFG
view](cfg-ownership-ir.md) reserved with a real `Unique`/`Shared`/`Unknown`
ownership dataflow, and surface it in `twk ir --cfg`. It ties the semantic
design in [fact-lattice.md](fact-lattice.md) and the view design in
[cfg-ownership-ir.md](cfg-ownership-ir.md) — validated against
[worked-examples.md](worked-examples.md) — to the shipped Phase 1 code
(`boot/compiler/cfg.tw`), fixing the concrete data model, module layout,
analysis pipeline, and locked design decisions.

It implements the first four unchecked Phase 2 bullets in
[README.md](README.md) ("first ownership domain", "per-predecessor join and
back-edge transfers", "conservative publication and aliasing", "loop-carried
ownership"). The fact-lattice/summary designs are the canonical semantics; on
any conflict those win and this doc is corrected.

## Scope

**In scope (Phase 2):**

- A three-element ownership domain `Unique`/`Shared`/`Unknown`, computed
  **intraprocedurally**, populating per-block entry/exit fact maps.
- Full backward **liveness** (the `AInit` move-vs-alias hinge and publication
  last-use depend on it).
- **Binding-validity** ("Moved") tracked as a separate map, never as a lattice
  element.
- Control-flow **join** and **loop back-edge fixpoint** transfers.
- Conservative **publication and aliasing** demotion to `Shared`.
- Rendering the facts by extending `twk ir --cfg`.

**Out of scope (deferred — see "Deferrals and tracking"):**

- **No function summaries.** Every non-builtin Twinkle call, every extern, and
  every `Cell` op is a conservative publication boundary. Summaries are Phase 3;
  specialization is Phase 6.
- **No record shell/field ownership, no transport wrappers, no variant-payload
  paths.** A fresh `ARecord`/`AVariant`/`AArrayLit` shell is `Unique`; nested
  field/element ownership is Phase 6.
- **No codegen decisions or candidate verdicts.** Phase 2 produces *facts*;
  decision records for existing hooks are Phase 4.
- **No extern copying-borrow precision.** Externs are treated as conservative
  publication in Phase 2; the borrow precision is Phase 8.

Guiding rule (from fact-lattice): **soundness before coverage.** Every default is
the conservative one; any op whose effect the analysis cannot prove drops the
fact to `Unknown`/`Shared`.

## Locked decisions

| # | Decision | Choice |
|---|---|---|
| 1 | Fact storage shape | A grouped `BlockFacts` record per block boundary (entry + exit), holding the three separate maps |
| 2 | Liveness precision | Full backward block liveness (`live_in`/`live_out`), with per-instruction last-use derived by an in-block backward scan |
| 3 | Fixpoint engine | Reverse-postorder (RPO) + worklist, deterministic |
| 4 | Fact surface | Extend `--cfg` (fill the reserved `facts.in`/`facts.out` slots); no separate flag |
| 5 | Fact keying | Per `LocalId` (not per value). Aliasing (`L8 = init L7`) is sound in the flat domain because it marks **both** ids `Shared` |

## Module layout

`boot/compiler/cfg.tw` stays the **structural view owner**; a new
`boot/compiler/ownership.tw` holds the **analysis**. `build_view` stays purely
structural, so the Phase 1 structural suite keeps passing on the un-analyzed
view.

- **`cfg.tw`** — owns the data types (`Ownership`, `BlockFacts`, `CfgBlock` with
  `entry`/`exit: BlockFacts`), the empty-facts constructor, and `render_view`
  (learns to print populated facts).
- **`ownership.tw`** (new) — `analyze(view: CfgView, builtins, semantics) ->
  CfgView`: runs the pipeline below and returns a view with populated facts.
  Also the structural query helpers the tests need.
- **CLI** — `twk ir --cfg` becomes `build_view → ownership.analyze → render_view`.

Because Twinkle values are immutable, `analyze` returns a new `CfgView` with
facts filled in rather than mutating in place.

### Data model

```tw
pub type Ownership = { Unique, Shared, Unknown }

pub type BlockFacts = .{
  ownership: Dict<Int, Ownership>,   // for locals live across this boundary (⊇ carried params)
  binding_valid: Dict<Int, Bool>,    // "Moved" as binding-validity, not a lattice element
  live: Vector<Int>,                 // liveness set at this boundary, sorted LocalId ids (deterministic)
}
```

`CfgBlock`'s Phase 1 `entry_facts`/`exit_facts: Dict<Int, String>` fields are
replaced by `entry: BlockFacts`, `exit: BlockFacts`, empty on an un-analyzed
view. Per-instruction last-use is **derived** during the forward pass from
`exit.live` plus a backward in-block scan — it is not stored as a fourth map.

The Phase 1 `empty_fact_blocks` query and its structural test are updated to the
new shape (all three maps empty ⇒ un-analyzed).

## Analysis pipeline

`analyze` runs, per function:

1. **Liveness (backward).** Compute `live_in`/`live_out` per block to fixpoint.
   Standard backward dataflow: `live_out(b) = ∪ live_in(successors)`;
   `live_in(b) = uses(b) ∪ (live_out(b) − defs(b))`. This is what the `AInit`
   hinge, the consuming-op hinge, and publication last-use consume.
2. **Ownership transfer (forward).** For each block, apply the per-`AnfOp`
   transfer over its instructions, producing `exit.ownership` from
   `entry.ownership` (transfer table below).
3. **Merge + fixpoint (RPO + worklist).** Join predecessor `exit.ownership` into
   successor `entry.ownership`; reprocess successors whose entry changed;
   iterate loop headers to fixpoint. Monotone over the three-element lattice ⇒
   terminates.
4. **Binding-validity.** A value moved by an `AInit` move (or consumed by an
   accepted consuming op) marks its source local invalid until rebound; threaded
   alongside ownership, never folded into it.

Liveness (1) is computed first because the ownership transfer (2) depends on it.

## Transfer semantics

### Transfer function (per `AnfOp`)

The transfer is an **exhaustive `case` over `AnfOp`** (like `op_text` in
`cfg.tw`), so it stays total as the enum evolves. `EffectKind`/`call_info` is
consulted **only inside `ACall`** to classify the callee — *not* as a universal
driver. This distinction matters: `op_effect` classifies `AMakeClosure` as
`Allocate` and `ARecordGet` as `Pure`, but `AMakeClosure` must **publish** its
captures and `ARecordGet` is a **borrow**, so every non-call variant gets a
direct rule from [fact-lattice.md](fact-lattice.md) rather than an
`EffectKind` mapping.

| `AnfOp` | Rule |
|---|---|
| `ACall(callee, args)` | classify `call_info(sem, callee)`: `Allocate` (constructor / builder freeze) → `L ← Unique`; `Update` (consuming: `dict.set`, `vector.append`, `Vector.set`) → consuming-op hinge on `p0`; `ReadOnly` → borrow (no change); `Pure`/`Control` (trap-like) → neutral; **`.None` (unknown Twinkle call, extern, `Cell` op) → publish every ref arg → `Shared`, `L ← Unknown`** |
| `ARecord` / `AVariant` / `AArrayLit` | `L ← Unique` (fresh shell); each ref field arg follows the move-vs-alias hinge (nested field/element ownership is Phase 6) |
| `ARecordGet` / `AIndex` | borrow — no ownership change |
| `ARecordUpdate(base, f, v, …)` | consuming-op hinge on the shell `base` (`Unique` + last-use → `L ← Unique`, `base` invalid; else `L ← Unknown`); `v` follows the field-store hinge. Field-sensitive backing is Phase 6 |
| `AInit(A)` | move-vs-alias hinge (below) |
| `AAssign(local, A)` | `ownership[local] ← fact(A)` (the loop-carried / branch-arm rebind transfer) |
| `AMakeClosure(_, captured)` | publish each captured local → `Shared` |
| `AGlobalSet(_, A)` | publish `A` → `Shared` |
| `AWrapAnyref(A, _)` / `AUnwrapAnyref(A, _)` | representationally transparent: follow the move-vs-alias hinge on `A` |
| `ABinOp` / `AUnOp` | neutral (scalar) |
| `ADefer(_)` | unreachable — hard error (defer-free contract, as in Phase 1) |

`fact(A)` is the ownership of an atom: `ownership[S]` if `A = ALocal(S)`,
otherwise `Unknown` (literals/globals/funcs are ownership-neutral scalars or
conservatively unknown).

The `.None` call bucket is the soundness anchor: with no summaries, every user
function, every extern, and every `Cell` op falls here and publishes. A
**soundness guard test** asserts `Cell`/extern ops actually resolve to `.None`
(an `OptimizerSemantics` that classified `Cell.set` as `Update` would be
unsound), so this is checked, not assumed.

### The two hinges (both consume Stage-1 liveness)

- **`AInit`** — `L = init A`:
  - `A = ALocal(S)` and `last_use(S here)` → **move**: `L ← ownership[S]`;
    `binding_valid[S] ← false`.
  - `A = ALocal(S)` and `S` still live → **alias**: `ownership[S] ← Shared`;
    `L ← Shared`.
  - `A` non-local (literal/global/func) → `L ← Unknown`.
- **Consuming op** — `L = op(p0, …)` with `EffectKind = Update`:
  - `ownership[p0] == Unique` and `last_use(p0 here)` → `L ← Unique`;
    `binding_valid[p0] ← false` (the eventual in-place path).
  - otherwise → `L ← Unknown`; `p0` ownership unchanged (the op reads `p0`; the
    persistent result may share backing, so it is not `Unique` — never conflate
    "new value" with "unique storage").

### Terminator publication

Beyond the per-op publications in the table (`AGlobalSet`, `AMakeClosure`, the
field-store hinge, and the `.None` call bucket), the block **terminators**
publish on their exit edges:

- `Return(A)` / value-carrying `Break(A)` publish `A`. This only demotes a value
  still observable elsewhere (an alias), which the `AInit` rule already caught,
  but it is the exit-edge fact that a downstream join/loop merge reads.
- `try` (an `AMatch` whose error arm ends in `Return`, per worked-examples
  Case T) is a multi-exit publication: the error arm publishes on its exit edge
  while the `Ok` arm keeps the value alive. Phase 1 already represents this as a
  diverging match arm, so no new structure is needed — Phase 2 just attaches the
  publish fact to that exit edge.

### Control flow falls out of block transfer + join

No separate forward-vs-rebound machinery is needed. A rebinding arm updates
`exit.ownership[L]` via `AAssign`; a forwarding arm leaves it as the carried-in
fact. So the join is simply:

```
entry.ownership[L] = ⊔ over predecessors of  pred.exit.ownership[L]
```

with `Unique ⊔ Unique = Unique`, `Unique ⊔ Shared = Shared`, and anything
`⊔ Unknown = Unknown` (a fact survives only if it holds on **every**
predecessor). Loop back-edges are the same join at the header, iterated to
fixpoint. This directly satisfies README Phase 2 tasks 3 (per-predecessor join
and back-edge transfers) and 5 (loop-carried ownership).

## Rendering

Extend `render_view` to fill the reserved slots, keyed by each block's carried
values (params), sorted by `LocalId` for determinism:

```
facts.in={L3: Unique, L9: Shared} facts.out={L3: Unique, L9: Shared}
```

The internal `ownership` map may hold more (all locals live across the
boundary); the printed surface is the block-param subset, matching the Phase 1
reserved shape. Binding-validity and liveness are computed but off the default
print for now.

## Testing

A new `boot/tests/suites/cfg_ownership_facts_suite.tw`, TDD, mirroring the
Phase 1 structural suite. Synthetic inline fixtures compiled via
`pipeline.compile_source → ownership.analyze`, one per rule, each
cross-referenced to its [worked-examples.md](worked-examples.md) case:

- **introduce** — `d := Dict.new()` ⇒ `d: Unique`.
- **move** — `a := Dict.new(); b := a` with `a` dead ⇒ `b: Unique` (Case B).
- **alias** — `a := Dict.new(); b := a`, both read later ⇒ both `Shared`
  (Case C — the linearity hinge).
- **publish** — `AGlobalSet`/return of a unique value ⇒ `Shared`.
- **branch join** — both arms allocate ⇒ join `Unique`; one arm publishes ⇒
  join `Shared` (Case V join).
- **loop-carried** — accumulator consumed-then-reassigned across the back-edge
  stays `Unique` (Case A).
- **determinism** — facts byte-identical across two builds.
- **soundness guard** — `Cell`/extern calls resolve to the `.None` publish
  bucket.

Registered in `boot/tests/main.tw`. The CLI flag is verified with
`make bundle-cli` and a byte-identical `twk ir --cfg` smoke check, as in
Phase 1.

**Fixture caveat (as in Phase 1).** The analysis runs on `artifacts.opt`, so the
optimizer's copy-propagation / dead-let elimination may already have removed the
very `AInit`/alias pattern a naive fixture intends to test (e.g. a bare
`b := a` copy-propagates away). Each fixture must be checked against
`twk ir <fixture> --opt` and crafted so the tested op **survives optimization** —
the alias fixture in particular needs both locals genuinely read later, exactly
the shape worked-examples Case C shows in real optimized ANF. Acceptance is tied
to the optimized-ANF shape, not the source syntax.

## Non-goals

- No function summaries, no interprocedural facts (Phase 3).
- No record shell/field ownership, transport wrappers, or variant-payload paths
  (Phase 6).
- No codegen decisions, candidate verdicts, or in-place emission (Phase 4/5).
- No extern copying-borrow precision (Phase 8).
- No `Moved` lattice element; no runtime uniqueness flags/refcounts/COW checks.
- No new mutable-region intrinsics (Phase 7).

## Deferrals and tracking

Every "later" in this design maps to an explicit home in
[README.md](README.md). The Phase 2 execution plan's final task makes these
edits, stated loud and clear:

| Deferred | README home |
|---|---|
| **Extern copying-borrow precision** (args preserved, GC result `Unique`, per the copying-marshalling contract) — Phase 2 treats externs as conservative publication | New **Phase 8** bullet + Future-work ledger row, stated bluntly |
| **Dead-merge block-param pruning** using the new liveness facts | Added to **Phase 3** ("move ownership-relevant pass queries to CFG facts") |
| Per-instruction candidate verdicts / codegen decision records | Already **Phase 4** — Phase 2 renders block-boundary ownership only |
| Binding-validity / liveness render surface in `--cfg` | Future-work ledger nicety |
| Record/field ownership, transport wrappers, summaries, specialization | Already **Phase 3 / 6 / 7** + Future-work ledger (unchanged) |

The five existing Phase 2 README bullets already match this design; the plan
marks them delivered and adds the two new tracking rows above.

## Determinism-sensitive spots (decided; the plan must lock with tests)

These are settled by this design; they are called out because a wrong choice
silently breaks self-host fixed-point stability, so the plan gates each with the
byte-identical determinism test:

- **`live` representation** — a sorted `Vector<Int>` of `LocalId` ids (decided in
  the data model). No hash-set iteration in any order-sensitive path.
- **Boundary `ownership` map contents** — stores every local live across the
  boundary (the transfer needs this); the render shows only the carried-param
  subset. Decided; confirm it stays cheap during the liveness slice.
- **RPO worklist seed order** — derived from optimized-ANF block-id traversal
  order, consistent with the Phase 1 determinism rule (block ids assigned by ANF
  traversal, never hash-map order).
