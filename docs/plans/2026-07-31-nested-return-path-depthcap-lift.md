# Nested Return-Path Ownership (depth-cap lift) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (inline) task-by-task. Heavy verification (full boot suite, `make stage2`) runs **sequentially, never backgrounded**. **Task 1 is a validating spike — do it before any polish; if it fails to flip end-to-end, STOP and re-report (three prior spikes each falsified an assumption).**

**Goal:** Make the transported-record dict update `next := out.ctx; next.types[k] = v` (`field_transport_ctx`, and the real `cfg.tw` `BuildCtx`/`BuildExprOut` idiom) emit `rt_dict__set_in_place`, by lifting the ownership depth cap so a **two-level field path** (`out.ctx.types` = `[Field, Field]`) can be represented, **returned across a call boundary**, and projected back to a proven-owned collection.

**Architecture:** The depth cap is enforced at four coordinated layers, all of which must lift together. Three were validated by spikes (see "Spike-validated inputs"); the fourth — the **interprocedural return-path fact** — is the new work and the true blocker:
1. `field_facts.tw` codec — `[Field, Field]` is not encoded. *(spike-validated fix: add a disjoint key range.)*
2. `ownership.tw` `graft`/`graft_path_prov` — a `[Field]` inner under a `[Field]` prefix is dropped. *(spike-validated fix: represent it.)*
3. `ownership.tw` transport projection licensing — the multi-block `next := out.ctx` isn't licensed. *(spike-validated fix: field-path cross-block consume-dead; conditions 2–5; sound, negatives reject.)*
4. **`ReturnPathOwn` / `ret_paths` (the blocker this plan is really about).** `ReturnPathOwn.field` is a single `Int?` (`ownership.tw:76`), the construction only emits **single-segment** `.Field` paths (`ownership.tw:8529` gates `p.segs.len() == 1`), and `record_ret_path` writes **exactly one key** (`ownership.tw:~4144`). So `pass`'s summary records `out.ctx = from p0` but has **no way to say** `out.ctx.types = from p0.types`. The deep field never crosses the `pass` call, so `next.types` is never owned — even with layers 1–3 fixed (proven by spike #2).

> **Line-ref note:** the refs in this plan drifted against the current file — `record_ret_path` is at ~4144 (not 4202) and `graft_path_prov` at ~4223 (not 4242). Function names are correct; navigate by name.

**Tech stack:** Boot compiler (`.tw`, self-hosted). Core: `boot/compiler/ownership.tw`, `boot/compiler/field_facts.tw`, `boot/compiler/summary.tw`. Fixtures: `boot/tests/fixtures/cfg/sound_uniqueness/`. Tests: `boot/tests/suites/field_backed_collection_suite.tw`. Verify: `TWK_TEST_FILTER=… target/twk run boot/tests/main.tw`, then `make stage2`.

---

## Spike-validated inputs (do NOT re-derive; reuse verbatim)

Three spikes (2026-07-31, all reverted) established:

- **S-a — codec `[Field,Field]` (field_facts.tw).** Add `fn ff2_base() Int { 4194312 }` (= `8 + 1048576*4`, above the single-field key range). `path_key`: `segs.len()==2` `.Field(f) → .Field(f2) => ff2_base() + f*1048576 + f2` (guard both `< 1048576`). `path_of_key`: add `k >= ff2_base() => [Field(rel/1048576), Field(rel%1048576)]` **before** the `k >= 8` arm. `graft`: under a `.Field` prefix, add `.Field(_) => fm.paths[path_key([prefix, inner.segs[0]])] = v`. Positive, disjoint from single-field (`< base`) and payload (`< 0`). Round-trips.
- **S-b — path_prov lockstep (ownership.tw `graft_path_prov`, ~4242).** In the `.Field(_) => case inner.segs[0]` arm add `.Field(_) => true` so `[Field,Field]` is represented (not folded up). Publication stays sound: `publish_local` leaks every `path_prov` entry's origins. `project_path_prov`/`remove_prefix_pp` are generic (`segs[1]`/`seg_eq`) and need no change.
- **S-c — transport licensing (ownership.tw `transport_reason` + threading).** Replace the whole-local live-out tail with: loop guard `live_contains_int(blk.entry.live, out)`; `out_crosses_block_boundary(blocks, out)`; `out_field_read_count(blocks, out, fid) == 1`; `out_field_dead_other_blocks(blocks, blk.id.id, out, fid)`. Thread `blocks: Vector<CfgBlock>` via **both** paths: `block_prep`(5 sites, real `blocks` only at `ownership_stage` ~7401; `[]` elsewhere — sound) → `recognize_transport_moves` → `transport_ok`; and `block_verdicts` → `transport_verdict`. **Empirically sound**: all five adversarial negatives (loop, re-read before/after, escape, two-projection) stayed persistent; `field_transport_ctx` shell flipped to `reuse(unique)`.

**With S-a+S-b+S-c applied, the dict still stays persistent** (`field=persistent(insufficient deep ownership)`) — because layer 4 is missing. This plan adds layer 4.

---

## The layer-4 design: nested return-path facts

### Representation (`ownership.tw:76`)

`ReturnPathOwn = .{ via: RetVia, field: Int?, own: ReturnOwn }` → add a second, nested field slot:

```tw
pub type ReturnPathOwn = .{ via: RetVia, field: Int?, field2: Int?, own: ReturnOwn }
```

`field2` is `.Some(f2)` only when `via = .Direct`, `field = .Some(f1)`, denoting the return path `[.Field(f1), .Field(f2)]` (i.e. `ret.f1.f2`). This caps nested return paths at depth 2 (`out.ctx.types`), matching the codec; deeper stays dropped (sound under-claim). A full `AccessPath` field is the more general alternative — **deferred**; `field2` is the minimal shape that unblocks the transport idiom and keeps the codec/meet bounded.

> **Why not a Vector/AccessPath now:** every helper that touches `.field` (sort key, equality, render, and the two payload/direct cases) must handle the new shape; `field2: Int?` keeps each a one-line addition and the fixpoint-equality total order simple. Revisit if a depth-3 return idiom appears.

### The coordinated touch points

> Two touch points were added in review (2026-07-31): **5b** (prefix-aware `drop_aliased_param_paths`) and the ownership.tw half of **8** (`ret_via_field_eq` + `meet_ret_paths_tagged`). Both are load-bearing: without 5b the flip regresses the existing shell win; without the ownership.tw half of 8 the cross-site meet silently conflates `ret.ctx` and `ret.ctx.types`.

| # | Site | Change |
|---|---|---|
| 1 | `field_facts.tw` codec + graft | S-a (verbatim) |
| 2 | `ownership.tw` `graft_path_prov` | S-b (verbatim) |
| 3 | `ownership.tw` transport licensing + threading | S-c (verbatim) |
| 4 | `ownership.tw:76` `ReturnPathOwn` | add `field2: Int?` |
| 5 | `ownership.tw:8515–8558` ret_paths construction | for a returned-atom path `[.Field(f1), .Field(f2)]` (currently skipped by the `segs.len()==1` gate at 8529), emit `ReturnPathOwn.{ via: .Direct, field: .Some(f1), field2: .Some(f2), own }` via `classify_path_own`. Single-segment paths set `field2: .None`. **`drop_aliased_param_paths` needs a PREFIX-AWARE fix — see touch 5b; the naïve reading is wrong and REGRESSES the existing shell win.** |
| 5b | `ownership.tw:7476` `drop_aliased_param_paths` | **[HIGH — do not skip]** This function counts by **param origin `k`** (`r.own = OwnedFromParam(k)` → `counts[k]`), NOT by field. For `Out.{ ctx, tag }`, BOTH `ret.ctx` (=p0) and `ret.ctx.types` (=p0.types) classify as `OwnedFromParam(0)`, so `counts[0]` becomes 2 and **both get dropped** — this kills the new nested claim AND regresses the existing `transport_wrapper_single` shell win (which today survives only because the codec drops the nested path, leaving `counts[0]==1`). `ret.ctx` and `ret.ctx.types` are in a **prefix/containment** relationship (owning `ctx` transitively includes `ctx.types`), not the two-disjoint-fields aliasing hazard this guard exists for (`Out.{ a: p, b: p }`). Make the count **prefix-aware**: a path and its own ancestor (same `via`, `field` a prefix of the other's `(field, field2)`) sharing a param origin is containment → keep both; only count genuinely disjoint sibling paths sharing an origin as aliased. Do **not** switch the key to full `(via, field, field2)` identity — that makes two disjoint fields each distinct and stops dropping the real aliasing case (**unsound**). Lock with a `Out.{ a: p, b: p }` negative (both drop) AND a `Out.{ ctx, tag }` positive (both survive). |
| 6 | `ownership.tw:~4144` `record_ret_path` | when `via=.Direct` and `field2=.Some(f2)`, build `ap = [.Field(f1), .Field(f2)]` (codec now supports it) and write that one key; else unchanged. |
| 7 | `ownership.tw:~4100–4130` call application | unchanged in shape — it already loops `eff_ret_paths` and calls `record_ret_path`; the deep key now lands in `result_fields`/`result_pp`, so `out` gains `[.ctx.types]:Unique`. **Verify** the origins (`prov_of(pre_prov, args[k])`) are the right provenance for the nested path (they are the arg's shell origins; sound). |
| 8 | `summary.tw` **and** `ownership.tw` comparators | **summary.tw:** `field_key` (sort, ~200 — reuse for `field2`), `cmp_ret_path` (~214), `ret_paths_eq` (~242), `render_ret_paths` (~1664): fold `field2` into the total order / equality / render so the fixpoint converges and dedup is stable. `via_kind/tag/idx` unchanged. **ownership.tw (DO NOT MISS — these drive the meet, not summary.tw):** `ret_via_field_eq` (~7447) compares only `via`+`field` and is **silent** (adding `field2` to the type does NOT force a change) — it must also compare `field2`, else `ret.ctx` (field2=None) and `ret.ctx.types` (field2=Some) collapse to one meet key and `find_ret_own`/`contains_via_field` return the wrong `own`. `meet_ret_paths_tagged` (~7610) rebuilds `ReturnPathOwn.{ via, field, own }` — it MUST carry `field2: cand.field2` (a compile error will force a touch here; do not silence it with `.None`, which decays the nested path). |

### Soundness obligations (review focus)

- **Meet across return sites** (construction, ~8514): a nested path survives only if owned at *every* return — the existing per-key meet over `path_prov` already does this once `[Field,Field]` keys exist; confirm the meet iterates the new keys (it iterates `sorted_dict_keys(pp)`, so yes). **But the cross-site meet (`meet_ret_paths_tagged`) keys sites via `ret_via_field_eq`, which ignores `field2` until touch 8 fixes it — without that fix the meet conflates `ret.ctx` and `ret.ctx.types`.**
- **Aliased-param drop is the load-bearing subtlety** (touch 5b): the genuine hazard is two *disjoint* fields backed by the SAME param (`Out.{ a: p, b: p }`) — neither may own it, both drop. A path and its own *prefix* backed by the same param (`ret.ctx` and `ret.ctx.types`, both from p0) is **containment, not aliasing** — both must survive. The current `drop_aliased_param_paths` counts by param origin and cannot tell these apart, so it wrongly drops the containment pair (killing the flip AND regressing the shell win). The fix must be prefix-aware, NOT full-identity keyed (full-identity would stop dropping the disjoint case — unsound). See touch 5b for the exact rule and the two fixtures that lock it.
- **Param nested-field seeding (VERIFY IN SPIKE — could be the real blocker):** for `out.ctx.types` to classify as `OwnedFromParam(0)`, param `ctx` must carry a `[.types]` entry in `path_prov`/`field_own` that survives to `pass`'s return; `classify_path_own` returns `.None` on an absent key. If params seed only the shell (or shallow) field ownership, the nested key never acquires a param origin and NOTHING is emitted — a distinct failure mode from the codec/representation story. Confirm params are seeded with depth-1 field ownership before concluding the codec is the last gap.
- **Second ret_paths consumer (VERIFY):** `summary.tw`'s `mark_ret_path_field` (~757) → `mark_derived_field` → `scan_param_threading` (candidate-variant SEED) is a separate, depth-1 consumer of ret_paths. Likely fine for the target fixture (`go` has no params; `ctx` is fresh) and for the real cross-param idiom (the coarse shell seed fires via whole-value derivation of `next`), but the spike must confirm the candidate seed still fires for the param-crossing `BuildCtx`/`BuildExprOut` case; if it does not, the depth-1 seed path needs a parallel lift.
- **Publication** (S-b): representing `[Field,Field]` in `path_prov` (not folding up) keeps publication sound because `publish_local` leaks each entry's origins — confirm the nested key's origins are the param's, so publishing `out` still marks `p0` Shared.
- **No over-claim on depth-3**: a returned `[.Field, .Field, .Field]` path stays dropped (construction only emits len-1 and len-2). Sound under-claim.
- **Codec disjointness invariant** (S-a): `ff2_base()` is disjoint from single-field keys `8 + f*4 + {0,1,2}` only while `f < 1048576`. S-a guards `f, f2 < 1048576` in the two-field arm but the existing single-field arm (`field_facts.tw` `path_key`) is unbounded — add an assert or a documented invariant so the disjointness claim is actually enforced, not merely assumed.

---

## Task 1 — VALIDATING SPIKE (do first; STOP if it fails)

**Goal:** prove touches 1–8 together flip `field_transport_ctx` to `rt_dict__set_in_place`, keep the five negatives persistent, and self-host. Throwaway branch; revert after.

- [ ] **Step 1:** `git checkout -b spike-nested-retpath`.
- [ ] **Step 2:** Apply S-a, S-b, S-c verbatim (from "Spike-validated inputs").
- [ ] **Step 3:** Apply touches 4, 5, **5b**, 6, 8 (add `field2`; emit the depth-2 ret_path; **prefix-aware `drop_aliased_param_paths`**; build the deep key; fold `field2` into BOTH the summary.tw comparators AND the ownership.tw meet comparators `ret_via_field_eq`/`meet_ret_paths_tagged`). Before assuming the codec is the last gap, **confirm param nested-field seeding** (Soundness obligations) — if `out.ctx.types` classifies as `.None`, the blocker is seeding, not representation.
- [ ] **Step 4:** Recreate the 5 negative fixtures + the positive/negative WAT assertions in `field_backed_collection_suite.tw` (see this plan's git history / the transport plan Task 4 for exact fixture bodies).
- [ ] **Step 5:** `TWK_TEST_FILTER=field_backed target/twk run boot/tests/main.tw`. **Expected: the positive now emits `rt_dict__set_in_place` (test passes) AND all five negatives stay persistent.** If the positive still shows `field=persistent(insufficient deep ownership)`, dump the census (`ir_sites_text`) and the `--cfg` transport verdict, find the next uncaught layer, and STOP + report — do not keep patching.
- [ ] **Step 6 (only if Step 5 flips):** `make stage2` → `Fixed point reached`. Full boot suite green.
- [ ] **Step 7:** Record the result in this plan; `git checkout -- .` and delete the branch. If validated, proceed to the polished implementation (Tasks 2+); if not, the finding reframes scope again.

## Task 2+ — Polished implementation (only after Task 1 validates)

TDD each touch point as its own RED→GREEN→self-host slice, in dependency order: (2) codec + graft with a `field_facts` round-trip unit test for `[Field,Field]`; (3) `graft_path_prov` lockstep; (4) `ReturnPathOwn.field2` + summary helpers + `drop_aliased_param_paths` (unit tests on sort/eq/render); (5) construction (a summary-level test that `pass` publishes `ret.ctx.types = from p0`); (6) `record_ret_path` deep key; (7) transport licensing (S-c) with the full negative battery from the transport plan; (8) the end-to-end `field_transport_ctx` flip + the `cfg.tw` real-idiom census delta. Each slice: `fmt` + `lint` + filtered suite + (at integration points) `make stage2`. Final: full suite + self-host + a census count of how many real `cfg.tw` transport updates now flip.

## Task 3 — Docs + commit

Update `docs/plans/sound-uniqueness/README.md`, `codegen/README.md` (8H transport → landed), and the deferred-item list. Branch `nested-return-path-depthcap-lift`; commit message describes the four-layer lift and the depth-2 nested-return-path representation.

---

## Risks / open questions for review

- **`field2` vs full `AccessPath`.** `field2` caps at depth-2 return paths. If the boot compiler has depth-3 transport idioms (`out.a.b.c`), they stay persistent (sound). Confirm depth-2 covers the real `cfg.tw` `BuildExprOut` cases (spike census will show).
- **Fixpoint cost.** More ret_path keys per function → larger summaries → possibly slower `summarize`. Measure with `TWINKLE_TIMINGS=1`; the meet is still O(keys).
- **Variant-payload interaction.** This plan extends only the `.Direct` nested case. `.Variant(tag,i)` payload fields stay single-segment; a nested payload-field (`ret.Variant.f.g`) is out of scope (dropped, sound).
- **Publication origin fidelity.** Touch 7 assumes the nested path's origins are the arg's shell provenance. Verify against a fixture where `out` is published after the nested update — it must still mark `p0` Shared (no in-place licensed).
