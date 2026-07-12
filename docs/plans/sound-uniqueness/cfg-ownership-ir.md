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

Make ownership, liveness, publication, and specialization analysis consume a
shared CFG view instead of each pass rediscovering control-flow facts by walking
the ANF tree independently.

The first version does not need full machine-style SSA for every temporary, but
it must make the hard ownership cases explicit:

- branch joins;
- loop back-edges;
- value-carrying `break` exits;
- loop-carried owned handles;
- block-entry and block-exit ownership facts;
- record shell and field ownership facts;
- call-site specialization facts;
- mutable-region candidates and rejection reasons;
- codegen-ready mutable-region decisions attached back to ANF nodes.

## Position in the pipeline

Initial shape:

```text
monomorphized Core
  -> ANF lowering
  -> build CFG ownership view from ANF
  -> run analysis on CFG view
  -> attach decisions/facts to ANF nodes or side tables keyed by ANF ids
  -> existing ANF/prepared-IR/codegen path consumes those decisions mechanically
```

There is no CFG-to-ANF round trip in the initial design. The CFG view is derived,
queried, printed, and can be recomputed after ANF-changing passes. Rewrites and
mutable-region choices are represented as ANF-keyed annotations or side tables.

This avoids a de-SSA/tree-reconstruction problem and avoids needing a relooper or
stackification pass for Wasm's structured control flow. Because Twinkle source and
Wasm targets are structured, keeping ANF authoritative is an asset, not legacy
baggage.

## SSA-style block parameters

Use block parameters for values that merge at control-flow boundaries. Ownership
facts are separate lattice maps keyed by those values at block entry/exit.

Do **not** encode ownership facts themselves as block parameters.

Examples:

```text
block loop_header(flags_value):
  facts.in[flags_value] = OwnedMutable(Vector<Bool>)
  ...
  br loop_header(flags_after_update)

block join(env_value):
  facts.in[env_value] = ShallowRecordOwned(...)
  ...
```

The block parameter names the carried value. The fact map records what ownership
state is known for that value at the block boundary. Printed IR should make both
visible.

## Required data model

The CFG view should model:

- function id/name and original ANF mapping;
- deterministic block ids;
- block parameters for carried/merged values;
- instructions mapped back to ANF lets/ops where possible;
- terminators: branch, conditional branch, switch/match, loop back-edge, return,
  value-carrying break, void break, and continue-equivalent edges if still
  present;
- predecessor/successor lists in deterministic order;
- ownership facts at block entry and exit;
- record shell ownership facts;
- field-sensitive ownership facts for record fields, including projected fields;
- nested collection ownership facts for element/value projections when modeled;
- per-instruction borrow/publication/update facts;
- candidate mutable update verdicts;
- selected mutable-region operations for codegen, attached to ANF nodes or an
  ANF-keyed side table.

## Value-carrying break

`break` can carry a value out of a loop. For ownership analysis, that is not only
a terminator; it can be a publication/region-exit edge.

If a loop-carried owned handle exits through `break value`, the analysis must
classify what happens to that value:

- returned/published persistent value;
- frozen mutable-region result;
- rejected because an old version remains observable;
- rejected because the break publishes a still-mutable handle.

The printed CFG ownership view should show value-carrying break edges and their
ownership effect explicitly.

## Codegen contract

The CFG ownership view must produce enough ANF-keyed information for codegen to
be a machinery pass. Codegen should not re-prove uniqueness, rediscover field
ownership, or repeat escape analysis. It should consume explicit, checked
decisions from the analysis.

For each accepted mutable region or update, the side table/annotation should
identify:

- the source persistent value or owned mutable handle;
- the operation family: vector, dict, record shell, or nested projection;
- begin/thaw point if needed;
- reads/borrows that remain inside the region;
- writes/updates/removes/appends;
- freeze/publish point;
- fallback persistent operation if the region is rejected or a specialization cap
  routes the caller to the generic variant;
- proof/debug id linking the codegen decision to the printed analysis facts.

For records, codegen-relevant facts must distinguish:

- record shell reuse allowed/not allowed;
- field ownership known/unknown/shared;
- field projection that transfers or borrows an owned collection;
- record update that preserves a shared field and therefore cannot imply deep
  ownership;
- wrapper records such as `Set<K>` projecting to owned `Dict<K, Void>` when
  proven sound.

This keeps the backend simple: emit the mutable intrinsic sequence selected by
the analysis, or emit the ordinary persistent operation. It should not make new
soundness decisions.

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
6. Enable mutable-intrinsic lowering from ANF-keyed decisions.

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

The exact flag names can change, but the output should show:

- block graph;
- block parameters as carried values;
- ownership facts at entry/exit as separate maps;
- record shell and field ownership facts;
- loop-carried facts;
- value-carrying break edges and their publication/freeze effect;
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

## Open questions

- Should all ANF locals become block-parameter-capable values, or only locals
  whose ownership facts cross block boundaries?
- What is the exact representation for field-sensitive record ownership facts?
- Should accepted mutable codegen decisions be represented as annotations on
  original ANF instructions, a side table keyed by proof/debug id, or both?
- Which existing peephole passes should migrate first?
- How should `defer` be represented if it survives to CFG view construction?

## Relationship to main architecture

[architecture.md](architecture.md) leaves open whether the ownership optimizer
needs a CFG/SSA-like IR before codegen. This subplan answers that question: the
optimizer gets explicit control-flow structure as a **derived view over ANF**,
not as a competing source of truth. ANF stays authoritative, decisions are
ANF-keyed annotations, and the Wasm backend keeps its direct
structured-control-flow mapping. The architecture doc's "may need a CFG IR" hedge
should be read as decided in favor of this derived-view model.
