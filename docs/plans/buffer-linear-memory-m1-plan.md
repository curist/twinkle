# Linear-Memory Buffer M1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land linear-memory infrastructure (wider load/store, a program memory, a bump/arena allocator) as internal compiler-runtime machinery and prove it pays off by making `Vector<Int>.sort()` faster via a dense linear-memory scratch merge sort.

**Architecture:** Twinkle is self-hosted and emits Wasm GC. The wasm IR already models a linear memory (`MemoryDef`, byte load/store, memory section emit, linker memory concat) — only the separate `bridge.wasm` uses it today. M1 adds the missing wider load/store instructions, gives the *program* module its own memory + a bump/arena allocator (new `rt.buf` runtime module), and rewrites the i64 typed sort to gather into a linear scratch region, merge through it with O(1) `i64.load`/`i64.store`, and scatter back — eliminating the `anyref` casts + GC-array copies that sank the reverted `Scratch<T>` attempt (`b915637`).

**Tech Stack:** Twinkle (`.tw`), boot self-hosted compiler, Deno/V8 Wasm-GC runtime. No stage0 changes (boot-codegen only; new Instr variants are ordinary enum additions that stage0 compiles like any enum).

**Spec:** `docs/plans/buffer-linear-memory.md`

---

## File structure

- **Modify** `boot/compiler/codegen/wasm_ir.tw` — add `I32Load`/`I32Store`/`I64Load`/`I64Store`/`F64Load`/`F64Store`/`MemorySize` Instr variants.
- **Modify** `boot/compiler/codegen/wasm.tw` — emit opcode bytes for the new instructions.
- **Modify** `boot/compiler/codegen/wat.tw` — render the new instructions to WAT text.
- **Create** `boot/compiler/codegen/runtime/buf.tw` — the `rt.buf` runtime module: program `MemoryDef`, `buf_heap_ptr` global, `buf_alloc`/`buf_mark`/`buf_reset` functions.
- **Modify** `boot/compiler/codegen/codegen.tw` — import `rt.buf` and add `rt_buf.module()` to `runtime_modules`.
- **Modify** `boot/compiler/codegen/runtime/arr.tw` — replace the i64 `sort_typed_fn` body with a dense linear-scratch merge sort (`sort_i64_dense_fn`), and swap it into the rt.arr module list.
- **Test** `boot/tests/suites/wat_suite.tw` — WAT rendering of the new instructions.
- **Test** `boot/tests/suites/api_vector_suite.tw` — existing sort correctness (the oracle; must stay green).
- **Bench** `examples/sort-bench/sort_repeat_probe.tw` — the go/no-go perf proof.

---

## Task 1: Add memory load/store/size instructions (IR + emit + WAT)

The encode match in `wasm.tw` and the render match in `wat.tw` are exhaustive over `Instr`, so the variant, its emit arm, and its WAT arm must land together or the build breaks.

**Files:**
- Modify: `boot/compiler/codegen/wasm_ir.tw` (the `Instr` enum, near `I32Load8U`/`I32Store8`/`MemoryGrow`)
- Modify: `boot/compiler/codegen/wasm.tw` (instruction encode match, near the `.I32Load8U`/`.MemoryGrow` arms)
- Modify: `boot/compiler/codegen/wat.tw` (instruction render match, near the `.I32Load8U`/`.MemoryGrow` arms)
- Test: `boot/tests/suites/wat_suite.tw`

- [ ] **Step 1: Write the failing WAT test**

`wat_suite.tw` already imports `FuncDef`, `Instr`, `WasmModule` from `wasm_ir`, `emit_wat_unlinked_for_test` from `wat`, and `assert`; it has an `empty_module()` helper. Add this test function and register it in the suite's test list the same way the existing tests are registered:

```tw
fn test_memory_instr_wat() Result<Void, String> {
  m := empty_module()
  module := WasmModule.{
    namespace: m.namespace,
    types: m.types,
    imports: m.imports,
    funcs: [
      FuncDef.{
        name: "f",
        params: [],
        results: [],
        locals: [],
        body: [
          Instr.I32Const(0),
          Instr.I64Load(3, 16),
          Instr.Drop,
          Instr.MemorySize,
          Instr.Drop,
        ],
      },
    ],
    globals: m.globals,
    tables: m.tables,
    elems: m.elems,
    exports: m.exports,
    memory_exports: m.memory_exports,
    memories: m.memories,
    data: m.data,
    start: m.start,
  }
  wat := emit_wat_unlinked_for_test(module)
  try assert.str_contains(wat, "i64.load offset=16 align=8")
  try assert.str_contains(wat, "memory.size")
  .Ok(void)
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `target/twk run boot/tests/main.tw wat`
Expected: FAIL — `I64Load`/`MemorySize` are undefined variants (parse/type error), or the WAT assertion fails.

- [ ] **Step 3: Add the Instr variants**

In `boot/compiler/codegen/wasm_ir.tw`, replace the trailing memory block of the `Instr` enum:

```tw
  I32Load8U(Int, Int),
  I32Store8(Int, Int),
  MemoryGrow,
```

with (load/store carry `(align, offset)` exactly like `I32Load8U`):

```tw
  I32Load8U(Int, Int),
  I32Store8(Int, Int),
  I32Load(Int, Int),
  I32Store(Int, Int),
  I64Load(Int, Int),
  I64Store(Int, Int),
  F64Load(Int, Int),
  F64Store(Int, Int),
  MemoryGrow,
  MemorySize,
```

- [ ] **Step 4: Add the emit arms**

In `boot/compiler/codegen/wasm.tw`, in the instruction encode match, alongside the existing `.I32Load8U`/`.I32Store8`/`.MemoryGrow` arms, add (opcodes: `i32.load=0x28`, `i64.load=0x29`, `f64.load=0x2B`, `i32.store=0x36`, `i64.store=0x37`, `f64.store=0x39`, `memory.size=0x3F`):

```tw
    .I32Load(align, offset) => {
      out := emit_u8(buf, 0x28)
      out = emit_u32_leb(out, align)
      .{ buf: emit_u32_leb(out, offset), cache }
    },
    .I32Store(align, offset) => {
      out := emit_u8(buf, 0x36)
      out = emit_u32_leb(out, align)
      .{ buf: emit_u32_leb(out, offset), cache }
    },
    .I64Load(align, offset) => {
      out := emit_u8(buf, 0x29)
      out = emit_u32_leb(out, align)
      .{ buf: emit_u32_leb(out, offset), cache }
    },
    .I64Store(align, offset) => {
      out := emit_u8(buf, 0x37)
      out = emit_u32_leb(out, align)
      .{ buf: emit_u32_leb(out, offset), cache }
    },
    .F64Load(align, offset) => {
      out := emit_u8(buf, 0x2B)
      out = emit_u32_leb(out, align)
      .{ buf: emit_u32_leb(out, offset), cache }
    },
    .F64Store(align, offset) => {
      out := emit_u8(buf, 0x39)
      out = emit_u32_leb(out, align)
      .{ buf: emit_u32_leb(out, offset), cache }
    },
    .MemorySize => {
      .{ buf: emit_u8(emit_u8(buf, 0x3F), 0x00), cache } // memory.size mem=0
    },
```

- [ ] **Step 5: Add the WAT render arms**

In `boot/compiler/codegen/wat.tw`, alongside the existing `.I32Load8U`/`.MemoryGrow` arms (note `align` is rendered as `1 << align`, matching the existing arms):

```tw
    .I32Load(align, offset) => ["${pad}i32.load offset=${offset} align=${1 << align}"],
    .I32Store(align, offset) => ["${pad}i32.store offset=${offset} align=${1 << align}"],
    .I64Load(align, offset) => ["${pad}i64.load offset=${offset} align=${1 << align}"],
    .I64Store(align, offset) => ["${pad}i64.store offset=${offset} align=${1 << align}"],
    .F64Load(align, offset) => ["${pad}f64.load offset=${offset} align=${1 << align}"],
    .F64Store(align, offset) => ["${pad}f64.store offset=${offset} align=${1 << align}"],
    .MemorySize => ["${pad}memory.size"],
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `target/twk run boot/tests/main.tw wat`
Expected: PASS.

- [ ] **Step 7: Run the codegen suites to confirm nothing else broke**

Run: `target/twk run boot/tests/main.tw codegen`
Expected: all codegen suites pass (exhaustive matches now cover the new variants).

- [ ] **Step 8: Commit**

```bash
git add boot/compiler/codegen/wasm_ir.tw boot/compiler/codegen/wasm.tw boot/compiler/codegen/wat.tw boot/tests/suites/wat_suite.tw
git commit -m "codegen: add i32/i64/f64 load/store and memory.size instructions"
```

---

## Task 2: Add the `rt.buf` runtime module (program memory + bump/arena allocator)

`align=3` in the load/store memarg means 8-byte alignment (`1 << 3`); the allocator hands out 8-byte-aligned offsets so `i64.load`/`f64.load` are aligned. The allocator grows the memory on demand via `memory.size`/`memory.grow` (page = 65536 bytes). Arena discipline is mark/reset of the bump pointer.

**Files:**
- Create: `boot/compiler/codegen/runtime/buf.tw`
- Modify: `boot/compiler/codegen/codegen.tw` (add the import and the `runtime_modules` entry)

- [ ] **Step 1: Write `rt.buf` with the memory, global, and allocator**

Create `boot/compiler/codegen/runtime/buf.tw`. Use the same `FuncDef`/`GlobalDef`/`MemoryDef` construction style as `boot/compiler/codegen/runtime/dict.tw`. Offset 0 is reserved as a null sentinel; allocation starts at 8.

```tw
//! rt.buf — program linear memory + bump/arena allocator for transient
//! (scratch) buffers. Internal compiler-runtime machinery; no user-facing
//! Buffer type yet. Lifetime is arena-scoped: buf_mark()/buf_reset(mark)
//! save and restore the bump pointer (high-water reset).

use compiler.codegen.wasm_ir.{
  ExportDef, FuncDef, GlobalDef, Instr, MemoryDef, ValType, WasmModule,
}

pub fn module() WasmModule {
  .{
    namespace: "rt.buf",
    types: [],
    imports: [],
    funcs: [alloc_fn(), mark_fn(), reset_fn()],
    globals: [
      GlobalDef.{ name: "buf_heap_ptr", mutable: true, ty: .I32, init: [.I32Const(8)] },
    ],
    tables: [],
    elems: [],
    // Exported so rt.arr (and later modules) can import the allocator. The
    // linker resolves (module, name) imports against these namespace-qualified exports.
    exports: [
      ExportDef.{ wasm_name: "buf_alloc", func_sym: "buf_alloc" },
      ExportDef.{ wasm_name: "buf_mark", func_sym: "buf_mark" },
      ExportDef.{ wasm_name: "buf_reset", func_sym: "buf_reset" },
    ],
    memory_exports: [],
    memories: [MemoryDef.{ name: "heap", min_pages: 16, max_pages: .None }],
    data: [],
    start: .None,
  }
}

// buf_alloc(nbytes) -> ptr : bump-allocate an 8-byte-aligned region, growing
// the memory by whole pages when the bump would exceed current capacity.
fn alloc_fn() FuncDef {
  // p0=nbytes; L1=cur(ptr returned), L2=new(bump end), L3=need_pages, L4=have_pages
  p_n := 0
  l_cur := 1
  l_new := 2
  l_need := 3
  l_have := 4
  .{
    name: "buf_alloc",
    params: [.I32],
    results: [.I32],
    locals: [.I32, .I32, .I32, .I32],
    body: [
      .GlobalGet("buf_heap_ptr"),
      .LocalSet(l_cur),
      // new = (cur + nbytes + 7) & ~7   (8-byte align the bump end)
      .LocalGet(l_cur),
      .LocalGet(p_n),
      .I32Add,
      .I32Const(7),
      .I32Add,
      .I32Const(-8),
      .I32And,
      .LocalSet(l_new),
      // need_pages = (new + 65535) >> 16
      .LocalGet(l_new),
      .I32Const(65535),
      .I32Add,
      .I32Const(16),
      .I32ShrU,
      .LocalSet(l_need),
      // have_pages = memory.size
      .MemorySize,
      .LocalSet(l_have),
      // if need_pages > have_pages: memory.grow(need - have); drop result
      .LocalGet(l_need),
      .LocalGet(l_have),
      .I32GtS,
      .If(
        .None,
        [
          .LocalGet(l_need),
          .LocalGet(l_have),
          .I32Sub,
          .MemoryGrow,
          .Drop,
        ],
        [],
      ),
      // buf_heap_ptr = new; return cur
      .LocalGet(l_new),
      .GlobalSet("buf_heap_ptr"),
      .LocalGet(l_cur),
    ],
  }
}

fn mark_fn() FuncDef {
  .{
    name: "buf_mark",
    params: [],
    results: [.I32],
    locals: [],
    body: [.GlobalGet("buf_heap_ptr")],
  }
}

fn reset_fn() FuncDef {
  // p0=mark
  .{
    name: "buf_reset",
    params: [.I32],
    results: [],
    locals: [],
    body: [.LocalGet(0), .GlobalSet("buf_heap_ptr")],
  }
}
```

- [ ] **Step 2: Wire `rt.buf` into the runtime module set**

In `boot/compiler/codegen/codegen.tw`, near the other runtime imports (`use compiler.codegen.runtime.arr as rt_arr`):

```tw
use compiler.codegen.runtime.buf as rt_buf
```

and in `runtime_modules`, add `rt_buf.module()` to the returned list alongside `rt_arr.module()`:

```tw
    rt_types.module(),
    rt_str.module(),
    rt_arr.module(),
    rt_dict.module(),
    rt_buf.module(),
```

- [ ] **Step 3: Build a trivial program and confirm the memory + allocator link in**

Create a throwaway `/tmp/mem_smoke.tw`:

```tw
println("ok")
```

Run: `target/twk build /tmp/mem_smoke.tw -o /tmp/mem_smoke.wat`
Then: `grep -E '\(memory|buf_alloc|buf_heap_ptr' /tmp/mem_smoke.wat`
Expected: the WAT contains `(memory` and the `buf_alloc`/`buf_heap_ptr` symbols — the program module now owns a linear memory and the allocator, even though nothing calls it yet.

- [ ] **Step 4: Confirm it runs and the full suite is green**

Run: `target/twk run /tmp/mem_smoke.tw`
Expected: prints `ok` (module with both GC and a linear memory validates and runs under V8).
Run: `target/twk run boot/tests/main.tw`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/codegen/runtime/buf.tw boot/compiler/codegen/codegen.tw
git commit -m "codegen: add rt.buf program memory + bump/arena allocator"
```

---

## Task 3: Dense i64 sort over linear scratch

Replace the GC-array scratch in the i64 typed sort with two arena-allocated linear regions. This is an adaptation of the existing `sort_typed_fn` in `arr.tw` (which already merge-sorts over GC `ArrayI64` scratch via `l_src`/`l_aux`). The dispatch already routes `Vector<Int>.sort()` → builtin `vector$sort_i64` → rt.arr function named `sort_i64` (`builtins.tw:531`, `monomorphize.tw:1466`), so naming the new function `sort_i64` keeps it wired with no dispatch change.

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw` (add `sort_i64_dense_fn`; swap it for `elem_i64().sort_typed_fn()` in the rt.arr module function list near line 151)
- Test (oracle): `boot/tests/suites/api_vector_suite.tw` (existing sort tests — unchanged, must stay green)

- [ ] **Step 1: Confirm the correctness oracle currently passes**

Run: `target/twk run boot/tests/main.tw vector`
Expected: PASS (current GC-array typed sort). These tests are the behavioral oracle — the dense rewrite must keep every one green.

- [ ] **Step 2: Import the allocator into rt.arr**

Cross-module calls resolve through explicit imports (the linker maps `(module, name)` to the exporting module's qualified symbol and rewrites `.Call(as_sym)`). In `arr.tw`'s `module()`, add these to the existing `imports:` list (the file already imports `ImportDef`; if not, add it to the `wasm_ir` `use`):

```tw
      .{ module: "rt.buf", name: "buf_alloc", as_sym: "buf_alloc", params: [.I32], results: [.I32] },
      .{ module: "rt.buf", name: "buf_mark", as_sym: "buf_mark", params: [], results: [.I32] },
      .{ module: "rt.buf", name: "buf_reset", as_sym: "buf_reset", params: [.I32], results: [] },
```

The dense sort then calls them as `.Call("buf_alloc")`, `.Call("buf_mark")`, `.Call("buf_reset")` (the `as_sym` names).

- [ ] **Step 3: Write `sort_i64_dense_fn`**

Add `sort_i64_dense_fn() FuncDef` to `arr.tw`, named `"sort_i64"`, signature `params: [pvec_null()]`, `results: [pvec_ref()]`. Adapt the body of the existing `sort_typed_fn` (i64 case) with these exact substitutions:

  1. **Arena mark.** At entry (after the `len <= 1` early return), `buf_mark()` → store in a local `l_mark`.
  2. **Scratch regions.** Replace the two `ArrayNewDefault(e.arr_ty)` allocations (`l_src`, `l_aux`) with two linear regions: `buf_alloc(n * 8)` → base offsets `l_src_base`, `l_aux_base` (i32 locals). Each element `i` of region `base` lives at byte offset `base + i*8`.
  3. **Gather.** The copy-in loop that does `ArraySet(e.arr_ty, src, i, get(vec,i))` becomes: compute `src_base + i*8`, push the unboxed i64 from `vec[i]` (the existing `get` → `RefCast` → `StructGet(boxed,0)` sequence yields the i64), then `I64Store(3, 0)`.
  4. **Merge passes.** Every read of a scratch element `arr[k]` (`ArrayGet`) becomes `I64Load(3, 0)` at `base + k*8`; every write (`ArraySet`) becomes `I64Store(3, 0)` at `base + k*8`. Comparisons stay i64 (`I64LeS`/`I64GtS`). The src/aux ping-pong swaps the two base offsets each width pass instead of swapping array refs.
  5. **Scatter.** The final loop that pushes scratch elements into the i64 builder reads each via `I64Load(3, 0)` at `src_base + i*8`, then `box_i64` + builder push (reuse the existing builder-freeze tail of `sort_typed_fn`).
  6. **Arena reset.** Immediately before the `return`/freeze of the result vector, call `buf_reset(l_mark)`.

Address arithmetic helper pattern (inline at each access; `base` and `idx` are i32 locals): `base + idx*8` = `LocalGet(base), LocalGet(idx), I32Const(8), I32Mul, I32Add`.

- [ ] **Step 4: Swap the dense function into the rt.arr module list**

In `arr.tw` near line 151, replace `elem_i64().sort_typed_fn()` with `sort_i64_dense_fn()`. Leave `elem_f64().sort_typed_fn()` unchanged (f64 dense sort is out of M1 scope).

- [ ] **Step 5: Run the sort correctness oracle**

Run: `target/twk run boot/tests/main.tw vector`
Expected: PASS — identical results to Step 1 (dense path is behavior-preserving). Pay attention to: empty vector, single element, already-sorted, reverse-sorted, duplicates, large n crossing the initial 16-page memory (forces `memory.grow`).

- [ ] **Step 6: Run the full boot suite**

Run: `target/twk run boot/tests/main.tw`
Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/codegen/runtime/arr.tw
git commit -m "codegen: dense Vector<Int> sort over linear-memory scratch"
```

---

## Task 4: Self-host fixed point and the go/no-go bench

**Files:** none (validation + decision). 

- [ ] **Step 1: Rebuild the bundled compiler through the self-host loop**

Run: `make bundle-cli`
Expected: ends with `Fixed point reached: stage3 == stage4` and `Built Deno Twinkle CLI: target/twk`. (boot.wasm now contains and uses the program memory + dense sort; the self-host loop proves the compiler compiles itself correctly with the new codegen.)

- [ ] **Step 2: Capture the baseline before measuring (reference)**

The pre-existing baselines to beat are recorded in the spec: the current recursive merge and the reverted GC-array `Scratch<T>` (which regressed plain `Vector<Int>` sort ~16%). Run the bench on the new build:

Run: `target/twk run examples/sort-bench/sort_repeat_probe.tw`
Expected output: per-pass timings for `native xs.sort()` on 1,000,000 ints (run twice for V8 tier-up). Record the `native xs.sort()` numbers.

- [ ] **Step 3: Compare against the old path**

`git stash` is not usable across the rebuilt binary; instead compare to the numbers from `main` (the recursive/GC-array sort). Build a `main`-based `twk` if a clean comparison is needed:

```bash
git worktree add /tmp/twk-main main
cd /tmp/twk-main && make quick-bundle-cli   # if target/boot.wasm is fresh there; else make bundle-cli
/tmp/twk-main/target/twk run examples/sort-bench/sort_repeat_probe.tw
```

Compare `native xs.sort()` ms: the dense linear-memory build should be **faster** than the `main` build (and not regress like `Scratch<T>` did).

- [ ] **Step 4: Record the decision**

Update `docs/plans/buffer-linear-memory.md` Status line with the measured result:
- If dense sort wins: mark M1 done, note the before/after ms, and that the linear-memory direction is validated (proceed to M2 — user-facing Buffer).
- If it does not win: record the numbers and stop at M1; the linear-memory primitive does not pay off for this workload, which is the go/no-go the spec called for.

- [ ] **Step 5: Commit**

```bash
git add docs/plans/buffer-linear-memory.md
git commit -m "buffer M1: record self-host + dense-sort bench result"
```

---

## Notes for the implementer

- **No stage0 changes.** All edits are boot-codegen. The new `Instr` variants are plain enum additions; stage0 compiles `boot/main.tw` (which constructs them) like any other enum. Do not touch `src/`.
- **Big WAT files:** use `grep`/`sed`, not full reads, when inspecting emitted `.wat`.
- **Run the formatter** on every edited `.tw` (`target/twk fmt <file>`) and the linter (`target/twk lint boot/main.tw`) before committing.
- **Unaligned safety:** the allocator 8-byte-aligns every region, so `i64`/`f64` loads use `align=3`. If a future caller needs byte access into the same region, use `I32Load8U` (`align=0`).
- **Memory is per-module:** the program memory (index 0) is independent of `bridge.wasm`'s `"staging"` memory — they are separate modules, so there is no multi-memory concern.
