# Phase 5 Implementation Design — Transport-wrapper and `Result`-payload Return-path Summaries

**Status:** Draft design (feeds the Phase 5 execution plan)

This is the implementation design for **Phase 5** (architecture **1D**) of the
sound-uniqueness track: extend the whole-parameter / whole-return summary layer
(Phases 2–4) with **return-path ownership** — the ownership handed back through a
returned *record field* (`out.ctx` / `out.state`) or a *variant payload*
(`Ok[0].state`) — plus the caller-side machinery that recovers it. This stops the
compiler's source-wide transport-wrapper and `Result`-payload state threading from
classifying as aggregate publication. It remains **analysis-only**: no codegen,
decision records, or in-place lowering (Phases 7–8), and no ownership
specialization or per-parameter `in_place_paths` (Phase 6).

Canonical semantics:
[summary-specialization.md](summary-specialization.md) (return-path summaries,
transport-wrapper returns, variant-wrapped transport returns, the field-projection
move), [worked-examples.md](worked-examples.md) (Cases **W**, **R**, and the
`try`/early-exit Case **T**), and [records-fields.md](records-fields.md) (the
shell-vs-deep split this reuses). It implements the Phase 5 bullets in
[README.md](README.md). On any genuine conflict the canonical docs win and this
doc is corrected.

## Current state (verified against the branch)

Established before designing, because it fixes what Phase 5 extends and what it
must not disturb:

- **The summary schema is whole-parameter + whole-return.** `Summary = .{ params:
  Vector<ParamSummary>, ret: ReturnEffect }` (`ownership.tw:47`); `ReturnEffect =
  { OwnedFresh, MayAliasParams(Vector<Int>), Shared }` (`ownership.tw:45`). There
  is **no per-field or per-payload return classification** anywhere.
- **`summarize_function` classifies the whole return only** (`ownership.tw:2316`):
  per return block it joins entry facts, runs `forward_block_body`, then classifies
  the returned atom via `prov_to_indices` (→ `MayAliasParams`) or `fact_of` (→
  `OwnedFresh`/`Shared`). The post-return `ForwardState` (`body`) and the returned
  atom are already in hand at that site — the natural hook for return-path facts.
- **The caller consumes summaries in `transfer_summarized_call`**
  (`ownership.tw:976`): Retained args publish, Borrowed args are untouched, and the
  **result takes a whole-value fact** (`OwnedFresh`→Unique, `Shared`→Unknown,
  `MayAliasParams`→Shared+prov). The result **carries no `field_own`** today, and
  `Consumed` capability is deliberately *recorded-only, never read* here (the
  comment marks binding invalidation as Phase 6).
- **The field-projection move already exists intraprocedurally** in `ARecordGet`
  (`ownership.tw:1107`): it moves a deeply-owned field to `R` when `base` is
  **whole-record last-use** *or* **quartet-licensed** (`quartet_has`), removing the
  `[.f]*` subtree from `base`; otherwise it borrows and demotes both sides. It only
  fires on facts the *local* analysis established — a call result carries no
  `field_own`, so transport wrappers never reach it today.
- **Path liveness is whole-record.** Phase 4 explicitly deferred path-granular
  liveness. The **quartet recognizer** (`quartet_ok` `ownership.tw:242`,
  `recognize_quartet_moves`, surfaced via `BlockPrep.moves` `ownership.tw:511` and
  consumed as the `quartet` set in `transfer_op`) is the block-local peephole
  precedent Phase 5's transport recognizer mirrors.
- **`field_facts.tw` is payload-free.** `PathSeg = { Field(Int), Elem, Val }`
  (`field_facts.tw:12`); the reversible `PathKey` codec is depth ≤ 2
  (`field_facts.tw:82`). `AVariant`'s transfer is **shell-only**
  (`ownership.tw:1073`) — Phase 4 left variant-payload ownership for Phase 5,
  noting it "needs a payload `PathSeg`".
- **Match arm pattern-bound locals are killed at entry and seeded Unknown**
  (`ownership.tw:499`): a destructured payload is treated as a borrow from the
  scrutinee with no facts flowing in. This is exactly the seam Case R must open.
- **Field facts join by per-path meet** (`join_entry_field_own`
  `ownership.tw:1776`, over `FieldMap.merge` `field_facts.tw:211`), skipping
  unprocessed back-edges; `set_own_st` (`ownership.tw:642`) is the single choke
  point that clears `field_own` whenever a shell leaves `Unique`.
- **`prov` is per-*local*, not per-path, and conflates field origins.** `ARecord`
  (`ownership.tw:1058`) unions **every** field value's origins into the record
  local's single `prov`, so a fresh `.{ state: p0, accum: p1 }` carries `prov =
  {0,1}` on the shell and **cannot** attribute `[.state]→p0` vs `[.accum]→p1`.
  Path-attributed provenance is a **Phase 5 prerequisite** (Blocker 2), not
  something today's `prov` supplies.
- **`Return` publishes the returned value at the block exit** (`forward_block`
  `ownership.tw:1394` calls `publish_atom(a)`), which — because the shell `prov`
  conflates field origins — transitively marks a returned wrapper's field-origin
  params `Shared` ⇒ `Retained`. For a transport wrapper this is exactly the
  aggregate-publication behavior Phase 5 must remove: the caller would then publish
  the very argument the return path wants to hand back (Blocker 3). Return
  classification itself (`ret`) is prov-based (`prov_to_indices`, `ownership.tw:2335`)
  and runs on the **body-only** state (before this publish), so it is unaffected by
  removing the publish.

## Scope

**In scope (Phase 5):**

- A **return-path summary layer**: classify a returned record's owned *fields*
  (`[.ctx]`, `[.state]`, `[.accum]`) and a returned variant's owned *payload paths*
  (`Ok[0]`, `Ok[0].state`, `Err[0].state`) as `OwnedFresh` or `OwnedFromParam(k)`,
  stored **additively** alongside the existing whole-value `ret`.
- **Intraprocedural variant-payload ownership** — the payload `PathSeg` the Phase 4
  deferral named — so `AVariant` can carry a deep payload fact and the return
  classifier / match-arm projection can read it.
- **Caller-side return-path recovery** in `transfer_summarized_call`: attach the
  return-path facts to the call result (record-field return paths populate the
  result's `field_own`; variant-payload return paths are recovered at the match
  arm), gated soundly on the argument's ownership for `OwnedFromParam(k)` paths.
- **The transport-wrapper projection move** — a **path-aware** block-local
  recognizer that licenses `x = out.f` to move `out.f` even while `out` stays live
  for **sibling** reads (`out.ty`), the finer liveness Phase 4 deferred.
- **Variant-payload projection at match arms + handled-`Result` joins**: a
  pattern-bound payload local receives the scrutinee's `Variant[i]` return-path
  facts; ordinary arm joins merge transported payload facts like record-field
  facts; `try` / `return` / value-carrying `break` stay **publishing** exit edges
  (Case T unchanged).
- **Rendering**: the return-path summary in the `twk ir --cfg` header, and the
  projection-move verdict at transport sites.

**Out of scope (deferred):**

- **No parameter-side field-path summaries.** `in_place_paths` and per-path
  `Consumed` granularity are **Phase 6**; Phase 5 leaves `ParamSummary` at its
  Phase 3/4 whole-parameter shape and **does not invalidate any caller binding**
  (the `transfer_summarized_call` invariant holds).
- **No ownership specialization / variant selection / call-site variant decision**
  → Phase 6.
- **No general per-path liveness** beyond the bounded transport-wrapper shape.
- **No codegen, decision records, or in-place emission** → Phases 7–8.
- **No deep return paths.** Return paths are capped at one field under the returned
  record or one field under a variant payload (`Ok[0].state`); deeper nesting is a
  sound under-claim (dropped), consistent with Phase 4's depth-2 field cap.

Guiding rule (unchanged): **soundness before coverage.** Every new rule
under-claims when unsure; a return-path fact is `OwnedFromParam`/`OwnedFresh` only
when proven, and any doubt drops it (the worst case is exactly today's aggregate
publication).

## Worked target — what "classify correctly" means

**Case W (record transport):**

```tw
out := helper(ctx, x)   // summary: helper returns [.ctx] = OwnedFromParam(0)
ctx = out.ctx           // MOVE: ctx recovers [.ctx]'s ownership (unique)
ty  = out.ty            // sibling read; does NOT block the move
```

Exit fact: after the block, `ctx` is `Unique` (state thread preserved), not
`Shared`. Requires: (1) the return-path summary `[.ctx]=OwnedFromParam(0)`; (2) the
caller passed `ctx` (arg 0) `Unique`+last-use, so `out.ctx` is soundly unique; (3)
the transport-wrapper recognizer licenses the `out.ctx` projection move even though
`out` is live for `out.ty`.

**Case R (`Result`-payload transport):**

```tw
loaded := case acc.state.load_source(...) {   // Ok[0].state / Err[0].state = OwnedFromParam(0)
  .Ok(v)   => v,
  .Err(err) => return acc.record_failure(canonical, err),   // Case T publish edge
}
acc.state = loaded.state
```

Exit fact: on the handled `.Ok` arm, `v` recovers the payload facts (`v.state`
`Unique`), so `loaded.state` stays `Unique`; the `.Err` arm `return`s — a CFG leaf
that contributes **nothing** to the join (Case T), so it cannot corrupt the `.Ok`
arm's transported state. Requires: (1) variant-payload return summary; (2) match-arm
payload projection seeding `v` from `Ok[0]`'s facts; (3) arm joins merging
transported payload facts, with `try`/`return` arms as leaf exits (Decision 3).

## Locked decisions

Confirmed in brainstorming or forced by soundness / the canonical docs. Genuinely
open choices are in **Open decision forks** below (for the end-of-doc review).

| # | Decision | Choice |
|---|---|---|
| 1 | `ret` = shell ownership; `ret_paths` = deep ownership | `Summary` gains `ret_paths: Vector<ReturnPathOwn>`, but `ret` is **revised to mean shell-level ownership of the returned value** (Blocker 3): `OwnedFresh` = fresh shell (no *shell*-level param alias), `MayAliasParams(ks)` = the returned *shell itself* aliases params `ks` (e.g. `return x`), `Shared` = unknown. Deep field/payload ownership and aliasing move into `ret_paths`. A fresh wrapper `.{ ctx: p0 }` is `ret=OwnedFresh` **+** `ret_paths=[.ctx]=OwnedFromParam(0)`, not `MayAliasParams([0])`. Not purely additive — the shell/deep split is the point |
| 2 | Path-attributed provenance with a **three-way** reading (Blocker 2, +review pt 3) | Add `path_prov: Dict<Int, Dict<Int, Vector<Int>>>` (local → PathKey → origins) mirroring `field_own`; grafting a field/payload copies **that value's** origins to the path, and the **shell `prov` of a freshly-constructed aggregate becomes shell-level only** (no longer unions field origins). **Read via `Dict.get`, three-way:** `.None` (absent) = provenance **unknown** → conservative drop; `.Some([])` = proven **fresh** → `OwnedFresh`; `.Some([k])` = single origin → `OwnedFromParam(k)`; `.Some([multi])` = conservative (drop path + treat as aliasing). **Invariant:** every `Unique` `field_own` path has a `path_prov` entry (at least `Some([])`), so "absent" genuinely means unknown, never proven-fresh. `publish_local` publishes `path_prov` origins, so whole-value publication still leaks fields |
| 3 | Return is not callee retention, but still a leaf exit (Blocker 3, +review pt 2) | **Precise split.** (a) *For caller-visible escape:* remove `Return`'s terminator publication in `forward_block` (keep `ValueBreak`'s) — returning hands the value to the caller, whose `ret`/`ret_paths`+gate account for it, so a param handed forward stays `Borrowed` rather than falsely `Retained`. (b) *For intra-function joins / Case T:* a `Return` block is a CFG **leaf** (no successors), so its value never merges into a continuing-arm join — the `.Ok`/fallthrough arm is unaffected **structurally**, without any publish. This is why `ValueBreak` differs: its value flows to a **real successor** (post-loop join), so it still publishes. Genuine leaks (globals, closures, channels, unknown calls, escaping aggregates, aliasing) still mark `Retained` independently. **Deliberate change to Phase 3 escape behavior; re-baseline affected summary tests** |
| 4 | Tagged variant-payload segment (Fork 1a amended) | `PathSeg` gains `Payload(Int, Int)` = `(variant_tag, payload_index)` — **tagged**, so an `.Err` arm can never recover an `.Ok` payload fact (Blocker 1). Payload keys occupy a **disjoint negative `PathKey` range** via a reversible pairing (the positive range is unbounded field keys), locked by a round-trip test |
| 5 | `OwnedFromParam(k)` caller gate needs pre-call facts + `last`, and **publishes on failure** (Blocker 4, +review pt 1) | At the caller `OwnedFromParam(k)` refers to the argument atom `args[k]`. It yields a `Unique` result path **only when** `args[k]` is `Unique`+last-use at the call. **On failure it does not silently drop — it publishes `args[k]`** (the conservative `MayAliasParams`-equivalent): the returned value may carry a reference into `args[k]`'s region, so leaving `args[k]` `Unique` would be an unpublished alias. Publishing an already-`Shared` arg is idempotent, so the single rule covers both "unique-but-read-later" and "already-shared" failures. `OwnedFresh` is unconditional (no arg). `transfer_summarized_call` gains a `last` parameter; the gate reads **pre-call** arg facts before any escape/return handling. On success, the recovered path's `path_prov` at the caller is `prov_of(args[k])`, so a later publish of the result still leaks `args[k]`'s origins |
| 6 | No binding invalidation | Phase 5 does **not** flip `valid[arg_k]` for `OwnedFromParam` handoffs; Decision 5's last-use gate already makes the recovered path sound. Binding invalidation stays Phase 6 |
| 7 | Return-path depth cap | One field under the returned record (`[.f]`) or one field under a variant payload (`Variant[tag,i]`, `Variant[tag,i].f`). Deeper is dropped (sound under-claim) |
| 8 | Multi-accumulator returns | Support multiple independent owned return fields (`.{ state, accum }` → two `OwnedFromParam` paths); the schema is a `Vector` and per-path provenance (Decision 2) is what makes independent attribution sound |
| 9 | Path-aware liveness | **Bounded transport-wrapper recognizer** (Fork 2A), a block-local pre-scan in the spirit of the quartet recognizer, surfaced as a `BlockPrep` annotation the `ARecordGet` transfer consults — not general per-path liveness |
| 10 | Determinism | `ret_paths` canonical-sorted (variant tag, payload index, field id); the negative-range `PathKey` codec stays reversible + collision-proof; the transport recognizer is a pure function of block structure + the liveness scan |

## Decision forks (resolved at review — pros/cons kept as rationale)

**Status:** the design review resolved all three forks. Fork 1 → **1a-i + 1b-i,
amended** (tagged `Payload(tag,index)` segment, dedicated `ReturnPathOwn`,
negative-range codec) — now Decision 4 + Decision 1. Fork 2 → **2A** (bounded
transport recognizer) — Decision 9. Fork 3 → **3-i** (compositional match-entry
seeding, narrow to top-level `Var` payload bindings) with the `cfg.tw` metadata as a
named prerequisite. The pros/cons below are retained as the rationale record.

### Fork 1 — Representation of variant-payload ownership

Variant/`Result` handling needs a payload representation in **two** places that
answer different questions. Brainstorming's original "A vs B" framing conflated
them; digging into the code split it:

**1a. Intraprocedural payload facts (a specific variant local).** `AVariant`'s
result and a match-arm-bound payload local carry payload ownership; the variant tag
is *implicit in the local* (an `AVariant(Ok, …)` result is an `Ok`).

- **Option 1a-i — extend `field_facts.tw` `PathSeg` with a *tagged*
  `Payload(Int, Int)` = `(variant_tag, payload_index)`** and extend the codec.
  `AVariant(tag, …)` grafts `[Payload(tag, i)]`/`[Payload(tag, i), .f]`; match-arm
  projection reuses `FieldMap.project`. **The tag is mandatory (Blocker 1):** an
  untagged `Payload(Int)` would let an `.Err` arm recover an `.Ok` payload fact
  when both use payload index `0`.
  - *Pros:* one path machinery; `graft`/`project`/`remove_prefix`/`merge` reused;
    matches the Phase 4 deferral note ("needs a payload `PathSeg`"); tag keeps arms
    disjoint.
  - *Cons:* the codec must grow a **disjoint negative range** for payload keys (the
    positive range holds unbounded field keys `8 + f*4`), via a reversible pairing
    over `(tag, index, optional field)` — a real codec extension, not a slot in the
    existing range.
- **Option 1a-ii — a separate *tagged* per-local payload channel** on
  `ForwardState`, leaving `field_facts.tw` payload-free.
  - *Pros:* keeps the field-fact codec untouched.
  - *Cons:* a parallel path system with its own merge/join/clear/`path_prov`
    mirror; duplicates the demotion-cascade discipline `set_own_st` centralizes.
- **Recommendation: 1a-i (amended, tagged).** The payload segment is the exact
  thing Phase 4 deferred; a tagged segment in one codec avoids a second path system
  that would need the same join/clear/prov plumbing. The negative-range codec is
  the concrete cost and is locked by a round-trip determinism test.

**1b. The summary return-path schema (aggregated over return sites).** A summary
must distinguish `Ok[0].state` from `Err[0].state`, so it needs the **variant
discriminant** that an intraprocedural local does not.

- **Option 1b-i — a dedicated `ReturnPathOwn`** carrying the discriminant, reusing
  the field-tail encoding:
  ```tw
  type RetVia   = { Direct, Variant(Int, Int) }   // Direct record return | Variant(tag, payload_index)
  type ReturnOwn = { OwnedFresh, OwnedFromParam(Int) }
  type ReturnPathOwn = .{ via: RetVia, field: Int?, own: ReturnOwn }  // field=None ⇒ whole payload / return value's sub-shell
  ```
  - *Pros:* carries the discriminant the summary needs; small bounded structure
    computed once per function; independent of the intraprocedural codec.
  - *Cons:* a distinct type from `field_facts.AccessPath` (a little duplication at
    the record-field tail).
- **Option 1b-ii — encode the discriminant into a unified `field_facts` path.**
  - *Pros:* one type everywhere.
  - *Cons:* forces a discriminant onto intraprocedural facts that never need it,
    and complicates the reversible codec further.
- **Recommendation: 1b-i.** Summaries and intraprocedural facts genuinely answer
  different questions; a dedicated, discriminant-carrying `ReturnPathOwn` is the
  honest schema and keeps the intraprocedural codec lean.

### Fork 2 — Path-aware liveness precision

*(Brainstorming leaned 2A; restated here for the review.)*

- **2A — bounded transport-wrapper recognizer** (block-local peephole): prove the
  `out := call(...); x = out.f; …sibling reads…; out dead-after` shape and license
  the `out.f` projection move. Narrow, sound, matches the census-dominant idiom.
  - *Cons:* a projection whose consumer is in another block falls back to borrow
    (sound, but uncovered) — same scoping the quartet recognizer accepts.
- **2B — general `(local, path)` liveness** across the CFG.
  - *Cons:* a large dataflow addition with no exit-criteria coverage beyond 2A.
- **Recommendation: 2A**, consistent with Phase 4's discipline.

### Fork 3 — Where the caller recovers variant-payload facts

For a **record**-returning call, the result local directly gets `field_own` and the
existing `ARecordGet` move recovers it — no new seam. For a **variant**-returning
call (Case R), the payload is bound by a `case` pattern, and pattern-bound locals
are seeded Unknown today (`ownership.tw:499`). Two ways to route the facts:

- **3-i — call result carries tagged payload facts (via 1a-i's `[Payload(tag,i)]*`),
  and match block-entry seeds the payload-bound local by projecting the matching
  `[Payload(tag,i)]`** (the variant-payload analogue of `ARecordGet`'s field
  projection). The tag makes the projection arm-correct. Needs `cfg.tw`'s match
  lowering to expose, per arm, the scrutinee local + the arm's variant tag + the
  payload binding, so entry-seeding can project the right subtree. **Start narrow:**
  only direct top-level `Variant(… Var(v) …)` payload bindings; nested / wildcard /
  non-`Var` payload patterns **under-claim** (seed Unknown) until CFG metadata
  supports them.
  - *Pros:* uniform with the record-field projection; the payload move reuses
    `FieldMap.project`/`remove_prefix`; no fused-shape pattern match.
  - *Cons:* requires (small) `cfg.tw` match-lowering metadata + a block-entry seed
    step for bound locals (today they are only *killed*).
- **3-ii — a fused `case call(...) { .Ok(v) => … }` peephole** that recognizes the
  call-immediately-matched shape and seeds arm bindings directly from the summary.
  - *Pros:* no per-local payload channel.
  - *Cons:* brittle to intervening ANF; misses `loaded := case … { .Ok(v) => v }`
    where the payload is re-bound; a special-case rather than a compositional fact.
- **Recommendation: 3-i.** It composes (variant projection is "field projection one
  level up"), and the `cfg.tw` metadata is modest. The block-entry payload seed is
  the single new integration point and is where the fork's risk concentrates —
  called out as a prerequisite task.

## Module layout

- **`field_facts.tw`** (extend, Fork 1a-i) — add `Payload(Int, Int)` to `PathSeg`;
  extend the reversible `PathKey` codec with a **disjoint negative range** encoding
  `[Payload(tag,i)]` and `[Payload(tag,i), Field(f)]` via a reversible pairing over
  `(tag, i, optional f)`; `graft`/`project`/`remove_prefix` gain the `Payload`
  prefix case. Still a LEAF module (no `ownership`/`cfg` import).
- **`ownership.tw`** (extend):
  - `Summary` gains `ret_paths` (Decision 1); `RetVia`/`ReturnOwn`/`ReturnPathOwn`
    live beside `ReturnEffect`. `ForwardState` gains `path_prov` (Decision 2).
  - `path_prov` plumbing: the aggregate builders (`ARecord`/`AVariant`/`AArrayLit`/
    `ARecordUpdate`) set the **shell** `prov` to shell-level only and write each
    field/payload value's origins into `path_prov` under the grafted path (Decision
    2); `graft`/`project`/`remove_prefix`/join/`set_own_st`-clear carry `path_prov`
    in lockstep with `field_own`; `publish_local` publishes `path_prov` origins too.
  - `AVariant(tag, …)`'s transfer grafts payload facts under `[Payload(tag, i)]`.
  - **Remove `Return`'s publication** in `forward_block` (Decision 3); keep
    `ValueBreak`'s.
  - `summarize_function` classifies `ret` as shell-level and `ret_paths` from the
    body-only `ForwardState` + returned atom (record fields via `field_own` +
    `path_prov`; variant payloads via the returned variant local's `[Payload(tag,i)]*`
    facts), joining across return sites.
  - `transfer_summarized_call` gains a `last` parameter; applies the Decision 5 gate
    against pre-call arg facts; attaches `Direct` record-field return paths to the
    result's `field_own`+`path_prov`, and stores `Variant(tag,i)` return paths as the
    result's `[Payload(tag,i)]*` facts for the match-arm seed (Fork 3-i).
  - `ARecordGet`'s move condition gains `or transport_has(transport, result)`.
  - Match block-entry seeds pattern-bound payload locals from the scrutinee's
    `[Payload(tag,i)]*` facts (Fork 3-i), replacing seed-Unknown for those locals.
  - `join_entry_field_own` meets payload paths for free (more `PathKey`s); its
    `path_prov` sibling meets in lockstep.
- **`summary.tw`** (extend) — `conservative_summary` (`summary.tw:58`) seeds
  `ret_paths: []`; `same_summary` (`summary.tw:113`) compares `ret_paths`
  (canonical-sorted) so the SCC fixpoint terminates; `render_summary`
  (`summary.tw:332`) prints the return-path summary.
- **`cfg.tw`** (extend) — expose per-match-arm `(scrutinee, variant_tag,
  payload_index, payload_binding)` metadata for Fork 3-i; `BlockPrep` gains the
  transport-move annotation (Decision 9); `render_view` prints the projection-move
  verdict.
- **CLI** — `twk ir --cfg` renders the new summary line and verdict; no new flag.

### Data model

```tw
// ownership.tw
pub type ReturnOwn = { OwnedFresh, OwnedFromParam(Int) }
pub type RetVia = { Direct, Variant(Int, Int) }              // Direct | Variant(tag, payload_index)
pub type ReturnPathOwn = .{ via: RetVia, field: Int?, own: ReturnOwn }  // field=None ⇒ whole payload sub-shell
pub type Summary = .{
  params: Vector<ParamSummary>,   // unchanged (Phase 3/4 whole-parameter)
  ret: ReturnEffect,              // REVISED meaning: shell-level ownership (Decision 1)
  ret_paths: Vector<ReturnPathOwn>,   // NEW: deep sub-paths of the returned value
}

// field_facts.tw — Fork 1a-i (tagged)
pub type PathSeg = { Field(Int), Elem, Val, Payload(Int, Int) }   // Payload(variant_tag, payload_index)

// ownership.tw — ForwardState gains path-attributed provenance (Decision 2)
type ForwardState = .{
  own: Dict<Int, Int>,                         // unchanged — the [] shell fact
  valid: Dict<Int, Bool>,                      // unchanged
  prov: Dict<Int, Vector<Int>>,                // REVISED: shell-level origins only (fresh aggregate ⇒ [])
  field_own: Dict<Int, ff.FieldMap>,           // unchanged (Phase 4), now also carries Payload paths
  path_prov: Dict<Int, Dict<Int, Vector<Int>>>,   // NEW: local -> PathKey -> origins, mirrors field_own
}
```

`ret_paths` / `path_prov` hold a path only when proven; absence is "no claim".
Invariants:

1. `ret_paths` describes **deep sub-paths**; the returned value's **shell** stays
   in `ret`. A fresh wrapper is `ret=OwnedFresh` + deep `ret_paths` (Decision 1).
2. `OwnedFromParam(k)` names the region of parameter `k` for a **single** origin —
   multi/unknown/conflicting `path_prov` **drops** the path (Decision 2); the caller
   gates on arg `k` (Decision 5). `OwnedFresh` is caller-unconditional.
3. `path_prov` is kept in lockstep with `field_own` through the **same** choke
   point: whenever `field_own[L]` drops a path (demotion, `remove_prefix`, join
   meet, `set_own_st` clear), `path_prov[L]` drops it too, so the two never
   disagree about which paths exist.

## Transfer / summary semantics

### `path_prov` on the aggregate builders (Decision 2)

The redesign starts at construction. Today `ARecord` unions field origins into the
record's shell `prov` (`ownership.tw:1058`); Phase 5 splits shell from field:

- **Shell `prov`** becomes shell-level only. A freshly-constructed aggregate
  (`ARecord`/`AVariant`/`AArrayLit`) has an empty shell `prov` (a fresh shell aliases
  no parameter). Shell `prov` is set only by shell-forwarding ops (`AInit`/`AAssign`/
  `AWrapAnyref`/`ARecordGet` projection) that carry a param region at the shell level.
- **Field/payload origins** go into `path_prov[result]` under the grafted path,
  taken from the stored value's own shell `prov` (and its own `path_prov`, rebased),
  in lockstep with the existing `field_own` graft — same single-retention gate, same
  `set_own_st`/`remove_prefix`/join choke points. **Invariant (review pt 3):** every
  `Unique` `field_own` path is grafted with a `path_prov` entry — at least
  `Some([])` when the stored value has no param origin — so a *missing* `path_prov`
  entry unambiguously means "provenance unknown" (conservative drop), never
  "proven fresh".
- **`publish_local`** additionally publishes every origin in `path_prov[id]`, so
  publishing a whole aggregate still leaks its fields (soundness preserved).

### Return: not retention, but still a leaf exit (Decision 3)

Two orthogonal effects, kept distinct (review pt 2):

- **Caller-visible escape:** `forward_block` stops calling `publish_atom(a)` on
  `Return` (it still does for `ValueBreak`). Returning a value is a hand-off to the
  caller, not a leak; the caller's `ret`/`ret_paths`+gate account for it. Genuine
  leaks inside the callee (globals, closures, channels, unknown calls, escaping
  aggregates, aliasing) still demote independently, so `Retained` continues to catch
  real escapes. This is what lets a param handed forward through a return path stay
  `Borrowed`.
- **Intra-function joins / Case T:** a `Return` block is a CFG **leaf** — it has no
  successors, so its returned value never merges into any continuing-arm join. The
  `.Ok`/fallthrough arm of a `try` is therefore unaffected **by construction**, with
  no publish needed: Case T's "error arm exits, Ok arm continues" falls out of the
  block structure, not out of a publication. `ValueBreak` is the asymmetric case —
  its value flows to a **real successor** (the post-loop join), so it still
  publishes (conservative until a later phase refines loop-carried value breaks).

Because return blocks are leaves, removing the return-publish changes **only** the
escape scan (returned-only params are no longer `Retained`) and nothing about
joins. It is a **deliberate change to Phase 3 escape behavior** — affected
`cfg_summary_suite` expectations are re-baselined.

### Computing `ret` and `ret_paths` (in `summarize_function`)

At each return block, after `forward_block_body` yields the **body-only** `body`
and the returned atom `a` (the existing site, `ownership.tw:2319`):

- **`ret` (shell-level):** classify `a`'s **shell** — `MayAliasParams(ks)` iff `a`'s
  **shell** `prov` is non-empty (the returned value *itself* aliases params `ks`,
  e.g. `return x`); else `OwnedFresh` iff the shell fact is `Unique`; else `Shared`.
  Because shell `prov` no longer conflates field origins (Decision 2), a fresh
  wrapper `.{ ctx: p0 }` now classifies `OwnedFresh`, not `MayAliasParams`.
- **`ret_paths`, `a` a record local** (`Direct`): for each `[.f]:Unique` path in
  `body.field_own[a]`, read `path_prov[a].get([.f])` **three-way** (Decision 2):
  `.Some([k])` ⇒ `OwnedFromParam(k)`; `.Some([])` ⇒ `OwnedFresh`; `.Some([multi])`
  or `.None` ⇒ **drop the path**. Multiple fields emit multiple entries
  (Decision 8).
- **`ret_paths`, `a` a variant local** (`Variant(tag, i)`): read `a`'s
  `[Payload(tag,i)]*` facts. `[Payload(tag,i)]:Unique` emits `field: None`; each
  `[Payload(tag,i), .f]:Unique` emits `field: Some(f)`; `own` from `path_prov` as
  above. The tag is carried into `RetVia.Variant(tag, i)`.
- **Join across return sites:** a return path survives only if present and
  `own`-compatible on **every** return block that returns the same `via` (meet); any
  return block that omits or publishes it drops it. `ret` joins via the existing
  `join_return`.

This reuses Phase 4 `field_own` + the new `path_prov` at the return site — a richer
readout of the state already computed there, plus the shell/field prov split.

### Caller consumption (`transfer_summarized_call`, now with `last`)

Unchanged for `params`. `ret` is applied as today (`OwnedFresh`→Unique,
`Shared`→Unknown, `MayAliasParams`→publish origins + Shared result). Then, per
`ReturnPathOwn` in `s.ret_paths`, evaluated against **pre-call** arg facts captured
before any escape/return handling (Decision 5). At the caller, `OwnedFromParam(k)`
refers to the argument atom `args[k]`:

- **`OwnedFresh` path:** result path `Unique`, unconditionally.
- **`OwnedFromParam(k)` path, gate passes** (`args[k]` `Unique`+last-use): result
  path `Unique`, with the recovered `path_prov` set to `prov_of(args[k])` (so a
  later publish of the result still leaks `args[k]`'s origins).
- **`OwnedFromParam(k)` path, gate fails:** **publish `args[k]`** and make no unique
  claim on the result path (review pt 1). This is the conservative
  `MayAliasParams`-equivalent — the returned value may alias `args[k]`'s region, so
  the source arg must not be left `Unique`. Idempotent when `args[k]` is already
  `Shared`.
- **`Direct` paths** with a `Unique` verdict populate the result's `field_own` +
  `path_prov` at `[.f]`; the caller recovers them through the existing `ARecordGet`
  move. (`field: None`+`Direct` is unused — the whole `Direct` value is `ret`.)
- **`Variant(tag, i)` paths** with a `Unique` verdict are stored on the result as
  `[Payload(tag,i)]*` facts, projected when a `case` arm binds that payload
  (Fork 3-i).

`Consumed` capability stays recorded-only; no binding is invalidated (Decision 6).

### The transport-wrapper projection move (Decision 9)

A block-local recognizer (`recognize_transport_moves`, analogous to
`recognize_quartet_moves`) marks an `ARecordGet(out, f) → R` result-local as
**move-licensed** when, strictly after the projection in the same block:

- `out.f` is **not read again** and `out` is **not** published / returned / stored
  / passed to a call / aliased / used by the terminator or a successor edge arg
  **for anything other than sibling projections/reads**; and
- `out` is dead after the block (no surviving alias can observe R's later in-place
  mutation).

Sibling reads (`out.ty`, `out.diags`) are explicitly permitted — that is the whole
point relative to Phase 4's whole-record last-use. The recognizer surfaces via
`BlockPrep`; `transfer_op`'s `ARecordGet` adds `or transport_has(transport,
result)` to its move condition, reusing the existing move/borrow mechanics
(transfer `[.f]*`, `remove_prefix` on `out`).

### Match-arm payload projection + handled-`Result` joins (Fork 3-i)

At a match block whose arm binds payload `v` of variant `tag` payload-index `i`
from scrutinee `S`:

- **Seed** `v` from `S`'s `[Payload(tag,i)]*` facts (the arm's own tag, so an `.Err`
  arm reads only `Err` payloads): `project(.Payload(tag,i))` gives `v`'s shell (from
  `[Payload(tag,i)]`) and inner fields (`[Payload(tag,i), .f] → [.f]`), plus the
  `path_prov` under it, exactly like `ARecordGet` one level up. This replaces the
  seed-Unknown for payload-bound locals (`ownership.tw:504` keeps killing them as
  live-ins; the forward pass now seeds their *facts* from the scrutinee).
- **Move vs borrow:** the payload projection is a move when `S` is dead after the
  match (the scrutinee is typically consumed by the `case`), removing
  `[Payload(tag,i)]*` from `S`; otherwise borrow-demote, same as `ARecordGet`.
- **Arm joins:** the ordinary field-own join (`join_entry_field_own`) already merges
  the arms' facts by per-path meet — a payload field that stays `Unique` on every
  continuing arm survives the join; an arm that publishes it drops it.
- **Exit arms (Case T):** an arm ending in `Return` / `try`'s error path is a CFG
  **leaf** (no successor), so it contributes **no** surviving fact to the continuing
  join — the `.Ok` arm is unaffected by construction (Decision 3), with the returned
  value handed to the caller (not published inside this callee). A value-carrying
  `Break` differs: it targets a real post-loop successor, so it still publishes
  conservatively. So `.Err(err) => return …` neither preserves nor corrupts the
  `.Ok` arm's transported state.

## Join, fixpoint, determinism

- **Intraprocedural join:** payload paths are ordinary `PathKey`s, so
  `join_entry_field_own` / `FieldMap.merge` handle them unchanged (per-path meet,
  skip unprocessed back-edges, cleared by `set_own_st` when a shell leaves Unique).
- **Summary fixpoint (Blocker 5 + review pt 4).** `ret_paths` is seeded **empty** by
  `conservative_summary`. It is **not** monotone-only: paths are *discovered* (added
  when proven across all relevant return sites) and may be *retracted* on a later
  iteration. Because a retractable path is speculative until convergence, the
  discipline is stricter than "expose fewer paths":
  - **Hide in-progress paths via an in-SCC suppression set.** During SCC iteration,
    a within-SCC recursive call reads the callee's `ret_paths` as **empty** — the
    call transfer blanks `ret_paths` for any callee in the current SCC's member set
    (only escape/`params`/`ret` — the monotone facts — flow through the recursive
    edge). Keying suppression on **membership, not table contents**, makes the
    readout **order-independent**: no matter the member processing order, an in-SCC
    read never observes another member's freshly-classified `ret_paths`. So no
    speculative return path can influence the fixpoint; the recursive transport case
    merely *under*-approximates. (This replaces an earlier "strip during the
    worklist, restore in a separate final pass" sketch, which was order-dependent
    because members restore into the shared table one at a time. No separate final
    pass is needed — each member's `ret_paths` are computed under suppression as the
    worklist runs, and `same_summary` drives termination.) Full recursive-transport
    precision — a self-threading `visit`-shaped wrapper recovering its *own* return
    paths — is deferred to Phase 6's SCC-granularity specialization; Phase 5
    under-approximates it, which is sound.
  - **Strip on non-convergence.** If `run_scc` hits its cap without converging,
    **clear `ret_paths` for every SCC member** before returning (expose empty =
    fully conservative aggregate publication). Never expose a cap-time snapshot,
    which could contain a path a further iteration would retract.
  - Callers in later SCCs consume only the converged (or stripped) summary
    (`run_scc` fixes each SCC before its callers, per `summary.tw`'s callee-first
    order). `same_summary` compares canonical-sorted `ret_paths` so the loop
    terminates. `ret` follows the same discovery/convergence rule via `join_return`.
- **Determinism:** `ret_paths` sorted by `(variant tag, payload index, field id)`;
  the extended `PathKey` codec stays canonical, reversible, and collision-free (the
  disjoint negative payload range); the transport recognizer is a pure function of
  block structure + liveness scan; all rendered output is sorted.

## Rendering

Extend `render_summary` (`summary.tw:332`) and `render_view`:

- Summary header gains the return-path line, e.g.
  `ret_paths: [.ctx]=from(p0) [.accum]=fresh  Ok[0].state=from(p0) Err[0].state=from(p0)`.
- Per transport projection site, the move verdict:
  `L9 = record_get out.ctx  transport=move([.ctx] from p0)` or
  `… transport=borrow(out published)` / `… transport=borrow(arg0 not unique)`.

Never a silent bail — every rejection prints its reason (the `records-fields.md`
discipline, matching Phase 4's two-verdict output).

## Testing

New `boot/tests/suites/cfg_return_paths_suite.tw`, TDD, mirroring the Phase 2–4
suites; each fixture checked against `twk ir <fixture> --opt`/`--cfg` so the tested
shape **survives optimization**. Unlike Phase 4 (strictly intraprocedural),
Phase 5 fixtures are **cross-function** (that is the point):

- **Case W — record transport:** a `helper(ctx) -> .{ ctx, ty }` returning
  `[.ctx]=OwnedFromParam(0)`; a caller `out := helper(ctx); ctx = out.ctx; …read
  out.ty…` shows `ctx` `Unique` after (transport move fires despite the sibling
  read).
- **Multi-accumulator:** `helper -> .{ state, accum }` with two `OwnedFromParam`
  paths; a caller recovers both independently.
- **Case R — `Result` payload:** `load(state) -> SourceLoad!Err` with
  `Ok[0].state`/`Err[0].state = OwnedFromParam(0)`; a caller `case load(state) {
  .Ok(v) => …v.state…, .Err(e) => return … }` shows the `.Ok` arm's state `Unique`
  and the join preserving it; the `.Err(return)` edge publishes (Case T).
- **Variant-tag isolation (Blocker 1):** a helper returning `.Ok` with an owned
  payload but `.Err` with a **shared/foreign** payload — the `.Err` arm must **not**
  recover an owned fact (the tagged `[Payload(Err,0)]` has no owned claim); confirms
  arms are disjoint by tag.
- **Path-provenance drop (Blocker 2):** `.{ a: p0, b: p0 }` (same origin twice) and
  a field with **multi/unknown** origin classify as **dropped**, *not* `OwnedFresh`;
  `.{ state: p0, accum: p1 }` classifies `[.state]=from(p0)` and `[.accum]=from(p1)`
  independently (the per-path prov actually separates them).
- **Shell-vs-alias `ret` (Blocker 3):** a fresh wrapper `.{ ctx: p0 }` classifies
  `ret=OwnedFresh` + `[.ctx]=OwnedFromParam(0)` (not `MayAliasParams`); `return x`
  (param) classifies `ret=MayAliasParams([0])` with empty `ret_paths`; the
  Return-publish removal is re-baselined in `cfg_summary_suite`.
- **`OwnedFromParam` gate negatives + publish-on-fail (Decision 5):** the same
  helper called with `ctx` **still read after the call** → the return path makes no
  unique claim **and `ctx` is published** (demoted to `Shared`), not left `Unique`;
  a **shared/aliased** `ctx` → path dropped, `ctx` already `Shared`. Verdict names
  the reason.
- **`path_prov` absent-vs-fresh (review pt 3):** a proven-fresh field (`Some([])`)
  classifies `OwnedFresh`; a field whose provenance is unknown (`None`) is dropped —
  the two are distinguished, never conflated.
- **Transport borrow negatives:** `out` published / returned / stored, or `out.ctx`
  read twice → borrow, both demoted.
- **Variant shell-only regression:** a variant with a **shared** payload claims no
  payload fact (the `AVariant` deep graft respects single-retention).
- **`PathKey` codec round-trip:** `path_key`/`path_of_key` invert for every
  `[Payload(tag,i)]` and `[Payload(tag,i), .f]` over a spread of tags/indices/fields,
  with **no collision** against the positive field range.
- **Determinism:** `twk ir --cfg` byte-identical across two builds.

## Non-goals

- No parameter-side `in_place_paths` / per-path `Consumed` (Phase 6).
- No ownership specialization / variant selection / call-site variant decision
  (Phase 6).
- No caller binding invalidation (Phase 6; Decision 6).
- No general per-path liveness (Fork 2A only).
- No codegen, decision records, or in-place emission (Phases 7–8).
- No return paths deeper than one field under a record / variant payload
  (Decision 7).
- No `Moved` lattice element; no runtime uniqueness flags/refcounts/COW checks.

## Acceptance criteria

All via the boot suite unless noted:

1. **Record-transport return paths** — a `helper -> .{ ctx, … }` shows
   `ret_paths: [.ctx]=from(p0)`; a caller reassigning `ctx = out.ctx` while reading
   a sibling shows `ctx` `Unique` after (Case W classifies as an
   ownership-preserving handoff, not aggregate publication).
2. **Shell-vs-alias `ret`** — a fresh wrapper `.{ ctx: p0 }` is `ret=OwnedFresh` +
   `[.ctx]=OwnedFromParam(0)`; `return x` (param) is `ret=MayAliasParams([0])` with
   empty `ret_paths` (Blocker 3).
3. **Per-path provenance** — `.{ state: p0, accum: p1 }` yields two independent
   `OwnedFromParam` paths; a same-origin or multi/unknown-origin field is
   **dropped**, never mislabeled `OwnedFresh` (Blocker 2).
4. **`Result`-payload return paths + tag isolation** — `load -> T!E` shows
   `Ok[0].state`/`Err[0].state=from(p0)`; the handled `.Ok` arm keeps state
   `Unique`, the join preserves it, the `.Err`/`return` edge publishes (Case R + T);
   an `.Err` arm cannot recover an `.Ok`-only payload fact (Blocker 1).
5. **`OwnedFromParam` soundness gate + publish-on-fail** — a read-after argument
   makes no unique result claim **and is published** (not left `Unique`); an
   already-shared argument is dropped idempotently; the verdict names the reason
   (Decision 5, review pt 1).
6. **Transport move vs borrow** — sibling reads do **not** block the move; a
   published/returned/stored `out` or a double-read of `out.f` forces borrow
   (both demoted).
7. **Variant-payload single-retention** — a variant over a shared payload claims no
   payload fact; the downward-closed invariant holds (no `[Payload(tag,i)]*` while
   the variant shell is non-Unique).
8. **`PathKey` codec round-trip** — `path_key`/`path_of_key` invert for all
   `[Payload(tag,i)]`/`[Payload(tag,i), .f]` with no collision against field keys.
9. **Recursive under-approximation (fixpoint discipline)** — a self-threading
   (recursive) transport helper exposes **no speculative `ret_path`** during
   iteration (within-SCC recursive reads see empty `ret_paths`); non-recursive
   callers still recover it. Cap-hit stripping is confirmed by inspection
   (review pt 4).
10. **Determinism** — `twk ir --cfg` (summary + verdicts) byte-identical across two
    builds.
11. **No regression to in-place** — `twk ir --census` still shows **0 in-place**
    (Phase 5 changes no codegen).
12. **Full verification** — `make boot-test` green and `make stage2` reaches the
    self-host fixed point (Phase 5 is boot-only and adds no stage0-parity construct),
    including the re-baselined `cfg_summary_suite` for the Return-publish change.

## Deferrals and tracking

| Item | Home |
|---|---|
| Parameter-side `in_place_paths` / per-path `Consumed` | Phase 6 ([summary-specialization.md](summary-specialization.md)) |
| Ownership specialization over return/param paths; call-site variant decision; binding invalidation | Phase 6 |
| General per-path liveness beyond the transport shape | Post-codegen follow-up — tracked in [README.md](README.md)'s "Analysis deferrals" table (conservative-by-default; sound without it) |
| Return paths deeper than one field under a record / variant payload | Post-codegen follow-up — tracked in [README.md](README.md)'s "Analysis deferrals" table (sound under-claim without it) |
| Candidate verdicts → decision records → in-place emission | Phases 7–8 ([../codegen/README.md](../codegen/README.md)) |

## Determinism-sensitive spots (lock with tests)

- **Extended `PathKey` codec** — payload keys occupy a disjoint, canonical,
  collision-free **negative** range via a reversible pairing over `(tag, index,
  optional field)`; round-trip `path_key`/`path_of_key` covers `[Payload(tag,i)]`
  and `[Payload(tag,i), .f]`.
- **`ret_paths` ordering** — canonical-sorted `(variant tag, payload index, field
  id)`; `same_summary` compares the sorted form so the fixpoint is order-stable.
- **Transport recognizer** — a pure function of block structure + the liveness
  scan, precomputed once per block like the quartet recognizer (off the fixpoint
  iteration path).
