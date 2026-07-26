# Making the Ownership Fixpoint's Own Maps Mutate In-Place

**Status:** Diagnosed; implementation path identified for the main `run_fixpoint`
optimization. The clean tree still reports `run_fixpoint` as 30/30 persistent, but
an isolated cold-only proof flipped the core loop-carried map updates to in-place.
Progress + dead-ends as of 2026-07-26:
- **Lever D LANDED** (commit `7ddda40f`): registering `Dict.keys` as `.ReadOnly`
  stopped `.keys()` from conservatively publishing its dict. Cleared
  `merge_targeted`'s `p0` and `same_map`'s `p0`/`p1` to `Borrowed`. Sound; flips no
  in-place decision on its own.
- **Lever A BUILT then REVERTED** (stashed): scalar-arg non-publication. Sound and
  self-host stable, but a diagnostic **disproved its premise** — the scalar skip
  fires on the closure args yet `p6` stays `Published`, and a follow-up probe traced
  `p6` to the `lat_get` return-alias fallback instead. `p6` (a scalar default) isn't
  the flip blocker anyway (the map `p0` was already cleared by D). See the Lever A
  entry below.
- **Cold/warm split PROVED as the main enabler:** the hot `run_fixpoint` body joins
  two initialization shapes in one function: cold `Dict.new()` maps and warm maps
  loaded from `FixState`. The warm arm makes the non-backedge loop seed contribution
  `Unknown`, so the loop-carried maps cannot be proven Unique. A temporary cold-only
  edit rebuilt self-host and flipped `run_fixpoint` from 30/30 persistent to 28/30
  selected in-place; the remaining two persistent rows were the `dirty0`/worklist map,
  not the core fixpoint maps.

Not a bounded quick win, but now bounded: split the cold solver path from the warm
incremental solver path without extracting the hot map loop into a param-taking
helper. The lesson from Lever A still stands: trace each param's *actual*
publication route from the census before building a lever for it — the
"scalar-through-closure" model was assumed, not measured, and was wrong.

**Goal:** Get the compiler's hottest analysis loop — the ownership fixpoint in
`run_fixpoint` (`boot/compiler/ownership.tw`) — to emit in-place dict mutation
(`dict$set_in_place`) for its own loop-carried maps instead of persistent HAMT
`dict$set`. Because the compiler is self-hosted, this speeds up the compiler
itself.

**Tracked marker:** the red assertion `phase 8A mutable decision production::boot
ownership fixpoint maps should produce in-place dict decisions`
(`boot/tests/suites/mutable_produce_suite.tw`) currently asserts both
`merge_targeted__` and `run_fixpoint` produce a selected in-place dict decision.
The cold-only proof shows the `run_fixpoint` half is independently reachable;
`merge_targeted__` remains a separate helper/call-variant question. If the cold/warm
split lands first, either split this marker or keep it red until the helper side is
also enabled.

---

## Why this is the lever

After incremental re-propagation landed (`docs/plans/archive/incremental-repropagation.md`),
the dominant compile-time cost is the summary stage:

```text
[time:summary:roots] total≈6.86s … run≈6.78s funcs=3841 roots=464 wanted=3194
```

`summary:roots run` is the `for scc in sccs { run_scc(...) }` loop in
`summary.tw` — running `run_fixpoint_validated` → `run_fixpoint` over ~3194
functions. `run_fixpoint` maintains **14 loop-carried maps** (`exits`,
`exit_valid`, `exit_prov`, `exit_field_own`, `exit_path_prov`, the `prev_*`
snapshots, `locked_*`, `prev_seen`, `changed_visits`, `processed`), mostly
`Dict<Int, Dict<Int, T>>`. Every round, for every block, it does `nested_get`
reads, a `merge_targeted` (builds a new dict), and ~5 outer-dict index-sets —
each a persistent `dict$set` that rebuilds a HAMT spine, over 243 blocks × several
rounds × 3194 functions. Instrumentation attributed ~5.4 s of the loop to genuine
fixpoint computation dominated by persistent dict churn — exactly what in-place
cuts. (A small ~360 ms `reruns==0` redundant-final-pass elision is possible and
orthogonal; it is not the prize.)

The self-hosting insight: an optimization that makes owned-collection code emit
in-place mutation speeds up any dict-heavy Twinkle program — **including the
compiler's own fixpoint.**

## The soundness frame (unchanged, load-bearing)

`persistent(aliased shell)` is a **sound** verdict: mutating a genuinely aliased
dict in place corrupts the other alias (worked-examples Case C, the linearity
hinge). The fix is always to **prove non-aliasing more precisely**, never to force
in-place past the verdict. **Codegen is already correct** — `join_entry_ownership`
proves in-place fires end-to-end when handed a unique proof
(`phase8b-loop:join_entry_ownership:carry …`). All work here is upstream, in
summaries/facts. Byte-identity is **not** the acceptance gate (turning `dict$set`
into `dict$set_in_place` changes emitted bytes by design); the gate is behavioral
equivalence (the 8D/8E round-trip/equivalence guards), `TWINKLE_FIXVERIFY` clean,
self-host stable, full boot suite green, plus a real `summary:roots run`
improvement.

---

## Current ground truth (re-baselined 2026-07-26, post copy-carrier engine)

The original diagnosis (Phase 0, 2026-07-24) pinned the cause as **breadth**
(Hypothesis 4): the loop-carried maps were `aliased shell` because the whole
read-helper family *published* its `Dict` param whenever a value read out of it
flowed to a return. That has since changed — measure before acting.

**The read-helper route is already repaired.** Reading current summaries
(valid no-rebuild probe — see gotcha) shows every helper Phase 0 named now
**borrows** its dict param. Per-param provenance prints on the `summary:` line
**under** each `fn` header in `--cfg`, so pair the two lines with `grep -A1`:

```bash
target/twk ir boot/main.tw --cfg \
  | grep -A1 -E '^fn (is_processed|is_dirty|fact_of|valid_of_local|nested_get|lat_get)\b' \
  | grep -E '^fn |summary:'
# → every summary reports p0=Borrowed  (Phase 0 reported p0=Published)
```

| helper | dict param | was (Phase 0) |
|---|---|---|
| `is_processed`, `is_dirty`, `fact_of`, `valid_of_local` | `p0=Borrowed` | `Published` |
| `nested_get` (all monomorphs, incl. container-returning) | `p0=Borrowed` | `Published` |
| `lat_get` | `p0=Borrowed` | — |

This repair came in with the copy-carrier-era work; the `AIndex` scalar-read
precedent (`ownership.tw`, the `scalar_result_ty(elem_ty)` guard: "a primitive
element read cannot alias the collection shell") is the pattern that generalized.

**But `run_fixpoint` is still 30/30 `persistent(aliased shell)`** — the
read-helper repair was necessary but not sufficient. Confirm with the census
(every `dict_set` row's in-place column is `false`):

```bash
target/twk ir boot/main.tw --census --sites \
  | awk -F'\t' '$1=="run_fixpoint" && $2=="dict_set"{print $5}' | sort | uniq -c
# → 30 false        (0 flipped to dict$set_in_place)
```

### Cold/warm split proof (temporary probe, reverted)

A focused seed probe on the first `run_fixpoint` loop-carried map showed the backedge
already preserves uniqueness; the seed is dropped only because the non-backedge entry
comes from an `Unknown` warm/cold join:

```text
[probe:seed] label=summary:run_fixpoint block=51 lid=3558 seed=true kept=false
[probe:seedpred] block=51 pred=49 backedge=false arg=3558 own=Unknown valid=true
[probe:seedpred] block=51 pred=55 backedge=true  arg=3558 own=Unique  valid=true
```

The next header initially validates while the first seed is still assumed, then
cascades to `Unknown` after block 51's seed is removed:

```text
[probe:seed] label=summary:run_fixpoint block=71 lid=3558 seed=true kept=true
[probe:seedpred] block=71 pred=70  backedge=false arg=3558 own=Unique valid=true
[probe:seedpred] block=71 pred=149 backedge=true  arg=3558 own=Unique valid=true
```

The proof edit was deliberately blunt: inside `run_fixpoint`, replace every
`case warm_state { .Some(w) => w.<map>, .None => Dict.new() }` map initializer with
fresh `Dict.new()` and set `warm_started := false`, while leaving the rest of the
function unchanged. After `make bundle-cli`, the census changed to:

```bash
target/twk ir boot/main.tw --census --sites \
  | awk -F'\t' '$1=="run_fixpoint" && $2=="dict_set"{print $5}' | sort | uniq -c
# → 2 false
# → 28 true
```

Representative flipped rows:

```text
run_fixpoint dict_set dict$set dict$set_in_place true  L3718 = update L3556 base=reuse(unique)
run_fixpoint L3718 dict_set dict$set dict$set_in_place dict$set_in_place selected MutableSelected phase8b-loop:run_fixpoint:carry L3556:site L3718:depth 1
```

The two remaining persistent rows were updates to the `dirty0`/worklist map (`L3593`
in the probe), not the core five exit maps or their widening state. This proves the
main optimization path is a **source-shape split**: keep the cold map allocations in a
cold-only solver body so the ownership pass sees fresh `Dict.new()` maps, and use a
separate warm/incremental body only for reruns that actually need `FixState`.

Breadth still holds for the `merge_targeted__` helper body, but it is no longer the
first enabler for the hot `run_fixpoint` map churn. The post-probe ranking is:

1. **Cold/warm source-shape conflation (primary for `run_fixpoint`).** The current
   clean body contains both cold `Dict.new()` initialization and warm `FixState` loads.
   That single source shape makes the non-backedge predecessor of the hot loop
   contribute `Unknown`, even when the backedge is `Unique`. Splitting the cold body is
   the path that actually flipped the core map sites in the proof.
2. **`merge_targeted__` helper/call-variant precision (separate follow-up).** Current
   summaries are reproducible with:

   ```bash
   target/twk ir boot/main.tw --cfg \
     | grep -A1 -E '^fn (merge_targeted|same_map)' | grep -E '^fn |summary:'
   ```

   Post-Lever-D, `same_map__*` reports borrowed map params, and `merge_targeted__*`
   reports `p0=Borrowed`; `p1`/`p3` are legitimately `Published` because
   `ret_paths=.f0=from(p1) .f1=from(p3)` means they are returned. `p6` is a separate
   scalar-default route through `lat_get`'s return alias, not a closure route. If the
   generic `merge_targeted__` body must flip too, chase that as a distinct helper or
   owned-variant problem, not as the first `run_fixpoint` enabler.
3. **`FixState` + `FixResult` return double-embed / non-scalar closure precision.**
   These remain plausible later precision levers, but the cold-only proof shows they
   are not required to flip the core direct `run_fixpoint` map updates.

## The residual levers (re-ranked after the cold-only proof)

The next implementation should target the cold/warm source-shape split first. The
probe showed this alone flips the core `run_fixpoint` updates. Levers B/C remain
useful for the `merge_targeted__` helper body and broader precision, but they are no
longer the first enabler for the main fixpoint-map optimization.

- **Lever A — scalar-argument non-publication at the closure/indirect-call
  boundary. BUILT + MEASURED + REVERTED (2026-07-26); sound but ineffective, and
  the premise was wrong.** The idea: extend the `AIndex`/`scalar_result_ty`
  principle to `publish_call` (the `for a in args { st = .publish_atom(a) }` loop) —
  a scalar (unboxed) argument has no interior to corrupt, so publishing it is
  meaningless and only poisons provenance. It was fully implemented: a
  `CfgFunction.op_result_mono` field (populated in `build_function` from
  `AnfFunctionDef.op_result_mono`), a `build_is_scalar` predicate, a ride-along
  `ForwardState.is_scalar` field threaded through `run_fixpoint` /
  `stabilize_seeds` / `run_fixpoint_validated` / `ownership_stage` /
  `summarize_seeded` / `call_uniques` / `analyze_function`, and an `atom_is_scalar`
  skip in `publish_call`. It is **self-host stable** (stage3==stage4) and
  behaviorally clean. **The implementation is preserved in `git stash` (message
  "Lever A: scalar-arg non-publication"); it is not committed.**

  **Why it was reverted — a diagnostic probe disproved the premise.** Instrumenting
  `summarize_seeded` showed `is_scalar` is populated correctly: for
  `merge_targeted__Int` the closure args `L3091`/`L3092` (`old_x`/`next_x`, both
  `Int`) report `has3091=true has3092=true`, so `publish_call` **does** skip them.
  Yet `p6` (`default_value`) **stays `Published`.** Therefore the assumed route —
  "`p6` is published because scalar values read from the maps flow through the
  `eq`/`join` closures" — is **false**: the skip fires on exactly those atoms and
  p6 does not clear. Follow-up probing found the real route: the first
  `lat_get__Int(old, k, default_value)` call summarizes as `ret=alias(p2)`, and
  `transfer_summarized_call` cannot move that return alias because the default
  argument is not Unique+last-use, so the `.MayAliasParams` fallback publishes
  arg 2 directly. Second, and more decisive: **`p6` is `default_value`, a scalar,
  not a loop-carried map.** The map that gates the flip is `p0` (`old`), which
  **Lever D already cleared to `Borrowed`.** So even a working scalar skip would not
  advance the `run_fixpoint` flip — `merge_targeted`'s `out[k]=` write (where `out`
  aliases `p1`/`next`) is gated on **caller-side uniqueness (the copy-carrier
  boundary)**, not on scalar-arg publication. Net: Lever A neither cleared its
  target nor targeted the blocker.

  Gotcha for anyone reviving the stash: `op_result_mono` is a **complete** local→type
  map at the analyzed level (the IR printer reads the `: Int` annotation straight
  from `func.op_result_mono[local.id]` at `ir_print.tw:313`), despite being passed
  read-only through the optimizer's fixed-point simplifications — so coverage was
  *not* the problem; the premise was.
- **Lever E — split cold and warm `run_fixpoint` solver bodies. PROVED by probe,
  not yet landed.** Today one function contains both shapes: cold `Dict.new()` maps
  and warm maps loaded from `FixState`. The ownership analysis must summarize the
  joined source, so the cold loop header's non-backedge predecessor contributes
  `Unknown` and the loop seed is dropped even though the backedge is `Unique`. A real
  fix should keep the hot cold solve in a body whose map locals are initialized only
  from `Dict.new()`. Do not extract the hot loop into a helper that takes the maps as
  parameters, or the proof will likely be lost again. Route first seed pass,
  non-incremental reruns, and final pass to the cold body; route only incremental
  warm reruns to the warm body.
- **Lever B — non-scalar closure argument/value provenance.** Deferred for the
  helper/body side. The scalar skip does not cover all active fixpoint paths:
  `old_prov`/`next_prov`, `old_field`/`next_field`, and `old_pp`/`next_pp` flow
  through closure comparisons over `Vector`, field-map, and nested-dict values. These
  values may be genuine references, so they cannot be blanket-skipped like scalars;
  the analysis needs to distinguish borrowing a value read from a map for an
  equality/join callback from publishing the map shell that supplied it.
- **Lever C — the `FixState`/`FixResult` double-embed.** Deferred unless the cold/warm
  split leaves return-side publication as the next measured blocker. A source
  restructure in `run_fixpoint` may still be needed so the five exit maps are not
  simultaneously published into two returned aggregates.
- **Lever D — register `Dict.keys` in the optimizer's `CallSemantics` (new,
  verified 2026-07-26; landed + measured).** `dict$keys` is a registered runtime builtin
  (`builtins.tw:516`) but has **no `CallSemantics` entry** in
  `opt/semantics.tw` (which registers `Dict` `set`/`remove`/`get`/`new` but not
  `keys`). So `call_info` returns `.None`, it is not summarized (it's an rt
  extern), and `transfer_call` drops it into the conservative `publish_call`
  bucket — which **publishes the dict argument**. That is the actual route by which
  `merge_targeted`'s `p0` (`old`) is `Published`: the `old.keys()` call **is** the
  publisher — `publish_call` marks `old` Shared directly and does not even
  propagate provenance to the keys result (so `int_keys_union`'s own
  `p0=Published` is incidental, operating on an empty-provenance vector). Not a
  scalar route, not a closure route — a missing registration. The apparent fix is a
  `Dict.get`-shaped entry (`effect: .ReadOnly, cow_base_arg: .None,
  retained_args: .Some([])`: borrows the dict, returns a fresh keys vector carrying
  no dict provenance). **Caveat — not a free one-liner:** keys-order provenance is
  load-bearing for the copy-carrier key-stream machinery (`ownership.tw:722`, "a
  keys-order loan created before the write survives"; the completed key-stream
  uniqueness work). The `.ReadOnly, cow_base_arg: .None`
  registration was applied and is **sound**: self-host reaches its stage3==stage4
  fixed point and the full boot suite passes except the intentionally-red tracked
  marker. Key-stream detection is independent (it matches `ops.keys` on the builtin
  id, not `CallSemantics`), so the two do not interact. **Measured effect on
  provenance** (before → after): `merge_targeted p0=Published → Borrowed`;
  `same_map p0=Published p1=Published → Borrowed Borrowed` (both false positives
  fully cleared). **But it flips no in-place decision** — the clean tree still has
  `run_fixpoint` at 30/30 `persistent` and `merge_targeted` at 0. Later probing
  separated those blockers: `run_fixpoint` is blocked by cold/warm source-shape
  conflation, while `merge_targeted__` remains a helper/copy-carrier precision
  problem; `p6` is only the scalar default-value return-alias route. Lever D is thus
  a confirmed **necessary-not-sufficient** precision fix: independently correct
  (`.keys()` genuinely borrows the dict and returns a fresh vector, so the old
  conservative publish was pure imprecision affecting every dict-keys loop), and a
  prerequisite for the flip, but no standalone win. This is the hard-data instance
  of "breadth."

### Route breakdown for `merge_targeted__Int` (2026-07-26; p6 route CORRECTED)

`p0` and `p6` are **two different routes**:

- **`p0` (`old`) — unregistered `Dict.keys`, now fixed by Lever D.** `old.keys()`
  hit the conservative publish bucket and published the dict directly; the `.ReadOnly`
  registration cleared it to `Borrowed`.
- **`p6` (`default_value`) — `lat_get` return-alias fallback, not scalar-through-closure.**
  The tempting model was: `lat_get` summarizes `ret=alias(p2)`, so
  `old_x := lat_get(old, k, default_value)` makes `old_x` alias `p6`; `old_x`/`next_x`
  flow into the `eq`/`join` closures, and `publish_call` publishes them → `p6`. This
  is **disproven.** Lever A skipped exactly those scalar closure args and `p6`
  **stayed `Published`.** The measured route is earlier: in `transfer_summarized_call`,
  `lat_get__Int`'s `ret=alias(p2)` enters the `.MayAliasParams` path. Because the
  default-value argument is not Unique+last-use, the return-alias move gate fails, so
  the fallback publishes arg 2 (`default_value`) directly; the first such call in
  `merge_targeted__Int` marks p6 Shared, and `summarize_seeded` observes it as
  `.Retained` on later block exits. This is not on the `run_fixpoint` critical path
  for the flip: p6 is a scalar default, not a loop-carried map.

**Correction to the earlier "needs A+D together" claim:** that was wrong. `p6` is not
the map blocker and Lever A does not clear it. The map that mattered (`p0`) is cleared
by **D alone**. A later cold-only probe refined the remaining blocker again: for the
hot `run_fixpoint` updates, the first enabler is not B/C but splitting the cold solver
from the warm `FixState` solver so fresh `Dict.new()` maps are not joined with warm
state-loaded maps. B/C remain candidate follow-ups for the standalone `merge_targeted__`
helper verdict and broader precision.

## Copy-carrier boundary (why `merge_targeted` still won't flip on its own)

The copy-carrier borrow/effect engine
(`docs/plans/archive/2026-07-24-copy-carrier-engine-impl-plan.md`) is landed and
self-host-stable. `merge_targeted__` earns an **accepted** copy-carrier proof for
its `out := next; out[k] = …` shape — but the generic helper body still renders its
own update as `persistent(aliased shell)`:

```text
merge_targeted__Int dict_set dict$set dict$set_in_place false L3110 = update L3070 base=persistent(aliased shell) borrow-effect copy-carrier source L3062 key L3076
```

The cold-only split proof did **not** flip these `merge_targeted__*` rows; it flipped
the direct `run_fixpoint` map updates. That means the `run_fixpoint` win can land
first via Lever E even if the helper marker remains red. Treat `merge_targeted__` as a
separate follow-up: either a caller-selected owned variant must be made visible in the
production/census path, or B/C-style precision work must make the generic helper body
prove its carrier Unique. Re-census, not assumption, is the acceptance gate.

---

## Acceptance (for any later fix)

Micro-fixtures are only the gate's first half. A fix is not done until the target
in the compiler itself flips after rebuilding `target/twk`:

```bash
make bundle-cli
target/twk ir boot/main.tw --census --sites \
  | rg -n "^run_fixpoint\t|^merge_targeted|^join_entry_ownership_assumed"
```

For Lever E, the first acceptance gate is narrower than the historical marker:
`run_fixpoint`'s core loop-carried map updates should flip from
`base=persistent(aliased shell)` to `base=reuse(unique)` / `dict$set_in_place`.
The cold-only probe's target was 28 selected rows and 2 remaining persistent
`dirty0`/worklist rows; a real implementation should match or explain any drift.
Then run the behavioral gates under the soundness frame above: `TWINKLE_FIXVERIFY`
clean, self-host stable, full boot suite green, and a real `summary:roots run`
improvement. If `run_fixpoint` flips but `merge_targeted__` remains persistent,
that is a follow-up marker/precision issue, not evidence that the cold/warm split
failed.

## Methodology gotcha (cost several inert probes historically)

`target/twk ir --census/--cfg` computes ownership with `target/twk`'s
**already-compiled** analysis logic; it only treats `boot/main.tw` as *input
source*. Editing **analysis logic** (`opt/semantics.tw`, `ownership.tw` transfer /
`publish_call`) has **no effect** on the output until `target/twk` is rebuilt
(`make bundle-cli`). Only edits to **analyzed function bodies** (e.g. tweaking
`nested_get` itself) are valid no-rebuild probes. Reading existing summaries — as
in the ground-truth table above — is always valid.

Second gotcha, cheaper but real: in `--cfg` the per-param `p0=…` provenance is on
the `summary:` line **beneath** the `fn` header, not on the header itself, so a
naive `grep 'p0='` filtered by function name matches nothing. Always pair the two
lines (`grep -A1 -E '^fn NAME' | grep -E '^fn |summary:'`) as the ground-truth
commands above do.

## References

- `docs/plans/sound-uniqueness/README.md` — the analysis/codegen track this belongs to.
- `docs/plans/sound-uniqueness/analysis/worked-examples.md` — Case W (transport-wrapper),
  Case C (linearity hinge), and cross-cutting finding #6 (the breadth pattern).
- `docs/plans/performance/compiler.md` — perf history incl. incremental re-propagation.
- Verdict/census tooling: `boot/compiler/census.tw`,
  `boot/compiler/codegen/ownership_verdicts.tw`.
