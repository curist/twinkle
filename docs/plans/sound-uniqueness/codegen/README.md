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
proven-owned sites. **Current focus: Phase 8C, existing builder-region lowering**
(a distinct region-shaped lowering), then records (8F), function variants (8G), and
record-backed field collections (8H). The remaining 7E dry-run item is not
abandoned: variant-routing dry-runs are an 8G
gate when specialized clone routing becomes real. The full analysis track is
complete through Phase 6 (record/field ownership,
transport-wrapper / `Result`-payload return-path summaries, ownership-specialization
decision facts, and recursive SCC variant-qualified diagnostics), so codegen consumes
a trustworthy fact set rather than rediscovering ownership. (These "Codegen Phase
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
- [ ] **Include variant routing dry-runs.** Deferred to **Phase 8G's clone-routing
  inspection gate**, not skipped. Current `--cfg` call diagnostics render only the
  accepted `-> f<id>[unique:...]` shape, not the full generic callee, would-be cloned
  callee, exact `VariantId`, route site, and fallback reason. When 8G introduces
  ownership-specialized function variants, add dry-run/inspection output before or
  alongside real routing so clone selection and generic fallback remain auditable.

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

## Codegen Phase 8C — Existing builder-region lowering

Builder lowering has a different region shape from indexed update and should not
be bundled with it. Current boot hooks cover vector builders and string concat
builders; typed vector builder shims (`i64`/`bool`) are implementation details of
the vector family. Details: [vector-lowering.md](vector-lowering.md) and
[string-lowering.md](string-lowering.md).

- [ ] **Lower existing vector builder regions from facts.** Reuse current
  `vector$builder_new/from/push/freeze` hooks; do not redesign builder/runtime
  representation yet.
- [ ] **Lower existing string builder regions from facts.** Reuse current
  `string$builder_from/extend/freeze` hooks for `String.concat` accumulator loops.
- [ ] **Preserve semantic builder uses.** `collect` and any builder lowering
  required independent of optimization must keep working without an ownership
  decision.
- [ ] **Keep unregistered builder helpers out of scope until cataloged.** The
  runtime exports additional helpers such as vector `builder_extend`, but they are
  not currently a first-cut boot builtin/shim target.

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

## Codegen Phase 8F — Record shell update emission

- [ ] **Lower simple record shell updates from facts.** Reuse record shells only
  when CFG facts explicitly permit shell reuse.
- [ ] **Keep shell reuse separate from deep field mutation.** A fresh shell around
  shared fields does not prove ownership of vector/dict storage reachable through
  those fields.
- [ ] **Preserve variant qualification.** If shell reuse is proven only in a
  `variant fn ... [unique:...]` diagnostic body, emit it only in the cloned
  ownership-specialized variant, never in the generic function body.

## Codegen Phase 8G — Ownership-specialized function variants and call-site routing

This phase turns Phase 6's printed specialization story into real functions. It is
required for [worked-examples Case V](../analysis/worked-examples.md#case-v--graph_sccvisit-the-whole-compiler-idiom-real):
`graph_scc.visit`'s generic body stays conservative, while `variant fn visit
[unique:p0]` supplies the shell-reuse decisions for owned callers and recursive calls.

- [ ] **Clone functions by exact `VariantId`.** Generate ownership-specialized
  variants only for accepted, reachable keys; keep the original function as the
  persistent/generic fallback.
- [ ] **Route call sites from decision records.** A caller with a live owned
  decision calls the matching clone; absent, stale, over-cap, unsupported, or
  ambiguous decisions call the generic function.
- [ ] **Tie recursive and mutual-recursive calls through the same key.** Inside a
  cloned SCC member, recursive calls that the variant analysis proved reachable
  route to the matching clone/peer clone, not back to the generic summary by
  accident.
- [ ] **Keep variant count capped and inspectable.** Render clone names, source
  `VariantId`, fallback reason, and proof id in `twk ir`/WAT inspection.
- [ ] **Revisit deferred 7E variant-routing dry-runs here.** Before or alongside
  real clone routing, render route-site dry-runs that name the generic callee,
  would-be specialized callee, exact `VariantId`, accepted/rejected reason, and
  persistent fallback path.

## Codegen Phase 8H — Record-backed field collection updates

This phase composes the dict/vector and record-shell slices for the compiler's
common quartet (`record_get` → collection update → `record_update` → `assign`). It
covers `Set<K>` wrappers, transported `out.ctx`/`out.state` records, and Case V's
`cur.indices[...]`, `cur.stack = ...`, and similar field-backed updates.

- [ ] **Lower field-backed dict/vector updates only with explicit field-path
  decisions.** The decision must name the record shell path, projected collection
  path, old-field liveness proof, persistent fallback, and mutable collection target.
- [ ] **Allow shell-only wins without deep field wins.** A cloned `visit` variant
  may reuse the `State` shell while a dict/vector field update remains persistent;
  do not require all-or-nothing lowering.
- [ ] **Preserve sibling and nested-value safety.** Reads of disjoint sibling paths
  are allowed only when licensed; ownership of a dict/vector backing is not
  ownership of reference-typed values stored inside it.

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
