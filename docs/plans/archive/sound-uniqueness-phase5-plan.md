# Phase 5 Return-path Summaries — Implementation Plan  ✅ COMPLETE (archived record)

> **STATUS: DONE — historical implementation record, do NOT re-execute.** All 15
> tasks landed (analysis-only; census still 0 in-place; self-host at fixed point).
> This document is the *original* plan; the implementation **evolved during
> execution**, so several inline task snippets below are **stale**. The source of
> truth is the code + tests, not these snippets:
> - **Code:** `boot/compiler/{field_facts,ownership,summary,cfg}.tw`.
> - **Tests:** `boot/tests/suites/{cfg_field_facts,cfg_summary,cfg_return_paths}_suite.tw`.
>
> **Known snippet drift (read before trusting any Task 7/8/10/13 code block):**
> - **Task 7/8 classification** iterates the returned atom's **`path_prov`** (not
>   `field_own` — params are `Unknown` at entry, so `field_own` is empty for
>   param-valued fields), and `classify_path_own(pp, k, params, fm)` takes an extra
>   `fm` arg that gates `OwnedFresh` on `field_own` membership (a `path_prov`-only
>   `[]` can hide an interior alias). An explicit `drop_aliased_param_paths` replaces
>   the `single_retention` aliasing gate.
> - **Return-site meet is tag-aware.** The plan's plain `meet_ret_paths` /
>   `ret_paths_acc` was replaced by `RetSite`/`RetWitness` + `meet_ret_paths_tagged`,
>   so a real two-tag `Result` and `Option`/any sum keep their payload paths
>   (see the "RESOLVED" note below).
> - **Task 10/11/13 fixtures** thread a **fresh unique local**, never a param (the
>   recovery gate rejects params — see the param-threaded TODO below), and multi-block
>   / arm-bound assertions use **`own_in_block`**, not `caller_own`. The tag-isolation
>   test asserts **`own_unknown()`**, not `own_shared()`.
>
> **The only Phase-5 deferral carried to Phase 6** is the param-threaded recovery gate
> (see the TODO below and `README.md`'s Phase 6 section). The tag-aware-meet TODO is
> **resolved** — do not carry it forward.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the boot ownership analysis *return-path ownership* — the region handed back through a returned record field (`out.ctx`) or variant payload (`Ok[0].state`) — and the caller-side recovery that keeps transport-wrapper / `Result`-payload state threading from classifying as aggregate publication. Analysis-only: no codegen changes.

**Architecture:** Extend the intraprocedural field-fact layer (`field_facts.tw`) with a tagged variant-payload path segment; add path-attributed provenance (`path_prov`) so a returned record's fields attribute to individual params; revise `Return` to be a non-retaining CFG leaf and `ret` to mean shell ownership; classify per-return-path ownership into a new `Summary.ret_paths`; recover it at the caller under a `last`-gated publish-on-fail rule, a bounded transport-wrapper projection recognizer, and match-arm payload seeding. Canonical design: [phase5-design.md](../sound-uniqueness/analysis/phase5-design.md).

**Tech Stack:** Twinkle (`.tw`) boot compiler. Build: `make bundle-cli` (→ `target/twk`) or `cargo run --release -- …` for stage0. Boot tests: `target/twk run boot/tests/main.tw`. Ownership modules: `boot/compiler/{field_facts,ownership,summary,cfg}.tw`. Unit suites: `boot/tests/suites/cfg_*_suite.tw`.

**Conventions for every task:** after editing a `.tw` file run `target/twk fmt <file>` then `target/twk lint <entry>`. Run the boot suite with `target/twk run boot/tests/main.tw`. Commit messages: imperative subject, what/why body, no metrics. **Co-Authored-By trailer:** per `AGENTS.md`, add `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` **only when the commit is actually authored through Claude tooling in the executing session** — the example commits below include it because this plan is expected to be executed that way; drop it if that is not accurate for your session. Heavy verification (`make stage2`, `make bundle-cli`, full suite) runs **sequentially, never backgrounded**.

**Build note:** the ownership modules are embedded into the self-hosted compiler, so a source change is only exercised after rebuilding `target/twk`. During TDD, rebuild the CLI once per task before running the suite: `make quick-bundle-cli` if `target/boot.wasm` is fresh, else `make bundle-cli`. Where a task says "run the suite," it means: rebuild the CLI, then `target/twk run boot/tests/main.tw`.

---

## File structure

| File | Responsibility | Phase 5 change |
|---|---|---|
| `boot/compiler/field_facts.tw` | Leaf path-fact unit: `PathSeg`/`AccessPath`/`FieldMap`, reversible `PathKey` codec, map ops | Add tagged `Payload(tag,index)` segment + negative-range codec + `Payload` prefix in graft/project/remove_prefix |
| `boot/compiler/ownership.tw` | Forward ownership transfer, `ForwardState`, summaries, `summarize_function` | Add `path_prov`; split shell/field prov in aggregate builders; `AVariant` payload facts; remove `Return` publish; `ret_paths` types + classification; caller consumption with `last`+gate; transport recognizer; match-arm payload seed |
| `boot/compiler/summary.tw` | Whole-program SCC summary driver + rendering | Seed/compare/render `ret_paths`; hide-in-progress + strip-on-cap discipline |
| `boot/compiler/cfg.tw` | Structural CFG view; `CfgBlock`, `build_match` | Per-arm payload-projection metadata (`payload_src`) for top-level `Var` payload bindings |
| `boot/tests/suites/cfg_field_facts_suite.tw` | Field-fact unit tests | Payload codec + graft/project round-trip tests |
| `boot/tests/suites/cfg_summary_suite.tw` | Summary unit tests | Re-baseline Return/escape; `ret_paths` classification; caller recovery |
| `boot/tests/suites/cfg_return_paths_suite.tw` (new) | Phase 5 cross-function fixtures | Cases W, R, gate negatives, transport move/borrow, determinism |

Task order is strictly dependency-first: Stage A (codec) → B (provenance) → C (return semantics) → D (summary schema) → E (fixpoint) → F (caller) → G (transport move) → H (match/Case R) → I (rendering + verification).

**Soundness boundary — do not ship a partial Phase 5.** Every task keeps the suite
green, but the *analysis* is only whole again at Task 13. Between Task 5 (Return no
longer publishes) and Task 10/13 (caller recovery + gate + payload seeding), a caller
of a wrapper-returning function sees `ret=OwnedFresh` and makes no publish/gate
decision for the handed-back region — a transient under-publication. This is
**harmless here because Phase 5 is analysis-only and nothing consumes these facts yet**
(census stays 0 in-place; Phase 6+ is the first consumer), so the incremental commits
are safe to land. But it means **no commit before Task 13 is a valid soundness
checkpoint** — do not wire any codegen/decision consumer against `ret_paths` until the
whole stage is in.

> ## TODO (revisit after all Phase 5 tasks land): caller-recovery gate rejects param-threaded state
>
> **Finding (surfaced during Task 9/10 execution).** The caller-side return-path
> recovery gate (Task 10) fires only when the recovered argument is a **fresh unique
> local** — it can *never* fire for a **parameter**. Params are seeded `Unknown`
> (borrowed) at function entry (`join_entry_ownership` seeds no ownership for block 0;
> only `seed_param_prov` runs), and the gate requires `Unique`+last-use. Concretely:
> - `fn f() { ctx := Dict.new(); out := helper(ctx); ctx = out.ctx }` → **recovers**
>   (fresh-local transport; this is Case W). ✅
> - `fn f(ctx) { out := helper(ctx); ctx = out.ctx }` → **does not recover** (`ctx` is
>   a param → `Unknown` → gate fails → `ctx` published/Shared). ❌
>
> **Why it's not a bug:** under-approximation is sound (worst case = today's aggregate
> publication), and parameter-side ownership is explicitly a **Phase 6** concern
> (`in_place_paths` / per-param `Consumed`). The same root cause makes recursive
> self-threading transport under-approximate (a recursive/param-arg call fails the
> gate; a fresh-arg recursive call yields `OwnedFresh` that the meet drops), which is
> why the Task 9 suppression mechanism is correct-but-rarely-triggered insurance.
>
> **Why it matters:** the census-dominant transport idiom (LSP `AnalysisState`,
> dataframe query state, the compiler's own threaded ctx records) typically receives
> the threaded state **as a parameter** and passes it down — exactly the shape the
> gate rejects. So **Phase 5 standalone optimizes only fresh-local transport; the
> common param-threaded case waits for Phase 6.** Phase 5 remains the necessary
> foundation (it builds the `ret_paths` summaries Phase 6 parameter-ownership will
> consume), but its measured impact alone will be narrow.
>
> **Action (do NOT do mid-Phase-5; revisit once Tasks 11–15 are done):**
> 1. Grep the real transport/threading sites the design cites and classify each as
>    fresh-local vs incoming-param, to quantify how much of the idiom Phase 5 alone
>    reaches.
> 2. Based on that, decide whether to (a) confirm the Phase 5 → Phase 6 ordering as-is,
>    or (b) pull a slice of Phase 6 parameter-ownership forward so param-threaded state
>    also benefits. Feeds the Phase 6 design review.

> ## ~~TODO~~ RESOLVED (commit `2d23394c`): variant-return meet is now tag-aware (covers Result, Option, any sum)
>
> **Scope of the bug (broadened).** The original `meet_ret_paths` was a plain
> intersection across return sites, so it was variant-agnostic in the bad way: it
> treated "path absent because this return site is a *different tag*" as a
> contradiction and dropped the payload path. This affected **every sum-typed return
> with payload-bearing and payload-less (or differently-tagged) alternatives** — not
> just two-tag `Result` (`.Ok`-owned / `.Err`-foreign) but also `Option`
> (`.Some(payload)` / `.None`), where any `.None` return site — including `try`
> early-returning `.None` — would erase the `.Some[0].*` paths.
>
> **Fixed as a Phase 5 hardening follow-up.** `meet_ret_paths` was replaced by
> `meet_ret_paths_tagged`: each return site gets a witness (`Direct` /
> `Variant(tag)` / `Unknown`, from owned facts or the return block's constructor op),
> and a `Variant(tag)` path is met only across sites that could return that tag. A
> could-produce site that lacks the path (`.Ok(shared)` / a `Some`-with-shared-payload
> site, or an `Unknown` witness) still drops it; **different-tag sites — including a
> payload-less `.None`/`.Err` — are irrelevant to a `.Some`/`.Ok` payload path (they
> carry no payload to recover), so they no longer contradict it.** Verified: two-tag
> `Result` keeps both arms (`Ok[0].f0=from(p0)`), and `Option` keeps `Some[0].f0=from(p0)`
> across a payload-less `.None` return. Sound — behaviourally identical for
> single-witness / all-`Direct` functions (the recursive two-`Direct` meet still drops
> `[.f0]`); only the cross-tag erasure is fixed.
>
> **One residual (sound, minor) caveat:** the witness comes from the returned atom's
> owned facts or a scan of the *return block* for its constructor op. A variant that is
> *passed through* or constructed in an *earlier* block (so neither owned facts nor a
> same-block constructor pin its tag) falls to `Unknown` → conservative drop of the
> affected claims. Common shapes (fresh `.Some`/`.Ok`/`.None`/`.Err` at the return, and
> `try`'s fresh `.None`) are covered; the fallback is sound. Widening to cross-block
> variant-tag tracking is optional future precision, not a soundness need.
>
> **Note:** Case R end-to-end still needs a *fresh* scrutinee arg because of the
> param-gate TODO above — the two gaps were independent, and only this one is closed.
> The historical analysis is kept below for context.
>
> **Finding (empirically confirmed during Task 13).** `meet_ret_paths` joins `ret_paths`
> across a function's multiple return sites by **plain intersection** on `(via, field)`.
> A real two-arm `Result` function returns different **tags** on different paths:
> ```
> fn load(s) { if c { return .Ok(Record{state: s}) }  return .Err(Record{msg: fresh}) }
> ```
> The Ok site produces `Variant(Ok,0)*` paths, the Err site produces `Variant(Err,0)*`
> paths. Plain intersection drops both (each is absent on the other site), so **`load`'s
> `ret_paths` is EMPTY** (verified: `ret_paths.len() == 0` for exactly this shape). So
> **Case R only fires for a contrived single-return-Ok `load`, never for a real
> `Result`-returning function.**
>
> **Why it's not a bug:** empty `ret_paths` = fully conservative (no recovery) = today's
> aggregate-publication behavior. Sound, just imprecise — it defeats the primary
> Result-payload transport goal (design acceptance criterion 4) in practice.
>
> **Root cause vs design:** the canonical design specifies a **tag-aware** meet (a path
> survives if present on every return block "that returns the same via"); the
> implementation (`meet_ret_paths`) simplified it to a plain intersection and lost the
> tag-awareness. A correct fix needs **per-return-site returned-tag tracking** (so an
> `Ok` path is met only across sites that return `Ok`, and a site that returns `.Ok(shared)`
> — which carries no owned payload path — still contradicts an owned-`Ok` claim). That is
> real plumbing (the returned atom's variant tag per site), not a small meet tweak.
>
> **Also correct a stale expectation:** Task 13's tag-isolation test was specified to
> assert the Err-arm binding is `own_shared()`. That is unreachable — `field_own` only
> stores `Unique`, and an *unrecovered* payload binding stays **Unknown** (the seed's
> `.None` branch). The shipped test correctly asserts `own_unknown()`; treat the plan's
> `own_shared()` as superseded.
>
> **~~Action~~ SUPERSEDED — this section is historical.** The tag-aware meet was
> **implemented in Phase 5** (commit `2d23394c`; `RetSite`/`RetWitness` +
> `meet_ret_paths_tagged`), so it is **not** Phase 6 work — see the RESOLVED note at
> the top of this TODO. The only gap that folds into Phase 6 is the param-threaded
> recovery gate (the separate TODO above). The analysis below is retained only as the
> original problem statement.

---

## Fixture construction discipline (read before writing any cross-function test)

The cross-function fixtures (`case_w_fixture`, `case_r_*`, `recursive_transport_fixture`,
`case_w_sibling_fixture`, …) are prose-specified, and **a fixture with the wrong
liveness/last-use shape makes its test pass for the wrong reason** — e.g. a "move"
test where the scrutinee/`out` isn't actually dead-after would pass via a fallback,
and a borrow-negative could pass trivially. That silently defeats the gate. So:

1. **Prefer building from real frontend output.** Where practical, write the fixture
   as a small `.tw` snippet and obtain its ANF/CFG through the real pipeline
   (`cfg.build_view` on compiled ANF) rather than hand-encoding local ids and block
   structure. Hand-built ANF is acceptable for the tiny single-block summary
   fixtures (Tasks 5/7/8) but error-prone for the multi-block caller fixtures
   (Tasks 10/11/13).
2. **Every move/borrow claim needs its inverse.** For each fixture asserting a move
   (Unique recovered), the plan already pairs a borrow negative — when authoring,
   confirm the *only* difference between the pair is the liveness fact under test
   (published `out`, later-block read, in-arm read), so the gate is provably live.
   If flipping that one fact does **not** flip the verdict, the fixture is wrong, not
   the analysis.
3. **Eyeball the CFG once per new fixture.** Run `twk ir <snippet> --cfg` (or dump
   the built view) and confirm block preds, `entry.live`/`exit.live`, and the
   dead-after shape match the case name before trusting a green result.
4. **`caller_own` is block-0-only — add a block-aware variant for multi-block
   fixtures.** The copied `caller_own(f, local)` reads `f.blocks[0].exit.ownership`
   and returns `Shared` (tag 2) for any local it doesn't find there
   (`cfg_summary_suite.tw:154`). For single-block Case W that's correct, but Case R
   (Task 13, payload bound in an **arm** block) and the live-out negatives (Task 11)
   observe locals that don't exist in block 0. Reading them via `caller_own` makes a
   **move** assertion fail spuriously and — the dangerous direction — makes a
   **borrow-negative pass trivially** via the `.None → Shared` default, proving
   nothing. Add `own_in_block(f, block_id, local) Int` (same body, indexed at the
   block that actually defines/observes the local) and assert against that block. A
   borrow-negative **must** target a block where the local genuinely exists, so
   `Shared` is a real verdict, not an absence.

---

## Stage A — Tagged payload segment + codec

### Task 1: Add `Payload(tag,index)` to `PathSeg` and extend the codec

**Files:**
- Modify: `boot/compiler/field_facts.tw` (`PathSeg` `:12`, `seg_eq` `:45`, `path_key` `:82`, `path_of_key` `:103`, `graft` `:152`, `project` `:180`, `remove_prefix` `:197`)
- Test: `boot/tests/suites/cfg_field_facts_suite.tw`

- [ ] **Step 1: Write the failing codec round-trip test**

Add to `cfg_field_facts_suite.tw` (inside `suite()`), using the module alias already imported as `ff` or `field_facts` — check the file's imports and match it:

```tw
.test(
  "payload path_key round-trips and stays disjoint from field keys",
  fn() {
    // [Payload(tag,i)] and [Payload(tag,i), Field(f)] over a spread.
    for tag in range(3) {
      for i in range(2) {
        p0 := field_facts.AccessPath.{ segs: [.Payload(tag, i)] }
        k0 := field_facts.path_key(p0)
        try assert.equal(k0 < 0, true) // payload keys are negative
        try assert.equal(field_facts.path_eq(field_facts.path_of_key(k0), p0), true)
        for f in range(3) {
          p1 := field_facts.AccessPath.{ segs: [.Payload(tag, i), .Field(f)] }
          k1 := field_facts.path_key(p1)
          try assert.equal(field_facts.path_eq(field_facts.path_of_key(k1), p1), true)
          // disjoint from a positive field key with the same f
          fk := field_facts.path_key(field_facts.field_path(f))
          try assert.equal(k1 == fk, false)
        }
      }
    }
    .Ok({})
  },
)
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `target/twk run boot/tests/main.tw` (after `make quick-bundle-cli`).
Expected: parse/type error — `PathSeg` has no `Payload` variant yet.

- [ ] **Step 3: Extend `PathSeg` and `seg_eq`**

In `field_facts.tw:12`:

```tw
pub type PathSeg = { Field(Int), Elem, Val, Payload(Int, Int) }
```

Add the `Payload` case to `seg_eq` (`:45`):

```tw
.Payload(ta, ia) => case b {
  .Payload(tb, ib) => ta == tb and ia == ib,
  _ => false,
},
```

- [ ] **Step 4: Extend the codec (negative disjoint range, fixed-width bit-packing)**

Positive keys stay as-is. Payload keys pack `(tag, index, fieldslot)` into a
disjoint NEGATIVE range by **fixed-width bit-packing**, not a search-based pairing:
`tag`/`index`/`field` are small per-type indices (`VariantId.id` is a per-type
variant index; see `lower_core/records.tw`), so 20 bits each is ample, and packing
is **O(1) to encode and decode**. This matters because `path_of_key` is called
inside `graft`/`project`/`remove_prefix` loops within the fixpoint — a √z search
loop per decode (Cantor) would be a real compile-time cost on wide enums. Use
multiplication (not `<<`/`|`) to sidestep the boot bitwise-precedence + fmt-strips-
parens gotcha. Add above `path_key`:

```tw
// 20-bit fields: 2^20 = 1048576, 2^40 = 1099511627776.
// fieldslot = 0 (no field, i.e. the payload shell) or 1+f.
// Packed nonneg -> disjoint NEGATIVE key (offset by 1 so 0/positive keys are free).
fn payload_key(tag: Int, index: Int, fieldslot: Int) Int {
  if tag < 0 or tag >= 1048576 or index < 0 or index >= 1048576
    or fieldslot < 0 or fieldslot >= 1048576 {
    error("field_facts: payload key component out of 20-bit range")
  }
  packed := tag * 1099511627776 + index * 1048576 + fieldslot
  0 - (packed + 1)
}
```

Add to `path_key` (`:82`) — a `Payload` case at `segs.len() == 1` and a `Payload`-prefixed case at `segs.len() == 2`:

```tw
// inside segs.len() == 1 case:
.Payload(tag, i) => payload_key(tag, i, 0),
// inside segs.len() == 2 case (segs[0]):
.Payload(tag, i) => case segs[1] {
  .Field(f) => payload_key(tag, i, 1 + f),
  _ => error("field_facts: payload nested seg must be Field in Phase 5"),
},
```

Add to `path_of_key` (`:103`) a branch for `k < 0`:

```tw
k < 0 => {
  packed := (0 - k) - 1
  fieldslot := packed % 1048576
  index := packed / 1048576 % 1048576
  tag := packed / 1099511627776
  if fieldslot == 0 {
    AccessPath.{ segs: [.Payload(tag, index)] }
  } else {
    AccessPath.{ segs: [.Payload(tag, index), .Field(fieldslot - 1)] }
  }
},
```

- [ ] **Step 5: Add the `Payload` prefix case to `graft`, `project`, `remove_prefix`**

`graft` (`:152`) currently special-cases `.Field(_)` and drops others. Add `.Payload` alongside `.Field` so a payload prefix carries a `Field` inner:

```tw
// change the match head in graft to handle both Field and Payload prefixes:
case prefix {
  .Field(_) => { /* existing loop grafting [prefix, Elem/Val] */ },
  .Payload(_, _) => {
    for k, v in src.paths {
      inner := path_of_key(k)
      if inner.segs.len() == 1 {
        case inner.segs[0] {
          .Field(_) => fm.paths[path_key(AccessPath.{ segs: [prefix, inner.segs[0]] })] = v,
          _ => {},   // Elem/Val/Payload inners not representable under a payload prefix at depth 2
        }
      }
    }
    fm
  },
  _ => fm,
}
```

`project` (`:180`) and `remove_prefix` (`:197`) already match on any `prefix: PathSeg` structurally via `seg_eq`, so they work for `Payload` unchanged — **verify by reading them**; no edit expected. Add a one-line comment noting payload prefixes are supported.

- [ ] **Step 6: Run the codec test — expect PASS**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: the payload round-trip test passes; all existing field-fact tests still pass.

- [ ] **Step 7: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/field_facts.tw boot/tests/suites/cfg_field_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/field_facts.tw boot/tests/suites/cfg_field_facts_suite.tw
git commit -m "field_facts: add tagged Payload path segment and negative-range codec

Phase 5 needs variant-payload ownership keyed by (variant_tag, payload_index)
so an .Err arm can never recover an .Ok payload fact. Encode payload paths in a
disjoint negative PathKey range via O(1) fixed-width bit-packing, keeping positive
field keys intact, and extend graft to carry a Field inner under a payload prefix.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage B — Path-attributed provenance

### Task 2: Add `path_prov` to `ForwardState` (inert)

**Files:**
- Modify: `boot/compiler/ownership.tw` (`ForwardState` `:618`, and every `ForwardState.{ … }` constructor site — search `ForwardState.{`)
- Test: existing suites (regression only)

- [ ] **Step 1: Add the field and accessors**

Extend `ForwardState` (`:618`):

```tw
type ForwardState = .{
  own: Dict<Int, Int>,
  valid: Dict<Int, Bool>,
  prov: Dict<Int, Vector<Int>>,
  field_own: Dict<Int, ff.FieldMap>,
  path_prov: Dict<Int, Dict<Int, Vector<Int>>>,   // local -> PathKey -> origins
}
```

Add near `field_own_get` (`:625`):

```tw
fn path_prov_get(st: ForwardState, id: Int) Dict<Int, Vector<Int>> {
  case st.path_prov.get(id) {
    .Some(m) => m,
    .None => Dict.new(),
  }
}
fn set_path_prov(st: ForwardState, id: Int, m: Dict<Int, Vector<Int>>) ForwardState {
  st.path_prov[id] = m
  st
}
fn clear_path_prov(st: ForwardState, id: Int) ForwardState {
  st.path_prov[id] = Dict.new()
  st
}
```

- [ ] **Step 2: Seed `path_prov` empty at every constructor**

Search `ForwardState.{` (the fixpoint entry builder near `:2328`, the initial-entry builder, and any join builder) and add `path_prov: Dict.new()` (or the joined map — Task 9 fills joins). For now every site seeds `Dict.new()`.

- [ ] **Step 3: Fold `path_prov` into the `set_own_st` choke point**

In `set_own_st` (`:642`), when the shell leaves Unique, also clear path_prov:

```tw
st = if o.own_tag() != 0 {
  st.clear_field_own(id).clear_path_prov(id)
} else {
  st
}
```

- [ ] **Step 4: Run the suite — expect PASS (no behavior change yet)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: all green; `path_prov` is populated empty and read nowhere.

- [ ] **Step 5: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw
git commit -m "ownership: add inert path_prov map to ForwardState

Mirrors field_own (local -> PathKey -> origins); cleared through the same
set_own_st choke point. Populated empty for now; later tasks split shell vs
field provenance and read it for return-path classification.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

### Task 3: Split shell vs field provenance in aggregate builders

This is the Blocker 3 groundwork; the observable `Retained`→`Borrowed` flip and the
`ret=OwnedFresh` classification only land once Task 5 removes the return-publish, so
**the shell/deep observable tests live in Task 5, not here.** Task 3 is a
behavior-preserving refactor: with the return still publishing, `publish_local` now
cascades `path_prov` origins, so a returned wrapper's param stays `Retained` exactly
as today — the existing suite must stay **green**.

> **Execution unit:** Tasks 3, 4, and 5 form one unit. Implement 3 → 4 → 5, running
> the suite after each (Tasks 3–4 keep it green; Task 5 re-baselines and adds the
> shell/deep tests). If your workflow demands a failing test per commit, treat 3–5
> as a single commit; otherwise commit each with the "green, no regression" gate for
> 3–4 and the re-baseline for 5.

**Files:**
- Modify: `boot/compiler/ownership.tw` — builders `ARecord` `:1048`, `AArrayLit` `:1082`, `ARecordUpdate` `:1141`; rebind/projection `AAssign` `:1167`, `init_hinge` `:710`, `ARecordGet` `:1107`; `publish_local` `:689`
- Test: none new in Task 3 (regression only; the split's observable tests are Task 5)

- [ ] **Step 1: (no new test) Confirm the current suite is green as a baseline**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: green. Task 3 must preserve this (the flip is Task 5).

- [ ] **Step 2: Route field origins to `path_prov`, empty the shell prov**

In `ARecord` (`:1048`) replace the `set_prov_st(result, origins)` (which unions field origins) with shell-empty + per-field path_prov. Current code computes `origins` and grafts `field_own`; add path_prov grafting in the same loop and set shell prov empty:

```tw
.ARecord(_, fields) => {
  operands: Vector<Atom> = collect fa in fields { fa.value }
  for fa in fields {
    st = .field_store(fa.value, last)
  }
  st = .set_result(result, .Unique)
  st = .set_prov_st(result, [])   // SHELL prov: fresh shell aliases no param

  rf := ff.empty()
  pp: Dict<Int, Vector<Int>> = Dict.new()
  for fa in fields {
    if st.single_retention(fa.value, last, operands) {
      seg := ff.PathSeg.Field(fa.field.id)
      rf = .graft(seg, st.atom_field_own(fa.value))
      // path_prov: this field's origins (its own shell prov + rebased inner path_prov)
      pp = graft_path_prov(pp, seg, st, fa.value)
    }
  }
  st = if rf.is_empty() { st } else { st.set_field_own(result, rf) }
  st.set_path_prov(result, pp)
}
```

Add the `graft_path_prov` helper (near the aggregate builders) that mirrors `ff.graft` for provenance — it writes the value's shell prov at `[seg]` (as `Some([...])`, possibly `[]` for fresh) and rebases the value's inner `path_prov` under `seg`:

`graft_path_prov` must mirror `ff.graft` **exactly**, prefix-dependently, so
`path_prov` never has a path `field_own` lacks or vice-versa (design invariant,
`phase5-design.md`): under a `Field` prefix, rebase the value's `Elem`/`Val` inner
paths and drop unrepresentable nested `Field`; under a `Payload` prefix, rebase the
value's `Field` inner path and drop `Elem`/`Val`. The `[prefix]` entry is always
the value's shell prov (empty vector = proven fresh):

```tw
// Copy `src`'s provenance under `prefix` into `pp`, mirroring ff.graft's
// representable-inner rules so pp and field_own stay in lockstep.
fn graft_path_prov(pp: Dict<Int, Vector<Int>>, prefix: ff.PathSeg, st: ForwardState, a: Atom) Dict<
  Int, Vector<Int>,
> {
  pp[ff.path_key(ff.AccessPath.{ segs: [prefix] })] = prov_of(st.prov, a) // [] = fresh
  case atom_local_id(a) {
    .Some(sid) => {
      for k, v in st.path_prov_get(sid) {
        inner := ff.path_of_key(k)
        if inner.segs.len() == 1 {
          keep := case prefix {
            .Field(_) => case inner.segs[0] { .Field(_) => false, _ => true },   // rebase Elem/Val
            .Payload(_, _) => case inner.segs[0] { .Field(_) => true, _ => false }, // rebase Field
            _ => false,
          }
          if keep {
            pp[ff.path_key(ff.AccessPath.{ segs: [prefix, inner.segs[0]] })] = v
          }
        }
      }
    },
    .None => {},
  }
  pp
}
```

For `AArrayLit` (`:1082`): set shell prov `[]`, and **when the all-owned rule sets
`[Elem]:Unique`, record `path_prov[Elem]` too** — the union of the element atoms'
origins (empty ⇒ proven-fresh elements). This preserves the invariant "every unique
`field_own` path has a `path_prov` entry," so publishing the array leaks any param
held in an element even though `[Elem]` is not a `ret_paths` candidate:

```tw
// inside the all_owned branch, alongside set_field_own(result, [Elem]:Unique):
elem_origins: Vector<Int> = []
for e in elems { elem_origins = union_sorted(elem_origins, prov_of(st.prov, e)) }
epp: Dict<Int, Vector<Int>> = Dict.new()
epp[ff.path_key(ff.elem_path())] = elem_origins
st = st.set_path_prov(result, epp)
```

For `ARecordUpdate` (`:1141`): shell prov should follow the base's shell prov (a
rebuilt shell of a param-aliased record still aliases that param at the shell level
only via base) — keep `origins := prov_of(st.prov, base)` for the shell (drop the
`v` union). **Capture base's `path_prov` up front**, in the same place the current
code captures `base_fields := st.atom_field_own(base)` and **before `consume_base`**
(which invalidates base and — if a later edit routes base through the `set_own_st`
choke point — would clear it). Then build `path_prov[result]` as base's captured
`path_prov` with `[.f]*` removed, grafting the replacement under `[.f]` only when
single-retention, mirroring the `field_own` block directly above it:

```tw
.ARecordUpdate(base, f, v, _, _) => {
  origins := prov_of(st.prov, base)          // shell-only: drop the old union with v
  base_fields := st.atom_field_own(base)
  base_pp := case atom_local_id(base) {       // capture BEFORE consume_base
    .Some(bid) => st.path_prov_get(bid),
    .None => Dict.new(),
  }
  st = .consume_base(result, base, last)
  st = .field_store(v, last)
  st = .set_prov_st(result, origins)
  st = if own_is_unique(st.own, result) {
    rf := base_fields.remove_prefix(.Field(f.id))
    pp := remove_prefix_pp(base_pp, ff.PathSeg.Field(f.id))
    if st.single_retention(v, last, [base, v]) {
      rf = .graft(.Field(f.id), st.atom_field_own(v))
      pp = graft_path_prov(pp, ff.PathSeg.Field(f.id), st, v)
    }
    st.set_field_own(result, rf).set_path_prov(result, pp)
  } else {
    st
  }
  st
}
```

This is behavior-preserving under the Return-publish (Task 3): the field origin that
moved out of the shell union now lives in `path_prov[[.f]]`, and `publish_local`
(Step 4) publishes `path_prov` origins, so publishing the updated record still leaks
`v`. The overwrite-drops-the-stale-origin behavior is locked by a dedicated test in
Task 7 (which is also the lockstep-invariant regression guard — see the invariant
note below).

**Lockstep invariant — which directions are self-guarding (read before you "fix" a
mismatch).** The `field_own ⇄ path_prov` key-set invariant is maintained by hand
across the builders/projections. It is worth knowing that a *key-set* drift degrades
to soundness, not unsoundness, so you don't over-correct:

- **`classify_path_own` iterates `field_own` keys**, then reads `path_prov.get(k)`
  three-way. An *extra* `path_prov` key (no matching `field_own` path) is never read;
  a *missing* one reads `.None` ⇒ drop. Both safe.
- **`publish_local` iterates `path_prov`** and publishes every origin. A *stale*
  `path_prov` key merely over-publishes ⇒ conservative, safe.

The genuinely dangerous failure is not a key-set drift but a **wrong origin** grafted
for a *real* `field_own` path (e.g. `graft_path_prov` rebasing the wrong inner seg,
or an overwrite leaving a stale origin under a reused key). That is a logic bug, and
it is what the Task 7 tests target directly: multi-accumulator attribution
(`[.state]=p0`, `[.accum]=p1`), same-origin-twice ⇒ drop, and the ARecordUpdate
overwrite ⇒ stale origin dropped. Keep those green rather than adding a heavyweight
runtime invariant checker.

- [ ] **Step 3: Thread `path_prov` through rebinding and projection**

`path_prov` must ride the same copy/project/remove flows as `field_own`, or a
unique rebind loses/stales its provenance and the projection returns wrong origins.
Wherever Phase 4 copies/projects/removes `field_own`, add the parallel `path_prov`:

- **`AAssign(local, a)`** (`:1167`): today it copies `field_own` when the atom is
  Unique. Also copy `path_prov`: `st = st.set_path_prov(local.id, st.path_prov_get(aid))`
  for the atom's local `aid` (same Unique guard).
- **`AInit` / `init_hinge` move branch** (`:710`): today the move copies the
  source's `field_own`. Also copy the source's `path_prov` to `result` on the move
  branch; the alias branch leaves `path_prov[result]` empty (result is Shared).
- **`ARecordGet(base, f)`** (`:1107`): on **move** (last-use / quartet / transport),
  the result takes the projected subtree. Compute `proj_pp := project_path_prov(base.path_prov, .Field(f))`
  (the `.{ shell, inner }` shape) and use **both** halves — `inner` for the result's
  `path_prov`, `shell` for the result's shell prov:

  ```tw
  proj_pp := project_path_prov(st.path_prov_get(bid), ff.PathSeg.Field(f.id))
  st = st.set_path_prov(result, proj_pp.inner)
  // Result shell prov from the PROJECTED FIELD's provenance (not base's shell prov):
  st = st.set_prov_st(result, case proj_pp.shell {
    .Some(o) => o,
    .None => prov_of(st.prov, base),   // opaque base (e.g. a param) -> today's behavior
  })
  st = st.set_path_prov(bid, remove_prefix_pp(st.path_prov_get(bid), ff.PathSeg.Field(f.id)))
  ```

  On **borrow**, result is Shared (no prov claim) and `base`'s `[.f]*` subtree is
  stripped from both `field_own` and `path_prov`.

Add here (reused by Task 13) the two shared helpers with a **single, standardized**
shape:

- `project_path_prov(pp, seg) .{ shell: Vector<Int>?, inner: Dict<Int, Vector<Int>> }`
  — mirror `ff.project`: the `[seg]` entry becomes `shell`, each strict descendant
  `[seg, X]` becomes `inner[X]`.
- `remove_prefix_pp(pp, seg) Dict<Int, Vector<Int>>` — mirror `ff.remove_prefix`.

Both consumers (this `ARecordGet` step and Task 13's `seed_payload_binding`) use the
`.{ shell, inner }` return (`.inner` for the sub-map, `.shell` for shell prov). This
step is still **inert for existing tests**: opaque param bases carry no `path_prov`,
so the projected-field fallback keeps result prov unchanged; the refinement only
bites once builders populate `path_prov` and a caller recovers a real record
(Tasks 7–13).

- [ ] **Step 4: Publish `path_prov` origins in `publish_local`**

In `publish_local` (`:689`), also publish path_prov origins. `set_own_st(id, .Shared)` clears `path_prov[id]` via the choke point, so **capture the map before** clearing:

```tw
fn publish_local(st: ForwardState, id: Int) ForwardState {
  pp := st.path_prov_get(id)                 // capture before the choke point clears it
  shell := case st.prov.get(id) { .Some(o) => o, .None => [] }
  st = .set_own_st(id, .Shared)
  for o in shell { st = .set_own_st(o, .Shared) }
  for k, os in pp {
    for o in os { st = .set_own_st(o, .Shared) }
  }
  st
}
```

- [ ] **Step 5: Run — expect GREEN (behavior-preserving refactor)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: the **whole suite stays green**. With the return still publishing, `publish_local` now cascades `path_prov`, so a returned wrapper's param remains `Retained` exactly as before — no regression. Opaque param bases carry no `path_prov`, so the Step-3 projection refinement is inert for existing tests. The observable flip is Task 5. If any test regresses (other than an intended Task-5 re-baseline, which is not touched yet), stop and investigate.

- [ ] **Step 6: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw
git commit -m "ownership: split shell vs field provenance in aggregate builders

A freshly-constructed record/variant/array now carries an empty shell prov and
routes each field's origins into path_prov under [.f]; publish_local publishes
path_prov origins so whole-value publication still leaks fields. Groundwork for
per-path return classification; the returned-wrapper ret flip lands with the
Return-publish removal.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

### Task 4: `AVariant` grafts payload facts + path_prov

Part of the Tasks 3–5 execution unit. The observable variant-payload fact is
verified by Task 8's `ret_paths` test (which needs Task 5's return semantics + the
classifier); Task 4 is the transfer change plus a green regression check.

**Files:**
- Modify: `boot/compiler/ownership.tw` (`AVariant` transfer `:1073`)
- Test: none new in Task 4 (regression only; the variant `ret_paths` test is Task 8)

- [ ] **Step 1: (no new test) Baseline is green**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: green (carried from Task 3).

- [ ] **Step 2: Graft payload facts in `AVariant`**

Replace the `AVariant` transfer (`:1073`) — keep shell Unique, empty shell prov, and graft each single-retention payload under `[Payload(vid.id, i)]`:

```tw
.AVariant(_, vid, args) => {
  for a in args { st = .field_store(a, last) }
  st = .set_result(result, .Unique)
  st = .set_prov_st(result, [])   // shell aliases no param

  rf := ff.empty()
  pp: Dict<Int, Vector<Int>> = Dict.new()
  for a, i in args {
    if st.single_retention(a, last, args) {
      seg := ff.PathSeg.Payload(vid.id, i)
      rf = .graft(seg, st.atom_field_own(a))
      pp = graft_path_prov(pp, seg, st, a)
    }
  }
  st = if rf.is_empty() { st } else { st.set_field_own(result, rf) }
  st.set_path_prov(result, pp)
}
```

- [ ] **Step 3: Run — expect GREEN (behavior-preserving)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: green (return still publishes, so a returned variant still cascades its payload origins). No regression.

- [ ] **Step 4: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw
git commit -m "ownership: AVariant carries tagged payload facts + path_prov

An AVariant(tag, [payload...]) result now grafts single-retention payload
ownership under [Payload(tag, i)] with path-attributed provenance, so return
classification and match-arm seeding can recover variant-payload state.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage C — Return is a non-retaining leaf

### Task 5: Remove `Return`'s publication (keep `ValueBreak`'s)

**Files:**
- Modify: `boot/compiler/ownership.tw` (`forward_block` `:1393`)
- Test: `boot/tests/suites/cfg_summary_suite.tw` (re-baseline)

- [ ] **Step 1: Re-baseline the existing escape tests**

The existing test `t1 aggregate-escape: fn f(x) { Wrapper.{x} } -> p0 retain (blocker 1)` asserts `Retained`. Under Decision 3 a returned wrapper does **not** retain its param. Change its expectation and rename:

```tw
.test(
  "returned wrapper param is Borrowed (return is a hand-off, not retention)",
  fn() {
    b := b_reg()
    body: AnfExpr = .Let(lid(1), wrapper_record(lid(0)), .Atom(.ALocal(lid(1))))
    s := summ1("f", 1, body)
    try assert.equal(p_escape(s, 0), escape_tag(.Borrowed))
    .Ok({})
  },
)
```

Audit the rest of `cfg_summary_suite.tw` for any test asserting `Retained`/`MayAliasParams` that depended on the return-publish (e.g. `fn id(x){x}` retain expectations). A genuine leak (`global_set G0 = x`) must STAY `Retained` — do not change those. Update only return-hand-off cases.

Also add the shell/deep observable tests that the Task 3–4 changes enable once the return-publish is gone (these were deliberately deferred out of Tasks 3/4 so no test is committed RED):

```tw
.test(
  "fresh two-field record: ret = OwnedFresh (shell), not MayAliasParams",
  fn() {
    two := AnfOp.ARecord(TypeId.{ id: 0 }, [
      .{ field: FieldId.{ id: 0 }, value: .ALocal(lid(0)) },
      .{ field: FieldId.{ id: 1 }, value: .ALocal(lid(1)) },
    ])
    s := summ1("f", 2, .Let(lid(2), two, .Atom(.ALocal(lid(2)))))
    try assert.equal(ret_tag(s.ret), 0)       // OwnedFresh shell
    .Ok({})
  },
)
.test(
  "returned variant with single-retention payload: ret = OwnedFresh shell",
  fn() {
    vop := AnfOp.AVariant(TypeId.{ id: 0 }, VariantId.{ id: 5 }, [.ALocal(lid(0))])
    s := summ1("f", 1, .Let(lid(1), vop, .Atom(.ALocal(lid(1)))))
    try assert.equal(ret_tag(s.ret), 0)
    .Ok({})
  },
)
```

- [ ] **Step 2: Run — expect RED (return still publishes)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: the re-baselined escape test and the two new `ret == OwnedFresh` tests fail (`ret` is still `MayAliasParams`/param still `Retained` while the return publishes).

- [ ] **Step 3: Remove the Return publish**

In `forward_block` (`:1393`):

```tw
case blk.terminator {
  // Return hands the value to the CALLER; it is not a callee leak. The caller's
  // ret / ret_paths handling accounts for it. Return blocks are CFG leaves, so
  // this does not affect intra-function joins (Case T). ValueBreak still targets
  // a real post-loop successor, so it still publishes.
  .Some(.ValueBreak(a)) => {
    st = .publish_atom(a)
  },
  _ => {},
}
```

- [ ] **Step 4: Run — expect PASS (re-baseline + Task 3/4 tests green)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: returned-wrapper param `Borrowed`; fresh-wrapper `ret` = `OwnedFresh`; variant `ret` = `OwnedFresh`; genuine leaks still `Retained`. Full suite green.

- [ ] **Step 5: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "ownership: Return is a non-retaining leaf, not a publication

A returned value is handed to the caller, so it no longer demotes its param
origins to Retained; genuine leaks (globals/closures/aggregates/aliasing) still
mark Retained independently, and return blocks being CFG leaves keeps Case T
join behavior intact. ValueBreak still publishes (real post-loop successor).
Re-baselines the returned-wrapper escape expectations.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage D — Summary `ret_paths` schema + classification

### Task 6: Add the `ret_paths` schema (seeded, compared, rendered)

**Files:**
- Modify: `boot/compiler/ownership.tw` (types near `:45`; `summarize_function` `:2355` returns `ret_paths: []` for now), `boot/compiler/summary.tw` (`conservative_summary` `:58`, `same_summary` `:113`, `render_summary` `:332`)
- Test: existing suites (regression)

- [ ] **Step 1: Add the types and extend `Summary`**

In `ownership.tw` near `:45`:

```tw
pub type ReturnOwn = { OwnedFresh, OwnedFromParam(Int) }
pub type RetVia = { Direct, Variant(Int, Int) }        // Direct | Variant(tag, payload_index)
pub type ReturnPathOwn = .{ via: RetVia, field: Int?, own: ReturnOwn }
pub type Summary = .{ params: Vector<ParamSummary>, ret: ReturnEffect, ret_paths: Vector<ReturnPathOwn> }
```

Update the `Summary.{ params, ret }` construction in `summarize_function` (`:2355`) to `Summary.{ params, ret, ret_paths: [] }` for now.

- [ ] **Step 2: Seed / compare / render in `summary.tw`**

`conservative_summary` (`:58`): `Summary.{ params, ret: .Shared, ret_paths: [] }`.

`same_summary` (`:113`): after the param + `return_eq` checks, compare `ret_paths` with a canonical-sorted equality. Add:

```tw
fn ret_own_eq(a: ownership.ReturnOwn, b: ownership.ReturnOwn) Bool {
  case a {
    .OwnedFresh => case b { .OwnedFresh => true, _ => false },
    .OwnedFromParam(ka) => case b { .OwnedFromParam(kb) => ka == kb, _ => false },
  }
}
// Canonical tuple ordering (no lossy numeric packing): (via-kind, tag, index,
// field, own). Each component uses Int.compare, which returns Order.
fn via_kind(v: ownership.RetVia) Int { case v { .Direct => 0, .Variant(_, _) => 1 } }
fn via_tag(v: ownership.RetVia) Int { case v { .Direct => 0 - 1, .Variant(t, _) => t } }
fn via_idx(v: ownership.RetVia) Int { case v { .Direct => 0 - 1, .Variant(_, i) => i } }
fn field_key(f: Int?) Int { case f { .Some(x) => x, .None => 0 - 1 } }
fn own_key(o: ownership.ReturnOwn) Int { case o { .OwnedFresh => 0 - 1, .OwnedFromParam(k) => k } }

// Comparator returning Order, as Vector.sort_by requires (boot/prelude/vector.tw).
fn cmp_ret_path(a: ownership.ReturnPathOwn, b: ownership.ReturnPathOwn) Order {
  c1 := Int.compare(via_kind(a.via), via_kind(b.via))
  case c1 { .Eq => {}, _ => return c1 }
  c2 := Int.compare(via_tag(a.via), via_tag(b.via))
  case c2 { .Eq => {}, _ => return c2 }
  c3 := Int.compare(via_idx(a.via), via_idx(b.via))
  case c3 { .Eq => {}, _ => return c3 }
  c4 := Int.compare(field_key(a.field), field_key(b.field))
  case c4 { .Eq => {}, _ => return c4 }
  Int.compare(own_key(a.own), own_key(b.own))
}
fn sort_ret_paths(v: Vector<ownership.ReturnPathOwn>) Vector<ownership.ReturnPathOwn> {
  v.sort_by(cmp_ret_path)
}
fn ret_paths_eq(a: Vector<ownership.ReturnPathOwn>, b: Vector<ownership.ReturnPathOwn>) Bool {
  if a.len() != b.len() { return false }
  sa := sort_ret_paths(a)
  sb := sort_ret_paths(b)
  for r, i in sa {
    o := sb[i]
    same := via_kind(r.via) == via_kind(o.via) and via_tag(r.via) == via_tag(o.via)
      and via_idx(r.via) == via_idx(o.via) and field_key(r.field) == field_key(o.field)
      and ret_own_eq(r.own, o.own)
    if !same { return false }
  }
  true
}
```

`Vector.sort_by(xs, cmp: fn(T,T) Order)` is confirmed at `boot/prelude/vector.tw:343`; `Order = { Lt, Eq, Gt }` and `Int.compare(a,b) Order` at `boot/prelude/int.tw:3`. In `same_summary`, `return return_eq(a.ret, b.ret) and ret_paths_eq(a.ret_paths, b.ret_paths)`.

`render_summary` (`:332`): append a `ret_paths=…` clause. Add a renderer:

```tw
fn render_ret_paths(rps: Vector<ownership.ReturnPathOwn>) String {
  parts: Vector<String> = []
  for r in sort_ret_paths(rps) {
    via := case r.via { .Direct => "", .Variant(t, i) => "V${t}[${i}]" }
    fld := case r.field { .Some(f) => ".f${f}", .None => "" }
    own := case r.own { .OwnedFresh => "fresh", .OwnedFromParam(k) => "from(p${k})" }
    parts = .append("${via}${fld}=${own}")
  }
  if parts.len() == 0 { "" } else { "  ret_paths=${parts.join(" ")}" }
}
```

Append its result to the `render_summary` output string.

- [ ] **Step 3: Run — expect PASS (regression only)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: green; `ret_paths` empty everywhere so rendering is unchanged and equality is unaffected.

- [ ] **Step 4: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/compiler/summary.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/compiler/summary.tw
git commit -m "ownership/summary: add ret_paths schema (seed, compare, render)

Adds ReturnOwn/RetVia/ReturnPathOwn and Summary.ret_paths, seeded empty and
compared canonical-sorted so the SCC fixpoint stays stable. Classification and
consumption land next.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

### Task 7: Classify `Direct` record return paths + shell-level `ret`

**Files:**
- Modify: `boot/compiler/ownership.tw` (`summarize_function` `:2316`–`:2356`)
- Test: `boot/tests/suites/cfg_summary_suite.tw`

- [ ] **Step 1: Write the failing test — multi-accumulator attribution**

```tw
.test(
  "fn f(x,y){ Record{f0:x, f1:y} } -> ret_paths [.f0]=from(p0) [.f1]=from(p1)",
  fn() {
    two := AnfOp.ARecord(TypeId.{ id: 0 }, [
      .{ field: FieldId.{ id: 0 }, value: .ALocal(lid(0)) },
      .{ field: FieldId.{ id: 1 }, value: .ALocal(lid(1)) },
    ])
    s := summ1("f", 2, .Let(lid(2), two, .Atom(.ALocal(lid(2)))))
    try assert.equal(ret_tag(s.ret), 0)              // OwnedFresh shell
    try assert.equal(s.ret_paths.len(), 2)
    // helper below extracts (field, own-param) pairs, sorted
    got := ret_path_pairs(s.ret_paths)
    try assert.equal(same_ints(got, [0, 0, 1, 1]), true) // [f0->p0, f1->p1] flattened
    .Ok({})
  },
)
```

Add the helper:

```tw
// Flatten Direct ret_paths to [field, param, field, param, ...] sorted by field.
fn ret_path_pairs(rps: Vector<ownership.ReturnPathOwn>) Vector<Int> {
  pairs: Vector<Int> = []
  for r in rps {
    case r.via {
      .Direct => case r.field {
        .Some(f) => case r.own {
          .OwnedFromParam(k) => { pairs = .append(f); pairs = .append(k) },
          .OwnedFresh => { pairs = .append(f); pairs = .append(0 - 1) },
        },
        .None => {},
      },
      .Variant(_, _) => {},
    }
  }
  pairs
}
```

- [ ] **Step 2: Run — expect RED (`ret_paths` empty)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: `ret_paths.len()` is 0.

- [ ] **Step 3: Classify shell `ret` and `Direct` paths**

In `summarize_function`, the return loop (`:2318`–`:2352`) currently computes only `ret`. Replace the per-return-block `r` computation so it also builds `ret_paths`. Keep the shell `ret` classification prov-based but shell-only (shell prov is now empty for fresh records, so `MayAliasParams` only fires when the returned local's own shell aliases a param, e.g. `return x`). Add, using `body` (the body-only state) and the returned atom `a`:

```tw
// after computing `body`:
// CAPTURE the CfgFunction's params BEFORE the loop — inside the loop `.Field(f)`
// binds `f` to the FIELD id and would shadow the outer CfgFunction `f`.
fn_params := f.params            // Vector<LocalId>
rp_here: Vector<ReturnPathOwn> = []
case atom_local_id(a) {
  .Some(aid) => {
    fm := body.field_own_get(aid)
    pp := body.path_prov_get(aid)
    for k in fm.sorted_keys() {
      p := ff.path_of_key(k)
      // Direct record field: single Field seg.
      if p.segs.len() == 1 {
        case p.segs[0] {
          .Field(fid) => case classify_path_own(pp, k, fn_params) {
            .Some(own) => rp_here = .append(ReturnPathOwn.{ via: .Direct, field: .Some(fid), own }),
            .None => {},
          },
          _ => {},
        }
      }
    }
  },
  .None => {},
}
```

Add `classify_path_own` (three-way path_prov, Decision 2):

```tw
// params is CfgFunction.params : Vector<LocalId> (NOT Vector<Param>).
// .None => drop; Some([]) => OwnedFresh; Some([k]) => OwnedFromParam(k); Some([multi]) => drop.
fn classify_path_own(pp: Dict<Int, Vector<Int>>, k: Int, params: Vector<LocalId>) ReturnOwn? {
  case pp.get(k) {
    .None => .None,
    .Some(os) => cond {
      os.len() == 0 => .Some(.OwnedFresh),
      os.len() == 1 => case param_index_of(params, os[0]) {
        .Some(pi) => .Some(.OwnedFromParam(pi)),
        .None => .None,          // origin is not a parameter -> drop
      },
      _ => .None,
    },
  }
}

// Position of the param whose LocalId.id == local_id, or .None.
fn param_index_of(params: Vector<LocalId>, local_id: Int) Int? {
  for p, i in params {
    if p.id == local_id { return .Some(i) }
  }
  .None
}
```

This mirrors what `prov_to_indices` already does for a whole origin vector (it maps origin local-ids to param indices over `f.params: Vector<LocalId>`); `param_index_of` is just the single-id variant. `f.params` at the call sites is the `CfgFunction`'s `Vector<LocalId>` — do **not** introduce a `Vector<Param>` binding or shadow `f.params`.

Join `rp_here` across return sites with a per-`(via,field)` meet (a path survives only if present & `own`-compatible on every return block). Maintain an accumulator mirroring the existing `ret`/`seen` join:

```tw
ret_paths_acc = if seen { meet_ret_paths(ret_paths_acc, rp_here) } else { rp_here }
```

Add `meet_ret_paths` (intersection by `(via,field)`, `own` must match else drop). Finally return `Summary.{ params, ret, ret_paths: ret_paths_acc }`.

- [ ] **Step 4: Run — expect PASS**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: the multi-accumulator test passes; `[.f0]=from(p0)`, `[.f1]=from(p1)`, shell `OwnedFresh`.

- [ ] **Step 5: Add per-path-prov negative tests**

```tw
.test(
  "fn f(x){ Record{f0:x, f1:x} } -> no ret_paths (same origin twice = alias)",
  fn() {
    two := AnfOp.ARecord(TypeId.{ id: 0 }, [
      .{ field: FieldId.{ id: 0 }, value: .ALocal(lid(0)) },
      .{ field: FieldId.{ id: 1 }, value: .ALocal(lid(0)) },
    ])
    s := summ1("f", 1, .Let(lid(1), two, .Atom(.ALocal(lid(1)))))
    try assert.equal(s.ret_paths.len(), 0)  // single_retention fails -> no field claim
    .Ok({})
  },
)
```

Also add the **ARecordUpdate overwrite** test — the only task that exercises
`ARecordUpdate`'s `path_prov` carry + reattribution, and the lockstep-invariant
regression guard for the stale-origin direction. The base is a **fresh** record
(so `ret=OwnedFresh`, not a param-aliased shell), `f0` is sourced from `p0` and is
last-used at the build, and `f1` is overwritten with a **fresh** value bound in an
intervening `Let` (so no reuse perturbs `f0`'s liveness). The overwrite must (a)
carry `[.f0]=from(p0)` through unchanged, (b) **drop** the stale `[.f1]=from(p1)`,
and (c) attribute `[.f1]` to the fresh replacement:

```tw
.test(
  "fn f(x,y){ r := Rec{f0:x,f1:y}; r.f1 = fresh; r } -> [.f0]=from(p0) [.f1]=fresh (overwrite drops p1)",
  fn() {
    rec := AnfOp.ARecord(TypeId.{ id: 0 }, [
      .{ field: FieldId.{ id: 0 }, value: .ALocal(lid(0)) },
      .{ field: FieldId.{ id: 1 }, value: .ALocal(lid(1)) },
    ])
    fresh := AnfOp.ARecord(TypeId.{ id: 1 }, [])   // fresh empty record for the new f1
    // ARecordUpdate(base, field, value, in_place: Bool, tid: TypeId)
    upd := AnfOp.ARecordUpdate(.ALocal(lid(2)), FieldId.{ id: 1 }, .ALocal(lid(3)), false, TypeId.{ id: 0 })
    body: AnfExpr = .Let(lid(2), rec, .Let(lid(3), fresh, .Let(lid(4), upd, .Atom(.ALocal(lid(4))))))
    s := summ1("f", 2, body)
    try assert.equal(ret_tag(s.ret), 0)                        // OwnedFresh shell (base is fresh)
    got := ret_path_pairs(s.ret_paths)
    try assert.equal(same_ints(got, [0, 0, 1, 0 - 1]), true)   // f0->p0, f1->fresh(-1); p1 dropped
    .Ok({})
  },
)
```

If this instead yields `[0, 0, 1, 1]`, `remove_prefix_pp` failed to drop the stale
`[.f1]=from(p1)` — that is the lockstep bug the test exists to catch, not a flaky
fixture.

Run again — expect PASS.

- [ ] **Step 6: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "ownership: classify Direct record return paths + shell-level ret

Reads each returned record field's field_own with a three-way path_prov
(absent=drop, empty=OwnedFresh, single=OwnedFromParam, multi=drop), joins across
return sites, and classifies ret as shell ownership so a fresh wrapper is
OwnedFresh with per-field ret_paths instead of MayAliasParams.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

### Task 8: Classify `Variant` payload return paths

**Files:**
- Modify: `boot/compiler/ownership.tw` (the same return loop in `summarize_function`)
- Test: `boot/tests/suites/cfg_summary_suite.tw`

- [ ] **Step 1: Write the failing test — variant payload path**

The payload is a fresh record `{f0:x}`, so the returned variant's `field_own` has
**two** paths: `[Payload(7,0)]` (the payload shell — fresh, `path_prov []`) and
`[Payload(7,0), .f0]` (from `p0`). The classifier emits **both** as distinct
`ret_paths` — `V7[0]=fresh` (`field: None`) and `V7[0].f0=from(p0)`
(`field: Some(0)`). Do **not** expect a single collapsed entry.

```tw
.test(
  "fn f(x){ Variant#7(Record{f0:x}) } -> ret_paths V7[0]=fresh AND V7[0].f0=from(p0)",
  fn() {
    inner := AnfOp.ARecord(TypeId.{ id: 0 }, [.{ field: FieldId.{ id: 0 }, value: .ALocal(lid(0)) }])
    vop := AnfOp.AVariant(TypeId.{ id: 0 }, VariantId.{ id: 7 }, [.ALocal(lid(1))])
    body: AnfExpr = .Let(lid(1), inner, .Let(lid(2), vop, .Atom(.ALocal(lid(2)))))
    s := summ1("f", 1, body)
    try assert.equal(s.ret_paths.len(), 2)
    // the payload shell is fresh (field: None):
    try assert.equal(has_variant_shell_fresh(s.ret_paths, 7, 0), true)
    // the payload field is from p0 (field: Some(0)):
    try assert.equal(has_variant_field_from(s.ret_paths, 7, 0, 0, 0), true)
    .Ok({})
  },
)
```

Add predicates `has_variant_shell_fresh(rps, tag, idx)` (some ret_path with
`via=Variant(tag,idx)`, `field=None`, `own=OwnedFresh`) and
`has_variant_field_from(rps, tag, idx, field, param)` to the suite.

- [ ] **Step 2: Run — expect RED**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: `ret_paths.len()` is 0 (variant paths not classified yet).

- [ ] **Step 3: Add the variant-payload classification branch**

In the same `for k in fm.sorted_keys()` loop, handle payload-prefixed paths (`Payload(tag,i)` shell and `Payload(tag,i), Field(f)`):

```tw
// `fn_params` is captured once before the loop (Task 7); do NOT use `f.params`
// here — `.Field(fid)` below binds the field id, and the outer `f` is the CfgFunction.
if p.segs.len() >= 1 {
  case p.segs[0] {
    .Payload(tag, i) => {
      fld: Int? = if p.segs.len() == 2 {
        case p.segs[1] { .Field(fid) => .Some(fid), _ => .None }
      } else { .None }
      case classify_path_own(pp, k, fn_params) {
        .Some(own) => rp_here = .append(ReturnPathOwn.{ via: .Variant(tag, i), field: fld, own }),
        .None => {},
      }
    },
    _ => {},  // Field handled above; Elem/Val are not ret_path candidates
  }
}
```

Restructure the loop so `Field`-single and `Payload*` are both handled off `p.segs[0]` (fold Task 7's Field branch and this into one `case p.segs[0]`).

- [ ] **Step 4: Run — expect PASS**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: the variant-payload test passes.

- [ ] **Step 5: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "ownership: classify variant-payload return paths

Reads [Payload(tag,i)] and [Payload(tag,i), .f] facts on the returned variant
local into RetVia.Variant(tag,i) ret_paths with three-way path_prov, so
Result-payload state transport (Ok[0].state) is summarized with its tag.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage E — Fixpoint discipline

### Task 9: Suppress in-SCC `ret_paths` (order-independent) + strip on cap; join `path_prov`

**Files:**
- Modify: `boot/compiler/summary.tw` (`run_scc` `:286` — pass `scc_set` as `suppress`, strip on cap), `boot/compiler/ownership.tw` (thread `suppress: Dict<Int,Bool>` through `summarize_function` → `run_fixpoint` → `forward_*` → `transfer_op`/`transfer_call` → `transfer_summarized_call`; add `join_entry_path_prov` sibling to `join_entry_field_own`)
- Test: `boot/tests/suites/cfg_summary_suite.tw`

> **Note:** threading a new `suppress` parameter through the transfer stack touches
> several signatures. Add it with a `Dict.new()` default at the non-SCC call sites
> (`summ1` in the suite, `analyze`/`analyze_with_summaries` if they call
> `summarize_function`) so only `run_scc` passes a non-empty set.

- [ ] **Step 1: Write the failing recursive test**

Two assertions are needed. Determinism alone would not catch a *consistently*
wrong speculative path, so also assert the **specific** sound value: a self-thread
recovered only through the recursive edge must be **under-approximated to empty**,
while a field owned directly (not via the recursive call) is still classified.

The fixture has **two return sites** so the meet exposes the hazard cleanly:

```text
fn g(x, y) {
  if y {                      // recursive return site
    r := g(x, y)              // r's ret_paths are SUPPRESSED in-SCC
    return Record{ f0: r.f0,  // f0 sourced from the recursive result's field ->
                   f1: fresh } //   NOT owned under suppression (r.f0 is a borrow)
  }
  Record{ f0: x, f1: fresh }  // base return site: f0 IS from p0 here
}
```

`f0` is `from(p0)` only on the base return; on the recursive return it comes from
`r.f0`, which suppression leaves unrecovered. The per-`(via,field)` meet across
return sites therefore **drops `[.f0]`** (absent on the recursive site). `f1` is
locally fresh on **both** sites, so it survives as `OwnedFresh`. This proves
suppression blanks only the recursive read, not all classification.

```tw
.test(
  "recursive helper: self-thread ret_paths are under-approximated (no speculative path)",
  fn() {
    funcs := recursive_transport_fixture()   // builds g above; g is func_id 1
    t := compute_of(funcs)
    s := summ_of(t, 1)
    try assert.equal(has_from_param_path(s.ret_paths, 0), false)  // [.f0] dropped by the meet
    try assert.equal(has_direct_field_fresh(s.ret_paths, 1), true) // [.f1]=fresh survives
    try assert.equal(same_summary_pub(s, summ_of(compute_of(funcs), 1)), true) // deterministic
    .Ok({})
  },
)
```

Add `recursive_transport_fixture()` (the two-return-site `g` above, where the
recursive site sources `f0` from the recursive result's field so it is only
recoverable if in-SCC `ret_paths` were visible), `has_from_param_path(rps, k)` (true
iff some ret_path is `OwnedFromParam(k)`), `has_direct_field_fresh(rps, f)` (true iff
some `Direct`/`field=Some(f)` ret_path is `OwnedFresh`), and a `same_summary_pub`
wrapper calling `summary.same_summary` (or make `same_summary` `pub`).

- [ ] **Step 2: Run — expect RED (speculative path or nondeterminism)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: without suppression, the recursive read exposes a speculative `[.f0]=from(p0)` (assertion fails) and/or the two computes disagree.

- [ ] **Step 3: Suppress in-SCC `ret_paths` reads (order-independent)**

The order-dependent hazard is: if `run_scc` restores member summaries into the shared `table` one at a time (it updates in sorted member order — `summary.tw:294-310`), a later member's classification could observe an earlier member's just-restored `ret_paths`, while the earlier member saw them empty. That is speculative, order-dependent exposure.

Fix with an **explicit suppression set**, not a strip-and-final-pass. Thread the SCC member set (`scc_set`, already built at `summary.tw:296`) down to the call transfer, and blank `ret_paths` for any callee that is in the set. This makes in-SCC reads see empty `ret_paths` **regardless of table order**, during every round and any settle pass:

1. Add a `suppress: Dict<Int, Bool>` parameter to `summarize_function` (default `Dict.new()` at its other call sites — `cfg_summary_suite.tw`'s `summ1` passes `Dict.new()`), threaded into `run_fixpoint` → `forward_*` → `transfer_op` → `transfer_call` → `transfer_summarized_call`.
   **Also add a `callee_id: Int` parameter to `transfer_summarized_call`** — it does
   *not* currently receive the callee id (its signature is `(st, result, s, args)`),
   and the suppression check below needs it. The id is in scope at the one call site
   inside `transfer_call` (`:844`), where the summary is resolved as
   `case table.summary_get(fid.id)` (`:861`): pass that `fid.id` down. (Task 10 adds
   `last` to the same signature, so both new params land together.)
2. In `transfer_summarized_call`, before consuming `s.ret_paths`, check the callee id:

```tw
// callee_id is the fid.id passed from transfer_call's summary_get(fid.id) resolution.
eff_ret_paths := if in_set(suppress, callee_id) { [] } else { s.ret_paths }
for rp in eff_ret_paths { /* gate + recover (Task 10) */ }
```

3. In `run_scc` (`:286`), pass `scc_set` as `suppress` to every `summarize_function` call in the worklist loop. Because suppression is keyed on membership (not table contents), the classification each member produces is a pure function of its own body + earlier-SCC (final) summaries + the converged in-SCC escape/param/ret facts — **independent of member processing order**.
4. The combined fixpoint converges when the monotone escape/param/ret facts do (`ret_paths` are a deterministic readout of them once suppression fixes the recursive reads). On **cap-hit** (`rounds >= cap`, non-convergence), strip `ret_paths` for every member as a final safety net:

```tw
if rounds >= cap {
  for id in members {
    s := table_get(table, id)
    table = table_put(table, id, Summary.{ params: s.params, ret: s.ret, ret_paths: [] })
  }
}
```

No separate "final settle pass" is needed: the worklist already computes each member's `ret_paths` under suppression, and `same_summary` (comparing `ret_paths` too) drives termination. Recursive-transport precision (a helper recovering its **own** return paths through the recursive edge) is intentionally under-approximated here and deferred to Phase 6.

- [ ] **Step 4: Carry `path_prov` through the fixpoint exits + join it**

First, make per-block `path_prov` exits available (parallel to `exit_field_own`).
Add an `exit_path_prov: Dict<Int, Dict<Int, Dict<Int, Vector<Int>>>>` map to
`FixResult` (`:1867`) and populate it in `run_fixpoint` from each block's exit
`ForwardState.path_prov`, exactly as `exit_field_own` is populated. This is what
`summarize_function`'s return classification and `seed_payload_binding` (Task 13)
read as `fx.exit_path_prov`.

Then join it: `join_entry_field_own` (`:1776`) meets `field_own` per path; add a sibling `join_entry_path_prov` that meets `path_prov` for the same locals — a path survives only if present on every processed pred; on origin **conflict** across preds keep the **union** (so a divergent origin makes the path multi-origin ⇒ later dropped by `classify_path_own`, sound). Wire it into the `ForwardState.{ … }` entry builder next to `entry_field`.

> **Do NOT call `seed_payload_binding` here.** That helper is defined in Task 13,
> which adds the call at these same entry-builder sites once it exists. Task 9 stops
> at (a) making `exit_path_prov` available on `FixResult` and (b) joining `path_prov`
> in lockstep with `field_own`. Adding the call now would reference an undefined
> function and break the green-per-task discipline. Task 13 Step 3 owns the wiring.

- [ ] **Step 5: Run — expect PASS + determinism**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw` (twice; outputs stable).
Expected: recursive fixture converges deterministically; the self-thread `[.f0]` is under-approximated to absent (suppression) while the locally-fresh `[.f1]` is still classified; non-recursive `ret_paths` (Tasks 7–8) unchanged.

- [ ] **Step 6: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/summary.tw boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/summary.tw boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "summary: suppress in-SCC ret_paths reads (order-independent), strip on cap

ret_paths are non-monotone, so an in-SCC suppression set blanks ret_paths for
in-SCC callees regardless of member processing order — within-SCC recursive
reads always see them empty, and a cap-hit strips them entirely (conservative).
path_prov joins in lockstep with field_own so cross-arm origin divergence
degrades to a dropped path.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage F — Caller consumption

### Task 10: `transfer_summarized_call` gains `last` + gate with publish-on-fail

**Files:**
- Modify: `boot/compiler/ownership.tw` (`transfer_call` `:844`, `transfer_summarized_call` `:976`)
- Test: `boot/tests/suites/cfg_return_paths_suite.tw` (new)

- [ ] **Step 1: Create the new suite skeleton + Case W caller test**

Create `boot/tests/suites/cfg_return_paths_suite.tw` mirroring `cfg_summary_suite.tw`'s imports/helpers (copy `lid`, `b_reg`, `sem`, `fdef`, `module_of`, `compute_of`, `analyzed_caller`, `caller_own`, `own_unique`, `own_shared`). **Also add the block-aware `own_in_block(f, block_id, local) Int`** (same body as `caller_own` but indexed at the given block, not hardcoded block 0) — the multi-block fixtures in Tasks 11 and 13 observe locals bound outside block 0, and `caller_own`'s `.None → Shared` default would otherwise make a borrow-negative pass trivially (see the "Fixture construction discipline" section, point 4). Register the suite in `boot/tests/main.tw` (add to the suite list next to the other cfg suites). First test — a caller recovering `out.ctx` (single-block, so `caller_own` is fine here):

```tw
.test(
  "Case W: out := helper(ctx); ctx = out.ctx keeps ctx Unique",
  fn() {
    // helper(x) returns Record{f0:x}; ret_paths [.f0]=from(p0).
    // caller(c): let out = helper(c); let g = record_get out.f0; assign c = g; c
    funcs := case_w_fixture()   // builds helper (id 1) + caller (id 2)
    f := analyzed_caller(funcs, "caller")
    // after the block, c (local 0) is Unique.
    try assert.equal(caller_own(f, 0), own_unique())
    .Ok({})
  },
)
```

Add `case_w_fixture()` building the two functions with the exact ANF (helper returns `ARecord{f0: p0}`; caller does `ACall(helper, [c]) -> out`, `ARecordGet(out, f0) -> g`, `AAssign(c, g)`, return `c`).

- [ ] **Step 2: Run — expect RED**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: `c` is not Unique — the call result carries no field_own and the gate isn't applied yet.

- [ ] **Step 3: Thread `last`, snapshot pre-call facts, apply the gate**

Change `transfer_summarized_call` (`:976`) to accept `last: Vector<Int>`, the
`suppress` set, and `callee_id: Int` (all three land on this signature; `suppress`
and `callee_id` are introduced in Task 9). Its caller `transfer_call` (`:844`)
already has `last` and resolves the callee as `fid.id` in
`case table.summary_get(fid.id)` (`:861`) — pass `last`, `suppress`, and that
`fid.id` through at the call site `st.transfer_summarized_call(result, s, args)`.

**Critical ordering (review note):** the existing `params`/`ret` handling *mutates* `st` — it publishes `Retained` args and `MayAliasParams` origins in place (`ownership.tw:976-999`). The gate must read the argument's ownership **as it was before the call**, so **snapshot the pre-call facts first**, then apply `params`/`ret`, then recover using the snapshot:

```tw
// 0. SNAPSHOT pre-call arg ownership (before any summary effect mutates st).
pre_own := st.own          // Dict copy; Twinkle Dicts are persistent, so this is a cheap alias
pre_prov := st.prov
arg_unique: Vector<Bool> = collect a in args {
  case atom_local_id(a) {
    .Some(id) => own_is_unique(pre_own, id) and is_last_use(last, id),
    .None => false,
  }
}
// 1. existing params (publish Retained) + ret (OwnedFresh/Shared/MayAliasParams) handling …

// 2. Return-path recovery, gated on the SNAPSHOT (not the mutated st).
eff_ret_paths := if in_set(suppress, callee_id) { [] } else { s.ret_paths }
result_fields := ff.empty()
result_pp: Dict<Int, Vector<Int>> = Dict.new()
result_ok := case s.ret { .OwnedFresh => true, _ => false }  // only a fresh result shell carries recovered paths
for rp in eff_ret_paths {
  own_here := case rp.own {
    .OwnedFresh => true,
    .OwnedFromParam(k) => if k < args.len() and arg_unique[k] {
      true
    } else {
      // publish-on-fail: the result may alias args[k]'s region.
      if k < args.len() { st = .publish_atom(args[k]) }
      false
    },
  }
  if own_here and result_ok {
    origins := case rp.own {
      .OwnedFromParam(k) => prov_of(pre_prov, args[k]),   // provenance follows args[k]
      .OwnedFresh => [],
    }
    // build result_fields / result_pp for this path (helper below)
    r := record_ret_path(result_fields, result_pp, rp, origins)
    result_fields = r.fields
    result_pp = r.pp
  }
}
st = if result_ok and !result_fields.is_empty() {
  st.set_field_own(result, result_fields).set_path_prov(result, result_pp)
} else { st }
```

Because Twinkle is immutable, `record_ret_path(fields, pp, rp, origins)` returns `.{ fields, pp }` and writes **exactly one** key — the one this ret_path names — into both `fields` (tag Unique = 0) and `pp` (`origins`). **Never synthesize or overwrite a sibling key:**

- `via == Direct`, `field == Some(f)` → `[.f]`
- `via == Variant(tag,i)`, `field == None` → `[Payload(tag,i)]` (the payload **shell**)
- `via == Variant(tag,i)`, `field == Some(f)` → `[Payload(tag,i), .f]` (the payload **field**)

A `Variant` payload shell is recovered **only** from a `field: None` ret_path (which the classifier emits as `OwnedFresh` when the payload record is fresh); a `field: Some(f)` ret_path must **not** touch `[Payload(tag,i)]`, or a fresh payload shell would be mislabeled `from(pk)` (review Blocker 2). Since a payload's field key `[Payload(tag,i), .f]` presupposes its shell key `[Payload(tag,i)]` (downward-closed), the shell's own `field: None` ret_path supplies the shell entry independently. Add `is_last_use`-over-atom via `atom_local_id`. Note publish-on-fail and `MayAliasParams` publication **compose** — both may publish `args[k]`, which is idempotent.

- [ ] **Step 4: Run — expect PASS (Case W caller)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: after `ctx = out.ctx`, `c` is Unique (the existing `ARecordGet` move recovers `[.f0]` from the result's field_own once the whole-record last-use / transport move applies — for this fixture `out` is dead after the projection, so Phase 4's whole-record last-use already fires).

- [ ] **Step 5: Add the gate-failure publish test**

```tw
.test(
  "gate fail: ctx read after the call is published (not left Unique)",
  fn() {
    // caller2(c): let out = helper(c); let g = record_get out.f0; let _ = record_get c.f9; ...
    // c is read again after the call -> not last-use at the call -> publish c.
    funcs := case_w_gatefail_fixture()
    f := analyzed_caller(funcs, "caller")
    try assert.equal(caller_own(f, 0), own_shared())  // c published
    .Ok({})
  },
)
```

Run — expect PASS.

- [ ] **Step 6: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw boot/tests/main.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw boot/tests/main.tw
git commit -m "ownership: recover return paths at the caller with publish-on-fail

transfer_summarized_call now takes last and, per ret_path, gates OwnedFromParam(k)
on args[k] Unique+last-use: on success the result carries the recovered field/
payload facts (provenance = args[k]'s), on failure it publishes args[k] rather
than leaving an unpublished alias. OwnedFresh paths are unconditional.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage G — Transport-wrapper projection move

### Task 11: Bounded transport recognizer + `ARecordGet` move condition

**Files:**
- Modify: `boot/compiler/ownership.tw` (`BlockPrep` `:511`, `block_prep` `:513`, `ARecordGet` transfer `:1107`, add `recognize_transport_moves`)
- Test: `boot/tests/suites/cfg_return_paths_suite.tw`

- [ ] **Step 1: Write the failing sibling-read test**

```tw
.test(
  "transport move: out.ctx moves even though out.ty is read after",
  fn() {
    // caller(c): out=helper(c); g=record_get out.f0; ty=record_get out.f1; assign c=g; use ty; c
    // out is live for out.f1, but out.f0 is dead-through-out -> move licensed.
    funcs := case_w_sibling_fixture()
    f := analyzed_caller(funcs, "caller")
    try assert.equal(caller_own(f, 0), own_unique())
    .Ok({})
  },
)
```

- [ ] **Step 2: Run — expect RED**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: `c` not Unique — Phase 4 whole-record last-use fails because `out` is live for `out.f1`, and no transport recognizer exists yet.

- [ ] **Step 3: Add `recognize_transport_moves`**

Add a block-local recognizer mirroring `recognize_quartet_moves` (which returns `Dict<Int,Bool>` of licensed `ARecordGet` result-locals). A projection `R = record_get out.f` is transport-move-licensed when, strictly after it in the block: `out.f` is not read again; `out` is not published/returned/stored/passed-to-call/aliased/used-by-terminator-or-successor-arg **except** for other `record_get out.g` sibling reads; and `out` is **dead after the block**. Model it on `quartet_ok` (`:242`) — scan forward, allow sibling `ARecordGet(out, g)` (g != f) and their pure downstream reads, reject any other mention of `out` or a second `record_get out.f`.

**Dead-after must check live-out, not only the terminator (Blocker).**
`exit_mentions_local` (`:210`) only checks the terminator's direct uses and successor edge args — it does **not** catch `out` being read in a *later block* (live-through). If `out` is live-out of this block, a successor can observe R's in-place mutation, so the move is unsound. Require **both**: not mentioned at exit **and** not in `blk.exit.live` (the liveness live-out filled by the liveness stage):

```tw
fn transport_ok(blk: CfgBlock, scan: BlockScan, i: Int, projected: Int, out_id: Int, fid: Int) Bool {
  insts := blk.instructions
  for j in range_from(i + 1, insts.len()) {
    op := insts[j].op
    // a second read of out.f fails
    if is_record_get_of(op, out_id, fid) { return false }
    // sibling read out.g (g != fid) is allowed and does not mention-fail
    is_sibling := case op {
      .ARecordGet(base, f2, _) => atom_is_local(base, out_id) and f2.id != fid,
      _ => false,
    }
    if is_sibling { } else if op_mentions_local(op, out_id) {
      return false   // any other mention of out fails
    }
  }
  // dead after the block: not used by the terminator/edges AND not live-out
  // (so no later block can read `out` and observe R's mutation).
  !exit_mentions_local(blk, out_id) and !live_contains_int(blk.exit.live, out_id)
}
fn recognize_transport_moves(blk: CfgBlock, scan: BlockScan) Dict<Int, Bool> {
  out: Dict<Int, Bool> = Dict.new()
  for inst, i in blk.instructions {
    case inst.op {
      .ARecordGet(base, f, _) => case atom_local_id(base) {
        .Some(bid) => if transport_ok(blk, scan, i, inst.anf_local.id, bid, f.id) {
          out[inst.anf_local.id] = true
        },
        .None => {},
      },
      _ => {},
    }
  }
  out
}
```

Extend `BlockPrep` (`:511`) with `transport: Dict<Int, Bool>` and set it in `block_prep` (`:513`): `transport: recognize_transport_moves(blk, scan)`. Thread it into `forward_block_body` → `transfer_op` next to `quartet`.

- [ ] **Step 4: Consult it in `ARecordGet`**

In the `ARecordGet` transfer (`:1110`), change the move condition:

```tw
.Some(bid) => if is_last_use(last, bid) or quartet_has(quartet, result) or transport_has(transport, result) {
```

Add `transport_has` (twin of `quartet_has`). The move mechanics (transfer `[.f]*`, `remove_prefix` on base) are unchanged.

- [ ] **Step 5: Run — expect PASS**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: sibling-read test passes; the earlier Case W tests still pass.

- [ ] **Step 6: Add borrow negatives (published `out`, and `out` read in a later block)**

```tw
.test(
  "transport borrow: publishing out forces borrow, both demoted",
  fn() {
    // caller(c): out=helper(c); g=record_get out.f0; global_set G0 = out; ...
    funcs := case_w_published_out_fixture()
    f := analyzed_caller(funcs, "caller")
    try assert.equal(caller_own(f, 0), own_shared())   // no clean recovery
    .Ok({})
  },
)
.test(
  "transport borrow: out live into a later block forces borrow (live-out check)",
  fn() {
    // caller(c): out=helper(c); g=record_get out.f0; assign c=g;
    //   if cond { ... read out.f1 ... }   // out is LIVE-OUT of the projection block
    // exit_mentions_local alone would miss this; the blk.exit.live check catches it.
    funcs := case_w_out_liveout_fixture()
    f := analyzed_caller(funcs, "caller")
    try assert.equal(caller_own(f, 0), own_shared())   // move must NOT fire
    .Ok({})
  },
)
```

Run — expect PASS (both borrow; the second only passes because of the `blk.exit.live` check).

- [ ] **Step 7: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw
git commit -m "ownership: bounded transport-wrapper projection move

A block-local recognizer licenses out.f to move even while out stays live for
sibling reads (out.g), the finer path-liveness Phase 4 deferred; publishing/
storing/re-reading out falls back to borrow. Wired through BlockPrep and the
ARecordGet move condition, mirroring the quartet recognizer.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage H — Match-arm payload seeding + Case R

### Task 12: CFG per-arm payload metadata

**Files:**
- Modify: `boot/compiler/cfg.tw` (`CfgBlock` `:55`, `build_match` `:531`, add `PayloadSrc`)
- Test: `boot/tests/suites/cfg_return_paths_suite.tw` (structural)

- [ ] **Step 1: Add `PayloadSrc` and the block field**

In `cfg.tw`:

```tw
// Set only for a match-arm block whose incoming pattern is a top-level
// Variant(tag, [Var(v)]) single-payload binding; drives Phase 5 payload seeding.
pub type PayloadSrc = .{ scrutinee: Int, variant_tag: Int, payload_index: Int, binding: Int }
```

Add `payload_src: PayloadSrc?` to `CfgBlock` (`:55`), default `.None` in every `CfgBlock.{ … }` builder (search the file; the primary one is near `:320` where `bound: []`).

- [ ] **Step 2: Populate it in `build_match`**

`build_match` (`:531`) sets `bound`; also detect a top-level single-`Var` variant pattern and set `payload_src`. Add a recognizer:

```tw
// Only Variant(_, vid, [Var(v)]) with a single Var payload; else .None (under-claim).
fn arm_payload_src(scrutinee: Atom, pattern: CorePattern) PayloadSrc? {
  case atom_local_id(scrutinee) {
    .Some(sid) => case pattern {
      .Variant(_, vid, subs) => if subs.len() == 1 {
        case subs[0] {
          .Var(v) => .Some(PayloadSrc.{ scrutinee: sid, variant_tag: vid.id, payload_index: 0, binding: v.id }),
          _ => .None,
        }
      } else { .None },
      _ => .None,
    },
    .None => .None,
  }
}
```

In the arm loop (`:543`), after `set_block_bound`, set the payload src:

```tw
ctx = .set_block_payload_src(nb.id, arm_payload_src(scrutinee, arm.pattern))
```

Add `set_block_payload_src` mirroring `set_block_bound` (`:525`).

> **Known asymmetry (sound under-claim, not a bug):** the *summary* side (Task 8's
> `AVariant` classification) records **every** payload index `Payload(tag, i)`, but
> this *seeding* side recognizes only a single-`Var` payload and hardcodes
> `payload_index: 0`. A multi-payload arm (`.Rect(a, b)`) yields `.None` and its
> bound locals are not seeded. That is an under-claim (those bindings stay Unknown,
> never wrongly Unique), so it is sound; widening to multi-payload seeding is a
> Phase 6 concern. Do not "fix" it by guessing indices here.

- [ ] **Step 3: Write a structural test**

Assert that for a `case scrutinee { .Ok(v) => v, ... }` module, the arm block has `payload_src` set with the right tag/binding. Build via `cfg.build_view` and inspect `f.blocks`. Add the test to the new suite.

- [ ] **Step 4: Run — expect PASS**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: metadata present on the `.Ok`/`.Err` arm blocks; `.None` for non-`Var` patterns.

- [ ] **Step 5: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/cfg.tw boot/tests/suites/cfg_return_paths_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/cfg.tw boot/tests/suites/cfg_return_paths_suite.tw
git commit -m "cfg: record per-arm payload-projection metadata for match seeding

A match-arm block over a top-level Variant(tag, [Var(v)]) pattern now records
(scrutinee, variant_tag, payload_index, binding) so Phase 5 can seed the bound
payload local from the scrutinee's tagged payload facts; other patterns
under-claim (None).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

### Task 13: Seed the payload-bound local; Case R end-to-end

**Files:**
- Modify: `boot/compiler/ownership.tw` — add a shared `seed_payload_binding(st, blk, last)` helper and call it at **every** per-block entry-`ForwardState` construction, so summaries, the materialization/verdict pass, and the return replay all agree. Search `ForwardState.{` / the entry builders in `run_fixpoint`, in the materialization/verdict pass, and in `summarize_function`'s return replay (near `:2328`); each must invoke the helper after building the entry state.
- Test: `boot/tests/suites/cfg_return_paths_suite.tw`

- [ ] **Step 1: Write the failing Case R test**

> **Assert with `own_in_block` at the arm block, not `caller_own`.** `v` (and the
> `ok_arm_*`/`err_arm_*` locals) are bound in a match-**arm** block, absent from
> block 0. Using `caller_own` here reads block 0's exit and returns the `.None →
> Shared` default — the move assertion below would fail spuriously and the
> borrow-negatives (Steps 5–6) would pass without proving anything. Resolve the arm
> block id from the built view (the `.Ok` arm's block) and assert
> `own_in_block(f, ok_arm_block_id, <local>)`. The `ok_arm_state_local()` /
> `ok_arm_payload_local()` / `err_arm_payload_local()` helpers return the local ids;
> pair each with its arm block.

```tw
.test(
  "Case R: case load(state){.Ok(v)=>v, .Err(e)=>return ...} keeps loaded.state Unique",
  fn() {
    // load(s) -> Result-shaped variant with Ok[0].state=from(p0), Err[0].state=from(p0).
    // caller(acc): out=load(acc); case out { .Ok(v)=> v ; .Err(e)=> return e } ; ... use v.state
    funcs := case_r_fixture()
    f := analyzed_caller(funcs, "caller")
    // the Ok arm's bound payload v recovers .state Unique; assert via the join local.
    try assert.equal(caller_own(f, ok_arm_state_local()), own_unique())
    .Ok({})
  },
)
```

`case_r_fixture()` builds `load` returning an `AVariant` wrapping `ARecord{state: p0}` on the Ok path (and similarly on Err), and a caller that matches. Keep it minimal but exercising the tagged payload + record field.

- [ ] **Step 2: Run — expect RED**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: the payload-bound `v` is seeded Unknown (current behavior), so `.state` is not Unique.

- [ ] **Step 3: Add the shared `seed_payload_binding` helper (move **or** borrow-demote)**

Mirror `ARecordGet`'s move-vs-borrow exactly (`ownership.tw:1107-1134`): the payload projection is a **move** only when the scrutinee is dead after the match; **otherwise it is a borrow that demotes both sides** — the binding becomes `Shared` (choke point clears its field facts) and the scrutinee's payload subtree is cleared. Seeding an owned binding while the scrutinee stays live would create two unique aliases of the same region.

**Scrutinee facts come from the predecessor exit, not the arm entry (Blocker).** An
arm block has exactly one predecessor — the match block — and empty edge args. The
scrutinee is typically *dead after the match* (moved into the payload), so it is
**not** live-in at the arm and its `field_own`/`path_prov`/`own`/`prov` are dropped
from the live-filtered arm entry state. The seed must therefore read the scrutinee's
facts from the **predecessor (match) block's exit facts**, which have them
regardless of liveness. So the helper takes those exit maps.

**Binding shell prov (Blocker).** On a move the binding also inherits the projected
payload's **shell provenance** (the `[Payload(tag,i)]` `path_prov` entry), analogous
to the Task 3 `ARecordGet` projected-shell rule; otherwise `return v` / publishing
`v` after the match loses the shell-origin.

```tw
// Applied at every per-block entry-state construction, after the base entry facts.
// pred_* are the match block's EXIT facts (arm's single predecessor).
fn seed_payload_binding(
  st: ForwardState, blk: CfgBlock, last: Vector<Int>,
  pred_own: Dict<Int, Int>, pred_field: Dict<Int, ff.FieldMap>,
  pred_pp: Dict<Int, Dict<Int, Vector<Int>>>, pred_prov: Dict<Int, Vector<Int>>,
) ForwardState {
  case blk.payload_src {
    .None => st,
    .Some(ps) => {
      seg := ff.PathSeg.Payload(ps.variant_tag, ps.payload_index)
      src_fields := field_map_of(pred_field, ps.scrutinee)          // pred exit facts
      src_pp := pp_of(pred_pp, ps.scrutinee)
      proj := src_fields.project(seg)                                // {shell, fields}
      proj_pp := project_path_prov(src_pp, seg)                      // {shell: Vector<Int>?, inner}
      case proj.shell {
        .None => st,   // no owned payload fact -> leave binding as seeded (Unknown)
        // MOVE only when the scrutinee is NOT live-in at the arm: liveness live-in
        // subsumes both a later read INSIDE this arm block and any live-out to a
        // successor. If the scrutinee is live-in it is read somewhere -> borrow.
        .Some(t) => if !live_contains_int(blk.entry.live, ps.scrutinee) {
          // MOVE: binding takes shell own + shell prov + inner field/prov facts.
          st = .set_own_st(ps.binding, own_of_tag(t))               // Unique first (choke order)
          shell_origins := case proj_pp.shell {
            .Some(o) => o,
            .None => case pred_prov.get(ps.scrutinee) { .Some(o) => o, .None => [] },
          }
          st = .set_prov_st(ps.binding, shell_origins)              // BINDING SHELL PROV
          st = .set_field_own(ps.binding, proj.fields)
          st.set_path_prov(ps.binding, proj_pp.inner)
          // scrutinee is dead-after -> nothing to demote (not read anywhere).
        } else {
          // BORROW: scrutinee is live-in, so its facts are in `st` (via the join).
          // Binding Shared; strip the scrutinee's payload subtree in the arm entry.
          st = .set_own_st(ps.binding, .Shared)
          live_fields := st.field_own_get(ps.scrutinee)
          st = .set_field_own(ps.scrutinee, live_fields.remove_prefix(seg))
          st.set_path_prov(ps.scrutinee, remove_prefix_pp(st.path_prov_get(ps.scrutinee), seg))
        },
      }
    },
  }
}
```

Add: `project_path_prov(pp, seg)` returning `.{ shell: Vector<Int>?, inner: Dict<Int, Vector<Int>> }` (mirror `ff.project`: `[seg]` → `shell`, strict descendants `[seg, X]` → `inner[X]`); `remove_prefix_pp(pp, seg)` (mirror `ff.remove_prefix`); and `field_map_of`/`pp_of` (map lookups with empty defaults). The move gate uses **liveness live-in** (`blk.entry.live`, filled by the liveness stage before the forward pass), which is the correct choice for an entry-point seed: it catches a later read of the scrutinee *inside* the arm as well as any live-out — unlike Task 11's transport recognizer, which scans mid-block and so uses live-**out**. At each entry-state construction site, look up the arm's **single predecessor** (a match arm has exactly one; assert `blk.preds.len() == 1`) and read its exit maps: the predecessor block id is `blk.preds[0].target.id` (in a `preds` edge, `CfgEdge.target` holds the *predecessor* id — `cfg.tw:368-372`), indexed into `fx.exits`/`fx.exit_field_own`/`fx.exit_path_prov`/`fx.exit_prov`. The `.Err`→`return` arm is a leaf and contributes nothing to any join.

- [ ] **Step 4: Run — expect PASS (Case R move)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: the Ok-arm payload `v` recovers `.state` Unique (scrutinee `out` dead after the match ⇒ move); the Err/return arm does not corrupt the join.

- [ ] **Step 5: Add the live-scrutinee borrow-demote negative tests**

Two shapes must borrow — the scrutinee read in a **later block**, and the scrutinee
read **inside the arm** after the payload binding. The live-in gate must catch both
(the in-arm read is the one a live-out-only check would miss).

```tw
.test(
  "live scrutinee (later block): payload binding borrow-demotes",
  fn() {
    // caller(acc): out=load(acc); case out {.Ok(v)=> use v ...}; then read out.f in a later block.
    // out is live-out of the arm's predecessor -> live-in-ish -> v must NOT be unique.
    funcs := case_r_live_scrutinee_fixture()
    f := analyzed_caller(funcs, "caller")
    try assert.equal(caller_own(f, ok_arm_payload_local()), own_shared())
    .Ok({})
  },
)
.test(
  "live scrutinee (in-arm read): reading out inside the Ok arm after binding borrows",
  fn() {
    // Ok arm body: v bound; then `record_get out.f1` INSIDE the same arm block.
    // out is live-in at the arm -> move must not fire; v Shared.
    funcs := case_r_inarm_read_fixture()
    f := analyzed_caller(funcs, "caller")
    try assert.equal(caller_own(f, ok_arm_payload_local()), own_shared())
    .Ok({})
  },
)
```

Run — expect PASS (both borrow; the second only passes because the gate is `blk.entry.live` live-in, not just live-out).

- [ ] **Step 6: Add the tag-isolation negative**

```tw
.test(
  "tag isolation: Err arm cannot recover an Ok-only payload fact",
  fn() {
    // load returns Ok with owned payload, Err with a SHARED/foreign payload.
    funcs := case_r_tag_isolation_fixture()
    f := analyzed_caller(funcs, "caller")
    try assert.equal(caller_own(f, err_arm_payload_local()), own_shared())
    .Ok({})
  },
)
```

Run — expect PASS (the `[Payload(Err,0)]` projection finds no owned fact).

- [ ] **Step 7: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw
git commit -m "ownership: seed match-arm payload locals from tagged scrutinee facts

A top-level Variant(tag,[Var(v)]) arm seeds v by projecting the scrutinee's
[Payload(tag,i)]* facts (field_own + path_prov), moving them out when the
scrutinee is dead after the match; the tag keeps arms disjoint so an Err arm
cannot recover an Ok payload. Completes Case R: handled Result state stays
Unique while return arms are leaf exits.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage I — Rendering + full verification

### Task 14: Render the transport verdict; determinism

**Files:**
- Modify: `boot/compiler/cfg.tw` or `boot/compiler/summary.tw` (`render_view`/`render_cfg` path), `boot/compiler/ownership.tw` (verdict string)
- Test: `boot/tests/suites/cfg_return_paths_suite.tw`

- [ ] **Step 1: Write the determinism test**

```tw
.test(
  "cfg render with ret_paths + transport verdict is byte-identical across builds",
  fn() {
    funcs := case_w_sibling_fixture()
    b := b_reg()
    a := render_cfg_for_entry_of(funcs)   // helper: build_view -> summary.compute -> render_cfg
    b2 := render_cfg_for_entry_of(funcs)
    try assert.equal(a == b2, true)
    .Ok({})
  },
)
```

- [ ] **Step 2: Run — expect RED if the verdict/summary line isn't rendered yet**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: RED only if the render helper isn't wired; otherwise PASS (determinism should already hold — this test also guards it).

- [ ] **Step 3: Add the transport verdict to `render_view`**

At each `ARecordGet` transport site, print `transport=move([.f] from pK)` / `transport=borrow(<reason>)` alongside Phase 4's shell/field verdict, driven by `BlockPrep.transport` and the classified `path_prov`. The `ret_paths` summary line already renders via `render_summary` (Task 6). Keep every rejection reasoned (no silent bail).

- [ ] **Step 4: Run — expect PASS + eyeball a real dump**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Then eyeball: `target/twk ir boot/compiler/checker.tw --cfg 2>&1 | grep -E 'ret_paths|transport' | head`
Expected: transport helpers in the real checker show `ret_paths` and `transport=` verdicts; determinism test green.

- [ ] **Step 5: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/compiler/summary.tw boot/compiler/cfg.tw boot/tests/suites/cfg_return_paths_suite.tw
target/twk lint boot/main.tw
git add -A
git commit -m "cfg/ownership: render ret_paths summary and transport-move verdict

twk ir --cfg now prints the per-function ret_paths line and, per transport
projection site, move([.f] from pK) or borrow(reason), keeping the print-facts
discipline; output is deterministic (byte-identical across builds).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

### Task 15: Full verification + census guard + self-host

**Files:** none (verification only), plus doc bookkeeping.

> **What "done" means for Phase 5.** This phase is analysis-only: no codegen decision
> consumes `ret_paths` yet (verified — `summarize_function`/`SummaryTable` are read
> only within `{ownership,summary,cfg,field_facts}.tw` + tests; `census.tw` contains
> no ownership analysis). So there is **no observable runtime/codegen change by
> design** — the recovered ownership only pays off once Phase 6 wires it to a
> specialization decision. Phase 5's validation is therefore exactly: (a) unit +
> cross-function suites green, (b) `ret_paths`/transport verdicts visible in a real
> `twk ir --cfg` dump (Task 14), (c) self-host fixed point, (d) census still 0
> in-place, (e) no compile-time regression (Step 3b). If you expected a functional
> win here, that expectation belongs to Phase 6, not this plan.

- [ ] **Step 1: Census still zero in-place**

Run: `target/twk ir boot/main.tw --census 2>&1 | tail -20`
Expected: **0 in-place** (Phase 5 changes no codegen). If nonzero, a transfer edit leaked into a decision path — stop and investigate.

- [ ] **Step 2: Full boot suite (sequential)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: all suites green, including the re-baselined `cfg_summary_suite` and the new `cfg_return_paths_suite`.

- [ ] **Step 3: Self-host fixed point (sequential, not backgrounded)**

Run: `make stage2`
Expected: reaches the self-host fixed point (boot compiles boot to a stable `target/boot.wasm`). Phase 5 is boot-only and adds no stage0-parity construct, so stage0 needs no change; if `make stage2` fails in stage0, a Phase 5 construct leaked into boot *source* usage — revert that usage (analysis code must not require new stage0 support).

- [ ] **Step 3b: Compile-time regression check**

Phase 5 adds a per-block `path_prov` (`Dict<Int, Dict<Int, Vector<Int>>>`) joined at
every block entry across the whole self-host, plus the payload codec. Confirm this
did not silently regress compile time:

Run: `TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/stage2.wasm 2>&1 | grep '^\[time'`
Compare the total (and the `[time:check]` sub-counter, which covers the ownership
pass) against the pre-Phase-5 baseline captured from `main` (re-run the same command
on a clean checkout if no baseline was recorded). A single-digit-percent increase is
expected and acceptable; a large jump points at the codec (should be O(1) after the
bit-packing fix) or an unbounded `path_prov` join — investigate before landing, do
not just accept it.

- [ ] **Step 4: Rust reference sanity (targeted)**

Run: `cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture` (reference distribution only; not a gate). Confirm it still runs.

- [ ] **Step 5: Update the analysis README checkboxes**

In `docs/plans/sound-uniqueness/analysis/README.md`, tick the four Phase 5 bullets (`[x]`) and change the Phase 5 status line / the top `README.md` "Current focus" to reflect Phase 5 done, Phase 6 next. Follow the memory guidance: on completion, remove the plan's row from any plans index (there is none here) — just update the phase status.

- [ ] **Step 6: Commit the bookkeeping**

```bash
target/twk fmt docs/plans/sound-uniqueness/analysis/README.md 2>/dev/null || true
git add docs/plans/sound-uniqueness/
git commit -m "docs/sound-uniqueness: mark Phase 5 return-path summaries done

Cases W and R classify as ownership-preserving handoffs; generated code
unchanged (census still 0 in-place). Phase 6 (ownership-specialization decision
facts) is next.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-review checklist (run before executing)

- **Spec coverage:** Task 1 (tagged payload segment + codec) ↔ Decision 4; Tasks 2–4 (path_prov + shell/field split + AVariant) ↔ Decision 2; Task 5 (Return leaf) ↔ Decision 3; Tasks 6–8 (ret_paths schema + Direct/Variant classification + shell ret) ↔ Decisions 1, 7, 8; Task 9 (fixpoint hide/strip + path_prov join) ↔ Decisions 10 + review pt 4; Task 10 (caller gate + publish-on-fail) ↔ Decision 5 + review pt 1; Task 11 (transport recognizer) ↔ Decision 9 / Fork 2A; Tasks 12–13 (cfg metadata + payload seeding) ↔ Fork 3-i + Blocker 1; Task 14 (render) ↔ design "Rendering"; Task 15 ↔ acceptance 10–12. Acceptance 1–9 map to tests across Tasks 5, 7, 8, 10, 11, 13, 1.
- **No parameter `in_place_paths` / specialization** appears in any task (correctly Phase 6).
- **Type consistency:** `PathSeg.Payload(Int,Int)`, `ReturnPathOwn.{via,field,own}`, `RetVia.Variant(Int,Int)`, `PayloadSrc.{scrutinee,variant_tag,payload_index,binding}` are used identically across tasks. `path_prov` is `Dict<Int, Dict<Int, Vector<Int>>>` throughout.
- **Review-round-2 corrections (applied):** (1) `path_prov` mirrors `field_own` at every path — `graft_path_prov` is prefix-dependent (Field⇒rebase Elem/Val; Payload⇒rebase Field) and `AArrayLit` records `[Elem]` provenance (Task 3). (2) Match-arm seeding has an explicit **borrow-demote else branch** for a live scrutinee + a live-scrutinee negative test (Task 13). (3) SCC uses an **in-SCC suppression set** (order-independent), not a strip-and-final-pass; the recursive test asserts the specific under-approximation, not just determinism (Task 9). (4) `ret_paths` comparator returns `Order` via chained `Int.compare` on a canonical tuple, no packed keys (Task 6). (5) Caller gate **snapshots pre-call facts** before `params`/`ret` mutate `st` (Task 10). (6) Payload seeding is a **shared helper** applied at all entry-state sites (Task 13). (7) Co-Authored-By trailer is **conditional** per `AGENTS.md` (header).
- **Review-round-7 corrections (applied):** (1) `transfer_summarized_call` gains an explicit **`callee_id: Int`** parameter (it did not previously receive the callee id, which the SCC suppression check needs) — passed from `transfer_call`'s `summary_get(fid.id)` resolution, landing alongside `last`/`suppress` (Task 9 Step 3, Task 10 Step 3). (2) **`ARecordUpdate` is now concrete code**, not prose: captures `base_pp` before `consume_base`, shell prov is base-only, and `path_prov` mirrors the `field_own` remove-then-graft (Task 3 Step 2). (3) Added the **`ARecordUpdate` overwrite test** — the only coverage of `ARecordUpdate` path_prov carry/reattribution, doubling as the lockstep-invariant stale-origin guard (fresh base, `f0` from `p0`, `f1` overwritten with a fresh value ⇒ `[.f0]=from(p0) [.f1]=fresh`, `p1` dropped) (Task 7 Step 5). (4) Documented **which lockstep-invariant directions are self-guarding** (classify iterates field_own keys; publish over-publishes) so a key-set drift is not mistaken for unsoundness — the real risk is a wrong grafted origin, covered by the Task 7 attribution/drop tests; no heavyweight runtime checker added (Task 3 Step 2 note). (5) Added a **soundness-boundary note**: Tasks 5–12 are transiently under-publishing but harmless (analysis-only, unconsumed); no commit before Task 13 is a soundness checkpoint (header).
- **Review-round-6 corrections (applied):** (1) Payload codec is **O(1) fixed-width bit-packing** of `(tag, index, fieldslot)` into the negative range (20 bits each, multiply-not-shift to dodge the bitwise-precedence/fmt gotcha), replacing the √z-search Cantor pairing — `path_of_key` runs in `graft`/`project`/`remove_prefix` fixpoint loops, so decode must be constant-time (Task 1; design Decision 4 aligned). (2) **Task 9 no longer calls `seed_payload_binding`** (defined in Task 13) — Task 9 stops at `exit_path_prov` + the `path_prov` join; Task 13 owns the entry-site wiring, restoring green-per-task (Task 9/13). (3) Added a **"Fixture construction discipline"** section: build multi-block caller fixtures from real frontend output, confirm each move/borrow pair differs only in the liveness fact under test, eyeball the CFG once. (4) Task 15 gains a **compile-time regression check** (Step 3b, `TWINKLE_TIMINGS=1`) since `path_prov` is joined per block across the self-host. (5) Documented the **summary/seeding index asymmetry** (Task 8 records all payload indices; Task 13 seeds only single-`Var` index 0 — sound under-claim) and framed Phase 5 as **analysis-only with no observable win** (validation = suites + verdict dump + self-host + census 0 + no perf regression; win deferred to Phase 6). (6) Flagged that the copied **`caller_own` is block-0-only** and its `.None → Shared` default would make multi-block borrow-negatives (Tasks 11, 13) pass trivially — added a block-aware `own_in_block` helper and instructed the arm-bound Case R assertions to use it (Task 10 helper list, Task 13 Step 1, discipline point 4). All named helpers otherwise verified present in `cfg_summary_suite.tw`.
- **Review-round-5 corrections (applied):** (1) Payload-move gate uses liveness **live-in** (`blk.entry.live`), which catches a scrutinee read *inside* the arm (after the binding) as well as any live-out — with an in-arm-read negative test (Task 13). (2) Predecessor block id is `blk.preds[0].target.id` (in a `preds` edge `CfgEdge.target` holds the predecessor id, `cfg.tw:368-372`); asserts single-predecessor arm shape (Task 13). (3) `project_path_prov` has one standardized `.{ shell, inner }` return used by both `ARecordGet` (Task 3) and `seed_payload_binding` (Task 13).
- **Review-round-4 corrections (applied):** (1) `.Field(f)`/`.Field(fid)` no longer shadows the CfgFunction `f` — `fn_params := f.params` is captured before the classification loop and passed to `classify_path_own` in both the Direct and Variant branches (Task 7/8). (2) Payload-move seeding sets the binding's **shell prov** from the projected payload shell provenance (Task 13). (3) Payload seeding reads the scrutinee's facts from the **predecessor (match-block) exit** maps (`fx.exit_path_prov`/`exit_field_own`/…), not the live-filtered arm entry, since a dead scrutinee isn't live-in; `FixResult` gains `exit_path_prov` (Task 9/13). (4) Transport `dead-after` requires **both** `!exit_mentions_local` and `!live_contains_int(blk.exit.live, out)` (live-out), with an `out`-read-in-a-later-block negative test (Task 11).
- **Review-round-3 corrections (applied):** (1) `path_prov` threads through **projection & rebinding** — `AAssign`/`init_hinge` copy it with `field_own`, and `ARecordGet` projects/removes it and sets the result **shell prov from the projected field's provenance** (fallback to base prov for opaque param bases, keeping Task 3 inert); Task 13 payload move sets binding shell prov from the projected payload shell prov (Task 3, Task 13). (2) Variant payload **shell vs field ret-paths stay distinct** — the classifier emits both `V7[0]=fresh` and `V7[0].f0=from(p0)`; `record_ret_path` writes **exactly one key** per ret_path and never synthesizes a shell from a field path (Task 8, Task 10). (3) `classify_path_own` uses `Vector<LocalId>` (`CfgFunction.params`), not `Vector<Param>`, with a `param_index_of` over LocalIds (Task 7). (4) Design's SCC wording updated to the suppression-set mechanism (`phase5-design.md`). (5) Recursive fixture is a **two-return-site** `g` (recursive site sources `f0` from the recursive result; base site from `p0`) so the meet drops `[.f0]` while `[.f1]=fresh` survives (Task 9).
- **No test committed RED:** Tasks 3–5 are one execution unit; Tasks 3–4 are behavior-preserving (suite stays green), and the shell/deep observable tests live in Task 5 where they go green after the Return-publish removal.
- **Fixture helpers** (`case_w_fixture`, `case_r_fixture`, `case_r_live_scrutinee_fixture`, `recursive_transport_fixture`, etc.) are named per task; implement each in the suite when first referenced, mirroring `cfg_summary_suite.tw`'s ANF-builder style. Because they are prose-specified rather than fully coded, the executor builds them from that harness — the one deliberate concession to plan length. **Follow the "Fixture construction discipline" section** above: build multi-block caller fixtures from real frontend output where practical, confirm each move/borrow pair differs only in the liveness fact under test, and eyeball the CFG once before trusting a green run.
