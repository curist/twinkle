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

Landed 2026-08-03 (commit 420bb6e0). Deviation from the plan's literal text: the
single decision point is `elem_family_suffix(mono) String?` (a family SUFFIX tag,
not the emission-side `PVecFamily`), backed by `mutvec_families()` — the detector/
repr/route key on builtin FuncIds + MonoType, not on the Instr-carrying PVecFamily.

- [x] **Step 1: shared classifier** — `elem_family_suffix(mono)` in `elem_family.tw`
  (Int→`"_i64"`, else `.None`), backed by `mutvec_families()` (source of truth for
  registered mutvec families). Drives the detector's element gate
  (`handle_is_vector_int` → `handle_vector_family`) and `mutvec_repr`'s ElemRepr
  choice (via `candidate_typed_vec_family`). Only Int claimable, so the claim set
  is unchanged.
- [x] **Step 2: Parameterize the rewrite's `MutVecIds`** — generic fields resolved
  per region via `resolve_mutvec_ids(b, suffix)`; `MutVecRegion` carries `family`.
  The typed builder-freeze producer recognizer (`OpIds.freeze_i64`, the GOTCHA)
  is now `OpIds.typed_freeze: Dict<suffix,id>`, matched against the claimed family.
- [x] **Step 3: Route** — `route_ids.mutvec_freeze_i64` → `mutvec_freeze:
  Dict<mono_key,id>` per family; the `collect_candidate_from_op` hook matches the
  active family's `mutvec_freeze`. i64 byte-identical.
- [x] **Step 4: Gate.** `make bundle-cli` fixed point + `make boot-test` (3410
  passed). Behavior-neutrality proven by compiling fixed i64-mutvec programs
  (`mutvec_producers`, `mutvec_slice1_bench`, `mutvec_spike`) with pre- and
  post-change `twk` and diffing to byte-identity. NOTE: the whole-file
  `boot/main.tw` WAT diff is NOT a valid gate — Phase 2 necessarily changes the
  compiler's own struct types (`MutVecIds`/`RouteIds`/`OpIds`/`MutVecRegion`),
  which perturbs its self-compiled WAT (type renumbering cascade). Use a FIXED
  user program compiled by old vs new `twk` instead.

---

## Phase 3: Bool family (end-to-end) — DONE (commit 1440419d)

Goal: an owned `Vector<Bool>` region lowers to `MutVecBool` and freezes to `PVecBool`. Phase 2 paid off — enabling Bool was one entry in `mutvec_families()`; the rest was registering ops + the GC struct.

**The per-op registration is now family-parameterized** (so Float/Byte are near-zero churn). Adding a family = one `mutvec_specs()` row + one `mutvec_fns()` concat + one `MutVec<Fam>` struct + one `mutvec_families()` entry + one `mutvec_wasm_type` arm:
- `builtins.tw`: `MutVecFamilySpec` + `mutvec_specs()` drive `mutvec_abi` (order-independent, i64 inline ABI removed) and `mutvec_rt_defs` (rt() entries; i64 stays inline for FuncId stability, new families `.concat`ed at end).
- `arr.tw`: `mutvec_fns(f)` returns a family's seven ops; bool via `.concat(family_bool().mutvec_fns())`. NOTE: bool also needed `builder_append_leaf_range_bool` emitted (mutvec_freeze's bulk-freeze slow path calls it).
- `types.tw`: `MutVecBool` GC struct (`ArrayBool` + len).
- `wasm_layout.tw`: `mutvec_i64_wasm_type()` → `mutvec_wasm_type(ElemRepr)`; the two backend `.MutVec(_)` arms dispatch on element family. (Gotcha for Phase 4: Bool and Byte both map to `ElemRepr.I32`, so that dispatch can't distinguish them — Byte needs its own repr/struct handling.)

- [x] **Step 1: Detector test** in `mutvec_region_suite.tw` (collect-bool + set + return → region_count 1, lowers to `mutvec_*_bool`).
- [x] **Step 2: Register bool ops + `mutvec_families() += family_bool()`.** `make bundle-cli` fixed point.
- [x] **Step 3: Fixture** `fixtures/mutvec_bool.tw` — collect-bool + indexed set + return; lowers to `mutvec_*_bool` + one `mutvec_freeze_bool`; self-checks against an independent boxed baseline ("OK 7").
- [x] **Step 4: Gate.** 3411 boot tests; self-host fixed point; i64 behaviorally identical (fixed i64 programs identical modulo uniform builtin-id renumbering + the added struct). Side effect: the `phase8b_vector_set_nested` Vector<Bool> sieve now lowers to mutvec instead of the generic `set_in_place` — intended supersession; that test was updated (ArrayLit-seeded owned vectors stay unclaimed, so phase8a keeps set_in_place coverage).

---

## Phase 4: Float and Byte families (end-to-end) — DONE (Float 5032b4ed, Byte f5b661eb)

Goal: owned `Vector<Float>` lowers to `MutVecF64` → `PVecF64`, and owned `Vector<Byte>` lowers to `MutVecByte` → `PVecByte`. **Land the typed-storage family first** (separate; the dedup plan's payoff), then the mutvec ride-along is the same shape as Phase 3. Float and Byte are **independent siblings of identical shape** — do whichever is wanted; doing both together is cheap and de-risks the family abstraction beyond i64/bool.

**Scope decided 2026-08-04 — both families, Float first.** Each family is the **same subset Bool has**: typed reads (`get`/`len`) + `collect`/`make`/`builder_*`/`box`/`gather` + the mutvec ride-along. It does **not** include the full `set_in_place` write-routing (that is i64-only today; extending it to Float/Byte is tracked as **Phase 6** below). Two decisions from the scoping discussion:
- **Float is clean:** `ArrayF64`, `BoxedFloat`, and the sort-axis `elem_f64()` already exist, and `ElemRepr.F64` is distinct — the `mutvec_wasm_type` dispatch already has an `.F64` arm. No new GC array type, no collision.
- **Byte needs the `ElemRepr` collision fix:** `candidate_typed_vec_family` maps both `Vector<Bool>` and `Vector<Byte>` to `ElemRepr.I32`, so the mutvec-handle wasm-type dispatch can't tell them apart. Resolve by **extending `ElemRepr` with a distinct `Byte` variant** (repr_policy). This lives **entirely in the transient mutvec-handle domain** — `ElemRepr` is only really consumed by `mutvec_wasm_type` (`ReprKind.TypedVec(ElemRepr)` is inert; durable typed reads take their wasm type from the mono, and route types durable slots from `fi.fam.pvec_type`). So it is **orthogonal to `physical-repr-planner-refactor.md`**, whose doctrine explicitly excludes mutvec transient handles from `PhysPlan`. That refactor is not a prerequisite: route's durable typed-read path is already family-generic (iterates `all_families()`), so adding Float/Byte durable reads is just `all_families()`/`ElemFamily` entries.

Per-family typed-storage prerequisite (each its own mini-slice; gate = boot-test + fixed point):
- **Float:** `rt_types__PVecF64` struct + `family_float()` + get/make/builder ops + route recognizing `builder_freeze_f64`. `ArrayF64` already exists; boxing is `BoxedFloat`.
- **Byte:** define `ArrayByte` = `array i8` in `types.tw` (the one genuinely new GC type — native `array.new`/`get_u`/`set`), then `rt_types__PVecByte` struct + `family_byte()` + get/make/builder ops + route recognizing `builder_freeze_byte`. `elem_ty` is `.I32` (i8 read via `array.get_u`); boxing is `ref.i31`, so `leaf_store`/`elem_box` **reuse bool's** (`RefCast(.I31)`+`I31GetU` / `RefI31`). Verify a boxed `Vector<Byte>` (e.g. a `readfile` result) round-trips through the typed rep. Note the Buffer relationship (value-ranking row): this is the in-language byte path, not a Buffer replacement.

**Files:** the typed family first (`arr.tw`/`types.tw`), then `arr.tw`/`builtins.tw`/`mutvec_region.tw` for the f64/byte mutvec ops, `mutvec_region_suite.tw` + `fixtures/mutvec_float.tw` / `fixtures/mutvec_byte.tw`.

**Float slice — DONE (commit 5032b4ed).** typed `PVecF64` family + mutvec ride-along, mirroring Bool. `fixtures/mutvec_float.tw` self-checks OK; 3412 boot tests; fixed point; i64/bool neutral. Two things learned:
- **Codegen gotcha (bit Byte too):** `emit/runtime_abi.tw` keeps THREE hardcoded per-family builder-name lists — `is_builder_buffer_arg` / `is_builder_void_push` / `is_builder_seed` — that must gain `_byte`. Missing `_f64` caused the anyref→Array builder-arg shim to misfire (handle cast to PVec → V8 rejected the module). These are the ONLY per-family hardcoded names left; everything else is family-driven.
- **DRY:** the six typed-vector builtins are now generated from `typed_vec_specs` (`typed_vec_abi` + `typed_vec_rt_defs`), parallel to the mutvec generators. Byte = one `typed_vec_specs` row + one `mutvec_specs` row + concats.

**Byte slice — DONE (commit f5b661eb).** Typed packed `PVecByte` family (`ArrayByte
= array i8`) + mutvec ride-along, mirroring Bool/Float. Two byte-specific seams:
- **Packed-read hook.** A packed i8 leaf must be read with `array.get_u`, not the
  `array.get` the full-width families emit (i8 otherwise sign-extends into the i32
  result). Added `PVecFamily.leaf_get` (`ArrayGet` for i64/i32/f64, `ArrayGetU` for
  byte), spliced at the four leaf-read sites (`get` + `mutvec_get`); writes truncate,
  so `ArraySet`/`ArrayCopy` are unchanged. This was the only place packing surfaced —
  it did not balloon, so packed i8 (1 byte/element) was kept over the array-i32 fallback.
- **ElemRepr collision fix.** Byte's element wasm is i32 like Bool, so `elem_wasm`
  can't distinguish them for the mutvec-handle wasm type. Extended `ElemRepr` with a
  distinct `Byte` variant, keyed directly off the family (`mono_key == "vec_byte"`) in
  `candidate_typed_vec_family`, plus a `.Byte => MutVecByte` arm in `mutvec_wasm_type`.
  `ElemRepr` is only materially matched by `mutvec_wasm_type` (`TypedVec(ElemRepr)` is
  inert; `ReprKind` has its own I64/F64/I32), so no other exhaustive match needed a Byte arm.

Everything else was the declarative Float shape: `types.tw` structs, `arr.tw`
`family_byte()` + op registrations + `unbox_byte` + `mutvec_fns` concat, `elem_family.tw`
family entry, one `typed_vec_specs`/`mutvec_specs` row each, and the three `runtime_abi.tw`
`_byte` builder-name entries. Fixture `fixtures/mutvec_byte.tw` runs a UTF-8 byte vector
through the lowered collect+set+return path and cross-checks a boxed reference; detector
test in `mutvec_region_suite`; `repr_policy` gets `Vector<Byte> -> Some(Byte)`. Gate:
self-host fixed point; boot-test green; fixture prints OK; i64/bool/float WAT byte-identical
(normalized id renumbering).

---

## Phase 5 (optional, low-value): Boxed family

Goal: an owned `Vector<T>` for reference `T` (records/strings/nested vectors) with indexed updates lowers to a flat mutable `MutVec` (anyref backing) → boxed `PVec`. No unboxing benefit — only O(1) write vs O(log n) trie — so **do this only for a measured mutation-heavy reference-vector workload.**

**Files:** `arr.tw` (register `family_boxed().pvec_mutvec_*_fn()`), `builtins.tw`, `mutvec_region.tw` (`elem_family` accepts reference types), fixtures.

- [x] **Step 1: Decide with a bench, not on principle.** DONE — `boot/bench/mutvec_boxed_spike.tw`. **The margin clears the bar, but with an important floor.** The baseline for an owned `Vector<Record>` is already `set_in_place` (the optimizer promotes the unique `set_unsafe`), i.e. an O(log n) trie-navigation + in-place store — the hard baseline, the same regime as `set_in_place_i64`. The spike isolates the per-write cost (subtracting a `k=0` build row) into three shapes at identical n/k/index-stream:

  | per write (n=1M, k=2M) | ns/write | what it is |
  |---|---|---|
  | `record_alloc` (`xs[i] = Record.{…}`) | ~51 | alloc + trie-nav + store |
  | `record_shared` (`xs[i] = shared`) | ~39 | trie-nav + store |
  | `int_mutvec` (`xs[i] = j`) | ~1.2 | flat store (MutVec) |

  **Finding:** the trie navigation (~38 ns/write) is the single largest per-write cost — *larger than the record allocation itself* (~13 ns/write, the `record_alloc − record_shared` delta). MutVec-boxed removes only the trie nav (the allocation is unavoidable — records are immutable, every update builds a new one — and is a floor it cannot cross), projecting the realistic `record_alloc` write phase from ~103 ms to ~alloc+flat-store+anyref-freeze-barriers ≈ 35–45 ms → **~2.3–2.9× on the write phase, ~1.5–2.2× on total time**, growing with mutation density (k=8M widens it). It never reaches the i64 family's ratios because of the allocation floor and the anyref freeze/store write barriers.

  **So the O(1)-write-vs-trie lever alone is worth ~2× on mutation-heavy owned record vectors** — the plan's "Low–Med value" prior was about *how narrow the workload is*, not about the ratio being small. Boxed is a real ~2× lever, buildable cheaply now that the family machinery is parameterized (`family_boxed()` already exists as a `PVecFamily`), but only for the high-mutation-count owned-record-vector niche. **Recommendation: keep deferred until a concrete such workload appears** (per the plan's driver note this track optimizes user numeric/sim code, and mutation-heavy reference-vector code is narrower than the Float/Byte numeric case that just landed); revisit as that workload's customer, not on principle. The number is recorded; the gate is cleared, so Step 2 is *justified on ratio* whenever the workload materializes.
- [ ] **Step 2 (only if a workload justifies it):** register boxed ops (`family_boxed()` mutvec_fns), widen `elem_family` to accept reference element types (`elem_family_of` currently returns `.None` for non-primitive `Vector<T>`), add the `MutVec` (anyref-backed) struct + builtins, fixtures, gate. Watch aliasing: the region detector already requires the vector handle owned/non-escaping, so a mutable ref array is sound; confirm no element-aliasing assumption is introduced (storing the same element ref into many slots is fine — the spike's `record_shared` does exactly that).

---

## Phase 6 (deferred, tracked): extend in-place write routing (`set_in_place`) to Float/Byte

**Depends on:** Phase 4 (the Float/Byte typed `PVec` families). Independent of Phase 5. Deferred, but tracked here so the Float/Byte write story is complete rather than silently i64-only.

**The gap it closes (perf parity, not correctness).** Phase 4 gives Float/Byte the mutvec fast path for **owned, locally-born** (collect/make) indexed-write regions — the sieve / numeric-buffer case. It does **not** cover owned typed vectors that mutvec can't claim: a uniquely-owned `Vector<Float>` arriving as a **parameter**, or written outside a claimable region. i64 has a second fast path for those — route's typed functional `set` promoted to `set_in_place` when the base is provably unique (the `typed_vector_write` track). For Float/Byte those writes stay **persistent** (boxed rebuild) after Phase 4 — always correct, just not in-place. This phase gives Float/Byte the same second path.

**This is the `typed_vector_write` / 8C-Plan-4 track**, tracked here for the Float/Byte completeness story. The machinery is i64-specific today:
- `route_typed_vec.tw`: `FamilyIds.set` is `if f.mono_key == "vec_i64" { set_i64 } else { -1 }`; the write-result group closure (`compute_eligible_v` step 2e; `collect_write_results`, gated `fi.fam.mono_key == "vec_i64"`) — generalize both to a per-family typed `set` id.
- `arr.tw`: register `family_float()/family_byte()` `.pvec_set_fn()` / `.pvec_set_in_place_fn()` / `.pvec_do_set_fn()` / `.pvec_get_leaf_fn()` (family-generated; Bool intentionally skips these).
- `builtins.tw`: `set_<fam>` / `set_in_place_<fam>` ABI + rt() (appended at END).
- Confirm the ownership decision-remap (`SwappedSetSite` → in-place, from `project_typed_vector_write`) is family-agnostic or generalize it.

Steps: [ ] register per-family set / set_in_place ops; [ ] generalize route's `FamilyIds.set` + write-result group off the `"vec_i64"` literal; [ ] fixture: an owned param-sourced `Vector<Float>` indexed write emits `set_in_place_f64` (not persistent `set`); [ ] gate = fixed point + boot-test + i64/bool byte-neutral (normalized WAT).

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
