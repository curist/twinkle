# Transport Deep-Graft (wrapper subfield ownership across a call) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (inline) or superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax. Heavy verification (full boot suite, `make stage2`) runs **sequentially, never backgrounded** (per project convention).

**Goal:** Make the transported-record dict update `next := out.ctx; next.types[k] = v` (fixture `field_transport_ctx`, and the real `cfg.tw` `BuildCtx`/`BuildExprOut` idiom) emit `rt_dict__set_in_place` instead of the persistent `rt_dict__set`, without regressing any aliasing guard.

**Architecture:** Four coordinated changes, all validated end-to-end by a throwaway spike (branch `spike-nested-retpath`, 2026-08-01; `make stage2` fixed point held):
1. **S-a** — `field_facts.tw` codec + `graft`: encode/represent depth-2 `[Field, Field]` paths.
2. **S-b** — `ownership.tw` `graft_path_prov`: keep `[Field, Field]` in `path_prov` in lockstep with the codec.
3. **S-c** — `ownership.tw` transport licensing: license the projection `next := out.ctx` to MOVE across blocks when `out` stays live only for *sibling* field reads.
4. **Fix A** — `ownership.tw` call-application deep graft: when a callee wraps a unique param wholesale into a direct field (`ret.fN = from(pK)`), graft the **argument's** owned subfield ownership under `.fN` onto the call result, so a caller projecting `out.fN` recovers the deep field ownership the argument carried.

**This plan deliberately does NOT touch `ReturnPathOwn`.** An earlier plan (`2026-07-31-nested-return-path-depthcap-lift.md`) proposed a depth-2 `ReturnPathOwn` representation ("layer 4"). The spike proved that path has **no producer** (a shell-owned param carries no subfield facts, so `pass`'s summary stays `ret_paths=.f0=from(p0)`) and is **unnecessary** — Fix A delivers the win by grafting the argument's already-known subfield ownership at the call site. Keep `ReturnPathOwn` exactly as it is.

**Tech stack:** Boot compiler (`.tw`, self-hosted). Analysis: `boot/compiler/ownership.tw`, `boot/compiler/field_facts.tw`. Fixtures: `boot/tests/fixtures/cfg/sound_uniqueness/`. Tests: `boot/tests/suites/field_backed_collection_suite.tw`, `boot/tests/suites/cfg_field_facts_suite.tw`, `boot/tests/suites/cfg_return_paths_suite.tw`. Verify: `TWK_TEST_FILTER=… target/twk run boot/tests/main.tw`, then `make stage2`.

---

## Soundness contract (the load-bearing part — review this first)

**Fix A fires iff** the applied ret_path is `via=.Direct, field=.Some(fN), field2=.None, own=.OwnedFromParam(K)` **and** `arg_unique[K]` (the argument is unique at the call). When it fires it grafts `args[K]`'s owned field subtree under prefix `.Field(fN)` onto the call result.

**Why sound — the wholesale-identity invariant:** `OwnedFromParam(K)` at return path `[.fN]` is assignable **only when `out.fN` IS `pK` wholesale** (same object identity), never when `out.fN` is merely *derived from* `pK`. Proof sketch, from `transfer_op`'s `ARecordGet` arm (`ownership.tw:4508–4541`):
- A projection `r := record_get base.f` yields `OwnedFromParam`/deep-owned facts only on the **move** branch, which requires `pr.shell = .Some` — i.e. `base` **deeply owns** `.f` (`base.field_own` has `[.f]` Unique).
- A bare param `p` owns only its **shell** (params seed shell prov `[pK]` and no field-own), so projecting `p.inner` gives `pr.shell = .None` → the `.None` arm (`result = Unknown`, `prov = []`). So `Out.{ ctx: p.inner }` does **not** record `ret.ctx = from(p)`.
- The only routes to `OwnedFromParam(K)` at `[.fN]` are (a) placing the param itself in field `fN` (`Out.{ ctx, tag }` → `out.ctx == pK`), or (b) projecting a field that is *itself* `pK` wholesale (inductive). Both preserve identity.
- This rests on the **field-own / path-prov lockstep** (graft and `graft_path_prov` mirror each other), which S-b maintains. State it explicitly; it is the invariant the whole fix leans on.

**Negatives that MUST stay persistent** (Task 5): a wrapper that (a) returns a **fresh** record for the field (`Out.{ ctx: fresh_copy(ctx), tag }` → `OwnedFresh`, not `OwnedFromParam` → Fix A does not fire); (b) returns a **projection** of the param (`Out.{ ctx: ctx.inner, tag }` → borrow/Unknown, not `from(pK)`); (c) passes the same param into **two disjoint fields** (`Out.{ a: p, b: p }` → each is a distinct-field alias; `drop_aliased_param_paths` already drops both). Each fixture's `go` must keep `rt_dict__set` (no `_in_place`).

---

## File structure

- **Modify** `boot/compiler/field_facts.tw` — S-a: `ff2_base`/`ff2_key` helpers; `path_key`/`path_of_key` depth-2 `[Field,Field]` arm; `graft` `.Field`-prefix inner `.Field` arm.
- **Modify** `boot/compiler/ownership.tw` — S-b (`graft_path_prov` inner `.Field => true`); S-c (`transport_reason`/`transport_ok`/`recognize_transport_moves`/`block_prep`/`transport_verdict`/`block_verdicts` gain a `blocks` param + the field-path cross-block conditions); Fix A (the deep graft in the call-application `for rp in eff_ret_paths` loop).
- **Create fixtures** in `boot/tests/fixtures/cfg/sound_uniqueness/`: `transport_field_reread_after.tw`, `transport_field_loop.tw`, `transport_field_two_proj.tw` (S-c negatives), `transport_fresh_wrap.tw`, `transport_proj_wrap.tw` (Fix A soundness negatives).
- **Modify tests**: `field_backed_collection_suite.tw` (flip `field_transport_ctx` to in-place; add the negatives); `cfg_field_facts_suite.tw` (S-a round-trip; update the depth-3-drop test); `cfg_return_paths_suite.tw` (update the sibling-read borrow test).
- **Reference (do not merge):** the spike branch `spike-nested-retpath` holds the exact working diff (including the now-dropped `ReturnPathOwn` touches — do **not** port those).

---

## Task 1: S-a — encode & represent `[Field, Field]` paths

**Files:** Modify `boot/compiler/field_facts.tw`; Test `boot/tests/suites/cfg_field_facts_suite.tw`

- [ ] **Step 1: Write the failing round-trip test.** In `cfg_field_facts_suite.tw`, add inside the suite:

```tw
    .test(
      "path codec round-trips [Field, Field] depth-2 paths",
      fn() Result<Void, String> {
        p := ff.AccessPath.{ segs: [.Field(0), .Field(1)] }
        k := ff.path_key(p)
        back := ff.path_of_key(k)
        try assert.ok(ff.path_eq(p, back), "round-trip [.f0,.f1] must be identity")
        // disjoint from single-field keys 8 + f*4 + {0,1,2}
        try assert.ok(k >= 4194312, "[Field,Field] keys live above the single-field range")
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run it, expect failure.** Run: `TWK_TEST_FILTER=cfg_field_facts target/twk run boot/tests/main.tw` → FAIL (`path_key` errors "nested seg must be Elem/Val" for `[.Field,.Field]`).

- [ ] **Step 3: Add the codec helpers.** In `field_facts.tw`, after `payload_key` (ends `0 - (packed + 1)`):

```tw
// Depth-2 [Field(f), Field(f2)] paths use a disjoint POSITIVE range above the
// single-field keys. Single-field keys are 8 + f*4 + {0,1,2}; for f < 1048576
// they stay below ff2_base(), so the ranges are disjoint (the single-field arm
// relies on the same f < 1048576 invariant — enforced here for the two-field
// components). f2 in the low 20 bits, f in the next 20.
fn ff2_base() Int { 4194312 } // == 8 + 1048576 * 4

fn ff2_key(f: Int, f2: Int) Int {
  if f < 0 or f >= 1048576 or f2 < 0 or f2 >= 1048576 {
    error("field_facts: [Field,Field] key component out of 20-bit range")
  }
  ff2_base() + f * 1048576 + f2
}
```

- [ ] **Step 4: Encode in `path_key`.** In the `segs.len() == 2` `.Field(f)` arm, add the `.Field(f2)` case:

```tw
      .Field(f) => case segs[1] {
        .Elem => 8 + f * 4 + 1,
        .Val => 8 + f * 4 + 2,
        .Field(f2) => ff2_key(f, f2),
        _ => error("field_facts: nested seg must be Elem/Val/Field"),
      },
```

- [ ] **Step 5: Decode in `path_of_key`.** Add a `k >= ff2_base()` arm **before** the `k >= 8` arm (cond is top-down; `ff2_base() > 8`, so ordering is load-bearing):

```tw
    k >= ff2_base() => {
      rel := k - ff2_base()
      AccessPath.{ segs: [.Field(rel / 1048576), .Field(rel % 1048576)] }
    },
```

- [ ] **Step 6: Represent in `graft`.** In `graft`'s `.Field(_)` prefix arm, add a `.Field(_)` inner case:

```tw
          case inner.segs[0] {
            .Elem => fm.paths[path_key(AccessPath.{ segs: [prefix, .Elem] })] = v,
            .Val => fm.paths[path_key(AccessPath.{ segs: [prefix, .Val] })] = v,
            .Field(_) => fm.paths[path_key(AccessPath.{ segs: [prefix, inner.segs[0]] })] = v,
            _ => {}, // Payload inners not representable under Field prefix
          }
```

- [ ] **Step 7: Run the round-trip test, expect pass.** Run: `TWK_TEST_FILTER=cfg_field_facts target/twk run boot/tests/main.tw`. Round-trip test PASSES. **One existing test now fails** (`graft Field-prefix: inner [.f] path is silently dropped`) — that is expected; Task 6 updates it. Do not fix it here.

- [ ] **Step 8: Commit.**

```bash
git add boot/compiler/field_facts.tw boot/tests/suites/cfg_field_facts_suite.tw
git commit -m "field_facts: represent depth-2 [Field,Field] paths in codec + graft"
```

---

## Task 2: S-b — keep `[Field, Field]` in `path_prov` (lockstep)

**Files:** Modify `boot/compiler/ownership.tw`

- [ ] **Step 1: Change the `graft_path_prov` inner arm.** In `graft_path_prov`, the `.Field(_)` prefix / `case inner.segs[0]` block, change the `.Field` inner from dropped to kept:

```tw
            .Field(_) => case inner.segs[0] {
              .Elem => true,
              .Val => true,
              .Field(_) => true, // [Field, Field] now representable (S-b lockstep with codec)
              _ => false, // [Field, Payload] is not representable
            },
```

- [ ] **Step 2: Build-check.** Run: `target/twk build boot/main.tw -o /tmp/plan.wasm` → `WASM output` (no type errors). No isolated behavioral test here; S-b is verified at the Task 4 integration (it is the path-prov twin of S-a's field-own graft, and publication stays sound because `publish_local` leaks each entry's origins).

- [ ] **Step 3: Commit.**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: keep [Field,Field] in graft_path_prov (S-a lockstep)"
```

---

## Task 3: S-c — field-path cross-block transport licensing + threading

**Files:** Modify `boot/compiler/ownership.tw`; Create `boot/tests/fixtures/cfg/sound_uniqueness/transport_field_reread_after.tw`, `transport_field_loop.tw`, `transport_field_two_proj.tw`

The transport machinery (`transport_ok`/`transport_reason`/`recognize_transport_moves`) currently rejects a projection when `out` is live-out of its block. S-c replaces that whole-local tail with a field-path-sensitive whole-function scan: license the move iff, in every block other than the projection's, every mention of `out` is a sibling read `record_get out.g` (`g ≠ f`), `out` never crosses a block boundary as a value, `out.f` is read exactly once function-wide, and the projection is not loop-carried.

- [ ] **Step 1: Add the three cross-block helpers.** In `ownership.tw`, immediately before `transport_ok` (replacing the old `transport_ok`/`transport_reason` comment+bodies at ~423–465):

```tw
// Does `out_id` appear in ANY block's terminator or outgoing edge args? If so,
// `out` crosses a block boundary as a value and could be read as `.fid` through
// an aliased successor block-param this scan cannot see — reject (condition 2).
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
// (condition 3). A `record_get out.fid` in another block is a non-sibling mention
// -> rejects (this also backstops condition 4 across other blocks).
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

- [ ] **Step 2: Rewrite `transport_ok` / `transport_reason` with a `blocks` param.** Replace their bodies with:

```tw
// ... (keep the existing doc comment, updated for the field-path relaxation) ...
fn transport_ok(
  blk: CfgBlock,
  scan: BlockScan,
  i: Int,
  out_id: Int,
  fid: Int,
  blocks: Vector<CfgBlock>,
) Bool {
  transport_reason(blk, scan, i, out_id, fid, blocks) == ""
}

fn transport_reason(
  blk: CfgBlock,
  scan: BlockScan,
  i: Int,
  out_id: Int,
  fid: Int,
  blocks: Vector<CfgBlock>,
) String {
  insts := blk.instructions
  for j in range_from(i + 1, insts.len()) {
    op := insts[j].op
    if is_record_get_of(op, out_id, fid) {
      return "L${out_id}.f${fid} read twice"
    }
    is_sibling := case op {
      .ARecordGet(base, f2, _) => atom_is_local(base, out_id) and f2.id != fid,
      _ => false,
    }
    if is_sibling {} else if op_mentions_local(op, out_id) {
      return "L${out_id} published"
    }
  }
  // Condition 5 (loop guard): out live-IN to the projection block means a
  // loop-carried projection re-reading the same out.f each iteration — reject.
  if live_contains_int(blk.entry.live, out_id) {
    return "L${out_id} live-in (loop-carried projection)"
  }
  // Condition 2: out must not cross a block boundary as a value.
  if out_crosses_block_boundary(blocks, out_id) {
    return "L${out_id} crosses a block boundary"
  }
  // Condition 4: out.fid read exactly once in the whole function.
  if out_field_read_count(blocks, out_id, fid) != 1 {
    return "L${out_id}.f${fid} read more than once"
  }
  // Condition 3: every OTHER-block mention of out is a sibling read.
  if !out_field_dead_other_blocks(blocks, blk.id.id, out_id, fid) {
    return "L${out_id}.f${fid} not field-dead across blocks"
  }
  ""
}
```

- [ ] **Step 3: Thread `blocks` through Path A (licensing).** `recognize_transport_moves(blk, scan)` gains a `blocks: Vector<CfgBlock>` param and forwards it to `transport_ok(..., blocks)`. `block_prep(blk, sem)` gains `blocks: Vector<CfgBlock>` and forwards to `recognize_transport_moves(blk, scan, blocks)`. Then at `block_prep`'s call sites: `prep_blocks` passes its own `blocks`; `ownership_stage`'s per-block call (the one immediately before `block_verdicts`) passes `blocks`; every OTHER call site (`prep_get` fallback, and the summary/seed passes) passes `[]` — an empty list makes condition 4's read-count 0, so transport is conservatively **never** licensed there (sound; the real decision is in `ownership_stage`).

- [ ] **Step 4: Thread `blocks` through Path B (render).** `transport_verdict(...)` gains a trailing `blocks: Vector<CfgBlock>` param and forwards to `transport_reason(..., blocks)`. `block_verdicts(...)` gains a trailing `blocks: Vector<CfgBlock>` param, forwards to the `pre.transport_verdict(..., blocks)` call, and `ownership_stage` passes `blocks` at the `block_verdicts(...)` call. **Omitting Path B is a compile error** (two callers of `transport_reason`).

- [ ] **Step 5: Build-check.** Run: `target/twk build boot/main.tw -o /tmp/plan.wasm` → `WASM output`.

- [ ] **Step 6: Create the S-c negative fixtures.** Each keeps the same `Ctx`/`Out`/`pass` preamble as `field_transport_ctx.tw`.

`transport_field_reread_after.tw` — `out.ctx` re-read in a successor → persistent:
```tw
pub type Ctx = .{ types: Dict<Int, Int>, values: Dict<Int, Int> }
pub type Out = .{ ctx: Ctx, tag: Int }
pub fn pass(ctx: Ctx) Out { Out.{ ctx, tag: 1 } }
pub fn go() Int {
  ctx := Ctx.{ types: Dict.new(), values: Dict.new() }
  out := pass(ctx)
  next := out.ctx
  next.types[2] = 40
  again := out.ctx            // second read of out.ctx -> reject (read_count > 1)
  case again.types.get(2) { .Some(v) => v + out.tag, .None => out.tag }
}
println(go().to_string())
```

`transport_field_loop.tw` — projection inside a loop → persistent (loop guard):
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

`transport_field_two_proj.tw` — `out.ctx` projected in two blocks → persistent (read_count across blocks):
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
    b := out.ctx             // second projection of out.ctx in a different block
    b.types[2] = 2
    b.types.len()
  }
}
println(go(true).to_string())
```

- [ ] **Step 7: Commit.**

```bash
git add boot/compiler/ownership.tw boot/tests/fixtures/cfg/sound_uniqueness/transport_field_*.tw
git commit -m "ownership: field-path cross-block transport licensing (S-c) + negatives"
```

---

## Task 4: Fix A — deep-graft the argument's subtree; flip the dict

**Files:** Modify `boot/compiler/ownership.tw`, `boot/tests/suites/field_backed_collection_suite.tw`

- [ ] **Step 1: Replace the deferred `field_transport_ctx` test with the desired-behavior test.** In `field_backed_collection_suite.tw`, replace the existing `field_transport_ctx` test body with:

```tw
    .test(
      "transported ctx field update lowers dict set in place (transport deep-graft)",
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

- [ ] **Step 2: Run it, expect failure.** Run: `TWK_TEST_FILTER=field_backed target/twk run boot/tests/main.tw` → FAIL (`transported ctx dict update must set in place`; still `rt_dict__set`). Fixtures S-a/S-b/S-c alone give shell reuse but leave `field=persistent(insufficient deep ownership)`.

- [ ] **Step 3: Add the deep graft to the call-application loop.** In `ownership.tw`, inside the `for rp in eff_ret_paths { ... }` loop, in the `if own_here and result_ok { ... }` block, **after** `result_pp = r.pp`:

```tw
      // Fix A (deep wrapper graft): when the callee wraps a unique param wholesale
      // into a direct field (ret.fN = from(pK), field2=None), out.fN IS args[k], so
      // out.fN's owned subtree == args[k]'s owned subtree. record_ret_path wrote only
      // the shell [.fN]; graft args[k]'s inner field ownership under .fN too, so a
      // caller projecting out.fN recovers the deep field ownership the arg carried.
      // Sound because OwnedFromParam(K) is assignable only when out.fN IS pK wholesale
      // (see the soundness contract; rests on the field-own/path-prov lockstep).
      case rp.via {
        .Direct => case rp.field {
          .Some(fN) => case rp.own {
            .OwnedFromParam(k) => {
              arg := args[k]
              result_fields = result_fields.graft(ff.PathSeg.Field(fN), st.atom_field_own(arg))
              result_pp = graft_path_prov(result_pp, ff.PathSeg.Field(fN), st, arg)
            },
            .OwnedFresh => {},
          },
          .None => {},
        },
        .Variant(_, _) => {},
      }
```

> Note: `ReturnPathOwn` has no `field2` field in this plan, so the case is on `rp.via`/`rp.field`/`rp.own` only. `graft` and `graft_path_prov` already skip non-representable/non-owned inners, so nothing over-claims.

- [ ] **Step 4: Run the target test, expect pass.** Run: `TWK_TEST_FILTER=field_backed target/twk run boot/tests/main.tw` → the flip test PASSES; all other `field_backed` tests pass.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/ownership.tw boot/tests/suites/field_backed_collection_suite.tw
git commit -m "ownership: deep-graft wrapped-param subtree at call sites (Fix A)

Flips the transported-ctx dict update (field_transport_ctx / cfg.tw BuildCtx
idiom) to rt_dict__set_in_place by carrying the argument's owned subfield
ownership across a wholesale-param wrapper return."
```

---

## Task 5: Fix A soundness negatives (lock the wholesale-identity boundary)

**Files:** Create `boot/tests/fixtures/cfg/sound_uniqueness/transport_fresh_wrap.tw`, `transport_proj_wrap.tw`; Modify `field_backed_collection_suite.tw`

- [ ] **Step 1: `transport_fresh_wrap.tw` — wrapper returns a FRESH record for the field → `OwnedFresh`, Fix A must not fire.**

```tw
pub type Ctx = .{ types: Dict<Int, Int>, values: Dict<Int, Int> }
pub type Out = .{ ctx: Ctx, tag: Int }
// Returns a FRESH Ctx (not the param): out.ctx shell is fresh -> OwnedFresh.
pub fn pass(ctx: Ctx) Out { Out.{ ctx: Ctx.{ types: Dict.new(), values: Dict.new() }, tag: 1 } }
pub fn go() Int {
  ctx := Ctx.{ types: Dict.new(), values: Dict.new() }
  out := pass(ctx)
  next := out.ctx
  next.types[2] = 40
  case next.types.get(2) { .Some(v) => v + out.tag, .None => out.tag }
}
println(go().to_string())
```

> This one MAY legitimately flip via a different (fresh-ownership) route — the point of the fixture is that it must remain **correct**, and that Fix A's `OwnedFromParam` arm does not fire on a fresh field. Assert value correctness (Step 3) rather than persistence for this case; keep it if it stays persistent, and if it flips, confirm via runtime round-trip that the value is right.

- [ ] **Step 2: `transport_proj_wrap.tw` — wrapper returns a PROJECTION of the param → borrow, not `from(pK)`, Fix A must not fire; dict stays persistent.**

```tw
pub type Inner = .{ types: Dict<Int, Int>, values: Dict<Int, Int> }
pub type Ctx = .{ inner: Inner, meta: Int }
pub type Out = .{ ic: Inner, tag: Int }
// out.ic = ctx.inner is a PROJECTION of the param; its shell is not the param.
pub fn pass(ctx: Ctx) Out { Out.{ ic: ctx.inner, tag: 1 } }
pub fn go() Int {
  ctx := Ctx.{ inner: Inner.{ types: Dict.new(), values: Dict.new() }, meta: 0 }
  out := pass(ctx)
  next := out.ic
  next.types[2] = 40
  case next.types.get(2) { .Some(v) => v + out.tag, .None => out.tag }
}
println(go().to_string())
```

- [ ] **Step 3: Add tests.** In `field_backed_collection_suite.tw`, add one persistence test per S-c negative (`transport_field_reread_after`, `transport_field_loop`, `transport_field_two_proj`) and for `transport_proj_wrap`, plus a runtime round-trip for `transport_fresh_wrap`. Pattern:

```tw
    .test(
      "transport reject: <name> stays persistent",
      fn() Result<Void, String> {
        wat := try compile_fixture_wat("<name>")
        body := try wat_func_body_result(wat, "go")
        try assert.ok(!wat_has_instr(body, "rt_dict__set_in_place"),
          "<name> must not move; dict set stays persistent")
        .Ok({})
      },
    )
```

- [ ] **Step 4: Run.** Run: `TWK_TEST_FILTER=field_backed target/twk run boot/tests/main.tw` → all pass (positive flips; every negative stays persistent).

- [ ] **Step 5: Runtime round-trip** (value correctness of the emitted code). Run: `target/twk run boot/tests/fixtures/cfg/sound_uniqueness/field_transport_ctx.tw` (expect `42`), and each negative — each must print the same value its persistent form yields.

- [ ] **Step 6: Commit.**

```bash
git add boot/tests/fixtures/cfg/sound_uniqueness/transport_fresh_wrap.tw \
        boot/tests/fixtures/cfg/sound_uniqueness/transport_proj_wrap.tw \
        boot/tests/suites/field_backed_collection_suite.tw
git commit -m "tests: lock Fix A wholesale-identity boundary (fresh/projection wrappers)"
```

---

## Task 6: Update the two pre-existing behavior-change tests

Both encode behavior that S-a/S-c intentionally relax. Update them to assert the new behavior (do NOT weaken soundness — verify each new expectation is correct).

**Files:** Modify `boot/tests/suites/cfg_field_facts_suite.tw`, `boot/tests/suites/cfg_return_paths_suite.tw`

- [ ] **Step 1: `cfg_field_facts_suite.tw` — the `graft Field-prefix: inner [.f] path is silently dropped (depth-3 under-claim)` test.** S-a now **keeps** the `[Field,Field]` inner. Locate the test (asserts the inner is dropped, `expected 1`), read the fixture it builds, and change the expectation to reflect that the `[.f0,.f0]` path is now present (the count becomes 2). Rename it to `graft Field-prefix: inner [.f] path is kept (depth-2 representable)`. Genuinely depth-3 inners (`[Field,Field,Field]`) are still dropped — if the test also covers those, keep that half asserting the drop.

- [ ] **Step 2: `cfg_return_paths_suite.tw` — the `transport borrow: out live into a later block forces borrow (live-out check)` test.** Its fixture `case_w_out_liveout_fixture` is `g := out.f0; if cond { ty := out.f1 }` — `out.f0` read once, only a **sibling** `out.f1` read later. S-c now correctly licenses this as a MOVE. Change `assert.equal(caller_own(f, gsib_local), own_shared())` to `own_unique()`, and update the test name/comment to `transport move: out live only for sibling reads is licensed`. **Verify** by inspection that the later read is genuinely `out.f1` (sibling), not a re-read of `out.f0` — if it were `out.f0`, the move would be unsound and the test should stay `own_shared()`.

- [ ] **Step 3: Run both suites.** Run: `TWK_TEST_FILTER=cfg_field_facts target/twk run boot/tests/main.tw` then `TWK_TEST_FILTER=cfg_return_paths target/twk run boot/tests/main.tw` → all pass.

- [ ] **Step 4: Commit.**

```bash
git add boot/tests/suites/cfg_field_facts_suite.tw boot/tests/suites/cfg_return_paths_suite.tw
git commit -m "tests: update graft-depth and sibling-read-borrow assertions for S-a/S-c"
```

---

## Task 7: Full boot suite + self-host fixed point

- [ ] **Step 1 (sequential, foreground):** `target/twk run boot/tests/main.tw` → `… tests: N passed` (0 failed). Since this touches the boot compiler's own analysis and `cfg.tw` uses the `BuildCtx`/`BuildExprOut` transport idiom, watch for new in-place decisions there.

- [ ] **Step 2 (sequential, foreground):** `make stage2` → `Fixed point reached: stage3 == stage4`. This is the critical soundness gate: the boot compiler compiling itself with the new licensing + graft must be byte-stable and correct. (The spike confirmed this holds.)

- [ ] **Step 3:** `make quick-bundle-cli`, then `target/twk ir --census --sites boot/main.tw 2>&1 | grep -c record_backed` to quantify how many transport-shaped updates in the real compiler now flip.

---

## Task 8: Format, lint, docs, commit

- [ ] **Step 1:** `target/twk fmt boot/compiler/field_facts.tw boot/compiler/ownership.tw boot/tests/suites/field_backed_collection_suite.tw boot/tests/suites/cfg_field_facts_suite.tw boot/tests/suites/cfg_return_paths_suite.tw boot/tests/fixtures/cfg/sound_uniqueness/transport_*.tw` then `target/twk lint boot/main.tw` (expect `No findings.`).

- [ ] **Step 2:** Update `docs/plans/sound-uniqueness/codegen/README.md` (mark the transport `out.ctx`/`out.state` in-place win **landed**, describe the field-path cross-block licensing + the call-site deep graft) and the 8H/transport mention in `docs/plans/sound-uniqueness/README.md`. Delete the two superseded plan docs' rows from `docs/plans/README.md` (per the plans-README convention) and move `2026-07-31-8h-transport-projection-move.md` + `2026-07-31-nested-return-path-depthcap-lift.md` to the archive, since this plan supersedes both (they remain the record of why layer-4 was abandoned).

- [ ] **Step 3: Commit.**

```bash
git add boot/ docs/plans/
git commit -m "docs: land transport deep-graft; archive superseded depth-cap plans"
```

---

## Self-review notes

- **Spec coverage:** S-a (Task 1), S-b (Task 2), S-c + negatives (Task 3), Fix A + flip (Task 4), Fix A soundness negatives (Task 5), the two behavior-change test updates (Task 6), self-host gate (Task 7). The wholesale-identity soundness contract is stated up front and locked by Task 5's fixtures.
- **Dropped from the prior plan:** all `ReturnPathOwn` work (touches 4/5/5b/6/8) — the spike proved it unnecessary (no producer). `drop_aliased_param_paths` needs **no change** in this scope, because no depth-2 ret_paths are produced; the existing param-origin count is correct for the depth-1 paths this plan uses.
- **Type consistency:** `blocks: Vector<CfgBlock>` threads identically through `transport_reason`/`transport_ok`/`recognize_transport_moves`/`block_prep`/`transport_verdict`/`block_verdicts`. Fix A uses `st.atom_field_own(arg)`, `graft_path_prov(pp, prefix, st, arg)`, `ff.PathSeg.Field(fN)`, and `FieldMap.graft(prefix, src)` — all confirmed to exist.
- **Known invariant to preserve:** field-own / path-prov lockstep (S-b). Fix A's soundness rests on it; if a future change lets `field_own` hold a `[.f]` that `path_prov` lacks (or vice versa), re-audit Fix A.
