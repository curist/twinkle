# CFG Ownership View with SSA-Style Block Parameters

**Status:** Draft subplan

## Purpose

Define the early analysis foundation for sound uniqueness analysis and mutable
lowering.

The compiler does not currently have a reusable CFG or SSA IR. The sound
uniqueness project should add a deterministic CFG **view** over ANF early, with
SSA-style block parameters for values that cross branch joins and loop
back-edges.

ANF remains the authoritative program IR and the backend-facing structured spine.
The CFG exists to expose control-flow structure for analysis, not to replace ANF
or become a new codegen IR.

## Why a view, not an IR

ANF and SSA/CFG are near-duals (Kelsey's CPS↔SSA correspondence; Appel's "SSA is
functional programming"). They encode the same information, so maintaining both
as co-authoritative IRs — converted back and forth — is redundant and forces a
de-SSA/tree-reconstruction round trip. Two properties of Twinkle let us take the
analysis benefit without that cost:

- **Structured source, structured target.** Source control flow (`for`, `if`,
  `case`, `cond`) is structured, and Wasm GC control flow is structured
  (blocks/loops/`br`, no `goto`). ANF maps directly onto Wasm; a
  CFG-authoritative IR would need a relooper/stackification pass to recover
  structured control flow. Keeping ANF authoritative avoids that entirely.
- **Reducible, structure-preserving CFG.** Because the source is structured, the
  derived CFG is reducible and its loop headers and join points are already
  syntactically identifiable in ANF. The view adds explicit merge points for
  *facts*; it never restructures control flow.

So the view gives the analysis clarity of a CFG (explicit joins, back-edges,
merged facts at boundaries) while ANF stays the one source of truth for program
structure and codegen.

## Goal

Make ownership, binding validity, liveness, and publication analysis consume a
shared CFG view instead of each pass rediscovering control-flow facts by walking
the ANF tree independently. Ownership is the first consumer; later escape,
borrow, effects, and specialization analyses should reuse the same view.

The first version does not need full machine-style SSA for every temporary, but
it must make the hard control-flow cases structurally explicit:

- branch joins;
- loop back-edges;
- value-carrying `break` exits;
- loop-carried handles.

### First slice is structural only

The first committable slice is the **structural CFG view**: the control-flow
scaffolding above, block parameters for carried values, deterministic block ids
and pred/succ lists, terminators, per-block instruction→ANF mapping, the `--cfg`
printer, and **empty entry/exit fact maps** reserved for the next slice. It runs
no ownership analysis and changes no generated code — it is verifiable purely on
its own terms (graph shape + determinism), the way Phase 0's rails were.

The **populated** facts ride on top in the following slice
([Phase 2](README.md) / architecture §1A-facts):

- block-entry and block-exit `Unique`/`Shared`/`Unknown` facts;
- binding-validity and last-use facts separate from ownership;
- mutable candidates and rejection reasons for existing lowering hooks;
- codegen-ready decisions attached back to ANF nodes.

Later versions add record shell/field ownership, return-path transport facts,
call-site specialization facts, and richer mutable-region decisions.

## Position in the pipeline

Initial shape:

```text
monomorphized Core
  -> ANF lowering
  -> optimize (defer elimination first, then peephole passes) => artifacts.opt
  -> build CFG ownership view from the optimized, defer-free ANF (artifacts.opt)
  -> run analysis on CFG view
  -> attach decisions/facts to ANF nodes or side tables keyed by ANF ids
  -> existing ANF/prepared-IR/codegen path consumes those decisions mechanically
```

The view is built from `artifacts.opt`, not the freshly lowered ANF, so it reads
the same defer-free, codegen-bound form the census and backend consume (see "The
CFG input is defer-free"). A later optimizer-migration step (architecture §1B) may
revisit where the view is constructed; the structural slice builds on
`artifacts.opt`.

There is no CFG-to-ANF round trip in the initial design. The CFG view is derived,
queried, printed, and can be recomputed after ANF-changing passes. Rewrites and
mutable-region choices are represented as ANF-keyed annotations or side tables.

This avoids a de-SSA/tree-reconstruction problem and avoids needing a relooper or
stackification pass for Wasm's structured control flow. Because Twinkle source and
Wasm targets are structured, keeping ANF authoritative is an asset, not legacy
baggage.

## The CFG input is defer-free

The view is built from the **codegen-bound optimized ANF** (`artifacts.opt`).
`eliminate_defers` is the first optimizer pass (`opt/pipeline.tw`), so `ADefer`
never survives into `artifacts.opt`: a `defer { … }` body is already inlined onto
every scope-exit path (fall-through, `break`, `return`, `try` early-return) as
ordinary lets/ops. That flattened form is exactly the faithful shape for ownership
analysis — a read in the cleanup body appears as a real use on each exit edge — so
the CFG needs **no** special `ADefer` modeling.

The builder therefore treats defer-free as a **contract**: encountering an
`ADefer` node is a hard error, not something to inline on the fly. Inlining it
during CFG construction would silently misrepresent defer's exit-path duplication
semantics; failing loud instead catches any future pipeline change that stops
eliminating defer before this stage.

## SSA-style block parameters

Use block parameters for values that merge at control-flow boundaries. Ownership
facts are separate lattice maps keyed by those values at block entry/exit.

Do **not** encode ownership facts themselves as block parameters.

**Only carried values get block parameters — not every ANF local.** Full value
SSA is a non-goal; the view names exactly the values that cross a boundary, and
those are identifiable from ANF *syntax* alone (no liveness pass, so this stays in
the structural slice). The key syntactic fact is that **every `AAssign` target is
a pre-existing local being rebound** (a fresh binding lowers to `Let`, only a
rebind lowers to `AAssign`), so an `AAssign` inside a branch arm or loop body is,
by construction, a value whose post-boundary version depends on control flow — a
merged/carried value:

- **loop back-edge carried values** — the `AAssign` targets in an `ALoop` body
  (accumulators *and* compiler-introduced iterator state), carried across the
  back-edge;
- **branch-merged values** — the `AAssign` targets inside `AIf` / `AMatch` **arms**,
  merged at the branch join. These are distinct from the branch *result* below: in
  `if c { x = 10 } else { x = 20 }; x`, the `AIf` result binds `Void`, but `x` is
  rebound in both arms and merges at the join, so `x` — not the result — is the
  carried value;
- **branch/loop result values** — the `Let`-bound result local of an `AIf` /
  `AMatch` / `ALoop` op (the value of the `if`/`case`/`loop` *expression* itself,
  e.g. `m := if c { a } else { b }`);
- **value-carrying break** — the `Break(Atom?)` payload leaving a loop.

An `AAssign` target gets a block parameter at each enclosing join/back-edge it sits
under (an arm rebind nested in a loop merges at the branch join, then carries the
back-edge). A local that is defined and consumed within a single block stays an
ordinary straight-line `Let`; it is never lifted to a block parameter.

This rule is deliberately conservative: it includes an `AAssign` target even if
that value happens to be dead after the join (a harmless extra parameter). Pruning
such dead merges is an optional refinement for the Phase 2 liveness facts, not the
structural slice — keeping the structural rule purely syntactic.

Examples:

```text
block loop_header(flags_value):
  facts.in[flags_value] = Unique
  binding.valid[flags_value] = true
  ...
  br loop_header(flags_after_update)

block join(env_value):
  facts.in[env_value] = Unique
  ...
```

The block parameter names the carried value. The fact map records what ownership
state is known for that value at the block boundary. Printed IR should make both
visible.

## Required data model

**Phase 1 — structural data (the structural slice builds all of this):**

- function id/name and original ANF mapping;
- deterministic block ids;
- block parameters for carried/merged values (loop and branch-arm `AAssign`
  targets, branch/loop result bindings, break payloads);
- instructions mapped back to ANF lets/ops where possible;
- terminators: branch, conditional branch, switch/match, loop back-edge, return,
  value-carrying break, void break, and continue-equivalent edges if still
  present;
- predecessor/successor lists in deterministic order;
- **empty** entry/exit fact maps, keyed by block parameter, reserved for Phase 2.

**Phase 2+ — reserved facts and decisions (populated by later slices, not the
structural view):**

- ownership facts at block entry and exit (`Unique`/`Shared`/`Unknown` first);
- binding-validity and last-use facts separate from ownership;
- per-instruction borrow/publication/update facts;
- candidate mutable update verdicts;
- selected existing-hook decisions for codegen, attached to ANF nodes or an
  ANF-keyed side table;
- later: record shell/field-sensitive ownership facts, return-path ownership facts
  for transport wrappers, nested collection ownership facts, and mutable-region
  intrinsic decisions.

## Value-carrying break

`break` can carry a value out of a loop. For ownership analysis, that is not only
a terminator; it can be a publication/region-exit edge.

If a loop-carried unique handle exits through `break value`, the analysis must
classify what happens to that value:

- returned/published persistent value;
- demoted to `Shared` on the exit edge;
- rejected because an old version remains observable.

Future mutable-region lowering may add explicit freeze/handle states, but the
first ownership view should not require them.

The printed CFG ownership view should show value-carrying break edges and their
ownership effect explicitly.

## Codegen contract

The CFG ownership view must produce enough ANF-keyed information for codegen to
be a machinery pass. Codegen should not re-prove uniqueness, rediscover field
ownership, or repeat escape analysis. It should consume explicit, checked
decisions from the analysis.

This section defines the decision-record *schema* (which fields a decision
carries). The *soundness* half — the invariants that let codegen consume a
decision without re-checking it, and the fail-safe rule that absence/ambiguity/
staleness of a decision falls back to the persistent op — lives in
[mutable-intrinsics.md](mutable-intrinsics.md) "Analysis → codegen handoff
contract". The two are a pair: this doc says *what* is handed over, that doc says
*what it guarantees*.

For each accepted first-cut update, the side table/annotation should identify:

- the source value;
- the operation family: vector, dict, builder, or record shell;
- the required `Unique` fact and last-use/binding-validity proof;
- reads/borrows that remain non-escaping;
- the existing in-place or builder lowering to use;
- fallback persistent operation if the decision is absent or rejected;
- proof/debug id linking the codegen decision to the printed analysis facts.

Later mutable-region decisions can add begin/thaw, freeze/publish, nested
projection, and specialization-cap fields.

For records, codegen-relevant facts must distinguish:

- record shell reuse allowed/not allowed;
- field ownership known/unknown/shared;
- field projection that transfers or borrows an owned collection;
- record update that preserves a shared field and therefore cannot imply deep
  ownership;
- wrapper records such as `Set<K>` projecting to owned `Dict<K, Void>` when
  proven sound.

This keeps the backend simple: emit the existing helper/builder path selected by
the analysis, or emit the ordinary persistent operation. Later, the selected path
may be a mutable intrinsic sequence. Codegen should not make new soundness
decisions.

## Optimizer pass migration

Current optimizer passes should migrate toward consuming the CFG view or shared
facts derived from it. The point is to avoid parallel, inconsistent notions of
control flow, ownership, and codegen legality.

Early migration order:

1. Build CFG view and print it without changing generated code.
2. Run ownership/liveness/publication analysis on the CFG view.
3. Attach facts and codegen decisions to ANF nodes or deterministic side tables.
4. Move or wrap existing dead-let/copy-prop/const-fold/branch-simp decisions so
   they use CFG-derived use/liveness/control-flow facts where relevant.
5. Emit the same ANF as before until the analysis is trusted.
6. Enable existing in-place/builder lowering from ANF-keyed decisions; migrate to
   mutable-intrinsic lowering later if needed.

Not every peephole pass must be rewritten on day one. Pure local simplifications
can remain ANF-based temporarily, but the source of truth for ownership,
liveness, branch joins, and loop back-edges should be the CFG view once this
subplan lands.

## View staleness and recomputation

Because the CFG is derived from ANF, any pass that changes ANF control flow or
binding structure must either invalidate and recompute the view or update the
mapping in a checked way.

This is a safer failure mode than making CFG authoritative: stale analysis views
can be detected and recomputed, while an incorrect CFG-to-ANF reconstruction
would risk changing program structure.

## Determinism requirements

CFG construction must be deterministic:

- block ids assigned by source/ANF order, not hash-map order;
- successor/predecessor lists sorted by construction order;
- block parameters printed and emitted in stable order;
- side-table keys assigned in ANF/source order;
- specialization decisions derived from deterministic worklists.

This is required for self-host fixed-point stability.

## `twk ir` output

Add an IR/debug mode that can print the CFG ownership view, for example:

```bash
target/twk ir file.tw --cfg
target/twk ir file.tw --ownership
```

The exact flag names can change. The **structural slice's `--cfg`** prints only
the structural rows (block graph, carried block parameters, terminators including
value-carrying break edges, per-block ANF mapping) with the fact maps shown empty;
the fact/candidate/decision rows below are populated by the Phase 2+ slices.

Full output (across slices) should show:

- block graph;
- block parameters as carried values;
- ownership facts at entry/exit as separate maps;
- record shell and field ownership facts;
- return-path ownership and field/payload-projection move/borrow facts for
  transport wrappers;
- loop-carried facts;
- value-carrying break edges and their publication effect (freeze is a later
  mutable-region detail);
- publication sinks;
- candidate mutable updates;
- accepted codegen decisions and their proof/debug ids;
- rejection reasons;
- call-site specialization choices.

## Non-goals for the first CFG view

- Do not get rid of ANF.
- Do not make CFG the authoritative codegen IR.
- No CFG-to-ANF de-SSA/tree reconstruction pass.
- No relooper/stackification pass for Wasm codegen.
- No full value SSA conversion unless the ownership analysis later proves it is
  needed.
- No register allocation or machine-oriented optimization IR.
- No codegen rewrite in the first step.
- No runtime uniqueness checks.

## Resolved

- **Block-parameter scope:** only *carried* values, identified from ANF syntax —
  not every local. Carried = every `AAssign` target (loop-body rebinds *and*
  branch-arm rebinds, since a rebind implies a pre-existing local whose merged
  value depends on control flow), plus `AIf`/`AMatch`/`ALoop` result bindings and
  `Break` payloads. See "SSA-style block parameters" above.
- **`defer`:** does not survive to CFG construction; the view builds on the
  defer-free `artifacts.opt` and asserts it. See "The CFG input is defer-free".
- **Phase boundary:** the first slice is the structural view only; populated
  `Unique`/`Shared`/`Unknown` facts are the following slice. See "First slice is
  structural only".

## Open questions (later slices)

- What is the exact representation for field-sensitive record ownership facts?
  (Phase 2+ facts.)
- Should accepted mutable codegen decisions be represented as annotations on
  original ANF instructions, a side table keyed by proof/debug id, or both?
  (Phase 5 codegen handoff.)
- Which existing peephole passes should migrate first? (Optimizer migration.)

## Relationship to main architecture

[architecture.md](architecture.md) leaves open whether the ownership optimizer
needs a CFG/SSA-like IR before codegen. This subplan answers that question: the
optimizer gets explicit control-flow structure as a **derived view over ANF**,
not as a competing source of truth. ANF stays authoritative, decisions are
ANF-keyed annotations, and the Wasm backend keeps its direct
structured-control-flow mapping. The architecture doc's "may need a CFG IR" hedge
should be read as decided in favor of this derived-view model.
