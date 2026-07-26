# Making the Ownership Fixpoint's Own Maps Mutate In-Place

**Status:** Diagnosed; deferred into the sound-uniqueness analysis track.
`run_fixpoint` still does not flip. Progress + dead-ends as of 2026-07-26:
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
- **Real remaining blocker** (re-grounded by the above): the loop-carried maps
  reach `merge_targeted`/the write sites without being provably **unique at the
  caller** — the copy-carrier boundary — not a scalar-publication issue. Levers B
  (non-scalar closure-value provenance) and C (`FixState`/`FixResult` return
  double-embed) remain candidate analysis fixes, but the next real work is
  caller-side uniqueness, folded into the sound-uniqueness track.

Not a bounded quick win. The lesson from Lever A: trace each param's *actual*
publication route from the census before building a lever for it — the
"scalar-through-closure" model was assumed, not measured, and was wrong.

**Goal:** Get the compiler's hottest analysis loop — the ownership fixpoint in
`run_fixpoint` (`boot/compiler/ownership.tw`) — to emit in-place dict mutation
(`dict$set_in_place`) for its own loop-carried maps instead of persistent HAMT
`dict$set`. Because the compiler is self-hosted, this speeds up the compiler
itself.

**Tracked marker:** the red assertion `phase 8A mutable decision production::boot
ownership fixpoint maps should produce in-place dict decisions`
(`boot/tests/suites/mutable_produce_suite.tw`) asserts both `merge_targeted__`
and `run_fixpoint` produce a selected in-place dict decision. Both are currently
0. This doc owns that marker; it is an intentional target, not a regression.

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

Breadth still holds; the remaining publication routes are narrower and different
from Phase 0's:

1. **Closure-boundary argument/value conservatism (primary residual).**
   `merge_targeted` (`p0=Published p1=Published p2=Borrowed p3=Published …
   p6=Published ret_paths=.f0=from(p1) .f1=from(p3)`) and `same_map`
   (`p0=Published p1=Published`) publish their map params **through their opaque
   `eq`/`join` closure params**. Traced ops in `merge_targeted__Int`:
   `call L3069(L3077, L3078)` (the `eq` param applied to `old_x`/`next_x`).
   Because the callee is an indirect/closure atom, `publish_call` (`ownership.tw`)
   publishes **every** argument unconditionally. The scalar case is the easiest
   false positive — scalar-typed arguments cannot alias anything — but the active
   `run_fixpoint` path also uses non-scalar closure comparisons/joins
   (`merge_targeted__Vec_Int`, `same_map__Vec_Int`, `same_map__T538`,
   `same_map__Dict_Int_Vec_Int`) for provenance, field-ownership, and path-
   provenance maps. Those need a broader value-provenance/borrow story, not only a
   scalar skip. `merge_targeted`'s `p1`/`p3` are legitimately `Published`
   (returned as `.f0`/`.f1`). Do **not** include `p6` in this closure route: a later
   probe showed that the scalar default is published earlier by `lat_get`'s
   `ret=alias(p2)` fallback, not by `eq`/`join`.
2. **`FixState` + `FixResult` return double-embed.** `run_fixpoint` returns both
   aggregates sharing the same five exit-map objects; publishing one map into two
   aggregates aliases it. Bounded to the 5 exit maps, addressable by a source
   restructure.
3. **Outer-map threading through the `join_entry_*` family.** The
   `Dict<Int, Dict<Int, T>>` outer maps are threaded through many helpers per
   block-visit; the outer spine stays aliased even though the inner reads borrow.

Route 1's provenance is reproducible directly — the `ret_paths=` clause is what
separates legitimate publication from the false positives:

```bash
target/twk ir boot/main.tw --cfg \
  | grep -A1 -E '^fn (merge_targeted|same_map)' | grep -E '^fn |summary:'
```

**Two things condensed restatements of this route keep getting wrong** — read
them off that output, do not paraphrase from memory:

- **The active fixpoint paths are not scalar.** `same_map__Vec_Int`,
  `same_map__Dict_Int_Vec_Int`, and `merge_targeted__Vec_Int` all print
  `p0=Published`, and they carry `Vector`/nested-`dict` values through the
  `eq`/`join` closures — a scalar-only skip (Lever A) does **not** clear them.
  The non-scalar value-provenance work (Lever B) is on the live path, not a
  corner case; do not describe this route as "scalar publication."
- **`merge_targeted`'s `p1`/`p3` are legitimately `Published`.** The
  `ret_paths=.f0=from(p1) .f1=from(p3)` clause means those two maps are genuinely
  returned — a **sound** verdict. Trying to "un-publish" `p1`/`p3` is forcing past a
  correct verdict, exactly what the soundness frame forbids. `p6` is a separate,
  scalar-default route through `lat_get`'s return alias; it is useful for analysis
  correctness, but not the map blocker for the fixpoint flip.

## The residual levers (scoped, deferred)

None is a one-liner, and multiple routes likely must land together to flip
`run_fixpoint` (breadth). This is why the recommendation is to fold into the
sound-uniqueness analysis track (`docs/plans/sound-uniqueness/`), gated by its
equivalence guards, rather than pursue a standalone quick win.

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
- **Lever B — non-scalar closure argument/value provenance.** The scalar skip does
  not cover all active fixpoint paths: `old_prov`/`next_prov`, `old_field`/
  `next_field`, and `old_pp`/`next_pp` flow through closure comparisons over
  `Vector`, field-map, and nested-dict values. These values may be genuine
  references, so they cannot be blanket-skipped like scalars; the analysis needs
  to distinguish borrowing a value read from a map for an equality/join callback
  from publishing the map shell that supplied it.
- **Lever C — the `FixState`/`FixResult` double-embed.** A source restructure in
  `run_fixpoint` so the five exit maps are not simultaneously published into two
  returned aggregates.
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
  fully cleared). **But it flips no in-place decision** — `run_fixpoint` stays
  30/30 `persistent`, `merge_targeted` stays 0 — because the actual map *writes*
  are gated on other routes (`merge_targeted`'s `out[k]=` needs `p1`/`next` unique
  at the caller = copy-carrier boundary; `p6` is only the scalar default-value
  return-alias route). Lever D is thus a confirmed **necessary-not-sufficient**
  precision fix: independently correct (`.keys()` genuinely borrows the dict and
  returns a fresh vector, so the old conservative publish was pure imprecision
  affecting every dict-keys loop),
  and a prerequisite for the flip, but no standalone win. This is the hard-data
  instance of "breadth."

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
by **D alone**. The remaining blocker to the flip is **caller-side uniqueness** of the
loop-carried maps at the `merge_targeted` call (copy-carrier boundary), plus possibly
Lever B (non-scalar closure-value provenance for the `__Vec_Int`/`__Dict_Int_Vec_Int`
monomorphs) and Lever C (the return double-embed) — folded into the sound-uniqueness
track, not chased as standalone levers.

## Copy-carrier boundary (why `merge_targeted` still won't flip on its own)

The copy-carrier borrow/effect engine
(`docs/plans/archive/2026-07-24-copy-carrier-engine-impl-plan.md`) is landed and
self-host-stable. `merge_targeted__` earns an **accepted** copy-carrier proof for
its `out := next; out[k] = …` shape — but it stays `persistent(aliased shell)`
because its `run_fixpoint` callers do not pass the map **uniquely**, and the
`uniform_entry_seeds` mixed-caller guard correctly refuses to seed it Unique. So
`merge_targeted` is proven safe *for a unique caller* but its actual caller is not
unique. The working hypothesis is that making the loop-carried maps provably
Unique at the point they are threaded into `merge_targeted__` — across the closure
provenance routes above, plus the return double-embed where relevant — will
unblock the helper and caller sites together. Re-census, not assumption, is the
acceptance gate.

---

## Acceptance (for any later fix)

Micro-fixtures are only the gate's first half. A fix is not done until the target
in the compiler itself flips after rebuilding `target/twk`:

```bash
make bundle-cli
target/twk ir boot/main.tw --census --sites \
  | rg -n "^run_fixpoint\t|^merge_targeted|^join_entry_ownership_assumed"
```

The `run_fixpoint` loop-carried map updates (and any helper sites in scope) must
flip from `base=persistent(aliased shell)` to `base=reuse(unique)` /
`dict$set_in_place`, the tracked marker assertion must go green, and
`summary:roots run` must actually improve — under the soundness frame above. If
the fixtures pass but `run_fixpoint` stays persistent, the fix is incomplete;
return to tracing the remaining publishing route.

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
