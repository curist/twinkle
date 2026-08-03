# MutVec Slice 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Lower a proven-owned local `Vector<Int>` into a flat, unboxed, mutable Wasm GC representation (`MutVecI64`) across a region, materializing back to `PVecI64` only at the boundary — reproducing the Tier-0 spike's ~30× mutate speedup on a compiled program.

**Architecture:** A new default-off (`TWINKLE_MUTVEC`) ANF→ANF pass (`mutvec_region`) runs immediately before `builder_region` in `codegen.tw`. It claims owned local Int-vector regions that contain ≥1 indexed-update, fully rewrites the producer/`set`/`append`/reads into `mutvec_*` runtime ops, and relocates the freeze to a single proven boundary. Backend `repr_assign` marks the handle slots with a new `ReprKind.MutVec(ElemRepr)` physical repr. Because the flag is off during self-host, stage0 and the fixed point are untouched.

**Tech Stack:** Boot compiler (self-hosted Twinkle in `boot/`), hand-written Wasm-GC runtime IR (`boot/compiler/codegen/runtime/`), ANF backend (`boot/compiler/codegen/`, `boot/compiler/backend/`).

**Design source of truth:** `docs/plans/sound-uniqueness/storage/mutvec-slice1-design.md`. Read it before starting; this plan implements it.

---

## Altitude note (read first)

This feature spans hand-written Wasm-GC IR, an ANF dataflow pass, and backend repr assignment. Two conventions in this plan:

1. **Runtime op bodies are specified as deltas from named, verified analogue functions** (exact `file:line`). The analogue is real working code; adapt it with the stated field/instruction changes and confirm against the backend verifier (`TWINKLE_VERIFY_LEVEL=basic` dumps codegen even when the verifier rejects). This is grounding in existing code, not a placeholder.
2. **The ANF pass reuses `builder_region_detect` patterns.** That module already walks ANF for owned accumulator regions; the new pass mirrors its structure. Read `boot/compiler/builder_region_detect.tw` and `boot/compiler/codegen/builder_region.tw` in full before Phase 3.

**Rebuild loop:** `target/twk` is the compiled boot compiler. After editing any `boot/` compiler source, you MUST rebuild it before behavior changes take effect:

```bash
make bundle-cli    # rebuilds target/boot.wasm via self-host, then target/twk. Must print "Fixed point reached".
```

Use `make quick-bundle-cli` only when `target/boot.wasm` is already fresh. Each task's test steps assume you rebuild once after writing the code, before the "verify it passes" run.

**Scope guardrails (from the design):** Int-only; region needs ≥1 indexed-update; single freezable exit; persistent fallback for everything else; `builder_region` and `set_in_place` stay byte-identical (guaranteed because MutVec runs first and fully rewrites claimed regions, so no builder-visible shape survives — no exclusion parameter is added).

---

## Grounding corrections (verified against code, 2026-08-01)

The task bodies below were drafted against a few stale/incorrect assumptions. These were checked against the tree; **where a task conflicts with this section, this section wins.**

1. **The ops are codegen-internal — NOT source-callable (decision 2026-08-01, supersedes the plan's "test-only source visibility").** The plan proposed exposing `Vector.__mutvec_*` from the signatures prelude so Phase 1 could unit-test each op by name. That was rejected: registering them in `boot/prelude/signatures/vector.tw` makes them real methods on *every* `Vector<Int>` (LSP/autocomplete pollution) **and** a runtime footgun — calling one on an ordinary vector traps, because the ABI `ref.cast`s the argument to `MutVecI64` while a user vector is a `PVecI64`/boxed `PVec`. Follow the existing internal-op precedent instead (the typed vector builders `vector$builder_new_i64` etc.): register each with **`.None` canonical** in `builtins.tw` `builtin_specs()`, add **no** signature stub and **no** `checker.tw` arm. They are unused (hence DCE'd from every program) until the region pass emits them in Phase 4. **Consequence for Phase 1 tasks:** there is no `mutvec_runtime_suite` and no "run to see it fail / verify pass" via name-calls — Phase 1 lands the substrate (type + ops + ABI + repr) and is verified only by (a) `make bundle-cli` reaching a fixed point and (b) the ops passing the backend verifier when the region pass first emits them in Phase 4. Per-op behavioral coverage (grow-by-doubling, OOB traps on set/get, negative `make`) moves to **Phase 4/5 region + standalone trap fixtures** that drive the ops through real lowered code. Delete every Phase-1 step that writes/registers/asserts a name-callable `Vector.__mutvec_*` test.

2. **Two type layers, kept distinct.** Add a `mutvec_n()`/`mutvec_()` helper (`ref_null`/`ref_nn("rt_types__MutVecI64")`) beside `pvec_n()`/`pvec_()` in `builtins.tw`, then give each op a `builtin_abi` arm modeled on `"vector$set_in_place" => abi([pvec_n(), .I32, .Anyref], [pvec_()])`:
   - `"vector$__mutvec_new_i64" => abi([.I32], [mutvec_()])`
   - `"vector$__mutvec_make_i64" => abi([.I32, .I64], [mutvec_()])`
   - `"vector$__mutvec_push_i64" => abi([mutvec_n(), .I64], [mutvec_()])`
   - `"vector$__mutvec_set_i64" => abi([mutvec_n(), .I32, .I64], [mutvec_()])`
   - `"vector$__mutvec_get_i64" => abi([mutvec_n(), .I32], [.I64])`
   - `"vector$__mutvec_len_i64" => abi([mutvec_n()], [.I32])`
   - `"vector$__mutvec_freeze_i64" => abi([mutvec_n()], [pvec_i64_()])`
   Do **not** leave these on `empty_abi()`.

3. **(Moot after §1.)** The original worry was that a source-callable handle local would need a boxed-`anyref` slot to survive without the `MutVec(I64)` repr; empirically it did not survive (3 test failures + a `val_type_of_mono called on Never` crash), which is part of why §1 dropped source visibility. With the ops internal-only, the handle only ever exists inside a region the Phase-4 pass fully controls (producer → `mutvec_*` → freeze), and Phase 2's `ReprKind.MutVec` is what types those slots — there is no un-repr'd source-level handle to worry about.

4. **Struct constructor is positional:** `.Struct(name, [fields], supertype_opt, is_final)` — see `PVecI64` at `types.tw:49`. Task 1 Step 1's record-form literal is wrong; the corrected form is inlined in that step. Also add a `t_MUTVEC_I64 := "rt_types__MutVecI64"` constant near `t_ARRAY_I64` (`arr.tw:26`) and use it for `StructGet`/`StructSet`/`StructNew`.

5. **ReprKind exhaustive-match sites** (the `case … ReprKind` arms that need a `MutVec` arm) are: `repr_assign.tw` (`wasm_type_of_repr_cached`), `verify_common.tw` (`expected_wasm_type` + `repr_name`), `verify_expr.tw` (`repr_matches_category`), and **`codegen/emit/helpers.tw`** (a second `repr_name`) — the last is easy to miss; the plan's `verify_slots.tw` does not exist. `prepared_ir.tw` is the definition site; `repr_policy.tw` only *mentions* `TypedVec` in a comment (no arm). Task 5 Step 2's `cargo run … build` loop finds all the *compiler* sites — trust it over any hand-listed set. **But `cargo build` compiles only `boot/main.tw`, not the tests**, so it will not flag the two exhaustive `case … ReprKind` matches in `boot/tests/suites/backend_repr_suite.tw` (`assert_repr`, `assert_slot_repr`); those surface as boot-suite *compile* failures (which the dot output shows as several `x`s — use `TWK_TEST_REPORT=verbose` to see the real error). Add a `.MutVec(_)` arm there too. Add the variant at the **end** of the `ReprKind` enum (tagged-enum convention). The MutVecI64 wasm type is shared via `wasm_layout.mutvec_i64_wasm_type()` (`.Ref(true, .Named("rt_types__MutVecI64"))`), mirroring how `val_type_of_mono` builds `rt_types__PVec`. (Note: on this branch nothing serializes `ReprKind` by tag — the only `repr_cache` is an in-memory string-keyed Dict — so end-placement is convention, not a hard requirement here.)

6. **Phase 3 detection (as built).** `builder_region_detect.tw` lives at `boot/compiler/` (NOT `boot/compiler/codegen/`), and there is no `builder_region_detect` split under codegen — the region *rewrite* is `codegen/builder_region.tw`, the *detector/decision-producer* are `builder_region_detect.tw` + `codegen/builder_region_produce.tw`. The MutVec detector (`codegen/mutvec_region.tw`) reuses its `pub`-exported reference scans (`atom_is_local`, `op_references_deep`). Design facts learned by dumping real ANF (`twk ir <f> --opt`): (a) `collect`/`Vector.make` reach the source-level handle through **one `AInit` indirection** (`L_h = init L_tmp; L_tmp = freeze(...)|make(...)`), so producer classification must peek through it; (b) a `collect` over `Vector<Int>` uses the **boxed** builder freeze (`vector$builder_freeze`) unless typed-vector routing promoted it to `vector$builder_freeze_i64` — accept **both**, gated by the Int-mono check; (c) an indexed write `xs[i]=v` is `ACall(vector$set_unsafe,[H,i,v])` (index *reads* are `AIndex`). The detector **keys on `set_unsafe`** → handle `H`, then is sound-by-rejection: every use of `H` must be whitelisted (set/append/index-read/len) and exactly one terminal may carry it, which folds the plan's separate Task-6 detection and Task-7 exit-classifier into one whitelist walk. The `debug_regions` harness lives in the **test file** (calls `pipeline.compile_source` then `detect_regions`) — never in `mutvec_region.tw`, which must not import `pipeline` (that would cycle `pipeline`→`codegen`→`mutvec_region`).

7. **Append new `builtin_specs()` entries at the END of the list, never mid-list.** `builtin_specs()` order *is* the 0-based FuncId assignment, and other code depends on those ids. Inserting the seven `rt(...)` specs after `builder_freeze_i64` (as an early draft did) shifted every later builtin's FuncId and produced 3 boot-suite failures plus a `val_type_of_mono called on Never` codegen crash, while `make bundle-cli` still reached a (self-consistent) fixed point — so the self-host green light does **not** catch this; only `make boot-test` does. The file already documents the rule ("// Appended at end to preserve FuncId assignment of earlier builtins."); follow it. `builtin_abi` arms are keyed by name, so their position is irrelevant. **Always run `make boot-test` after touching `builtin_specs()`, not just `make bundle-cli`.**

---

## File Structure

**Create:**
- `boot/compiler/codegen/mutvec_region.tw` — the ANF→ANF detection + rewrite pass (`rewrite_module`), the region decision record type, the exit classifier.
- `boot/tests/suites/mutvec_region_suite.tw` — positive/negative region-detection and region-lowering fixtures. Before emit lands it exercises detection only; after emit lands it is the behavioral surface for the internal ops.
- `boot/bench/mutvec_slice1_bench.tw` — the compiled-program speedup check.

**Modify:**
- `boot/compiler/codegen/runtime/types.tw` — add the `MutVecI64` GC struct type (positional `.Struct`, after the `PVecBool` entry; see §4).
- `boot/compiler/codegen/runtime/arr.tw` — add `t_MUTVEC_I64` + `mutvec_MIN_CAP` constants and the seven `mutvec_*_i64` FuncDefs, and register them in the `module()` func list (after `family_i64().pvec_make_fn()`).
- `boot/compiler/builtins.tw` (`builtin_specs()`, `builtin_abi()`) — register the ops as **`.None`** internal builtins + their wasm ABI (see Grounding corrections §1–§2). Add the `mutvec_n()`/`mutvec_()` ValType helpers.
- `boot/compiler/backend/prepared_ir.tw:48-59` (`ReprKind`) — add `MutVec(ElemRepr)`.
- `boot/compiler/backend/repr_assign.tw` — assign/lower `MutVec(I64)` and its wasm type.
- `boot/compiler/backend/repr_policy.tw` — map the family (reuse `ElemRepr`).
- `boot/compiler/backend/verify_common.tw`, `boot/compiler/backend/verify_expr.tw` — exhaustive `ReprKind` match arms (see Grounding corrections §5).
- `boot/compiler/codegen/codegen.tw:78,131` — add `mutvec_region_enabled()` and insert the pass before `builder_region.rewrite_module`.
- `boot/tests/main.tw` — register the MutVec region suite. Do not add a source-callable MutVec runtime suite.

**Do NOT modify:** `builder_region.tw`, `boot/compiler/builder_region_detect.tw` (except to `pub`-export helpers the new pass shares), the `set_in_place` decision path, or any `src/` (stage0) — deferred to the flag-on slice.

---

## Phase 1: Runtime substrate (S3) — completed as internal-only substrate

Goal: the seven `mutvec_*_i64` runtime ops exist, are registered as codegen-internal builtins, and carry explicit backend ABI. They are **not source-callable**: no `Vector.__mutvec_*` signatures, no canonical source names, and no checker arms. Behavioral testing waits until Phase 4 emits the ops through real lowered regions.

**Files:**
- `boot/compiler/codegen/runtime/types.tw`
- `boot/compiler/codegen/runtime/arr.tw`
- `boot/compiler/builtins.tw`

- [x] Add `rt_types__MutVecI64` as a Wasm-GC struct with mutable `data: ArrayI64` and `len: i32` fields.
- [x] Add runtime ops: `mutvec_new_i64`, `mutvec_make_i64`, `mutvec_push_i64`, `mutvec_set_i64`, `mutvec_get_i64`, `mutvec_len_i64`, `mutvec_freeze_i64`.
- [x] Preserve vector semantics in the substrate: grow by doubling with a nonzero `MIN_CAP`, clamp negative `make` lengths to empty, and bounds-check `get`/`set` against logical `len`, not backing capacity.
- [x] Append internal builtin specs at the end of `builtin_specs()` with `.None` canonical names, and add explicit `builtin_abi()` arms using `rt_types__MutVecI64`.
- [x] Verify the inert substrate with self-host/boot-test gates. Because no codegen path emits these ops yet, runtime behavior is intentionally covered later by Phase 4 region fixtures rather than by source-level name calls.

**Do not add:** `boot/prelude/signatures/vector.tw` stubs, a source-callable `mutvec_runtime_suite`, or `.Some("Vector.__mutvec_…")` canonical names. Those were rejected because they expose an internal handle ABI as public vector methods and can trap when called on ordinary persistent vectors.

---

## Phase 2: Physical repr (`ReprKind.MutVec`) — completed as plumbing

Goal: the backend can describe a slot as `MutVec(I64)` and lower it to `rt_types__MutVecI64`, distinct from `TypedVec(I64)` / `PVecI64` and never `Anyref`. This phase only adds the representation category; Phase 4 is responsible for assigning it from region records.

**Files:**
- `boot/compiler/backend/prepared_ir.tw`
- `boot/compiler/backend/repr_assign.tw`
- `boot/compiler/backend/verify_common.tw`
- `boot/compiler/backend/verify_expr.tw`
- `boot/compiler/codegen/emit/helpers.tw`
- `boot/compiler/codegen/wasm_layout.tw`
- `boot/tests/suites/backend_repr_suite.tw`

- [x] Add `ReprKind.MutVec(ElemRepr)` at the end of the enum.
- [x] Add `wasm_layout.mutvec_i64_wasm_type()` and map `.MutVec(_)` to `rt_types__MutVecI64` wherever wasm slot types are derived or verified.
- [x] Add exhaustive-match arms in compiler and backend-repr tests.
- [x] Keep default `Vector<Int>` representation unchanged; no slot becomes `MutVec` until a Phase 4 region record explicitly assigns it.

---

## Phase 3: Region detection (analysis, no emit yet) — current checkpoint

Goal: `mutvec_region.detect_regions` identifies conservative, claimable Int-vector regions and returns audit records without changing emitted ANF. The detector is intentionally sound-by-rejection and remains inert until Phase 4 consumes richer records.

**Files:**
- `boot/compiler/codegen/mutvec_region.tw`
- `boot/compiler/builder_region_detect.tw` (only shared helper exports)
- `boot/tests/suites/mutvec_region_suite.tw`

### Task 6: Eligibility predicate — completed for detector-only use

- [x] Reuse `builder_region_detect` reference-scan helpers without moving the detector under `codegen/`.
- [x] Classify supported producers through the real ANF shapes: collect/builder freeze, `Vector.make`, and array literal seeds, including one `AInit` indirection from producer temp to source handle.
- [x] Gate candidates to semantic `Vector<Int>`.
- [x] Key candidate regions on at least one `vector$set_unsafe` base. Append-only regions remain builder/persistent territory.
- [x] Whitelist only in-region `set`, append, index read, and `len`; reject other uses of the handle.
- [x] Add positive detector coverage for collect, `Vector.make`, set+append, and get/len; add negative coverage for append-only, non-Int, call escape, record escape, slice, multiple exits, early return, parameter-sourced/not locally born, and closure capture.

### Task 7: Exit classifier — negative matrix + content tests done; full record deferred to Phase 4

- [x] Reject nested early returns carrying the handle, including the single-early-return shape where the top-level fallback returns a different value.
- [x] Reject `break value` in the scanner implementation when it directly carries the handle.
- [x] Add the rest of the source-expressible negative fixture matrix: aliased/not-owned handle, variant storage, dict storage, outer-vector storage, and concat. Notes on the three not added as standalone fixtures: **`break value` is not source-expressible** (`checker.tw:5158` rejects "break with a value is not supported"), so a user handle can never reach `Break(.Some)` — the scanner's rejection is defensive-only for internal desugaring; **host/import boundary** reduces to "handle passed to a call" (an `extern` is an `AGlobalFunc` callee, so a handle argument hits the same non-base-arg `reject()` as the covered call-escape fixture); **`try`/early-return arm** reduces to "a `.Return(.Some(handle))` in a non-terminal position", already covered by the `terminal_exit_ok=false` tightening plus the two `if { return xs }` fixtures.
- [x] Add tests that inspect record contents (producer class + op-site kinds), not just region counts/proof ids: `sole_region` + `producer_name`/`kind_count` assert collect→CollectSeed, `Vector.make`→MakeSeed, and the set/push/get/len op-site multiset — guarding emit against producer/op mis-classification.
- [ ] **Deferred to Phase 4 Task 9** (where the consumer exists): enrich `MutVecRegion` into the full backend-facing lifecycle record — source physical repr, materialization-exit site, post-freeze value, invalidation point, fallback behavior, validation results. These fields are what `repr_assign` keys off; building their exact encoding ahead of that consumer would be speculative (the design §4 itself defers "the concrete record type / which fields `repr_assign` keys off" to the plan's emit phase). The detector-only record (handle, producer, op_sites, proof_id) is sufficient and byte-accurate for the analysis checkpoint.

**Stop rule:** do not wire `detect_regions` into lowering while the record is detector-only. Backend emitters must consume the full lifecycle schema without re-proving exits or handle validity.

---

## Phase 4: Atomic emit + repr handoff + flag wiring

> **Phase 4 core LANDED (commits `f168f2cc` + `44d1c875`, branch `mutvec-slice1-phase4`): both producers, end-to-end.** `mutvec_region.rewrite_module` lowers claimed **MakeSeed** and **CollectSeed** regions (indexed set/append/read/len→mutvec twins; single relocated `mutvec_freeze_i64` at the return boundary). MakeSeed: producer→`mutvec_make_i64`. CollectSeed: the `builder_new`→loop `builder_push`→`builder_freeze` chain is retyped (`builder_new()`→`mutvec_new_i64(0)`, loop `builder_push`→`mutvec_push_i64`, `builder_freeze`→elided AInit passthrough so the handle *is* the mutvec; the loop's Void-typed discarded push-result binding is retyped to `Vector<Int>` so it gets a slot). `TWINKLE_MUTVEC=1` flag wired in `codegen.tw` before `builder_region`. Repr handoff via a **site-aware backend override** (`backend/mutvec_repr.assign_mutvec_reprs`, threaded into `prepare` after routing) that marks handle slots `MutVec(I64)` and the freeze result `TypedVec(I64)` by reading the already-rewritten prepared IR — the same pattern `route_typed_vec` uses for PVecI64, instead of threading a record across stage boundaries. Verified: flag-on == flag-off results for make/collect/collect+append; WAT shows `mutvec_new/push/set/get` + one `freeze`, no boxed builder on the handle path; a returned-vector mutation loop runs **~2.6× faster** than the typed `set_in_place_i64` baseline (n=4096, k=400: 21.7→8.4ms). Self-host fixed point holds; 3401 boot tests pass.
>
> **Perf note (recalibrated):** the spike's 15–35× was vs *boxed* `set_in_place`. Sub-project A's typed indexed-write (`set_in_place_i64`) landed after the spike and is now the baseline, so MutVec's real end-to-end win over the *current* path is ~2.6× (it skips PVec trie navigation for a direct flat-array write), not 15–35×.
>
> **One follow-up carried forward:** the boundary freeze result is currently **reboxed PVecI64→PVec** at the return because `phys_return` stays boxed (route doesn't recognize the mutvec'd producer) — correct but a one-time O(n) boundary cost; making the return stay PVecI64 needs the cross-fn typed-return machinery (`analyze_typed_repr`) to recognize `mutvec_freeze_i64` as a typed producer.

Goal: with `TWINKLE_MUTVEC=1`, claimed regions emit `mutvec_*` ops, keep the handle physically `MutVec(I64)` across the region, materialize exactly once at the boundary, and leave flag-off output unchanged. Rewrite, flag wiring, and repr assignment are one atomic slice: do not land a flag-on path that emits `mutvec_*` before handle slots are assigned from the region record.

**Files:**
- `boot/compiler/codegen/mutvec_region.tw`
- `boot/compiler/codegen/codegen.tw`
- backend prepare/repr plumbing that carries region records into `repr_assign`
- `boot/tests/suites/mutvec_region_suite.tw` and `boot/tests/suites/fixtures/`

### Task 8: Write failing end-to-end fixtures first

- [ ] Add a collect-born indexed-update fixture that returns a `Vector<Int>` and prints indexed results. Flag-off WAT should show the current persistent/`set_in_place` path and no `mutvec_*`.
- [ ] Add `Vector.make` and set+append fixtures.
- [ ] Add standalone trap fixtures that exercise `mutvec_set_i64`/`mutvec_get_i64` logical-length OOB only through lowered regions once emit exists.
- [ ] Add fixture coverage for negative region classes so flag-on WAT still contains no `mutvec_*` when the detector rejects.

The first flag-on WAT check should fail by absence of `mutvec_*` calls. After implementation it should show `mutvec_new_i64`/`mutvec_make_i64`, in-region ops, exactly one `mutvec_freeze_i64`, and no boxed `set_in_place` for claimed sites.

### Task 9: Complete the lifecycle record and ANF rewrite

- [ ] Extend `MutVecRegion` to the full lifecycle schema from Phase 3 before rewriting.
- [ ] Implement `rewrite_module` so a claimed region is fully lowered: producer -> `vector$__mutvec_new_i64` or `vector$__mutvec_make_i64`, append -> `vector$__mutvec_push_i64`, indexed update -> `vector$__mutvec_set_i64`, index read -> `vector$__mutvec_get_i64`, len -> `vector$__mutvec_len_i64`, boundary -> `vector$__mutvec_freeze_i64`.
- [ ] Remove or bypass the producer's immediate persistent freeze only for claimed regions, and insert the single materialization at the recorded boundary.
- [ ] Leave all unclaimed code byte-identical so `builder_region` and mutable-decision hooks see their existing shapes.

### Task 10: Thread records into backend prepare and assign `MutVec(I64)`

- [ ] Carry accepted region records alongside the rewritten ANF into backend preparation.
- [ ] In `repr_assign`, mark every live mutable handle slot named by the record as `ReprKind.MutVec(.I64)` and the post-freeze slot as `TypedVec(.I64)` / `PVecI64`. Do not re-derive ownership or exits in the backend.
- [ ] Verify WAT locals for a claimed fixture include `rt_types__MutVecI64` handle refs and the returned/published value is `PVecI64`.

### Task 11: Wire `TWINKLE_MUTVEC` after rewrite+repr are ready

- [ ] Add `mutvec_region_enabled()` in `codegen.tw` (`TWINKLE_MUTVEC=1`, default off).
- [ ] Run `mutvec_region.rewrite_module` immediately before `builder_region.rewrite_module` only when the flag is enabled.
- [ ] Confirm flag-off WAT and `--census --sites` output for existing paths are unchanged except for intentionally additive analysis rows.
- [ ] Run focused fixtures, then `make bundle-cli` and `make boot-test`.

---

## Phase 5: Census, negatives, regression, performance

### Task 12: Census region-audit rows — DONE (commit `cf1328b6`)

**Files:** `boot/commands/ir.tw` (the `--census --sites` renderer, `render_mutvec_rows`), `boot/compiler/codegen/codegen.tw` (`mutvec_region_enabled` made `pub`), `boot/tests/suites/mutvec_region_suite.tw`, `boot/tests/suites/fixtures/mutvec_indexed.tw`.

- [x] **Step 1–3:** `ir.render_mutvec_rows` renders one additive row per claimed region sourced from `mutvec_region.detect_regions`, gated behind `codegen.mutvec_region_enabled()` in `render_census_report`. Columns: `region_id | begin_site (producer@handle) | family=MutVecI64 | op_sites=[kind@site,…] | exit (freeze|scratch) | would_use | consumed`. `consumed` re-detects after `rewrite_module` (a lowered region's `set_unsafe` base is gone → "yes"). In-suite tests assert row content flag-independently (calling `render_mutvec_rows` directly) plus a gating test that the full flag-off report carries no rows.
- [x] **Step 4: Rebuild + verify.** Flag-on `--census --sites` on `mutvec_indexed.tw` shows the rows; flag-off and unclaimed fixtures show none; existing rows untouched (the block is skipped flag-off).
- [x] **Bonus (surfaced by the fixture):** fixed a latent detector dup — `collect_set_bases` deduped candidate handles through a side `Dict` that was mutated but never threaded back (Twinkle Dicts are persistent), so a handle written at ≥2 index sites was collected — and rewritten — once per write (duplicate row + redundant second boundary freeze). Dedup now scans the threaded `out` accumulator; regression test added (one region, one freeze, two sets).
- [x] **Step 5: Commit.**

### Task 13: Full negative + positive fixture matrix — DONE (commit `96d9380c`)

**Files:** `boot/tests/suites/mutvec_region_suite.tw` (ANF-level matrix, already present), `boot/tests/suites/fixtures/mutvec_producers.tw`, `boot/tests/suites/fixtures/mutvec_oob.tw`.

- [x] **Step 1: Positive fixture per producer.** `mutvec_producers.tw`: collect-freeze (`new`/`push`/`set`+1 `freeze`), `Vector.make`-freeze (`make`/`set`+1 `freeze`), collect+append (`push`×2+`set`+1 `freeze`), non-escaping scratch (`get`/`len`, **no** `freeze`). Verified flag-on WAT per fn + flag-on/off run agree (123).
- [x] **Step 2: Negative per rejected class.** The rejected-class matrix is automated at the ANF-rewrite level in the mutvec suite (aliased/not-owned, non-Int, append-only, concat/slice, multiple exits, early return, capture, aggregate-store, call/host escape). Since emit consumes the rewritten ANF, `rewritten_calls==0` is the emit guarantee; a flag-on emit spot-check confirms rejected regions produce zero `mutvec_*` calls in WAT.
- [x] **OOB traps.** `mutvec_oob.tw`: `mutvec_set_i64`/`mutvec_get_i64` bounds-check the logical length and trap (`RuntimeError: unreachable`) on an out-of-range index, identically to the persistent path flag-off.
- [x] **Step 3: Run.** `make boot-test` → 3409 pass.
- [x] **Step 4: Commit.**

### Task 14: Self-host + regression gate — DONE (verification-only, no code delta)

- [x] **Step 1: Full self-host.** `make bundle-cli` → `Fixed point reached: stage3 == stage4` with all Phase-5 compiler edits in place (census renderer + detector dedup).
- [x] **Step 2: Boot suite.** `make boot-test` → 3409 passed.
- [x] **Step 3: Rust suite (reference).** `make rust-test` → all suites ok, 0 failed (stage0 untouched; no `src/` coupling).
- [x] **Step 4: Census regression.** `target/twk ir boot/main.tw --census --sites` (flag off) shows **no** `mutvec regions:` section (boot claims 0 regions and the block is flag-gated) → existing census output unchanged.
- [x] **Step 5:** No fixture/expected-output snapshot files exist (census asserted via in-suite substrings), so nothing to commit for this verification step.

### Task 15: Performance validation (end-of-slice gate) — DONE (commit `8ddae21e`)

**Files:** `boot/bench/mutvec_slice1_bench.tw`

- [x] **Step 1: Write the bench.** collect + `xs[i]=v` loop + return, timed with `@std.date`.
- [x] **Step 2–3: Baseline vs MutVec.** Run both ways (env-gated).
- [x] **Step 4: Result — NOT a uniform win (bench corrected the plan's premise).** The baseline is now typed `set_in_place_i64`, not boxed, so the spike's 15–35× does not carry over. Measured: MutVec's O(1) write vs the baseline's O(log n) trie walk, minus a one-time O(n) freeze-trie-build. Mutation-density crossover (n=1M): k=2M → 1.5× slower; k=20M → 2.5×; k=60M → 3.9×. n=65536, k=2M → 1.7×. So it is a strong win only for mutation-heavy owned vectors and a regression otherwise.
- [x] **Step 5: Commit.** `8ddae21e`.
- [x] **Step 6: Record status.** Crossover documented in the bench header, this plan (Phase 6 decision), and memory. The "stage0 mirror" next-slice note was wrong and is removed; Phase 6 (remove flag) is blocked by this bench (keep opt-in).

```bash
git add docs/plans/sound-uniqueness/storage/README.md docs/plans/sound-uniqueness/storage/mutvec-slice1-design.md
git commit -m "docs(mutvec): record slice 1 landed (flag-gated) + next-slice note"
```

---

## Phase 6: Enable unconditionally (remove the flag) — BLOCKED by the bench

> **Decision (from Phase 5 bench, 2026-08-02): do NOT remove the flag yet.** The bench (`boot/bench/mutvec_slice1_bench.tw`) shows MutVec trades an O(1) per-write for a one-time O(n) freeze-trie-build, so it **regresses build-heavy/low-mutation returned vectors** (n=1M, k=2M: 1.5× slower) while winning big on mutation-heavy ones (k=20M: 2.5×, k=60M: 3.9×). Enabling it unconditionally would regress low-mutation-density user code, which fails this phase's own Step 5 gate. Also: boot's own source claims **0** regions, so unconditional-on gives the compiler self-build nothing anyway. Keep MutVec **opt-in (flag-gated)** until either the freeze tax is reduced (e.g. the frozen PVecI64 adopting the flat backing without a full rebuild) or the detector becomes mutation-density-aware. The mechanics below stay valid for when/if that changes.

Goal: retire `TWINKLE_MUTVEC` and run the pass always. **No stage0 mirror is needed** — the self-host fixed point is boot-compiler-only (the loop compares stage3 vs stage4, both boot-compiled; stage0's output, stage1, is never compared). stage0 (Rust) has no mutvec pass and never runs it; it only makes a functionally-correct stage1, and mutvec applies from stage2 onward. Convergence only requires the boot compiler's mutvec output to be deterministic, so `src/` stays untouched.

- [ ] **Step 1:** Delete `mutvec_region_enabled()` in `codegen.tw` and call `mutvec_region.rewrite_module` unconditionally before `builder_region`.
- [ ] **Step 2:** Remove the env read and any flag-off test scaffolding; the pass now runs when compiling boot itself, so boot's own eligible regions become mutvec.
- [ ] **Step 3: Self-host gate.** `make bundle-cli` must still print `Fixed point reached` (stage3 == stage4) — this is the real test that the pass is deterministic and correct over the whole compiler, not a handful of fixtures. If it diverges or traps, a real bug in the pass surfaced on boot's own code; diagnose before landing.
- [ ] **Step 4:** `make boot-test` + `make rust-test` green.
- [ ] **Step 5 (decision gate):** confirm the census/bench show a net win on boot's own build (or at least no regression) before removing the flag for good — MutVec's ~2.6× is over the typed `set_in_place_i64` baseline, so the whole-compiler impact depends on how many of boot's hot regions are claimed. If the net is neutral/negative, keep the flag off-by-default instead of removing it, and record why.

## Slice 1 complete — deferred work moved to a successor plan

Slice 1 is done: owned `Vector<Int>` regions lower to flat mutable `MutVecI64`
and freeze to `PVecI64`, the pass runs **unconditionally** (the `TWINKLE_MUTVEC`
flag was retired), the bulk-freeze made it a strict win at every mutation
density, and boot's own two regions lower correctly (self-host fixed point).

The deferred work is captured, with its real dependencies and value ranking, in
**[docs/plans/mutvec-later-slices.md](../mutvec-later-slices.md)**:

- `Bool` / `Float` / boxed element families — generalize the seven `mutvec_*`
  ops over `PVecFamily` (depends on [rt-arr-family-dedup.md](../rt-arr-family-dedup.md);
  Float additionally needs a typed `PVecF64`).
- S4: thaw-from-`PVec` for param-sourced owned vectors (depends on the
  sound-uniqueness track).
- Append-only-loop / `builder_region` unification (Approach A).

The interim deep-module bailout guard added while landing this slice is
superseded by [compiler-stack-safety.md](../compiler-stack-safety.md) Phase 2.
