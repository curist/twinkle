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
any genuine conflict those win and this doc is corrected. The one deliberate
exception is a **sanctioned conservative deviation**: where Phase 2 chooses a
*more* conservative (strictly sound, less precise) treatment than the canonical
rule and tracks the precision to a later phase, that is not a conflict — it is
staging. The externs row (below) is the sole such deviation, and it is
reconciled with a forward-pointer in [fact-lattice.md](fact-lattice.md) so the
two docs agree.

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
  field/element ownership is Phase 4; transport wrappers and variant-payload paths
  are Phase 5.
- **No codegen decisions or candidate verdicts.** Phase 2 produces *facts*;
  decision records for existing hooks are Phase 7.
- **No extern copying-borrow precision.** Externs are treated as conservative
  publication in Phase 2 (**sanctioned deviation**, see above): sound because
  over-publishing only loses optimization, never correctness. The canonical
  borrow/`Unique`-result precision from [fact-lattice.md](fact-lattice.md):135,
  [sound-analysis.md](sound-analysis.md):200, and
  [concurrency-publication.md](concurrency-publication.md):64-100 is the Phase 10
  target, and fact-lattice's extern row carries a forward-pointer noting Phase 2
  over-approximates it.

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
  (learns to print populated facts). It also carries the **structural inputs the
  analysis needs but Phase 1 discarded** (see [Structural inputs](#structural-inputs-phase-1-must-preserve)).
- **`ownership.tw`** (new) — `analyze(view: CfgView, builtins, semantics) ->
  CfgView`: runs the pipeline below and returns a view with populated facts.
  Also the structural query helpers the tests need. The analysis is
  **view-driven**: it reads the raw `AnfOp` and function parameters off the view
  (not rendered text), so `analyze` needs no separate `AnfModule` argument.
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

### Structural inputs Phase 1 must preserve

The transfer function and liveness both operate on the **raw `AnfOp` and its
operand atoms** — which locals a node defines and uses, whether a call arg is a
ref local, which locals a closure captures, which atom a field stores. Phase 1
threw all of that away: `CfgInstruction` keeps only the rendered `text`
(`cfg.tw:15`), the raw op is dropped right after `op_text(op)` (`cfg.tw:616`),
and `CfgFunction` (`cfg.tw:42`) omits the function's own parameters. Recovering
any of it by parsing debug text is not acceptable. Phase 2 therefore extends the
Phase 1 structural types:

```tw
pub type CfgInstruction = .{ anf_local: LocalId, op: AnfOp, text: String }
//                                               ^^^^^^^^^^  raw op drives transfer + liveness

pub type CfgFunction = .{
  func_id: Int,
  name: String,
  params: Vector<LocalId>,   // the function's own parameters: live + binding_valid at entry
  blocks: Vector<CfgBlock>,
}
```

`build_view`/`build_function` keep the raw `op` on each pushed instruction and
record `func.params`. This stays purely structural (`op_text` still renders the
same string), so the Phase 1 structural suite is unaffected apart from the new
fields being populated.

**Edge args must carry the real transferred atoms (phi arguments).** This is the
other thing Phase 1 discarded: the fall-through tail atom (`FallThrough.tail`,
`cfg.tw:53`) is computed but dropped when wiring joins (`cfg.tw:401-405`, `457`,
`471`, `513-517`), and break payloads survive as `ValueBreak(atom)` but the
loop-exit edge args are still placeholders (`cfg.tw:537-542`, `562`, `588`). So
`edge_args_for(params)` returns the target params verbatim — an **arity
placeholder, not a value transfer**. Ownership of a join's *result* local can
never be inferred from block transfer + join alone, because the result local is
not a value any arm computes; each arm computes its own tail atom. Phase 2
therefore makes edge args real, positionally aligned to the target block's
`params`:

```tw
pub type CfgEdge = .{ target: BlockId, args: Vector<Atom> }
//                                           ^^^^^^^^^^^^  was Vector<LocalId>: now the
//                                           actual atom each predecessor feeds into params[i]
```

For every edge into a join / loop-exit / loop-header, the builder records, per
target param `params[i]`, the atom that predecessor supplies:

- **result param ← arm tail atom** (`if`/`match` join): the falling arm's
  `ft.tail` (may be a literal/global, hence `Atom` not `LocalId`).
- **loop-result param ← break payload** on a `ValueBreak(atom)` → exit edge.
- **carried local ← its value on that path**: `ALocal(L)` for both the rebinding
  arm (the reassigned `L`) and the forwarding arm (the unchanged `L`).

Non-local transferred atoms (literal/global tails) are ownership-neutral, so
their contribution to the join is `Unknown` via `fact` below — exactly the
desired result for `if c { 1 } else { 2 }`. Arity checks (`Vector<Atom>` length
vs param count) and rendering (`atom_text`) carry over unchanged.

**One authoritative edge-arg representation.** Phase 1 stores arg vectors in
*two* places: the per-block `succs: Vector<CfgEdge>` **and** the terminator
payloads (`Branch`, `CondBranch`, `LoopBackEdge`, `Match` via its edges —
`cfg.tw:19-27`), both `Vector<LocalId>` today and kept in sync by `finish_block`.
To avoid two divergent representations once args carry real atoms, the terminator
payloads also become `Vector<Atom>`, and **`succs` is the single authoritative
source the analysis reads** (it already is for arity — see
`count_edge_arity_mismatches`'s note). The terminator copy exists only for
`terminator_text` rendering and must stay identical to the matching `succs` edge;
`render`/`local_list_text` switch to `atom_text`. The analysis never reads edge
args off a terminator.

**Extern needs no extra metadata.** A conservative Phase 2 does *not* require
extern-import or global type tables: an extern (host import) callee is not in
`OptimizerSemantics.call_semantics`, so `call_info` returns `.None` and the op
falls into the publish bucket automatically — the same path as any unsummarized
Twinkle call. The Phase 10 extern-borrow precision (out of scope here) is what
would need the import allow-list and result-type metadata.

**No type table either — the conservative ref rule.** The publication and
field-store rules say "publish every **ref** arg" / "each **ref** field arg",
which strictly needs a local→type oracle (`Param.ty` in `core_ir.tw:98`, plus
`op_result_mono` in `anf.tw`). Phase 2 deliberately does **not** carry that
table, and `CfgFunction.params` stays `Vector<LocalId>` (ids only). Instead it
adopts the conservative rule: **treat every `ALocal` operand as a possible ref**
and apply the publish/demote to all of them; non-local atoms (literals, globals,
funcrefs) are ownership-neutral and untouched. This is sound because demotion is
the conservative direction — marking a scalar local `Shared` is harmless (scalars
carry no ownership and are never in-place candidates), and it never demotes a
genuinely-`Unique` ref that a positive case depends on (Case B/V dicts/records
are consumed by `Update` calls and returned once, not published). The only cost
is precision noise: a scalar local may render as `Shared`. Carrying real types
(`Vector<Param>` + a local-type oracle from `op_result_mono`) is the precision
upgrade, deferred until a consumer must distinguish scalar locals — noted, not
built.

## Analysis pipeline

`analyze` runs, per function:

1. **Liveness (backward, edge-arg aware).** Compute `live_in`/`live_out` per
   block to fixpoint. Because the CFG is SSA-ish — a successor names carried
   values by its own `params`, and each predecessor supplies them through
   `edge.args` — the naive `live_out(b) = ∪ succ.live_in` is **wrong**: a join
   param `r` is not live in the predecessor; the *edge atom feeding `r`* is. So
   liveness must translate successor params back through the edge:

   ```text
   live_out_on_edge(pred → succ) =
     (succ.live_in − succ.params)                       // values live past the join, named directly
     ∪ { locals(edge.args[i]) | succ.params[i] ∈ succ.live_in }   // params translated to the fed atom
   live_out(pred) = ∪ over succ edges of  live_out_on_edge(pred → succ)
   live_in(b)     = uses(b) ∪ (live_out(b) − defs(b))
   ```

   `locals(A)` is `{S}` for `A = ALocal(S)` and `∅` for non-locals. This is what
   the `AInit` hinge, the field-store hinge, the consuming-op hinge, and
   publication last-use consume — e.g. an arm falling through with
   `ft.tail = ALocal(x)` into join param `r` keeps **`x`** (not `r`) live on that
   edge, so `x`'s last-use at the arm tail is computed correctly.
2. **Ownership transfer (forward).** For each block, apply the per-`AnfOp`
   transfer over its instructions, producing `exit.ownership` from
   `entry.ownership` (transfer table below).
3. **Merge + fixpoint (RPO + worklist).** Join predecessor `exit.ownership` into
   successor `entry.ownership`; reprocess successors whose entry changed;
   iterate loop headers to fixpoint. Monotone over the three-element lattice ⇒
   terminates.
4. **Binding-validity.** A value moved by an `AInit` move (or consumed by an
   accepted consuming op) marks its source local invalid until rebound; threaded
   alongside ownership in the same forward pass, never folded into it. Full
   default/transfer/join rules in [Binding-validity](#binding-validity-semantics)
   below.

Liveness (1) is computed first because the ownership transfer (2) depends on it.

## Transfer semantics

### Transfer function (per `AnfOp`)

The transfer runs over each block's **non-structural instructions** — the raw
`op` now carried on `CfgInstruction`. It is an **exhaustive `case` over `AnfOp`**
(like `op_text` in `cfg.tw`), so it stays total as the enum evolves.
`EffectKind`/`call_info` is consulted **only inside `ACall`** to classify the
callee — *not* as a universal driver. This distinction matters: `op_effect`
classifies `AMakeClosure` as `Allocate` and `ARecordGet` as `Pure`, but
`AMakeClosure` must **publish** its captures and `ARecordGet` is a **borrow**, so
every non-call variant gets a direct rule from
[fact-lattice.md](fact-lattice.md) rather than an `EffectKind` mapping.

**Structural ops are unreachable, not omitted.** `AIf`, `AMatch`, and `ALoop`
(`anf.tw`) never appear as `CfgInstruction`s — Phase 1 lowers them to block
structure in `build_if`/`build_match`/`build_loop` (`cfg.tw:599-614`) before any
instruction is pushed. So, exactly like the defer-free `ADefer` contract, the
transfer's arms for `AIf`/`AMatch`/`ALoop` are **hard errors** ("structural op
reached ownership transfer; expected block-lowered CFG"), keeping the `case`
total without inventing ownership rules for ops that can't occur here. Their
control-flow effect is realized by the join + fixpoint (below), not by a
per-instruction rule.

| `AnfOp` | Rule |
|---|---|
| `ACall(callee, args)` | extract the `FuncId` from `callee` (`AGlobalFunc(fid)`) and classify `call_info(sem, fid)`: `Allocate` (constructor / builder freeze) → `L ← Unique`; `Update` (consuming: `dict.set`, `vector.append`, `Vector.set`) → consuming-op hinge on the **base arg named by `CallSemantics.cow_base_arg`** (not hardcoded arg 0); `ReadOnly` → borrow (no change); `Pure`/`Control` (trap-like) → neutral; **`.None` (unknown Twinkle call, extern, `Cell` op) → publish every ref arg → `Shared`, `L ← Unknown`**. An **indirect callee** (`callee = ALocal(_)`, a closure/funcref call) has no `FuncId` and thus no summary → it takes the `.None` publish bucket |
| `ARecord` / `AVariant` / `AArrayLit` | `L ← Unique` (fresh shell); each ref field arg follows the **field-store hinge** (nested field/element ownership is Phase 4) |
| `ARecordGet` / `AIndex` | borrow — no ownership change |
| `ARecordUpdate(base, f, v, …)` | consuming-op hinge on the shell `base` (`Unique` + last-use → `L ← Unique`, `base` invalid; else `L ← Unknown`); `v` follows the field-store hinge. Field-sensitive backing is Phase 4 |
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
**soundness guard test** covers both directions of misclassification, so the
classification is checked, not assumed:

- `Cell`/extern ops must resolve to `.None` (an `OptimizerSemantics` that
  classified `Cell.set` as `Update` would wrongly license in-place — unsound).
- `dict.set` / `vector.append` / `Vector.set` must resolve to `.Update` **with a
  well-formed `cow_base_arg`** (were one to drift to `.Allocate`, the consuming
  hinge would mint a bogus `Unique` result — the unsound direction).

### The three hinges (all consume Stage-1 liveness)

- **`AInit`** — `L = init A`:
  - `A = ALocal(S)` and `last_use(S here)` → **move**: `L ← ownership[S]`;
    `binding_valid[S] ← false`.
  - `A = ALocal(S)` and `S` still live → **alias**: `ownership[S] ← Shared`;
    `L ← Shared`.
  - `A` non-local (literal/global/func) → `L ← Unknown`.
- **Consuming op** — `L = op(…)` with `EffectKind = Update`, where `b` is the
  base arg named by `CallSemantics.cow_base_arg` (the reusable collection/shell,
  not assumed to be arg 0):
  - `ownership[b] == Unique` and `last_use(b here)` → `L ← Unique`;
    `binding_valid[b] ← false` (the eventual in-place path).
  - otherwise → `L ← Unknown`; `b` ownership unchanged (the op reads `b`; the
    persistent result may share backing, so it is not `Unique` — never conflate
    "new value" with "unique storage").
- **Field-store hinge** — a local `A` stored into a fresh/updated aggregate (an
  `ARecord`/`AVariant`/`AArrayLit` field, or the value slot `v` of an
  `ARecordUpdate`). Phase 2 does not track nested field ownership (Phase 4), so
  the store is treated exactly like the `AInit` operand rule applied to `A`,
  never as ownership entering a tracked field:
  - `A = ALocal(S)` and `last_use(S here)` → **move**: `binding_valid[S] ←
    false` (S is consumed into the shell; the shell fact is set by its own row).
  - `A = ALocal(S)` and `S` still live → **alias**: `ownership[S] ← Shared`
    (the value is now reachable through the shell as well).
  - `A` non-local → neutral (scalar/global).

  This is the conservative first cut: the shell itself is `Unique` (fresh), but
  the analysis makes no `Unique` claim about the stored field's contents until
  field precision arrives (Phase 4).

### Binding-validity semantics

`binding_valid: Dict<Int, Bool>` is a separate forward fact, threaded in the
same forward pass as ownership, joined at the same points — never a lattice
element.

- **Default.** A local is valid the moment it is defined (any defining
  instruction) and function parameters are valid at the entry block. A local
  with no entry is treated as valid (absent ⇒ valid); only an explicit `false`
  marks it moved-out.
- **Transfer.** The `AInit` move, the consuming-op move, and the field-store
  move each set their consumed source's `binding_valid ← false`. `AAssign(local,
  A)` **rebinds**, so it sets `binding_valid[local] ← true` again (a moved local
  becomes usable once reassigned — the loop-carried `assign Lc = Ln` pattern).
- **Join.** Conservative **meet**, and — like ownership — **positional over edge
  args**: `binding_valid[params[i]]` at a block entry is `true` iff, for **every**
  predecessor, that predecessor's `edge.args[i]` is non-local *or* its source
  local is valid on that predecessor's exit. So a result param fed by an arm tail
  `ALocal(x)` is valid only where `x` is valid on every incoming arm. If any
  predecessor moved the fed local and another forwarded it live, the merge is
  invalid — a value that *might* have been moved cannot be used. This is the dual
  of the ownership join, and it is why moved-on-one-arm / forwarded-on-the-other
  branches resolve to "not usable" rather than silently keeping the value.

Phase 2 computes and joins `binding_valid` but keeps it off the default `--cfg`
print (see Rendering).

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

With real edge args (above), no separate forward-vs-rebound machinery is needed:
the rebinding-vs-forwarding distinction is already encoded in *which atom* a
predecessor supplies for each param, so the join is **positional over the edge
args**. For target block param `params[i]`:

```
entry.ownership[params[i]] = ⊔ over predecessors pred of  fact(pred, pred_edge.args[i])
```

where `fact(pred, A)` is `pred.exit.ownership[S]` when `A = ALocal(S)` and
`Unknown` otherwise (literal/global/funcref tails are neutral). The result local
gets its fact from each arm's tail atom; a carried local gets it from that path's
`ALocal(L)` (reassigned or forwarded, same id either way). The lattice join is
`Unique ⊔ Unique = Unique`, `Unique ⊔ Shared = Shared`, and anything
`⊔ Unknown = Unknown` (a fact survives only if it holds on **every**
predecessor). `binding_valid` merges by the meet in parallel (see
[Binding-validity](#binding-validity-semantics)). Loop back-edges are the same
positional join at the header, iterated to fixpoint. This directly satisfies
README Phase 2 tasks 3 (per-predecessor join and back-edge transfers) and 5
(loop-carried ownership).

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
- **soundness guard** (both directions) — `Cell`/extern calls resolve to the
  `.None` publish bucket, **and** `dict.set`/`vector.append`/`Vector.set` resolve
  to `.Update` with a well-formed `cow_base_arg`.

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
  (Phases 4-5).
- No codegen decisions, candidate verdicts, or in-place emission (Phases 7-8).
- No extern copying-borrow precision (Phase 10).
- No `Moved` lattice element; no runtime uniqueness flags/refcounts/COW checks.
- No new mutable-region intrinsics (Phase 9).

## Deferrals and tracking

Every "later" in this design maps to an explicit home in the reorganized track
READMEs. This table records where the Phase 2 deferrals now live after the
three-track split:

| Deferred | Tracking home |
|---|---|
| **Extern copying-borrow precision** (args preserved, GC result `Unique`, per the copying-marshalling contract) — Phase 2 treats externs as conservative publication (sanctioned deviation) | [../migration/README.md](../migration/README.md) Phase 10, plus the staging note in [fact-lattice.md](fact-lattice.md)'s extern row |
| **Dead-merge block-param pruning** using the new liveness facts | [README.md](README.md) Phase 3 |
| Per-instruction candidate verdicts / codegen decision records | [../codegen/README.md](../codegen/README.md) Phase 7 — Phase 2 renders block-boundary ownership only |
| Binding-validity / liveness render surface in `--cfg` | Analysis-track nicety; facts are computed, but not rendered by default |
| Record/field ownership, transport wrappers, summaries, specialization | Later analysis precision; see [records-fields.md](records-fields.md) and [summary-specialization.md](summary-specialization.md) |

## Determinism-sensitive spots (decided; the plan must lock with tests)

These are settled by this design; they are called out because a wrong choice
silently breaks self-host fixed-point stability, so the plan gates each with the
byte-identical determinism test:

- **`live` representation** — a sorted `Vector<Int>` of `LocalId` ids (decided in
  the data model). No hash-set iteration in any order-sensitive path. The sorted
  insert/union reuse `cfg.tw`'s existing `sorted_insert_local` / `sorted_union`
  (they are `Vector<LocalId>`-typed; either thread `LocalId` end-to-end or add a
  one-line `Int` twin — decided as an implementation detail, not a new algorithm).
- **`ownership` / `binding_valid` map iteration** — never drives an
  order-sensitive result. The ownership join is commutative/idempotent over the
  three-element lattice and the binding-validity join is a commutative meet, so a
  fold over either `Dict`'s entries is order-independent; Twinkle's `Dict` is
  insertion-order-deterministic regardless. Any *rendered* or compared output is
  keyed by a sorted `LocalId` list, not by raw `Dict` iteration.
- **Boundary `ownership` map contents** — stores every local live across the
  boundary (the transfer needs this); the render shows only the carried-param
  subset. Decided; confirm it stays cheap during the liveness slice.
- **RPO worklist seed order** — derived from optimized-ANF block-id traversal
  order, consistent with the Phase 1 determinism rule (block ids assigned by ANF
  traversal, never hash-map order).
