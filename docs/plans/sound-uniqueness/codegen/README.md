# Codegen Track

**Status:** In progress. Phases 7A (hook inventory), 7B (operation catalog),
the 7C/7D backend decision seam, Phase 8A local vector indexed-update
emission, Phase 8B loop-carried vector indexed-update emission, Phase 8D
dict-set in-place emission, and Phase 8E dict-remove in-place emission are done
(2026-07-21). The surviving mutable hooks
and their persistent→mutable mappings are cataloged, and codegen now has a
persistent-only selector + plumbing seam: `compiler.codegen.mutable_select` (the
decision table + selector), `compiler.codegen.mutable_catalog` (shared
persistent→mutable catalog lookup), and `compiler.codegen.emit.mutable_sites`
(prepared-site extraction), threaded through `PreparedModule` and `EmitCtx`. The
Phase 7D seam was verified byte-identical while persistent-only; Phase 8A now
uses that same seam to select the vector in-place helper only for proven local
owned sites. The Phase 7E update-site slice is done: `twk ir --census --sites`
now renders persistent→mutable
targets plus ownership verdicts for vector/dict/record update candidates. Phase
8A added the first emitted slice: proven local owned vector indexed updates select
`vector$set_in_place`, while aliasing cases fall back through the persistent path.
Phase 8B extends that to loop-carried accumulators (single and nested loops): the
producer no longer suppresses loop-contained candidates, so index-assignment sugar
carried across a loop selects `vector$set_in_place` when the ownership fixpoint
proves the carried vector unique across every entry and back-edge, and aliased loop
vectors stay persistent. Phase 8D generalized the decision producer to all
supported call-swap families and enabled `dict$set_in_place` for owned `Dict.set`
sites via the cumulative `enabled_emit_policy`; Phase 8E flipped that policy's
`emit_dict_remove` so owned `Dict.remove` sites select `dict$remove_in_place`. All
three call-swap families (vector set, dict set, dict remove) now emit in-place for
proven-owned sites. Phase 8C (builder-region lowering, first slice) and Phase 8F
(local record shell update emission) are also done. Phase 8G
(ownership-specialized function variants + call-site routing) is done: a clone of a
recursive owned function emits `vector$set_in_place` (and record shell reuse) end to
end via `variant_specialize.tw`; self-host reaches a fixed point. **Current focus:
record-backed field collections (8H)**, with 8C follow-up slices
(non-empty seeds, typed routing, conditional/multi-exit folds) deferred. The 7E
variant-routing dry-run item is closed by 8G's `render_routes`. The full analysis track is
complete through Phase 6 (record/field ownership,
transport-wrapper / `Result`-payload return-path summaries, ownership-specialization
decision facts, and recursive SCC variant-qualified diagnostics), so codegen consumes
a trustworthy fact set rather than rediscovering ownership — **except** the reopened 8C
`linearly_folded` region-safety prerequisite (Plan 1), which the analysis track does not
yet produce and must add before 8C's rewrite (see
[builder-region-design.md](builder-region-design.md) and
[../analysis/README.md](../analysis/README.md)). (These "Codegen Phase
7A/…" labels are the codegen track's own local numbering; see the phase-numbering
note in [../analysis/README.md](../analysis/README.md).)

This track owns the practical bridge from proof facts to emitted code. It should
first reuse today's persistent/in-place/builder mechanisms, not introduce the
future mutable-intrinsic layer. Source code keeps one immutable API such as
`xs.set_at(i, v)`; internally, a proven site may select a mutable target instead
of the persistent target.

- Phase 8A mutable decision production now uses fingerprinted shared ownership
  artifacts. Build codegen still consumes decision records only; CFG ownership
  remains the proof source, and stale artifact keys, absent verdicts,
  stale prepared sites, ambiguous decisions, and aliases fall back persistently.
  Default production scopes ownership analysis to the candidate functions and
  their dependency closure (verdict-equivalent to whole-program analysis), which
  removed a whole-program ownership pass from every build
  (`produce_mutable_decisions` ~22.9s → ~1.3s on `boot/main.tw`).

[../architecture.md](../architecture.md) is the umbrella design. The analysis
inputs come from [../analysis/README.md](../analysis/README.md); later cleanup
belongs to [../migration/README.md](../migration/README.md).

## Focused slice docs

| Doc | Purpose |
|---|---|
| [existing-hooks.md](existing-hooks.md) | Current backend/runtime hooks and non-hooks to verify before codegen work. |
| [operation-catalog.md](operation-catalog.md) | Mutable operation families and persistent→mutable target mapping. |
| [handoff-contract.md](handoff-contract.md) | ANF-keyed decision records, stale fallback, and variant-routing records. |
| [vector-lowering.md](vector-lowering.md) | Vector indexed update and vector builder slice notes. |
| [string-lowering.md](string-lowering.md) | String-concat builder-region slice notes. |
| [dict-lowering.md](dict-lowering.md) | Dict set/remove slice notes. |

## Track invariants

- Codegen consumes decisions; it does not re-prove uniqueness, last-use, field
  ownership, or escape.
- Absence, ambiguity, or staleness of a decision emits the persistent operation.
- Each lowering family starts as a narrow vertical slice with inspection output
  before broadening.
- Existing runtime/compiler hooks are implementation targets, not independent
  legality sources.
- Mutable-target lookup and persistent fallback stay centralized; backend families
  do not hand-roll ownership legality or stale-decision checks.
- Variant-qualified diagnostics never license rewriting the generic function body.
  Codegen must clone/route by exact `VariantId`, with the generic function as the
  persistent fallback.

## Codegen Phase 7A — Existing hook inventory ✅ done (2026-07-20)

No emitted-code change. Start with the concrete backend/runtime surface we already
have before designing side tables around it.

- [x] **Inventory current hooks.** Recorded in [existing-hooks.md](existing-hooks.md):
  vector set (`vector$set_in_place`), vector/typed/string builders, dict in-place
  set/remove, and the record-update `can_reuse` slot. Function-cloning/routing has
  no runtime hook (compiler cloning only) and is deferred to Phase 8G.
- [x] **Verify hook signatures.** Helper/op names, operand order, result behavior
  (all in-place helpers return the updated ref), ABIs, and persistent fallbacks are
  in the verified hook table. Phase 8A has now captured WAT/call-inspection
  evidence for the vector mutable target; later mutable families still need their
  own emitted-call evidence when they are enabled.
- [x] **Classify non-hooks.** `@std.buffer`, `Cell`/`Task`/`Channel`/host I/O, and
  read/share ops (slice/concat/gather/reads) are classified as non-targets in
  existing-hooks.md's Non-hooks section.

**Headline finding:** vector/dict runtime helpers, builder families, and record
`can_reuse` emit support survive from the previous COW era, but the ownership-driven
rewrite pass that selected them was removed (`opt/pipeline.tw`). Semantic builder
lowering such as `collect` is separate and still uses builders; codegen re-drives
ownership-optimized uses from the new sound facts (`ownership.tw`), not from scratch.

## Codegen Phase 7B — Operation catalog ✅ catalog and first inspection done (2026-07-20)

No emitted-code change. This phase answers: “if this candidate is accepted, what
exact existing target would codegen use?”

- [x] **Catalog mutable operation families.** [operation-catalog.md](operation-catalog.md)
  now maps persistent fallback → mutable target with ABIs and operand mapping for
  vector indexed update, vector/string builder regions, dict set/remove, and record
  shell update. Record-backed field collections (8H) and ownership-specialized
  variants (8G) remain named-but-later, as intended.
- [x] **Keep unsupported families persistent.** The catalog's "Later families"
  section keeps concat/extend, private representations, and multi-key specialization
  out of the first cut; the emission-state note reiterates persistent-by-default.
- [x] **Name inspection signatures.** Phase 8A captured the first concrete
  evidence: `twk ir --census --sites` renders the selected vector-set decision,
  and `twk wat ... --calls` shows the selected `rt_arr__set_in_place` call for
  the owned fresh fixture while aliasing and loop-contained fixtures stay on
  `rt_arr__set`. Later families will add their own family-specific call evidence.

## Codegen Phase 7C — Decision records and handoff contract ✅ seam done (2026-07-21)

No optimized emission yet. This phase makes the analysis→backend seam explicit
and fail-safe.

- [x] **Define first-cut backend decision records.** Implemented as
  `compiler.codegen.mutable_select.MutableDecision`: operation family, ANF site
  (`Site` = func + result local), source/result locals, stable argument shape
  (arg count + base-arg index), persistent fallback, would-be mutable target,
  optional `VariantId`, type-qualified field/path key, and `proof_debug_id`. The
  real analysis proof-payload producer is deliberately deferred; 7C/7D proves the
  backend seam with explicit tables first.
- [x] **Define one catalog-driven selection helper/layer.** Implemented as
  `compiler.codegen.mutable_select`, shared catalog lookup in
  `compiler.codegen.mutable_catalog`, plus prepared-site extraction in
  `compiler.codegen.emit.mutable_sites`. Backend lowering asks the selector for
  call or record selections and receives `emit_func` / `emit_can_reuse` values that
  remain persistent in Phase 7D, while `would_func` / `would_reuse` preserve the
  would-be mutable target for tests and later dry-run rendering. Per-family emit
  sites do not duplicate ownership legality, staleness, or fallback checks.
- [x] **Attach decisions as ANF-keyed side tables.** `PreparedModule` carries a
  `MutableDecisionTable` across backend preparation; `prepare_backend(...)` supplies
  an empty table by default and `prepare_backend_with_mutable_decisions(...)` exists
  for tests and future producers. `EmitCtx` carries the same table into emission.
- [x] **Define staleness handling.** The selector reports absent, ambiguous,
  unsupported, wrong-family, wrong-persistent-target, source/result mismatch,
  argument-shape mismatch, base-argument mismatch, and field-path mismatch cases,
  and every such case emits the persistent fallback.

## Codegen Phase 7D — Backend lookup and persistent fallback plumbing ✅ done (2026-07-21)

Still no optimized emission. This phase wires the backend to consume the side table
while deliberately returning the persistent target for every site.

- [x] **Thread the decision table to backend/codegen entry points.**
  `PreparedModule.mutable_decisions` is copied into `EmitCtx`; the default empty
  table preserves persistent output, verified byte-identical against the pre-seam
  compiler.
- [x] **Validate lookup/fallback paths.** Selector and prepared-site tests cover
  present, absent, stale/mismatched, ambiguous, and unsupported decisions; emission
  tests guard that vector/dict calls and record updates still emit persistent output
  (WAT keeps the persistent runtime call / `struct.new` copy path, never the
  `*_in_place` helper or `struct.set` reuse).
- [x] **Keep fallback centralized.** Emission delegates prepared call/record site
  extraction to `compiler.codegen.emit.mutable_sites`, mutable target mapping to
  `compiler.codegen.mutable_catalog`, and decision classification to
  `compiler.codegen.mutable_select`; family-specific emit code does not re-prove
  ownership or hand-roll stale-decision checks.

## Codegen Phase 7E — Dry-run rendering 🚧 update-site slice done (2026-07-21)

No optimized emission yet. This phase proves the seam and inspection story before
any helper or cloned variant is emitted. The first update-site slice is landed;
full decision-table and variant-routing dry-run output remains deferred.

- [x] **Print dry-run rewrite targets (update sites).** `twk ir --census --sites`
  now shows `persistent -> mutable` per vector/dict/record update candidate, plus
  an ownership verdict and `would_use` flag (true only when the base is owned and
  a mutable target exists), via `compiler.codegen.dry_run` and new update-call
  verdicts in `ownership.tw`. The dry-run path now uses typed reusable-shell flags
  from CFG facts rather than parsing verdict text.
- [x] **Render consumed vs ignored decisions.** Completed with Phase 8A's
  first-emission inspection gate. `twk ir --census --sites` now includes a separate
  post-prepare `mutable decisions` table, rendered by `compiler.codegen.mutable_audit`,
  that classifies backend selector consumption as `selected`, `policy_disabled`,
  `stale_or_ignored`, or `absent_fallback` and names the persistent, mutable, and
  emitted targets plus the proof id.
- [x] **Include variant routing dry-runs.** Closed by Phase 8G's `render_routes`
  under `twk ir --census --sites` (the `variant routes` table): one line per group
  names the generic callee, the clone func + real name, the exact canonical
  `VariantId`, caller/recursive site counts, and the verdict (`routed` /
  `fallback:over-cap` / …).

## Codegen Phase 8A — Local vector indexed-update emission ✅ done

First emitted-code change. The slice is intentionally narrow.

- [x] **Lower one local owned vector `set_at` family through the existing helper.**
  Phase 8A produces ANF-keyed decisions from optimized semantic ownership facts,
  threads them through backend preparation, and emits `vector$set_in_place` only
  when the prepared-site selector validates the exact vector-set shape under the
  Phase 8A policy. Dicts, builders, record shells, and specialization remain disabled.
- [x] **Keep interleaved reads conservative unless facts explicitly certify them.**
  Unsupported, aliased, absent, stale, and loop-contained sites fall back to the
  persistent `Vector.set_at` path; loop-contained positives are deferred to Phase 8B.
- [x] **Gate with guard programs and WAT/call inspection.** The fresh local vector
  fixture emits `rt_arr__set_in_place`; the aliasing and loop-contained fixtures
  emit `rt_arr__set`.
- [x] **Revisit deferred 7E decision rendering here.** The post-prepare audit table
  now renders selected, policy-disabled, stale/ignored, and absent fallback states,
  so helper selection is auditable from decision consumption rather than inferred
  from WAT alone.

## Codegen Phase 8B — Loop-carried vector updates ✅ done

- [x] **Extend vector indexed-update lowering to loop-carried accumulators.**
  Index-assignment sugar (`flags[k] = false`) carried through single and nested
  loops now selects `vector$set_in_place` when back-edge facts prove ownership.
  The producer no longer suppresses loop-contained candidates; a decision is
  produced whenever `reusable_shell` holds, and the analysis's entry/back-edge
  validation is the soundness source (aliased loop vectors stay persistent
  because `reusable_shell` is `false` for them). Note: explicit `.set_at(...)`
  method calls lower to a prelude call whose update sits in a borrowed-param
  body, so they are not caller-side candidates; the index-sugar form is the
  emittable shape. Self-host reaches a fixed point with the boot compiler's own
  loop-carried vector updates emitting in-place.
- [x] **Render loop proof ids near emitted decisions.** Loop-carried decisions
  carry a `phase8b-loop:<func>:carry L<base>:site L<result>:depth <n>` proof id
  and a `decision produced (loop-carried, carry L<base>)` render reason, both
  visible via `twk ir --census --sites` (e.g. the nested fixture renders
  `...:depth 2` with a `selected` / `MutableSelected` audit row).

## Codegen Phase 8C — Existing builder-region lowering ✅ first slice done (Plans 1–2)

Plans 1–2 have landed: string and vector empty-seed accumulator loops lower
end-to-end via the pure-ANF detector (`builder_region_detect.tw`), the
`BuilderRegionDecision` producer (`builder_region_produce.tw`, with the FU-1
fold-result deadness gate + non-overlap resolution + FU-2 re-fold surfacing), and
the ANF-to-ANF rewrite (`codegen/builder_region.tw`) run at the top of
`link_program` to produce ANF′. `repr_assign` erases `string$builder_from` result
slots to `OpaqueAnyref`; `--census --sites` renders a `consumed` column reflecting
actual rewrite application; self-host fixed point holds. Vector regions stay boxed
(typed routing is Plan 4). Plans 3–7 (non-empty seeds, typed routing,
conditional/`continue` folds, multi-exit, straight-line chains) remain deferred
follow-ups per the design. The Plan 2 execution plan is archived at
[../../archive/2026-07-22-8c-plan2-builder-region-rewrite.md](../../archive/2026-07-22-8c-plan2-builder-region-rewrite.md).

Builder lowering has a different region shape from indexed update and should not
be bundled with it. Current boot hooks cover vector builders and string concat
builders; typed vector builder shims (`i64`/`bool`) are implementation details of
the vector family. Full design (forks, safety fact, ANF shape, plan split):
[builder-region-design.md](builder-region-design.md). Slice notes:
[vector-lowering.md](vector-lowering.md) and [string-lowering.md](string-lowering.md).

**First-slice narrowing (per builder-region-design.md, do not implement the broad
shape).** The first slice is loop accumulators only, gated by an analysis
region-safety fact and consumed as a `BuilderRegionDecision` record (not a per-local
boolean). Vector is restricted to **empty-seed `builder_new`**; these regions stay **boxed**
(correct, not typed — the frozen accumulator is `AAssign`-rebound, which
`route_typed_vec` does not follow, so typed routing is a deferred follow-up).
Non-empty `from(base)`/`builder_from` is deferred. Folds are **unconditional only**
(conditional/`continue` deferred). Regions use a **single post-loop freeze** and
**reject every intra-region publication/early-exit** (`return`, value-carrying
`break`, `try`, closure capture, escaping call); per-exit freeze insertion is a later
slice.

- [x] **Lower existing vector builder regions from facts.** Emits `vector$builder_new` →
  `builder_push` → `builder_freeze` (empty seed only); never `vector$builder_from`. Boxed.
- [x] **Lower existing string builder regions from facts.** Reuses
  `string$builder_from/extend/freeze` for `String.concat` empty-seed accumulator loops.
- [x] **Preserve semantic builder uses.** `collect` produces no builder-region decision
  (guarded by the collect-free fixture); its semantic lowering is untouched.
- [x] **Keep unregistered builder helpers out of scope until cataloged.** The rewrite emits
  only the cataloged `builder_new`/`builder_from`/`builder_push`/`builder_extend`/`builder_freeze`
  helpers.

## Codegen Phase 8D — Dict set emission ✅ done

The decision producer is now family-neutral (`produce_update_call_decisions`,
collecting vector/dict set and remove via `is_supported_call_family`), and
`enabled_emit_policy` is the cumulative build policy that turns on the families
the compiler actually emits mutably.

- [x] **Lower proven-owned `Dict.set` through existing in-place helpers.** Owned
  `Dict.set` sites select `dict$set_in_place`; a round-trip fixture confirms key
  lookup, key-update value replacement, insertion order, and old-version
  observability are preserved (aliased dicts stay persistent).
- [x] **Keep nested value ownership conservative.** `dict$set_in_place` mutates
  only the HAMT backing, never reference-typed values stored inside the dict.
- [x] **Inspect dict helper selection.** `twk ir --census --sites` shows
  `selected` / `dict$set_in_place` for owned sites and persistent / `absent_fallback`
  for aliased ones. Self-host reaches a fixed point with the boot compiler's own
  owned dict sets emitting in-place.

## Codegen Phase 8E — Dict remove emission ✅ done

A one-flag policy extension of 8D: the producer already collected `DictRemove`
candidates, so `enabled_emit_policy` just flips `emit_dict_remove`.

- [x] **Catalog and verify remove helper semantics first.** A round-trip fixture
  confirms `dict$remove_in_place` preserves insertion order among survivors, treats
  absent-key removal as a no-op, and matches persistent `dict$remove` output.
- [x] **Lower proven-owned `Dict.remove` through existing in-place helpers.** Owned
  removes select `dict$remove_in_place`; aliased/absent/stale removes fall back to
  persistent `dict$remove`. Self-host reaches a fixed point with the boot compiler's
  own owned dict removes emitting in-place.
- [x] **Keep remove inspection distinct from set.** `twk ir --census --sites`
  reports the `dict_remove` family and `dict$remove_in_place` selection separately
  from `dict_set`.

## Codegen Phase 8F — Record shell update emission ✅ done

Local, non-variant record shell updates now emit `struct.set` shell reuse for
proven-owned sites, mirroring the 8A/8D/8E call families. A new record-shell
candidate walk in `mutable_produce.tw` (`record_update_candidates` →
`produce_record_update_decisions_with_artifacts`) joins `ARecordUpdate` sites
with the `reusable_shell` verdict already produced by `ownership.tw`
(`shell_reusable(base, last)`, which requires both uniqueness and last use). The
combined `produce_mutable_decisions` producer computes one ownership-artifact set
over the union of call and record candidate roots and merges both decision tables
(call and record decisions live at disjoint sites). `enabled_emit_policy` now sets
`emit_record_shell_update`, and `select_record_update_with_policy` flips
`emit_can_reuse` to `true` (reason `MutableSelected`) for a valid decision under
that policy; absent/ambiguous/wrong-family/mismatched/aliased sites fall back to
the persistent `struct.new` copy. Self-host reaches a fixed point (stage3 ==
stage4) with the boot compiler's own owned record updates emitting `struct.set`.

- [x] **Lower simple record shell updates from facts.** Reuse record shells only
  when CFG facts explicitly permit shell reuse (`reusable_shell`). Owned fresh and
  aliased fixtures gate `struct.set` vs `struct.new` via `link_program` WAT tests.
- [x] **Keep shell reuse separate from deep field mutation.** `emit_record_update`'s
  reuse branch emits only `struct.set` on the record shell; it never mutates
  reference-typed field storage. Field-backed collection updates are Phase 8H.
- [x] **Preserve variant qualification.** The producer only records purely local
  decisions (`variant_key: .None`); no shell reuse is emitted from a variant-qualified
  body. Cloning/routing owned variants remains Phase 8G.

## Codegen Phase 8G — Ownership-specialized function variants and call-site routing ✅ done

Landed as `boot/compiler/codegen/variant_specialize.tw`, an ANF→ANF pass in
`link_program` (between the builder-region rewrite and closure conversion), gated by
`TWINKLE_VARIANT_SPECIALIZE` (default on). It turns Phase 6's printed specialization
story into real functions, realizing [worked-examples Case V](../analysis/worked-examples.md#case-v--graph_sccvisit-the-whole-compiler-idiom-real):
the generic recursive body stays conservative (`p0=Published`), while an
owned-seeded clone supplies the in-place decisions for owned callers and recursion.

- [x] **Clone functions by exact `VariantId`.** `specialize_module` builds the
  summary + variant tables, finds callers whose argument uniqueness satisfies a
  published variant (`call_uniques_sited` + `select_variant_for_args`), and
  physically clones the callee under an owned entry seed. The generic function
  remains the persistent fallback.
- [x] **Route call sites.** Satisfying caller sites retarget to the clone; every
  other site keeps the generic callee. Emission is unchanged — the clone is an
  ordinary function whose owned seed makes the live 8A–8F seeded producer emit
  `Site`-keyed in-place decisions at its own (disjoint) sites.
- [x] **Tie recursion through the same key (self + independently-cloned peers).** A
  clone's in-SCC recursive calls that are satisfied under the owned seed retarget to
  a peer clone **that was independently cloned** — for a single recursive function
  this is the self-call, so recursion is specialized end to end (`go → clone → clone
  …`), all in-place. **Not yet:** a peer demanded only from inside another clone is
  not created on demand; its recursive call stays generic (sound, just unspecialized).
  Full SCC closure of mutual recursion is a follow-up (no mutual-recursion fixture
  exercises it today).
- [x] **Keep variant count capped and inspectable.** `variant_cap()` (default 4)
  bounds clones per generic function. `render_routes` prints one line per route
  (generic → clone name, canonical variant key, caller/recursive site counts,
  accept/fallback verdict, proof id) under `twk ir --census --sites`.
- [x] **Closed the deferred 7E variant-routing dry-run** via `render_routes`.

**Design note (perf-driven).** There is no separate seeded *decision gate*:
`link_program` already runs the seeded producer over the specialized module and is
the in-place decision authority, so a cheap AST-level structural filter
(`updatable_funcs` — callee has a supported update site) replaces it, and a clone
that yields no decision merely emits the persistent path. Skipping the discarded
whole-program `analyze`, removing the redundant gate, and pre-filtering the caller
scan to callers of published callees kept the pass at ~12s on `boot/main.tw`
(down from ~38s in the first cut). Self-host reaches a fixed point (stage3 ==
stage4) with the boot compiler's own owned recursive calls emitting in-place.

**Why the lever is recursion (empirically established).** A **non-recursive**
owned-param function (`fn setb(s: S, v) { s.b = v; s }`) already gets its in-place
win from `uniform_entry_seeds` on the *generic* body — probing `--census --sites`
shows `shell=reuse(unique)` / `would_use: true` with no clone. Cloning it adds
nothing. 8G only changes the outcome where the generic body is forced conservative
**despite** ownability — i.e. **recursion**: a recursive function's summary stays
`p0=Published`, so uniform seeds do not apply, and only the owned *variant* seed on
a clone unlocks in-place. All 8G fixtures use the recursive shape for that reason.

**Scope-outs (analysis-track follow-ups, not codegen).** `compute_variants`
publishes variants only for **user** functions whose owned param threads to the
return, so two shapes are out of 8G's reach today and were left for the analysis
track: (a) **non-recursive user vector wrappers** (`fn bump(xs: Vector<Int>, i) {
xs[i]=0; xs }`) do not publish a variant; and (b) **prelude-wrapper calls**
(`flags = .set_at(...)`) call a prelude helper, which is never a user function.
Collections behind a **record field** (`s.xs[i]=v`) render `field=persistent
(insufficient deep ownership)` and belong to **Phase 8H**, not 8G.

## Codegen Phase 8H — Record-backed field collection updates — DONE

This phase composes the dict/vector and record-shell slices for the compiler's
common quartet (`record_get` → collection update → `record_update`). It lands the
first slice: direct, ANF-visible field-backed updates (`env.types[k] = v`,
`st.xs[i] = v`) for both local and recursive-clone functions.

- [x] **Lower field-backed dict/vector updates only with explicit field-path
  decisions.** `field_backing_reusable` is a **structured** verdict bit (not parsed
  from verdict text); the produced call decision names the record shell, projected
  collection, update result, write-back, persistent fallback, mutable target, and a
  `phase8h:` proof id. Emitted helpers are ordinary vector/dict call swaps with a
  non-empty `field_path_key` audit field (`twk ir --census --sites` shows
  `record_backed_<family>`).
- [x] **Allow shell-only wins without deep field wins.** Variant publication is
  **dual-tier**: a shell variant (projected to shell-only paths, preserves 8G shell
  reuse and composes through delegation) plus a full variant (exact reference-typed
  field paths). A shell-unique caller keeps shell reuse even when the deep field
  update stays persistent.
- [x] **Preserve sibling and nested-value safety.** A shared sibling field does not
  block an owned updated field; a shell reuse never mutates a shared field backing.
  Field-path seeds come from the exact canonical `VariantId` (a unique record shell
  never implies unique reference-typed field storage); the reference-typed field
  table (`OptimizerSemantics.ref_fields`, built from the resolver) keeps primitive
  fields out of variant requirements.

**Deferred to a follow-up slice:**

- **`Set<K>` wrappers and transported `out.ctx`/`out.state` records** are recognized
  as record-backed dict quartets (`twk ir --census --sites` shows `record_backed_dict`),
  but their in-place *emission* is not yet achieved: `Set` needs the prelude
  `Set.insert` specialized for a unique receiver, and the transport shape needs
  path-level consume-dead across a multi-level record-field projection (a sibling
  `out.tag` read keeps `out` live). The first-slice detector requires whole-argument
  last-use and the quartet in one straight-line `Let` chain.
- **Field-tier recursive self-routing.** A recursive field clone (`visit$v` for
  `[unique:p0,p0.f0]`) emits its own field-backed collection update in-place, but its
  in-SCC recursive call currently stays on the generic function: `recursive_routes_for`
  re-analyzes the clone under a shell-only seed, so the recursive call proves only the
  shell tier. Field-aware recursive-route seeding (threading `field_seed_for_variant`
  through `call_uniques_sited` and the ownership fixpoint) is the follow-up; the
  emitted program stays correct (later iterations use the persistent path).

## Codegen Phase 8I — Codegen-track verification gate

- [ ] **Run correctness and aliasing guards.** The negative-aliasing suite must
  remain persistent/correct; positive anchors should lower only where proved.
- [ ] **Inspect emitted helper calls.** Use WAT/call inspection to verify chosen
  helper families rather than relying on timing.
- [ ] **Run performance only as an end-of-track signal.** Full AWFY comparisons
  wait until vector/dict/record paths are end-to-end enough to be meaningful.

## Deferrals after codegen

| Deferred work | Home |
|---|---|
| Private mutation-enabled collection storage: scoped mutable regions, owned-specialized mutable ABI, typed/dense vector targets, and mutable/transient dict storage | [../storage/README.md](../storage/README.md) |
| Compiler-private `begin`/`read`/`write`/`freeze` intrinsic family | [../migration/mutable-intrinsics.md](../migration/mutable-intrinsics.md) |
| Migrating current hooks behind the intrinsic layer | [../migration/README.md](../migration/README.md) |
| Removing remaining split-brain mutability decisions | [../migration/README.md](../migration/README.md) |
| Buffer retirement policy, after the storage performance gate shows ordinary code no longer needs Buffer as the local-update workaround | [../migration/buffer-cleanup.md](../migration/buffer-cleanup.md) |
