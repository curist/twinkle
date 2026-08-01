# 8H Transport Projection-Move (field-path consume-dead) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (inline) to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Every heavy verification step (full boot suite, `make stage2`) runs **sequentially, never backgrounded**.

**Goal:** Make the transported-record field update `next := out.ctx; next.types[k] = v` (fixture `field_transport_ctx`, and the real `cfg.tw` `BuildCtx`/`BuildExprOut` idiom) emit `rt_dict__set_in_place` instead of the persistent `rt_dict__set`, without regressing any negative/aliasing guard.

> ## ⚠️ Spike findings (2026-07-31) — READ FIRST; the goal above is BLOCKED as scoped
>
> A throwaway spike implemented the licensing relaxation (conditions 2–5) end to end and hit it with the adversarial battery. Results:
>
> 1. **The soundness contract is empirically validated.** All five adversarial negatives (loop-carried, re-read before, re-read after, escape-to-call, two-projection) stayed **persistent** (`rt_dict__set`, no in-place) — the guards do not over-license. Persistent emission cannot miscompile, so conditions 2–5 are sound as implemented.
> 2. **The licensing relaxation works.** `field_transport_ctx`'s projection now MOVES: its record-update shell flipped `persistent(aliased shell)` → **`reuse(unique)`**. So the transport slice *does* deliver **record-shell reuse** (`struct.set` instead of `struct.new` copy).
> 3. **But the headline goal — the DICT `set_in_place` — does NOT fire, and the blocker is deeper than Task 6 assumed.** The verdict stays `field=persistent(insufficient deep ownership)`: `next.types` is never proven owned because the field-ownership codec **caps at depth 2 and silently drops `[Field, Field]` paths** (`boot/compiler/field_facts.tw:78, 200–206`). `out.ctx.types` is exactly `[Field, Field]`, so `out` cannot deeply own `.ctx.types`, and projecting `.ctx` into `next` carries the shell but not `.types`. This is **not** the `pass`-summary gap (Task 6) — it is the representation depth cap, a core `field_facts` limitation.
>
> **Consequence:** the licensing change (Task 2) is sound and buys transport **shell reuse only**. Getting the dict in-place additionally requires lifting the `field_facts` depth-2 cap to represent `[Field, Field]` (a change to a core, soundness-sensitive data structure with its own graft/project/publish rules) — materially larger and out of this plan's scope. **Decision required before proceeding** (see the reframed scope options at the end of this doc).
>
> ### Combined spike #2 (codec `[Field,Field]` + transport licensing) — 2026-07-31
>
> A second spike implemented BOTH the `[Field,Field]` codec extension (`field_facts.tw` `path_key`/`path_of_key`/`graft` + a disjoint key base) **and** its `path_prov` lockstep (`graft_path_prov`), on top of the transport licensing. Result:
>
> - **Still sound** (negatives reject), **shell reuse still works** — but **the dict flip STILL does not fire**: `field=persistent(insufficient deep ownership)` is unchanged.
> - **Root cause is multi-layer, deeper than the codec.** `record_ret_path` (`ownership.tw:4202`) writes **exactly one field key**, and the return-path fact type `ReturnPathOwn` (`rp.via`/`rp.field`) can express only a **single** field (`out.ctx`), never a nested field-of-field (`out.ctx.types`). So `pass`'s summary cannot record that `out.ctx.types` came from `ctx.types`; the deep field never crosses the call boundary, and `next.types` is never owned regardless of the codec.
> - **Real scope of "lift the depth cap":** the interprocedural return-path fact **representation** (`ReturnPathOwn`) + its **computation** in `summary.tw` + `record_ret_path` + the `field_facts` codec + the `path_prov` lockstep — a coordinated, soundness-critical change across the whole ownership summary pipeline, for the transport **dict** win specifically. This is a major sub-project, not a contained codec addition.
>
> **Bottom line:** the sound licensing change delivers transport shell-reuse cheaply; the dict-in-place win is gated behind a summary-system depth-cap lift that is much larger than the codec change first suggested.

**Architecture:** The transport projection-move machinery **already exists** in `boot/compiler/ownership.tw` (`transport_ok` / `transport_reason` / `recognize_transport_moves`, lines ~435–484; consumed by `transfer_op`'s `ARecordGet` arm at ~4507 and `projection_move_licensed` at ~4204). It licenses a projection `R = record_get out.f` to **move** the `out.f` subtree into `R` even while `out` stays live for *sibling* field reads **within the projection's own block**. The sole blocker for the multi-block case is that `transport_reason` rejects when `out` is **live-out of the block** (`live_contains_int(blk.exit.live, out_id)`, ownership.tw:461) — which a trailing `case … out.tag` trips because the sibling read lives in a *successor* block. This plan replaces that **whole-local** cross-block rejection with a **field-path-sensitive whole-function scan**: license the move iff, in every block other than the licensing projection, every mention of `out` is a sibling read `record_get out.g` (`g ≠ f`) and `out` never crosses a block boundary as a value (terminator / edge-arg). That is sound because `out.f` is then exclusively moved into `R` while the disjoint sibling fields stay observable.

**Tech stack:** Boot compiler (`.tw`, self-hosted). Analysis: `boot/compiler/ownership.tw`. Fixtures: `boot/tests/fixtures/cfg/sound_uniqueness/`. Tests: `boot/tests/suites/field_backed_collection_suite.tw`. Verification: `TWK_TEST_FILTER=… target/twk run boot/tests/main.tw`, then `make stage2`.

---

## Background: the exact diagnosis (verified)

`field_transport_ctx.tw`:
```tw
pub type Ctx = .{ types: Dict<Int, Int>, values: Dict<Int, Int> }
pub type Out = .{ ctx: Ctx, tag: Int }
pub fn pass(ctx: Ctx) Out { Out.{ ctx, tag: 1 } }
pub fn go() Int {
  ctx := Ctx.{ types: Dict.new(), values: Dict.new() }
  out := pass(ctx)
  next := out.ctx          // B0: L10 = record_get L2.f0  → transport=borrow(L2 live-out)
  next.types[2] = 40       // B0: dict update on next.types stays persistent (cascade)
  case next.types.get(2) { .Some(v) => v + out.tag, .None => out.tag }  // B1/B2 read out.tag (sibling f1)
}
```
`target/twk ir --cfg` shows (verified):
- `B0: verdict L10 = record_get L2.f0 transport=borrow(L2 live-out)` — the projection is rejected **only** because `out` (L2) is live-out.
- `B1: verdict L17 = record_get L2.f1 transport=move([.f1] …)` / `B2: L19 = record_get L2.f1 transport=move(…)` — the *only* post-projection uses of `out` are sibling reads of `.f1` (`out.tag`). `out.f0` (`ctx`) is read **exactly once**, at the projection.
- Successor blocks reference `out` (L2) **directly** (`record_get L2.f1`), **not** via edge-args. So a whole-function scan of `record_get out.f…` is the complete set of `out` field reads.

Because `out.f0` is read only at the projection and every other `out` use is a sibling `.f1` read, moving `out.f0` into `next` is sound. Once licensed, `transfer_op`'s `ARecordGet` arm (ownership.tw:4507–4533) gives `next` the projected subtree's field ownership (`set_field_own(result, pr.fields)`), so the downstream `next.types[2]=40` quartet then flips through the existing 8H path.

**Two-gap contingency.** The flip needs BOTH (1) the projection licensed (this plan) AND (2) `pr.shell = atom_field_own(out).project(.Field(f0)).shell` to be `.Some` — i.e. `out` deeply owns `.ctx`. Gap (2) is already produced by the transport-wrapper return-path summary (`transport_wrapper_single` proves `Out.ctx = from(p0)`); Task 2's observation step verifies it. If (2) is absent, Task 6 records a follow-up (do **not** widen soundness to compensate).

---

## Soundness contract (the load-bearing part — review this first)

> **Revised after soundness review (2026-07-31).** The first draft presented conditions 2–4 as *sufficient* for soundness. They are not: as written they over-license three shapes — a `record_get out.f` **before** the projection in the same block, an escape of `out` before the projection, and a **loop-carried** projection — each of which is only saved from miscompile by a *separate forward-state invariant* the contract never named (`remove_prefix` on any field read at ownership.tw:4531/4546; publish-on-escape at 4108–4116; the back-edge field_own **meet** at 5507/5519). The revision below (a) actually wires condition 4 as a whole-function read-count so the double-projection gap closes *at the contract*, (b) adds an explicit **loop guard** so the loop case is rejected by the contract rather than by the meet, and (c) scopes what the delta itself guarantees.

**What the delta itself soundly adds** (narrow, self-evident): licensing a projection whose only reason for being live-out is **straight-line successor sibling reads `out.g` (g ≠ f)**. A sibling read touches a disjoint subtree and can never alias `out.f`, so moving `out.f` into `R` while siblings stay observable is sound. Everything the delta must *reject* is enumerated below; the rejections lean on the forward state only where noted, and the added guards remove that dependence for the two dangerous cases.

The move `R = record_get out.f` at index `i` of block `P` is licensed iff **all** hold:
1. **Within-block, after the projection:** no non-sibling mention of `out` and no second `record_get out.f` after `i` in `P` — *the existing `transport_reason` within-block scan (ownership.tw:445–457), unchanged.*
2. **`out` never crosses a block boundary as a value:** `out_id` appears in **no** block's terminator or outgoing edge-arg list (`out_crosses_block_boundary` over `exit_mentions_local`, which covers both terminator scrutinees at ownership.tw:294–304 **and** edge args at 305–311 — confirmed by review). Prevents an aliased successor block-param reading `.f` untracked.
3. **Field-path dead across all other blocks:** in every block `≠ P`, every mention of `out_id` is a sibling read `record_get out.g` with `g ≠ f`. Any other mention — `record_get out.f`, publish, pass-to-call, store, return, alias, `AAssign` target/source, closure capture — **rejects** (`op_mentions_local` catches all of these, incl. `AMakeClosure` captures at ownership.tw:170 — confirmed by review).
4. **`out.f` read exactly once in the WHOLE function** (wired, not merely asserted): `out_field_read_count(blocks, out_id, f) == 1`, counting every `record_get out.f` across all blocks — *including instructions before `i` in `P`*, which conditions 1 and 3 do **not** cover. This closes the before-projection re-read gap at the contract level instead of relying on `remove_prefix` having already fired.
5. **Loop guard (NEW):** reject if `out` is **live-in to `P`** (`live_contains_int(P.entry.live, out_id)`). A loop-carried projection re-reads the *same* `out.ctx` each iteration; an in-place set would accumulate into the shared value, breaking the immutable-copy semantics of `next := out.ctx`. The old whole-local `live_contains_int(P.exit.live, …)` check caught this incidentally; the field-path relaxation must re-add it explicitly at the entry so the contract — not the fixpoint meet — rejects loop carriers.

Conditions 2–5 **only under-license, never over-license**: every rejection is a superset of a real read/escape/carry. The existing whole-local tail at ownership.tw:458–463 (`exit_mentions_local` + `live_contains_int(exit.live)`) is **replaced** by conditions 2–5; condition 1 stays.

**Negatives that MUST stay persistent** (fixtures in Task 4): (a) `out.f` re-read in a successor block; (b) `out.f` re-read **before** the projection in the same block (locks condition 4); (c) `out` returned / passed to a call / stored in a successor; (d) `out` used in a terminator / as a `case out {…}` scrutinee; (e) `out` forwarded as an **edge-arg** into a join block that reads the block-param's `.f` (locks condition 2's edge-arg branch, which the direct-reference fixtures never exercise); (f) **loop-carried `out`** — the projection inside a `for` (locks condition 5, the highest-value missing case); (g) the existing `red_transport_read_after` (original `ctx` re-read after the wrapper call — a *different* `ret_paths` mechanism, must remain unaffected).

---

## File structure

- **Modify:** `boot/compiler/ownership.tw`
  - `transport_reason` (~443–465): add a `blocks: Vector<CfgBlock>` param; replace the whole-local live-out tail (lines ~458–463) with the new field-path cross-block checks (conditions 2, 4, 5) — note `blk.id.id` is already the projection block id inside `transport_reason`, so **no** separate `proj_block_id` param is needed.
  - New helpers (co-located near `transport_reason`): `out_crosses_block_boundary(blocks, out_id) Bool`, `out_field_dead_other_blocks(blocks, proj_block_id, out_id, fid) Bool`, `out_field_read_count(blocks, out_id, fid) Int`. Pure scans over `Vector<CfgBlock>` reusing `is_record_get_of`, `op_mentions_local`, `exit_mentions_local`, and the inlined sibling test.
  - **Thread `blocks` to BOTH callers of `transport_reason` (review found a second one):**
    - **Path A (the licensing path):** `transport_ok` (~435–437) → `recognize_transport_moves` (~470–484) → `block_prep` (~3348). `block_prep` has **five** call sites, all with a `blocks`/`Vector<CfgBlock>` in scope (confirmed by review): `prep_blocks` (~3361), `prep_get` fallback (~3369), `call_uniques_sited_with_field_seed` (~6950), `collect_move_recovered_params` (~8019), `summarize_seeded` (~8483), and `ownership_stage` (~7325). Add a `blocks` param to `block_prep`, `recognize_transport_moves`, `transport_ok`, and forward at every site.
    - **Path B (the debug-render path):** `transport_verdict` (~4723) calls `transport_reason` at ~4753 and is called from `block_verdicts` (~4831, sole call site ~7326 in `ownership_stage`, where `blocks` is the stage's first param ~7219). Add `blocks` through `block_verdicts` → `transport_verdict` → `transport_reason`. **Omitting this is a compile error** — the plan's first draft missed it.
- **Create fixtures:** `boot/tests/fixtures/cfg/sound_uniqueness/`
  - `transport_field_reread_after.tw` (negative: `out.ctx` read again in a successor)
  - `transport_field_reread_before.tw` (negative: `out.ctx` read before the projection — locks condition 4)
  - `transport_field_escape.tw` (negative: `out` passed to a call in a successor)
  - `transport_field_scrutinee.tw` (negative: `case out {…}` / `out` in a terminator in a successor — locks condition 2 terminator branch)
  - `transport_field_edge_arg.tw` (negative: `out` forwarded as an edge-arg into a join that reads the param's `.f` — locks condition 2 edge-arg branch)
  - `transport_field_loop.tw` (negative: projection inside a `for` — locks condition 5, the highest-value case)
  - `transport_field_two_proj.tw` (negative: `out.ctx` projected in two blocks — locks condition 4 across blocks)
- **Modify tests:** `boot/tests/suites/field_backed_collection_suite.tw` — flip the `field_transport_ctx` test to "emits `rt_dict__set_in_place`"; add one negative test per fixture above (each asserts `!rt_dict__set_in_place` and round-trips).
- **Modify docs (Task 7):** `builder-region-design.md` is unrelated; update `docs/plans/sound-uniqueness/codegen/README.md` (8H follow-up list) and `docs/plans/sound-uniqueness/README.md`.

---

## Task 1: RED — assert the transport dict update lowers in place

**Files:** Modify `boot/tests/suites/field_backed_collection_suite.tw`

- [ ] **Step 1: Replace the deferred `field_transport_ctx` test with the desired-behavior test.**

Locate the existing test (currently asserts only `record_backed_dict` surfaces, persistent). Replace its body with:

```tw
    .test(
      "transported ctx field update lowers dict set in place (8H transport)",
      fn() Result<Void, String> {
        // next := out.ctx; next.types[..] = ..  with a sibling out.tag read in the
        // trailing case. out.ctx is consume-dead (read once, only sibling out.tag
        // read afterward), so the projection moves and the dict set flips in place.
        wat := try compile_fixture_wat("field_transport_ctx")
        body := try wat_func_body_result(wat, "go")
        try assert.ok(
          wat_has_instr(body, "rt_dict__set_in_place"),
          "transported ctx dict update must set in place",
        )
        try assert.str_contains(ir_sites_text("field_transport_ctx"), "record_backed_dict")
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run it to verify it fails.**

Run: `TWK_TEST_FILTER=field_backed target/twk run boot/tests/main.tw`
Expected: FAIL — `transported ctx dict update must set in place` (the body still emits `rt_dict__set`, not `rt_dict__set_in_place`).

---

## Task 2: GREEN — field-path cross-block licensing + observe the flip

**Files:** Modify `boot/compiler/ownership.tw`

- [ ] **Step 1: Add the cross-block field-path helpers near `transport_reason` (after line ~465).**

```tw
// Does `out_id` appear in ANY block's terminator or outgoing edge args? If so,
// `out` crosses a block boundary as a value and could be read as `.fid` through
// an aliased successor block-param that this scan cannot see — reject (soundness
// condition 2).
fn out_crosses_block_boundary(blocks: Vector<CfgBlock>, out_id: Int) Bool {
  for blk in blocks {
    if exit_mentions_local(blk, out_id) {
      return true
    }
  }
  false
}

// In every block OTHER than `proj_block_id`, every mention of `out_id` must be a
// sibling read `record_get out.g` (g != fid). Any other mention rejects
// (soundness condition 3). Also enforces condition 4 across other blocks:
// a `record_get out.fid` in another block is a non-sibling mention -> rejects.
fn out_field_dead_other_blocks(
  blocks: Vector<CfgBlock>,
  proj_block_id: Int,
  out_id: Int,
  fid: Int,
) Bool {
  for blk in blocks {
    if blk.id.id == proj_block_id {
      // handled within-block by transport_reason's existing after-projection scan
    } else {
      for inst in blk.instructions {
        op := inst.op
        is_sibling := case op {
          .ARecordGet(base, f2, _) => atom_is_local(base, out_id) and f2.id != fid,
          _ => false,
        }
        if is_sibling {} else if op_mentions_local(op, out_id) {
          return false
        }
      }
    }
  }
  true
}

// Count EVERY `record_get out.fid` across all blocks (condition 4). Unlike
// out_field_dead_other_blocks this does NOT skip the projection block, so a
// second projection of out.fid BEFORE index i in the projection block is counted
// too — closing the before-projection re-read gap at the contract level.
fn out_field_read_count(blocks: Vector<CfgBlock>, out_id: Int, fid: Int) Int {
  n := 0
  for blk in blocks {
    for inst in blk.instructions {
      if is_record_get_of(inst.op, out_id, fid) {
        n = n + 1
      }
    }
  }
  n
}
```

`CfgBlock` accessors confirmed against `boot/compiler/cfg.tw`: `id: BlockId` (`blk.id.id`), `instructions: Vector<CfgInstruction>` (`inst.op`), `exit: BlockFacts`, and `entry` facts carry `.live` (`P.entry.live`, used by the loop guard).

- [ ] **Step 2: Thread the block list into `transport_ok` / `transport_reason` and replace the whole-local tail with conditions 2, 4, 5.**

Add a `blocks: Vector<CfgBlock>` param to `transport_reason`/`transport_ok`. Replace the tail of `transport_reason` (the current `exit_mentions_local` + `live_contains_int(blk.exit.live, …)` block at ownership.tw:458–463) with:

```tw
  // Condition 5 (loop guard): out live-IN to the projection block means a
  // loop-carried projection re-reading the same out.f each iteration — reject.
  if live_contains_int(blk.entry.live, out_id) {
    return "L${out_id} live-in (loop-carried projection)"
  }
  // Condition 2: out must not cross a block boundary as a value.
  if out_crosses_block_boundary(blocks, out_id) {
    return "L${out_id} crosses a block boundary"
  }
  // Condition 4: out.fid read exactly once in the whole function (this projection).
  if out_field_read_count(blocks, out_id, fid) != 1 {
    return "L${out_id}.f${fid} read more than once"
  }
  // Condition 3: every OTHER-block mention of out is a sibling read.
  if !out_field_dead_other_blocks(blocks, blk.id.id, out_id, fid) {
    return "L${out_id}.f${fid} not field-dead across blocks"
  }
  ""
```

Confirm `blk.entry` exposes `.live` (it should mirror `blk.exit.live`); if the entry live-set is not materialized, derive the loop-carried test from a back-edge predecessor check instead (a predecessor block id `>=` `blk.id.id`, or the CFG's loop-header flag). Keep the existing within-block after-projection loop (ownership.tw:445–457) unchanged — it enforces condition 1.

- [ ] **Step 3: Pass `blocks` through BOTH call chains (Path A licensing + Path B render).**
  - **Path A:** add `blocks` to `recognize_transport_moves(blk, scan)` → forward to `transport_ok(blk, scan, i, bid, f.id, blocks)` → `transport_reason(…, blocks)`. Add `blocks` to `block_prep` and forward at ALL FIVE call sites (`prep_blocks` ~3361, `prep_get` ~3369, ~6950, ~8019, ~8483, `ownership_stage` ~7325) — each has a `Vector<CfgBlock>` in scope.
  - **Path B:** add `blocks` to `block_verdicts` (~4831; sole call at ~7326 in `ownership_stage`, `blocks` = its first param ~7219) → forward to `transport_verdict` (~4723) → `transport_reason(…, blocks)`. Skipping this is a **compile error**.

- [ ] **Step 4: Build-check + observe.**

Run: `target/twk ir --cfg boot/tests/fixtures/cfg/sound_uniqueness/field_transport_ctx.tw 2>&1 | grep -aE "transport=|update L1|field="`

Expected: `L10 = record_get L2.f0 transport=move([.f0] …)` (was `borrow(L2 live-out)`), and the downstream `update L11 base=reuse(unique)` / `field=in-place([.f0] unique)`.

**If** the projection now shows `transport=move` but the dict update still shows `base=persistent` / `field=insufficient deep ownership`, that is the two-gap contingency: `pr.shell` (`atom_field_own(out).project(.Field(ctx)).shell`) is `.None` because `out` does not deeply own `.ctx`. Stop and record it for Task 6 (a `pass`-summary follow-up) — do **not** relax soundness conditions 2–5. (Review confidence: HIGH that this is already `.Some` — `pass`'s `ret_paths=.f0=from(p0)` summary is applied at the `pass` call via `record_ret_path`/`set_field_own` at ownership.tw:4105–4130 with `ctx` unique, exactly as `transport_wrapper_single`'s `wrap` — so the flip should land without Task 6.)

- [ ] **Step 5: Run the target test.**

Run: `TWK_TEST_FILTER=field_backed target/twk run boot/tests/main.tw`
Expected: the Task 1 test PASSES; all other `field_backed` tests still pass.

---

## Task 3: Verify the ownership fact-suite is unregressed

- [ ] **Step 1:** Run the CFG/ownership suites (the transport proof's home):

Run: `TWK_TEST_FILTER=cfg_ownership target/twk run boot/tests/main.tw` then `TWK_TEST_FILTER=return_field_ownership target/twk run boot/tests/main.tw`
Expected: all pass. In particular `red_transport_read_after` / `red_transport_wrapper_chain` / `transport_wrapper_single` behavior is unchanged.

---

## Task 4: Negative fixtures (lock the soundness boundary)

**Files:** Create the fixtures listed in File structure; add one test per fixture. The two review-critical additions are `transport_field_loop` (condition 5) and `transport_field_reread_before` (condition 4). Each fixture keeps the same `Ctx`/`Out`/`pass` preamble; only the `go` body differs. **After writing each, confirm with `target/twk ir --cfg <fixture> | grep transport=` that the projection shows `borrow(<the expected reason>)`, not `move` — a fixture that fails to reject for a *different* reason gives false confidence.**

- [ ] **Step 1: `transport_field_loop.tw` — projection inside a loop (condition 5) → persistent.** Expect `borrow(L… live-in (loop-carried projection))`.

```tw
pub type Ctx = .{ types: Dict<Int, Int>, values: Dict<Int, Int> }
pub type Out = .{ ctx: Ctx, tag: Int }
pub fn pass(ctx: Ctx) Out { Out.{ ctx, tag: 1 } }
pub fn go() Int {
  ctx := Ctx.{ types: Dict.new(), values: Dict.new() }
  out := pass(ctx)
  acc := 0
  for i in range(3) {
    next := out.ctx          // loop-carried: re-reads the SAME out.ctx each iter
    next.types[i] = i
    acc = acc + out.tag
  }
  acc
}
println(go().to_string())
```

- [ ] **Step 2: `transport_field_reread_before.tw` — `out.ctx` read BEFORE the projection (condition 4) → persistent.** Expect the licensing projection to reject via read-count > 1.

```tw
pub type Ctx = .{ types: Dict<Int, Int>, values: Dict<Int, Int> }
pub type Out = .{ ctx: Ctx, tag: Int }
pub fn pass(ctx: Ctx) Out { Out.{ ctx, tag: 1 } }
pub fn peek(ctx: Ctx) Int { ctx.types.len() }
pub fn go() Int {
  ctx := Ctx.{ types: Dict.new(), values: Dict.new() }
  out := pass(ctx)
  n := peek(out.ctx)          // FIRST read of out.ctx (before the projection)
  next := out.ctx             // second read -> out_field_read_count == 2 -> reject
  next.types[2] = 40
  case next.types.get(2) { .Some(v) => v + n, .None => n }
}
println(go().to_string())
```

- [ ] **Step 3: `transport_field_reread_after.tw` — `out.ctx` read again in a successor → persistent.**

```tw
pub type Ctx = .{ types: Dict<Int, Int>, values: Dict<Int, Int> }
pub type Out = .{ ctx: Ctx, tag: Int }
pub fn pass(ctx: Ctx) Out { Out.{ ctx, tag: 1 } }
pub fn go() Int {
  ctx := Ctx.{ types: Dict.new(), values: Dict.new() }
  out := pass(ctx)
  next := out.ctx
  next.types[2] = 40
  again := out.ctx            // second read of out.ctx -> reject
  case again.types.get(2) { .Some(v) => v + out.tag, .None => out.tag }
}
println(go().to_string())
```

- [ ] **Step 4: `transport_field_escape.tw` — `out` passed to a call in a successor → persistent.**

```tw
pub type Ctx = .{ types: Dict<Int, Int>, values: Dict<Int, Int> }
pub type Out = .{ ctx: Ctx, tag: Int }
pub fn pass(ctx: Ctx) Out { Out.{ ctx, tag: 1 } }
pub fn use_out(o: Out) Int { o.tag }
pub fn go() Int {
  ctx := Ctx.{ types: Dict.new(), values: Dict.new() }
  out := pass(ctx)
  next := out.ctx
  next.types[2] = 40
  case next.types.get(2) { .Some(v) => v + use_out(out), .None => 0 }  // out passed to a call
}
println(go().to_string())
```

- [ ] **Step 5: `transport_field_scrutinee.tw` and `transport_field_edge_arg.tw`** — `case out { … }` in a successor (terminator branch of condition 2), and `out` forwarded as an edge-arg into a join that reads the param's `.f` (edge-arg branch). Both expect `borrow(L… crosses a block boundary)`. Construct each so `out` (not just a sibling read) reaches the terminator / edge — e.g. `if flag { use_out(out) } else { … }` after the update, or an `if` whose both arms need `out` so it is threaded as a block param. If a natural source shape cannot force `out` into an edge-arg (the compiler may prefer direct dominating-local references), note that the edge-arg branch is covered by construction/inspection rather than a runnable fixture, and keep `transport_field_two_proj.tw` (below) which is easy to force.

- [ ] **Step 6: `transport_field_two_proj.tw`** — `out.ctx` projected in two different blocks (condition 4 across blocks) → persistent.

```tw
pub type Ctx = .{ types: Dict<Int, Int>, values: Dict<Int, Int> }
pub type Out = .{ ctx: Ctx, tag: Int }
pub fn pass(ctx: Ctx) Out { Out.{ ctx, tag: 1 } }
pub fn go(flag: Bool) Int {
  ctx := Ctx.{ types: Dict.new(), values: Dict.new() }
  out := pass(ctx)
  if flag {
    a := out.ctx
    a.types[1] = 1
    a.types.len()
  } else {
    b := out.ctx            // second projection of out.ctx in a different block
    b.types[2] = 2
    b.types.len()
  }
}
println(go(true).to_string())
```

- [ ] **Step 7: Add one test per negative fixture** — each asserts the `go` body stays on `rt_dict__set` (not `_in_place`) and round-trips. Pattern (repeat per fixture name from Steps 1–6):

```tw
    .test(
      "transport reject: loop-carried out.ctx stays persistent",
      fn() Result<Void, String> {
        wat := try compile_fixture_wat("transport_field_loop")
        body := try wat_func_body_result(wat, "go")
        try assert.ok(!wat_has_instr(body, "rt_dict__set_in_place"),
          "loop-carried projection must not move; dict set stays persistent")
        .Ok({})
      },
    )
    // …one analogous test each for transport_field_reread_before,
    // transport_field_reread_after, transport_field_escape,
    // transport_field_scrutinee (if runnable), transport_field_two_proj…
```

- [ ] **Step 8:** Run `TWK_TEST_FILTER=field_backed target/twk run boot/tests/main.tw` — all pass (positive flips, every negative stays persistent).

- [ ] **Step 9: Runtime round-trip** to confirm correctness of the emitted code:

Run: `target/twk run boot/tests/fixtures/cfg/sound_uniqueness/field_transport_ctx.tw` (expect `42`), and each negative (expect the same result its persistent form yields — the point is *value* correctness, so a fixture whose in-place variant would corrupt must still print the immutable-semantics answer).

---

## Task 5: Full boot suite + self-host fixed point

- [ ] **Step 1 (sequential, foreground):** `target/twk run boot/tests/main.tw` — expect `… tests: N passed` (0 failed). Since this change touches the boot compiler's own analysis and `cfg.tw` uses the `BuildCtx`/`BuildExprOut` transport idiom, watch for new in-place decisions there.

- [ ] **Step 2 (sequential, foreground):** `make stage2` — expect `Fixed point reached: stage3 == stage4`. This is the critical soundness gate: the boot compiler compiling itself with the new licensing must be byte-stable and correct.

- [ ] **Step 3:** `make quick-bundle-cli`, then `target/twk ir --census --sites boot/main.tw 2>&1 | grep -c record_backed` to quantify how many transport-shaped updates in the real compiler now flip.

---

## Task 6: Contingency capture (only if Task 2 Step 4 hit the two-gap case)

- [ ] If the projection licensed but `pr.shell` was `.None`, record in this plan a follow-up task: `pass`'s summary must publish `ret.[.ctx] = from(p0)` field ownership so `out` deeply owns `.ctx`. Cross-check against `transport_wrapper_single` (which proves the analogous `Out.ctx = from(p0)`), and reuse that path rather than adding a new one. Do not merge until the flip is end-to-end.

---

## Task 7: Format, lint, docs, commit

- [ ] **Step 1:** `target/twk fmt boot/compiler/ownership.tw boot/tests/suites/field_backed_collection_suite.tw boot/tests/fixtures/cfg/sound_uniqueness/transport_field_*.tw` then `target/twk lint boot/main.tw` (expect `No findings.`).

- [ ] **Step 2:** Update `docs/plans/sound-uniqueness/codegen/README.md` (8H "Deferred to a follow-up slice" → mark transport `out.ctx`/`out.state` **landed**, describe the field-path cross-block consume-dead extension) and the 8H mention in `docs/plans/sound-uniqueness/README.md` and `codegen/README.md`'s 8I note ("the transport shape emit the persistent …" → now in-place).

- [ ] **Step 3: Commit** (branch first if on `main`):

```bash
git checkout -b 8h-transport-projection-move
git add boot/compiler/ownership.tw boot/tests/suites/field_backed_collection_suite.tw \
        boot/tests/fixtures/cfg/sound_uniqueness/transport_field_*.tw \
        docs/plans/
git commit -m "8H transport: field-path consume-dead across a projection

Make the transport projection-move proof field-path-sensitive across
blocks: license next := out.ctx to move the .ctx subtree even when out is
live-out for SIBLING field reads (out.tag) in successor blocks, provided
out.ctx is read exactly once (whole-function), out never crosses a block
boundary as a value, and the projection is not loop-carried. Flips the
transported-ctx dict update (field_transport_ctx / cfg.tw BuildCtx idiom)
to rt_dict__set_in_place. Negatives (re-read before/after, escape, loop,
edge-arg, two-projection) stay persistent. Self-host fixed point holds."
```

---

## Self-review notes

> ⚠️ **These notes and the Changelog below predate the spike banner at the top and were NOT reconciled with it.** Where they claim the dict `rt_dict__set_in_place` flip lands with "HIGH confidence" and "Task 6 is a no-op," the spike **falsified that**: the flip is blocked behind the `field_facts` depth-2 cap + the `ReturnPathOwn` representation (see the top banner and the **Reframed scope** section below). Under THIS plan alone, Task 1's dict-in-place assertion **cannot pass** — this plan's landable deliverable is transport **shell-reuse only**. Read these notes as accurate for the *licensing* mechanics (conditions 2–5, threading, fixtures) and stale for the *dict outcome*.

- **Spec coverage:** the deferred 8H item (transport `out.ctx`/`out.state`) is implemented (Tasks 1–5); its negatives are locked (Task 4); the existing `red_transport_read_after` guard is preserved (Task 3).
- **Type consistency:** helpers `out_crosses_block_boundary` / `out_field_dead_other_blocks` / `out_field_read_count` are used consistently in Task 2. `CfgBlock` accessors **confirmed**: `blk.id.id`, `inst.op`, `blk.exit.live`; the loop guard needs `blk.entry.live` (verify it is materialized, else use a back-edge-predecessor test).
- **Threading:** confirmed **wider than the first draft** — `blocks` must reach `transport_reason` via **two** paths (licensing: `block_prep` ×5 sites → `recognize_transport_moves` → `transport_ok`; render: `block_verdicts` → `transport_verdict`). Both listed in File structure / Task 2 Step 3.

## Changelog vs. the reviewed draft (soundness review 2026-07-31)

The review verdict was **sound-but-incomplete**; every finding is folded in:
- **[SOUNDNESS] Condition 4 now wired** as `out_field_read_count(blocks, out, f) == 1` (whole-function, incl. before-`i` in the projection block) — closes the before-projection re-read gap at the contract instead of leaning on `remove_prefix`. Fixture `transport_field_reread_before`.
- **[SOUNDNESS] Loop guard added** (condition 5, `live_contains_int(P.entry.live, out)`) — rejects loop-carried projections at the contract instead of leaning on the fixpoint field_own meet. Fixture `transport_field_loop` (the highest-value negative).
- **[SOUNDNESS] Contract scoped honestly:** the delta's own guarantee is limited to the straight-line-sibling-read case; the forward-state invariants it delegates to (`remove_prefix`, publish-on-escape, meet) are named.
- **[MECHANICS] Second `transport_reason` caller** (`transport_verdict`/`block_verdicts`) and the **five** `block_prep` call sites are now enumerated (the draft said "one-level").
- **[COVERAGE] Negative fixtures expanded** from 2 → 7 (before/after re-read, escape, scrutinee, edge-arg, loop, two-projection), each with the expected `borrow(reason)` to confirm it rejects for the *right* reason.
- **[CORRECTNESS] End-to-end flip confidence: HIGH** — the deep-ownership prerequisite (`pr.shell = .Some`) is already produced by `pass`'s `ret_paths=.f0=from(p0)` summary (ownership.tw:4105–4130), the same path that makes `transport_wrapper_single` flip. Task 6 is expected to be a no-op. **← FALSIFIED BY THE SPIKE.** `pr.shell = .Some` (shell-reuse) does hold, but the DICT flip additionally needs `next.types` deeply owned = `out.ctx.types` = `[Field, Field]`, which the depth-2 codec drops. Confidence in the *dict* flip under this plan is therefore NIL; the shell-reuse flip is what actually lands. Task 6 is not a no-op — it is the whole remaining sub-project, now carried by `2026-07-31-nested-return-path-depthcap-lift.md`.

---

## Reframed scope (decision — 2026-07-31)

The top banner promised this section. The spike result forces a split between what this plan can land and what it cannot:

**What this plan CAN land (sound, self-contained):** the field-path cross-block licensing relaxation (conditions 2–5, S-c). It buys transport **record-shell reuse** — `next := out.ctx` moves the `.ctx` subtree, flipping the record-update shell `persistent(aliased shell)` → `reuse(unique)` (`struct.set` instead of a `struct.new` copy). All five adversarial negatives stay persistent, so it does not over-license.

**What this plan CANNOT land:** the headline **dict `rt_dict__set_in_place`** flip. That needs `out.ctx.types` (`[Field, Field]`) represented and carried across the `pass` boundary — the four-layer depth-cap lift, which is a materially larger, soundness-critical change to the ownership summary pipeline.

**Resolution (chosen):** the dict work is carried by **`docs/plans/2026-07-31-nested-return-path-depthcap-lift.md`**, which SUBSUMES this plan's licensing change as its **touch 3 (= S-c, verbatim)** and adds the depth-cap lift (layers 1–2 + the layer-4 `ReturnPathOwn` representation). **Do not execute this plan standalone against its current Task 1 assertion** (`rt_dict__set_in_place`) — it will fail at Task 2 Step 5. Two ways forward:

1. **(Recommended) Fold this plan into the nested plan.** Land licensing + depth-cap lift together; retire this doc. The nested plan's Task 1 spike already re-applies S-c and validates the full chain end-to-end.
2. **Land shell-reuse as an independent increment first.** Keep this plan but **rescope Task 1** to assert the *shell* flip (record-update becomes `reuse(unique)` / `struct.set`), NOT `rt_dict__set_in_place`; drop Task 6; note in docs that the dict flip is deferred to the nested plan. Useful only if shell-reuse alone is worth a separate landing.
