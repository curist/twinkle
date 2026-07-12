# Function Summaries and Ownership Specialization

**Status:** Draft subplan

The interprocedural layer of sound uniqueness: the **function summary** schema
(what a caller needs to know about a callee without re-analyzing it), how
summaries are computed, and how call sites select or generate an
**ownership-specialized variant** of a callee.

This builds on the fact lattice ([fact-lattice.md](fact-lattice.md)) and is
validated against the same anchors ([worked-examples.md](worked-examples.md),
especially Cases B ∩ C and V). Soundness rule unchanged: a call site takes the
mutable path only with a static proof; otherwise it uses the persistent callee.

## Why summaries

The transfer function in fact-lattice.md is intraprocedural. But every real update
in the boot compiler flows **through a call** — `add_type` (record threading),
`set_at` (sieve wrapper), `visit` (recursive). Without a summary, an `ACall` is a
hard publication boundary and nothing optimizes. The summary is the minimum a
caller needs to (a) propagate facts across the call and (b) decide specialization,
without inlining the callee.

## Summary schema

Sketched as the boot data model (reference-typed parameters only; scalars are
ownership-neutral):

```
type ParamRole = { Borrowed, Consumed, Published }

type ReturnOwn =
  { OwnedFresh              // return independent of params, freshly owned
  , OwnedFromParam(Int)     // return is the same region as param k (consume-produce)
  , Shared }                // return may be aliased/persistent

// [] = the record shell / whole value; [f] = field f's backing; deeper paths
// allowed but depth-capped and rare. Collections have only the [] path.
type AccessPath = Vector<FieldId>

type ParamSummary = .{
  base_role: ParamRole,             // whole-value escape status (Borrowed/Consumed/Published)
  in_place_paths: Vector<AccessPath>,  // paths whose ownership unlocks an in-place op inside the callee
  flows_to_return: Bool,
}

type FunctionSummary = .{
  func: FuncId,
  params: Vector<ParamSummary>,
  ret: ReturnOwn,
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
- field paths are **hierarchical under the shell**: `fields[f] = Owned`
  presupposes the record shell is owned (lattice), so `[f]` is only satisfiable
  when `[]` is, bounding the satisfiable combinations;
- non-record parameters carry only `[]`, so collections gain no complexity.

This also yields **graceful degradation** at the call site: owning the shell only
→ shell reuse with persistent field ops; owning shell + field → full in-place;
owning neither → generic.

Implementation may begin by emitting only the whole-value path `[]` (≈
whole-parameter) and refine to named field paths later; the schema is path-based
from the start, so no migration is needed.

**Parameter roles** (the callee's observable treatment of the argument value):

- **`Borrowed`** — only read; never consumed, never escapes, not returned.
  Ownership-neutral: the caller's fact for the argument is unchanged after the
  call.
- **`Consumed`** — the callee would mutate/move it (takes a mutable region on it
  if licensed). If the mutable variant is used, the caller's argument becomes
  `Moved` after the call.
- **`Published`** — the value escapes inside the callee (stored in an escaping
  aggregate, captured by a closure/task, sent on a channel, passed to an unknown
  call, written to a global). The caller's argument → `Shared` after the call
  regardless of variant. (An *indirect* call whose funcref target set is resolved
  by control-flow analysis borrows the meet of the possible targets' summaries
  instead of the blanket unknown-call publication — see
  [closure-capture.md](closure-capture.md).)

**`in_place_paths`** is populated only for a `Consumed` parameter that also
`flows_to_return` (the region is handed back out). A `Published` parameter has
**empty** `in_place_paths` — passing an owned value cannot help if the callee
leaks it — so it never participates in specialization.

**Worked summaries** (from the anchors; `in_place_paths` after the role):

| Callee | Param summary | Return |
|---|---|---|
| `set_at` wrapper | `p0: Consumed, paths {[]}`; `p1,p2: Borrowed` | `OwnedFromParam(0)` |
| `add_type` | `p0: Consumed, paths {[], [.types]}`; `p1,p2: Borrowed` | `OwnedFromParam(0)` |
| `visit` | `p0(State): Consumed, paths {[], [.indices], [.lowlinks], [.stack], [.on_stack], [.components]}`; `p1,p2: Borrowed` | `OwnedFromParam(0)` |

`add_type` names `[.types]` but **not** `[.values]`, so a caller with a shared
`.values` still specializes the `.types` update. `set_at`'s vector carries only
`[]`.

## Computing summaries

Summaries are the fixpoint of the intraprocedural analysis observed at parameters
and the return:

1. Run the fact-lattice transfer function over the callee body with each
   reference parameter entering as **`Unowned`** (the generic, conservative
   assumption).
2. Read off each parameter's `base_role` from how the body used it (consumed by a
   consuming op / record-update-in-place-candidate ⇒ `Consumed`; flowed to a
   publication sink ⇒ `Published`; only borrowed ⇒ `Borrowed`), and collect
   `in_place_paths` from the specific field paths (or `[]` for the whole value)
   the body mutates in place under the owned-entry assumption.
3. Classify the return by tracing the returned atom's fact back to a parameter or
   a fresh allocation.

**Order: bottom-up over call-graph SCCs.** The compiler already computes Tarjan
SCCs; process SCCs in reverse-topological order so a callee's summary exists
before its callers are analyzed.

**Recursion (within an SCC):** iterate the SCC's summaries to a fixpoint. Start
from the conservative bottom (every parameter `Borrowed`, not in-place-capable)
and **promote monotonically** — a parameter becomes `Consumed`/in-place-capable
only when every path (including recursive calls, using the current iteration's
summaries) supports it. Because promotion is monotone over the finite role
lattice and a partial result is always the conservative under-approximation, any
stopping point is sound; the fixpoint is the most precise sound answer. `visit`
resolves here: its own recursive call reuses the in-progress summary.

## Ownership specialization

A callee with in-place-capable parameters may need **two codegen shapes**: a
generic persistent variant and one or more mutable-specialized variants. The
choice is made **at the call site**, statically — no runtime uniqueness test.

### Variant identity

```
type OwnedReq = .{ param: Int, path: AccessPath }   // one (param, field-path) proven owned
type OwnedKey = Vector<OwnedReq>                     // canonical-sorted; [] ⇒ generic variant
type VariantId = .{ func: FuncId, owned: OwnedKey }
```

A variant is the callee re-analyzed with the keyed `(param, path)` slots entering
as `Owned` instead of `Unowned`. Re-running the transfer function under
that assumption is what turns the body's `record_update [in_place=false]` /
consuming calls into in-place operations at those paths and yields a specialized
summary (`OwnedFromParam(k)` becomes an owned mutable hand-off). This is ownership
monomorphization, structurally analogous to type monomorphization. Because field
paths are hierarchical under the shell, an `OwnedKey` is downward-closed: a
`(k, [.f])` req implies `(k, [])` is also present.

### Call-site decision

At `let L = call f(a0, a1, …)`, for each `(param k, path p)` in `f`'s
`in_place_paths`:

```
owned_here(k, p) = fact(a_k) at path p is Owned   // no live alias reaches that path (lattice)
                && last_use(a_k) at this site       // a_k dead after the call (a true move)
```

Both conditions are required. **`Owned` alone is not sufficient** — a uniquely
owned value that is still read after the call must not be mutated in place, or the
later read sees the write. `last_use` is the linear "consume" condition, supplied
by the same liveness the `AInit` hinge needs. The per-path `Owned` check reads the
argument's per-field fact directly from the lattice `Record{shell, fields}` shape.

- `key = sort({ (k, p) : (k, p) ∈ in_place_paths and owned_here(k, p) })`,
  downward-closed under the shell (drop a field req whose `(k, [])` is unmet).
- If `key` is non-empty and within the cap, select/emit `VariantId{f, key}`;
  otherwise select the generic variant. A partially-satisfied key (shell owned,
  one field shared) is fine — it yields the shell-reuse-only variant.
- **Post-call fact updates:** if any `(k, ·) ∈ key`, `a_k → Moved`; the result `L`
  takes the specialized return (`OwnedFromParam(k)` ⇒ `L` is owned, the region
  handed out). For the generic variant, `Borrowed` args are unchanged, `Consumed`
  args stay as they were (persistent update does not touch them), and `L` follows
  the generic `ReturnOwn`.

### Cases B ∩ C, concretely

- `build_env`: `call Fn295(L4,…)` — `L4` Owned incl. its fresh `.types`,
  reassigned right after (`assign L4 = L10`) so last-use holds ⇒
  `key={(0,[]), (0,[.types])}` ⇒ `add_type[owned:0,.types]` (shell + `.types`
  in-place). Two owned sites, same key, one variant.
- `branch_env`: `call Fn295(L7,…)` — `L7` is `Shared` (aliased by `L8`, read
  later) ⇒ no path owned ⇒ `key=[]` ⇒ generic persistent `add_type`.

So `add_type` materializes exactly two variants; the census shows most callees are
like `visit` (uniform owned callers ⇒ a single specialized variant, generic
possibly dead).

### Cap and explosion control

- Bound the number of `OwnedKey` variants per `(mono-instance, func)`. On
  overflow, fall back to the generic variant — always sound.
- The budget composes with type monomorphization: total = type clones ×
  ownership variants (architecture.md). The cap is on the ownership axis per
  type-instance.
- Read-only/`Borrowed` and `Published` parameters (and read-only sibling fields)
  never enter a key, which keeps keys small in practice.

### Determinism

- `OwnedKey` is a canonical-sorted list of `(param, path)` reqs (paths compared
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
summary add_type: p0=Consumed paths{[],[.types]}  p1=Borrowed  p2=Borrowed  -> OwnedFromParam(0)
call build_env#L10 -> add_type[owned:0,.types]  (a0=L4 Owned incl .types, last_use)
call branch_env#L10 -> add_type[generic]        (a0=L7 Shared: aliased by L8, read later)
```

Every specialized variant traces back to the argument facts that licensed it,
consistent with the Phase-1 "print facts before rewriting" discipline.

## Open questions

- Should a parameter distinguish `Consumed`-and-`Returned` from `Consumed`-and-
  `freeze-published` (returned indirectly, e.g. stored into an owned aggregate
  that is itself returned)? Both could be in-place-capable but with different
  post-call result facts.
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
  a summary that another SCC member's variant depended on.

## Non-goals

- No runtime variant dispatch or uniqueness test; the caller selects statically.
- No unbounded ownership monomorphization; the cap and conservative fallback are
  mandatory.
- No in-place licensing from `Owned` alone — `last_use` is required.
- No specialization keyed on `Borrowed` or `Published` parameters.
