# MutVec — Progress Checklist

**Purpose:** A single at-a-glance tracker for the MutVec effort (owned `Vector<T>`
→ flat unboxed mutable Wasm-GC storage → freeze to persistent `PVec<fam>`).

**Independent numbering.** MutVec phases here are counted on their own, 1..N. They
do **not** map to the sound-uniqueness storage `S1..S6` numbering or the codegen
`8A..8I` numbering — those tracks sequence differently and are not a dependency of
this checklist except where an item explicitly says "cross-track".

**Detail lives elsewhere.** This file is the checklist only. Design rationale,
per-op file maps, gotchas, and bench numbers are in
[mutvec-later-slices.md](mutvec-later-slices.md) and the slice-1 design under
[sound-uniqueness/storage/mutvec-slice1-design.md](sound-uniqueness/storage/mutvec-slice1-design.md).

Legend: ✅ done · ◐ partial · ⏳ deferred (tracked, not started) · ⬜ not started

---

## Status summary

- **Shipped & unconditional:** `Int`, `Bool`, `Float`, `Byte` lower end-to-end
  (owned locally-born `collect`/`make` region with ≥1 indexed write → flat typed
  `MutVec<fam>` → freeze to typed `PVec<fam>`). Self-host fixed point holds;
  boot-test green.
- **Deferred (tracked):** boxed/record element family (spike done, ~2× lever, no
  workload yet); Float/Byte param-sourced `set_in_place` write-routing; S4
  param-sourced / thaw-from-`PVec` (cross-track).

---

## Phases

### Phase 1 — Slice-1 foundation: `Vector<Int>` end-to-end ✅
The mechanism: region pass (`codegen/mutvec_region.tw`) claims an owned,
locally-born `Vector<Int>` carrying ≥1 indexed update, rewrites to `mutvec_*` ops
with a single boundary freeze; backend repr pass (`backend/mutvec_repr.tw`) types
the handle slots `MutVec(I64)`.

- [x] Region detector + ANF→ANF rewrite (locally-born, ≥1 indexed write)
- [x] Backend repr/route handoff; `census --sites` region-audit rows
- [x] Freeze optimizations: bulk-copy freeze (removes low-density crossover),
      trie-less small-vector fast path, no-escape scratch buffers freeze-free,
      escaping regions kept `PVecI64` across return (no rebox)
- [x] Run the region pass **unconditionally** — `TWINKLE_MUTVEC` kill-switch retired
- [x] Emit-level producer + OOB-trap fixtures

### Phase 2 — Family-generate the runtime ops (byte-identical i64) ✅
Fold the seven `mutvec_*_i64` ops into `PVecFamily`-parameterized builders.

- [x] Prerequisite: `rt-arr-family-dedup` — `PVecFamily` gains `suffix` /
      `leaf_store` / `elem_box` / `leaf_get` (archived 2026-08-03)
- [x] Per-family `MutVec<Fam>` GC struct
- [x] Seven ops folded (`new`/`make`/`len`/`get`/`set`/`push`/`freeze`), i64 WAT
      byte-identical

### Phase 3 — Detector/repr/route driven off element family ✅
- [x] `elem_family_suffix(mono)` classifier + `mutvec_families()` source of truth
- [x] Region rewrite `MutVecIds` resolved per family; `MutVecRegion` carries `family`
- [x] Route recognizes each `mutvec_freeze_<fam>` as a typed producer
- [x] i64 behavior-neutral (fixed user program, old-vs-new `twk` byte-identical)

### Phase 4 — Bool family ✅ (`1440419d`)
- [x] Register bool ops + `family_bool()`; `MutVecBool` struct
- [x] `mutvec_wasm_type(ElemRepr)` dispatch (replaces i64 hard-code)
- [x] Fixture + detector test; self-host fixed point

### Phase 5 — Float family ✅ (`5032b4ed`)
- [x] Typed-storage prerequisite: `PVecF64` (`family_float()`, get/make/builder,
      route recognizes `builder_freeze_f64`)
- [x] `MutVecF64` mutvec ride-along; `fixtures/mutvec_float.tw`
- [x] `typed_vec_specs` generator introduced (DRY the six typed-vector builtins)
- [x] Gotcha resolved: `emit/runtime_abi.tw` three hardcoded builder-name lists
      gained `_f64`

### Phase 6 — Byte family ✅ (`f5b661eb`)
- [x] Typed-storage prerequisite: `ArrayByte` = `array i8` + packed `PVecByte`
      (`family_byte()`, ops, route recognizes `builder_freeze_byte`)
- [x] Packed-read hook: `PVecFamily.leaf_get` (`ArrayGetU` for byte at the four
      leaf-read sites)
- [x] `ElemRepr` collision fix: distinct `Byte` variant (Bool/Byte both i32) + a
      `.Byte => MutVecByte` arm in `mutvec_wasm_type`
- [x] `MutVecByte` mutvec ride-along; `fixtures/mutvec_byte.tw`; `runtime_abi.tw`
      `_byte` builder-name entries

### Phase 7 — Boxed family (records / strings / nested) ⏳
No unboxing win (elements are already refs); lever is O(1) write vs O(log n) trie.

- [x] **Step 1 — decide with a bench** (`e6d7d57b`, `boot/bench/mutvec_boxed_spike.tw`).
      Result: ~2.3–2.9× on the write phase / ~1.5–2.2× total for mutation-heavy
      owned record vectors; trie-nav is the lever, record allocation is the floor
      it can't cross. Gate cleared on ratio.
- [ ] **Step 2 — implement** (only when a concrete mutation-heavy owned-record-vector
      workload appears): register `family_boxed()` mutvec ops, widen `elem_family`
      to reference element types, `MutVec` (anyref-backed) struct + builtins,
      fixtures, gate. **Deferred by choice — do not build on principle.**

### Phase 8 — Float/Byte in-place write-routing (`set_in_place`) ⏳
Perf parity, not correctness. Phase 5/6 cover owned **locally-born** regions;
this covers owned typed vectors mutvec can't claim (param-sourced / written outside
a claimable region) — the `typed_vector_write` / 8C-Plan-4 second fast path, i64-only
today. Without it those Float/Byte writes stay persistent (boxed rebuild) — correct,
not in-place.

- [ ] Register per-family `set` / `set_in_place` / `do_set` / `get_leaf` ops
      (`family_float()` / `family_byte()`); builtins ABI + rt() appended at END
- [ ] Generalize route's `FamilyIds.set` + write-result group off the `"vec_i64"`
      literal (`route_typed_vec.tw`)
- [ ] Confirm/generalize the ownership decision-remap (`SwappedSetSite`) is
      family-agnostic
- [ ] Fixture: owned param-sourced `Vector<Float>` indexed write emits
      `set_in_place_f64`, not persistent `set`
- [ ] Gate: fixed point + boot-test + i64/bool byte-neutral (normalized WAT)

### Phase 9 — Param-sourced owned vectors / thaw-from-`PVec` ⏳ (cross-track)
The detector requires locally-born vectors so ownership is trivial. Claiming an
owned vector arriving as a `PVec` **parameter** needs a cross-function uniqueness
proof + an owned-specialized mutable ABI (caller hands off ownership; callee
thaws → mutates → refreezes). Lives on the **sound-uniqueness storage S4** track;
not startable until that lands. Revisit as an S4 customer.

- [ ] (blocked on sound-uniqueness storage S4)

---

## Cross-cutting follow-ups & maintenance notes

- ⏳ **Interim bailout guard.** `backend/prepare.tw`'s deep-module depth bailout
  calls `mutvec_repr.assign_mutvec_reprs` inside the bailout branch (otherwise
  handle slots stay boxed → `illegal cast`). Superseded and removed by
  **compiler-stack-safety Phase 2**. Keep it going through the one family-driven
  `assign_mutvec_reprs`; do **not** entrench a per-family special case there.
- ⚠️ **Adding a new family = the fixed touch-point set.** One `mutvec_specs()` row +
  one `mutvec_fns()` concat + one `MutVec<Fam>` struct + one `mutvec_families()`
  entry + one `mutvec_wasm_type` arm + (if typed reads) one `typed_vec_specs` row +
  the three `emit/runtime_abi.tw` `_byte`-style builder-name entries (the only
  remaining hardcoded per-family names).
- ⏳ **`Vector<Byte>` ↔ Buffer interop** — [vector-byte-buffer-interop.md](vector-byte-buffer-interop.md)
  (placeholder; blocked on nothing now that `PVecByte` exists — narrow follow-ups:
  typed-leaf `from_bytes`/`to_bytes`, re-bench codec go/no-go vs unboxed baseline).

## Testing discipline (every behavior-changing task)

- Self-host fixed point (`make bundle-cli`) — boot claims i64 regions, so the
  compiler compiling itself through mutvec is a live check.
- `make boot-test` + `make rust-test`.
- Per-family end-to-end fixture (`fixtures/mutvec_<fam>.tw`): WAT shows the family's
  `mutvec_*` + freeze, run result matches a boxed baseline.
- Family folds: byte-identical i64 WAT gate (per-op `twk wat --func` diff).
