# Buffer Linear-Memory M2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a user-facing, opt-in, manually-managed, first-class `Buffer` linear-memory type (plus typed views) in `@std.buffer`, as the sandboxed-low-level abstraction over linear memory.

**Architecture:** A free-list `rt.buf` allocator + word/byte load-store runtime funcs are exposed as internal-only `__buf_*` intrinsics. `@std.buffer` (pure Twinkle) wraps them in a `Buffer` record + three view submodules (`u8view`/`i64view`/`f64view`, transparently re-exported). The `rt.arr→rt.buf` dependency is severed (drop the parity-only dense sort) **before** the allocator becomes a free-list, and the linear memory is emitted only when a buffer op survives DCE, so non-buffer programs pay nothing.

**Tech Stack:** Boot compiler only (`boot/`), Twinkle (`.tw`), hand-written Wasm IR (the `.Instr` DSL). No Rust/stage0 changes — `boot/main.tw` never imports `@std.buffer`, so the self-host fixed point + boot test suite are the gates.

**Design doc:** `docs/plans/buffer-linear-memory.md` (M2 section). Read it before starting.

---

## Orientation (read once before Task 1)

**Key files:**
- `boot/compiler/codegen/runtime/buf.tw` — the `rt.buf` runtime module (memory + allocator + load/store funcs). M1/M3 built a bump allocator (`buf_alloc`/`buf_mark`/`buf_reset`) + `buf_load_u8`/`buf_store_u8`.
- `boot/compiler/codegen/runtime/arr.tw` — imports `rt.buf` (line ~179) and contains `sort_i64_dense_fn` (the parity-only linear sort: func list ~line 151, body ~3740–4066). The existing `sort_typed_fn` (~3437) already handles the i64 case over a GC typed array — the fallback path.
- `boot/compiler/builtins.tw` — `builtin_abi(name)` (the i64↔i32 wasm-type bridge, ~line 117) and `builtin_specs()` (`rt(...)` entries, ~line 445).
- `boot/compiler/base_env.tw` — `builtin_env()` currently registers `__buf_*` globally (~line 423–432, the M3 leak); `add_internal_host_builtins()` (~line 436) is the internal-only path used by stdlib modules (applied in `query/analyze.tw`).
- `boot/compiler/lower_core/context.tw:44` — already aliases any `buf_*` runtime func to `__buf_*` (no change needed for new `buf_*` ops).
- `boot/compiler/codegen/codegen.tw` — `runtime_modules()` (~line 55) lists runtime modules; `link_program()` (~line 118) links them then runs `eliminate_dead_wasm()`.
- `boot/compiler/codegen/linker.tw` — `qualify(ns, sym)` = `${ns_prefix(ns)}__${sym}` (`.`→`_`), so `rt.buf`'s `buf_alloc` becomes **`rt_buf__buf_alloc`** in the linked module; globals likewise (`rt_buf__buf_heap_ptr`). Memories are concatenated **without** renaming (linker:373), so the `MemoryDef` name stays **`"heap"`**.
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
- WAT inspection: build with a `.wat` output and `grep` the text. Pre-bundle, use the stage1 path: `BOOT_WASM=/tmp/stage1.wasm deno run ... build <prog.tw> -o /tmp/out.wat`.

**Commit conventions:** one commit per task; short imperative subject + a what/why/how body for non-trivial changes; no line/count metrics. Follow the repo's trailer guidance in `CLAUDE.md`/`AGENTS.md` — add a `Co-Authored-By` trailer only when it is actually correct for the session/tooling; do not add it reflexively. Run `target/twk fmt <file>` + `target/twk lint <entry>` on every edited `.tw` before committing.

**No stage0/Rust changes.** `boot/main.tw` never imports `@std.buffer`; the self-host fixed point is the proof. Do **not** add `__buf_*` to `src/`.

---

## Task 1: Remove the M3 probe artifacts

Clears throwaway scaffolding so later internalization of `__buf_*` doesn't break probe code that calls them globally.

**Files:**
- Delete: `boot/lib/buf_codec.tw`, `boot/bench/buf_codec_bench.tw`, `boot/bench/md5_linear_bench.tw`, `boot/tests/suites/buf_codec_suite.tw`
- Modify: `boot/tests/main.tw` (remove the `buf_codec_suite` registration)

- [ ] **Step 1: Find the probe suite's registration lines**

Run: `grep -n 'buf_codec' boot/tests/main.tw`
Expected: a `use .suites.buf_codec_suite` line and a `buf_codec_suite.suite()` entry.

- [ ] **Step 2: Delete the four probe files**

```bash
git rm boot/lib/buf_codec.tw boot/bench/buf_codec_bench.tw \
       boot/bench/md5_linear_bench.tw boot/tests/suites/buf_codec_suite.tw
```

- [ ] **Step 3: Remove the suite registration from main.tw**

Delete the `use .suites.buf_codec_suite` line and remove `buf_codec_suite.suite()` from the suite list (mind the surrounding commas/brackets).

- [ ] **Step 4: Verify nothing else references the probes**

Run: `grep -rn 'buf_codec\|md5_linear' boot/`
Expected: no matches.

- [ ] **Step 5: Verify the suite still builds/runs**

Run: `make boot-test`
Expected: all suites pass (the probe suite is gone).

- [ ] **Step 6: Commit** — subject e.g. `buffer: remove throwaway M3 probe artifacts`, body noting the findings live in the design doc.

---

## Task 2: Sever `rt.arr → rt.buf`, then install the free-list allocator

Severance comes **first**: the old `buf_mark`/`buf_reset` only save/restore `buf_heap_ptr` and would corrupt a free-list (they don't track `buf_free_head`), so the dense sort must stop using `rt.buf` before the allocator changes. Then rewrite the allocator and add the i64/f64 accessors + `__buf_*` intrinsics (kept **global** for now; Task 4 internalizes them).

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw` (remove dense sort + imports + reroute dispatch)
- Modify: `boot/compiler/codegen/runtime/buf.tw` (drop mark/reset; free-list rewrite; new funcs; second global)
- Modify: `boot/compiler/builtins.tw` (`builtin_abi` + `builtin_specs`)
- Modify: `boot/compiler/base_env.tw` (temporary global `__buf_*` signatures)

### Part A — Severance

- [ ] **Step 1: Locate the dense sort and its dispatch**

Run: `grep -n 'sort_i64\b\|"sort_i64"\|sort_i64_dense' boot/compiler/codegen/runtime/arr.tw boot/compiler/codegen/*.tw`
Expected: the `sort_i64_dense_fn()` entry in the `funcs:` list (~151), the body (~3740–4066), and the dispatch/emit site routing i64-element sorts to the `sort_i64` symbol.

- [ ] **Step 2: Remove the dense sort, its imports, and reroute**

In `arr.tw`: remove `sort_i64_dense_fn()` from the `funcs:` list; remove its definition (~3740–4066); remove the three `rt.buf` import entries (~179–182, `buf_alloc`/`buf_mark`/`buf_reset`); at the dispatch site route i64-element sorts to `sort_typed_fn`'s i64 case (the canonical `Vector.sort` path the other element types use).

- [ ] **Step 3: Drop `buf_mark`/`buf_reset` from `rt.buf` and its registrations**

In `buf.tw`: delete `mark_fn`/`reset_fn`, their `funcs:` entries, and their `ExportDef`s. In `builtins.tw`: delete the `"buf_mark"`/`"buf_reset"` arms in `builtin_abi` and the `rt("buf_mark"...)`/`rt("buf_reset"...)` specs. (Nothing else references them: `@std.buffer` doesn't exist yet, and `rt.arr` no longer imports them.)

- [ ] **Step 4: Build + verify sort still works**

Create `/tmp/sort_check.tw`:
```tw
xs: Vector<Int> = [5, 3, 9, 1, 7, 2, 8, 4, 6, 0]
println("${xs.sort()}")
```
Run:
```bash
python3 tools/generate_core_lib.py && cargo build --release && \
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm && \
  BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
    tools/js_runtime/deno_main.mjs run /tmp/sort_check.tw
```
Expected: `[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]`.

- [ ] **Step 5: Confirm `rt.arr` no longer references `rt.buf`**

```bash
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/sort_check.tw -o /tmp/sort.wat
grep -c 'buf_mark\|buf_reset\|"rt.buf"' /tmp/sort.wat
```
Expected: `0`.

### Part B — Free-list allocator + accessors

**Memory layout.** Bytes 0–7 reserved (so payload pointer 0 = null). Two mutable i32 globals: `buf_heap_ptr` (bump frontier, init 8) and `buf_free_head` (address-ordered free-list head, init 0 = empty). Each block = 8-byte header + 8-aligned payload:
- header `+0`: `size` = payload bytes (multiple of 8).
- header `+4`: free block → `next` free-block address (0 = end); allocated block → `0`.
- `buf_alloc` returns `block + 8`; `buf_free(p)` uses `block = p - 8`.

**`buf_alloc(n) -> ptr` (`[.I32]→[.I32]`):**
```
need = (n + 7) & ~7
prev = 0 ; cur = buf_free_head
while cur != 0:
  size = i32.load[cur+0]
  if size >= need:
    if size >= need + 16:                 # split (header 8 + payload >= 8)
      newblk = cur + 8 + need
      i32.store[newblk+0] = size - need - 8
      i32.store[newblk+4] = i32.load[cur+4]
      i32.store[cur+0] = need
      if prev == 0: buf_free_head = newblk else i32.store[prev+4] = newblk
    else:
      if prev == 0: buf_free_head = i32.load[cur+4] else i32.store[prev+4] = i32.load[cur+4]
    i32.store[cur+4] = 0
    return cur + 8
  prev = cur ; cur = i32.load[cur+4]
block = buf_heap_ptr ; end = block + 8 + need
grow_to(end)
i32.store[block+0] = need ; i32.store[block+4] = 0
buf_heap_ptr = end
return block + 8
```
`grow_to(end)` mirrors the existing `alloc_fn` page-growth: `need_pages = (end + 65535) >> 16`; if `need_pages > memory.size`, `if memory.grow(need_pages - memory.size) == -1: unreachable` (trap on grow failure).

**`buf_free(ptr)` (`[.I32]→[]`):**
```
block = ptr - 8 ; size = i32.load[block+0]
prev = 0 ; cur = buf_free_head
while cur != 0 and cur < block: prev = cur ; cur = i32.load[cur+4]
if cur != 0 and block + 8 + size == cur:           # coalesce forward
  size = size + 8 + i32.load[cur+0]
  i32.store[block+0] = size ; i32.store[block+4] = i32.load[cur+4]
else:
  i32.store[block+4] = cur
if prev == 0:
  buf_free_head = block
else if prev + 8 + i32.load[prev+0] == block:      # coalesce backward
  i32.store[prev+0] = i32.load[prev+0] + 8 + size
  i32.store[prev+4] = i32.load[block+4]
else:
  i32.store[prev+4] = block
```

**Accessor funcs (verbatim `.Instr`, modeled on the existing `load_u8_fn`):** address = `base + off`, memarg `(0, 0)`.
- `buf_load_i64`: `[.LocalGet(0), .LocalGet(1), .I32Add, .I64Load(0, 0)]`
- `buf_store_i64`: `[.LocalGet(0), .LocalGet(1), .I32Add, .LocalGet(2), .I64Store(0, 0)]`
- `buf_load_f64`: `[.LocalGet(0), .LocalGet(1), .I32Add, .F64Load(0, 0)]`
- `buf_store_f64`: `[.LocalGet(0), .LocalGet(1), .I32Add, .LocalGet(2), .F64Store(0, 0)]`

- [ ] **Step 6: Rewrite `buf.tw`**

Add the `buf_free_head` global (`GlobalDef.{ name: "buf_free_head", mutable: true, ty: .I32, init: [.I32Const(0)] }`). Replace `alloc_fn` with the free-list `buf_alloc`; add `free_fn`/`load_i64_fn`/`store_i64_fn`/`load_f64_fn`/`store_f64_fn`; add each to `funcs:` and an `ExportDef` per new `buf_*` symbol. Use the existing `alloc_fn`/`load_u8_fn` as `.Instr` syntax references. If a needed `Instr` variant (`Unreachable`, `I32LtU`, `I32Eqz`, etc.) is missing from `boot/compiler/codegen/wasm_ir.tw`, add it + its `wasm.tw` opcode arm + `wat.tw` text arm (same pattern M1 used for load/store).

- [ ] **Step 7: ABI arms in `builtins.tw`** (beside the existing `buf_*` arms)
```
"buf_free" => abi([.I32], []),
"buf_load_i64" => abi([.I32, .I32], [.I64]),
"buf_store_i64" => abi([.I32, .I32, .I64], []),
"buf_load_f64" => abi([.I32, .I32], [.F64]),
"buf_store_f64" => abi([.I32, .I32, .F64], []),
```

- [ ] **Step 8: rt() specs in `builtins.tw`** (beside the existing `rt("buf_*", ...)`)
```
rt("buf_free", "rt.buf", "buf_free", .None),
rt("buf_load_i64", "rt.buf", "buf_load_i64", .None),
rt("buf_store_i64", "rt.buf", "buf_store_i64", .None),
rt("buf_load_f64", "rt.buf", "buf_load_f64", .None),
rt("buf_store_f64", "rt.buf", "buf_store_f64", .None),
```

- [ ] **Step 9: Temporary global signatures in `base_env.tw`** (chain into the existing `__buf_*` block; these MOVE to internal in Task 4)
```
.add_function(builtin_sig("__buf_free", [], ["ptr"], [.Int], .Some(.Void)))
.add_function(builtin_sig("__buf_load_i64", [], ["base", "i"], [.Int, .Int], .Some(.Int)))
.add_function(builtin_sig("__buf_store_i64", [], ["base", "i", "v"], [.Int, .Int, .Int], .Some(.Void)))
.add_function(builtin_sig("__buf_load_f64", [], ["base", "i"], [.Int, .Int], .Some(.Float)))
.add_function(builtin_sig("__buf_store_f64", [], ["base", "i", "v"], [.Int, .Int, .Float], .Some(.Void)))
```
(The pre-existing `__buf_alloc`/`__buf_load_u8`/`__buf_store_u8` stay; the now-removed `__buf_mark`/`__buf_reset` typed sigs, if present, should be deleted here.)

- [ ] **Step 10: Build + smoke-test the allocator**

```bash
python3 tools/generate_core_lib.py && cargo build --release && \
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm
```
Create `/tmp/buf_smoke.tw`:
```tw
a := __buf_alloc(16)
__buf_store_i64(a, 0, 42)
__buf_store_i64(a, 8, 99)
println("a0=${__buf_load_i64(a, 0)} a8=${__buf_load_i64(a, 8)}")
__buf_free(a)
b := __buf_alloc(16)
println("reused=${b == a}")
__buf_store_f64(b, 0, 3.5)
println("f=${__buf_load_f64(b, 0)}")
__buf_free(b)
```
Run via the stage1 path. Expected: `a0=42 a8=99`, `reused=true`, `f=3.5`. If `reused` is false, debug the free-list before proceeding. (Delete `/tmp/buf_smoke.tw` after Task 4.)

- [ ] **Step 11: Format, lint, commit** — subject e.g. `buffer: sever rt.arr->rt.buf and install free-list allocator`. Run `make boot-test` first to confirm no sort regression.

---

## Task 3: `@std.buffer` module + three view submodules

Pure-Twinkle surface over the intrinsics, with size-validation on `new` and the suite that also pins allocator behavior.

**Files:**
- Create: `boot/stdlib/buffer.tw`, `boot/stdlib/buffer/u8view.tw`, `boot/stdlib/buffer/i64view.tw`, `boot/stdlib/buffer/f64view.tw`
- Create: `boot/tests/suites/stdlib_buffer_suite.tw`
- Modify: `boot/tests/main.tw`

- [ ] **Step 1: i64 view submodule** — create `boot/stdlib/buffer/i64view.tw`:
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

- [ ] **Step 2: f64 view submodule** — create `boot/stdlib/buffer/f64view.tw`, identical to i64view but element type `Float`, stride `8`, `__buf_load_f64`/`__buf_store_f64`:
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

- [ ] **Step 3: u8 view submodule** — create `boot/stdlib/buffer/u8view.tw`, element type `Int` (raw byte 0–255 in Int domain — avoids the `Byte.from_int` Option dance in hot loops), stride `1`, `__buf_load_u8`/`__buf_store_u8`:
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

- [ ] **Step 4: `@std.buffer` module** — create `boot/stdlib/buffer.tw`. Note the **size guard** in `new` (validated in the i64 domain, before the ABI narrows to i32 — this is the trap on negative/oversized requests the design requires):
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
/// Traps on a negative request or one too large to address (linear-memory
/// pointers are 32-bit), since the raw allocator's ABI narrows `Int` to i32.
pub fn new(nbytes: Int) Buffer {
  if nbytes < 0 {
    error("Buffer.new: negative size")
  }

  if nbytes > 0x70000000 {
    error("Buffer.new: size too large")
  }

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
```bash
python3 tools/generate_core_lib.py && cargo build --release && \
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm
```
Expected: clean build. (If the resolver rejects the re-export, re-check `use .buffer.i64view as i64view_mod` + `pub type I64View = i64view_mod.I64View` against `boot/stdlib/tuple.tw:16,21`.)

- [ ] **Step 6: Write the suite** — create `boot/tests/suites/stdlib_buffer_suite.tw`:
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
        try assert.equal(b2.ptr, ptr)
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
        b.set_u8(8, b.get_u8(0))
        try assert.equal(b.get_u8(8).to_int(), b.get_u8(0).to_int())
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
      "coalescing: two adjacent frees reused by one larger alloc",
      fn() {
        a := buffer.new(16)
        base := a.ptr
        c := buffer.new(16)
        a.free()
        c.free()
        big := buffer.new(40)
        try assert.is_true(big.ptr <= base)
        big.free()
        .Ok({})
      },
    )
}
```
Implementer notes: verify `String.utf8_bytes()` (memory lists `STRING_UTF8_BYTES`); if the name differs, build `src` from `Byte`-valued literals instead. The coalescing assertion is deliberately loose (`big.ptr <= base`) to prove the freed region is reused rather than bumped past; tighten to `== base` only if allocation order makes it deterministic.

- [ ] **Step 7: Register the suite** in `boot/tests/main.tw` (`use .suites.stdlib_buffer_suite` + `stdlib_buffer_suite.suite()` in the list).

- [ ] **Step 8: Run the suite**
```bash
python3 tools/generate_core_lib.py && cargo build --release && \
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm && \
  BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
    tools/js_runtime/deno_main.mjs run boot/tests/main.tw 2>&1 | grep -i 'buffer\|fail'
```
Expected: the "stdlib buffer" suite passes.

- [ ] **Step 9: Verify the size guard traps** — create `/tmp/buf_neg.tw`:
```tw
use @std.buffer
b := buffer.new(-1)
println("${b.len()}")
```
Run it via the stage1 path; expected: a trap with message `Buffer.new: negative size` (non-zero exit), **not** a printed length. (Then delete the temp file.)

- [ ] **Step 10: Format, lint, commit** — `target/twk fmt` the four stdlib files + suite; `target/twk lint boot/stdlib/buffer.tw`. Subject e.g. `buffer: @std.buffer module + u8/i64/f64 view submodules`.

---

## Task 4: Internalize the `__buf_*` surface

Move the `__buf_*` signatures out of the global `builtin_env()` into `add_internal_host_builtins()` so only stdlib/prelude (including `@std.buffer`) can call them — closing the M3 global leak.

**Files:** Modify `boot/compiler/base_env.tw`.

- [ ] **Step 1: Remove the `__buf_*` block from `builtin_env()`** (the chain at ~line 423–432 plus the Task 2 additions), including the now-stale "available in all modules" comment.

- [ ] **Step 2: Add the full set to `add_internal_host_builtins()`** (after the `__host_*` chain):
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

- [ ] **Step 3: Rebuild + confirm `@std.buffer` still works**
```bash
python3 tools/generate_core_lib.py && cargo build --release && \
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm && \
  BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
    tools/js_runtime/deno_main.mjs run boot/tests/main.tw 2>&1 | grep -i 'buffer\|fail'
```
Expected: "stdlib buffer" suite still passes.

- [ ] **Step 4: Confirm user code can no longer call `__buf_*`** — run `/tmp/buf_smoke.tw` via the stage1 path. Expected: a typecheck error ("Undefined variable: __buf_alloc"). Then delete `/tmp/buf_smoke.tw`.

- [ ] **Step 5: Format, commit** — subject e.g. `buffer: hide __buf_* behind @std.buffer (internal-host only)`.

---

## Task 5: Conditional linear-memory emission

Emit the `rt.buf` memory + globals only when a buffer op survives DCE, so non-buffer programs carry no linear memory. DCE prunes funcs but **not** memories/globals, so prune them by name after DCE — using the **linker-qualified** names (`rt_buf__*` for funcs/globals; the memory keeps its unqualified name `"heap"`).

**Files:** Modify `boot/compiler/codegen/codegen.tw`.

- [ ] **Step 1: Inspect the linked-module shape**

Run: `grep -n 'fn eliminate_dead_wasm\|memories\|globals\|funcs\|LinkedModule' boot/compiler/codegen/linker.tw | head`
Confirm `LinkedModule`'s field names for funcs/memories/globals and whether it's mutable or needs a record-update rebuild.

- [ ] **Step 2: Confirm the actual qualified names** (don't trust the prefix blindly)

Build a buffer program to WAT and read the names:
```bash
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/with_buf.tw -o /tmp/with_buf.wat   # create /tmp/with_buf.tw per Step 4 first
grep -oE 'rt_buf__buf_[a-z_0-9]+|\(memory[^)]*|\(global [^ )]+' /tmp/with_buf.wat | sort -u | head
```
Expected: func/global names like `rt_buf__buf_alloc`, `rt_buf__buf_heap_ptr`, and a `(memory ...)`. Note the exact memory name token (expected `"heap"`); use the real strings in Step 3.

- [ ] **Step 3: Add the post-DCE prune in `link_program`** (immediately after `linked = eliminate_dead_wasm(linked)`):
```tw
// Conditional linear-memory emission: rt.buf's memory + globals survive Wasm
// DCE (which only prunes funcs), so drop them when no buffer op is live. Names
// are linker-qualified: rt.buf funcs/globals -> "rt_buf__*"; the memory keeps
// its unqualified name "heap" (linker concatenates memories without renaming).
buf_live := linked.funcs.any(fn(f) { f.name.starts_with("rt_buf__") })

if !buf_live {
  linked.memories = linked.memories.filter(fn(m) { m.name != "heap" })
  linked.globals = linked.globals.filter(fn(g) { !g.name.starts_with("rt_buf__") })
}
```
Adjust field/method names to the actuals from Step 1 (e.g. `linked.funcs` vs `linked.functions`; rebuild via record update if `LinkedModule` is immutable). If Step 2 showed a different memory-name token, match that instead of `"heap"`.

- [ ] **Step 4: Non-buffer program emits no memory** — create `/tmp/no_buf.tw` (`println("hello ${1 + 2}")`) and `/tmp/with_buf.tw`:
```tw
use @std.buffer
b := buffer.new(16)
b.set_i64(0, 7)
println("${b.get_i64(0)}")
b.free()
```
```bash
python3 tools/generate_core_lib.py && cargo build --release && \
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm
for p in no_buf with_buf; do
  BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
    tools/js_runtime/deno_main.mjs build /tmp/$p.tw -o /tmp/$p.wat
  echo "$p memory count: $(grep -c '(memory' /tmp/$p.wat)"
done
```
Expected: `no_buf memory count: 0`, `with_buf memory count: 1` (or more). Then run both to confirm execution: `no_buf` prints `hello 3`, `with_buf` prints `7`.

- [ ] **Step 5: Sort program also has no memory** (post-severance baseline guard)
```bash
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs build /tmp/sort_check.tw -o /tmp/sort2.wat
grep -c '(memory' /tmp/sort2.wat
```
Expected: `0`.

- [ ] **Step 6: Format, commit** — subject e.g. `buffer: emit linear memory only when a buffer op is live`.

---

## Task 6: Documentation

**Files:** Modify `docs/spec.md`, `docs/API.md`.

- [ ] **Step 1: Spec** — near the immutability/`Cell` discussion in `docs/spec.md`, add a short paragraph: all values are immutable except the two explicit, opt-in mutate-in-place reference types — `Cell` and `@std.buffer`'s `Buffer` (sandboxed linear-memory region, manually freed). Link to `docs/plans/buffer-linear-memory.md`.

- [ ] **Step 2: API** — in `docs/API.md`'s Standard Library section, add a `@std.buffer` entry: the two import lines; `buffer.new`/`from_bytes`/`free`/`len`/`to_bytes`; raw `get/set_u8|i64|f64` (byte-addressed, little-endian, unaligned, **unchecked**, `new` traps on negative/oversized); `view_u8`/`view_i64`/`view_f64` → `at`/`set`/`len`/`slice`/`iter` (element-indexed); the manual-lifetime / use-after-free caveat. Add `@std.buffer` to the `use @std.X` list line if present.

- [ ] **Step 3: Commit** — subject e.g. `docs: document @std.buffer (linear-memory Buffer + views)`.

---

## Task 7: Full-gate verification + closeout

- [ ] **Step 1: Self-host fixed point + full suite**

Run: `make bundle-cli` — expect "Fixed point reached". Then `make boot-test` — expect all suites pass, including "stdlib buffer".

- [ ] **Step 2: Re-confirm both memory cases with the bundled CLI**
```bash
target/twk build /tmp/no_buf.tw -o /tmp/no_buf2.wat && grep -c '(memory' /tmp/no_buf2.wat      # 0
target/twk build /tmp/with_buf.tw -o /tmp/with_buf2.wat && grep -c '(memory' /tmp/with_buf2.wat # >=1
target/twk run /tmp/with_buf.tw    # prints 7
```

- [ ] **Step 3: Update the design doc + README** — in `docs/plans/buffer-linear-memory.md`, flip the top status line and the M2 roadmap bullet to DONE, noting M3b (fast IR codec) is the next consumer. In `docs/plans/README.md`, update the Linear-memory Buffer row (M2 shipped; M3b/M4 future).

- [ ] **Step 4: Archive this plan** — `git mv docs/plans/buffer-linear-memory-m2-plan.md docs/plans/archive/buffer-linear-memory-m2-plan.md`, and update the README link accordingly.

- [ ] **Step 5: Commit** — subject e.g. `buffer: M2 complete — mark done, archive plan`.

---

## Notes / risks for the implementer

- **The allocator (Task 2 Part B) is the highest-risk piece.** Its correctness is pinned by Task 3's suite (round-trip, free-list reuse, coalescing); write the `.Instr` against those tests using `alloc_fn`/`load_u8_fn` as syntax references. Missing trivial `Instr` variants get added with their `wasm.tw` opcode + `wat.tw` arm (the M1 pattern).
- **Severance precedes the free-list switch** (Task 2 Part A before Part B): `buf_mark`/`buf_reset` don't track `buf_free_head`, so a dense sort over a free-list allocator would corrupt it. Never have both live at once.
- **`make bundle-cli` is required before `target/twk` reflects codegen/builtin changes.** During development use the stage1 path.
- **Verify names/APIs against the actuals before relying on them:** `LinkedModule` field names + the qualified `rt_buf__*` / `"heap"` strings (Task 5 Step 2), `String.utf8_bytes` (Task 3), `Int.clamp`, `Iterator.unfold`/`UnfoldStep` (prelude). The `error(...)` trap is uncatchable by the runner, so the negative-size case is checked by a standalone program run (Task 3 Step 9), not a suite assertion.
- **`v[i]` element sugar and bare `for x in v` are intentionally out of scope** (they need concrete-receiver contract lowering in the checker). Ship `v.at(i)` + `v.iter()`. `v[lo..hi]` slice sugar works via `synth_slice` and needs no new wiring.
