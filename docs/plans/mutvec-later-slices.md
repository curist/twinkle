# MutVec Storage — Later Slices Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Scope / driver (read before picking this up):** This plan optimizes **user
> workloads**, not the compiler's own self-build. MutVec claims *owned, indexed-write*
> `Vector<T>` regions (`xs[i] = v`); the boot compiler almost never does that — it
> appends and uses `Dict<Int,_>` for keyed-int maps, so it claims only ~2 scratch
> `Int` regions today and element-family broadening adds ~0 more. The real customer is
> numeric code (the Float family — see the value ranking below). **For a general
> boot-compiler speedup, this is the wrong plan** — advance the 8C builder-region
> follow-up slices instead (`docs/plans/sound-uniqueness/codegen/README.md` →
> "Where the general boot-compiler win is"; Plans 3-vector / 4 / 5). Note that **8C
> Plan 4 (typed routing)** is the *append-side* analog of MutVec's typed storage — it
> unboxes `Vector<Int>` builder accumulators to flat i64 — so the two plans are the
> two halves of the same typed-storage story (MutVec = index-write side, 8C Plan 4 =
> append side).

**Goal:** Extend MutVec beyond the landed slice-1 (owned `Vector<Int>` → flat mutable `MutVecI64` → freeze to `PVecI64`, now unconditional) to the remaining element families (`Bool`, `Float`, `Byte`, boxed) and, further out, to param-sourced owned vectors — reusing the `PVecFamily` abstraction rather than hand-duplicating per element type. `Float` and `Byte` each need a new typed `PVec` family first (the dedup payoff); they are the same shape of work and can land together.

**Architecture:** Slice 1 proved the mechanism — a region pass (`codegen/mutvec_region.tw`) claims an owned, locally-born `Vector<Int>` carrying ≥1 indexed update, rewrites it to `mutvec_*` ops with a single boundary freeze, and a backend repr pass (`backend/mutvec_repr.tw`) types the handle slots `MutVec(I64)`. That mechanism is **element-agnostic** except for three seams: (1) the seven `mutvec_*_i64` runtime ops are hand-written for i64; (2) the detector gates on `Vector<Int>`; (3) `mutvec_repr` hard-codes `.MutVec(.I64)`. This plan drives all three off the element type. The runtime seam piggybacks on **`docs/plans/rt-arr-family-dedup.md`**, which folds the PVec trie ops into the `PVecFamily`-parameterized builder and enriches `PVecFamily` with `suffix` / `leaf_store` / `elem_box` — exactly the fields a family-generated mutvec op needs. `ReprKind.MutVec(ElemRepr)` is already generic, so the backend seam is a `MonoType → family` lookup, not new machinery.

**Tech Stack:** Boot compiler (self-hosted Twinkle in `boot/`), Wasm-GC runtime emission (`boot/compiler/codegen/runtime/arr.tw`), ANF region pass (`boot/compiler/codegen/mutvec_region.tw`), backend repr/route (`boot/compiler/backend/`).

---

## Prerequisites & dependencies (read first)

- **`docs/plans/rt-arr-family-dedup.md` — HARD prerequisite for a clean runtime fold.** It enriches `PVecFamily` with `suffix`, `leaf_store` (anyref→element leaf conversion), and `elem_box` (the inverse), and proves the byte-identical-WAT family-fold methodology. Without it, generalizing the mutvec ops means adding a *fourth* copy-paste axis (`mutvec_new_bool_fn`, `mutvec_new_f64_fn`, …). With it, the mutvec ops fold the same way the trie ops do. Do not start Phase 1 until dedup's `PVecFamily` fields (at least `suffix` + `leaf_store`) exist.
- **A new typed-storage family — HARD prerequisite for Float and for Byte (Phase 4).** Neither `Vector<Float>` nor `Vector<Byte>` has a typed persistent vector today; both are stored **boxed** (each element boxed to anyref), and a `MutVec<fam>` needs a typed `PVec<fam>` to freeze *into*. Adding one — `PVecF64` (struct + `family_float()` + get/make/builder ops) and/or `PVecByte` (struct + `family_byte()` + ops) — is the explicit **payoff** the dedup plan names (its line 505): once the family fold lands, instantiating a new `family_*()` is cheap. **Float and Byte are the same shape of work** (one typed family each) and can land together as sibling families — they differ only in element type, boxing, and leaf array (see the Byte note in the value ranking). Each is a storage feature valuable independently of mutvec; do the needed family/families first, then its Phase-4 mutvec ride-along.
- **`Bool` needs neither** — `PVecBool` / `family_bool` already exist, so Bool (Phase 3) can follow Phases 1–2 directly.
- **`docs/plans/compiler-stack-safety.md` — related (soft).** MutVec currently ships an **interim guard**: `backend/prepare.tw`'s deep-module depth bailout calls `mutvec_repr.assign_mutvec_reprs` inside the bailout branch, because that bailout otherwise skips the repr handoff and leaves mutvec handle slots boxed → `illegal cast`. That guard is superseded and removed by **compiler-stack-safety Phase 2** (make the routing walkers iterative + narrow the bailout to per-function). Relevance here: `assign_mutvec_reprs` is family-driven, so once this plan's **Phase 2** generalizes it off the element `MonoType`, the bailout-path call generalizes *for free* — do **not** entrench a per-family special case in the bailout branch; keep it going through the one `assign_mutvec_reprs`, so compiler-stack-safety can delete the branch in one move.

## Value ranking (do the valuable ones; skip the rest)

| Family | Value | Blocked on | Notes |
|---|---|---|---|
| **Float** | **High** | `PVecF64` + Phases 1–2 | Numerics mutate `Vector<Float>` heavily; the real prize. `ArrayF64` already exists; boxing is `BoxedFloat`. |
| **Byte** | **High** | `PVecByte` (`array i8`) + Phases 1–2 | `Vector<Byte>` is already real and already **boxed** — it is the `readfile` result type (`Result<Vector<Byte>, String>`) and the natural type for codecs/parsing/binary formats. A typed `array i8` `PVecByte` unboxes it at **1 byte/element** (native GC `array.get_u`; `elem_ty` `.I32`, i31 boxing — reuses bool's `leaf_store`/`elem_box`). **Complements `@std.buffer`, does not replace it:** a Byte GC vector covers *in-language* byte work with `v[i]`/`v[a..b]` sugar and no linear-memory opt-in, but GC arrays aren't addressable, so Buffer stays for FFI / linear-memory / shared-memory. Only new work vs Float: define the `ArrayByte` = `array i8` GC type (Float's `ArrayF64` already exists). |
| **Bool** | Low | Phases 1–2 | Cheap ride-along; bool vectors are rare. Do it because it falls out of the fold, not for its own sake. |
| **Boxed** (records/strings/nested) | Low–Med | Phases 1–2 | No *unboxing* win (elements are already refs); only O(1)-write-vs-trie. Phase 5, optional — do only for a measured mutation-heavy reference-vector workload. |

`S4` (param-sourced thaw) and append-only unification are captured as **explicitly deferred** at the end, with their real dependencies.

---

## Phase 1: Family-generate the mutvec runtime ops

Goal: the seven `mutvec_*_i64` ops become `pvec_mutvec_*_fn(f: PVecFamily)` builders, registered per family, emitting **byte-identical WAT for i64** (the existing behavior) so the refactor is provably a no-op before any new family is added.

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw` (the `mutvec_*_i64_fn` cluster + `module()` registration)
- Modify: `boot/compiler/codegen/runtime/types.tw` (per-family `MutVec<Fam>` GC structs)

### Task 1.1: Capture the i64 baseline

- [ ] **Step 1: Baseline every mutvec op's WAT** (the byte-identical gate for the fold):

```bash
for f in mutvec_new_i64 mutvec_make_i64 mutvec_push_i64 mutvec_set_i64 mutvec_get_i64 mutvec_len_i64 mutvec_freeze_i64; do
  target/twk wat boot/main.tw --func "$f" > "/tmp/$f.before.wat"
done
```

Note: boot claims two mutvec regions, so these ops are emitted in `boot/main.tw` — the baseline is non-empty. If a future dead-code pass DCEs them, add a tiny fixture that forces emission and baseline against that instead.

### Task 1.2: Per-family `MutVec<Fam>` struct

**Files:** `boot/compiler/codegen/runtime/types.tw`, `arr.tw` (the `t_MUTVEC_I64` constant)

- [ ] **Step 1: Parameterize the struct.** Slice 1 added `rt_types__MutVecI64` (mutable `data: ArrayI64` + `len: i32`) and `t_MUTVEC_I64` near `t_ARRAY_I64`. Add a `PVecFamily.mutvec_ty: String` field (`"rt_types__MutVecI64"` / `"rt_types__MutVecBool"` / `"rt_types__MutVecF64"` / boxed `"rt_types__MutVec"`), and generate the GC struct per typed family from `f.arr_ty` + the shared `len: i32`. Register the new structs in `types.tw`'s module list. For i64 the emitted struct must be identical to today's `rt_types__MutVecI64`.

### Task 1.3: Fold the seven ops (one op per commit, WAT-identical gate)

Apply the dedup plan's **canonical transformation** to each `mutvec_*_i64_fn`, using the enriched `PVecFamily`:

- type-specific pieces → family fields: `t_ARRAY_I64` → `f.arr_ty`, `t_MUTVEC_I64` → `f.mutvec_ty`, `t_PVEC_I64` (freeze result) → `f.pvec_ty`, i64 leaf load/store → `f.leaf_store` / `f.elem_box`, `I64Const(0)` fill → `f.zero_value`, name `"mutvec_new_i64"` → `"mutvec_new${f.suffix}"`.
- internal `.Call` targets → `${f.suffix}` (e.g. `mutvec_freeze`'s slow path calls `builder_new_i64` / `builder_append_leaf_range_i64` / `builder_freeze_i64` → the family-suffixed names; slice-1's bulk-freeze already routes through `builder_append_leaf_range_i64`, so this is a straight retarget).

- [ ] **Step 1: Fold `mutvec_new` / `mutvec_make` / `mutvec_len`** (no leaf-value conversion — pure struct/array ops). One `pvec_mutvec_new_fn(f)` etc.; register `family_i64().pvec_mutvec_new_fn()`; delete the `_i64_fn` copies.
- [ ] **Step 2: Fold `mutvec_get` / `mutvec_set` / `mutvec_push`** — these touch the element, so they use `f.leaf_store` (set/push: anyref→element) and the family element load (get: element→result). Watch the logical-length bounds check (shared, family-agnostic — leave it).
- [ ] **Step 3: Fold `mutvec_freeze`** — retarget the fast path (`len <= rt_BF` bulk `ArrayCopy` + `StructNew(f.pvec_ty)`) and the slow path (`builder_new${f.suffix}` + `builder_append_leaf_range${f.suffix}` + `builder_freeze${f.suffix}`) to the family.
- [ ] **Step 4: Prove i64 byte-identical + behavior** after each fold:

```bash
target/twk fmt boot/compiler/codegen/runtime/arr.tw && make bundle-cli && target/twk lint boot/main.tw
for f in mutvec_new_i64 mutvec_make_i64 mutvec_push_i64 mutvec_set_i64 mutvec_get_i64 mutvec_len_i64 mutvec_freeze_i64; do
  target/twk wat boot/main.tw --func "$f" | diff - "/tmp/$f.before.wat" || echo "DIFF in $f"
done
make boot-test
```

Expected: every diff empty, self-host fixed point, boot-test green. A non-empty diff = a wrong `leaf_store`/`.Call` retarget; fix before committing.

- [ ] **Step 5: Commit** (per op or per grouped step).

```bash
git add boot/compiler/codegen/runtime/arr.tw boot/compiler/codegen/runtime/types.tw
git commit -m "rt.arr: family-generate the mutvec_* ops (i64 byte-identical)"
```

---

## Phase 2: Drive the detector + repr off the element family

Goal: the region pass and repr handoff choose the family from the vector's element `MonoType` instead of hard-coding i64. No new family is *claimed* yet (only i64 is registered as a builtin until Phase 3/4 add the ops), so this phase is behavior-neutral for i64.

**Files:**
- Modify: `boot/compiler/codegen/mutvec_region.tw` (the `Vector<Int>` gate + `OpIds` per family)
- Modify: `boot/compiler/backend/mutvec_repr.tw` (`.MutVec(.I64)` → family)
- Modify: `boot/compiler/backend/route_typed_vec.tw` (recognize each `mutvec_freeze_<fam>` as a typed producer)
- Modify: `boot/compiler/builtins.tw` (register the per-family ops + ABI, appended at END)

### Task 2.1: `MonoType → family` classifier

- [ ] **Step 1: Add a shared `elem_family(mono) PVecFamily?`** helper (Int→i64, Bool→bool, Float→f64, reference types→boxed; `.None` for unsupported), used by both the detector's element gate (`handle_is_vector_int` → `handle_vector_family`) and `mutvec_repr` (to pick `ReprKind.MutVec(<fam>)`). Keep it returning `.Some(family_i64())` only until Phase 3/4 register the other families' ops, so the claim set is unchanged.
- [ ] **Step 2: Parameterize the rewrite's `MutVecIds`** so `rewrite_region` emits `vector$__mutvec_set_<fam>` etc. based on the region's family (still only i64 in practice this phase).
- [ ] **Step 3: Route** — generalize the `mutvec_freeze_i64`-is-a-typed-producer hook (`route_ids.mutvec_freeze_i64`) to a per-family set, so each `mutvec_freeze_<fam>` types its `PVec<fam>` result. i64 stays byte-identical.

- [ ] **Step 4: Gate.** `make bundle-cli` fixed point + `make boot-test` green + byte-identical `boot/main.tw` WAT (only i64 in play). Commit.

---

## Phase 3: Bool family (end-to-end)

Goal: an owned `Vector<Bool>` region lowers to `MutVecBool` and freezes to `PVecBool`. `family_bool` / `PVecBool` already exist, so this is registering the ops + widening the classifier.

**Files:** `arr.tw` (register `family_bool().pvec_mutvec_*_fn()`), `types.tw` (`MutVecBool` struct), `builtins.tw` (bool ops + ABI, END), `mutvec_region.tw` (`elem_family` accepts Bool), `boot/tests/suites/mutvec_region_suite.tw` + `fixtures/`.

- [ ] **Step 1: Failing test.** Add a detector test asserting `region_count("fn f(n:Int) Vector<Bool> { xs := collect i in range(n) { i > 2 }; xs[0] = true; xs }")` is `1` (currently `0` — Bool not yet claimed). Run to see it fail.
- [ ] **Step 2: Register bool ops + widen `elem_family` to Bool.** `make bundle-cli` fixed point.
- [ ] **Step 3: Emit fixture.** Add `fixtures/mutvec_bool.tw` (collect-bool + indexed set + return); verify WAT shows `mutvec_*_bool` + one `mutvec_freeze_bool`, and flag/run result matches a boxed-baseline computation.
- [ ] **Step 4: Gate.** boot-test green (the new detector test passes); self-host fixed point. Commit.

---

## Phase 4: Float and Byte families (end-to-end) — depend on a new typed `PVec`

Goal: owned `Vector<Float>` lowers to `MutVecF64` → `PVecF64`, and owned `Vector<Byte>` lowers to `MutVecByte` → `PVecByte`. **Land the typed-storage family first** (separate; the dedup plan's payoff), then the mutvec ride-along is the same shape as Phase 3. Float and Byte are **independent siblings of identical shape** — do whichever is wanted; doing both together is cheap and de-risks the family abstraction beyond i64/bool.

Per-family typed-storage prerequisite (each its own mini-slice; gate = boot-test + fixed point):
- **Float:** `rt_types__PVecF64` struct + `family_float()` + get/make/builder ops + route recognizing `builder_freeze_f64`. `ArrayF64` already exists; boxing is `BoxedFloat`.
- **Byte:** define `ArrayByte` = `array i8` in `types.tw` (the one genuinely new GC type — native `array.new`/`get_u`/`set`), then `rt_types__PVecByte` struct + `family_byte()` + get/make/builder ops + route recognizing `builder_freeze_byte`. `elem_ty` is `.I32` (i8 read via `array.get_u`); boxing is `ref.i31`, so `leaf_store`/`elem_box` **reuse bool's** (`RefCast(.I31)`+`I31GetU` / `RefI31`). Verify a boxed `Vector<Byte>` (e.g. a `readfile` result) round-trips through the typed rep. Note the Buffer relationship (value-ranking row): this is the in-language byte path, not a Buffer replacement.

**Files:** the typed family first (`arr.tw`/`types.tw`), then `arr.tw`/`builtins.tw`/`mutvec_region.tw` for the f64/byte mutvec ops, `mutvec_region_suite.tw` + `fixtures/mutvec_float.tw` / `fixtures/mutvec_byte.tw`.

- [ ] **Step 0 (prerequisite): land the needed typed family/families** (`PVecF64` and/or `PVecByte`) per the per-family notes above. Verify `Vector<Float>` / `Vector<Byte>` collect/index/return works typed end-to-end.
- [ ] **Step 1: Failing detector test** for an owned `Vector<Float>` (and/or `Vector<Byte>`) set region (expects `1`).
- [ ] **Step 2: Register the mutvec ops + widen `elem_family`** to Float (and/or Byte).
- [ ] **Step 3: `fixtures/mutvec_float.tw` / `mutvec_byte.tw`** — collect + indexed set + return; WAT shows `mutvec_*_<fam>` + one freeze; result matches a boxed baseline.
- [ ] **Step 4: Bench.** Extend `boot/bench/mutvec_slice1_bench.tw` with the new-family variant(s); confirm the same mutation-density win profile as i64. For Byte, a codec/parse-shaped bench is the honest workload.
- [ ] **Step 5: Gate + commit.** boot-test + rust-test + fixed point.

---

## Phase 5 (optional, low-value): Boxed family

Goal: an owned `Vector<T>` for reference `T` (records/strings/nested vectors) with indexed updates lowers to a flat mutable `MutVec` (anyref backing) → boxed `PVec`. No unboxing benefit — only O(1) write vs O(log n) trie — so **do this only for a measured mutation-heavy reference-vector workload.**

**Files:** `arr.tw` (register `family_boxed().pvec_mutvec_*_fn()`), `builtins.tw`, `mutvec_region.tw` (`elem_family` accepts reference types), fixtures.

- [ ] **Step 1: Decide with a bench, not on principle.** Write a mutation-heavy `Vector<record>` bench; if MutVec-boxed doesn't clear a meaningful margin over the persistent path, **stop here and leave boxed deferred** — record the number.
- [ ] **Step 2 (only if the bench justifies it):** register boxed ops, widen `elem_family`, add fixtures, gate. Watch aliasing: the region detector already requires the vector handle owned/non-escaping, so a mutable ref array is sound; confirm no element-aliasing assumption is introduced.

---

## Explicitly deferred (out of scope for this plan)

- **S4 — thaw-from-`PVec` / param-sourced owned vectors.** The detector requires the vector to be **locally born** (collect/make) so ownership is trivial. Claiming an owned vector arriving as a `PVec` parameter needs (a) a cross-function proof that the parameter is uniquely owned and (b) an owned-specialized mutable ABI so callers hand off ownership and the callee thaws→mutates→refreezes. Both live on the **sound-uniqueness track** (`docs/plans/sound-uniqueness/`, storage S4). Not startable until that lands; revisit as an S4 customer, not here.
- **Append-only loop unification (Approach A).** Append-only accumulator loops are already handled correctly by `builder_region`. MutVec deliberately claims only ≥1-indexed-update regions. Merging the two passes into one is a risk-only refactor (the builder path is proven and byte-identical-critical); keep them separate until there is a concrete reason to unify. The typed-storage win for the append idiom lives on the **8C track as Plan 4 (typed routing)**, not here — see `docs/plans/sound-uniqueness/codegen/builder-region-design.md`.

## Testing strategy (summary)

- **Phase 1 gate = byte-identical i64 WAT** — the family fold must not change the emitted i64 ops (mirrors the dedup plan's discipline).
- **Self-host fixed point** (`make bundle-cli`) after every task — boot claims i64 regions, so the compiler compiling itself through mutvec is a live correctness check.
- **Per-family end-to-end fixtures** (`fixtures/mutvec_bool.tw` / `_float.tw`) — WAT shows the family's `mutvec_*` + one freeze (or none for scratch), and the run result matches a boxed-baseline computation.
- **`make boot-test` + `make rust-test`** at every behavior-changing task.

## Risks

- **Wrong `leaf_store`/`elem_box` for a family** → a WAT diff in Phase 1 (i64) or a wrong result in Phase 3/4. The per-op WAT-identity gate and per-family result fixtures catch it.
- **Float without `PVecF64`** → nowhere to freeze into. Enforced by Phase 4 Step 0 ordering.
- **Boxed built on principle rather than a bench** → dead complexity. Phase 5 Step 1 makes the bench the gate.
