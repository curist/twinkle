# Function Summaries and Ownership Specialization

**Status:** Draft subplan

The interprocedural layer of sound uniqueness: the **function summary** schema
(what a caller needs to know about a callee without re-analyzing it), how
summaries are computed, and — in later phases — how call sites select or generate
an **ownership-specialized variant** of a callee.

This builds on the fact lattice ([fact-lattice.md](fact-lattice.md)) and is
validated against the same anchors ([worked-examples.md](worked-examples.md),
especially Cases B ∩ C and V). Soundness rule unchanged: a call site takes the
mutable path only with a static proof; otherwise it uses the persistent callee.

## Why summaries

The transfer function in fact-lattice.md is intraprocedural. But many real update
patterns in the boot compiler flow through calls — `set_at` wrappers, context
helpers, `add_type`, and eventually recursive helpers such as `visit`. Without a
summary, an `ACall` is a hard publication boundary and optimization coverage stays
local.

The first summary implementation should be minimal. Its job is to keep known
helpers from looking like unknown publication boundaries, not to solve the full
record-threading problem immediately.

## First implementation summary subset

Start with reference-typed parameters only; scalars are ownership-neutral.

For each parameter, summarize only:

- **consumes parameter** — the callee logically consumes/replaces the argument;
- **retains parameter** — the callee stores, captures, publishes, or otherwise
  keeps the argument observable after the call.

For the return, summarize only:

- **returns fresh value** — the result is independent and can be `Unique`;
- **returns alias** — the result may alias a parameter or published value.

This subset is enough to make CFG ownership a shared primitive and to drive the
existing local in-place/builder decisions more cleanly. It deliberately delays
access paths, transported return fields, ownership-specialized variants, and
multi-variant caps until the core analysis is stable.

## Full target summary schema

The fuller schema below is the target for later path precision and ownership
specialization. It should not be required for the first implementation.

Sketched as the boot data model (reference-typed parameters only; scalars are
ownership-neutral). In this full target schema, `OwnedFresh` and `OwnedFromParam`
mean path-refined unique ownership; the first executable ownership fact remains
`Unique`:

```
type ParamRole = { Borrowed, Consumed, Published }

type ReturnOwn =
  { OwnedFresh              // return independent of params, freshly owned
  , OwnedFromParam(Int)     // return is the same region as param k (consume-produce)
  , Shared }                // return may be aliased/persistent

// [] = the record shell / whole value; [f] = field f's backing; deeper paths
// allowed but depth-capped and rare. Collections have only the [] path.
type AccessPath = Vector<FieldId>

// Return ownership is also path-sensitive. `ret_path=[]` describes the whole
// returned value; `[.ctx]` / `[.state]` describes a field inside a returned
// record; `Ok[0].state` / `Err[0].state` describes a field inside a variant
// payload record. Parameter in-place paths remain field-only `AccessPath`s;
// return paths additionally need variant/payload segments.
type ReturnPath = Vector<ReturnPathSegment>
type ReturnPathSegment = { Field(FieldId), VariantTag(Int), Payload(Int) }
type ReturnPathOwn = .{ ret_path: ReturnPath, own: ReturnOwn }

type ParamSummary = .{
  base_role: ParamRole,             // whole-value escape status (Borrowed/Consumed/Published)
  in_place_paths: Vector<AccessPath>,  // paths whose ownership unlocks an in-place op inside the callee
  flows_to_return: Bool,
}

type FunctionSummary = .{
  func: FuncId,
  params: Vector<ParamSummary>,
  returns: Vector<ReturnPathOwn>,
}
```

### Consumption is per access path (resolved)

Consumption is tracked at **field-path granularity**, not per whole parameter,
because the shell-vs-field split the fact lattice already requires (two in-place
decisions per record quartet) is itself path granularity — a single whole-param
flag cannot express "reuse the shell" separately from "mutate the `.types`
backing." Whole-parameter ownership is *sound but over-restrictive*: it would
reject the sound `.types` in-place update whenever an unrelated sibling field
(e.g. `.values`) is shared-in, which is exactly the mixed-ownership shape real
threaded context records take (shared parent env + fresh scope state).

`in_place_paths` therefore lists the exact paths the callee mutates in place if
the caller can prove them owned. Properties that keep this bounded:

- paths are precisely the fields the callee updates (small, static); read-only and
  sibling fields never appear, so keys stay *smaller* than a whole-record vector;
- field paths are **hierarchical under the shell**: a unique field fact
  presupposes the record shell is unique, so `[f]` is only satisfiable when `[]`
  is, bounding the satisfiable combinations;
- non-record parameters carry only `[]`, so collections gain no complexity.

This also yields **graceful degradation** at the call site: owning the shell only
→ shell reuse with persistent field ops; owning shell + field → full in-place;
owning neither → generic.

Phase 6 Part 1 temporarily emitted only the whole-value path `[]`; Phase 6 Part 2
must populate direct field paths such as `[.types]` for the worked examples. The
executable Phase 6 cap is `[]` plus direct record fields `[f]`. The `AccessPath` /
`ParamPath` shape leaves room for future deeper field chains, but unsupported deeper
parameter paths and `Elem`/`Val`/payload segments do not create `UniqueReq`
requirements; a direct ancestor appears only when that ancestor is itself an
independently supported mutation site.

### Transport-wrapper returns are first-class

The actual boot compiler does not only return threaded state directly. A large
fraction of helper calls return a small product record whose one or more fields
are updated accumulators: `ctx` (`SynthOut`, `CheckOut`, `ExprOut`, `LocalOut`,
`FuncIdOut`, `RewriteResult`), `state` (`FreshResult`, `ExprAccumResult`,
`SourceLoad`, `Discovery`, `SingletonResult`), `env` (`ResolveResult`,
`ImportEnvResult`), or pairs such as `Walk.{ b, env }` and ANF lowering's
`.{ state, accum }`. The caller shape is usually:

```tw
out := helper(ctx_or_state, ...)
ctx_or_state = out.ctx       // or out.state / out.env / out.b / out.accum
// read out.ty / out.expr / out.local / out.diags / other siblings as needed
```

A whole-return summary cannot express this. The callee's return is a fresh record
shell, but the ownership handoff is at a returned field path:

```text
helper: p0 Consumed paths{...} -> returns { []: OwnedFresh,
                                           [.ctx]: OwnedFromParam(0) }
```

Multiple returned fields may independently carry ownership, e.g. ANF lowering can
thread both `state` and `accum` through `.{ state, accum, expr }` wrappers. The
caller must then use the field-projection hinge from
[fact-lattice.md](fact-lattice.md): `out.ctx`/`out.state` transfers that field's
ownership when that path is not read or published through `out` afterward.
Sibling reads such as `out.ty` or `out.diags` do not by themselves block the
handoff. Publishing `out`, returning `out`, storing it in another aggregate, or
reading the same transported field again after the move does block it.

This is eventual required coverage for the boot compiler, but it is not required
for the first minimal summary implementation. Without return-path ownership and
path-sensitive projection, the analysis treats `.{ ctx, ... }` / `.{ state, ... }`
as escaping aggregates and fails to recover the dominant threaded-context and
threaded-state patterns used by checker, lowering, resolver, query analysis, and
boundary-rewrite helpers.

### Variant-wrapped transport returns

Query analysis commonly wraps transport records in `Result`: `load_source`
returns `SourceLoad!AnalysisError`, `parse_cached` returns `ParseLoad!AnalysisError`,
and callers handle both `.Ok(v)` and `.Err(err)` while continuing with
`v.state`/`err.state`. This is not the same as `try` early-return publication when
the error is handled locally; the state ownership flows through a variant payload
and then through a record field.

Return paths therefore need variant/payload segments, not only record fields:

```text
load_source: p0(state) Consumed -> returns {
  Ok[0].state:  OwnedFromParam(0),
  Err[0].state: OwnedFromParam(0),
}
```

At the caller, each `case` arm projects the state from the variant payload. The
ordinary branch/match join then merges the transported state facts: if every arm
continues with a unique state, the joined state remains unique; if an arm publishes
or aliases it, the join demotes it. `try` remains a publication edge only for the
arm that actually returns from the enclosing function.

**Parameter roles** (the callee's observable treatment of the argument value):

- **`Borrowed`** — only read; never consumed, never escapes, not returned.
  Ownership-neutral: the caller's fact for the argument is unchanged after the
  call.
- **`Consumed`** — the callee would mutate/move it (takes an in-place path if
  licensed). If the mutable variant is used, the caller's argument binding becomes
  invalid after the call; this is binding-validity state, not an ownership fact.
- **`Published`** — the value escapes inside the callee (stored in an escaping
  aggregate, captured by a closure/task, sent on a channel, passed to an unknown
  call, written to a global). The caller's argument → `Shared` after the call
  regardless of variant. (An *indirect* call whose funcref target set is resolved
  by control-flow analysis borrows the meet of the possible targets' summaries
  instead of the blanket unknown-call publication — see
  [closure-capture.md](closure-capture.md).)

**`in_place_paths`** is populated only for a `Consumed` parameter that also
`flows_to_return` (the region is handed back out). A `Published` parameter has
**empty** `in_place_paths` — passing a unique value cannot help if the callee
leaks it — so it never participates in specialization. Returning or transporting a
parameter is not itself retention: it is represented by `flows_to_return` and
path-keyed return ownership. True publication means the callee creates an escaping
or retained observation, such as a global/escaping aggregate/`Cell`, escaping
closure/task/channel, or unknown Twinkle callee.

**Worked summaries** (from the anchors; `in_place_paths` after the role):

| Callee | Param summary | Return |
|---|---|---|
| `set_at` wrapper | `p0: Consumed, paths {[]}`; `p1,p2: Borrowed` | `[]: OwnedFromParam(0)` |
| `add_type` | `p0: Consumed, paths {[], [.types]}`; `p1,p2: Borrowed` | `[]: OwnedFromParam(0)` |
| `visit` | `p0(State): Consumed, paths {[], [.indices], [.lowlinks], [.stack], [.on_stack], [.components]}`; `p1,p2: Borrowed` | `[]: OwnedFromParam(0)` |
| `fresh_meta` / `alloc_local`-style helper | `p0(ctx/state): Consumed, paths {[] plus any updated reference fields}`; other params borrowed | `[]: OwnedFresh record`; `[.ctx]`/`[.state]: OwnedFromParam(0)` |
| `load_source` / `parse_cached`-style Result helper | `p0(state): Consumed, paths {[] plus updated fields}`; other params borrowed | `Ok[0].state: OwnedFromParam(0)`, `Err[0].state: OwnedFromParam(0)` |

`add_type` names `[.types]` but **not** `[.values]`, so a caller with a shared
`.values` still specializes the `.types` update. `set_at`'s vector carries only
`[]`.

## Computing summaries

Summaries are the fixpoint of the intraprocedural analysis observed at parameters
and the return:

1. Run the generic fact-lattice transfer function over the callee body with each
   reference parameter entering as **`Unknown`**. This classifies the conservative
   caller-visible behavior: borrowed, retained/published, fresh return, or alias
   return.
2. Separately collect candidate `in_place_paths` from the callee's effect shape:
   consuming collection updates, record-update candidates, and helper calls that
   would be legal if the corresponding parameter/path entered as `Unique`. This is
   a syntactic/effect summary of requirements, not proof that the generic call is
   in-place-capable.
3. In later specialization phases, re-run or refine the transfer under explicit
   `Unique` entry assumptions for those candidate paths. Only that owned-entry
   pass can turn a candidate path into an accepted specialized in-place decision.
4. Classify the return by tracing the returned atom's fact back to a parameter or
   a fresh allocation. For returned records, classify both the whole return
   (`[]`) and unique reference-typed fields such as `[.ctx]`/`[.state]`; for
   returned variants, classify payload paths such as `Ok[0].state` and
   `Err[0].state`, so transport-wrapper helpers preserve the state handoff.

**Order: bottom-up over call-graph SCCs.** The compiler already computes Tarjan
SCCs; process SCCs in reverse-topological order so a callee's summary exists
before its callers are analyzed.

**Recursion (within an SCC):** iterate the SCC's summaries to a fixpoint, keeping
caller-visible safety facts separate from optimization capabilities. Escape and
return facts start at the conservative caller view (reference parameters may be
`Published`; returns may be aliases/`Shared`). In-place capability starts empty.
Each iteration may refine safety facts downward only when every path proves the
more precise behavior, and may add an `in_place_path` only when every relevant
recursive path supports it under the current summaries. If iteration is cut short,
expose the conservative safety facts and no newly speculative in-place paths; only
the fixpoint is used for codegen. `visit` resolves here: its own recursive call
reuses the in-progress summary without assuming a mutable variant exists before it
is proven.

## Ownership specialization

After the minimal summary layer is stable, a callee with in-place-capable
parameters may need **two codegen shapes**: a generic persistent variant and one
or more mutable-specialized variants. The choice is made **at the call site**,
statically — no runtime uniqueness test. This is a later precision/scaling step,
not the first ownership-analysis milestone.

### Variant identity

```
type UniqueReq = .{ param: Int, path: AccessPath }  // one (param, field-path) proven unique
type UniqueKey = Vector<UniqueReq>                   // canonical-sorted; [] ⇒ generic variant
type VariantId = .{ func: FuncId, unique: UniqueKey }
```

A variant is the callee re-analyzed with the keyed `(param, path)` slots entering
as `Unique` instead of `Unknown`. Re-running the transfer function under
that assumption is what turns the body's `record_update [in_place=false]` /
consuming calls into in-place operations at those paths and yields a specialized
summary (`OwnedFromParam(k)` becomes a unique mutable hand-off). This is ownership
monomorphization, structurally analogous to type monomorphization. Because field
paths are hierarchical under the shell, a `UniqueKey` is downward-closed: a
`(k, [.f])` req implies `(k, [])` is also present.

### Call-site decision

At `let L = call f(a0, a1, …)`, for each `(param k, path p)` in `f`'s
`in_place_paths`:

Selection is a **key-level fixed point**, not an independent per-path test. It is
also a logical-version observability proof: the pre-update source value may be
physically reused only when that old logical version has no observable continuation
except producing the post-update value.

```
candidate_key = downward_close({ (k, p) ∈ in_place_paths : fact(a_k) at p is Unique })
selected_key  = greatest subset of candidate_key with consume_dead(a_k, selected_key) true
                (drop any path whose region is observed later, re-close downward,
                 repeat until stable; else generic if no non-empty key survives)
```

**`Unique` alone is not sufficient** — a region still observable after the call must
not be mutated in place, or the later read sees the write. `consume_dead(a, key)` is
**key-level** (obligations interact) and `[]` denotes **different storage** on a
record vs a collection:

- **consumed record shell `[]`** = the record's **shell storage** (its field-pointer
  slots), **not** the whole logical binding: no future **whole-record** use or
  publish of `a` or any alias to that shell, and no future read/publish of the fields
  this variant updates; **disjoint sibling projections are allowed** (`a.values` after
  a `.types`-only update reads the correct untouched pointer).
- **consumed collection `[]`** = the whole backing region: no future use or publish
  of that collection through `a` or any alias.
- **consumed field path `[.f]`** = field `f`'s region: no future read or publish of
  it through `a` or aliases.

A partially-invalidated record binding may afterwards serve **only** as a carrier for
statically-proven disjoint sibling projections; any whole-value use drops it. This
`[]` split is what lets `add_type[unique:0,.types]` (downward-closed to `{[],
[.types]}`) coexist with a later `.values` read — treating record `[]` as the whole
binding would forbid it and leave the mixed-ownership target contradictory.
Whole-binding last-use is only the collection-`[]` / whole-record-use case. The
per-path uniqueness check reads the argument's per-field fact directly from the later
`Record{shell, fields}` shape.

For collection `[]` and collection-valued fields, `consume_dead` also requires alias
completeness: every live alias to the selected backing must be represented by the
existing provenance/path facts and checked for later observation. Missing,
multi-origin, or unknown alias facts force generic fallback. This is deliberately a
conservative gate over `prov`/`path_prov`/`field_own`, not a new points-to analysis.

- `key = sort(selected_key)` from the fixed point above: the greatest
  downward-closed subset of the `Unique` candidate paths for which the key-level
  `consume_dead` holds (drop a field req whose `(k, [])` is unmet; drop a
  later-observed path and re-close until stable).
- If `key` is non-empty and within the cap, select/emit `VariantId{f, key}`;
  otherwise select the generic variant. A partially-satisfied key (shell unique,
  one field shared) is fine — it yields the shell-reuse-only variant.
- **Post-call fact updates:** if any `(k, ·) ∈ key`, only `a_k`'s **consumed** paths
  are invalidated (partial invalidation per `consume_dead`) — disjoint untouched
  sibling paths of `a_k` stay readable; the result `L` takes the specialized return
  facts path-by-path. Consumed-path facts join by union at branches/back-edges and are
  cleared by normal all-edge rebinding to a fresh post-call value. A `[]`
  return path with `OwnedFromParam(k)` means `L` is unique as a whole; a `[.ctx]` or
  `Ok[0].state` return path with `OwnedFromParam(k)` means that projected path
  owns the handed-off region until projected or published. For the generic
  variant, `Borrowed` args are unchanged, `Consumed` args stay as they were
  (persistent update does not touch them), and `L` follows the generic return-path
  facts.

### Cases B ∩ C, concretely

- `build_env`: `call Fn295(L4,…)` — `L4` is `Unique`, and later field precision
  proves its fresh `.types` path unique; it is reassigned right after (`assign L4
  = L10`) so last-use holds ⇒ `key={(0,[]), (0,[.types])}` ⇒
  `add_type[unique:0,.types]` (shell + `.types` in-place). Two unique sites, same
  key, one variant.
- `branch_env`: `call Fn295(L7,…)` — `L7` is `Shared` (aliased by `L8`, read
  later) ⇒ no path unique ⇒ `key=[]` ⇒ generic persistent `add_type`.

So `add_type` materializes exactly two variants; the census shows most callees are
like `visit` (uniform unique callers ⇒ a single specialized variant, generic
possibly dead).

### Cap and explosion control

- Bound the number of `UniqueKey` variants per `(mono-instance, func)`. On
  overflow, fall back to the generic variant — always sound.
- The budget composes with type monomorphization: total = type clones ×
  ownership variants (architecture.md). The cap is on the ownership axis per
  type-instance.
- Read-only/`Borrowed` and `Published` parameters (and read-only sibling fields)
  never enter a key, which keeps keys small in practice.

### Determinism

- `UniqueKey` is a canonical-sorted list of `(param, path)` reqs (paths compared
  field-id-wise), so equal keys dedup.
- Call sites are visited in ANF/source order; variants are created via a
  deterministic worklist; `VariantId`s are assigned in creation order.
- Recursive calls within an SCC that match an in-progress variant's key reuse it
  (tie the knot) rather than spawning new variants — bounding recursion-driven
  growth and keeping self-host stable.

## IR / debug (`twk ir`)

Ownership output should print, per function, the summary; and per call site, the
selected variant and why:

```
summary add_type: p0=Consumed paths{[],[.types]}  p1=Borrowed  p2=Borrowed  -> []=OwnedFromParam(0)
summary fresh_meta: p0=Consumed paths{[]} -> []=OwnedFresh, [.ctx]=OwnedFromParam(0)
call build_env#L10 -> add_type[unique:0,.types]  (a0=L4 Unique incl .types, last_use)
call checker#L42 -> fresh_meta[unique:0]          (result.ctx moves to cur_ctx; sibling ty read)
call branch_env#L10 -> add_type[generic]        (a0=L7 Shared: aliased by L8, read later)
```

Every specialized variant traces back to the argument facts that licensed it,
consistent with the Phase-1 "print facts before rewriting" discipline.

## Open questions

- Should a parameter distinguish `Consumed`-and-`Returned` from `Consumed`-and-
  `freeze-published` when the value is returned through a non-record container?
  Record transport wrappers are resolved by return-path ownership such as
  `[.ctx] = OwnedFromParam(k)`.
- Path depth: nested field paths (`[.a, .b]`) are permitted but depth-capped —
  what cap, and does any real boot code need depth > 1? (Resolved that
  consumption is per-path, not whole-parameter; see "Consumption is per access
  path" above.)
- Variant cap value and the fallback-selection policy when the cap binds at a hot
  site.
- Should specialized variants be represented as cloned ANF functions, or as the
  same function plus an entry-ownership annotation consumed by codegen (mirrors
  the annotation-vs-clone question in cfg-ownership-ir.md)?
- Interaction with the SCC summary fixpoint when a variant's re-analysis changes
  a summary that another SCC member's variant depended on. **Resolved** in
  [phase6-design.md](phase6-design.md) D12: the `VariantId → Summary` memo is an
  **ascending iterated cell** — it starts at the generic (empty-capability) bottom and
  a within-SCC recursive call reads the **previous iteration's approximant**, growing
  the in-place capability only when every recursive path supports it. The cell is
  dirty-queued and re-run until `same_summary` stabilizes; if a **new** demanded key
  would exceed the variant cap, that call site routes to generic and no cell is
  created. Existing converging cells are never stripped on cap hit. (Unlike Phase 5's
  retract-able `ret_paths`, the capability lattice only ascends, so the
  previous-approximant schedule never exposes a fact a later round revokes.)

## Non-goals

- No runtime variant dispatch or uniqueness test; the caller selects statically.
- No unbounded ownership monomorphization; the cap and conservative fallback are
  mandatory.
- No in-place licensing from `Unique` alone — the path-aware `consume_dead`
  condition is required (whole-binding `last_use` is only its shell-consuming case).
- No specialization keyed on `Borrowed` or `Published` parameters.
