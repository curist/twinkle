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
2. **The ANF pass reuses `builder_region_detect` patterns.** That module already walks ANF for owned accumulator regions; the new pass mirrors its structure. Read `boot/compiler/codegen/builder_region_detect.tw` and `builder_region.tw` in full before Phase 3.

**Rebuild loop:** `target/twk` is the compiled boot compiler. After editing any `boot/` compiler source, you MUST rebuild it before behavior changes take effect:

```bash
make bundle-cli    # rebuilds target/boot.wasm via self-host, then target/twk. Must print "Fixed point reached".
```

Use `make quick-bundle-cli` only when `target/boot.wasm` is already fresh. Each task's test steps assume you rebuild once after writing the code, before the "verify it passes" run.

**Scope guardrails (from the design):** Int-only; region needs ≥1 indexed-update; single freezable exit; persistent fallback for everything else; `builder_region` and `set_in_place` stay byte-identical (guaranteed because MutVec runs first and fully rewrites claimed regions, so no builder-visible shape survives — no exclusion parameter is added).

---

## File Structure

**Create:**
- `boot/compiler/codegen/mutvec_region.tw` — the ANF→ANF detection + rewrite pass (`rewrite_module`), the region decision record type, the exit classifier.
- `boot/tests/suites/mutvec_runtime_suite.tw` — behavioral unit tests for the runtime ops.
- `boot/tests/suites/mutvec_region_suite.tw` — positive/negative region-lowering fixtures.
- `boot/bench/mutvec_slice1_bench.tw` — the compiled-program speedup check.

**Modify:**
- `boot/compiler/codegen/runtime/types.tw:9-90` — add the `MutVecI64` GC struct type.
- `boot/compiler/codegen/runtime/arr.tw:140-210` (`module()` func list) — add the six `mutvec_*_i64` FuncDefs.
- `boot/compiler/builtins.tw` (`builtin_specs()`, `builtin_abi()`) — register the ops as internal builtins.
- `prelude/signatures/vector.tw` — signature stubs so tests can name the internal ops (boot resolves via these; symlinked from `boot/`).
- `boot/compiler/backend/prepared_ir.tw:48-59` (`ReprKind`) — add `MutVec(ElemRepr)`.
- `boot/compiler/backend/repr_assign.tw` — assign/lower `MutVec(I64)` and its wasm type.
- `boot/compiler/backend/repr_policy.tw` — map the family (reuse `ElemRepr`).
- `boot/compiler/codegen/codegen.tw:78,131` — add `mutvec_region_enabled()` and insert the pass before `builder_region.rewrite_module`.
- `boot/tests/main.tw` — register the two new suites.

**Do NOT modify:** `builder_region.tw`, `builder_region_detect.tw` (except to `pub`-export helpers the new pass shares), the `set_in_place` decision path, or any `src/` (stage0) — deferred to the flag-on slice.

---

## Phase 1: Runtime substrate (S3)

Goal: the six `mutvec_*_i64` ops exist, are registered as internal builtins named `Vector.__mutvec_*`, and each is behaviorally unit-tested from Twinkle. These are codegen-internal but exposed name-callable **for testing only**; boot/main.tw never calls them, so stage0 and self-host are unaffected (unused runtime funcs are DCE'd).

### Test conventions (apply to every suite test below)

Boot suites use `@std.testing`, not an `expect`/`to_equal` API. Every suite file has this shape (see `boot/tests/suites/stdlib_buffer_suite.tw`):

```
use @std.testing.assert as assert
use @std.testing as runner

pub fn suite() runner.Suite {
  runner
    .suite("mutvec runtime")
    .test("desc", fn() {
      // ... body ...
      try assert.equal(actual, expected)
      .Ok({})
    })
    .test("next desc", fn() { /* ... */ .Ok({}) })
}
```

- Assertions are `try assert.equal(actual, expected)`; each test body ends with `.Ok({})` and the closure returns `Result<Void, String>`.
- **Register** a suite by adding `use .suites.mutvec_runtime_suite` (and `..._region_suite`) to `boot/tests/main.tw` and appending `.suite()` to the run list exactly as the neighboring suites are registered.
- **Trap tests cannot run in-process** (a trap aborts the program). Test each trap with a standalone fixture under `boot/tests/suites/fixtures/` run as its own process, asserting a non-zero exit:

```bash
TWINKLE_MUTVEC=0 target/twk run boot/tests/suites/fixtures/mutvec_set_oob.tw; test $? -ne 0 && echo TRAP_OK
```

The fixture bodies below are shown as the **meat** of a `.test("desc", fn() { … .Ok({}) })` wrapper (or a standalone `fixtures/*.tw` for traps); wrap them per this convention when writing the file.

### Task 1: `MutVecI64` type + `new` / `len` / `push` (with growth)

**Files:**
- Modify: `boot/compiler/codegen/runtime/types.tw:9-90`
- Modify: `boot/compiler/codegen/runtime/arr.tw` (`module()` list ~140-210; add FuncDefs near the `pvec_builder_*` group ~1610-1740)
- Modify: `boot/compiler/builtins.tw`
- Modify: `prelude/signatures/vector.tw`
- Test: `boot/tests/suites/mutvec_runtime_suite.tw`

- [ ] **Step 1: Add the GC struct type.** In `types.tw`, in the `module()` type list (alongside the `ArrayI64`/`PVecI64` entries at ~26/50), add:

```
.Struct("MutVecI64", .{
  fields: [
    .{ name: .Some("data"), mutable: true, ty: .Ref(false, .Named("ArrayI64")) },
    .{ name: .Some("len"), mutable: true, ty: .I32 },
  ],
}),
```

The emitted type name is `rt_types__MutVecI64` (the `rt_types__` prefix is applied like `t_ARRAY` in `arr.tw:16`).

- [ ] **Step 2: Write the failing test.** Create `boot/tests/suites/mutvec_runtime_suite.tw` per Test conventions, with two tests:

```
.test("mutvec new/push/len", fn() {
  v := Vector.__mutvec_new_i64(4)
  v = v.__mutvec_push_i64(10)
  v = v.__mutvec_push_i64(20)
  v = v.__mutvec_push_i64(30)
  try assert.equal(v.__mutvec_len_i64(), 3)
  .Ok({})
})
.test("mutvec push grows past capacity", fn() {
  v := Vector.__mutvec_new_i64(2)
  i := 0
  for i < 10 {
    v = v.__mutvec_push_i64(i * i)
    i = i + 1
  }
  try assert.equal(v.__mutvec_len_i64(), 10)
  try assert.equal(v.__mutvec_get_i64(9), 81)   // needs Task 2's get
  .Ok({})
})
```

(`__mutvec_get_i64` lands in Task 2; keep the grow test but temporarily drop its `get` assertion — or land both green by the end of Task 2.)

- [ ] **Step 3: Register the suite in the runner.** In `boot/tests/main.tw`, add `use .suites.mutvec_runtime_suite` (and later `use .suites.mutvec_region_suite`) and append `.suite()` to the run list exactly as neighboring suites are registered.

- [ ] **Step 4: Run the test to see it fail (no rebuild needed — the builtin is undefined yet).**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — `Vector has no method '__mutvec_new_i64'` / undefined builtin.

- [ ] **Step 5: Add the runtime FuncDefs.** In `arr.tw`, add `mutvec_new_i64_fn()`, `mutvec_push_i64_fn()`, `mutvec_len_i64_fn()` and add them to the `module()` funcs list. Model them on these verified analogues:

  - `mutvec_new_i64_fn` → `MutVecI64` : allocate `ArrayI64(max(cap, MIN_CAP))` via `.ArrayNew("ArrayI64")` (default-filled) or `.ArrayNewDefault`, push `0` (i32) for `len`, `.StructNew("MutVecI64")`. Mirror the allocation style in `pvec_builder_new_fn` (`arr.tw:1610`). `MIN_CAP = 4` as an `.I32Const`.
  - `mutvec_len_i64_fn(mv)` → i32 : `.LocalGet(0)`, `.RefAsNonNull`, `.StructGet("MutVecI64", 1)` (field index 1 = `len`). Mirror the field-read style in `pvec_builder_freeze_fn` (`arr.tw:1629`).
  - `mutvec_push_i64_fn(mv, x)` → `MutVecI64` : read `len` and `capacity = array.len(data)`; if `len == capacity`, grow — allocate `ArrayI64(max(MIN_CAP, capacity*2))`, `.ArrayCopy("ArrayI64","ArrayI64")` old→new for `capacity` elems, `.StructSet("MutVecI64", 0, new_data)`; then `.ArraySet("ArrayI64")` `data[len] = x`, `.StructSet("MutVecI64", 1, len+1)`, return `mv`. The `array.len`, `array.copy`, grow-and-copy pattern appears in `push_tail_fn` (`arr.tw:1011`) and the freeze copy in `pvec_builder_freeze_fn` (`arr.tw:1664-1673`). **Growth uses `max(MIN_CAP, capacity*2)` so `capacity==0` cannot stay 0.**

- [ ] **Step 6: Register the builtins.** In `builtins.tw` `builtin_specs()`, append (near the vector builder entries):

```
rt("vector$__mutvec_new_i64", "rt.arr", "mutvec_new_i64", .Some("Vector.__mutvec_new_i64")),
rt("vector$__mutvec_push_i64", "rt.arr", "mutvec_push_i64", .Some("Vector.__mutvec_push_i64")),
rt("vector$__mutvec_len_i64", "rt.arr", "mutvec_len_i64", .Some("Vector.__mutvec_len_i64")),
```

Add ABI arms in `builtin_abi()` (default `empty_abi()` is wrong for a self-returning mutating op — copy the arm shape used by `set_in_place`/builder ops so the base arg is treated as the mutated/returned handle). Add signature stubs in `prelude/signatures/vector.tw` so the typechecker resolves the names (e.g. `pub fn __mutvec_new_i64(cap: Int) MutVecXXX` — see the note below on the source-visible return type).

  **Return-type note:** these internal ops return a `MutVecI64`, which has no source `MonoType`. For the *test-only* signature stubs, type them as returning the opaque handle the same way builder ops are typed in `prelude/signatures/vector.tw` (builder ops return an internal handle type). Follow the existing builder-op stub exactly; do not invent a new source type.

- [ ] **Step 7: Rebuild the compiler.**

Run: `make bundle-cli`
Expected: ends with `Fixed point reached`.

- [ ] **Step 8: Run the test to verify it passes.**

Run: `target/twk run boot/tests/main.tw`
Expected: the `mutvec new/push/len` test PASSES (the grow test needs Task 2's `get`).

- [ ] **Step 9: Commit.**

```bash
git add boot/compiler/codegen/runtime/types.tw boot/compiler/codegen/runtime/arr.tw boot/compiler/builtins.tw prelude/signatures/vector.tw boot/tests/suites/mutvec_runtime_suite.tw boot/tests/main.tw
git commit -m "feat(mutvec): MutVecI64 type + new/push/len runtime ops"
```

### Task 2: `get` / `set` with logical-length bounds traps

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw`, `boot/compiler/builtins.tw`, `prelude/signatures/vector.tw`
- Test: `boot/tests/suites/mutvec_runtime_suite.tw`

- [ ] **Step 1a: Write the in-process happy-path test.** Add to the suite (per Test conventions):

```
.test("mutvec set then get", fn() {
  v := Vector.__mutvec_new_i64(4)
  v = v.__mutvec_push_i64(0)
  v = v.__mutvec_push_i64(0)
  v = v.__mutvec_set_i64(1, 99)
  try assert.equal(v.__mutvec_get_i64(1), 99)
  .Ok({})
})
```

- [ ] **Step 1b: Write the trap fixtures (standalone, run as their own process — traps abort, so they can't be in-suite).** Create two files:

`boot/tests/suites/fixtures/mutvec_set_oob.tw` (index 1 is in `[len=1, capacity=8)` — must trap, proving logical-length checking, not array bounds):

```
fn f() Int {
  v := Vector.__mutvec_new_i64(8)
  v = v.__mutvec_push_i64(0)
  v = v.__mutvec_set_i64(1, 5)
  v.__mutvec_len_i64()
}
println(f())
```

`boot/tests/suites/fixtures/mutvec_get_oob.tw`:

```
fn f() Int {
  v := Vector.__mutvec_new_i64(8)
  v = v.__mutvec_push_i64(0)
  v.__mutvec_get_i64(3)
}
println(f())
```

- [ ] **Step 2: Run to see it fail.**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — `__mutvec_set_i64` undefined.

- [ ] **Step 3: Implement `get`/`set` with explicit checks.** Add `mutvec_get_i64_fn` and `mutvec_set_i64_fn` to `arr.tw` + `module()`:

  - Both compute `len := StructGet(MutVecI64,1)`; guard `if i < 0 || i >= len { unreachable/trap }` using `.I32LtS` / `.I32GeS` + `.If(.None, [.Unreachable], [])` (see the trap style already used in `arr.tw` for OOB — grep `Unreachable`). **Do not** rely on `array.set`/`array.get` bounds: `capacity >= len`, so `array` bounds would wrongly allow `[len, capacity)`.
  - `get`: after the check, `.StructGet(MutVecI64,0)` (data), `.LocalGet(i)`, `.ArrayGet("ArrayI64")`.
  - `set`: after the check, `data`, `i`, `x`, `.ArraySet("ArrayI64")`, then `.LocalGet(0)` (return `mv`).

- [ ] **Step 4: Register the two builtins** in `builtins.tw` + `prelude/signatures/vector.tw` (mirror Task 1 Step 6).

- [ ] **Step 5: Rebuild.** Run: `make bundle-cli` → `Fixed point reached`.

- [ ] **Step 6: Verify pass (in-process).** Run: `target/twk run boot/tests/main.tw` — the `set then get` test and the Task-1 grow test PASS.

- [ ] **Step 7: Verify the trap fixtures abort.** Run each and confirm a non-zero exit:

```bash
target/twk run boot/tests/suites/fixtures/mutvec_set_oob.tw; test $? -ne 0 && echo SET_TRAP_OK
target/twk run boot/tests/suites/fixtures/mutvec_get_oob.tw; test $? -ne 0 && echo GET_TRAP_OK
```

Expected: both print `*_TRAP_OK` (the program traps on the OOB logical index).

- [ ] **Step 8: Commit.**

```bash
git add boot/compiler/codegen/runtime/arr.tw boot/compiler/builtins.tw prelude/signatures/vector.tw boot/tests/suites/mutvec_runtime_suite.tw boot/tests/suites/fixtures/mutvec_set_oob.tw boot/tests/suites/fixtures/mutvec_get_oob.tw
git commit -m "feat(mutvec): get/set with logical-length bounds traps"
```

### Task 3: `make` with `max(0, n)` clamp

**Files:** `boot/compiler/codegen/runtime/arr.tw`, `builtins.tw`, `prelude/signatures/vector.tw`, `boot/tests/suites/mutvec_runtime_suite.tw`

- [ ] **Step 1: Write the failing tests** (suite bodies, per Test conventions):

```
.test("mutvec make prefilled", fn() {
  v := Vector.__mutvec_make_i64(3, 7)
  try assert.equal(v.__mutvec_len_i64(), 3)
  try assert.equal(v.__mutvec_get_i64(0), 7)
  try assert.equal(v.__mutvec_get_i64(2), 7)
  .Ok({})
})
.test("mutvec make zero is empty", fn() {
  v := Vector.__mutvec_make_i64(0, 7)
  try assert.equal(v.__mutvec_len_i64(), 0)
  .Ok({})
})
.test("mutvec make negative is empty (no trap)", fn() {
  v := Vector.__mutvec_make_i64(0 - 2, 7)
  try assert.equal(v.__mutvec_len_i64(), 0)
  .Ok({})
})
```

- [ ] **Step 2: Run to see it fail.** `target/twk run boot/tests/main.tw` → FAIL undefined.

- [ ] **Step 3: Implement `mutvec_make_i64_fn(n, fill)`.** Model on `pvec_make_fn` (`arr.tw:1698`): compute `actual_len := max(0, n)` via `.LocalGet(n)`, `.I32Const(0)`, `.I32GtS` + `.Select` (or an `if`); allocate `ArrayI64(max(actual_len, MIN_CAP))`; fill `data[0..actual_len)` with `fill` using the `pvec_make_fn` loop shape (its `i >= size` guard already exits immediately when `size <= 0`, which is exactly the empty-on-negative behavior to preserve); set `len = actual_len`; `StructNew`. Register builtin + signature stub.

- [ ] **Step 4: Rebuild.** `make bundle-cli` → `Fixed point reached`.

- [ ] **Step 5: Verify pass.** `target/twk run boot/tests/main.tw` → PASS.

- [ ] **Step 6: Commit.**

```bash
git add -A && git commit -m "feat(mutvec): make with max(0,n) clamp (negative -> empty)"
```

### Task 4: `freeze` → `PVecI64`

**Files:** `boot/compiler/codegen/runtime/arr.tw`, `builtins.tw`, `prelude/signatures/vector.tw`, `boot/tests/suites/mutvec_runtime_suite.tw`

- [ ] **Step 1: Write the failing test** (suite body, per Test conventions):

```
.test("mutvec freeze to persistent vector", fn() {
  v := Vector.__mutvec_new_i64(2)
  i := 0
  for i < 5 {
    v = v.__mutvec_push_i64(i * 2)
    i = i + 1
  }
  frozen: Vector<Int> = v.__mutvec_freeze_i64()
  try assert.equal(frozen.len(), 5)
  try assert.equal(frozen[4], 8)
  .Ok({})
})
```

- [ ] **Step 2: Run to see it fail.** FAIL undefined.

- [ ] **Step 3: Implement `mutvec_freeze_i64_fn(mv) -> PVecI64`.** Reuse the typed builder: `Call("builder_new_i64")`, loop `j in 0..len` pushing `data[j]` via `Call("builder_push_i64_raw")`, then `Call("builder_freeze_i64")`. This mirrors `pvec_make_fn`'s builder loop (`arr.tw:1698`) but reads source elements from the `MutVecI64` backing rather than a constant fill. Register builtin; the signature stub returns `Vector<Int>` (the real persistent type).

- [ ] **Step 4: Rebuild.** `make bundle-cli` → `Fixed point reached`.

- [ ] **Step 5: Verify pass.** `target/twk run boot/tests/main.tw` → PASS.

- [ ] **Step 6: Confirm self-host unaffected.** Run: `make boot-test` (full boot suite) → all green. The new runtime funcs are unused by `boot/main.tw` and DCE'd from it.

- [ ] **Step 7: Commit.**

```bash
git add -A && git commit -m "feat(mutvec): freeze to PVecI64 via typed builder"
```

---

## Phase 2: Physical repr (`ReprKind.MutVec`)

Goal: the backend can carry a slot as `MutVec(I64)` and lower it to the `rt_types__MutVecI64` wasm ref, distinct from `TypedVec(I64)` and never `Anyref`. This is a supporting change; it is exercised end-to-end in Phase 4.

### Task 5: Add the `MutVec(ElemRepr)` repr variant

**Files:**
- Modify: `boot/compiler/backend/prepared_ir.tw:48-59` (`ReprKind`)
- Modify: `boot/compiler/backend/repr_assign.tw`
- Modify: `boot/compiler/backend/repr_policy.tw`
- Modify: `boot/compiler/backend/verify_slots.tw` (if it exhaustively matches `ReprKind`)

- [ ] **Step 1: Add the variant.** In `prepared_ir.tw`, add to `ReprKind`:

```
MutVec(ElemRepr),
```

- [ ] **Step 2: Make the compiler build (exhaustive-match fallout).** Rebuild and fix every non-exhaustive `case … ReprKind` the compiler now flags:

Run: `cargo run --release -- build boot/main.tw -o /tmp/x.wasm` (fast boot typecheck via stage0, ~10s)
Expected: type errors listing each `case` on `ReprKind` missing `MutVec`. For each, add a `MutVec(er)` arm:
  - wasm-type lowering (`repr_assign.tw` / wherever `TypedVec(er)` maps to the `PVecI64` ref): map `MutVec(I64)` → `.Ref(true, .Named("MutVecI64"))`.
  - `verify_slots.tw` expected-wasm-type: same ref.
  - any repr-display/debug arm: `"MutVec(i64)"`.
Repeat until `cargo run … build` succeeds.

- [ ] **Step 3: Rebuild the compiler.** `make bundle-cli` → `Fixed point reached` (proves the new variant, unused so far, doesn't perturb self-host).

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/backend/prepared_ir.tw boot/compiler/backend/repr_assign.tw boot/compiler/backend/repr_policy.tw boot/compiler/backend/verify_slots.tw
git commit -m "feat(mutvec): ReprKind.MutVec(ElemRepr) physical repr -> rt_types__MutVecI64"
```

---

## Phase 3: Region detection (analysis, no emit yet)

Goal: `mutvec_region.tw` can identify a claimable region and produce a region decision record, observable via a debug dump, without changing emitted code yet. Read `builder_region_detect.tw` fully first.

### Task 6: Region decision record type + eligibility predicate

**Files:**
- Create: `boot/compiler/codegen/mutvec_region.tw`
- Modify: `boot/compiler/codegen/builder_region_detect.tw` (only to `pub`-export helpers you reuse: local-reference scans `expr_references`/`op_references_deep`, seed detection, loop-arm helpers)
- Test: `boot/tests/suites/mutvec_region_suite.tw`

- [ ] **Step 1: Define the region decision record.** In `mutvec_region.tw`, define the full lifecycle schema from the design (§4 "Region decision record"):

```
pub type MutVecRegion = .{
  region_id: Int,
  proof_id: String,
  begin_local: LocalId,          // producer/handle SSA local
  producer: MutVecProducer,      // { NewSeed, MakeSeed, CollectSeed }
  source_repr: String,           // pre-change physical repr, for audit
  op_sites: Vector<MutVecOpSite>,// each in-region op + its shape
  freeze_at: MutVecExit,         // the single freezable exit
  post_repr: String,             // "TypedVec(i64)" after freeze
  invalidate_after: AnfPoint,    // handle dead past here
}

pub type MutVecProducer = { NewSeed, MakeSeed, CollectSeed }
pub type MutVecOpSite = .{ site: AnfPoint, kind: MutVecOpKind }
pub type MutVecOpKind = { New, Make, Push, Set, Get, Len }
pub type MutVecExit = .{ at: AnfPoint, into: MutVecExitKind }
pub type MutVecExitKind = { Return, PersistentConsumer }
```

Use the concrete `LocalId` / ANF point types from `compiler.anf` (see `builder_region_detect.tw:7` imports); match their real names.

- [ ] **Step 2: Write the failing detection test.** In `mutvec_region_suite.tw`, add a test that calls a `pub fn detect_regions(func_anf) Vector<MutVecRegion>` on a hand-built or parsed fixture and asserts one region is found for the collect-born + indexed-update shape, and zero for an append-only shape. Since building ANF by hand is verbose, prefer driving detection through a debug entry: add `pub fn debug_regions(src: String) Vector<String>` that lowers a source snippet to ANF and returns claimed-region proof ids. Test:

```
.test("detects collect-born indexed-update region", fn() {
  ids := mutvec_region.debug_regions("
    fn f(n: Int) Vector<Int> {
      xs := collect i in range(n) { i }
      xs[0] = 42
      xs
    }
  ")
  try assert.equal(ids.len(), 1)
  .Ok({})
})
.test("does not claim append-only region", fn() {
  ids := mutvec_region.debug_regions("
    fn f(n: Int) Vector<Int> {
      acc: Vector<Int> = []
      for i in range(n) { acc = acc.append(i) }
      acc
    }
  ")
  try assert.equal(ids.len(), 0)
  .Ok({})
})
```

If a source→ANF debug harness is impractical, instead assert via `twk ir` (Task 11) and mark these as pending until then — but prefer the debug entry for granular TDD.

- [ ] **Step 3: Run to see it fail.** `target/twk run boot/tests/main.tw` → FAIL (`debug_regions` undefined).

- [ ] **Step 4: Implement the eligibility predicate.** In `mutvec_region.tw`, walk each function's ANF (mirroring `builder_region_detect.detect_in_expr`/`detect_in_op`). A region is eligible iff ALL hold:
  - producer is `collect` builder chain, `Vector.make`, or `[]`-seed, binding local `L`, element type `Int` (check via the mono type of `L`);
  - `L` (through SSA rebinds) has **≥1 indexed-update** (`vector$set_unsafe`) — the genuinely-new shape;
  - every other use of `L` is a supported op (`set`, append `push`, index read, `len`) or the single boundary;
  - ownership: `L` is a local, single-writer per step (reuse the owned/liveness helpers `builder_region_detect` already uses).
  Return `MutVecRegion` records; do not rewrite anything yet. Add `debug_regions`.

- [ ] **Step 5: Rebuild.** `make bundle-cli` → `Fixed point reached`.

- [ ] **Step 6: Verify pass.** `target/twk run boot/tests/main.tw` → the two detection tests PASS.

- [ ] **Step 7: Commit.**

```bash
git add boot/compiler/codegen/mutvec_region.tw boot/compiler/codegen/builder_region_detect.tw boot/tests/suites/mutvec_region_suite.tw
git commit -m "feat(mutvec): region decision record + eligibility detection"
```

### Task 7: Exit classifier (reject unsound exits)

**Files:** `boot/compiler/codegen/mutvec_region.tw`, `boot/tests/suites/mutvec_region_suite.tw`

- [ ] **Step 1: Write failing tests — one per rejected class.** Add `debug_regions` tests asserting `ids.len() == 0` for each: handle passed to a call; closure capture of the handle; stored into a record/variant/dict/vector; `break value` carrying the handle; early `return` of the handle (multiple exits); `try`/early-return arm; non-`Int` element; unsupported op (`concat`/`slice`); and one positive control (single-return boundary) asserting `== 1`. Example:

```
.test("rejects handle passed to a call", fn() {
  ids := mutvec_region.debug_regions("
    fn sink(v: Vector<Int>) Int { v.len() }
    fn f(n: Int) Int {
      xs := collect i in range(n) { i }
      xs[0] = 1
      sink(xs)     // handle-to-call before any freeze boundary
    }
  ")
  try assert.equal(ids.len(), 0)
  .Ok({})
})
```

Write the remaining eleven analogously (do not abbreviate — each rejected class needs its own fixture).

- [ ] **Step 2: Run to see them fail.** Some will wrongly pass (region still claimed) until the classifier lands.

- [ ] **Step 3: Implement the exit classifier.** In `mutvec_region.tw`, before accepting a region, require **exactly one** exit that carries the live handle and that it is a freeze-point (a `return` of the handle, or a last use feeding a persistent-`Vector` consumer where the freeze is inserted before it). Reject (drop the region → persistent fallback) if the handle: reaches >1 exit; is passed to any call while still a handle; is captured by a closure; is stored into an aggregate; leaves via `break value` / `try` / early-return; or reaches a host/import boundary. Reuse `builder_region_detect`'s reference-scanning helpers to find every use of the handle local.

- [ ] **Step 4: Rebuild.** `make bundle-cli` → `Fixed point reached`.

- [ ] **Step 5: Verify pass.** `target/twk run boot/tests/main.tw` → all twelve classifier tests PASS.

- [ ] **Step 6: Commit.**

```bash
git add -A && git commit -m "feat(mutvec): exit classifier rejects unsound region exits"
```

---

## Phase 4: Emit + repr handoff + flag wiring

Goal: with `TWINKLE_MUTVEC=1`, claimed regions emit `mutvec_*` ops with one relocated freeze and correct results; with the flag off, output is byte-identical to today.

### Task 8: `rewrite_module` — emit ops and relocate the freeze

**Files:** `boot/compiler/codegen/mutvec_region.tw`, `boot/tests/suites/mutvec_region_suite.tw`

- [ ] **Step 1: Write the failing behavioral fixture.** The pass reads the accepted region record (Task 6/7) and rewrites. Prefer a standalone fixture run over an in-process helper, since the flag is read from the env at compile time.

Create `boot/tests/suites/fixtures/mutvec_indexed_run.tw`:

```
fn f() Vector<Int> {
  xs := collect i in range(5) { i }
  xs[2] = 99
  xs
}
println(f()[2])
println(f()[4])
```

Assert (this is the "run to see it fail" and later the "verify pass" command):

```bash
TWINKLE_MUTVEC=1 target/twk run boot/tests/suites/fixtures/mutvec_indexed_run.tw
# Expected after Task 8: prints
# 99
# 4
```

- [ ] **Step 2: Run to see it fail.** `TWINKLE_MUTVEC=1 target/twk run boot/tests/suites/fixtures/mutvec_indexed_run.tw` — before `rewrite_module` exists this is a no-op path (still correct output via `set_in_place`, but Step 4's WAT check in Task 9 confirms no `mutvec_*` yet). The failing signal for this task is the WAT check: `TWINKLE_MUTVEC=1 target/twk wat …mutvec_indexed_run.tw --func f --calls` shows **no** `mutvec_` calls.

- [ ] **Step 3: Implement `pub fn rewrite_module(m, b) AnfModule`.** For each accepted `MutVecRegion`: replace the producer with `Call(vector$__mutvec_new_i64 | __mutvec_make_i64)`; rewrite in-region append→`__mutvec_push_i64`, `vector$set_unsafe`→`__mutvec_set_i64`, index read→`__mutvec_get_i64`, len→`__mutvec_len_i64`; **remove the producer's immediate freeze and insert `__mutvec_freeze_i64` at `freeze_at`**. Mirror the ANF-splicing style of `builder_region.rewrite_module` (`builder_region.tw:374`). Leave unclaimed code untouched. (The pass is not yet wired into `codegen.tw` — that is Task 9 — so at this step call `rewrite_module` from the `debug_regions`/a direct unit path, or land Task 9's wiring first if you prefer to test through the flag. Recommended: do Task 9 immediately after Step 3, then run Step 5.)

- [ ] **Step 4: Rebuild.** `make bundle-cli` → `Fixed point reached`.

- [ ] **Step 5: Verify pass.** `TWINKLE_MUTVEC=1 target/twk run boot/tests/suites/fixtures/mutvec_indexed_run.tw` prints `99` then `4`; and `TWINKLE_MUTVEC=1 target/twk wat …mutvec_indexed_run.tw --func f --calls` shows `mutvec_new_i64`/`mutvec_set_i64`/`mutvec_freeze_i64` with exactly one freeze.

- [ ] **Step 6: Commit.**

```bash
git add -A && git commit -m "feat(mutvec): rewrite_module emits mutvec ops + relocates freeze"
```

### Task 9: Wire the flag + pass ordering into codegen

**Files:** `boot/compiler/codegen/codegen.tw:78,126-136`

- [ ] **Step 1: Add the flag.** Near `variant_specialize_enabled()` (`codegen.tw:78`):

```
// MutVec region lowering (storage slice 1) is OFF by default; TWINKLE_MUTVEC=1 enables it.
// Off during self-host so stage0/fixed point are untouched until the stage0 mirror lands.
fn mutvec_region_enabled() Bool {
  case proc.env("TWINKLE_MUTVEC") {
    .Some(v) => v == "1",
    .None => false,
  }
}
```

- [ ] **Step 2: Insert the pass before builder_region.** Change the `anf_prime` step (`codegen.tw:131`) so MutVec runs first when enabled:

```
anf_mv := if mutvec_region_enabled() {
  mutvec_region.rewrite_module(anf, builtins)
} else {
  anf
}
anf_prime := builder_region.rewrite_module(anf_mv, builtins)
```

Add the `use compiler.codegen.mutvec_region` import. (No exclusion parameter to `builder_region` — claimed regions no longer contain builder-visible shapes.)

- [ ] **Step 3: Write the flag-off regression test.** Compile the Task-8 fixture WITHOUT the flag and assert it still emits `set_in_place` (not `mutvec_*`):

Run: `target/twk wat boot/tests/suites/fixtures/mutvec_indexed.tw --func f --calls` (create that fixture file)
Expected: contains `rt_arr__set_in_place`, no `mutvec_`.

And with the flag:

Run: `TWINKLE_MUTVEC=1 target/twk wat boot/tests/suites/fixtures/mutvec_indexed.tw --func f --calls`
Expected: contains `mutvec_new_i64`/`mutvec_set_i64`/`mutvec_freeze_i64`, exactly one `freeze`, no `set_in_place`.

- [ ] **Step 4: Rebuild + verify.** `make bundle-cli` → run both `wat` commands, confirm expected output.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/codegen/codegen.tw boot/tests/suites/fixtures/mutvec_indexed.tw
git commit -m "feat(mutvec): gate region pass behind TWINKLE_MUTVEC, run before builder_region"
```

### Task 10: Repr handoff — mark handle slots `MutVec(I64)` in prepare

**Files:** `boot/compiler/backend/repr_assign.tw`, `boot/compiler/backend/prepare.tw`, `boot/compiler/codegen/mutvec_region.tw` (surface the record to the backend)

- [ ] **Step 1: Write the failing test.** With the flag on, assert the handle slot's wasm type is the `MutVecI64` ref (not boxed `PVec`, not `PVecI64`) via a WAT check on locals:

Run: `TWINKLE_MUTVEC=1 target/twk build boot/tests/suites/fixtures/mutvec_indexed.tw -o /tmp/mv.wat` then grep the `f` function's locals.
Expected: a local typed `(ref null $rt_types__MutVecI64)`; the returned value is `PVecI64`.

- [ ] **Step 2: Run to see it fail.** Likely the slot is boxed/`anyref` or the module fails verification.

- [ ] **Step 3: Implement the handoff.** Make the `MutVecRegion` records available to backend prepare (thread them alongside the ANF, mirroring how builder-region facts or variant routes are surfaced). In `repr_assign.tw`, mark each region's `begin_local`/handle slots `MutVec(I64)` and the post-freeze slot `TypedVec(I64)`, driven by the record — not re-derived. Ensure no region slot resolves to `OpaqueAnyref`.

- [ ] **Step 4: Rebuild + verify.** `make bundle-cli`; re-run the WAT check → the handle local is `$rt_types__MutVecI64`. Also run `TWINKLE_MUTVEC=1 target/twk run boot/tests/suites/fixtures/mutvec_indexed.tw` and confirm correct output (end-to-end through real repr).

- [ ] **Step 5: Commit.**

```bash
git add -A && git commit -m "feat(mutvec): repr_assign marks handle slots MutVec(I64) from region record"
```

---

## Phase 5: Census, negatives, regression, performance

### Task 11: Census region-audit rows

**Files:** the census/`--sites` renderer (find via `grep -rn "would_use\|--sites\|census" boot/compiler`), `boot/tests/suites/mutvec_region_suite.tw`

- [ ] **Step 1: Write the failing test.** Assert `twk ir --census --sites` on the fixture (flag on) prints a `mutvec` region row naming the region id, producer/begin site, family, op sites, exit, and `would_use`/`consumed` state:

Run: `TWINKLE_MUTVEC=1 target/twk ir boot/tests/suites/fixtures/mutvec_indexed.tw --census --sites`
Expected: a line matching `mutvec` with the region proof id and `consumed`.

- [ ] **Step 2: Run to see it fail.** No `mutvec` row yet.

- [ ] **Step 3: Implement the row.** Add a `mutvec` region-audit row to the census renderer, sourced from the `MutVecRegion` records (additive; do not alter existing rows). Pin the exact columns here: `region_id | begin_site | family=MutVecI64 | op_sites=[…] | exit | would_use | consumed`.

- [ ] **Step 4: Rebuild + verify.** `make bundle-cli`; re-run → row present. Also confirm a flag-off run and an unclaimed fixture show **no** `mutvec` rows and unchanged existing rows.

- [ ] **Step 5: Commit.**

```bash
git add -A && git commit -m "feat(mutvec): census --sites region-audit rows"
```

### Task 12: Full negative + positive fixture matrix

**Files:** `boot/tests/suites/mutvec_region_suite.tw`, `boot/tests/suites/fixtures/`

- [ ] **Step 1: Ensure a positive fixture per producer.** collect-born+indexed-update; `Vector.make`-born+indexed-update; +append; in-region `get`/`len`. Each: flag-on WAT shows `mutvec_*` + one freeze; flag-on run gives the correct result.

- [ ] **Step 2: Ensure a negative fixture per rejected class** (from Task 7, now checked at the emit level too): not-owned/aliased, non-Int, append-only, unsupported op (concat/slice), multiple exits, early return, `break value`, `try`-arm, capture, escaping-aggregate store, handle-to-call, host boundary. Each: flag-on WAT shows **no** `mutvec_*` and the existing path unchanged.

- [ ] **Step 3: Run.** `target/twk run boot/tests/main.tw` → all PASS.

- [ ] **Step 4: Commit.**

```bash
git add -A && git commit -m "test(mutvec): full positive/negative region fixture matrix"
```

### Task 13: Self-host + regression gate

- [ ] **Step 1: Full self-host.** Run: `make bundle-cli` → `Fixed point reached` (flag off by default → boot self-compilation byte-identical).
- [ ] **Step 2: Boot suite.** Run: `make boot-test` → all green.
- [ ] **Step 3: Rust suite (reference).** Run: `make rust-test` → green (stage0 untouched, so this should be unaffected; confirms no accidental `src/` coupling).
- [ ] **Step 4: Census regression.** Diff `target/twk ir boot/main.tw --census --sites` (flag off) against a pre-change capture → only expected differences (ideally none, since flag off).
- [ ] **Step 5: Commit any fixture/expected-output updates.**

```bash
git add -A && git commit -m "test(mutvec): self-host + regression gate green (flag off)"
```

### Task 14: Performance validation (end-of-slice gate)

**Files:** `boot/bench/mutvec_slice1_bench.tw`

- [ ] **Step 1: Write the bench.** Port the `mutvec_spike.tw` indexed-update shape as ordinary owned Twinkle (`collect` + `xs[i]=v` loop over n×k + return), timed with `@std.date`, at n ∈ {65536, 1048576}.
- [ ] **Step 2: Baseline (flag off).** Run: `target/twk run boot/bench/mutvec_slice1_bench.tw` — record mutate times (this is the current `set_in_place` path).
- [ ] **Step 3: MutVec (flag on).** Run: `TWINKLE_MUTVEC=1 target/twk run boot/bench/mutvec_slice1_bench.tw` — record mutate times.
- [ ] **Step 4: Assert the win.** Flag-on mutate should be ~an order of magnitude faster at large n, tracking the spike (`spike-tier0-vector.md`: 15–35× on the isolated microbench; expect somewhat less end-to-end but clearly large). If not, stop and diagnose (likely the handle slot fell back to boxed repr — re-check Task 10).
- [ ] **Step 5: Commit + record.**

```bash
git add boot/bench/mutvec_slice1_bench.tw
git commit -m "bench(mutvec): slice 1 speedup validation vs set_in_place"
```

- [ ] **Step 6: Update the design/README status.** Note slice 1 landed (flag-gated), the measured speedup, and that the stage0 mirror + flag-on-by-default is the next slice, in `docs/plans/sound-uniqueness/storage/README.md` (S3 line) and `mutvec-slice1-design.md` status.

```bash
git add docs/plans/sound-uniqueness/storage/README.md docs/plans/sound-uniqueness/storage/mutvec-slice1-design.md
git commit -m "docs(mutvec): record slice 1 landed (flag-gated) + next-slice note"
```

---

## Deferred to later slices (explicitly out of scope)

- **stage0 mirror** (`src/runtime/arr.rs`, `src/runtime/types.rs`, `src/ir/lower.rs`, `src/intrinsics/*`, `src/codegen/prelude.rs`, `src/types/env.rs`) and **flipping `TWINKLE_MUTVEC` on by default** — required together, because turning the pass on during self-host changes boot's own emitted code, so stage0 must produce identical mutvec output to hold the fixed point.
- `Bool` / `Float` / boxed element families.
- Thaw-from-`PVec` for param-sourced owned vectors, and owned-specialized mutable ABI across calls (S4).
- Standalone append-only loops (no indexed-update) — stay on the boxed builder until the unified-pass convergence (Approach A).
- Removing the `Vector.__mutvec_*` test-only source visibility once codegen is the sole caller (optional cleanup).
```
