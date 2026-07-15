# Phase 4 Implementation Design — Record Shell/Field and Nested-Collection Ownership

**Status:** Draft design (feeds the Phase 4 execution plan)

This is the implementation design for **Phase 4** (architecture **1C**) of the
sound-uniqueness track: extend the Phase 2/3 flat, per-`LocalId` ownership domain
with **path-sensitive field ownership** so the compiler's characteristic idiom —
unique record shells whose fields are dicts and vectors — stops classifying as
blanket publication. It remains **analysis-only**: no codegen, decision records,
or in-place lowering (those are Phases 7–8+).

Canonical semantics: [records-fields.md](records-fields.md) (the shell-vs-deep
split, the single place that story is reasoned end to end),
[fact-lattice.md](fact-lattice.md) (the `Record{shell, fields}` /
`Vec{elem}` / `Dict{val}` lattice shape and the field-projection hinge), and
[worked-examples.md](worked-examples.md) (the `advance`/`push_scope` record cases
and Cases B/V). It implements the Phase 4 bullets in
[README.md](README.md). On any genuine conflict the canonical docs win and this
doc is corrected.

## Current state (verified against the branch)

Established before designing, because it fixes what Phase 4 extends and what it
must not disturb:

- **The ownership domain is flat and per-`LocalId`.** `Ownership = { Unique,
  Shared, Unknown }` (`ownership.tw:14`); `ForwardState = .{ own: Dict<Int,Int>,
  valid: Dict<Int,Bool>, prov: Dict<Int,Vector<Int>> }` (`ownership.tw:390`). A
  record local carries exactly **one whole-value fact**. There is no
  path-sensitivity anywhere.
- **The `field_store` hinge is the deliberate Phase 2 cut** (`ownership.tw:465`):
  a value stored into a fresh shell only invalidates the source on last-use or
  publishes it on alias — it makes **no claim about the field's contents**.
- **`consume_base` reasons about the shell only** (`ownership.tw:477`): an
  `ARecordUpdate` licenses a `Unique` result iff `base` is `[]:Unique` and at
  last-use. The field backing is never considered.
- **Summaries are whole-parameter** (`ownership.tw:38-48`): `ParamSummary` =
  `escape` (`Borrowed`/`Retained`) + `capability` (`NoCap`/`Consumed`);
  `ReturnEffect` = `OwnedFresh` / `MayAliasParams(set)` / `Shared`. No field-path
  granularity.
- **`prov` is the precedent for additive, cascading facts** — Phase 3 threaded a
  per-local origin-param map through `ForwardState` and made publication
  transitive over it (`publish_local`) without disturbing Phase 2. Phase 4 follows
  the same additive shape.
- **The fixpoint machinery already handles monotone loop-carried facts**
  (`fixpoint_widen_cap`, `ownership.tw:18`, plus targeted oscillation locking).
  Field facts reuse it unchanged.
- **`artifacts.opt` is the whole-program monomorphized module** the analysis runs
  on, so field-path facts are intraprocedural over concrete function bodies.

## Scope

**In scope (Phase 4):**

- A **path-sensitive field-ownership layer** added to the analysis, tracking
  ownership at `(local, AccessPath)` granularity for the non-shell paths, computed
  intraprocedurally, populating new per-block entry/exit maps alongside the
  existing ones.
- **Record field-backing** (`[.f]`) proven and separated from **shell reuse**
  (`[]`) — the two independent decisions per record update.
- **Nested-collection ownership** (`Vector<Vector<T>>`, `Dict<K, Vector<V>>`) via
  grafted `[.f, Elem]` / `[.f, Val]` / `[Elem]` / `[Val]` paths, so provable inner
  ownership can be represented and (later) licensed.
- The **field-projection hinge** (`ARecordGet` moves a deeply-owned field out) at
  **whole-record last-use** precision.
- Rendering the field facts and the **two-verdict** (shell / field) output in
  `twk ir --cfg`.

**Out of scope (deferred):**

- **No codegen, decision records, or in-place emission** (Phases 7–8).
- **No path-granular liveness.** The projection move uses *whole-record* last-use;
  the finer "`base` still live but `.f` specifically dead" case (transport
  wrappers, `out.ctx` while `out.ty` is read) is **Phase 5**
  ([summary-specialization.md](summary-specialization.md),
  [records-fields.md](records-fields.md)).
- **No interprocedural field-path summaries.** `in_place_paths` and
  `OwnedFromParam` return paths (the summary-side of the record story) are
  **Phase 5**; Phase 4's summaries stay the whole-parameter Phase 3 shape.
- **No ownership specialization** (Phase 6).

Guiding rule (unchanged): **soundness before coverage.** Every field rule
under-claims when unsure; a path fact is `Unique` only when proven, and any doubt
drops it (the shell stays whatever Phase 2/3 decided, so the worst case is exactly
today's behavior).

## Locked decisions

| # | Decision | Choice |
|---|---|---|
| 1 | Representation | **Additive side map** (Approach C): `own` is untouched and *is* the `[]` shell fact; a new `field_own: Dict<Int, Dict<Int, Int>>` (local → PathKey → ownership tag) holds only non-shell paths |
| 2 | `AccessPath` | `Vector<PathSeg>` with `PathSeg = { Field(Int), Elem, Val }`; `[]` = shell/whole value; downward-closed (a `[.f]*` fact presupposes `[]` is `Unique`) |
| 3 | Path keying | Canonical `Int` `PathKey` encoding + a `PathKey → AccessPath` side table for rendering; inner-map iteration and all output sorted by `(LocalId, PathKey)` |
| 4 | Projection precision | Move-vs-borrow by **whole-record** last-use; path-granular liveness deferred to Phase 5 |
| 5 | Consuming-op field-backing | **No new op rule** — the existing `consume_base` hinge on a projected local already licenses in-place once projection supplies `[]:Unique`; the result carries nested facts through conservatively (structural share) |
| 6 | No type oracle | Field selectors come off the ops (`ARecordGet`/`ARecordUpdate` carry `f`; `Elem`/`Val` are structural). Scalar-field facts are harmless (never in-place candidates), consistent with Phase 2's "treat every `ALocal` as possible ref" |
| 7 | Module layout | New `boot/compiler/field_facts.tw` owns `PathSeg`/`AccessPath`/`PathKey` + the map operations (`graft`/`rebase`/`merge`/`clear`/lookup); `ownership.tw` threads the `field_own` field and calls the operations; `cfg.tw`'s `BlockFacts` gains field-fact storage |

## Module layout

- **`field_facts.tw`** (new) — the cohesive, independently-testable path unit:
  - `PathSeg` / `AccessPath` types; `PathKey` encode/decode; the `PathKey →
    AccessPath` side table.
  - Map operations over a local's `Dict<Int, Int>` (PathKey → tag): `graft(dst,
    path, src_shell, src_fields)` (copy a value's facts under a path prefix),
    `rebase(fields, prefix)` (the projection inverse, `[.f]* → []*`),
    `merge(a, b)` (per-path meet for joins), `clear(fields)` (downward-closed
    cascade), and lookup/`is_unique(local_fields, path)`.
  - Pure and value-returning (Twinkle immutability), mirroring `cfg.tw`/`summary.tw`.
- **`ownership.tw`** (extend) — `ForwardState` gains `field_own: Dict<Int,
  Dict<Int,Int>>`; the transfer rules below, the join, `publish_local`, and the
  rebind call into `field_facts.tw`. The public `analyze`/`analyze_with_summaries`
  signatures are unchanged (an empty `field_own` degrades to today's behavior).
- **`cfg.tw`** — `BlockFacts` gains an entry/exit field-fact slot; `render_view`
  learns to print it. Un-analyzed views carry an empty slot (structural suite
  unaffected).
- **CLI** — `twk ir --cfg` renders the new facts and the per-update verdict; no new
  flag.

### Data model

```tw
// field_facts.tw
pub type PathSeg = { Field(Int), Elem, Val }
pub type AccessPath = .{ segs: Vector<PathSeg> }   // segs = [] is the shell (never stored in field_own)

// In ownership.tw:
type ForwardState = .{
  own:   Dict<Int, Int>,                 // unchanged — the [] shell fact
  valid: Dict<Int, Bool>,                // unchanged
  prov:  Dict<Int, Vector<Int>>,         // unchanged
  field_own: Dict<Int, Dict<Int, Int>>,  // NEW: local -> (PathKey -> Ownership tag); non-shell paths only
}
```

`field_own[L]` holds a path only when that path is proven (`Unique`); absence
means "no claim" (the consumer must not treat a missing path as owned). The two
invariants:

1. **Downward-closed:** entries in `field_own[L]` are meaningful only while
   `own[L] == Unique`; whenever `own[L]` leaves `Unique`, `field_own[L]` is
   cleared. A `[.f, Elem]` entry likewise presupposes `[.f]` is present.
2. **Field-sensitivity:** sibling paths are independent — `[.types]:Unique` and
   `[.values]:Shared` (absent) coexist. This is the point: a shared sibling must
   not block a sound in-place on another field.

## Transfer semantics

The transfer runs in the same forward pass as Phase 2/3, extended so each rule
also updates `field_own`. Notation: `graft([.f], v)` copies `v`'s `[]` fact to the
shell's `[.f]` **and** each of `v`'s `field_own` paths rebased under `.f`
(`v`'s `[Elem]:Unique` → shell `[.f, Elem]:Unique`). `rebase([.f]*) → []*` is the
projection inverse.

### Introduction — storing a unique value into a fresh/updated shell

| Op | Field-fact rule |
|---|---|
| `ARecord{…, f: v, …}` | shell `[]:Unique` (Phase 2). For each field `f`: if `v` is `[]:Unique` **and** last-use → `graft([.f], v)`; aliased/shared `v` → no `[.f]` claim (existing `field_store` demotion still applies) |
| `AArrayLit([v…])` / `Vector.make(_, v)` | outer `[]:Unique`; if **every** stored element `v` is `[]:Unique`+last-use → `[Elem]:Unique` (grafting inner facts under `Elem`); any shared element → no `[Elem]` |
| `AVariant(f, [v…])` | outer `[]:Unique`; payload facts graft under the payload path |
| `ARecordUpdate(base, f, v, …)` | shell `[]` from existing `consume_base(base)`. Result `field_own` = **base's field facts with path `[.f]` replaced** by `graft([.f], v)` (when `v` is `[]:Unique`+last-use). Sibling fields carry over from `base` — the quartet write-back that keeps `.values` while refreshing `.types` |

### Projection — the field-projection hinge (`ARecordGet(base, f) → R`)

- R inherits `base`'s `[.f]*` facts **rebased to `[]*`** (so R is `[]:Unique` when
  `.f` is deeply owned).
- **Move vs borrow by whole-record last-use** (Decision 4): if `base` is at
  whole-value last-use here → **move**: invalidate `base`'s `[.f]*`, R keeps
  `Unique`. Else → **borrow**: `base` retains `.f`, so R's `[]` is `Shared`
  (mutating R in place would corrupt `base`'s still-live view — sound).
- The record quartet always rebinds the shell (`assign env = …`), so `base` is
  dead after the write-back and the projection is a clean move — no path-liveness
  needed for the dominant case.

### Consuming op on the projected field — no new op rule (Decision 5)

`dict.set` / `vector.append` on R already license in-place via the existing
`consume_base` hinge iff R is `[]:Unique`+last-use, which the clean projection move
now supplies. The result carries R's nested facts through (structurally shared:
append/set preserve untouched elements), covering Case V's `.components` inner
append and `Dict<K, Vector>` value fields.

### Threading and demotion

- `assign L = atom` carries the atom's `field_own` alongside its `own` (rebind
  transfers field facts).
- **Demotion cascade:** whenever `own[L]` leaves `Unique` — publish, alias, or a
  join that lowers the shell — **clear `field_own[L]`**. `publish_local` extends
  its existing prov-transitive cascade to drop field facts too; a projected field
  local that escapes (stored into an escaping shell, returned, captured) demotes
  the same way.

### Output — the two independent verdicts

At each `ARecordUpdate` / quartet site the analysis prints two orthogonal verdicts:

1. **shell reuse** — `base` `[]:Unique`+last-use? (already computed by
   `consume_base`).
2. **field-backing** — projected R `[]:Unique` (i.e. `base.[.f]` deeply `Unique`)
   + clean move?

Each carries its rejection reason when it fails (`outer owned but inner shared`,
`aliased shell`, `insufficient deep ownership`, `borrow-projection: base still
live`) — never a silent bail (the `records-fields.md` hard requirement).

## Join, fixpoint, determinism

- **Join.** `field_own` merges **positionally over edge args**, mirroring the
  `own` join: a path fact reaches a join/loop-header param only if present and
  `Unique` on **every** predecessor's fed atom (per-path meet); missing on any
  predecessor drops it. This yields loop-carried field ownership (`[.types]:Unique`
  crosses a back-edge iff every iteration preserves it), iterated to the same
  fixpoint. Monotone (facts only move down; finite lattice → terminates), reusing
  the existing widen-cap / oscillation-locking. If a param's `own` joins to
  non-`Unique`, its `field_own` is cleared (invariant stays join-stable).
- **Determinism.** `PathKey` is a canonical `Int` encoding; inner-map iteration and
  every rendered/compared output are sorted by `(LocalId, PathKey)` — the same
  discipline as the sorted `prov`/`live` vectors. The `PathKey → AccessPath` side
  table is built deterministically (in path-construction order).

## Rendering

Extend `render_view` (no new flag):

- Per-boundary field facts, keyed and sorted:
  `field_facts={L7:[.types]=U,[.indices]=U; L9:[Elem]=U}`.
- Per record-update site, the two-verdict line:
  `L3 = record_update L7.types  shell=reuse(unique) field=in-place([.types] unique)`
  or `… field=persistent(shared sibling)`.

Kept off nothing — this is the primary Phase 4 observable (generated code is
unchanged).

## Testing

New `boot/tests/suites/cfg_field_facts_suite.tw`, TDD, mirroring the Phase 2/3
suites. Synthetic inline fixtures compiled via `pipeline.compile_source →
ownership.analyze`, each cross-referenced to its
[worked-examples.md](worked-examples.md) case. As in Phase 2, each fixture is
checked against `twk ir <fixture> --opt` so the tested shape **survives
optimization**.

- **advance** — scalar `.pos` field: shell-reuse verdict, **no** spurious field
  claim.
- **push_scope** — `[.locals]:Unique` field-backing licensed; siblings
  (`.depth`/`.tokens`) not required unique.
- **Case B** — `[.types]:Unique` across the `add_type`-shaped quartet; `.values`
  shared but non-blocking.
- **Case V** — collection fields (`[.indices]`/`[.stack]`) field-backing +
  provable nested `[.components, Elem]`; loop-carried field ownership across the
  back-edges.
- **Negatives** — aliased shell (branch_env-shape) demotes field facts →
  persistent; a shared sibling does **not** block a sound sibling; borrow-
  projection (base still read) → `Shared` R, no in-place; a published field local
  → persistent.
- **Determinism** — `twk ir --cfg` byte-identical across two builds.
- **Nested-graft soundness guard** — a fresh shell over a **shared** collection
  field (`Wrapper.{ xs }` with `xs` aliased) yields **no** `[.xs]` claim.

## Non-goals

- No codegen, decision records, or in-place emission (Phases 7–8).
- No path-granular liveness (Phase 5).
- No interprocedural field-path summaries / `in_place_paths` / return-path
  ownership (Phase 5).
- No ownership specialization (Phase 6).
- No `Moved` lattice element; no runtime uniqueness flags/refcounts/COW checks.
- No record type oracle threaded into the analysis.

## Acceptance criteria

Concrete gates for the execution plan (all via the boot suite unless noted):

1. **Field-backing facts** — the `push_scope` / Case B fixtures show `[.f]:Unique`
   on the mutated collection field and the two-verdict line reports
   `field=in-place`.
2. **Field-sensitivity** — a shared sibling field does not demote the sound
   field; the shared sibling shows no `Unique` fact while the sound field does.
3. **Nested inner** — Case V's `[.components, Elem]` is proven where the inner
   vectors are owned; the outer-owned/inner-shared negative shows no inner claim
   and names the reason.
4. **Projection move/borrow** — a rebound-shell quartet classifies the projection
   as a move (R `[]:Unique`); a still-live-`base` projection classifies as a borrow
   (R `[]:Shared`, no in-place).
5. **Demotion cascade** — publishing / aliasing a shell clears its field facts
   (branch_env-shape negative).
6. **Downward-closed invariant** — no fixture ever exhibits a `[.f]*` fact while
   `own` for that local is non-`Unique` (checked in the suite).
7. **Determinism** — `twk ir --cfg` (facts + verdicts) byte-identical across two
   builds.
8. **No regression to in-place** — `twk ir --census` still shows **0 in-place**
   (Phase 4 changes no codegen).
9. **Full verification** — `make boot-test` green and `make stage2` reaches the
   self-host fixed point (Phase 4 is boot-only and adds no stage0-parity
   construct).

## Deferrals and tracking

| Item | Home |
|---|---|
| Path-granular liveness (transport-wrapper `out.ctx` while `out.ty` read) | Phase 5 ([summary-specialization.md](summary-specialization.md)) |
| Interprocedural field-path summaries / `in_place_paths` / `OwnedFromParam` return paths | Phase 5 |
| Ownership specialization over field-path keys | Phase 6 |
| Candidate verdicts → decision records → in-place emission | Phases 7–8 ([../codegen/README.md](../codegen/README.md)) |

## Determinism-sensitive spots (lock with tests)

- **`PathKey` encoding** — canonical and collision-free over the seg sequence; the
  determinism test (Acceptance 7) gates it.
- **`field_own` map iteration** — never drives an order-sensitive result (the join
  is a commutative per-path meet); any rendered/compared output is keyed by a
  sorted `(LocalId, PathKey)` list, not raw `Dict` iteration.
- **`PathKey → AccessPath` side table** — populated in deterministic
  path-construction order so rendering is byte-stable.
