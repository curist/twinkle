# Phase 8C — Builder-Region Lowering (Design)

**Status:** Design — pending review. **Date:** 2026-07-22. **Rev 2** (incorporates review:
region-record decision instead of a per-local boolean, CFG-edge safety fact with publication
edges, concrete void-call ANF shape, typed-vector `builder_from` hazard, builder-specific
candidate roots).

This is the design for [codegen Phase 8C](README.md#codegen-phase-8c--existing-builder-region-lowering).
It supersedes the draft slice notes in [string-lowering.md](string-lowering.md) and the
builder bullets in [vector-lowering.md](vector-lowering.md); those remain useful as
runtime-hook references. The decision-record shape follows
[handoff-contract.md](handoff-contract.md) (*"Builder decisions must name the region
boundaries and the helper sequence"*). The safety fact it introduces is analysis-track work
(see [../analysis/README.md](../analysis/README.md)).

## Problem

Builder-region rewrites (`opt/loop_builder.tw`, `opt/builder_region.tw`) were deleted in
`5d5ac090` as part of the clean-slate uniqueness redesign, together with the unsound
`opt/uniqueness.tw` / `opt/liveness.tw` they depended on. Today `String.concat` and
`Vector.append` accumulator loops compile fully persistent (allocate-per-iteration); only
`collect` still uses builders, via **semantic** lowering in `lower_core` (not an
optimization). Phase 8C re-introduces builder-region lowering, now driven by the sound
ownership facts the analysis track produces, reusing the surviving runtime builder hooks and
the `BuilderConfig` metadata in `boot/compiler/builder_family.tw`.

Unlike the call-swap families (8A/8B/8D/8E), a builder region is **not** a 1:1 op
substitution. Persistent:

```
acc = ""
for c in xs { acc = acc.concat(c) }
use(acc)
```

becomes a three-part structural change that introduces a builder local and moves code across
the loop boundary. It cannot be expressed as a per-site emit swap, which is why 8C is a
distinct region-shaped lowering rather than another call-swap policy flag.

## Decisions (forks)

### Fork 1 — First-slice scope

Options considered: string-loop only / vector-loop only / both loops / loops + straight-line
chains.

**Decision: both string and vector loop regions, in one implementation plan.** They share the
seed→step→freeze region shape and differ only by `BuilderConfig`. Straight-line concat/append
chains (the old `builder_region.tw` shape) are deferred to a later slice.

- **Pro:** the two loop families are near-identical; one pass parameterized over
  `BuilderConfig` avoids a duplicated near-copy and a second, redundant review.
- **Con:** slightly more surface in the first emitted-code change than a single family.
- **Why not straight-line now:** different (non-loop) shape, more detection surface, no shared
  loop machinery.

**Empty-seed-only sub-scope (revised after review #1/#2).** The first slice covers **empty-seed
accumulators only**, both families, and the emitted helper sequences are fixed:

- **String** (`acc = ""`): `string$builder_from("")` → `builder_extend*` → `builder_freeze`
  (`builder_from("")` copies nothing; matches the `from`-only `string_builder_config`).
- **Vector** (`acc = []`): `vector$builder_new` → `builder_push*` → `builder_freeze`, **and no
  other vector builder op** — in particular **never `vector$builder_from`** in this slice.

**These vector regions stay boxed (they are NOT typed-routed), and that is expected.** An
earlier draft wrongly claimed empty-seed vectors would type-route like `collect`. They do not:
the rewrite rebinds the frozen accumulator with `AAssign(acc, freeze_local)` (matching the old
`builder_region.tw` shape), and `route_typed_vec`'s `build_copy_map` follows **only**
`AInit(.ASlot(src))` copies (`route_typed_vec.tw:1042`) — `AAssign` is not a copy edge. So the
freeze result's v-group escapes, the candidate is dropped, and the whole `builder_new`/`push`/
`freeze` lineage stays boxed. This is **correct** (boxed vectors are always sound), just not the
typed-unboxing win. It also means the typed-`builder_from`-mix hazard cannot arise here — the
region never routes typed — but non-empty seeds are deferred regardless.

- **The win breakdown:** string is the primary win (persistent `String.concat` accumulation is
  O(n²) → builder is O(n)); boxed vector builder is a **modest** allocation win over persistent
  `Vector.append` (in-place tail mutation vs per-append spine copy), not the typed-unboxing win.
- **Deferred to committed plans (see roadmap):** (1) non-empty seeds — **Plan 3** (string is
  free once the detector is relaxed, since `builder_from` copies; vector after a one-line
  `builder_push` shared-trie check); (2) **typed routing** of builder-region accumulators, the
  real vector win — **Plan 4** (teach `route_typed_vec` to follow the freeze→accumulator `AAssign`
  as a copy edge, or a `Let`-binding rewrite).

### Fork 2 — Where the transform lives

- **(A) ANF-to-ANF rewrite pass, fact-gated.** *(chosen)*
- (B) Emit-time region emission driven by a whole-region decision record.

**Decision: (A) — an ANF-to-ANF rewrite pass driven by decision records.**

- **Pros:** reuses the structural approach of the deleted `loop_builder.tw`; a region is
  naturally an IR transform (hoist seed, rewrite steps, append freeze); the emitter stays
  per-op and simple.
- **Cons:** it *mutates* the ANF, so downstream fingerprint-keyed artifacts (the call-swap
  staleness guard) must be recomputed on the rewritten ANF — see [Pipeline ordering](#pipeline-ordering).
- **Why not (B):** emit is structured per-op; reconstructing a hoisted seed plus a trailing
  freeze from a single record at emit time is awkward and would special-case the loop boundary
  inside the emitter. Rejected.

### Fork 3 — The gate (what licenses the rewrite)

Ownership uniqueness alone is **necessary but not sufficient**. A provably-unique accumulator
that is also *observed* mid-region must **not** be rewritten, because a builder does not
materialize until `freeze`:

```
for c in xs {
  acc = acc.concat(c)
  log(acc)          // reads the materialized accumulator mid-loop — would see stale
}
```

- **(A)** Ownership verdict (`reusable_shell`) **plus** a structural "clean fold" check inside
  the rewrite pass.
- **(B)** A region-level safety fact from the analysis track, consumed as a decision record.
  *(chosen)*

**Decision: (B) — the analysis proves region safety; codegen consumes a decision record.**

- **Pros:** a single source of truth for soundness; the rewrite becomes a mechanical
  application of a validated record. A detection bug becomes a *missed optimization*, never a
  miscompile. Keeps the "codegen consumes decisions, does not re-prove" invariant literal.
- **Cons:** reopens the analysis track (Phase 6 was marked complete); any change must preserve
  the existing 8A/8B/8D/8E byte-identical and self-host guarantees. *(Accepted: the analysis is
  the correct home for this reasoning.)*
- **Why not (A):** the safety check would be load-bearing for soundness inside codegen —
  exactly the fragile arrangement to avoid.

**Correction from review #1 — the record, not a boolean.** An earlier draft proposed a
per-local `linearly_folded: Bool` on `SiteVerdict`. That is too weak: a bare `func#local`
boolean cannot distinguish multiple loops over the same accumulator local, multiple candidate
freeze points, the same local reused across independent regions, or a detector that finds a
*different* shape than the analysis proved — leaving codegen detection soundness-critical, the
opposite of this fork's intent. The decision is therefore a **region record** naming every
boundary and the helper sequence, matching `handoff-contract.md`.

## The safety fact (analysis track)

`linearly_folded` is defined **over CFG edges of a named region**, not as a per-local flag.
For a candidate region (accumulator local `a`, seed site, loop, fold-step sites, freeze/exit
points) it holds iff **all** of:

1. **Seed uniqueness — dormant guard for the current copy-families.** In principle the seed must
   be unique / at last use if `builder_from`/`builder_new` could *thaw or reuse* the seed's
   storage. **Verified: neither current runtime builder thaws.** `string$builder_from`
   (`str.tw:493`) `ArrayNew`s a fresh buffer and `ArrayCopy`s the base in; `vector$builder_from`
   (`arr.tw:4705`) copies the base tail and structurally shares the immutable trie. So for the
   copy-families this condition is **trivially satisfied regardless of seed aliasing** — it does
   **not** gate non-empty seeds (see Plan 3). It stays documented as a guard that a *future*
   thaw-style/mutable-ABI builder (migration/storage track) would have to re-establish.
2. **Carried uniqueness.** `a` is unique across every entry and back-edge predecessor of the
   loop (the 8B carried-uniqueness property, including multiple back-edges and nested loops).
3. **Linear fold with no interior observation.** Each value `a` takes on is consumed exactly
   once — by a fold step or the freeze — with no other read or join of `a` anywhere inside the
   region. **This is entirely an analysis obligation** (review #4): the certified region records
   the exact set of fold-step sites, and the analysis guarantees no other materialized use of `a`
   survives. Codegen does **not** re-derive this on any path; it only checks that the ANF still
   matches the recorded fold-step set (see Component 3).
   **First-slice narrowing (review #4):** the first slice accepts **unconditional simple loop
   folds only** — one fold step per iteration on the loop's main path, no conditional/skipped
   folds and no `continue`-guarded steps. Conditional and `continue` folds (which need
   path-sensitive join reasoning) are deferred to a later slice; until then such candidates are
   rejected, keeping both the analysis fact and codegen validation simple and non-path-sensitive.
4. **Single post-loop freeze — reject all intra-region publication/early-exit (first slice).**
   The first slice supports **only** regions whose observable exits *all* flow through the
   single post-loop freeze. Any intra-region edge that exits or publishes `a` before that freeze
   — `return a`, value-carrying `break`, `try` early-return, closure capture, a store, an
   unknown call, task/channel publication, or any other escape — **rejects the region**. This
   keeps the concrete rewrite (one `builder_freeze` after the loop) sound. Per-exit freeze
   insertion — enumerating a freeze point on each publishing edge in `BuilderRegionDecision` —
   is deferred to a later slice; it is the only way multi-exit regions become eligible.
5. **No self-alias in the fold step (self-concat rejection).** The chunk argument must not be
   the accumulator, and analysis must not have to assume otherwise: `acc = acc.concat(acc)` or a
   chunk that aliases the current accumulator is not a clean fold. The old matcher rejected this
   cheaply via `!atom_is_local(args[1], base)`; keep that literal guard and reject any chunk the
   analysis cannot prove disjoint from `a`.

The analysis surfaces, per certified region, the data the producer needs to build a record:
the region boundaries above plus the proof/debug id. It is fingerprinted by the same
`ArtifactKey` as the other ownership artifacts.

## Architecture

### Component 1 — Analysis: region safety facts

Extend the ownership analysis (`compiler.ownership` / `compiler.summary` over `compiler.cfg`
block-exit facts) to certify candidate builder regions per the **five conditions** above, over
CFG edges. All load-bearing path-sensitive reasoning (linear fold, no interior observation,
freeze-before-publication) lives **here**, not in codegen. Surface certified regions (boundaries
+ proof id) via a region-fact module (`boot/compiler/builder_region_fact.tw`) that **consumes**
`ownership_verdicts.tw`'s fingerprinted `OwnershipArtifacts`: the region's fold sites carry a
`fold_reusable` fact recorded in the existing `block_verdicts` pass (keyed `"${func_id}#${local}"`,
covered by the existing `ArtifactKey`), which the region fact composes with the structural
conditions into a per-region verdict keyed by region identity (not a bare local). It lives in its
own module rather than as a `SiteVerdict` field to keep the dependency acyclic (the fact already
depends on `ownership_verdicts`), preserving the design intent — a fact surfaced through a
fingerprinted-artifact consumer with the same staleness discipline. This is the **sole legality
authority**.

### Component 2 — Codegen producer: `BuilderRegionDecision` records

A producer (codegen-track, sibling to `mutable_produce.tw`). The pipeline is the same
non-circular staging the update-call producer uses (detection is a cheap ANF walk with **no**
ownership; ownership runs *after*, scoped to the detected roots):

```text
collect_builder_region_candidates_and_roots(opt)   // ANF walk only, no ownership
  → compute_candidate_artifacts(opt, roots)         // scoped linearly_folded facts
  → produce_builder_region_decisions(opt, artifacts)
  → rewrite certified regions
```

- **Detects** region candidates — empty-seed rebind of an accumulator, an unconditional fold
  step (`acc = acc.concat(chunk)` / `acc = acc.append(elem)`) carried through a loop, the
  post-loop use — and derives **builder-specific candidate roots** (the functions containing
  candidates) for scoping, *not* the update-call roots. Detection uses no ownership facts.
- **Joins** each candidate with the analysis region-safety fact.
- **Emits** a `BuilderRegionDecision` for each certified region. **Region identity** is a
  deterministic `BuilderRegionKey` = `(func_id, seed_site, loop_site, sorted fold_sites,
  freeze_site, family)`; equal keys denote the same region across runs. The record names those
  boundaries plus the helper sequence (`builder_new`/`builder_from("")` →
  `builder_push`/`builder_extend` → `builder_freeze`), the fresh builder/freeze/assign locals,
  the `ArtifactKey`, and a proof/debug id. Uncertified candidates emit no record.
- **Non-overlap rule:** if two accepted candidate regions share a fold site, loop, or the same
  accumulator local across overlapping ranges, **reject all but one deterministically** (lowest
  `BuilderRegionKey`) so the rewrite never double-transforms a site.
- **Fresh-local allocation is centralized:** all fresh locals for all accepted regions in a
  function are drawn from a single counter seeded at the function's max local id, not chosen
  independently inside each record. This guarantees no collisions across multiple/nested regions.

Detection lives here and is **not** trusted for soundness: an over-proposed candidate with no
certifying fact yields no record.

### Component 3 — Codegen rewrite pass

`boot/compiler/codegen/builder_region.tw`, parameterized over `BuilderConfig`. It consumes
`BuilderRegionDecision` records and its validation is **purely structural** (review #4): it
checks the recorded ANF ids, region shape, helper sequence, `ArtifactKey`, and **exact fold-site
coverage** (the ANF still contains exactly the recorded fold sites and nothing else references
`a`), then applies the mechanical transform. It performs **no** path-sensitive proof — any
mismatch → persistent.

**Concrete ANF rewrite shape (from the deleted `loop_builder.tw`, review #5).** Builder helper
steps are **void-returning and effectful** (`builder_extend(builder, chunk) -> void`,
`builder_push -> void`); the builder ref is created once and is **not** reassigned per
iteration. The transform, per certified region:

- Seed (empty only): insert `builder = string$builder_from("")` (string) or
  `builder = vector$builder_new()` (vector) before the loop, into a fresh `builder_local`.
  **Never `vector$builder_from` in this slice** (see the empty-seed sub-scope note).
- Each fold step: replace the `concat`/`append` `ACall` with a void
  `ACall(builder_push/builder_extend, [builder_local, chunk])`, and **neutralize** the old
  `acc = result` binding to `AInit(.ALitVoid)` (the accumulator is no longer materialized
  inside the loop).
- After the loop: `acc = builder_freeze(builder)` via fresh `freeze_local` / `assign_local`,
  rebinding the original accumulator with `AAssign(acc, freeze_local)`. This is the **single**
  freeze point; the record certifies (condition 4) that no publishing edge precedes it. (This
  `AAssign` is why vector regions stay boxed — see the empty-seed sub-scope note.)
- Update `op_result_mono` / local type metadata:
  - **`builder_local` is a physically erased builder handle, NOT accumulator-typed.**
    `string$builder_from` returns `rt_types__StrBuilder` and `vector$builder_new` returns the
    erased `Array` handle (`is_builder_seed` covers both, and emission stores the raw handle with
    no result adaptation). The slot repr must therefore be `OpaqueAnyref`/anyref, not `String` /
    `Vector<T>`. Vector seeds already get this because `repr_assign` hardcodes the vector seed ids
    (`repr_assign.tw:46-47,151`) and overrides their result slots to `OpaqueAnyref`; **`string$builder_from`
    is NOT recognized there**, so binding a `StrBuilder` into a `String`-typed slot would be
    verifier-invalid. **Plan 2 must extend `repr_assign`'s builder-seed recognition to include
    `string$builder_from`** (mark its result slot `OpaqueAnyref`), or allocate `builder_local` via
    an explicit erased/anyref metadata path. Whatever mono is attached to `builder_local` is moot
    once the seed is repr-erased, but the design intent is "erased handle," never the accumulator
    type.
  - **`freeze_local` takes the accumulator's mono type** (`builder_freeze` returns `String` /
    `Vector<T>`); this is the only builder-region local that carries the accumulator type.
  - Only the neutralized `AAssign` result local is forced to `Void`.
  - **The fold-call result local keeps its original mono** — it is dead after neutralization, and
    emission already handles a void builder push bound to a now-dead non-void local via the
    `is_builder_void_push` placeholder path (`emit/runtime_abi.tw:39-47`, `emit/calls.tw:398`),
    synthesizing a placeholder before the `local.set`. No mono rewrite is needed there.
  - Fresh locals come from the centralized per-function counter (Component 2), not an independent
    `next_local` bump.
- Coverage guard: the count of rewritten push sites must equal the recorded fold-step count, or
  the region is abandoned (persistent). This is a structural equality check, not a re-proof.

## Pipeline ordering

The rewrite *mutates* the ANF, so it runs **before** the call-swap producer, whose
stale-artifact guard keys off an ANF fingerprint. New step at the top of `link_program`
(`boot/compiler/codegen/codegen.tw`), before closure conversion:

1. `optimize_module` → optimized ANF (unchanged; happens in the caller).
2. **NEW:** builder-region producer + rewrite, consuming region-safety facts computed on the
   optimized ANF → **ANF′**.
3. `produce_update_call_decisions` (8A/8B/8D/8E) runs on **ANF′**; its ownership artifacts are
   recomputed against ANF′'s fingerprint, so nothing goes stale.
4. Closure conversion → `prepare_backend_with_mutable_config` → verify → emit, all on ANF′.

Both ownership computations (region safety for the rewrite; `reusable_shell` for call-swap)
stay cheap via candidate-scoping — the builder producer supplies its own roots.

## Scope

**First slice (Plans 1–2):** loop-carried, unconditional-fold, empty-seed accumulators —
`String.concat` (`builder_from("")`) and `Vector.append` (`builder_new`, boxed).

**Deferred to committed later plans (not open-ended — each has a concrete path in the roadmap
below):** non-empty seeds (Plan 3), typed vector routing (Plan 4), conditional/`continue` folds
(Plan 5), multi-exit per-edge freeze (Plan 6), straight-line chains (Plan 7).

**Permanently out (never a decision-driven target):** `collect` and any semantically-required
builder lowering — untouched; those live in `lower_core` and must keep working with **no**
ownership decision.

## Soundness invariants

- Codegen consumes region decisions; it re-proves nothing (uniqueness, read-position, exit
  materialization). Its validation is purely structural (recorded ids/shape/coverage).
- Absence, non-certification, shape mismatch, or staleness → persistent, builder-free lowering.
- First slice **rejects** any region with an intra-region publication/early-exit before the
  single post-loop freeze (it does not attempt to materialize before publication).
- Vector regions stay boxed in the first slice (correct, not typed); typed routing is deferred.
- `collect` and other semantic builder uses never depend on an ownership decision.

## Inspection gate (before emitted-code change)

Matching the 7E / 8A precedent of a printed dry-run before any emission, add an inspection
render (via `twk ir --census --sites`, alongside the existing mutable-decision audit) that lists
per candidate region: certified vs rejected, the `linearly_folded` result, the region boundaries
(`BuilderRegionKey`), the helper sequence, the proof/debug id, and the fallback reason for
rejected candidates. This lands in Plan 1 (rejected/certified regions render before any rewrite
exists) and is extended in early Plan 2 to show which certified regions the rewrite consumed.

To match `handoff-contract.md` literally (as 7D→8A did for the call-swap seam), the **first
step of Plan 2 wires the producer + decision records through the backend with the rewrite
disabled** — a decision-consumption dry-run that renders which regions *would* be rewritten and
verifies the output stays byte-identical, before the rewrite actually mutates any ANF. Only then
is the transform enabled.

## Testing

Analysis (Plan 1):

- `linearly_folded` true for a clean unconditional fold; false for (a) interior read of the
  accumulator, (b) aliased/escaping accumulator, (c) non-last-use of a carried value.
- **Publication-edge negatives (one test per named edge):** `return acc`, value-carrying `break`,
  `try` early-return, closure capture, a **store into a record field / global / `Cell`**, and a
  **task/channel publication** inside the region each **reject** (first slice does not materialize
  before publication — it rejects).
- **Self-alias negatives (condition 5):** `acc = acc.concat(acc)` and a chunk that aliases the
  current accumulator both **reject**.
- **First-slice-deferral negatives:** conditional step (`if keep(c) { acc = acc.concat(c) }`),
  `continue`-guarded steps, and non-empty seeds all **reject** in the first slice.
- **Nested loops / multiple back-edges** for the carried-uniqueness condition.
- **Same accumulator local across multiple independent regions** — each keyed independently by
  `BuilderRegionKey`.
- **Scoped-vs-full equivalence:** scoped-root artifacts produce the same certifications as
  whole-program artifacts (mirrors the update-call producer's equivalence test).

Rewrite (Plan 2):

- String and vector clean-fold loops emit `string$builder_from("")`/`vector$builder_new` →
  `builder_extend`/`builder_push` → `builder_freeze` (verified via `twk wat --calls` and
  `twk ir --census --sites`); negatives stay on persistent `String.concat` / `Vector.append`.
- **Typed `Vector<Int>` accumulator-loop fixture** asserts the *actual* routing outcome: in the
  first slice it stays **boxed** (`vector$builder_*`, not `*_i64`), documenting that typed routing
  of the `AAssign`-rebound accumulator is a deferred follow-up. Flip this fixture to assert `*_i64`
  only when that follow-up lands.
- **Multiple eligible loops in one function**, **nested candidate loops**, and **overlapping
  candidate rejection** — fresh locals never collide (centralized allocation); overlaps reject
  deterministically.
- **Stale region-boundary decision** (recorded region no longer matches the ANF) → persistent
  fallback (distinct from stale-artifact fallback).
- **Call-swap decisions recomputed on ANF′** after the builder rewrite (8A/8B/8D/8E sites still
  emit correctly, no staleness regression).
- **`collect` remains decision-free** — its semantic builder lowering is unchanged.
- Round-trip: each rewritten program produces identical results to its persistent form.
- Self-host fixed point (stage3 == stage4); full boot suite green; census 0 and byte-identical
  where expected (Plan 1 must be emission-invisible).

## Implementation plans

Each plan below is a committed unit of work with a concrete enabling change and acceptance test,
not an open-ended "later." Plans 1–2 are the first slice; Plans 3–7 are sequenced follow-ups,
each small and independently shippable.

- **Plan 1 — analysis region-safety facts + inspection.** CFG-edge `linearly_folded` over named
  regions (five conditions; unconditional folds only) + surfacing certified regions through
  `ownership_verdicts.tw` + the certified/rejected inspection render. No emitted-code change;
  byte-identical guarantee holds. Tests: publication-edge negatives and first-slice-deferral
  negatives.
- **Plan 2 — producer + rewrite pass (string + vector empty-seed) + pipeline wiring.** The
  non-circular candidate/roots/artifacts/produce staging, `BuilderRegionDecision` records with
  `BuilderRegionKey` + non-overlap + centralized fresh-local allocation, the ANF-to-ANF rewrite
  with the concrete void-call shape and structural-only validation, and ANF′ ordering.
  **Backend prerequisite:** extend `repr_assign`'s builder-seed recognition to mark
  `string$builder_from` result slots `OpaqueAnyref` (it currently recognizes only the vector seed
  ids), so string `builder_local`s are erased handles rather than `String`-typed slots. First
  emitted-code change. Tests: **a string builder region that actually emits and passes backend
  verification / WAT inspection** (guards the repr_assign fix); inspection + round-trip + negative
  + stale-region + multi/nested/overlap + boxed-`Vector<Int>` fixtures; scoped-vs-full
  equivalence; self-host fixed point.
- **Plan 3 — non-empty seeds (string now-free; vector after a `builder_push` check).**
  - *String:* relax the seed detector to accept any seed local; the rewrite is **identical**
    (`builder_from(seed)`), and **no new proof is needed** — `str.tw`'s `builder_from` copies
    (condition 1 is vacuous for it). Test: `acc = prefix; for … { acc = acc.concat(…) }` emits the
    builder sequence and round-trips.
    *Enabling change:* detector only. *Path is clear and verified.*
  - *Vector:* first confirm `vector$builder_push` never mutates a **shared** trie node in place
    (the boxed `builder_from` shares the base's immutable trie root). If confirmed, relax the
    vector seed to `builder_from(base)` (stays boxed). Test: non-empty vector accumulator
    round-trips and the base value is observably unchanged.
    *Enabling change:* one runtime read + detector relax. *Gated on a single named check.*
- **Plan 4 — typed routing of builder-region accumulators (the real vector win).** Two viable
  paths; the copy-edge one requires **two** coordinated changes, not one:
  - *Copy-edge path:* teach `route_typed_vec` to treat a certified freeze→accumulator
    `AAssign(acc, freeze_local)` as a copy — this means updating **both** `build_copy_map` (which
    today tracks only `AInit(.ASlot)`) **and** the escape classifier
    (`route_typed_vec.tw:1589`: `.AAssign(dst, a) => use_esc(slot_in(a, vs) or vs.has(dst.id))`
    currently demotes any v-group `AAssign` to boxed). Both must recognize the certified copy or
    the v-group still escapes.
  - *Let-binding path (preferred):* switch the rewrite to `Let`-bind the freeze result and rewrite
    downstream `acc` uses to the new local, avoiding `AAssign` entirely so the existing copy/escape
    logic routes it unchanged.
  Test: flip the boxed-`Vector<Int>` fixture from Plan 2 to assert `*_i64` builder calls.
  *Enabling change:* copy-map **+** escape classifier, or the Let-binding rewrite. *Concrete,
  localized.*
- **Plan 5 — conditional / `continue` folds.** Extend the analysis fact with path-sensitive join
  obligations (builder state flows on every continuing edge where the persistent step would);
  record the per-branch fold sites. Test: `if keep(c) { acc = acc.concat(c) }` and
  `continue`-guarded folds certify and round-trip.
- **Plan 6 — multi-exit regions (per-edge freeze).** Extend `BuilderRegionDecision` to enumerate
  a freeze insertion point on each publishing edge; lift condition 4 from reject-all to
  materialize-before-publication. Test: `return acc` / value-carrying `break` inside the region
  freezes on that edge and round-trips.
- **Plan 7 — straight-line concat/append chains.** The non-loop region shape (old
  `builder_region.tw`); separate detector, same decision-record + rewrite machinery.
