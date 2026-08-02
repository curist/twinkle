# Typed Vector Indexed-Write Promotion (Sub-project A) Implementation Plan

> **LANDED (branch `storage-track-mutvec`).** All phases done: runtime ops, i64-guarded routing (accept set + rebind, group-include the write result, swap `set_unsafe → set_i64`), emit-time decision remap to `set_in_place_i64` for owned sites, and `TWINKLE_TYPED_VEC_WRITE` kill-switch. Phase 4 (verifier) was a no-op — the generic arg-repr check already accepts the typed set on a PVecI64 base (boot itself emits `set_in_place_i64` at 3 sites, verified under full self-host). Gate: self-host fixed point + 3364 boot tests green + zero `src/` changes. Perf (n=1048576): typed vs boxed ~4× build, ~4–5× write, ~3× read. Two documented boundaries: (1) a `collect`-born producer keeps a **boxed return ABI** (pre-existing `return_atom_slots` loop-diving, identical flag-off; `Vector.make` producers get a typed return) — see the Phase-2 Step-8 caveat; (2) Phase-6 Step-4 census diff was skipped (superseded by the full-suite + self-host + flag-comparison gates). Sub-project **B** (`2026-08-02-typed-vector-append.md`) builds on this.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a `Vector<Int>` that carries an indexed update (`xs[i] = v`) be promoted to the typed flat `PVecI64` representation — today the indexed write disqualifies it and it stays boxed. After this, `collect`/`Vector.make` producers with `xs[i]=v` + reads + return get typed reads (`get_i64`), typed writes (`set_i64`/`set_in_place_i64`), and a `PVecI64` return ABI — no boxed vector ops (`rt_arr__get`/boxed `set_in_place`) and flat `i64` leaf storage instead of a `BoxedInt` pointer-chase. (The set *value* is still boxed at the ABI boundary and unboxed inside the op — fact 5.)

**Architecture:** Two concerns compose across two stages. **Typedness** is decided in `route_typed_vec` (backend prepare): pull the indexed-set *result* slot into the vector group, accept the set and its rebind as non-escaping, and swap the set call to a typed `set_i64` — all restricted to the `vec_i64` family. **Ownership** stays with the emit-time mutable-decision selector (`select_call_for_emit`), which emits the in-place `set_in_place_i64` for proven-owned sites — but because the ownership decision was recorded pre-route against `set_unsafe`, routing must **remap** each swapped site's decision to the `_i64` targets (the selector has a `persistent_matches` equality gate; cataloging alone is not enough). Four new runtime ops (`get_leaf_i64`, `set_in_place_i64`, `do_set_i64`, `set_i64`) are mechanical mirrors of the boxed ops with `i64` leaves (locals included), and the boxed path stays byte-identical. Routing runs on-by-default, so the gate is self-host fixed point + full suite, not byte-identity; a `TWINKLE_TYPED_VEC_WRITE` kill-switch bisects regressions.

**Tech Stack:** Boot compiler (self-hosted Twinkle in `boot/`), hand-written Wasm-GC runtime IR (`boot/compiler/codegen/runtime/arr.tw`), backend representation-routing pass (`boot/compiler/backend/route_typed_vec.tw`, `typed_param_abi.tw`), emit-time mutable-op selector (`boot/compiler/codegen/emit/mutable_sites.tw`, `mutable_catalog.tw`, `opt/semantics.tw`).

**Design source:** the `project-typed-vector-repr` track. This is the "typed writes" extension the S2.0 note deferred ("indexed-update vectors stay boxed"). Sub-project **B** (`2026-08-02-typed-vector-append.md`) adds typed persistent append on top of this.

---

## Critical facts (verified against the tree, 2026-08-02)

Read these before starting; they are the load-bearing grounding for every task.

1. **`route_typed_vec` runs ON by default — this changes boot's own codegen.** It runs unconditionally inside `prepare_backend_with_mutable_decisions` (`backend/prepare.tw:178`). Unlike the flag-gated mutvec work, there is **no flag-off byte-identity guarantee**: boot source contains `xs[i]=v` vectors that will newly promote. The regression gate is therefore **self-host fixed point + full boot suite green + a Rust-suite check**, not byte-identity. A targeted kill-switch (`TWINKLE_TYPED_VEC_WRITE=0`, default on) is added in Task 6 so a regression can be bisected/rolled back.

2. **Why an indexed-update vector stays boxed today.** `route_typed_vec.classify_op` (`route_typed_vec.tw:1416`) whitelists only `len(v)`, `gather(v,i)`, index-*read* (`AIndex .Array`, base allowed), and direct user calls as non-escaping. A `set_unsafe(v,i,x)` call falls through to the else arm (`route_typed_vec.tw:1439`), where `atoms_contain_any(args, vs)` is true (v is `args[0]`), so the group **escapes** → not eligible → boxed. This is the single gate this sub-project opens.

3. **The set op is a builtin CALL, swapped by routing (like `len`), not by emit-by-base-type (like the `xs[i]` read).** `xs[i]=v` lowers to `ACall(vector$set_unsafe, [v, i, x])` (confirmed by dumping `--opt`: `call Fn25(L0, 1, 9)` where `Fn25 = set_unsafe`). Routing swaps callee ids in `rewrite_op` (`route_typed_vec.tw:2256`) via a `cond` on `ids.*`/`fi.*` — e.g. `fid.id == ids.len ... => .Some(fi.len)`. We add a `set_unsafe → set_i64` arm there. The `xs[i]` *read* is a different node (`AIndex`), already handled in `emit/arrays.tw:86` (`emit_index_op`, `pvec_family_of(base_vt) → fam.get_call`); do not touch it.

4. **Two passes want to rewrite the set; composing them needs a decision REMAP (not just cataloging).** At routing time (prepare) the op is always `set_unsafe`; the `set_unsafe → set_in_place` in-place swap is an **emit-time** decision (`emit.tw:1858-1872` → `mutable_sites.select_call_for_emit`). Routing swaps `set_unsafe → set_i64`. **Cataloging `set_i64` alone is NOT sufficient** — verified in `mutable_select.tw:301-308`: `select_call_with_policy` has a `persistent_matches` gate that rejects (`.WrongPersistentTarget`) when the decision's recorded `d.persistent_func` (produced pre-route as `set_unsafe`) ≠ the current callee (`set_i64`); and even past that gate, `d.mutable_func` is the **boxed** `set_in_place` recorded pre-route (`mutable_select.tw:310`), which on a `PVecI64` base is type-unsound. So the composition requires BOTH:
   - (a) **Catalog `set_i64`** so `call_entry_for_persistent(set_i64)` finds an entry (`decision_family: .VectorSet`, `base_arg_index: .Some(0)`) — else `select_call_for_emit` returns `.None` → the owned site emits the functional `set_i64` (a correctness-safe but slower miss). Pairing declared in `opt/semantics.tw:127-131` (`set_unsafe → in_place_equivalent: .Some(set_in_place)`; add a `set_i64` twin); family in `mutable_catalog.tw:55` (`decision_family_for_persistent`); base-arg `mutable_catalog.tw:48` (`VectorSet → 0`).
   - (b) **Remap the decision** for each swapped site: rewrite that site's `MutableDecision.persistent_func` `set_unsafe → set_i64` and `.mutable_func` `set_in_place → set_in_place_i64`. Decisions are keyed by **site** (`MutableDecisionTable.by_site`, `site_key(func.id, local.id)`, `mutable_select.tw:39,89`) where `local` is the set call's **result** local — stable across the callee swap. Routing knows every site it swapped, so the remap is a targeted table transform. This is the step both plan reviews flagged as missing; it is mandatory, not optional.

5. **The set value stays boxed (`anyref`); the typed op unboxes internally.** `vector$set_unsafe`/`set_in_place` ABIs take `.Anyref` for the value (`builtins.tw:149`, `builtins.tw:222`). The typed variants keep `.Anyref` for the value and unbox to `i64` inside (a one-time cost, off the read hot path) — exactly like `builder_push_i64` (`arr.tw:2408`: takes boxed element, `RefCast BoxedInt; StructGet 0`). This means routing swaps **only the callee id**, never the value argument.

6. **Runtime analogues to mirror (leaf types are the only change):**
   - `get_leaf` (`arr.tw:876`): `params [pvec_ref(), .I32] → results [arr_ref(), .I32]`, `StructGet(t_PVEC, ...)`. Returns `(boxed-leaf, slot)`.
   - `set_in_place` (`arr.tw:2376`): `get_leaf` then `array.set(t_ARRAY)`. Trivially small.
   - `set` (`arr.tw:2283`): functional. Tail path copies the tail array + `array.set`; trie path calls `do_set`.
   - `do_set` (`arr.tw:1101`): trie-copy set. **NOT leaf-agnostic** — at the leaf it does `RefCast(t_ARRAY)`, `ArrayNew(t_ARRAY)`, `ArrayCopy(t_ARRAY,t_ARRAY)`, `ArraySet(t_ARRAY)` (`arr.tw:1101` body). So a typed functional set needs a `do_set_i64` twin. Internal-node navigation (`t_VEC_INTERNAL`/`t_VEC_CHILDREN`) is shared/unchanged.
   - `pvec_get_fn`/`pvec_len_fn` (`arr.tw:1463`,`1474`) are **already family-parameterized** over `PVecFamily` — `get`/`get_i64` come from one body. `set`/`set_in_place`/`do_set`/`get_leaf` are currently boxed-only standalone fns. **Add separate `_i64` fns** (do not refactor the boxed ones into family form) to keep the boxed path byte-identical.
   - `family_i64()` (`arr.tw:1418`) is the `PVecFamily` with `pvec_ty`, `arr_ty`, `elem_ty`, `pvec_null`, `get_name`, etc. Constants: `t_PVEC_I64`, `t_ARRAY_I64`, and helpers `pvec_i64_null()`/`pvec_i64_ref()`/`arr_i64_null()`/`arr_i64_ref()` already exist (used by `builder_push_i64` etc.).

7. **Builtin registration discipline.** New internal ops are registered in two places in `builtins.tw`: a `builtin_abi()` arm (keyed by name, position irrelevant) and a `rt(...)` entry in `builtin_specs()` (**append at the END** — `builtin_specs()` order IS the 0-based FuncId assignment; mid-list insertion shifts every later id and silently breaks codegen while self-host still reaches a self-consistent fixed point, so **only `make boot-test` catches it**). Existing `_i64` ops are registered at `builtins.tw:157-161`/`590-594`.

8. **Rebuild loop.** `target/twk` is the compiled boot compiler. After editing any `boot/` **compiler** source you MUST `make bundle-cli` (rebuild `target/boot.wasm` via self-host, then `target/twk`; must print `Fixed point reached`) before behavior changes. Editing only a **test suite** file needs no rebuild — the suite compiles the current compiler source from disk when `target/twk run boot/tests/main.tw` runs. Runtime-op bodies: `TWINKLE_VERIFY_LEVEL=basic` dumps codegen even when the backend verifier rejects. After editing any `.tw` file: `target/twk fmt <file>` then `target/twk lint boot/main.tw` (or the relevant entry).

9. **The load-bearing routing change: the write RESULT must join the group, and the REBIND must not escape.** Accepting the set in `classify_op` is necessary but **not sufficient** — verified by dumping the real ANF for the plan's own example:

   ```
   let L43: Vector<Int> = call Fn33(L1)        // builder_freeze  → candidate v; vs = {L43}
   let L9:  Vector<Int> = init L43             // xs (plain AInit copy) → vs = {L43, L9}
   let L44: Vector<Int> = call Fn25(L9, 0, 42) // set_unsafe(xs,0,42) — result L44 bound by ACall
   let L45: Void        = assign L9 = L44      // REBIND xs
   ...reads of L9..., return L9
   ```

   Two problems, both must be fixed or nothing promotes (and Phase 2's WAT check fails):
   - **(i) The result slot is never in the group.** `build_copy_map` (`route_typed_vec.tw:1033`) records only plain copies `Let(dst, AInit(ASlot(src)))`. `L44` is `ACall`-bound, so `aliases_for(L43) = {L43, L9}` **excludes `L44`**; the only result-producing fixpoint in `compute_eligible_v` (`:247-463`) is `collect_gather_results` (`:449`, for `gather`). So `L44`'s slot stays boxed `PVec` while `set_i64`'s ABI result is `PVecI64` → an un-coerced typed→boxed store. **Fix:** add a monotone fixpoint `collect_write_results` (modeled on `collect_gather_results`) that adds the result slot of every `ACall(set_unsafe, [base,…])` with `base ∈ vs` to `eligible_v`, iterated to fixpoint (a write result may itself be a base). Include these slots wherever `collect_gather_results` output is included.
   - **(ii) The rebind escapes.** `classify_op`'s `.AAssign(dst, a)` arm (`:1589`) is `use_esc(slot_in(a, vs) or vs.has(dst.id))` — designed to disqualify `acc = acc.take(...)` (source is a boxed combinator result). Here `dst = L9 ∈ vs`, so it escapes → `v_group_typeable` fails → `eligible_v` empty → `rewrite_op`'s `arg0_in(args, eligible_v)` is false → `set_unsafe` is **not even swapped**. **Fix:** revise the arm to a narrow exception — non-escaping when `vs.has(dst.id)` **and** the source `a` is a `collect_write_results` slot (an accepted in-region write result whose base ∈ vs); otherwise keep the original `use_esc(slot_in(a, vs) or vs.has(dst.id))`. This preserves the `take`/relaxed disqualification (a `take` result is not a write-result slot) while allowing the legitimate in-place rebind. Thread the write-result set into `classify_op`/`classify_expr` (it is computed once per function, alongside the alias set).

10. **`Int`-only guard is mandatory — routing iterates ALL families.** `route_typed_vectors` runs per-family over `all_families()` (`elem_family.tw:19`), which is `[i64, bool]`. `classify_op` matches the set by the **family-agnostic** boxed `set_unsafe` id. Without a guard: if `FamilyIds.set` is populated as `builtins.id("vector$set${suffix}")`, the bool iteration calls `builtins.id("vector$set_bool")` → `error("unknown builtin")` (`builtins.tw:377`) → **compiler crash on every build**; if hardcoded to `set_i64`, a `Vector<Bool>` `xs[i]=v` swaps to `set_i64` → **type-unsound codegen**. **Fix:** gate the new `classify_op` acceptance, the `collect_write_results` fixpoint, and the `rewrite_op` swap on `fam.mono_key == "vec_i64"` (bool has no typed set in this sub-project). Populate `FamilyIds.set` only for the i64 family; leave it a sentinel (`-1`) for bool so the `rewrite_op` arm cannot match.

---

## File Structure

**Create:**
- `boot/tests/suites/typed_vector_write_suite.tw` — routing/promotion fixtures (WAT-level: a promoted function shows `set_i64`/`set_in_place_i64`/`get_i64` and a `$rt_types__PVecI64` result type, and **no boxed vector ops** — no `rt_arr__get`, no boxed `rt_arr__set_in_place`, no `$rt_types__PVec` on the vector path; NOT "no `BoxedInt`", fact 5; negatives stay boxed) plus runtime-behavior fixtures (in-place mutation, functional copy, OOB traps).

**Modify:**
- `boot/compiler/codegen/runtime/arr.tw` — add `get_leaf_i64_fn`, `set_in_place_i64_fn`, `do_set_i64_fn`, `set_i64_fn`; register them in the `module()` func list (near the other `_i64` ops, after `mutvec_freeze_i64_fn()`).
- `boot/compiler/builtins.tw` — `builtin_abi()` arms for `vector$set_i64` + `vector$set_in_place_i64`; `rt(...)` specs appended at end of `builtin_specs()`.
- `boot/compiler/opt/semantics.tw` — a call-spec for `vector$set_i64` with `in_place_equivalent: .Some(b.id("vector$set_in_place_i64"))`, mirroring the `set_unsafe` entry at `:131`.
- `boot/compiler/codegen/mutable_catalog.tw` — `decision_family_for_persistent` (`:55`) returns `.VectorSet` for `vector$set_i64`; `base_arg_index` mapping already covers `VectorSet → 0` (`:48`, no change).
- `boot/compiler/backend/route_typed_vec.tw` — `RouteIds` gains `set_unsafe`; **`FamilyIds`** (`route_typed_vec.tw:58`, the struct `rewrite_op` reads via `fi.*` — NOT `ElemFamily`) gains `set` (the typed functional id, populated only for the i64 family, `-1` for bool); `route_ids()` populates them (zeroing `set_unsafe` when the kill-switch is off); a new `collect_write_results` fixpoint (fact 9-i); `classify_op` (`:1416`) accepts the i64 indexed set as non-escaping and its `.AAssign` arm (`:1589`) gains the write-result exception (fact 9-ii); `rewrite_op` (`:2256`) swaps `set_unsafe → fi.set` (i64 only). Routing also **reports the swapped set sites** (func id + result local) for the decision remap.
- `boot/compiler/backend/prepare.tw` — thread the `mutable_decisions` table through the routing step and **remap** the swapped sites' decisions (`persistent_func → set_i64`, `mutable_func → set_in_place_i64`); store the remapped table into the `PreparedModule` (fact 4-b). (Today `route_typed_vectors` is called at `prepare.tw:178` without the decision table and the table is passed through unchanged at `:201`; both change.)
- `boot/compiler/backend/verify_expr.tw` — accept a `set_i64`/`set_in_place_i64` call whose base slot is `PVecI64` (mirror the existing `get_i64`/PVecI64-slot exception; `verify_expr.tw` already handles PVecI64 read/field sites at `:730,:778,:1100`). Conditional — only if `make bundle-cli` flags it.
- `boot/compiler/codegen/mutable_catalog.tw` — also update `family_for_persistent` (`:66`) to return `"vector_set"` for `set_i64` (census/dry-run reporting; the in-place swap itself uses the `decision_family` enum, not this string).
- `boot/tests/main.tw` — register the new suite.

**Do NOT modify:** the boxed `set`/`set_in_place`/`do_set`/`get_leaf` fns, `emit/arrays.tw`'s `emit_index_op` read path, or any `src/` (stage0) — this is a boot-only codegen optimization; stage0 just compiles the boot source and does not run routing (no stage0 parity needed; see fact 1).

---

## Phase 1: Runtime substrate (`get_leaf_i64`, `set_in_place_i64`, `do_set_i64`, `set_i64`)

Goal: four typed `i64` runtime ops exist, registered as internal builtins with typed ABI, verified by self-host + a runtime-behavior suite. Each is a mirror of a named boxed analogue with only the leaf/struct types changed.

**Files:** `boot/compiler/codegen/runtime/arr.tw`, `boot/compiler/builtins.tw`, `boot/tests/suites/typed_vector_write_suite.tw`, `boot/tests/main.tw`.

- [ ] **Step 1: Add `get_leaf_i64_fn`** — copy `get_leaf_fn` (`arr.tw:876`), rename to `get_leaf_i64`, change `params` to `[pvec_i64_ref(), .I32]`, `results` to `[arr_i64_ref(), .I32]`, and every `StructGet(t_PVEC, …)` to `StructGet(t_PVEC_I64, …)`. Internal-node navigation (`vec_internal`, `t_VEC_INTERNAL`, `t_VEC_CHILDREN`) is unchanged — children are shared `ref eq`; only the tail/leaf come from the `PVecI64` struct and are `ArrayI64`. The returned leaf is the family's `arr_i64` type.

- [ ] **Step 2: Add `set_in_place_i64_fn`** — copy `set_in_place_fn` (`arr.tw:2376`), rename to `set_in_place_i64`, `params [pvec_i64_null(), .I32, .Anyref]`, `results [pvec_i64_ref()]`. Body: `LocalGet(vec); RefAsNonNull; LocalGet(idx); Call("get_leaf_i64")` (leaves `(ArrayI64 leaf, slot)` on the stack), then unbox the value and store: `LocalGet(val); RefCast(false, .Named(t_BOXED_INT)); StructGet(t_BOXED_INT, 0)` (→ i64), `ArraySet(t_ARRAY_I64)`, then `LocalGet(vec); RefAsNonNull` to return the same vec. (Compare boxed `set_in_place`, which does `LocalGet(val); ArraySet(t_ARRAY)` with no unbox because the boxed leaf holds anyref directly.)

- [ ] **Step 3: Add `do_set_i64_fn`** — copy `do_set_fn` (`arr.tw:1101`), rename to `do_set_i64`. Keep all internal-node handling (`t_VEC_INTERNAL`, `t_VEC_CHILDREN`, `Eq` casts) identical. **Change the leaf-copy LOCALS too:** any `locals` entry typed `arr_null()`/`arr_ref()` that holds a copied *leaf* becomes `arr_i64_null()`/`arr_i64_ref()` (do not only change the ops — a boxed local type with an `ArrayI64` value is a verifier error). At the **leaf** branch, change `RefCast(false, .Named(t_ARRAY)) → RefCast(false, .Named(t_ARRAY_I64))`, `ArrayNew(t_ARRAY) → ArrayNew(t_ARRAY_I64)`, `ArrayCopy(t_ARRAY, t_ARRAY) → ArrayCopy(t_ARRAY_I64, t_ARRAY_I64)`, and the final `ArraySet(t_ARRAY)`: unbox first (`RefCast BoxedInt; StructGet 0`) then `ArraySet(t_ARRAY_I64)`. The value parameter stays `.Anyref` and is unboxed at the leaf store; recursive calls thread it unchanged.

- [ ] **Step 4: Add `set_i64_fn`** — copy `set_fn` (`arr.tw:2283`), rename to `set_i64`, `params [pvec_i64_null(), .I32, .Anyref]`, `results [pvec_i64_ref()]`. **Change the LOCALS:** `set_fn`'s `locals` is `[.I32, arr_null()]` — the `arr_null()` (the copied new-tail, `L4`) becomes `arr_i64_null()`. Tail path: replace `t_PVEC → t_PVEC_I64`, `t_ARRAY → t_ARRAY_I64` for the tail copy/new, and unbox the value before the tail `ArraySet(t_ARRAY_I64)`. Trie path: replace `t_PVEC → t_PVEC_I64` struct gets and `Call("do_set") → Call("do_set_i64")`; the `do_set_i64` call takes the still-boxed `.Anyref` value (unboxing happens inside at the leaf). `StructNew(t_PVEC) → StructNew(t_PVEC_I64)`.

- [ ] **Step 5: Register the four in `module()`** — add `get_leaf_i64_fn()`, `set_in_place_i64_fn()`, `do_set_i64_fn()`, `set_i64_fn()` to the runtime func list in `arr.tw`'s `module()`, near the existing `_i64` ops (e.g. right after `mutvec_freeze_i64_fn()`, `arr.tw:197`). Order among runtime fns is irrelevant (linked by name).

- [ ] **Step 6: Register builtins.** In `builtins.tw` `builtin_abi()` add:
  - `"vector$set_i64" => abi([pvec_i64_n(), .I32, .Anyref], [pvec_i64_()]),`
  - `"vector$set_in_place_i64" => abi([pvec_i64_n(), .I32, .Anyref], [pvec_i64_()]),`
  In `builtin_specs()`, **appended at the very end** (after the last existing `rt(...)`):
  - `rt("vector$set_i64", "rt.arr", "set_i64", .None),`
  - `rt("vector$set_in_place_i64", "rt.arr", "set_in_place_i64", .None),`
  `get_leaf_i64`/`do_set_i64` are internal helpers called by name from the `set_*_i64` runtime bodies. Like the boxed `get_leaf`/`do_set` (verified: neither appears in `builtins.tw`), they are **not** registered as builtins at all — being in `module()`'s func list (Step 5) is enough. The Wasm linker keeps them because they are reachable from `set_i64`/`set_in_place_i64`, which are reachable from emitted code. Only `set_i64`/`set_in_place_i64` need builtin entries, because routing emits them as `ACall(AGlobalFunc(...))` and the compiler must resolve their FuncId + ABI.

- [ ] **Step 7: Rebuild.** Run: `make bundle-cli` — must print `Fixed point reached`. This proves the ops assemble and pass the backend verifier as dead code (nothing emits them yet). Expected: clean fixed point.

- [ ] **Step 8: Write runtime-behavior fixtures.** Because nothing routes to these ops yet, they can't be exercised by name from source (internal `.None` ops, and calling one on a boxed PVec would trap on the `ref.cast`). Defer behavioral coverage to Phase 5 (once routing emits them through real promoted regions): in-place mutation reflects in later reads; functional set on a shared vector does not mutate the original; `set_in_place_i64`/`set_i64` OOB traps at logical length; trie-depth set (n > 1024 elements, forcing `do_set_i64`) reads back correctly. Add a placeholder `pub fn suite()` returning an empty-named suite now and register it in `boot/tests/main.tw` so Phase 2+ can grow it.

- [ ] **Step 9: Commit.**

```bash
git add boot/compiler/codegen/runtime/arr.tw boot/compiler/builtins.tw boot/tests/suites/typed_vector_write_suite.tw boot/tests/main.tw
git commit -m "feat(typed-vec): set_i64 / set_in_place_i64 runtime ops (internal, inert)"
```

---

## Phase 2: Routing — group membership, accept the set, swap to `set_i64` (i64-guarded)

Goal: `route_typed_vec` (i) pulls the indexed-set RESULT slot into the vector group, (ii) treats the set and its rebind as non-escaping, and (iii) swaps `set_unsafe → set_i64` — but only for the `vec_i64` family. After this phase a `collect`/`make` + `xs[i]=v` + reads + return function promotes to `PVecI64` (return ABI, `get_i64`, and the **functional** `set_i64`; in-place composition is Phase 3). All new behavior is gated on `fam.mono_key == "vec_i64"` (fact 10). The `TWINKLE_TYPED_VEC_WRITE` kill-switch is implemented **here in Step 1** (not deferred) so this phase's behavior is real and testable; Phase 6 only adds the flag-off regression test + gate.

**Files:** `boot/compiler/backend/route_typed_vec.tw`.

- [ ] **Step 1: Extend `RouteIds`/`FamilyIds` and add the kill-switch.** First add `use @std.proc` to `route_typed_vec.tw`'s imports (verified: it currently imports only `compiler.*` modules, no `proc`). `RouteIds` (built by `route_ids(builtins)`) gains `set_unsafe: Int`. In `route_ids()`, read `proc.env("TWINKLE_TYPED_VEC_WRITE")`: when `.Some("0")`, set `set_unsafe = -1` (an id no real op has, disabling every new branch below — classify accept, `collect_write_results`, the `.AAssign` exception, the `rewrite_op` swap, and site reporting); otherwise `set_unsafe = builtins.id("vector$set_unsafe").id`. `FamilyIds` (`route_typed_vec.tw:58`, read by `rewrite_op` via `fi.*`) gains `set: Int`; populate it `= builtins.id("vector$set_i64").id` **only for the i64 family** and `-1` for the bool family (fact 10). Do not touch `ElemFamily`.

- [ ] **Step 2: Add the `collect_write_results` fixpoint (fact 9-i), and thread `vs ∪ write_results` into classification.** Model it on `collect_gather_results` (`route_typed_vec.tw:449`). Signature: given the function body, the current group `vs`, and `ids`, return the set of result slots of `ACall(vector$set_unsafe, [base, _, _])` where `base ∈ vs`; iterate to a fixpoint (a write result can be the base of a later set). Gate the whole thing on `fam.mono_key == "vec_i64"` (empty for bool). Two distinct uses of the output — keep them separate:
  - **(a) Group membership for classification and eligibility:** the escape classifier and `eligible_v` must see the write-result slots as group members, so form `vs_ext = vs ∪ write_results` and pass **`vs_ext`** as the `vs` argument to `classify_op`/`classify_expr`/`v_group_typeable` and include `write_results` in the final `eligible_v` (union it where `collect_gather_results`'s output is unioned). Otherwise a read of a rebound slot (`y := xs[1]` after `xs` was rebound from the set result) is classified against a `vs` that excludes it and misclassifies.
  - **(b) The narrow `.AAssign` exception (Step 4):** keep `write_results` as a **separate** set, because the exception must distinguish "rebind source is a write result" from "rebind source is merely any group member" (a plain group-member rebind still escapes). Do not collapse (a) and (b) into one set.

- [ ] **Step 3: Accept the set in `classify_op` (i64 only).** In `classify_op` (`route_typed_vec.tw:1427`, the `.ACall(.AGlobalFunc(fid), args)` arm), add a branch **before** the escaping else, mirroring the `len` branch, guarded by family:

```
} else if fam.mono_key == "vec_i64" and fid.id == ids.set_unsafe and args.len() == 3
  and slot_in(args[0], vs) and !slot_in(args[1], vs) and !slot_in(args[2], vs) {
  use_esc(false)
}
```

The base (`args[0]`) is the candidate; index/value must not be group slots. Do **not** whitelist `set_in_place` — at routing time the op is always `set_unsafe` (fact 4).

- [ ] **Step 4: Add the rebind exception to the `.AAssign` arm (fact 9-ii).** Thread the separate `write_results` set into `classify_op`/`classify_expr` (alongside the `vs` argument, which by Step 2 is now `vs_ext`). The `.AAssign(dst, a)` arm (`route_typed_vec.tw:1589`) is currently `use_esc(slot_in(a, vs) or vs.has(dst.id))`. Revise:

```
.AAssign(dst, a) => if vs.has(dst.id) and slot_in(a, write_results) {
  use_esc(false)   // legitimate in-place rebind of a group slot from an accepted in-region write result
} else {
  use_esc(slot_in(a, vs) or vs.has(dst.id))   // unchanged rule (vs is vs_ext); still disqualifies acc = acc.take(...)
},
```

`write_results` (not `vs_ext`) is the discriminator: a `take`/`drop` wrapper result is never a `write_results` slot, so the original disqualification (S2.0 soundness fix) still fires for it, and a rebind from a plain group member that is not a write result still escapes. Only a `set_unsafe`-result rebind is newly allowed.

- [ ] **Step 5: Swap the callee in `rewrite_op` (explicit i64 guard).** In `rewrite_op` (`route_typed_vec.tw:2266`, the `cond`), add a guard that never produces `.AGlobalFunc(-1)`:

```
fid.id == ids.set_unsafe and fi.set >= 0 and args.len() == 3 and arg0_in(args, eligible_v) => .Some(fi.set),
```

The explicit `fi.set >= 0` (which is `-1` for the bool family and under the kill-switch) is required — do not rely on `arg0_in` alone to exclude bool. Only the base must be `eligible_v`; the value stays boxed `anyref` (fact 5). Apply the same `>= 0` / `mono_key == "vec_i64"` guard to the swapped-site reporting in Step 6.

- [ ] **Step 6: Report swapped set sites (for Phase 3's remap).** Have the routing driver collect, per swapped set, `(func_id, result_local_id)` (the emit "site" key, `mutable_select.tw:89`). Return them alongside the routed funcs (extend `route_typed_vectors`' result, or expose a companion query prepare can call). This list is the input to the Phase-3 decision remap.

- [ ] **Step 7: Rebuild.** Run: `make bundle-cli` — must print `Fixed point reached`. Non-convergence means the routing change miscompiled boot's own code — diagnose before proceeding (fact 1).

- [ ] **Step 8: Verify promotion in WAT.** Create `/tmp/mvw.tw`:

```
fn f(n: Int) Vector<Int> {
  xs := collect i in range(n) { i }
  xs[0] = 42
  m := xs.len()
  y := xs[1]
  xs
}
println(f(5).len().to_string())
```

Run: `target/twk wat /tmp/mvw.tw --func "f307_f"` (find the mangled id via `--list`). Expected AFTER this phase: calls include `rt_arr__get_i64`, `rt_arr__len_i64`, and `rt_arr__set_i64` (the **functional** typed set — in-place is Phase 3); **no** boxed `rt_arr__get`, **no** boxed `rt_arr__set_in_place`. (A `BoxedInt` may still appear for the boxed set-value argument — fact 5 — so do NOT assert its global absence; see Phase 5.) If the set is boxed, promotion did not fire — recheck Steps 2–5 (most likely the `AAssign` arm still escaping, Step 4).

> **VERIFIED 2026-08-02, return-ABI caveat (pre-existing, not a Phase-2 bug):** a `collect`-born producer keeps a **boxed `$rt_types__PVec` return ABI** even when its set/reads are all typed. Cause: `return_family` → `return_atom_slots` (typed_param_abi.tw) recurses into the comprehension's lowered `ALoop`/`AMatch` arms and collects spurious non-vector "return" slots, so `slot_typed_after_route` finds one untyped and drops the typed-return bridge. This reproduces for a plain `collect`+return with **no set at all** and is **identical with `TWINKLE_TYPED_VEC_WRITE=0`**, so it is orthogonal to this sub-project (the returned typed vector is coerced by the documented emit boxing adapter — see the module docstring). A **`Vector.make`-born** producer (no loop) DOES get a `(ref null $rt_types__PVecI64)` typed return. Assert PVecI64-return only on the `Vector.make` fixture (Phase 5 Step 2); for the `collect` fixture assert the typed **ops** (`set_i64`/`get_i64`) and absence of boxed vector ops, NOT the return type. Fixing `return_atom_slots` to not dive into non-tail loops is a separate, out-of-scope return-typing improvement.

- [ ] **Step 9: Fmt + lint + commit.**

```bash
target/twk fmt boot/compiler/backend/route_typed_vec.tw && target/twk lint boot/main.tw
git add boot/compiler/backend/route_typed_vec.tw
git commit -m "feat(typed-vec): group-include indexed-set result, accept rebind, swap set_i64 (i64 only)"
```

---

## Phase 3: Emit composition — remap decisions so owned sites emit `set_in_place_i64`

Goal: a proven-owned promoted set emits the in-place `set_in_place_i64` (no COW copy). This needs BOTH the catalog wiring (so the emit selector recognizes `set_i64`) AND the decision remap (so the selector's `persistent_matches` gate passes and it selects the typed in-place target) — see fact 4. Non-owned promoted sites keep the functional `set_i64`.

**Files:** `boot/compiler/opt/semantics.tw`, `boot/compiler/codegen/mutable_catalog.tw`, `boot/compiler/backend/prepare.tw`.

- [ ] **Step 1: Declare the in-place pairing.** In `opt/semantics.tw`, add a call-spec for `vector$set_i64` mirroring `vector$set_unsafe` (`:127-131`): same `effect: .Update`, `cow_base_arg: .Some(0)`, and `in_place_equivalent: .Some(b.id("vector$set_in_place_i64"))`. (`is_cataloged_call`, `mutable_catalog.tw:99`, keys on `.Update`, so this makes `set_i64` catalogued.)

- [ ] **Step 2: Classify `set_i64` in the catalog.** In `mutable_catalog.tw` `decision_family_for_persistent` (`:55`) add `if fid == b.id("vector$set_i64").id { return .Some(.VectorSet) }`. `base_arg_index_for_decision_family` (`:48`) already maps `VectorSet → 0`. Also update `family_for_persistent` (`:66`) to return `"vector_set"` for `set_i64` (reporting only). Now `select_call_for_emit(persistent_fid = set_i64)` finds an entry — but it will still `reject(.WrongPersistentTarget)` until Step 3, because the decision was recorded with `persistent_func = set_unsafe`.

- [ ] **Step 3: Remap the swapped sites' decisions in `prepare` (fact 4-b).** Thread the `mutable_decisions` table into the routing step in `prepare_backend_with_mutable_decisions` (`prepare.tw:178`), and for each swapped set site reported by Phase-2 Step 6, transform its decision(s) in the table: `persistent_func: set_unsafe → set_i64`, `mutable_func: set_in_place → set_in_place_i64` (leave `site`, `source_local`, `result_local`, `base_arg_index`, `arg_count`, `family` unchanged — the site key and shapes are stable across the callee swap). Store the remapped table into the `PreparedModule` (`prepare.tw:201`). After this, `select_call_with_policy` (`mutable_select.tw:224`) sees `d.persistent_func == set_i64 == persistent_fallback` (gate passes) and `d.mutable_func == set_in_place_i64` → emits the typed in-place op.

- [ ] **Step 4: Rebuild.** Run: `make bundle-cli` — must print `Fixed point reached`.

- [ ] **Step 5: Verify the in-place swap in WAT.** Re-run the `/tmp/mvw.tw` dump (extend it with an obviously-owned set — the `xs[0]=42` on a locally-built, returned `xs` is owned). The owned set must now be `call $rt_arr__set_in_place_i64` (not `set_i64`). Add a non-owned analogue (a vector shared through an alias read after the set) and confirm it stays `set_i64`. If the owned case still shows `set_i64`, either the catalog entry is missing (Steps 1-2) or the remap did not hit the site (Step 3) — check the site-key match (`func.id`, result `local.id`).

- [ ] **Step 6: Fmt + commit.**

```bash
target/twk fmt boot/compiler/opt/semantics.tw boot/compiler/codegen/mutable_catalog.tw boot/compiler/backend/prepare.tw
git add boot/compiler/opt/semantics.tw boot/compiler/codegen/mutable_catalog.tw boot/compiler/backend/prepare.tw
git commit -m "feat(typed-vec): remap owned set decisions to set_in_place_i64 at emit"
```

---

## Phase 4: Verifier — accept typed set on a `PVecI64` slot

Goal: the Stage-1 post-route verifier accepts `set_i64`/`set_in_place_i64` whose base slot is physically `PVecI64`, so a legitimately-promoted write is not flagged as a repr mismatch.

**Files:** `boot/compiler/backend/verify_expr.tw`.

- [ ] **Step 1: Find the existing typed-vector verifier exception.** Locate where the verifier already tolerates a `get_i64`/typed op on a `PVecI64` slot (grep `verify_expr.tw` for `get_i64` / `PVecI64` / the arg-repr check for vector builtins). Confirm whether `make bundle-cli` in Phase 2/3 already passed verification (if routing produced `set_i64` on a `PVecI64` base and verification passed, the generic arg-repr check may already accept it because the `set_i64` ABI declares a `PVecI64` base — in that case this phase is a no-op and only needs a confirming test).

- [ ] **Step 2: If verification rejects, add the arm.** Mirror the `get_i64` exception: when the call is `vector$set_i64`/`vector$set_in_place_i64` and `args[0]`'s slot repr is `PVecI64` (the family PVec), accept it; the value arg is `anyref` (boxed) by ABI and needs no coercion check. Keep the check narrow (exact builtin ids + PVecI64 base).

- [ ] **Step 3: Rebuild + confirm.** Run: `make bundle-cli` — `Fixed point reached`, and `TWINKLE_VERIFY_LEVEL=full` (the default) is in force during self-host, so a clean fixed point is the verifier's green light on real promoted boot code.

- [ ] **Step 4: Commit (skip if Step 1 proved it a no-op).**

```bash
git add boot/compiler/backend/verify_expr.tw
git commit -m "verify(typed-vec): accept set_i64/set_in_place_i64 on PVecI64 base"
```

---

## Phase 5: Fixtures, negatives, and behavioral coverage

Goal: lock the promotion and the runtime semantics with suite tests. The suite compiles the current compiler from disk, so most steps need no rebuild.

**Files:** `boot/tests/suites/typed_vector_write_suite.tw`.

The suite uses two harness styles already established elsewhere: (a) **WAT-substring assertions** — compile a snippet with `pipeline`/`codegen` to WAT and assert on presence/absence of op names and the result type (see how other suites drive `codegen`); (b) **execution assertions** — `target/twk run` behavior, expressed as ordinary Twinkle asserting computed values.

**WAT-assertion discipline (fact 5 + boundary-matching):** the set VALUE stays boxed (`.Anyref`), so a promoted function legitimately still contains `rt_types__BoxedInt`. **Do NOT assert global absence of `BoxedInt`.** Assert the absence of boxed *vector* ops and presence of the typed ones. **Substring matching is a trap here:** `rt_arr__get` is a substring of `rt_arr__get_i64`, `set_in_place` of `set_in_place_i64`, and `$rt_types__PVec` of `$rt_types__PVecI64` — a naive `contains("rt_arr__get")` check would wrongly fire on the typed op. Assert on **exact, boundary-delimited call targets**: emitted WAT writes each call as `call $rt_arr__get_i64` / `call $rt_arr__set_in_place_i64` on a line, and types as `(ref null $rt_types__PVecI64)`. So assert presence of the exact tokens `$rt_arr__get_i64` / `$rt_arr__set_in_place_i64` / `$rt_types__PVecI64`, and absence of the boxed tokens with a **trailing boundary** — `$rt_arr__get)` or `call $rt_arr__get\n` (i.e. `$rt_arr__get` NOT followed by `_`), `$rt_types__PVec)` / `$rt_types__PVec ` (a `PVec` field/local, not `PVecI64`). Prefer a helper that extracts `call $…` target tokens and compares them exactly over raw `contains`.

- [ ] **Step 1: Positive — collect-born promotes.** Use a snippet that actually contains an indexed read so `get_i64` is expected: `fn f(n){ xs:=collect i in range(n){i}; xs[0]=42; y:=xs[1]; println(y.to_string()); xs }`. WAT shows result `$rt_types__PVecI64`, `rt_arr__set_in_place_i64` (owned), `rt_arr__get_i64` (the `xs[1]` read), and **no** boxed `rt_arr__get`/`rt_arr__set_in_place`/`$rt_types__PVec` on the vector path. (A `collect`+set with no read is a valid positive too, but then assert only `set_in_place_i64` + `PVecI64`, not `get_i64`.)

- [ ] **Step 2: Positive — `Vector.make`-born promotes.** WAT for `fn f(){ xs:=Vector.make(3,0); xs[1]=9; xs }` shows `make_i64` producer, `set_in_place_i64`, `PVecI64` return.

- [ ] **Step 3: Positive — deep trie functional set.** A snippet that forces a non-owned functional set on a >1024-element vector (so the trie/`do_set_i64` path runs) and reads the value back; assert the read value is correct (execution assertion) and WAT shows `set_i64` (not `set_in_place_i64`).

- [ ] **Step 4: Behavioral — in-place vs functional, OOB.** Execution assertions: owned `xs[i]=v` then `xs[i]` returns `v`; a functional set through a kept alias leaves the original unchanged; an out-of-bounds `xs[i]=v` traps (use the established trap-testing pattern in the suites).

- [ ] **Step 5: Negatives stay boxed.** Using the same boundary-aware token matching, assert **no** `$rt_arr__set_i64`/`$rt_arr__set_in_place_i64`/`$rt_types__PVecI64` for genuinely-disqualifying source shapes: non-`Int` element (`Vector<Float>` set — also proof only `vec_i64` promotes); stored into a **dict value** (`Dict<Int, Vector<Int>>`, `m[k]=xs` — dict storage is out of scope, a reliable escape). Notes on shapes to AVOID as fixtures:
  - The `!slot_in(args[2], vs)` clause (set value being a group vector) is **not source-expressible** — a `Vector<Int>` indexed set's value is an `Int`, never a vector, so `xs[i] = <vector>` does not typecheck. It is a defensive guard only; cover it (if at all) as a synthetic prepared-IR unit test, not a source fixture.
  - **Do NOT use "passed to a user function" (`sink(xs)`)** — `classify_op`'s `user_direct_call_accepts_boxed_arg` (`route_typed_vec.tw:1436`) treats a direct call to a non-builtin as **non-escaping** (the callee gets a boxed copy), so that shape does not stay-boxed and would make the test wrong.
  - Storing into a `Vector<Int>` **record field** also does not reliably stay boxed (typed-field analysis may promote the field) — avoid it as a "stays boxed" negative.
  Each valid negative must retain the existing boxed `set_in_place` path.

- [ ] **Step 6: Kill-switch negative — NOT here.** The flag-off assertion cannot be an in-suite Twinkle test (a suite run has one fixed process env; `route_ids()` reads the running `twk`'s env, and Twinkle tests can't set a subprocess env). It is a command-level WAT comparison — see Phase 6 Step 1. Leave this as a pointer, not an in-suite test.

- [ ] **Step 7: Run.** `TWK_TEST_FILTER="typed vector write" target/twk run boot/tests/main.tw` → all PASS.

- [ ] **Step 8: Fmt + commit.**

```bash
target/twk fmt boot/tests/suites/typed_vector_write_suite.tw
git add boot/tests/suites/typed_vector_write_suite.tw
git commit -m "test(typed-vec): indexed-write promotion + runtime-semantics coverage"
```

---

## Phase 6: Flag-off validation, self-host + regression gate, perf

The `TWINKLE_TYPED_VEC_WRITE` kill-switch is already implemented (Phase 2 Step 1). This phase only **validates** flag-off and runs the gates.

**Files:** `boot/bench/` (bench). (No `route_typed_vec.tw` change here — the flag lives in Phase 2.)

- [ ] **Step 1: Flag-off smoke test (command-level, NOT in-suite).** The boot suite compiles snippets in-process via `pipeline.compile_source`, and `route_ids()` reads the *running* `twk`'s process env — so a single `target/twk run boot/tests/main.tw` invocation cannot mix flag-on and flag-off assertions, and an in-suite Twinkle test cannot set the subprocess env. Validate the switch **externally** instead: build a promoting fixture both ways and compare the WAT —

  Run: `target/twk build /tmp/mvw.tw -o /tmp/on.wat` and `TWINKLE_TYPED_VEC_WRITE=0 target/twk build /tmp/mvw.tw -o /tmp/off.wat`
  Expected: `on.wat` contains `$rt_arr__set_in_place_i64` + `$rt_types__PVecI64`; `off.wat` contains neither (boxed `$rt_arr__set_in_place` / `$rt_types__PVec` instead). Capture this as a shell check in the perf/bench area or a `Makefile`/docs note; do **not** add it as an in-suite Twinkle test.

- [ ] **Step 2: Self-host gate.** Run: `make bundle-cli` → `Fixed point reached` (boot compiling boot converges with write-promotion on). Then `make boot-test` → all green.

- [ ] **Step 3: Rust reference suite.** Run: `make rust-test` → green (stage0 untouched; confirms no accidental `src/` coupling).

- [ ] **Step 4: Census regression.** Capture `target/twk ir boot/main.tw --census --sites` before and after (or with the flag off vs on) and diff — differences should be confined to newly-typed vector sites; no unrelated churn.

- [ ] **Step 5: Perf bench.** Port an indexed-update read+write shape (`collect` + a `xs[i]=v` loop + a read loop + return) into `boot/bench/`, timed with `@std.date` at n ∈ {65536, 1048576}. Run flag-off (`TWINKLE_TYPED_VEC_WRITE=0`, boxed) vs flag-on (typed). Realistic expectation: the **read** side tracks the ~6.8× typed-read win (flat `i64` leaves, no `BoxedInt` pointer-chase). The **write** side should at least match boxed `set_in_place` — the win is flat `i64` leaf **storage** (and enabling the whole vector to stay `PVecI64` so reads are typed), **not** removing value boxing (the set value is boxed on both paths — fact 5). Do not expect a large write speedup in isolation; the leverage is keeping the vector typed end-to-end. If flag-on regresses, stop and diagnose (likely a slot fell back to boxed — recheck Phase 3/4).

- [ ] **Step 6: Fmt + commit.**

```bash
target/twk fmt boot/bench/*.tw
git add boot/bench/
git commit -m "bench(typed-vec): indexed-write perf + flag-off smoke gate"
```

---

## Out of scope (this sub-project)

- **In-region append** (`xs = xs.append(v)` on a promoted vector) — sub-project **B** (`2026-08-02-typed-vector-append.md`). Until B lands, a set+append region has an append that still escapes routing → the whole vector stays boxed. That is the correct, sound fallback here.
- `Bool`/`Float`/`Byte` element families — Int (`PVecI64`) only.
- Relaxed (post-slice) vectors — a sliced vector escapes/relaxes and is not promoted (unchanged).
- `stage0` mirror — not needed (codegen optimization, not a source-level language feature).
