# Typed Vector Persistent Append Promotion (Sub-project B) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a promoted typed `PVecI64` vector also carry an in-region persistent append (`xs = xs.append(v)`), so a `collect`/`Vector.make` producer with `xs[i]=v` **and** `xs.append(v)` + reads + return types end to end instead of falling back to boxed on the append.

**Architecture:** Depends on sub-project **A** (`2026-08-02-typed-vector-indexed-write.md`), which already promotes indexed-update vectors to `PVecI64`. `Vector.append` is a compiler **intrinsic** (`.VectorAppend`), emitted inline as `rt_arr__push`. So typing it has two moving parts: (1) `route_typed_vec` must *accept* an append on a candidate as a non-escaping in-region op (today it escapes → boxed); (2) the intrinsic emitter swaps `rt_arr__push → rt_arr__push_i64` by the base's physical Wasm type — exactly the emit-by-base-type pattern the `xs[i]` read already uses (`emit_index_op` → `fam.get_call`). Append has **no** in-place variant, so there is no ownership composition: the typed append is a persistent radix push, valid for the strict vectors promotion produces.

**Tech Stack:** Boot compiler (self-hosted Twinkle in `boot/`), hand-written Wasm-GC runtime IR (`boot/compiler/codegen/runtime/arr.tw`), backend representation-routing pass (`boot/compiler/backend/route_typed_vec.tw`), intrinsic emitter (`boot/compiler/codegen/emit.tw`).

**Design source:** the `project-typed-vector-repr` track; completes the set+append scope agreed for typed indexed-update promotion.

> **HARD DEPENDENCY — do not assign B to a worker until A is merged.** B is **not executable** against the current tree: it reuses sub-project A's `collect_write_results` fixpoint, the `.AAssign` write-result exception, the i64-family guard, and the `TWINKLE_TYPED_VEC_WRITE` kill-switch — none of which exist until A lands. Phase 2 Step 0 below is an explicit precondition check; if those symbols are absent, stop and finish A first.

---

## Critical facts (verified against the tree, 2026-08-02)

1. **`Vector.append` is an intrinsic, not an `rt` builtin call.** It is registered `intr("vector$append", .Some("Vector.append"))` (`builtins.tw:516`) and emitted by `emit_intrinsic_vector_append` (`emit.tw:2531`) which does `rt_arr__push(vec, elem)` (`emit.tw:2537-2541`), dispatched from the `.VectorAppend` intrinsic kind (`emit.tw:2405`). Consequence: routing does **not** swap the append callee id (contrast the set in sub-project A, which is an `rt` call swapped in `rewrite_op`). The typed swap happens in the **emitter**, by base Wasm type.

2. **Emit-by-base-type is the established pattern.** `emit_index_op` (`emit/arrays.tw:86`) reads `base_vt := fns.atom_val_type(base, ctx)`, then `case pvec_family_of(base_vt) { .Some(fam) => … fam.get_call … }`. The append emitter mirrors this: if the base is a family `PVecI64`, call the family push (`rt_arr__push_i64`); else `rt_arr__push` unchanged.

3. **Append has no in-place variant.** `opt/semantics.tw` registers `Vector.append` (keyed as `builder.push_id` / `method_id("Vector","append")` at `semantics.tw:162`, not a literal `"vector$append"` string) with `in_place_equivalent: .None` — owned append loops are handled earlier by the builder-region pass, not here. So the typed append is purely a persistent push: no `select_call_for_emit` / catalog / decision-remap work, unlike sub-project A's set.

4. **Runtime substrate — do NOT mirror boxed `push` (it is not leaf-agnostic).** Verified: boxed `push` (`arr.tw:1209`) promotes a full tail via `wrap_leaf` + **`concat_trees`** (`arr.tw:1291-1293`), and `concat_trees` casts leaves to boxed `t_ARRAY` for RRB rebalancing (`RefCast t_array` mid-body — the documented `project-rrb-next` gotcha). Passing `ArrayI64` leaves through it traps / miscompiles. So a typed `push_i64` **cannot** copy `push`. Instead it must do a **radix** persistent append, the same leaf-agnostic path the typed builder uses:
   - **Correct analogue: `builder_push_i64` (`arr.tw:2409`)**, which already appends to the typed family with radix growth — it unboxes the element (`RefCast BoxedInt; StructGet 0`), writes into the `ArrayI64` tail when it has room, and on a full tail promotes via `promote_full_tail_i64` (`arr.tw:2476`) + grows the root radix-style. `push_i64` differs only in that it builds a **new persistent `PVecI64`** (copy-then-append tail semantics) instead of mutating a builder in place.
   - **Leaf-agnostic helpers that ARE reusable:** `push_tail` (`arr.tw:1023` — treats leaves as `ref eq`, casts only internal nodes), `new_path`, and `wrap_leaf` (widened to `ref eq`). Use these for root-overflow growth, **never** `concat_trees`.
   - **`promote_full_tail_i64` length semantics need care (reviewer-flagged).** It assumes the prefix length *excludes* the full tail and adds `rt_BF` internally; do not pass the whole vector's length or you overcount before adding the appended element. Mirror the exact arguments `builder_push_i64` passes to it (`arr.tw:2476` context), not a hand-derived length.
   - Because this op is subtle, **budget an explicit grounding sub-step** in Phase 1 to read `builder_push_i64` + `promote_full_tail_i64` in full and write `push_i64` as a delta from the builder (persistent-tail variant), then validate the tail-boundary and root-overflow cases with the Phase 4 execution fixtures.

5. **Radix-only is sound here.** `push_i64` builds strict/radix vectors and would trap on *relaxed* (post-slice/concat RRB) vectors. A promoted vector cannot be relaxed: slicing/concat escape routing and demote to boxed (unchanged), so any vector reaching `push_i64` was built strictly by `collect`/`Vector.make`/`set`. No relaxed handling is needed; add an assertion-style comment, not a code path.

6. **This sub-project SHARES sub-project A's group-membership machinery.** A's `collect_write_results` fixpoint + the `.AAssign` write-result exception + the i64-family guard already exist after A lands. B **extends** them to append: `collect_write_results` also collects `ACall(vector$append, [base,…])` result slots (so `xs = xs.append(v)`'s result joins the group and the rebind is non-escaping), and `classify_op` gains the append acceptance branch — both gated on `fam.mono_key == "vec_i64"`. There is **no** `rewrite_op` change (append is emitter-swapped, not a routed callee), and **no** decision-remap (append has no in-place variant, fact 3). The bool-family guard is mandatory in the **emitter** too (`pvec_family_of` can return bool; only `push_i64` exists), see Phase 3.

7. **This changes boot's own codegen (routing is default-on).** Same regression posture as sub-project A: gate behind the shared `TWINKLE_TYPED_VEC_WRITE` kill-switch (extended to also cover append acceptance), and validate with **self-host fixed point + full boot suite + Rust suite**, not byte-identity. Reuse A's kill-switch plumbing.

8. **Rebuild discipline** identical to sub-project A: `make bundle-cli` after any `boot/` compiler edit (must print `Fixed point reached`); test-suite edits need no rebuild; run `target/twk fmt <file>` + `target/twk lint boot/main.tw` after any `.tw` edit. **No `builtin_specs()` change is needed in this sub-project** (`push_i64` is `.Call`ed directly by the emitter — fact 2 / File Structure), so the FuncId-append discipline does not apply here; the earlier draft's mention of appending `builtin_specs()` entries was wrong and is removed.

---

## File Structure

**Create/extend:**
- `boot/tests/suites/typed_vector_write_suite.tw` — extend sub-project A's suite (or a sibling `typed_vector_append_suite.tw`) with append-promotion fixtures.

**Modify:**
- `boot/compiler/codegen/runtime/arr.tw` — add `push_i64_fn`, written as a **persistent-tail delta from `builder_push_i64` (`:2409`)**, NOT from boxed `push` (`:1209` uses `concat_trees`, not leaf-agnostic — fact 4); register in `module()` near the other `_i64` ops.
- `boot/compiler/builtins.tw` — **no change needed.** `push_i64` is reached only through the emitter's direct `.Call("rt_arr__push_i64")`, never as an ANF `ACall`. Like the boxed `push` (verified: `push` is absent from `builtins.tw`), it is kept purely by `module()` reachability — no `rt(...)` spec, no `builtin_abi` arm, no `intr`.
- `boot/compiler/backend/route_typed_vec.tw` — extends sub-project A's machinery (fact 6): `RouteIds` gains `append` (populated `= builtins.id("vector$append").id`, or `-1` under the kill-switch); the shared `collect_write_results` fixpoint also collects `ACall(append, [base∈vs,…])` results (so the append result joins the group and the A-added `.AAssign` exception makes the rebind non-escaping); `classify_op` (`:1416`) accepts `append(v, x)` (`v` the base) as non-escaping — all gated on `fam.mono_key == "vec_i64"`. **No** `rewrite_op` change and **no** decision remap (append is emitter-swapped, no in-place variant).
- **No `elem_family.tw` change** — `ElemFamily` (`boot/compiler/elem_family.tw:8`, verified: NOT under `backend/`) already carries `mono_key` ("vec_i64"/"vec_bool"). The emitter discriminates on `fam.mono_key`; no `push_call` field is added (avoids the String-vs-optional ambiguity and the unsound-boxed-fallback problem entirely).
- `boot/compiler/codegen/emit.tw` — `emit_intrinsic_vector_append` (`:2531`) reads the base valtype via the free function `atom_val_type(args[0], ctx)` (this emitter has no `fns`/`result_vt` params — see `emit.tw:2527`), then branches on the family's `mono_key` — see Phase 3 Step 1 for the exact three-way branch (i64 → `push_i64`; typed non-i64 → hard error, unreachable; boxed → `rt_arr__push`). **Do not silently boxed-fallback a typed `PVecBool` base** (fact / B-review): that would pass a `PVecBool` ref to `rt_arr__push` which expects a boxed `PVec` → type-unsound. A `PVecBool` base reaching this emitter means routing failed to demote a bool append; make it a loud error, not a silent miscompile.
- (Maybe) `boot/compiler/backend/verify_expr.tw` — only if the post-route verifier flags the append site; the append is intrinsic-emitted (not an ABI-checked builtin call), so it likely needs no verifier arm — confirm in Phase 3.

**Do NOT modify:** the boxed `push`/`push_tail`/`promote_full_tail` fns, the builder-region append path, or any `src/` (stage0).

---

## Phase 1: Runtime substrate (`push_i64`)

Goal: a typed persistent single-element append exists and is registered as an internal runtime op, verified by self-host as dead code.

**Files:** `boot/compiler/codegen/runtime/arr.tw`.

- [ ] **Step 0: Ground the algorithm (do NOT skip — fact 4).** Read `builder_push_i64` (`arr.tw:2409`) and `promote_full_tail_i64` (used at `arr.tw:2476`) in full, and read boxed `push` (`arr.tw:1209`) to see the parts you must NOT copy (its `wrap_leaf` + `concat_trees` full-tail promotion at `:1291-1293` is not leaf-agnostic). Confirm the exact length arguments `builder_push_i64` passes to `promote_full_tail_i64` (it uses prefix-length *excluding* the full tail; the helper adds `rt_BF`). Write down the three cases you must reproduce: (a) tail has room → copy tail + append; (b) tail full, root grows within radix → `promote_full_tail_i64` + `push_tail`; (c) new root level.

- [ ] **Step 1: Add `push_i64_fn`** — write it as a **persistent-append delta from `builder_push_i64`** (NOT from boxed `push`). `params [pvec_i64_ref(), .Anyref]`, `results [pvec_i64_ref()]`, tail locals `arr_i64_null()`. Semantics: produce a **new** `PVecI64` (copy the tail into a fresh `ArrayI64` of `tail_len+1` when it has room, then set the appended element), or on a full tail promote via `promote_full_tail_i64` with the SAME length arguments the builder uses and grow the root radix-style via the leaf-agnostic `push_tail`/`new_path`/`wrap_leaf`. Use `t_PVEC_I64`/`t_ARRAY_I64` throughout; unbox the element once (`RefCast(false, .Named(t_BOXED_INT)); StructGet(t_BOXED_INT, 0)`). **Never call `concat_trees`.** The tail-has-room case can also be cross-checked against boxed `push`'s tail branch (`arr.tw:1224-1264`, which is leaf-agnostic apart from the array types) — only the full-tail `else` branch of boxed `push` is off-limits.

- [ ] **Step 2: Register in `module()`** — add `push_i64_fn()` to the runtime func list near the other `_i64` ops (e.g. after `mutvec_freeze_i64_fn()`).

- [ ] **Step 3: No builtins registration** — `push_i64` needs no `builtins.tw` entry (it is `.Call`ed directly by the emitter, like boxed `push`; kept via `module()` reachability). Skip straight to rebuild.

- [ ] **Step 4: Rebuild.** Run: `make bundle-cli` — must print `Fixed point reached`. The op assembles as dead code (nothing emits it yet; it will be pruned by Wasm DCE until Phase 3 emits the call — that is expected).

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/codegen/runtime/arr.tw
git commit -m "feat(typed-vec): push_i64 typed persistent append runtime op (inert)"
```

---

## Phase 2: Routing — extend A's group machinery to append (i64-guarded)

Goal: an in-region append on a promoted candidate joins the group and no longer escapes, so a set+append vector stays eligible for `PVecI64`. This **reuses** sub-project A's `collect_write_results` fixpoint and `.AAssign` write-result exception (which must already exist) — see fact 6. If those are not present, A is not fully landed; stop and finish A first.

**Files:** `boot/compiler/backend/route_typed_vec.tw`.

- [ ] **Step 0: Precondition check (A must be merged).** Confirm the current `route_typed_vec.tw` contains A's `collect_write_results` fixpoint, the `.AAssign` write-result exception (`:1589` region), the `fam.mono_key == "vec_i64"` guards, and that `route_ids()` reads `TWINKLE_TYPED_VEC_WRITE`. If any is missing, STOP — A is not merged and B cannot proceed.

- [ ] **Step 1: Extend `RouteIds`.** Add `append: Int`; populate in `route_ids()` via `builtins.id("vector$append").id`. Under the `TWINKLE_TYPED_VEC_WRITE=0` kill-switch (from sub-project A — same `route_ids()` env read), set `append = -1` so neither the acceptance branch nor the `collect_write_results` append clause below matches.

- [ ] **Step 2: Collect append results into the group.** In A's `collect_write_results` fixpoint, add append: also add the result slot of every `ACall(vector$append, [base, _])` with `base ∈ vs` (alongside the existing `set_unsafe` clause), gated on `fam.mono_key == "vec_i64"`. This makes `xs = xs.append(v)`'s result a group member, so the A-added `.AAssign` write-result exception (`route_typed_vec.tw:1589`) already makes the rebind non-escaping — no further `.AAssign` change is needed.

- [ ] **Step 3: Accept append in `classify_op` (i64 only).** In the `.ACall(.AGlobalFunc(fid), args)` arm (`route_typed_vec.tw:1427`), add before the escaping else, mirroring A's set branch:

```
} else if fam.mono_key == "vec_i64" and fid.id == ids.append and args.len() == 2
  and slot_in(args[0], vs) and !slot_in(args[1], vs) {
  use_esc(false)
}
```

The base (`args[0]`) is the candidate; the value (`args[1]`) must not be a group slot (it is an `Int`). There is **no** `rewrite_op` change — the append callee stays `vector$append`; the typed swap is the emitter's job (Phase 3).

- [ ] **Step 4: Rebuild.** Run: `make bundle-cli` — `Fixed point reached`.

- [ ] **Step 5: Fmt + commit.**

```bash
target/twk fmt boot/compiler/backend/route_typed_vec.tw && target/twk lint boot/main.tw
git add boot/compiler/backend/route_typed_vec.tw
git commit -m "feat(typed-vec): group-include + accept in-region append (i64 only)"
```

---

## Phase 3: Emit — swap `rt_arr__push → rt_arr__push_i64` by base type

Goal: an append whose base is physically `PVecI64` emits the typed push.

**Files:** `boot/compiler/codegen/emit.tw`.

- [ ] **Step 1: Branch the append emitter on `mono_key`.** In `emit_intrinsic_vector_append` (`emit.tw:2531`), read the base valtype with the **free function** `atom_val_type(args[0], ctx)` — this emitter has no `fns` or `result_vt` param (see the nearby `emit_erased_container_ingress` at `emit.tw:2527`), so do NOT write `fns.atom_val_type`. Then branch three ways on the family's `mono_key` (no `push_call` field):

```
base_vt := atom_val_type(args[0], ctx)
case pvec_family_of(base_vt) {
  .Some(fam) => if fam.mono_key == "vec_i64" {
    buf4.append(.Call("rt_arr__push_i64")).append(.LocalSet(result_idx))  // typed persistent append
  } else {
    // typed non-i64 base (e.g. PVecBool) must NOT reach here: routing rejects bool append
    // (i64-guarded), so this is a routing bug. Boxed-fallback would be type-unsound
    // (PVecBool ref into a boxed-PVec-expecting rt_arr__push). Fail loud.
    error("append emitter reached a typed non-i64 vector base (${fam.mono_key}); routing should have demoted it")
  },
  .None => buf4.append(.Call("rt_arr__push")).append(.LocalSet(result_idx)),  // genuinely boxed base
}
```

The element is already boxed on the stack (the emitter boxes it for `rt_arr__push`); `push_i64` unboxes internally, so the element-prep code is unchanged. This preserves the boxed path exactly for a boxed base (`.None`) and only diverges for a proven `PVecI64` base.

- [ ] **Step 2: Rebuild.** Run: `make bundle-cli` — `Fixed point reached`.

- [ ] **Step 3: Verify in WAT.** Create `/tmp/mva.tw`:

```
fn f(n: Int) Vector<Int> {
  xs := collect i in range(n) { i }
  xs[0] = 42
  xs = xs.append(99)
  xs
}
println(f(5).len().to_string())
```

Run: `target/twk wat /tmp/mva.tw --func "f307_f"` (adjust via `--list`). Expected: result `(ref null $rt_types__PVecI64)`; exact call tokens `$rt_arr__push_i64` (the append), `$rt_arr__set_in_place_i64` (the owned set), `$rt_arr__get_i64` present; boxed `$rt_arr__push` and `$rt_types__PVec` **absent on the `xs` path**. **Boundary-match, do not substring** (`rt_arr__push` ⊂ `rt_arr__push_i64`, `$rt_types__PVec` ⊂ `$rt_types__PVecI64`) — see sub-project A's WAT-assertion discipline: assert absence of `$rt_arr__push)` / `call $rt_arr__push\n` (`push` not followed by `_i64`) and `$rt_types__PVec)` / `$rt_types__PVec ` (`PVec` not `PVecI64`). Do NOT assert absence of `rt_types__BoxedInt` — the appended VALUE is boxed for the ABI (fact 5). If the append still calls boxed `$rt_arr__push` or the vector demoted to boxed `PVec`, recheck Phase 2 (eligibility) and Step 1 (base-type read).

- [ ] **Step 4: Confirm the verifier is quiet.** The append is intrinsic-emitted, so the post-route ABI verifier likely does not inspect it; the clean `make bundle-cli` (full verify) in Step 2 is the check. If verification *did* reject a `push_i64` site, add a narrow `verify_expr.tw` arm (PVecI64 base) mirroring sub-project A's set exception, then rebuild.

- [ ] **Step 5: Fmt + commit.** (Only `emit.tw` — `route_typed_vec.tw` was committed in Phase 2.)

```bash
target/twk fmt boot/compiler/codegen/emit.tw && target/twk lint boot/main.tw
git add boot/compiler/codegen/emit.tw
git commit -m "feat(typed-vec): emit push_i64 for PVecI64-based append"
```

---

## Phase 4: Fixtures, behavioral coverage, gate

**Files:** `boot/tests/suites/typed_vector_write_suite.tw` (extend) — or a sibling append suite.

- [ ] **Step 1: Positive — set + append promotes.** WAT for the `/tmp/mva.tw` shape: `$rt_types__PVecI64` result and exact call tokens `$rt_arr__push_i64`/`$rt_arr__set_in_place_i64`/`$rt_arr__get_i64` present; boxed `$rt_arr__push`/`$rt_arr__get`/`$rt_types__PVec` absent on the vector path — using **boundary-delimited token matching** (A's WAT-assertion discipline; `rt_arr__push` ⊂ `rt_arr__push_i64`). Not "no `BoxedInt`" — the appended value is boxed by ABI.

- [ ] **Step 2: Positive — append across the tail boundary.** Execution assertion: a `collect` of exactly `32` (`rt_BF`, tail-full) then `xs.append(v)` (forces `promote_full_tail_i64` + fresh tail); read back `xs[32] == v` and `xs.len() == 33`. Also cross the **new-root** boundary: with `rt_BF = 32` / `rt_BITS = 5`, a single internal level holds `rt_BF²  = 1024` trie elements plus a `32` tail = `1056` before a new root level is needed; so `collect` exactly **`1056`** then `xs.append(v)` promotes a full `32`-tail into a full `1024`-trie → new root (`push_tail`/root-growth on the typed family). Assert `xs[1056] == v` and `xs.len() == 1057`. (Do NOT use `1024` — that does not cross the boundary. The formula is `rt_BF + rt_BF² = 1056`.)

- [ ] **Step 3: Positive — append-only still boxed (unchanged scope guard).** A pure append-accumulator loop (`acc=[]; for { acc=acc.append(i) }`) with **no** indexed set must remain on the boxed builder path (this is builder-region territory, out of scope). WAT shows no `push_i64` and no `PVecI64` for that function — confirms B did not accidentally capture the standalone-append case.

- [ ] **Step 4: Negatives stay boxed.** Append on a `Vector<Float>`; append where the vector then escapes to a call/record. WAT shows no `push_i64`/`PVecI64`.

- [ ] **Step 5: Kill-switch.** With `TWINKLE_TYPED_VEC_WRITE=0`, the Step-1 shape stays fully boxed (append included).

- [ ] **Step 6: Run.** `TWK_TEST_FILTER="typed vector" target/twk run boot/tests/main.tw` → all PASS (covers both A's and B's suites).

- [ ] **Step 7: Self-host + regression gate.** `make bundle-cli` → `Fixed point reached`; `make boot-test` → green; `make rust-test` → green; `--census --sites` diff confined to newly-typed append sites.

- [ ] **Step 8: Perf.** Extend sub-project A's bench with a set+append shape (`collect` + `xs[i]=v` loop + a few appends + read loop + return); flag-off vs flag-on. Realistic expectation (fact 5 — the element stays boxed on both paths): the append side should **at least match** boxed `push` (the persistent trie work is the same; typed removes the boxed-leaf storage, not the value box), and the read/set side keeps A's typed win. Do not claim the append "removes BoxedInt alloc/unbox" — the appended value is boxed for the ABI and unboxed inside `push_i64` on both paths. If flag-on regresses the append, diagnose (likely a demotion to boxed elsewhere in the group).

- [ ] **Step 9: Fmt + commit.**

```bash
target/twk fmt boot/tests/suites/*.tw boot/bench/*.tw
git add boot/tests/suites/ boot/bench/
git commit -m "test(typed-vec): set+append promotion fixtures + gate + perf"
```

---

## Out of scope (this sub-project)

- **In-place append.** Append stays persistent (COW tail copy); there is no `append_in_place_i64`. Owned append *loops* remain the builder-region pass's job.
- Standalone append-only accumulators (no indexed set) — stay on the boxed builder (Phase 4 Step 3 guards this).
- `Bool`/`Float`/`Byte` families; relaxed vectors (escape/demote, unchanged); `stage0` mirror (not needed — codegen optimization).
