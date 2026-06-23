# Buffer Linear-Memory M2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a user-facing, opt-in, manually-managed, first-class `Buffer` linear-memory type (plus typed views) in `@std.buffer`, as the sandboxed-low-level abstraction over linear memory.

**Architecture:** A free-list `rt.buf` allocator + word/byte load-store runtime funcs are exposed as internal-only `__buf_*` intrinsics. `@std.buffer` (pure Twinkle) wraps them in a `Buffer` record + three view submodules (`u8view`/`i64view`/`f64view`, transparently re-exported). The `rt.arr→rt.buf` dependency is severed (drop the parity-only dense sort) and the linear memory is emitted only when a buffer op survives DCE, so non-buffer programs pay nothing.

**Tech Stack:** Boot compiler only (`boot/`), Twinkle (`.tw`), hand-written Wasm IR (the `.Instr` DSL). No Rust/stage0 changes — `boot/main.tw` never imports `@std.buffer`, so the self-host fixed point + boot test suite are the gates.

**Design doc:** `docs/plans/buffer-linear-memory.md` (M2 section). Read it before starting.

---

## Orientation (read once before Task 1)

**Key files:**
- `boot/compiler/codegen/runtime/buf.tw` — the `rt.buf` runtime module (memory + allocator + load/store funcs). M1/M3 built a bump allocator + `buf_load_u8`/`buf_store_u8`.
- `boot/compiler/builtins.tw` — `builtin_abi(name)` (the i64↔i32 wasm-type bridge, ~line 117) and `builtin_specs()` (`rt(...)` entries, ~line 445).
- `boot/compiler/base_env.tw` — `builtin_env()` currently registers `__buf_*` globally (~line 423–432, the M3 leak); `add_internal_host_builtins()` (~line 436) is the internal-only path used by stdlib modules.
- `boot/compiler/lower_core/context.tw:44` — already aliases any `buf_*` runtime func to `__buf_*` (no change needed for new `buf_*` ops).
- `boot/compiler/codegen/codegen.tw` — `runtime_modules()` (~line 55) lists runtime modules; `link_program()` (~line 118) links them then runs `eliminate_dead_wasm()`.
- `boot/compiler/codegen/runtime/arr.tw` — imports `rt.buf` (line ~179) and contains `sort_i64_dense_fn` (the parity-only linear sort, func list ~line 151, body ~3740–4066).
- `boot/stdlib/view.tw`, `boot/stdlib/tuple.tw` + `boot/stdlib/tuple/triple.tw` — the stdlib-module and submodule-re-export precedents to mirror.
- `boot/tests/main.tw` — registers suites (`use .suites.X` + `X.suite()`); `boot/tests/suites/stdlib_path_suite.tw` is a model suite.

**Build / verify commands:**
- Fast boot-only iteration (after editing boot codegen/builtins/stdlib):
  ```bash
  python3 tools/generate_core_lib.py            # regen embedded stdlib (gitignored)
  cargo build --release                          # stage0
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm   # stage0 -> boot v1
  BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
    tools/js_runtime/deno_main.mjs run <prog.tw>
  ```
- Full gate (run before declaring a phase done): `make bundle-cli` (must print "Fixed point reached") then `make boot-test`.
- WAT inspection (for memory-emission / severance checks): `target/twk build <prog.tw> -o /tmp/out.wat` then `grep` the text. Note: `target/twk` is stale until `make bundle-cli`; for pre-bundle WAT use the stage1 path above with a `.wat` output.

**Commit discipline:** one commit per task; messages focus on what/why/how, not metrics; end with the Co-Authored-By trailer. Run `target/twk fmt <file>` + `target/twk lint <file>` on every edited `.tw` before committing.

---

## Task 1: Remove the M3 probe artifacts

Clears throwaway scaffolding so later internalization of `__buf_*` doesn't break probe code that calls them globally.

**Files:**
- Delete: `boot/lib/buf_codec.tw`
- Delete: `boot/bench/buf_codec_bench.tw`
- Delete: `boot/bench/md5_linear_bench.tw`
- Delete: `boot/tests/suites/buf_codec_suite.tw`
- Modify: `boot/tests/main.tw` (remove the `buf_codec_suite` registration)

- [ ] **Step 1: Find the probe suite's registration lines**

Run: `grep -n 'buf_codec' boot/tests/main.tw`
Expected: two lines — a `use .suites.buf_codec_suite` and a `buf_codec_suite.suite()` entry.

- [ ] **Step 2: Delete the four probe files**

```bash
git rm boot/lib/buf_codec.tw boot/bench/buf_codec_bench.tw \
       boot/bench/md5_linear_bench.tw boot/tests/suites/buf_codec_suite.tw
```

- [ ] **Step 3: Remove the suite registration from main.tw**

Delete the `use .suites.buf_codec_suite` line and remove `buf_codec_suite.suite()` from the suite list (mind the surrounding commas/brackets).

- [ ] **Step 4: Verify nothing else references the probes**

Run: `grep -rn 'buf_codec\|md5_linear' boot/ | grep -v 'buffer-linear-memory'`
Expected: no matches.

- [ ] **Step 5: Verify the test suite still builds/runs**

Run: `make boot-test`
Expected: all suites pass (the probe suite is simply gone).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "buffer: remove throwaway M3 probe artifacts

Drop the buf_codec/md5 probes and their suite ahead of internalizing
__buf_*; their findings live in docs/plans/buffer-linear-memory.md.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Free-list allocator + word/float load-store in `rt.buf`

Replace the bump allocator with a real free-list (`buf_alloc`/`buf_free`) and add i64/f64 accessors. Wire the new ops as `__buf_*` intrinsics — **kept global for now** (Task 4 internalizes them) so this task is verifiable in isolation.

**Files:**
- Modify: `boot/compiler/codegen/runtime/buf.tw` (allocator rewrite + new funcs + a second global)
- Modify: `boot/compiler/builtins.tw` (`builtin_abi` arms + `builtin_specs` rt entries)
- Modify: `boot/compiler/base_env.tw` (global `__buf_*` signatures — temporary)

### Allocator design (implement in `buf.tw`)

**Memory layout.** Bytes 0–7 reserved (so payload pointer 0 = null sentinel). Two mutable i32 globals: `buf_heap_ptr` (bump frontier, init 8) and `buf_free_head` (address-ordered free-list head, init 0 = empty). Each block = an 8-byte header followed by an 8-aligned payload:
- header `+0`: `size` = payload bytes (multiple of 8).
- header `+4`: for a **free** block, `next` free-block address (address-ordered, 0 = end); for an **allocated** block, `0`.
- `buf_alloc` returns `block + 8`; `buf_free(p)` uses `block = p - 8`.

**`buf_alloc(n) -> ptr` (params `[.I32]`, results `[.I32]`):**
```
need = (n + 7) & ~7                       // 8-align the payload
prev = 0 ; cur = buf_free_head
while cur != 0:
  size = i32.load[cur+0]
  if size >= need:
    if size >= need + 16:                 // split: room for header(8)+payload(>=8)
      newblk = cur + 8 + need
      i32.store[newblk+0] = size - need - 8
      i32.store[newblk+4] = i32.load[cur+4]
      i32.store[cur+0] = need
      if prev == 0: buf_free_head = newblk else i32.store[prev+4] = newblk
    else:                                 // take whole block
      if prev == 0: buf_free_head = i32.load[cur+4] else i32.store[prev+4] = i32.load[cur+4]
    i32.store[cur+4] = 0
    return cur + 8
  prev = cur ; cur = i32.load[cur+4]
// no fit: bump-allocate a fresh block
block = buf_heap_ptr
end = block + 8 + need
grow_to(end)                              // see below
i32.store[block+0] = need
i32.store[block+4] = 0
buf_heap_ptr = end
return block + 8
```

**`grow_to(end)` (inline, mirror the existing `alloc_fn` page-growth):**
```
need_pages = (end + 65535) >> 16
have = memory.size
if need_pages > have:
  if memory.grow(need_pages - have) == -1: unreachable   // trap on grow failure
```

**`buf_free(ptr)` (params `[.I32]`, results `[]`):**
```
block = ptr - 8
size  = i32.load[block+0]
// find insertion point: first free node with address > block
prev = 0 ; cur = buf_free_head
while cur != 0 and cur < block:
  prev = cur ; cur = i32.load[cur+4]
// coalesce forward with cur if adjacent
if cur != 0 and block + 8 + size == cur:
  size = size + 8 + i32.load[cur+0]
  i32.store[block+0] = size
  i32.store[block+4] = i32.load[cur+4]
else:
  i32.store[block+4] = cur
// link from prev, coalescing backward if adjacent
if prev == 0:
  buf_free_head = block
else if prev + 8 + i32.load[prev+0] == block:
  i32.store[prev+0] = i32.load[prev+0] + 8 + size
  i32.store[prev+4] = i32.load[block+4]
else:
  i32.store[prev+4] = block
```

**Accessor funcs (verbatim `.Instr`, modeled on the existing `load_u8_fn`):** address = `base + off`, memarg `(0, 0)` (align hint 0 = always valid even unaligned).
- `buf_load_i64(base, off) -> i64`: `[.LocalGet(0), .LocalGet(1), .I32Add, .I64Load(0, 0)]`
- `buf_store_i64(base, off, v)`: `[.LocalGet(0), .LocalGet(1), .I32Add, .LocalGet(2), .I64Store(0, 0)]`
- `buf_load_f64(base, off) -> f64`: `[.LocalGet(0), .LocalGet(1), .I32Add, .F64Load(0, 0)]`
- `buf_store_f64(base, off, v)`: `[.LocalGet(0), .LocalGet(1), .I32Add, .LocalGet(2), .F64Store(0, 0)]`

The existing `buf_load_u8`/`buf_store_u8` stay. The old `buf_mark`/`buf_reset` become dead once Task 5 severs `rt.arr` — remove them in Task 5, not here.

- [ ] **Step 1: Rewrite `buf.tw`**

In `boot/compiler/codegen/runtime/buf.tw`:
- Add the `buf_free_head` global: `GlobalDef.{ name: "buf_free_head", mutable: true, ty: .I32, init: [.I32Const(0)] }` (keep `buf_heap_ptr` init `[.I32Const(8)]`).
- Replace `alloc_fn` with the free-list `buf_alloc` above; add `free_fn` (`buf_free`); add `load_i64_fn`/`store_i64_fn`/`load_f64_fn`/`store_f64_fn`.
- Add each new func to the `funcs:` list and an `ExportDef` for each new `buf_*` symbol (`buf_free`, `buf_load_i64`, `buf_store_i64`, `buf_load_f64`, `buf_store_f64`).

Use the existing `alloc_fn` (bump + page-grow) and `load_u8_fn` as `.Instr` syntax references for `.If`, `.I32And`, `.I32ShrU`, `.MemoryGrow`, `.MemorySize`, `.Unreachable` (verify the exact `Unreachable`/`I32Eqz`/`I32LtU` variant names exist in `boot/compiler/codegen/wasm_ir.tw` before use; add any missing trivial Instr variant + its `wasm.tw` opcode + `wat.tw` arm if needed).

- [ ] **Step 2: Add ABI arms in `builtins.tw`**

In `builtin_abi`, beside the existing `buf_*` arms (~line 179), add:
```
"buf_free" => abi([.I32], []),
"buf_load_i64" => abi([.I32, .I32], [.I64]),
"buf_store_i64" => abi([.I32, .I32, .I64], []),
"buf_load_f64" => abi([.I32, .I32], [.F64]),
"buf_store_f64" => abi([.I32, .I32, .F64], []),
```

- [ ] **Step 3: Add rt() specs in `builtins.tw`**

In `builtin_specs()`, beside the existing `rt("buf_*", ...)` entries (~line 544), add:
```
rt("buf_free", "rt.buf", "buf_free", .None),
rt("buf_load_i64", "rt.buf", "buf_load_i64", .None),
rt("buf_store_i64", "rt.buf", "buf_store_i64", .None),
rt("buf_load_f64", "rt.buf", "buf_load_f64", .None),
rt("buf_store_f64", "rt.buf", "buf_store_f64", .None),
```

- [ ] **Step 4: Add temporary global signatures in `base_env.tw`**

In the `__buf_*` block (~line 426), chain these `.add_function(...)` calls (these MOVE to internal in Task 4):
```
.add_function(builtin_sig("__buf_free", [], ["ptr"], [.Int], .Some(.Void)))
.add_function(builtin_sig("__buf_load_i64", [], ["base", "i"], [.Int, .Int], .Some(.Int)))
.add_function(builtin_sig("__buf_store_i64", [], ["base", "i", "v"], [.Int, .Int, .Int], .Some(.Void)))
.add_function(builtin_sig("__buf_load_f64", [], ["base", "i"], [.Int, .Int], .Some(.Float)))
.add_function(builtin_sig("__buf_store_f64", [], ["base", "i", "v"], [.Int, .Int, .Float], .Some(.Void)))
```

- [ ] **Step 5: Build the boot compiler**

Run:
```bash
python3 tools/generate_core_lib.py && cargo build --release && \
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm
```
Expected: builds with no type/exhaustiveness errors (stage0 self-validates the emitted wasm).

- [ ] **Step 6: Smoke-test the allocator via a temp program**

Create `/tmp/buf_smoke.tw`:
```tw
a := __buf_alloc(16)
__buf_store_i64(a, 0, 42)
__buf_store_i64(a, 8, 99)
println("a0=${__buf_load_i64(a, 0)} a8=${__buf_load_i64(a, 8)}")   // 42 99
__buf_free(a)
b := __buf_alloc(16)                 // should reuse a's region
println("reused=${b == a}")          // true (free-list reuse)
__buf_store_f64(b, 0, 3.5)
println("f=${__buf_load_f64(b, 0)}") // 3.5
__buf_free(b)
```
Run:
```bash
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs run /tmp/buf_smoke.tw
```
Expected: `a0=42 a8=99`, then `reused=true`, then `f=3.5`. If `reused` is false, the free-list isn't reusing — debug `buf_free`/`buf_alloc` before proceeding.

- [ ] **Step 7: Format, lint, commit**

```bash
target/twk fmt boot/compiler/codegen/runtime/buf.tw boot/compiler/builtins.tw boot/compiler/base_env.tw
git add -A
git commit -m "buffer: rt.buf free-list allocator + i64/f64 accessors

Replace the bump allocator with an address-ordered free-list (first-fit,
split, coalesce) so first-class buffers can be freed in any order; add
buf_free and i64/f64 load/store funcs + __buf_* intrinsics (global for
now, internalized later).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: `@std.buffer` module + three view submodules

Pure-Twinkle surface over the intrinsics, with the comprehensive suite that also pins allocator behavior.

**Files:**
- Create: `boot/stdlib/buffer.tw` (`@std.buffer`)
- Create: `boot/stdlib/buffer/u8view.tw` (`@std.buffer.u8view`)
- Create: `boot/stdlib/buffer/i64view.tw` (`@std.buffer.i64view`)
- Create: `boot/stdlib/buffer/f64view.tw` (`@std.buffer.f64view`)
- Create: `boot/tests/suites/stdlib_buffer_suite.tw`
- Modify: `boot/tests/main.tw` (register the suite)

- [ ] **Step 1: Write the i64 view submodule**

Create `boot/stdlib/buffer/i64view.tw`:
```tw
//! Element-indexed i64 window over a linear-memory buffer. Holds a raw `ptr`
//! (a linear offset), not a `Buffer`, so this submodule does not import
//! `@std.buffer` — the dependency is one-directional. Satisfies `IndexRead<Int>`
//! via `at` + `len`, and `Sliceable` via `slice`.

pub type I64View = .{ ptr: Int, byte_off: Int, count: Int }

type IterState = .{ v: I64View, i: Int }

fn iter_step(s: IterState) UnfoldStep<Int, IterState> {
  if s.i < s.v.count {
    UnfoldStep.Yield(s.v.at(s.i), IterState.{ v: s.v, i: s.i + 1 })
  } else {
    UnfoldStep.Done
  }
}

/// Element count. With `at`, satisfies `IndexRead<Int>`.
pub fn len(v: I64View) Int {
  v.count
}

/// Element `i` (unchecked against `count`; only the whole-memory bound traps).
pub fn at(v: I64View, i: Int) Int {
  __buf_load_i64(v.ptr, v.byte_off + i * 8)
}

/// In-place element write.
pub fn set(v: I64View, i: Int, x: Int) {
  __buf_store_i64(v.ptr, v.byte_off + i * 8, x)
}

/// Sub-window `[lo, hi)` (O(1), shares the backing). Clamped like `@std.view`.
/// Satisfies `Sliceable`, backing `v[lo..hi]`.
pub fn slice(v: I64View, lo: Int, hi: Int) I64View {
  start := lo.clamp(0, v.count)
  end := hi.clamp(start, v.count)
  .{ ptr: v.ptr, byte_off: v.byte_off + start * 8, count: end - start }
}

/// Iterator over the elements, for `for x in v.iter()`.
pub fn iter(v: I64View) Iterator<Int> {
  Iterator.unfold(IterState.{ v, i: 0 }, iter_step)
}
```

- [ ] **Step 2: Write the f64 view submodule**

Create `boot/stdlib/buffer/f64view.tw` — identical structure to `i64view.tw` but element type `Float`, stride `8`, and `__buf_load_f64`/`__buf_store_f64`:
```tw
//! Element-indexed f64 window over a linear-memory buffer. See i64view.tw.

pub type F64View = .{ ptr: Int, byte_off: Int, count: Int }

type IterState = .{ v: F64View, i: Int }

fn iter_step(s: IterState) UnfoldStep<Float, IterState> {
  if s.i < s.v.count {
    UnfoldStep.Yield(s.v.at(s.i), IterState.{ v: s.v, i: s.i + 1 })
  } else {
    UnfoldStep.Done
  }
}

pub fn len(v: F64View) Int {
  v.count
}

pub fn at(v: F64View, i: Int) Float {
  __buf_load_f64(v.ptr, v.byte_off + i * 8)
}

pub fn set(v: F64View, i: Int, x: Float) {
  __buf_store_f64(v.ptr, v.byte_off + i * 8, x)
}

pub fn slice(v: F64View, lo: Int, hi: Int) F64View {
  start := lo.clamp(0, v.count)
  end := hi.clamp(start, v.count)
  .{ ptr: v.ptr, byte_off: v.byte_off + start * 8, count: end - start }
}

pub fn iter(v: F64View) Iterator<Float> {
  Iterator.unfold(IterState.{ v, i: 0 }, iter_step)
}
```

- [ ] **Step 3: Write the u8 view submodule**

Create `boot/stdlib/buffer/u8view.tw` — element type `Int` (raw byte value 0–255 in `Int` domain; avoids the `Byte.from_int` Option dance in the hot path), stride `1`, `__buf_load_u8`/`__buf_store_u8`:
```tw
//! Element-indexed byte window over a linear-memory buffer. Elements are raw
//! byte values in the Int domain (0..255). See i64view.tw.

pub type U8View = .{ ptr: Int, byte_off: Int, count: Int }

type IterState = .{ v: U8View, i: Int }

fn iter_step(s: IterState) UnfoldStep<Int, IterState> {
  if s.i < s.v.count {
    UnfoldStep.Yield(s.v.at(s.i), IterState.{ v: s.v, i: s.i + 1 })
  } else {
    UnfoldStep.Done
  }
}

pub fn len(v: U8View) Int {
  v.count
}

pub fn at(v: U8View, i: Int) Int {
  __buf_load_u8(v.ptr, v.byte_off + i)
}

pub fn set(v: U8View, i: Int, x: Int) {
  __buf_store_u8(v.ptr, v.byte_off + i, x)
}

pub fn slice(v: U8View, lo: Int, hi: Int) U8View {
  start := lo.clamp(0, v.count)
  end := hi.clamp(start, v.count)
  .{ ptr: v.ptr, byte_off: v.byte_off + start, count: end - start }
}

pub fn iter(v: U8View) Iterator<Int> {
  Iterator.unfold(IterState.{ v, i: 0 }, iter_step)
}
```

- [ ] **Step 4: Write the `@std.buffer` module**

Create `boot/stdlib/buffer.tw`:
```tw
//! Sandboxed, low-level, manually-managed linear-memory buffers — Twinkle's
//! second mutate-in-place reference type alongside `Cell`. Opt-in; correctness
//! (calling `free`, no use-after-free) is the programmer's responsibility.
//!
//! Two import lines give the full surface (mirroring `@std.view` / `@std.tuple`):
//!
//!   use @std.buffer                          // `buffer.new(n)`, `buf.view_i64(..)`
//!   use @std.buffer.{Buffer, U8View, I64View, F64View}   // name the types
//!
//! Each view type has its own nominal home (a submodule) and is re-exported here
//! transparently, because a single module cannot hold three `at`/`set`/`len`
//! witnesses.

use .buffer.u8view as u8view_mod
use .buffer.i64view as i64view_mod
use .buffer.f64view as f64view_mod

pub type Buffer = .{ ptr: Int, len: Int }
pub type U8View = u8view_mod.U8View
pub type I64View = i64view_mod.I64View
pub type F64View = f64view_mod.F64View

/// Allocate an uninitialized `nbytes`-byte region. Must be freed with `free`.
pub fn new(nbytes: Int) Buffer {
  .{ ptr: __buf_alloc(nbytes), len: nbytes }
}

/// Allocate and copy a byte vector into linear memory (a copy bridge — see the
/// gather-trap caveat in the design doc).
pub fn from_bytes(bytes: Vector<Byte>) Buffer {
  b := new(bytes.len())
  i := 0

  for i < bytes.len() {
    b.set_u8(i, bytes[i])
    i = i + 1
  }

  b
}

/// Release the region. Double-free is undefined (corrupts allocator bookkeeping).
pub fn free(b: Buffer) {
  __buf_free(b.ptr)
}

/// Byte length.
pub fn len(b: Buffer) Int {
  b.len
}

/// Copy the bytes out into an owned `Vector<Byte>`.
pub fn to_bytes(b: Buffer) Vector<Byte> {
  out: Vector<Byte> = []
  i := 0

  for i < b.len {
    out = .append(b.get_u8(i))
    i = i + 1
  }

  out
}

/// Byte read at `off` (byte offset). Unchecked against `len`.
pub fn get_u8(b: Buffer, off: Int) Byte {
  case Byte.from_int(__buf_load_u8(b.ptr, off)) {
    .Some(byte) => byte,
    .None => error("buffer get_u8: byte out of range"),
  }
}

/// Byte write at `off` (byte offset).
pub fn set_u8(b: Buffer, off: Int, v: Byte) {
  __buf_store_u8(b.ptr, off, v.to_int())
}

/// i64 read at byte offset `off` (little-endian, unaligned ok).
pub fn get_i64(b: Buffer, off: Int) Int {
  __buf_load_i64(b.ptr, off)
}

pub fn set_i64(b: Buffer, off: Int, v: Int) {
  __buf_store_i64(b.ptr, off, v)
}

/// f64 read at byte offset `off`.
pub fn get_f64(b: Buffer, off: Int) Float {
  __buf_load_f64(b.ptr, off)
}

pub fn set_f64(b: Buffer, off: Int, v: Float) {
  __buf_store_f64(b.ptr, off, v)
}

/// Element-indexed byte view over `[byte_off, byte_off + count)`.
pub fn view_u8(b: Buffer, byte_off: Int, count: Int) U8View {
  .{ ptr: b.ptr, byte_off, count }
}

/// Element-indexed i64 view; `count` is in elements, `byte_off` in bytes.
pub fn view_i64(b: Buffer, byte_off: Int, count: Int) I64View {
  .{ ptr: b.ptr, byte_off, count }
}

/// Element-indexed f64 view; `count` is in elements, `byte_off` in bytes.
pub fn view_f64(b: Buffer, byte_off: Int, count: Int) F64View {
  .{ ptr: b.ptr, byte_off, count }
}
```

- [ ] **Step 5: Regenerate core_lib and build**

Run:
```bash
python3 tools/generate_core_lib.py && cargo build --release && \
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm
```
Expected: builds clean. (If the resolver rejects `pub type I64View = i64view_mod.I64View`, re-check the `use .buffer.i64view as i64view_mod` form against `boot/stdlib/tuple.tw:19`.)

- [ ] **Step 6: Write the failing suite**

Create `boot/tests/suites/stdlib_buffer_suite.tw`:
```tw
use @std.buffer
use @std.buffer.{Buffer, I64View, F64View, U8View}

use tests.assert
use tests.runner

pub fn suite() runner.Suite {
  runner
    .suite("stdlib buffer")
    .test(
      "raw i64 round-trip and free reuse",
      fn() {
        b := buffer.new(16)
        b.set_i64(0, 42)
        b.set_i64(8, -99)
        try assert.equal(b.get_i64(0), 42)
        try assert.equal(b.get_i64(8), -99)
        ptr := b.ptr
        b.free()
        b2 := buffer.new(16)
        try assert.equal(b2.ptr, ptr)        // free-list reuse
        b2.free()
        .Ok({})
      },
    )
    .test(
      "f64 and u8 raw access",
      fn() {
        b := buffer.new(16)
        b.set_f64(0, 2.5)
        try assert.equal(b.get_f64(0), 2.5)
        b.set_u8(8, Byte.from_int(200).unwrap_or(b.get_u8(8)))
        try assert.equal(b.get_u8(8).to_int(), 200)
        b.free()
        .Ok({})
      },
    )
    .test(
      "from_bytes / to_bytes round-trip",
      fn() {
        src: Vector<Byte> = "hi".utf8_bytes()
        b := buffer.from_bytes(src)
        try assert.equal(b.len(), 2)
        try assert.equal(b.to_bytes(), src)
        b.free()
        .Ok({})
      },
    )
    .test(
      "i64 view get/set/len and slice",
      fn() {
        b := buffer.new(64)
        v := b.view_i64(0, 8)
        i := 0

        for i < 8 {
          v.set(i, i * 10)
          i = i + 1
        }

        try assert.equal(v.len(), 8)
        try assert.equal(v.at(3), 30)
        sub := v.slice(2, 5)
        try assert.equal(sub.len(), 3)
        try assert.equal(sub.at(0), 20)
        b.free()
        .Ok({})
      },
    )
    .test(
      "view iteration sums elements",
      fn() {
        b := buffer.new(32)
        v := b.view_i64(0, 4)
        v.set(0, 1)
        v.set(1, 2)
        v.set(2, 3)
        v.set(3, 4)
        total := 0

        for x in v.iter() {
          total = total + x
        }

        try assert.equal(total, 10)
        b.free()
        .Ok({})
      },
    )
    .test(
      "coalescing: two frees reused by one larger alloc",
      fn() {
        a := buffer.new(16)
        base := a.ptr
        c := buffer.new(16)
        a.free()
        c.free()                              // adjacent frees coalesce
        big := buffer.new(40)                 // <= 16+8+16 payload available
        try assert.equal(big.ptr, base)       // reuses the coalesced region
        big.free()
        .Ok({})
      },
    )
}
```

Notes for the implementer:
- Confirm `String.utf8_bytes()` is the real method name (memory lists `STRING_UTF8_BYTES`); if it differs, build the `src` vector with literal `Byte` values via `Byte.from_int(...).unwrap_or(...)` instead.
- The coalescing test's exact `ptr` reuse depends on allocation order being LIFO-adjacent; if the assertion is brittle, weaken it to `big.ptr <= base + 8` (still proves no fresh bump past the freed region) but keep a coalescing assertion.

- [ ] **Step 7: Register the suite in main.tw**

Add `use .suites.stdlib_buffer_suite` with the other `use .suites.*` lines, and add `stdlib_buffer_suite.suite()` to the suite list.

- [ ] **Step 8: Run the suite**

Run:
```bash
python3 tools/generate_core_lib.py && cargo build --release && \
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm && \
  BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
    tools/js_runtime/deno_main.mjs run boot/tests/main.tw 2>&1 | grep -i 'buffer\|fail'
```
Expected: the "stdlib buffer" suite passes. Debug allocator/view issues here.

- [ ] **Step 9: Format, lint, commit**

```bash
target/twk fmt boot/stdlib/buffer.tw boot/stdlib/buffer/u8view.tw \
  boot/stdlib/buffer/i64view.tw boot/stdlib/buffer/f64view.tw \
  boot/tests/suites/stdlib_buffer_suite.tw
target/twk lint boot/stdlib/buffer.tw
git add -A
git commit -m "buffer: @std.buffer module + u8/i64/f64 view submodules

First-class Buffer record + raw byte/i64/f64 accessors, Vector<Byte>
bridges, and three element-indexed view submodules re-exported
transparently. Suite covers round-trip, free-list reuse, coalescing,
slicing, and iteration.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Internalize the `__buf_*` surface

Move the `__buf_*` signatures out of the global `builtin_env()` into `add_internal_host_builtins()` so only stdlib/prelude (including `@std.buffer`) can call them — closing the M3 global leak.

**Files:**
- Modify: `boot/compiler/base_env.tw` (move the `__buf_*` block)

- [ ] **Step 1: Cut the `__buf_*` block from `builtin_env()`**

Delete the entire `__buf_*` chain added in Task 2 Step 4 + the pre-existing ones (`__buf_alloc`/`__buf_mark`/`__buf_reset`/`__buf_load_u8`/`__buf_store_u8`) from `builtin_env()` (~line 423–432), including the now-stale comment about being "available in all modules".

- [ ] **Step 2: Add them to `add_internal_host_builtins()`**

In `add_internal_host_builtins()` (after the `__host_*` chain), add the full set:
```
.add_function(builtin_sig("__buf_alloc", [], ["nbytes"], [.Int], .Some(.Int)))
.add_function(builtin_sig("__buf_free", [], ["ptr"], [.Int], .Some(.Void)))
.add_function(builtin_sig("__buf_load_u8", [], ["base", "i"], [.Int, .Int], .Some(.Int)))
.add_function(builtin_sig("__buf_store_u8", [], ["base", "i", "v"], [.Int, .Int, .Int], .Some(.Void)))
.add_function(builtin_sig("__buf_load_i64", [], ["base", "i"], [.Int, .Int], .Some(.Int)))
.add_function(builtin_sig("__buf_store_i64", [], ["base", "i", "v"], [.Int, .Int, .Int], .Some(.Void)))
.add_function(builtin_sig("__buf_load_f64", [], ["base", "i"], [.Int, .Int], .Some(.Float)))
.add_function(builtin_sig("__buf_store_f64", [], ["base", "i", "v"], [.Int, .Int, .Float], .Some(.Void)))
```
(Drop the `__buf_mark`/`__buf_reset` typed signatures entirely — nothing calls them: `@std.buffer` never used them, and `rt.arr` reaches `buf_mark`/`buf_reset` as direct wasm-level imports resolved by name, not via the typed `__buf_*` builtins. So removing the typed sigs here is safe regardless of Task 5 ordering; the `rt.buf` `mark`/`reset` funcs themselves are removed in Task 5.)

- [ ] **Step 3: Rebuild and confirm `@std.buffer` still works**

Run:
```bash
python3 tools/generate_core_lib.py && cargo build --release && \
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm && \
  BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
    tools/js_runtime/deno_main.mjs run boot/tests/main.tw 2>&1 | grep -i 'buffer\|fail'
```
Expected: "stdlib buffer" suite still passes (stdlib modules see the internal builtins).

- [ ] **Step 4: Confirm user code can no longer call `__buf_*` directly**

Run:
```bash
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs run /tmp/buf_smoke.tw 2>&1 | head
```
Expected: a typecheck error ("Undefined variable: __buf_alloc" or similar) — the raw surface is now hidden. (Delete `/tmp/buf_smoke.tw` afterward.)

- [ ] **Step 5: Format, commit**

```bash
target/twk fmt boot/compiler/base_env.tw
git add -A
git commit -m "buffer: hide __buf_* behind @std.buffer (internal-host only)

Move the raw linear-memory intrinsics from the global builtin env into
add_internal_host_builtins, so user programs reach buffers only through
@std.buffer; closes the M3 probe-era global leak.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Sever the `rt.arr → rt.buf` dependency

Remove the parity-only dense linear-scratch sort so `rt.buf` is reachable only through `@std.buffer`, and drop the now-unused `buf_mark`/`buf_reset`.

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw` (remove the dense sort + imports + reroute dispatch)
- Modify: `boot/compiler/codegen/runtime/buf.tw` (remove `buf_mark`/`buf_reset` + their exports)
- Modify: `boot/compiler/builtins.tw` (remove `buf_mark`/`buf_reset` abi + rt entries)

- [ ] **Step 1: Find where `sort_i64` (the dense fn) is dispatched**

Run: `grep -n 'sort_i64\b\|"sort_i64"\|sort_i64_dense' boot/compiler/codegen/runtime/arr.tw boot/compiler/codegen/*.tw`
Expected: the `sort_i64_dense_fn()` entry in the `funcs:` list (~151), the function body (~3740), and a dispatch/emit site that routes i64-element vector sorts to the `sort_i64` symbol. Note the existing `sort_typed_fn` (~3437) already handles the i64 case over a GC typed array — that is the path to fall back to.

- [ ] **Step 2: Remove the dense sort and reroute**

In `arr.tw`:
- Remove `sort_i64_dense_fn()` from the `funcs:` list.
- Remove the entire `sort_i64_dense_fn` definition (~3740–4066).
- Remove the three `rt.buf` import entries (~179–182: `buf_alloc`/`buf_mark`/`buf_reset`).
- At the dispatch site, route i64-element sorts to `sort_typed_fn`'s i64 case (the same canonical `Vector.sort` path the other element types use) instead of the removed `sort_i64` symbol.

- [ ] **Step 3: Remove `buf_mark`/`buf_reset` from `rt.buf`**

In `buf.tw`: delete `mark_fn`/`reset_fn`, their `funcs:` entries, and their `ExportDef`s. In `builtins.tw`: delete the `"buf_mark"`/`"buf_reset"` arms in `builtin_abi` and the `rt("buf_mark"...)`/`rt("buf_reset"...)` specs.

- [ ] **Step 4: Build and verify sort still works**

Create `/tmp/sort_check.tw`:
```tw
xs: Vector<Int> = [5, 3, 9, 1, 7, 2, 8, 4, 6, 0]
println("${xs.sort()}")   // [0,1,2,3,4,5,6,7,8,9]
```
Run:
```bash
python3 tools/generate_core_lib.py && cargo build --release && \
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm && \
  BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
    tools/js_runtime/deno_main.mjs run /tmp/sort_check.tw
```
Expected: `[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]`.

- [ ] **Step 5: Confirm `rt.arr` no longer imports `rt.buf`**

Build a sort-only program to WAT and check imports:
```bash
./target/release/twk build /tmp/sort_check.tw -o /tmp/sort.wat 2>/dev/null || \
  BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
    tools/js_runtime/deno_main.mjs build /tmp/sort_check.tw -o /tmp/sort.wat
grep -c 'buf_mark\|buf_reset\|"rt.buf"' /tmp/sort.wat
```
Expected: `0` (no rt.buf references from the sort path).

- [ ] **Step 6: Run the full boot suite (sort regression guard)**

Run: `BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs run boot/tests/main.tw 2>&1 | tail -5`
Expected: all suites pass (existing sort suites confirm no regression).

- [ ] **Step 7: Format, commit**

```bash
target/twk fmt boot/compiler/codegen/runtime/arr.tw boot/compiler/codegen/runtime/buf.tw boot/compiler/builtins.tw
git add -A
git commit -m "buffer: sever rt.arr->rt.buf (drop parity-only dense sort)

Remove the M1 dense linear-scratch i64 sort (benched at parity; the read
wall's lever is typed PVecI64, not this scratch) and route i64 sorts back
to the GC-typed-array path. rt.buf is now reachable only via @std.buffer,
unblocking conditional memory emission. Also drops the unused
buf_mark/buf_reset.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Conditional linear-memory emission

Emit the `rt.buf` memory + globals only when a buffer op survives DCE, so non-buffer programs carry no linear memory.

**Files:**
- Modify: `boot/compiler/codegen/codegen.tw` (post-DCE prune in `link_program`)

**Approach (post-DCE prune — leverages existing reachability).** After `eliminate_dead_wasm`, if no `buf_*` function survived (i.e. `@std.buffer` was unused, so DCE dropped `buf_alloc` et al.), drop the `rt.buf` memory (`MemoryDef` named `"heap"`) and the `buf_*` globals from the linked module. When `@std.buffer` is used, `buf_alloc` survives and the memory is kept. A module with no memory and no memory ops is valid Wasm.

- [ ] **Step 1: Inspect the linked-module shape**

Run: `grep -n 'fn eliminate_dead_wasm\|memories\|globals\|funcs' boot/compiler/codegen/linker.tw | head`
Confirm `LinkedModule` exposes `funcs`, `memories`, and `globals` as filterable vectors, and note their field names (used below).

- [ ] **Step 2: Add the prune after DCE in `link_program`**

In `codegen.tw`, immediately after `linked = eliminate_dead_wasm(linked)`:
```tw
// Conditional linear-memory emission: rt.buf's memory + globals survive DCE
// (which only prunes funcs), so drop them when no buffer op is live.
buf_live := linked.funcs.any(fn(f) { f.name.starts_with("buf_") })

if !buf_live {
  linked.memories = linked.memories.filter(fn(m) { m.name != "heap" })
  linked.globals = linked.globals.filter(fn(g) { !g.name.starts_with("buf_") })
}
```
Adjust field/method names to the actuals from Step 1 (e.g. `linked.funcs` vs `linked.functions`; `m.name` for `MemoryDef`). If `LinkedModule` is immutable, rebuild it via a record update with the filtered vectors.

- [ ] **Step 3: Build a non-buffer program and assert no memory**

Create `/tmp/no_buf.tw`:
```tw
println("hello ${1 + 2}")
```
Run:
```bash
python3 tools/generate_core_lib.py && cargo build --release && \
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm && \
  BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
    tools/js_runtime/deno_main.mjs build /tmp/no_buf.tw -o /tmp/no_buf.wat
grep -c '(memory' /tmp/no_buf.wat
```
Expected: `0` (no memory emitted). Also run it to confirm it still executes:
```bash
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs run /tmp/no_buf.tw
```
Expected: `hello 3`.

- [ ] **Step 4: Build a buffer program and assert memory IS emitted**

Create `/tmp/with_buf.tw`:
```tw
use @std.buffer
b := buffer.new(16)
b.set_i64(0, 7)
println("${b.get_i64(0)}")
b.free()
```
Run:
```bash
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/with_buf.tw -o /tmp/with_buf.wat
grep -c '(memory' /tmp/with_buf.wat
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs run /tmp/with_buf.tw
```
Expected: `grep` ≥ `1`, and the run prints `7`.

- [ ] **Step 5: Guard against a baseline regression**

Confirm a non-buffer program that previously pulled the memory only via the dense sort now also has none:
```bash
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/sort_check.tw -o /tmp/sort2.wat
grep -c '(memory' /tmp/sort2.wat
```
Expected: `0`.

- [ ] **Step 6: Format, commit**

```bash
target/twk fmt boot/compiler/codegen/codegen.tw
git add -A
git commit -m "buffer: emit linear memory only when a buffer op is live

DCE prunes funcs but not memories/globals, so after DCE drop rt.buf's
memory + globals when no buf_* function survives. Non-buffer programs
(now including all sorts, post-severance) emit no linear memory.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: Documentation

**Files:**
- Modify: `docs/spec.md` (record `Buffer` as the second mutate-in-place type)
- Modify: `docs/API.md` (Standard Library: `@std.buffer`)

- [ ] **Step 1: Spec — note the mutate-in-place pair**

In `docs/spec.md`, near the immutability/`Cell` discussion, add a short paragraph: all values are immutable except the two explicit, opt-in mutate-in-place reference types — `Cell` (single boxed value) and `@std.buffer`'s `Buffer` (sandboxed linear-memory region, manually freed). Link to `docs/plans/buffer-linear-memory.md` for the full model.

- [ ] **Step 2: API — document `@std.buffer`**

In `docs/API.md`'s Standard Library section, add a `@std.buffer` entry covering: the two import lines; `buffer.new`/`from_bytes`/`free`/`len`/`to_bytes`; raw `get/set_u8|i64|f64` (byte-addressed, little-endian, unaligned, **unchecked**); `view_u8`/`view_i64`/`view_f64` → `at`/`set`/`len`/`slice`/`iter` (element-indexed); and the manual-lifetime / use-after-free caveat. Add `@std.buffer` to the `use @std.X` list line if one exists.

- [ ] **Step 3: Commit**

```bash
git add docs/spec.md docs/API.md
git commit -m "docs: document @std.buffer (linear-memory Buffer + views)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 8: Full-gate verification + plan/README closeout

- [ ] **Step 1: Self-host fixed point + full suite**

Run: `make bundle-cli`
Expected: prints "Fixed point reached" (stage3 == stage4). This proves the boot compiler reproduces itself with all M2 changes.

Run: `make boot-test`
Expected: all suites pass, including "stdlib buffer".

- [ ] **Step 2: Re-confirm the two memory-emission cases with the bundled CLI**

```bash
target/twk build /tmp/no_buf.tw -o /tmp/no_buf2.wat && grep -c '(memory' /tmp/no_buf2.wat   # 0
target/twk build /tmp/with_buf.tw -o /tmp/with_buf2.wat && grep -c '(memory' /tmp/with_buf2.wat  # >=1
target/twk run /tmp/with_buf.tw    # prints 7
```

- [ ] **Step 3: Update the plans README and archive this plan**

Per the repo convention (completed plans move to `docs/plans/archive/` and the README row is updated):
- In `docs/plans/README.md`, bump the Linear-memory Buffer row status to reflect M2 shipped (M3b/M4 remain future), keeping the link to `buffer-linear-memory.md`.
- `git mv docs/plans/buffer-linear-memory-m2-plan.md docs/plans/archive/buffer-linear-memory-m2-plan.md`.

- [ ] **Step 4: Commit the closeout**

```bash
git add -A
git commit -m "buffer: M2 complete — archive plan, update README

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

- [ ] **Step 5: Update the design doc status**

In `docs/plans/buffer-linear-memory.md`, change the top status line and the M2 roadmap bullet from "active" to "DONE", and note that M3b (fast IR codec) is the next consumer. Commit:
```bash
git add docs/plans/buffer-linear-memory.md
git commit -m "buffer: mark M2 done in the living spec

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Notes / risks for the implementer

- **The allocator is the highest-risk task.** Its correctness is pinned by Task 3's suite (round-trip, free-list reuse, coalescing), not by transcription from this plan — write the `.Instr` against those tests, using `alloc_fn`/`load_u8_fn` as syntax references. If a needed `Instr` variant (`Unreachable`, `I32LtU`, `I32Eqz`, etc.) is missing from `wasm_ir.tw`, add it with its `wasm.tw` opcode + `wat.tw` arm (a one-line-each addition) — this is the same pattern M1 used for load/store.
- **No stage0/Rust changes.** `boot/main.tw` never imports `@std.buffer`, so stage0 needs none of the new surface; the self-host fixed point is the proof. Do **not** add `__buf_*` to `src/`.
- **`make bundle-cli` is required before `target/twk` reflects any codegen/builtin change.** During development use the stage1 path (`./target/release/twk build boot/main.tw -o /tmp/stage1.wasm` then `BOOT_WASM=/tmp/stage1.wasm ... run`).
- **Field/method name drift:** verify `LinkedModule` field names (Task 6), `String.utf8_bytes` (Task 3), and `Option.unwrap_or` against the actual prelude before relying on them; adjust to the real names.
- **`v[i]` element sugar and bare `for x in v` are intentionally out of scope** (they need concrete-receiver contract lowering in the checker). Ship `v.at(i)` + `v.iter()`. `v[lo..hi]` slice sugar works and may be spot-checked but needs no new wiring.
```
