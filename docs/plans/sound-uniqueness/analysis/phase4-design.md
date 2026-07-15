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
  **whole-record last-use** precision plus the narrow **quartet shell-writeback**
  proof needed for record field update idioms.
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
- **No intraprocedural variant-payload ownership.** `PathSeg` = `Field/Elem/Val`
  has no payload segment, so `AVariant` is shell-only; variant payload paths
  (`Ok[0].state`) are **Phase 5**.

Guiding rule (unchanged): **soundness before coverage.** Every field rule
under-claims when unsure; a path fact is `Unique` only when proven, and any doubt
drops it (the shell stays whatever Phase 2/3 decided, so the worst case is exactly
today's behavior).

## Locked decisions

| # | Decision | Choice |
|---|---|---|
| 1 | Representation | **Additive side map** (Approach C): `own` is untouched and *is* the `[]` shell fact; a new `field_own: Dict<Int, Dict<Int, Int>>` (local → PathKey → ownership tag) holds only non-shell paths |
| 2 | `AccessPath` | `Vector<PathSeg>` with `PathSeg = { Field(Int), Elem, Val }`; `[]` = shell/whole value; downward-closed (a `[.f]*` fact presupposes `[]` is `Unique`) |
| 3 | Path keying | Canonical `Int` `PathKey` encoding, **reversible** for the Phase 4 path shapes (depth ≤ 2), so `path_of_key` reconstructs the `AccessPath` by pure arithmetic and **no interned `PathKey → AccessPath` side table is needed** (the "side table" collapses to pure decode; a real interning table returns only when deeper Phase 5 paths exceed the reversible scheme); inner-map iteration and all output sorted by `(LocalId, PathKey)` |
| 4 | Projection precision | A projection moves when either `base` is at **whole-record** last-use or the site satisfies the Phase 4 **quartet shell-writeback** proof: `base` remains live only to perform the matching `ARecordUpdate(base, same f, replacement)` and old `base.[.f]` is unobservable before that write-back. Otherwise projection is a borrow and demotes **both** sides — `own[R] ← Shared` *and* `base`'s `[.f]*` cleared. General path-granular liveness remains Phase 5 |
| 5 | Consuming-op field-backing | The existing `consume_base` hinge licenses the **outer** shell in-place once projection supplies `[]:Unique` (no new rule there). **Nested** facts need a real rule: `dict.set`/`vector.append`/`Vector.set` keep `[Elem]`/`[Val]` only when the stored value is itself `[]:Unique`+last-use+single-store; a shared/aliased stored value **drops** `[Elem]`/`[Val]` and descendants |
| 6 | Alias-creation discipline | Deep facts are claimed only under **single-retention**: a value stored once, at last-use, into exactly one slot. Duplicate operands (`.{ a: x, b: x }`, repeated array elements) and replicated stores (`Vector.make(n, v)`) create intra-shell aliases → no deep fact |
| 7 | No type oracle | Field selectors come off the ops (`ARecordGet`/`ARecordUpdate` carry `f`; `Elem`/`Val` are structural). Scalar-field facts are harmless (never in-place candidates), consistent with Phase 2's "treat every `ALocal` as possible ref" |
| 8 | Module layout | New `boot/compiler/field_facts.tw` owns `PathSeg`/`AccessPath`/`PathKey` + the map operations (`graft`/`rebase`/`merge`/`remove_prefix`/`clear_all`/lookup); `ownership.tw` threads the `field_own` field and calls the operations; `cfg.tw`'s `BlockFacts` gains field-fact storage |

## Module layout

- **`field_facts.tw`** (new) — the cohesive, independently-testable path unit:
  - `PathSeg` / `AccessPath` types; `PathKey` encode/decode as **reversible
    arithmetic inverses** for the Phase 4 shapes (so rendering decodes a key
    directly — no interned side table to thread through the analysis).
  - Map operations over a local's `Dict<Int, Int>` (PathKey → tag): `graft(dst,
    path, src_shell, src_fields)` (copy a value's facts under a path prefix),
    `rebase(fields, prefix)` (the projection inverse, `[.f]* → []*`),
    `remove_prefix(fields, prefix)` (drop an entire subtree such as `[.f]*` while
    preserving siblings), `merge(a, b)` (per-path meet for joins), `clear_all(fields)`
    (whole-local demotion cascade), and lookup/`is_unique(local_fields, path)`.
  - Pure and value-returning (Twinkle immutability), mirroring `cfg.tw`/`summary.tw`.
- **`ownership.tw`** (extend) — `ForwardState` gains `field_own: Dict<Int,
  Dict<Int,Int>>`; the transfer rules below, the join, `publish_local`, and the
  rebind call into `field_facts.tw`. The public `analyze`/`analyze_with_summaries`
  signatures are unchanged (an empty `field_own` degrades to today's behavior).
- **`cfg.tw`** — `BlockFacts` (`cfg.tw:36`) gains a field-fact slot
  (`field_own: Dict<Int, Dict<Int, Int>>`); `empty_block_facts` (`cfg.tw:42`) seeds it
  empty and the `keys().len() == 0` "un-analyzed" guard (`cfg.tw:829`) is extended to
  cover it; `render_facts`/`render_view` learn to print it. Un-analyzed views carry the
  empty slot (structural suite unaffected).
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

**Where each rule lands.** `ARecord` / `AArrayLit` / `AVariant` / `ARecordGet` /
`ARecordUpdate` are true `AnfOp`s handled in `transfer_op` (`ownership.tw:670`), and
the field-fact rules attach directly there. The collection builtins named below —
`Vector.make`, `dict.set`, `vector.append`, `Vector.set` — are **not** ops: they are
`ACall`s routed through `transfer_builtin_call` (`ownership.tw:607`), where
`Vector.make`'s never-`[Elem]` rule lands on the `.Allocate`/`absorb_retained_call_args`
path and the consuming-op rule (Decision 5) lands on the `.Update`/`consume_call_base`
path. Consequently the `[Elem]`-vs-`[Val]` segment choice cannot be read off the op
shape — it needs a **vector-vs-dict signal** from `CallSemantics`, which today carries
only `effect`/`cow_base_arg`/`retained_args`. Supplying that signal (a small
`CallSemantics` field or a base-type classification) is a named Phase 4 prerequisite;
until it exists the consuming rule must default to **dropping** the nested path (sound
under-claim), never guessing a segment.
- `AVariant`'s current transfer (`ownership.tw:681` — `field_store` each payload, result
  `Unique`) is *already* the Phase 4 shell-only behavior, so it needs **no change**;
  this is called out only to keep it from accreting a deep claim later.

### Introduction — storing a unique value into a fresh/updated shell

All introduction is gated on **single-retention** (Decision 6): a value grafts a
deep fact only when it is `[]:Unique`, at last-use, **and** stored into exactly one
slot of the shell. Duplicate/replicated stores create intra-shell aliases and
graft nothing.

| Op | Field-fact rule |
|---|---|
| `ARecord{…, f: v, …}` | shell `[]:Unique` (Phase 2). For each field `f`: `graft([.f], v)` only when `v` is `[]:Unique`, at last-use, **and appears exactly once** among the shell's stored operands (a value in two fields — `.{ a: x, b: x }` — aliases itself, so neither is claimed). Otherwise no `[.f]` claim (`field_store` demotion applies) |
| `AArrayLit([v…])` | outer `[]:Unique`; `[Elem]:Unique` only when **every distinct** element is `[]:Unique`+last-use **and no element value is stored twice** (a duplicate element is an intra-array alias → drop `[Elem]`) |
| `Vector.make(n, v)` | outer `[]:Unique`; **never** `[Elem]:Unique` — it replicates the single reference `v` into `n` slots (`n` dynamic), so the elements alias each other |
| `AVariant(f, [v…])` | outer `[]:Unique` **shell only**. Intraprocedural payload ownership is **deferred to Phase 5** — `PathSeg` has no payload segment — so payload operands just follow the `field_store` move/alias demotion, with no deep claim |
| `ARecordUpdate(base, f, v, …)` | shell `[]` from existing `consume_base(base)`. Result `field_own` = base's field facts with the **entire `[.f]*` subtree removed first**, then `graft([.f], v)` (only when `v` is `[]:Unique`+last-use+single-store). Removing the whole subtree matters: a shared replacement must not leave stale `[.f, Elem]` descendants. Sibling fields carry over unchanged — the quartet write-back that keeps `.values` while refreshing `.types` |

### Projection — the field-projection hinge (`ARecordGet(base, f) → R`)

Exact shell-vs-field mechanics (Decision 4): R's facts come from `base`'s `[.f]*`
subtree **rebased to `[]*`** — the `[.f]` fact becomes `own[R]` (R's shell), and
each **strict descendant** `[.f, …]` becomes a `field_own[R]` entry
(`[.f, Elem] → [Elem]`).

Phase 4 has two move proofs:

1. **Whole-record last-use move:** if `base` is dead after the projection, transfer
   `base.[.f]*` to R and remove the `[.f]*` subtree from `base`.
2. **Quartet shell-writeback move:** if `base` is not whole-record-dead, the
   projection can still move the field when a syntactic/CFG proof shows that the
   only remaining live use of `base` is the matching shell write-back:

   ```text
   R   = record_get base.f
   R2  = consuming_builtin_or_move_chain(R, ...)
   env2 = record_update base.f = R2     // binds a FRESH local (the ANF shape);
                                        // `assign base = …` is an equivalent
                                        // source form, not a required element
   ```

   The `assign base = …` write-back is only one surface form: in optimized ANF the
   record-field-update rebind typically binds a **fresh** result local (`env2`), so
   the proof must **not** require an `AAssign` of `base`. The essential, form-agnostic
   conditions are: on every continuing path from the projection, `base` is used for
   **nothing but** the single matching `ARecordUpdate(base, f, replacement)` — not
   published, passed to an unknown/user call, stored, returned, aliased, read by the
   terminator (branch test / match scrutinee) or fed to a successor param, nor used
   for another field read/update; `base.[.f]` is not read again; and `base` is **dead
   after the block** (so no surviving view of the old shell can observe R's in-place
   mutation). The replacement must come from the projected field's consume/produce
   chain or another independently-owned value. This is a narrow Phase 4 substitute for
   general path-liveness, not a general permission to move fields out of live records.

For either move proof, transfer `own[R]` from the exact `[.f]` fact, populate R's
strict descendants, and remove `base`'s `[.f]*` subtree with `remove_prefix`. R's
`prov` still follows `base`'s origins (unchanged from today's `ARecordGet` at
`ownership.tw:699`), so an escaping R still publishes back through `base`'s origin
params; the field-fact demotion cascade (below) rides the same `prov` edges.
During a quartet shell-writeback move, `base` remains valid only as a shell for the
matched `ARecordUpdate(base, f, replacement)`; any other live use makes the proof
fail and the site follows the borrow rule instead.

**Recognizing the quartet is a pre-pass, not a forward-local decision.** The forward
transfer reaches `R = record_get base.f` *before* it sees the downstream write-back, so
it cannot decide the move locally. Phase 4 recognizes the quartet with a **block-local
linear-ANF pattern scan** run alongside liveness, producing a per-op annotation
(`quartet_move: Bool`) the transfer consults at the `ARecordGet`. Scope the recognizer
to a **single block's** straight-line op sequence — projection, the consume/produce
chain, and the matching `ARecordUpdate(base, f, …)` (whose result binds a fresh local;
no trailing `AAssign` of `base` is required), with `base` not otherwise used and dead
after the block. A projection whose write-back lands in another block (base live across
a block boundary) is **out of scope for the move** and falls to the borrow rule — a
sound scoping that keeps the proof a pure intra-block peephole and matches the
intraprocedural fixture set.

- **Borrow** (no move proof): the backing at `base.[.f]` is now aliased by R, so
  **both** sides demote — `own[R] ← Shared` (and thus, by downward-closure, no
  `field_own[R]`) **and** `base`'s `[.f]*` subtree **cleared**. Without either a
  whole-record last-use or the quartet shell-writeback proof, Phase 4 cannot prove
  R stays read-only, so aliasing must sink both, exactly like the `AInit` alias
  case. (Full precision recovery — `base` live but `.f` path-dead while siblings
  are read — is Phase 5.)

### Consuming op on the projected field (Decision 5)

The **outer** in-place is licensed by the existing `consume_base` hinge iff R is
`[]:Unique`+last-use — the clean projection move supplies that, so no new rule for
the shell. The **nested** facts need a real transfer rule, because a consuming op
can insert a *shared* inner collection:

- `vector.append(v, x)` / `Vector.set(v, i, x)` / `dict.set(d, k, x)`: the result
  keeps `[Elem]`/`[Val]:Unique` **only if** the stored `x` is itself
  `[]:Unique`+last-use (a single new owned element). A shared/aliased `x` **drops**
  `[Elem]`/`[Val]` and all descendants — the collection now holds a shared inner
  backing, so it is no longer deeply owned.
- Untouched existing elements are structurally preserved, so appending an *owned*
  `x` onto an all-owned `v` keeps `[Elem]:Unique` (Case V's `.components` inner
  append); appending a *shared* `x` drops it.

### Threading and demotion

- `assign L = atom` carries the atom's `field_own` alongside its `own` (rebind
  transfers field facts).
- **Demotion cascade:** whenever `own[L]` leaves `Unique` — publish, alias, or a
  join that lowers the shell — **clear `field_own[L]`**. To make this sound by
  construction rather than by remembering it at every lowering site (`init_hinge`'s
  alias branch, `consume_base`'s non-last-use `Unknown`, `transfer_builtin_call`'s
  `ReadOnly`/`Pure`, the join), fold the clear into the **single shell-fact setter**:
  `set_own_st(id, o)` drops `field_own[id]` whenever `o != Unique`. Introduction and
  projection therefore set the shell `Unique` **first**, then populate `field_own`.
  `publish_local` (`ownership.tw:425`) already lowers `own` for `id` and its prov
  origins via `set_own_st`, so its prov-transitive cascade drops field facts for free.

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

- **Join.** `field_own` merges by the **same two cases** `join_entry_ownership`
  (`ownership.tw:831`) uses for `own`, per-path meet in each: a **block param** takes
  the meet of `field_own[pred_exit_edge_arg[i]]` across preds (positional over edge
  args); a **live-through non-param local** takes the meet of `field_own[lid]` across
  preds (same-id). A path fact survives only if present and `Unique` on **every**
  contributing predecessor; missing on any drops it. Only **processed** predecessors
  contribute — unprocessed back-edges are skipped exactly as for `own`, so an
  uninitialized back-edge cannot spuriously mint a loop-carried field fact. This yields
  loop-carried field ownership (`[.types]:Unique` crosses a back-edge iff every iteration
  preserves it), iterated to the same fixpoint. Monotone (facts only move down; finite
  lattice → terminates), reusing the existing widen-cap / oscillation-locking. If a
  local's joined `own` is non-`Unique`, its `field_own` is cleared (the `set_own_st`
  choke point keeps the invariant join-stable).
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
suites. Each fixture is **intraprocedural**: a record constructed locally (so its
`[]:Unique` is established in the same function) and its field mutated by a
**builtin** (`Dict.set` / `vector.append`) — *not* received as a parameter or
updated across a user call. The cross-function forms of the worked examples are
gated on Phases 5–6 and are **not** Phase 4 fixtures. As in Phase 2, each fixture
is checked against `twk ir <fixture> --opt` so the tested shape **survives
optimization**.

- **advance-shaped** — a scalar-field update: shell-reuse verdict, **no** spurious
  field claim.
- **push_scope-shaped (inlined)** — a locally-fresh record whose dict field is
  updated by a builtin `Dict.set` in the same function: `[.f]:Unique` field-backing,
  in-place licensed; siblings not required unique.
- **Case B-shaped (inlined)** — a locally-fresh record with two direct builtin
  field updates: `[.types]:Unique` carried across them; `.values` shared but
  non-blocking. (Case B's cross-function `add_type` form is Phases 5–6.)
- **Case V-shaped (inlined)** — a locally-fresh `Vector<Vector>` field with an
  all-owned inner: `[.components, Elem]:Unique`; loop-carried field ownership across
  back-edges. (visit's recursive/param form is Phases 5–6.)
- **Alias-creation negatives** — duplicate-store aggregate (`.{ a: x, b: x }`,
  `Vector.make(n, v)`) claims **no** deep fact; a consuming op storing a **shared**
  value drops `[Elem]`/`[Val]`; a fresh shell over a shared field (`Wrapper.{ xs }`,
  `xs` aliased) yields no `[.xs]` claim.
- **Aliasing negatives** — aliased shell (branch_env-shape) demotes field facts →
  persistent; a shared sibling does **not** block a sound sibling; **borrow-
  projection** (base still read without a quartet shell-writeback proof) → `Shared`
  R **and** `base`'s `[.f]*` cleared, no in-place; a published field local →
  persistent.
- **Determinism** — `twk ir --cfg` byte-identical across two builds.

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

1. **Field-backing facts (intraprocedural)** — a locally-fresh record whose
   collection field is mutated by a **builtin** in the same function shows
   `[.f]:Unique` and the two-verdict line reports `field=in-place`. Cross-function
   forms (Case B `add_type`, Case V `visit`) are gated on Phases 5–6 and are
   **not** required here.
2. **Field-sensitivity** — a shared sibling field does not demote the sound
   field; the shared sibling shows no `Unique` fact while the sound field does.
3. **Nested inner** — a locally-fresh `Vector<Vector>` with all-owned inner shows
   `[.f, Elem]:Unique`; appending/storing a shared inner **drops** it and the
   verdict names the reason.
4. **Projection move/borrow** — whole-record last-use or quartet shell-writeback
   proof → R `[]:Unique`, base's `[.f]*` removed; borrow/no proof → R `[]:Shared`
   **and** base's `[.f]*` cleared (both demoted).
5. **Demotion cascade** — publishing / aliasing a shell clears its field facts
   (branch_env-shape negative).
6. **Downward-closed invariant** — no fixture ever exhibits a `[.f]*` fact while
   `own` for that local is non-`Unique` (checked in the suite).
7. **Alias-creation guards** — duplicate-store aggregates (`.{ a: x, b: x }`,
   `Vector.make(n, v)`) yield no deep fact; a consuming op storing a shared value
   drops the nested path.
8. **Determinism** — `twk ir --cfg` (facts + verdicts) byte-identical across two
   builds.
9. **No regression to in-place** — `twk ir --census` still shows **0 in-place**
   (Phase 4 changes no codegen).
10. **Full verification** — `make boot-test` green and `make stage2` reaches the
    self-host fixed point (Phase 4 is boot-only and adds no stage0-parity
    construct).

## Deferrals and tracking

| Item | Home |
|---|---|
| Path-granular liveness (transport-wrapper `out.ctx` while `out.ty` read) | Phase 5 ([summary-specialization.md](summary-specialization.md)) |
| Interprocedural field-path summaries / `in_place_paths` / `OwnedFromParam` return paths | Phase 5 |
| Intraprocedural variant-payload ownership (`AVariant` deep facts) | Phase 5 (needs a payload `PathSeg`) |
| Ownership specialization over field-path keys | Phase 6 |
| Candidate verdicts → decision records → in-place emission | Phases 7–8 ([../codegen/README.md](../codegen/README.md)) |

## Determinism-sensitive spots (lock with tests)

- **`PathKey` encoding** — canonical and collision-free over the seg sequence; the
  determinism test (Acceptance 8) gates it.
- **`field_own` map iteration** — never drives an order-sensitive result (the join
  is a commutative per-path meet); any rendered/compared output is keyed by a
  sorted `(LocalId, PathKey)` list, not raw `Dict` iteration.
- **`PathKey → AccessPath` side table** — populated in deterministic
  path-construction order so rendering is byte-stable.
