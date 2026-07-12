# Record Shell Reuse, Field Ownership, and Nested Collections

**Status:** Draft skeleton (to be expanded before record/dict codegen lowering)

## Purpose

Collects the record and nested-collection ownership story that is currently
scattered across [fact-lattice.md](fact-lattice.md) (the record quartet, the
`Record{shell, fields}` lattice shape), [summary-specialization.md](summary-specialization.md)
(field-path-granular `in_place_paths`), and [sound-analysis.md](sound-analysis.md)
(the record/nested coverage matrices). This doc is the single place the
**shell-vs-deep split** is reasoned about end to end.

Expands the `Record shell reuse and field ownership` and `Nested collection
ownership` sections of [architecture.md](architecture.md).

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
   shell `Owned`.)
2. **Field-backing mutation** — can the collection stored in / projected from
   field `.f` be treated as deeply owned and mutated? (needs `.f` deeply owned +
   no alias on the old `.f`.)

They must stay separate. Sibling fields untouched by the update need not be
owned — field sensitivity is the point (a shared `.values` must not block a
sound `.types` in-place update).

> TODO: enumerate the field-fact lattice interaction with the shell fact; the
> downward-closed `(param, path)` key relationship (owning `[.f]` presupposes
> owning `[]`).

## Wrapper records (`Set<K>`)

The builtin `Set<K>` is a thin record wrapper around `Dict<K, Void>` (field
`entries`). It should inherit the dict ownership story through field-sensitive
projection — **not** a bespoke Set-only optimizer. A proven-owned `Set` shell
whose `entries` field is deeply owned projects to an owned `Dict<K, Void>` region.

> TODO: worked `Set` projection example; confirm the field name and lowering.

## Nested collection conservatism (first cut)

The first implementation may **reject most inner mutation** and only optimize the
outer backing. The hard requirement is that the IR/debug output *names the
reason*: `outer owned but inner unknown/shared`, `projected inner ownership
proven`, or `nested publication detected` — never a silent bail.

> TODO: which nested cases (if any) the first analysis pass models beyond
> conservative rejection of projected-element/value inner mutation (Open Question
> in architecture.md).

## Codegen-ready decisions

Accepted record/field decisions feed [mutable-intrinsics.md](mutable-intrinsics.md):
codegen should see whether to reuse a shell, project an owned field, freeze a
field, or emit the persistent fallback — without making new soundness decisions.

## Relationship to main architecture

Expands the record/nested-ownership sections of
[architecture.md](architecture.md). The lattice mechanics live in
[fact-lattice.md](fact-lattice.md); the interprocedural field-path key lives in
[summary-specialization.md](summary-specialization.md); the coverage checklist in
[sound-analysis.md](sound-analysis.md). This doc is the connective narrative.
