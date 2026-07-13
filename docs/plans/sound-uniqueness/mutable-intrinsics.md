# Mutable-Collection Intrinsic Family and Lowering

**Status:** Draft skeleton (codegen contract — to be expanded before codegen
lowering begins)

## Purpose

The codegen-side counterpart to the analysis docs. Once the ownership analysis
([fact-lattice.md](fact-lattice.md), [summary-specialization.md](summary-specialization.md))
has proven a region owned and produced ANF-keyed decision records, this doc
defines **what codegen emits**: a small, backend-independent, compiler-private
intrinsic family for mutable collection regions, and how vector, dict, and the
freeze/publish boundary lower onto it.

These intrinsics are **not** prelude or `@std` APIs and **not** user-callable
escape hatches — they are the optimizer/codegen contract for proven mutable
regions. Codegen consumes accepted decisions and stays mechanical; it must not
re-prove uniqueness, field ownership, or escape (that all happened in analysis).

Expands the `Mutable collection intrinsic family`, `Mutable vector lowering`,
`Mutable dict lowering`, and `Promotion/freeze model` sections of
[architecture.md](architecture.md).

## Analysis → codegen handoff contract

The decision-record *schema* (which fields a decision carries) lives in
[cfg-ownership-ir.md](cfg-ownership-ir.md) "Codegen contract". This section is the
*soundness* half: the invariants that let codegen consume a decision **without
re-checking it**, so "codegen stays mechanical" is safe rather than merely
asserted. The seam between analysis and codegen is exactly where cross-pass
miscompiles live; for a project whose safety story is "the fallback guarantees
soundness," that guarantee has to be an enforceable contract here, not a mutual
understanding between two docs.

### The fail-safe default (the governing rule)

**Absence, ambiguity, or staleness of a decision ⟹ emit the persistent operation.
Codegen never guesses mutation.** If no decision is attached to an update site, if
a decision's ANF key no longer resolves (see the view-staleness rule in
cfg-ownership-ir.md), or if anything is unclear, codegen emits the ordinary
immutable op. This single rule is where "soundness is structural" is cashed out at
the boundary: **every way the contract can be violated collapses to the persistent
path**, so a dropped decision, a stale key, or an analysis that simply did not run
degrades *performance, never correctness*. The mutable path is opt-in per site and
must be positively licensed by a live decision.

### Analysis obligations (what each decision certifies)

Codegen is licensed to assume these because the analysis proved them; it must not
re-derive them. Every emitted decision must satisfy:

- **`begin`-in-place** ⟹ the source value is `Owned` **and** last-use at that
  point (no live alias, no later read) — the `Owned ∧ last_use` pair from
  [summary-specialization.md](summary-specialization.md). `Owned` alone is never
  sufficient.
- **Freeze totality** ⟹ every mutable region has a `freeze`/publish on *every*
  exit path reachable from its `begin`. No path lets a still-mutable handle escape
  a publication sink unfrozen; no path double-freezes.
- **No use-after-freeze / no double-begin** within a region.
- **Field-path keys are downward-closed** ⟹ a decision owning `[.f]` presupposes
  the shell `[]` is owned (see [records-fields.md](records-fields.md)).
- **Variant selection is total** ⟹ every specialized call site resolves to exactly
  one variant, and a generic/persistent fallback variant always exists (so the
  fail-safe default is always reachable).

### Codegen assumptions (the positive dual of "don't re-prove")

Given a live, well-formed decision, codegen **may**: thaw the source without
copying (ownership+last-use certified); read/write/append/remove through the
handle without alias checks; and freeze exactly at the decision's publish point.
Codegen **must not**: re-run uniqueness/field-ownership/escape analysis; insert a
freeze between internal updates of the same region; or promote a site to mutation
on its own initiative.

> A decision is a *capability*, not a hint: present ⟹ the proof holds and codegen
> acts on it; absent ⟹ persistent. There is no third state.

## The intrinsic family (shared shape)

One conceptual op-set spans vectors, dicts, and record shells, with
collection-specific operation metadata layered on top — not three ad hoc
rewrites to whatever helper exists today:

- `begin`/`thaw` — enter a mutable region from a proven-owned persistent value
  or a known-empty collection;
- `read` — read through the mutable handle (element / value / field);
- `write`/`update` — write through the mutable handle;
- `append`/`extend` — grow, for vector-like collections;
- `remove` — delete, for map-like collections;
- `freeze`/`publish` — leave the region, producing the ordinary immutable value.

Intrinsics carry enough type information for `Vector<T>`, `Dict<K,V>`, and record
shells without becoming public signatures.

> TODO: enumerate the exact intrinsic set + operand/type encoding; decide
> ANF-node vs annotation representation (Open Question in architecture.md).

## Mutable vector lowering

Compiler-private mutable vector target over the intrinsic family. May initially
lower to existing builder / `set_in_place` hooks; the conceptual target is an
internal mutable vector representation (create-from-fresh/empty, read, write,
append/extend, freeze).

First high-value patterns: `flags = flags.set_at(k, false)` and
`balls = balls.set_at(j, …)` in loops; vector accumulator append/build loops;
thin method-wrapper forms (`xs.set_at(i, v)`).

> TODO: decide PVec reuse vs a new private mutable PVec vs typed/flat storage
> (Open Question). Cross-link the typed-vector work.

## Mutable dict lowering

Compiler-private mutable HAMT target over the same family. Must preserve
language-level behavior: lookup semantics **and insertion-order iteration**.
Shares the ownership/escape/publication/join/loop-carried framework with vectors
— collection-specific metadata only. `Dict.set`/`Dict.remove`, with old-value
observability as the main blocker.

> TODO: mutable-HAMT node model; insertion-order preservation under in-place
> mutation; `Dict.remove` cost (see typed-dict census finding).

## Record shells over this family

Record shell reuse and field-backing mutation route through the same intrinsic
model where they touch collection storage. The shell-vs-deep-field split and
`Set<K>` wrapper projection are specified in [records-fields.md](records-fields.md);
this doc only owns the *emitted intrinsic shape* for the field-backing case.

> Open Question (architecture.md): does record shell reuse use this family, or a
> smaller record-specific intrinsic/annotation like today's `ARecordUpdate`
> in-place bit?

## Promotion / freeze model

Insert `freeze`/`publish` **only** when a mutable region must produce an ordinary
persistent value (the publication sinks in [fact-lattice.md](fact-lattice.md) /
[concurrency-publication.md](concurrency-publication.md)). Never insert a freeze
*between* internal updates of the same proven region. A proven-owned persistent
value may `begin` without copying; an unproven value stays on the persistent path
(no speculative clone-to-owned unless proven necessary and profitable).

> TODO: freeze as explicit ANF vs codegen annotation vs a separate post-ANF
> lowering phase (Open Question); branch-local region merge on joined publish.

## Relationship to existing boot mutable/in-place work

The stance is **own the decisions, reuse the mechanisms** — a layered split, not
coexistence-as-peers and not a migration rewrite. Existing hooks are not
competitors as lowering mechanisms; they become competitors only if they retain or
regain an independent legality decision path. The plan forbids that split-brain
shape: all mutability decisions come from the new ownership/CFG proof layer, and
the old hooks remain implementation targets for decisions already proven sound.

### Current-branch state (why this is the right split)

This branch deliberately removed the previous uniqueness/liveness/builder-region
passes, so the two layers are already in different states:

- **The decision layer is empty.** Nothing currently drives in-place: the
  `ARecordUpdate` `in_place` slot exists in ANF but is always `false`, and the
  uniqueness-driven builder/`set` rewrites are gone. There is no competing
  decider to coexist with — the new proof model simply *fills* this layer.
- **The mechanisms are live and load-bearing.** `vector$builder_*` (used by
  `collect` lowering regardless of optimization), `vector$set_unsafe` (index-set
  lowering), the `rt.arr`/`rt.dict` in-place primitives, and the `in_place` slot
  all remain. They are proven and stay.

### The split

- **Own the decision/contract layer.** The new proof model is the single source
  of truth for "this region is mutable." A *second* decision path is exactly the
  split-brain the branch deleted to avoid, and the seam where soundness would
  leak — so this layer is owned, not shared.
- **Reuse the mechanisms as the initial lowering backend.** The intrinsic family's
  first lowering emits the existing hooks (`builder_*`, `set_unsafe`, `rt.arr`/
  `rt.dict` in-place, the `in_place` slot) — reuse, not rewrite. Wins come from
  re-filling the dormant decision slots, not from replacing working runtime code.
- **Then hide/absorb (transient-hook cleanup phase).** Once the intrinsic family
  is the only thing driving them, the hooks become implementation details *behind*
  this family rather than separately special-cased. The migration path is not to
  keep adding special cases to `vector$builder_*`/`set_unsafe`. Note this targets
  their use as an *optimizer rewrite target*; `collect` still lowers to a builder
  independent of optimization, so the builder mechanism itself is not removed.

### Buffer is separate

`@std.buffer` is a **user-facing** linear-memory escape hatch, not an internal
hook to absorb into this family. Its retirement is conditional (only after
ordinary immutable code reaches the target performance class) and governed by its
own policy — see [buffer-cleanup.md](buffer-cleanup.md). Do not lump it with the
internal `builder_*`/`set_unsafe` scaffolding above.

## Relationship to main architecture

This doc expands the codegen sections of [architecture.md](architecture.md).
Nothing here decides ownership — it consumes proven decisions and emits code.
