# Record Shell Reuse, Field Ownership, and Nested Collections

**Status:** Expanded for Phase 4 (analysis facts). The intraprocedural
shell/field/nested model is specified in
[phase4-design.md](phase4-design.md); return-path summaries are Phase 5,
parameter-side `in_place_paths` / ownership specialization are Phase 6, and codegen
lowering is Phases 7–8.

## Purpose

Collects the record and nested-collection ownership story that is currently
scattered across [fact-lattice.md](fact-lattice.md) (the record quartet, the
`Record{shell, fields}` lattice shape), [summary-specialization.md](summary-specialization.md)
(field-path-granular `in_place_paths`), and [sound-analysis.md](sound-analysis.md)
(the record/nested coverage matrices). This doc is the single place the
**shell-vs-deep split** is reasoned about end to end.

Expands the `Record shell reuse and field ownership` and `Nested collection
ownership` sections of [architecture.md](../architecture.md).

## The core principle: fresh shell ≠ deep ownership

A freshly allocated outer aggregate is **not** proof that the reference-typed
contents reachable through it are owned. This applies identically to records and
to nested collections:

- a fresh record shell may wrap **shared** collection fields;
- a `Vector<Vector<T>>` may have an owned outer vector over **shared** inner
  vectors;
- a `Dict<K, Vector<V>>` may have an owned HAMT over **shared** value vectors;
- variants and array literals wrap shared collection values in fresh shells too.

Mutating the outer structure needs only outer-backing ownership. Mutating an
inner collection reached through an element/value/field projection needs a
**separate** proof for that inner backing.

## Two independent decisions per record update

For the dominant record quartet (`record_get .f` → consuming call →
`record_update .f` → `assign`; see [worked-examples.md](worked-examples.md)
Cases B/V), there are two orthogonal in-place questions:

1. **Shell reuse** — can the record shell itself be updated in place? (needs the
   shell to be `Unique` and at last use.)
2. **Field-backing mutation** — can the collection stored in / projected from
   field `.f` be treated as deeply owned and mutated? (needs `.f` deeply owned +
   no alias on the old `.f`.)

They must stay separate. Sibling fields untouched by the update need not be
owned — field sensitivity is the point (a shared `.values` must not block a
sound `.types` in-place update).

**The shell/field lattice interaction (intraprocedural).** The shell fact `[]`
lives in the flat `own` domain (`Unique`/`Shared`/`Unknown`); the deeper paths
live in a separate additive `field_own` map where a **present** path means
`Unique` and an **absent** path means "no claim" (never assume ownership of a
missing path). The relationship is **downward-closed**:

- a `[.f]` fact is meaningful only while `[]` is `Unique`; a `[.f, Elem]` fact
  presupposes `[.f]`;
- so whenever the shell `[]` leaves `Unique` (publish, alias, or a join that
  lowers it), **every** `[.f]*` fact for that local is cleared (the demotion
  cascade).

Field-sensitivity is the payoff: sibling paths are independent, so `[.types]`
can be `Unique` while `[.values]` carries no claim — a shared sibling never
blocks a sound in-place on another field. Full transfer/join rules:
[phase4-design.md](phase4-design.md).

The **interprocedural** `(param, path)` key — a summary's `in_place_paths` and the
`UniqueReq` specialization key in [summary-specialization.md](summary-specialization.md)
— is the Phase 5/6 lift of exactly this downward-closed invariant: a `(k, [.f])`
requirement implies `(k, [])`. Phase 4 establishes the intraprocedural facts that
the later summary/specialization layers project outward.

## Transport-wrapper records (`.{ ..., ctx/state/env }`)

Boot's dominant context-threading helper shape is a small result record whose one
or more fields are updated accumulators: `ctx` (`SynthOut`, `CheckOut`,
`ExprOut`, `LocalOut`, `FuncIdOut`, `RewriteResult`), `state` (`FreshResult`,
`ExprAccumResult`, `SourceLoad`, `Discovery`, `SingletonResult`), `env`
(`ResolveResult`, `ImportEnvResult`), and pairs such as occurrence building's
`Walk.{ b, env }`. These are not optimization-neutral wrappers: the caller
expects to continue the ownership chain through those fields.

Required model:

- the callee summary records return-path ownership (`[.ctx] = OwnedFromParam(k)`,
  `[.state] = OwnedFromParam(k)`, etc.), not only whole-return ownership;
- `ARecordGet(out, .ctx/.state/.env/...)` can move the field ownership out when
  that path is dead through `out` afterward;
- sibling field reads (`out.ty`, `out.local`, `out.diags`, etc.) do not block the
  transported-field move;
- multiple transported fields in the same wrapper can move independently;
- publishing the wrapper record, storing it, returning it, or reading a moved
  field again demotes the relevant field and forces persistent behavior.

This is the same shell-vs-field principle as ordinary record updates, but applied
to returned records. It is required for checker/lowering/resolver/query-analysis
coverage before record and dict codegen wins will show up in real boot workloads.
Variant-wrapped records such as `Result<SourceLoad, AnalysisError>` add variant
segments before the payload field path; see
[summary-specialization.md](summary-specialization.md).

## Wrapper records (`Set<K>`)

The builtin `Set<K>` is a thin record wrapper around `Dict<K, Void>` (field
`entries`). It should inherit the dict ownership story through field-sensitive
projection — **not** a bespoke Set-only optimizer. A proven-owned `Set` shell
whose `entries` field is deeply owned projects to an owned `Dict<K, Void>` region.

**Worked example.** `Set<K>` is a record with a single field `entries: Dict<K,
Void>`. On a proven-owned `Set` shell, the field-backing rule proves
`[.entries]:Unique`, and `ARecordGet(set, .entries)` projects it to an owned
`Dict<K, Void>` local (the projection hinge, `[.entries]* → []*`). `Set.insert` /
`Set.remove` desugar to `Dict.set` / `Dict.remove` on that projected local, which
the ordinary `consume_base` hinge then licenses in place — the **same** path as a
`.types` dict field. No Set-specific analysis or optimizer is needed; the win
falls out of field-sensitive projection.

## Nested collection conservatism (first cut)

Phase 4 goes past pure rejection: it **represents and proves inner ownership**
where it can, via grafted paths `[.f, Elem]` / `[.f, Val]` (and `[Elem]` / `[Val]`
on bare collections). Inner facts are introduced by construction only under a
single-retention proof: `ARecord`/`AArrayLit` graft a stored value's facts under the
element/field path only when that value is owned, last-use, and stored exactly once.
`Vector.make` does **not** introduce `[Elem]` for reference values because it
replicates one value into many slots. Projection preserves facts by `rebase`, and
consuming ops preserve `[Elem]`/`[Val]` only when newly stored inner values are also
owned single-retention values; storing a shared inner value drops the nested path.
The conservative fallback still governs whenever a path can't be proven — a
**shared** element yields no `[Elem]` claim — and the IR/debug output *names the
reason* (`outer owned but inner shared`, `projected inner ownership proven`,
`nested publication detected`), never a silent bail.

What Phase 4 does **not** model: per-index element facts (`[Elem]` summarizes
*all* elements, not `[0]`/`[1]` individually) and general path-granular liveness on
inner paths (both remain later precision; projection moves use whole-record
last-use or the narrow quartet shell-writeback proof — see
[phase4-design.md](phase4-design.md)).

## Codegen-ready decisions

Accepted record/field decisions first feed the codegen track ([record-lowering.md](../codegen/record-lowering.md)); later migration can route them through [mutable-intrinsics.md](../migration/mutable-intrinsics.md):
codegen should see whether to reuse a shell, project an owned field, freeze a
field, or emit the persistent fallback — without making new soundness decisions.

## Relationship to main architecture

Expands the record/nested-ownership sections of
[architecture.md](../architecture.md). The lattice mechanics live in
[fact-lattice.md](fact-lattice.md); the interprocedural field-path key lives in
[summary-specialization.md](summary-specialization.md); the coverage checklist in
[sound-analysis.md](sound-analysis.md). This doc is the connective narrative.
