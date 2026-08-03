# rt.arr Family Deduplication Implementation Plan

> **COMPLETED 2026-08-03** (branch `refactor/rt-arr-family-dedup`, merged to main).
> Outcome — the audit found the plan's original gate unworkable and several
> "folds" to be genuine divergences, so the shape differs from the task list:
> - **Gate corrected first:** `twk wat --func <name>` substring-matches boot's
>   self-hosted WAT (capturing the compiler's own builder functions the fold
>   deletes) and half the folded variants are DCE'd, so it can't work. Replaced
>   with an old-vs-new-`twk` diff over only the emitted `rt_arr__*` funcs on a
>   fixed corpus (harness kept in the branch history; see Global Constraints).
> - **Folded (gate byte-identical):** `get_leaf` (+`PVecFamily.suffix`), `do_set`
>   (+`leaf_store`), `set`/`set_in_place`, boxed `builder_append_leaf_range`,
>   `builder_push` (i64+bool), `box` (i64+bool, +`elem_box`).
> - **Kept standalone — genuine divergence, documented in the `PVecFamily` doc
>   block in `arr.tw`:** `promote_full_tail` & `gather` (relaxed-RRB / no boxed
>   raw-pusher), `push`/`push_i64` (relaxed-vs-radix + different local model),
>   boxed `builder_push` (no unbox scratch local), `unbox_i64`/`unbox_bool`
>   (different push conventions, never emitted so ungatable).
> - **Delivered for `mutvec-later-slices`:** `PVecFamily` now carries `suffix`,
>   `leaf_store`, and `elem_box` — its hard prerequisite is met (Phase 4 still
>   needs the separate `PVecF64`). NOTE: that plan's Phase 1 gate has the same
>   `twk wat --func` flaw corrected here; use the `rt_arr__`-only diff instead.
>
> Historical task-by-task plan follows.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the per-element-type copy-paste in `boot/compiler/codegen/runtime/arr.tw` by folding the hand-duplicated core trie ops (`get_leaf`, `do_set`, `set`, `set_in_place`, `push`/`push_tail`/`new_path`, `builder_push`, `box`/`unbox`) into the existing `PVecFamily`-parameterized builder pattern, and migrating the grandfathered boxed family onto the family builders it currently bypasses.

**Architecture:** `PVecFamily` (`arr.tw:1378`) already records every per-element-type difference the duplicated functions actually use — `pvec_ty`/`arr_ty` (type names), `pvec_ref`/`arr_ref`/`pvec_null`/`arr_null` (ValTypes), `elem_ty`, `zero_value`. A subset of ops is already factored into `pvec_*_fn(f)` builders registered via inherent-method sugar (`family_i64().pvec_get_fn()`); the core mutating ops were never migrated and exist as `_fn` + `_i64_fn` + `_bool_fn` copies. This plan finishes that migration one op at a time, each fold gated by **byte-identical emitted WAT** so it is provably behavior-preserving. `MutVecI64` (a flat, non-trie structure), the boxed-only RRB structural ops (`slice`/`concat`/`drop_last`/`pop_tail`), and the `VecElem`/`sort_typed` axis are **left as-is** — they are not duplication.

**Tech Stack:** Boot compiler (Twinkle in `boot/`), Wasm-GC runtime emission (`boot/compiler/codegen/runtime/arr.tw`), each `*_fn() FuncDef` builder emits raw Wasm IR (`Instr` vectors) assembled by `module()` (`arr.tw:146`).

## Global Constraints

- **The acceptance gate for every fold is byte-identical emitted `rt_arr__*` runtime.** A fold that changes emitted bytes is wrong even if tests pass — this is what makes the refactor provably a no-op.
  - **Do not use `twk wat boot/main.tw --func <name>` as the gate.** `--func` is a *substring* match over boot's self-hosted WAT, so each dump mixes the emitted runtime (`rt_arr__get_leaf`) with the compiler's *own* builder functions (`get_leaf_fn`, `get_leaf_i64_fn`) that this refactor deletes — the diff is guaranteed non-empty for reasons unrelated to codegen. Worse, ~half the folded runtime variants (`get_leaf_i64`, `set_i64`, `do_set_i64`, `unbox_i64`, `unbox_bool`, …) are **never emitted** by `boot/main.tw` (DCE'd; the compiler's typed-vector/in-place/boxing decisions route around them), so there is no per-function artifact to diff.
  - **Correct gate:** keep the pre-refactor `twk` binary (`cp target/twk /tmp/twk.prerefactor` before starting). After each fold + `make bundle-cli`, compile a fixed corpus (`boot/main.tw` plus a mixed boxed/i64/bool probe) with **old vs new twk** and diff **only the `rt_arr__*` emitted functions** (sorted by name — calls are symbolic so `module()` reordering is invisible; the `user__*_fn` compiler builders are excluded because they legitimately change). Harness: `scratchpad/rtarr_gate.sh` → `PASS` means byte-identical. Byte-identical emitted runtime = the fold is a proven no-op.
  - **Coverage caveat:** variants that can't be force-emitted are not directly gate-diffable. Their correctness rests on (a) the *boxed* instance of the same shared builder being byte-identical, (b) `make boot-test`, and (c) the self-host fixed point. This is a weaker guarantee for `*_i64`/`*_bool`-only substitutions than a direct diff — read the source substitution carefully for those.
- Behavioral gates in addition to WAT-identity: `make boot-test` passes, and self-host reaches a fixed point (`make bundle-cli` converges).
- **This is `arr.tw` only.** Stage0's `src/runtime/arr.rs` is an *independent* emitter (a correctness reference), not generated from `arr.tw`; do **not** touch it. The two only need to stay behaviorally equivalent, which byte-identical WAT guarantees.
- **Do not touch** the `MutVecI64` cluster (`mutvec_*_i64_fn`), the boxed-only RRB structural ops (`slice_*`, `concat*`, `drop_last_fn`, `pop_tail_fn`, `append_centre_fn`, `wrap_leaf_fn`), or the `VecElem`/`sort_typed_fn` axis. They are intentionally separate.
- After editing `arr.tw`, run `target/twk fmt boot/compiler/codegen/runtime/arr.tw` and `target/twk lint boot/main.tw`.
- Rebuild with `make bundle-cli`; `make quick-bundle-cli` only when `target/boot.wasm` is already fresh.
- Run heavy verification (`make bundle-cli`, `make boot-test`, self-host) **one at a time, never concurrently or backgrounded**.
- Each task deletes the copies it replaces and updates the `module()` registration list (`arr.tw:146`) in the same commit, so the tree never has both the old and new builder registered.

---

## How to fold (the canonical transformation — read once)

Every op in Tasks 3–7 is the same mechanical move; Task 1 proves it on the simplest case with full code. For an op `foo` currently existing as `foo_fn()` (boxed) / `foo_i64_fn()` / `foo_bool_fn()`:

1. Write one `pvec_foo_fn(f: PVecFamily) FuncDef` (inherent-method form: first param `f`), copying the boxed body but replacing the *type-specific* pieces with family fields:
   - `pvec_ref()` / `pvec_null()` → `f.pvec_ref` / `f.pvec_null`
   - `arr_ref()` / `arr_null()` → `f.arr_ref` / `f.arr_null`
   - `StructGet(t_PVEC, …)` → `StructGet(f.pvec_ty, …)` (field-index constants `pv_LEN`/`pv_TAIL`/`pv_ROOT`/`pv_SHIFT` are shared across families — leave them)
   - `RefCast(_, .Named(t_ARRAY))` → `RefCast(_, .Named(f.arr_ty))`
   - the function `name` → `"foo${f.suffix}"` (Task 1 adds `suffix` to `PVecFamily`)
2. In `module()`, replace `foo_fn()` → `family_boxed().pvec_foo_fn()`, `foo_i64_fn()` → `family_i64().pvec_foo_fn()`, `foo_bool_fn()` → `family_bool().pvec_foo_fn()` (only for families that currently have that variant).
3. Delete `foo_fn` / `foo_i64_fn` / `foo_bool_fn`.
4. Gate: WAT byte-identical for every emitted name, then `make boot-test` + self-host.

**Hints — what to look after before assuming a fold is mechanical:**
- **Internal `Call` recursion/targets.** Recursive ops (`do_set`, `new_path`) and composed ops (`set` → `do_set`+`get_leaf`, `push` → `push_tail`/`new_path`) call other family functions *by name*. In the merged builder every such `.Call("do_set")` must become `.Call("do_set${f.suffix}")`, etc. Missing one silently calls the boxed variant on a typed vector — the WAT diff catches it (a stray `get_leaf` where `get_leaf_i64` is expected).
- **Leaf value store (the one real divergence).** `do_set`/`push`/`builder_push` differ at the *leaf store*: boxed stores an `anyref` as-is; `i64` unboxes to `i64`; `bool` packs to `i32`. `do_set_i64_fn`'s own header comment says so ("only the leaf copy/store uses ArrayI64, and the boxed value is unboxed at the leaf store"). This needs a small family field carrying the leaf-store instruction sequence (see Task 3). `elem_ty` + `zero_value` cover the fresh-leaf/fill cases; the value-conversion step is the new field.
- **Not every op has all three variants.** `bool` may lack a `get_leaf`/`do_set` variant (bool `get` reads packed bits via `pvec_get_fn`); fold only the variants that actually exist. Grep before assuming.
- **The empty-string sentinels in `family_boxed()`** (`builder_push_raw_name: ""`, `promote_name: ""`, `gather_name: ""`) mark ops where a family builder already exists but boxed still uses a standalone — those are Task 2, not the Task 3–7 folds.

---

## Task 1: Pilot — fold `get_leaf` and introduce `PVecFamily.suffix`

`get_leaf_fn` (`arr.tw:882`) and `get_leaf_i64_fn` (`arr.tw:2542`) are byte-for-byte identical except `pvec_ref/arr_ref` (params/results), four `StructGet(t_PVEC…)` → `t_PVEC_I64`, and the final `RefCast(t_ARRAY)` → `t_ARRAY_I64`. This is the simplest fold and validates the whole methodology.

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw`

**Interfaces:**
- Produces: `PVecFamily.suffix: String` field; `pvec_get_leaf_fn(f: PVecFamily) FuncDef` emitting `get_leaf${f.suffix}`.
- Removes: `get_leaf_fn`, `get_leaf_i64_fn`.

- [ ] **Step 1: Capture the WAT baseline for the pilot.**

```bash
target/twk wat boot/main.tw --func get_leaf     > /tmp/get_leaf.before.wat
target/twk wat boot/main.tw --func get_leaf_i64 > /tmp/get_leaf_i64.before.wat
```

- [ ] **Step 2: Add the `suffix` field to `PVecFamily` and its three constructors.**

In the `PVecFamily` type (`arr.tw:1378`) add `suffix: String,`. In `family_boxed()` set `suffix: "",`; in `family_i64()` set `suffix: "_i64",`; in `family_bool()` set `suffix: "_bool",`.

- [ ] **Step 3: Add `pvec_get_leaf_fn(f)`.**

Add near the other `pvec_*_fn(f)` builders (e.g. after `pvec_get_fn`):

```tw
// get_leaf/get_leaf_i64(vec, idx) -> (leaf_array, leaf_local_idx)
fn pvec_get_leaf_fn(f: PVecFamily) FuncDef {
  p_vec := 0
  p_idx := 1
  l_trie_count := 2
  l_node := 3
  l_level := 4
  l_slot := 5

  .{
    name: "get_leaf${f.suffix}",
    params: [f.pvec_ref, .I32],
    results: [f.arr_ref, .I32],
    locals: [.I32, vec_internal_null(), .I32, .I32],
    body: [
      .LocalGet(p_vec),
      .StructGet(f.pvec_ty, pv_LEN),
      .LocalGet(p_vec),
      .StructGet(f.pvec_ty, pv_TAIL),
      .ArrayLen,
      .I32Sub,
      .LocalSet(l_trie_count),
      .LocalGet(p_idx),
      .LocalGet(l_trie_count),
      .I32GeS,
      .If(.None, [
        .LocalGet(p_vec),
        .StructGet(f.pvec_ty, pv_TAIL),
        .LocalGet(p_idx),
        .LocalGet(l_trie_count),
        .I32Sub,
        .Return,
      ], []),
      .LocalGet(p_vec),
      .StructGet(f.pvec_ty, pv_ROOT),
      .LocalSet(l_node),
      .LocalGet(p_vec),
      .StructGet(f.pvec_ty, pv_SHIFT),
      .LocalSet(l_level),
      .Block("brk", .None, [
        .Loop("lp", .None, [
          .LocalGet(l_level),
          .I32Const(rt_BITS),
          .I32LeS,
          .BrIf("brk"),
          .LocalGet(l_node),
          .RefAsNonNull,
          .LocalGet(p_idx),
          .LocalGet(l_level),
          .Call("vi_nav"),
          .LocalSet(p_idx),
          .LocalSet(l_slot),
          .LocalGet(l_node),
          .RefAsNonNull,
          .StructGet(t_VEC_INTERNAL, vi_CHILDREN),
          .LocalGet(l_slot),
          .ArrayGet(t_VEC_CHILDREN),
          .RefCast(true, .Named(t_VEC_INTERNAL)),
          .LocalSet(l_node),
          .LocalGet(l_level),
          .I32Const(rt_BITS),
          .I32Sub,
          .LocalSet(l_level),
          .Br("lp"),
        ]),
      ]),
      .LocalGet(l_node),
      .RefAsNonNull,
      .LocalGet(p_idx),
      .LocalGet(l_level),
      .Call("vi_nav"),
      .LocalSet(p_idx),
      .LocalSet(l_slot),
      .LocalGet(l_node),
      .RefAsNonNull,
      .StructGet(t_VEC_INTERNAL, vi_CHILDREN),
      .LocalGet(l_slot),
      .ArrayGet(t_VEC_CHILDREN),
      .RefCast(false, .Named(f.arr_ty)),
      .LocalGet(p_idx),
      .I32Const(rt_MASK),
      .I32And,
    ],
  }
}
```

Confirm `bool` has no `get_leaf_bool` variant to fold (grep `get_leaf_bool`); the plan assumes only boxed + i64 exist. If a bool variant exists, include `family_bool().pvec_get_leaf_fn()` in Step 4.

- [ ] **Step 4: Rewire `module()` and delete the copies.**

In `module()` (`arr.tw:146`): replace `get_leaf_fn(),` with `family_boxed().pvec_get_leaf_fn(),` and `get_leaf_i64_fn(),` with `family_i64().pvec_get_leaf_fn(),`. Delete `fn get_leaf_fn()` and `fn get_leaf_i64_fn()`.

- [ ] **Step 5: Prove byte-identical WAT, then behavior.**

```bash
target/twk fmt boot/compiler/codegen/runtime/arr.tw
make bundle-cli
target/twk lint boot/main.tw
target/twk wat boot/main.tw --func get_leaf     | diff - /tmp/get_leaf.before.wat
target/twk wat boot/main.tw --func get_leaf_i64 | diff - /tmp/get_leaf_i64.before.wat
make boot-test
```

Expected: both `diff`s produce **no output**; no lint findings; boot tests pass; self-host fixed point. If either diff is non-empty, the fold changed behavior — fix before committing.

- [ ] **Step 6: Commit.**

```bash
git add boot/compiler/codegen/runtime/arr.tw
git commit -m "rt.arr: fold get_leaf into family builder; add PVecFamily.suffix"
```

---

## Task 2: Migrate boxed onto the family builders it already bypasses

The cheapest chunk: the family builders already exist (written for i64/bool); for some ops boxed still uses a standalone copy, marked by an empty-string sentinel in `family_boxed()`. **A sentinel means one of two things — distinguish them:** (a) boxed *has a standalone duplicate* of the family builder → fold it; or (b) boxed *genuinely does not have that op* (e.g. no boxed `builder_push_raw`) → the sentinel is "not applicable," leave it, nothing to fold. Candidate ops: `promote_full_tail` (standalone `promote_full_tail_fn` @5390 vs `pvec_promote_full_tail_fn` @2165), `gather` (`gather_fn` @4600 vs `pvec_gather_fn` @2275), `builder_append_leaf_range` (`builder_append_leaf_range_fn` @5604 vs `pvec_builder_append_leaf_range_fn` @5773). Fold only case-(a) ops.

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw`

**Interfaces:**
- Produces: `family_boxed()` with populated `promote_name`/`gather_name`/`builder_push_raw_name` (or a documented reason to keep a sentinel).
- Removes: the standalone boxed duplicates that the family builder subsumes.

- [ ] **Step 1: For each sentinel op, diff boxed-standalone vs `family_boxed()`-through-the-family-builder.**

For `promote_full_tail`: temporarily register `family_boxed().pvec_promote_full_tail_fn()` under a scratch name and compare its WAT to `promote_full_tail_fn()`'s. If identical modulo the intended name, boxed is foldable. Repeat for `gather`, `builder_append_leaf_range`. Record any op where the boxed body genuinely diverges (e.g. relaxed-RRB handling the typed builder lacks) — **leave those as standalone and keep the sentinel with a one-line comment explaining why.**

- [ ] **Step 2: Fold each confirmed-identical op.**

Populate the corresponding `*_name` field in `family_boxed()` (e.g. `promote_name: "promote_full_tail"`), replace the standalone call in `module()` with `family_boxed().pvec_<op>_fn()`, and delete the standalone `fn`. Baseline + diff each name exactly as in Task 1 Step 5.

- [ ] **Step 3: Format, prove WAT-identical, rebuild, test.**

```bash
target/twk fmt boot/compiler/codegen/runtime/arr.tw
make bundle-cli
target/twk lint boot/main.tw
# diff each migrated function's WAT against its captured baseline (must be empty)
make boot-test
```

Expected: empty diffs, no lint findings, boot tests pass, self-host fixed point.

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/codegen/runtime/arr.tw
git commit -m "rt.arr: route boxed family through existing family builders"
```

---

## Task 3: Fold `do_set` (introduce the leaf-store family hook)

`do_set_fn` (`arr.tw:1107`) / `do_set_i64_fn` (`arr.tw:2643`). Recursive internal-node navigation is shared; the leaf branch diverges at the value store (boxed stores anyref; i64 unboxes to i64). This task introduces the family field the remaining mutating folds also need.

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw`

**Interfaces:**
- Produces: `PVecFamily.leaf_store: Vector<Instr>` (the instruction sequence that converts the incoming `anyref` value on the stack to the family's leaf element type, e.g. `[]` for boxed, an unbox call for i64) — or the minimal shape the diff proves is needed; `pvec_do_set_fn(f)`.
- Removes: `do_set_fn`, `do_set_i64_fn`.

- [ ] **Step 1: Baseline both.**

```bash
target/twk wat boot/main.tw --func do_set     > /tmp/do_set.before.wat
target/twk wat boot/main.tw --func do_set_i64 > /tmp/do_set_i64.before.wat
```

- [ ] **Step 2: Diff the two bodies to isolate the divergence.**

Confirm the only differences are (a) the family type/ref substitutions from the canonical transformation, (b) the leaf-store value conversion, and (c) the recursive `.Call("do_set")` → `.Call("do_set_i64")` target. This defines the exact shape of `leaf_store` (and whether any other field is needed).

- [ ] **Step 3: Add the family field and `pvec_do_set_fn(f)`.**

Add `leaf_store` to `PVecFamily` and its constructors (boxed: the no-op/identity store sequence used at `do_set_fn`'s leaf; i64: the unbox-then-store sequence from `do_set_i64_fn`). Write `pvec_do_set_fn(f)` applying the canonical transformation plus `f.leaf_store` at the leaf branch and `.Call("do_set${f.suffix}")` for recursion.

- [ ] **Step 4: Rewire `module()`, delete copies, prove WAT-identical.**

Replace `do_set_fn()`/`do_set_i64_fn()` registrations with `family_boxed().pvec_do_set_fn()`/`family_i64().pvec_do_set_fn()`; delete the two `fn`s.

```bash
target/twk fmt boot/compiler/codegen/runtime/arr.tw
make bundle-cli
target/twk lint boot/main.tw
target/twk wat boot/main.tw --func do_set     | diff - /tmp/do_set.before.wat
target/twk wat boot/main.tw --func do_set_i64 | diff - /tmp/do_set_i64.before.wat
make boot-test
```

Expected: empty diffs, no lint findings, boot tests pass, self-host fixed point.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/codegen/runtime/arr.tw
git commit -m "rt.arr: fold do_set into family builder with leaf-store hook"
```

---

## Task 4: Fold `set` and `set_in_place`

`set_fn` (`arr.tw:2421`) / `set_i64_fn` (`arr.tw:2742`); `set_in_place_fn` (`arr.tw:2514`) / `set_in_place_i64_fn` (`arr.tw:2838`). These compose `do_set`/`get_leaf`/leaf-store — thin wrappers, so the fold is the canonical transformation plus retargeting the internal `.Call`s to `${f.suffix}` names (now that `get_leaf` and `do_set` are family-named).

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw`

**Interfaces:**
- Produces: `pvec_set_fn(f)`, `pvec_set_in_place_fn(f)`.
- Removes: `set_fn`, `set_i64_fn`, `set_in_place_fn`, `set_in_place_i64_fn`.

- [ ] **Step 1: Baseline `set`, `set_i64`, `set_in_place`, `set_in_place_i64`.**

```bash
for f in set set_i64 set_in_place set_in_place_i64; do target/twk wat boot/main.tw --func $f > /tmp/$f.before.wat; done
```

- [ ] **Step 2: Fold both ops via the canonical transformation.**

Watch the internal `.Call` targets: `set` calls `do_set`/`get_leaf`/`push` — each must become `${f.suffix}`-named. `set_in_place` mutates the leaf in place, so it also uses `f.leaf_store` (or the same value-conversion). Write `pvec_set_fn(f)` and `pvec_set_in_place_fn(f)`; rewire `module()`; delete the four copies.

- [ ] **Step 3: Format, prove WAT-identical, rebuild, test.**

```bash
target/twk fmt boot/compiler/codegen/runtime/arr.tw
make bundle-cli
target/twk lint boot/main.tw
for f in set set_i64 set_in_place set_in_place_i64; do target/twk wat boot/main.tw --func $f | diff - /tmp/$f.before.wat; done
make boot-test
```

Expected: empty diffs, no lint findings, boot tests pass, self-host fixed point.

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/codegen/runtime/arr.tw
git commit -m "rt.arr: fold set/set_in_place into family builders"
```

---

## Task 5: Fold `push`, `push_tail`, and `new_path`

`push_fn` (`arr.tw:1202`) / `push_i64_fn` (`arr.tw:1768`); `push_tail_fn` (`arr.tw:1030`); `new_path_fn` (`arr.tw:981`). Verify whether `push_tail`/`new_path` have i64/bool variants or are already family-agnostic (they may operate purely on internal nodes and not need folding). Fold only the ones that duplicate per element type.

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw`

**Interfaces:**
- Produces: `pvec_push_fn(f)` (and `pvec_push_tail_fn(f)` / `pvec_new_path_fn(f)` if they have per-element copies).
- Removes: the folded copies.

- [ ] **Step 1: Determine which of the three actually duplicate per element type.**

Grep `push_tail_i64`, `push_tail_bool`, `new_path_i64`, `new_path_bool`. If `push_tail`/`new_path` have no per-element copies, they are shared internal-node helpers — leave them; only fold `push`.

- [ ] **Step 2: Baseline the duplicated push ops.**

```bash
for f in push push_i64; do target/twk wat boot/main.tw --func $f > /tmp/$f.before.wat; done
# add push_tail/new_path variants here only if Step 1 found per-element copies
```

- [ ] **Step 3: Fold via the canonical transformation.**

`push` appends to the tail and, on overflow, allocates a new leaf (`f.zero_value`/`f.arr_ty`) and stores the value (`f.leaf_store`). Retarget internal `.Call("push_tail")`/`.Call("new_path")` to `${f.suffix}` names only if those became family-named in Step 1. Rewire `module()`, delete copies.

- [ ] **Step 4: Format, prove WAT-identical, rebuild, test.**

```bash
target/twk fmt boot/compiler/codegen/runtime/arr.tw
make bundle-cli
target/twk lint boot/main.tw
for f in push push_i64; do target/twk wat boot/main.tw --func $f | diff - /tmp/$f.before.wat; done
make boot-test
```

Expected: empty diffs, no lint findings, boot tests pass, self-host fixed point.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/codegen/runtime/arr.tw
git commit -m "rt.arr: fold push into family builder"
```

---

## Task 6: Fold `builder_push`

`builder_push_fn` (`arr.tw:5514`) / `builder_push_i64_fn` (`arr.tw:2872`) / `builder_push_bool_fn` (`arr.tw:4660`). The builder appends into the transient builder's tail/leaves; the leaf store is again the divergence (`f.leaf_store`). Note the existing `pvec_builder_push_raw_fn(f)` (`arr.tw:2333`) — check whether `builder_push` should reuse it rather than reimplement the raw store.

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw`

**Interfaces:**
- Produces: `pvec_builder_push_fn(f)`.
- Removes: `builder_push_fn`, `builder_push_i64_fn`, `builder_push_bool_fn`.

- [ ] **Step 1: Baseline all three.**

```bash
for f in builder_push builder_push_i64 builder_push_bool; do target/twk wat boot/main.tw --func $f > /tmp/$f.before.wat; done
```

- [ ] **Step 2: Fold via the canonical transformation (three families).**

Apply the transformation across boxed/i64/bool; the bool leaf store packs to `i32` (`f.leaf_store` for bool). Retarget any internal `.Call` (e.g. to `builder_push_raw`/`promote`) to `${f.suffix}` names. Rewire `module()` for all three, delete the three copies.

- [ ] **Step 3: Format, prove WAT-identical, rebuild, test.**

```bash
target/twk fmt boot/compiler/codegen/runtime/arr.tw
make bundle-cli
target/twk lint boot/main.tw
for f in builder_push builder_push_i64 builder_push_bool; do target/twk wat boot/main.tw --func $f | diff - /tmp/$f.before.wat; done
make boot-test
```

Expected: empty diffs, no lint findings, boot tests pass, self-host fixed point.

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/codegen/runtime/arr.tw
git commit -m "rt.arr: fold builder_push into family builder"
```

---

## Task 7: Fold `box`/`unbox`

`box_i64_fn` (`arr.tw:2971`) / `unbox_i64_fn` (`arr.tw:3027`); `box_bool_fn` (`arr.tw:4755`) / `unbox_bool_fn` (`arr.tw:4807`). These convert a boxed `PVec` ↔ a typed `PVec` of one family, so they are parameterized by the **typed** family (not boxed). Fold `box_i64`/`box_bool` into `pvec_box_fn(f)` and `unbox_i64`/`unbox_bool` into `pvec_unbox_fn(f)`, driven off `f.pvec_ty`/`f.arr_ty`/`f.elem_ty`/`f.leaf_store` and the reverse conversion.

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw`

**Interfaces:**
- Produces: `pvec_box_fn(f)`, `pvec_unbox_fn(f)` (registered only for typed families: `family_i64()`, `family_bool()`).
- Removes: `box_i64_fn`, `unbox_i64_fn`, `box_bool_fn`, `unbox_bool_fn`.

- [ ] **Step 1: Baseline all four.**

```bash
for f in box_i64 unbox_i64 box_bool unbox_bool; do target/twk wat boot/main.tw --func $f > /tmp/$f.before.wat; done
```

- [ ] **Step 2: Diff i64 vs bool to confirm the family axis.**

Confirm `box_i64` vs `box_bool` differ only by family fields (`pvec_ty`/`arr_ty`/`elem_ty` and the boxed↔element conversion). If the conversion direction needs a second field (element→anyref for `box` is the inverse of `leaf_store`), add it (e.g. `elem_box: Vector<Instr>`).

- [ ] **Step 3: Fold and rewire.**

Write `pvec_box_fn(f)`/`pvec_unbox_fn(f)`; register `family_i64().pvec_box_fn()` etc.; delete the four copies. Boxed family is not registered for these (it has nothing to box to itself).

- [ ] **Step 4: Format, prove WAT-identical, rebuild, test.**

```bash
target/twk fmt boot/compiler/codegen/runtime/arr.tw
make bundle-cli
target/twk lint boot/main.tw
for f in box_i64 unbox_i64 box_bool unbox_bool; do target/twk wat boot/main.tw --func $f | diff - /tmp/$f.before.wat; done
make boot-test
```

Expected: empty diffs, no lint findings, boot tests pass, self-host fixed point.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/codegen/runtime/arr.tw
git commit -m "rt.arr: fold box/unbox into family builders"
```

---

## Task 8: Final sweep, registration tidy, and boundary comments

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw`

- [ ] **Step 1: Confirm no orphaned helpers or stale registrations.**

```bash
target/twk lint boot/main.tw          # flags unused private fns
rg -n "_i64_fn\(\)|_bool_fn\(\)" boot/compiler/codegen/runtime/arr.tw
```

Expect the remaining `_i64_fn`/`_bool_fn` matches to be only the intentionally-separate `mutvec_*_i64_fn` cluster. Remove any now-dead helper the folds orphaned.

- [ ] **Step 2: Record the intentional separations.**

Add a short comment block near the top (or at each cluster) stating what is deliberately *not* family-folded and why: `MutVecI64` (flat non-trie structure), the boxed-only RRB structural ops (`slice`/`concat`/`drop_last`/`pop_tail`, typed vectors rebuild instead), and `VecElem`/`sort_typed` (a comparison-type axis, not a storage-family axis). This stops a future reader from "finishing" a fold that was left separate on purpose.

- [ ] **Step 3: Full validation.**

```bash
target/twk fmt boot/compiler/codegen/runtime/arr.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
make rust-test
```

Expected: self-host fixed point, boot tests pass, Rust tests pass (stage0 unaffected — `arr.rs` untouched).

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/codegen/runtime/arr.tw
git commit -m "rt.arr: sweep dead helpers; document intentional non-folds"
```

---

## Risks & rollback

- **A fold changes emitted bytes.** The per-function WAT diff is the guard — a non-empty diff means stop, do not commit. Most folds are pure type-name substitution and will be byte-identical; the leaf-store ops (Tasks 3–7) are where a wrong `f.leaf_store` or a missed `.Call` retarget shows up as a diff. This is by design: the gate makes the mistake visible before it lands.
- **A boxed op genuinely diverges** (relaxed-RRB handling the typed builders lack). If Task 2's diff shows real divergence, **leave that op standalone** and keep its sentinel with a comment — a partial fold is fine; forcing a merge that isn't there is the actual risk.
- **`bool`/`push_tail`/`new_path` variants may not exist.** Each task greps first and folds only real duplicates; a family without a given variant is simply not registered for it.
- **Rollback.** Each task is a self-contained, WAT-identical commit. Revert the latest task commit to restore the prior state with zero behavior change — nothing in this plan alters runtime semantics, only how the identical Wasm is generated.

## Out of scope

- Touching stage0 `src/runtime/arr.rs` (independent emitter; not generated from this file).
- The `MutVecI64` cluster, the boxed-only RRB structural ops, and the `VecElem`/`sort_typed` axis.
- Adding a new element family (e.g. `PVecF64`) — this plan only removes duplication for the families that exist; adding one afterward is the payoff, not part of this work.
- Any change to the compiler's representation *decisions* (that is the separate physical-repr-ownership endeavor); this is runtime-emission DRY only.
