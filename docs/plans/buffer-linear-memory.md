# Linear-Memory `Buffer`

Status: **M1 + M3 validated (GO); M2 DONE.** The linear-memory direction is proven on
the workload it is actually for — dense byte indexing / codecs — where it is decisively
faster than GC `Vector<Byte>`. **M2 (the user-facing `@std.buffer` `Buffer` type + typed
views) shipped** on branch `buffer-linear-memory` (self-host fixed point holds, full boot
suite green, linear memory emitted only when a buffer op is live). The next consumer is
**M3b (fast IR codec)**. Branch: `buffer-linear-memory` (off `main`).

The detailed M1 and M3 *execution* plans are completed and archived under
`docs/plans/archive/buffer-linear-memory-m{1,3}-plan.md`; their validated findings are
condensed here as the evidence base for M2.

## Validated results

A LEB128 varint codec and a self-contained MD5, each implemented twice (once decoding
over linear memory via `i32.load8_u`, once over a GC `Vector<Byte>`), A/B'd on
decode-dominated workloads with data filled natively into each representation (no
cross-gather, decode/compress timed). Identical checksums, gated by a pre-timing
correctness trap. Same machine, rebundled CLI.

| workload | GC `Vector<Byte>` | linear memory | ratio |
|---|---|---|---|
| LEB128 decode, 4M varints (warm) | ~337 ms | ~11 ms | **~30×** |
| MD5 of 4M bytes (warm) | ~105 ms | ~47 ms | **~2.2×** |
| `Vector<Int>.sort()`, 1M ints (warm) | ~59–66 ms | ~59–63 ms | parity |

The codec gap exceeds the naive ~log₃₂ n depth advantage because each `Vector<Byte>`
index pays a persistent-trie walk **plus** a GC ref-cast/unbox per byte, whereas the
linear path is a single near-native byte load. MD5's ~47 ms floor is irreducible
compression arithmetic; the ~58 ms difference is pure byte-read overhead — i.e. byte
reads were ~55% of MD5's wall time over `Vector<Byte>`. The sort lands at parity
because it is comparator-bound and reads each scratch slot ~once (it was M1's
bellwether, not its target).

**Takeaways that shape M2:**
- Linear memory pays off for **byte-indexing / codec / decode** workloads, not sorts.
  (The sort read-wall lever remains typed `PVecI64` storage — `project_typed_vector_repr`.)
- The MD5 win is real only when message bytes **originate in linear memory**; copying a
  `Vector<Byte>` in costs the read overhead it saves. The same I/O-into-linear primitive
  is what a future `crypto.digest_file` and M4 both need.

## Motivation

Twinkle is entirely Wasm-GC: every collection is a GC object, and the only mutable
flat storage (`rt_types__Array`, the PVec backing) is reachable solely inside `rt.arr`.
The cost of having no raw, in-place, unboxed buffer recurs across the codebase: the
typed-`Vector<Int>` read wall (PVec random access is O(log₃₂ n)); the reverted native
dense sort (`b915637`, GC-array `Scratch<T>` regressed ~16% on `anyref` cast + call per
touch); columnar workloads (dataframe gather cliffs); and the Phase G result-decode
wall (a byte decoder over `Vector<Byte>` is O(n·log n)).

Linear memory provides what is missing: **O(1) indexed, unboxed, cache-local mutable
storage**, with `SharedArrayBuffer` as a much-later door to shared-memory parallelism.

### Hard constraint that shapes everything

Linear memory can hold **only unboxed primitives** (`Int`/i64, `Float`/f64, `Byte`/u8,
`Bool`). GC references (`String`, records, closures) cannot live in it. This aligns
with where the pain is — primitive numeric arrays and byte buffers — so `Buffer`
*augments* `Vector`; it never replaces it. GC-element collections keep using PVec.

## Roadmap

- **M1 — linear-memory infrastructure (DONE).** Wider load/store IR
  (`i32/i64/f64.load/store`, `memory.size/grow`) + a program memory + a bump allocator
  (`rt.buf`), proven to coexist with GC under V8. Internal machinery only; no
  user-facing type. Sort proxy came back parity — settled that a linear scratch sort is
  not the read-wall lever, but left the infrastructure intact.
- **M3 probe — byte codec (DONE, GO).** Raw `__buf_*` byte accessors + a LEB128 codec
  decisively beat `Vector<Byte>` (~30×). This is the honest go/no-go the sort never
  answered. Probe artifacts are throwaway.
- **M2 — user-facing `Buffer` + typed views (DONE).** A sandboxed, low-level, ergonomic
  type as the abstraction over linear memory (`@std.buffer`: first-class manually-managed
  `Buffer` + `U8View`/`I64View`/`F64View`). Success was the *type*, not a perf number; the
  perf consumers come later. The design below is what shipped.
- **M3b — fast IR codec (future).** A flat linear-memory artifact format with O(1)
  decode, attacking the Phase G result-decode wall — the first real perf consumer.
- **M4 — shared-memory parallelism (future, the original ask).**
  `SharedArrayBuffer`-backed byte buffers across compile Workers. Needs `shared` memory
  + atomics + SAB runtime backing on top of M1's IR (see *Strategic case* below).

---

## M2 — user-facing `Buffer` + typed views (active design)

### Philosophy and safety model

`Buffer` is an **opt-in, sandboxed-but-low-level, manually-managed** linear-memory
region — Twinkle's **second mutate-in-place reference type alongside `Cell`**, and the
explicit escape hatch from an otherwise-immutable language. It is **fully first-class**:
returnable, storable in records/collections, freely captured. Correctness — calling
`free`, not using after free — is the **programmer's responsibility, like C**. It is
*not* a "safe" abstraction; the only floor Wasm gives for free is that all access is
sandboxed within the linear memory, so the worst case is reading/corrupting *another
buffer's* bytes or trapping at the memory edge — never true UB or an escape from the
sandbox. Throughout this doc "sandboxed low-level," not "safe," is the accurate framing.

This deliberately rejects the heavier alternatives considered (arena scoping,
second-class/non-escaping `Buffer`, escape analysis). Manual management gives maximum
leverage and is markedly simpler to build, and it matches the intent: linear memory is
a low-level construct, so the surface should expose precise control of it.

### Lifetime

Manual `buffer.new` / `buf.free()`. The idiomatic scope hook is **`defer`**:

```tw
buf := buffer.new(1024)
defer buf.free()              // runs at block exit, LIFO, captures the handle by value
buf.set_i64(0, 42)
```

`defer` is tied to the nearest enclosing `{ }` block, runs on every exit except trap,
fires LIFO, and captures by value — so `defer buf.free()` is correct and composes with
nested allocations. It is the common pattern, not the only legal one (first-class
buffers may also be freed wherever their owner decides).

### Public API

Two import lines, per the stdlib-module convention (the plain form binds the module
alias `buffer` for constructors; the destructuring form binds the type names for
annotations):

```tw
use @std.buffer                                    // module alias -> buffer.new(...)
use @std.buffer.{Buffer, U8View, I64View, F64View} // type names for annotations
```

Constructors are **module-qualified functions** (`buffer.new`), not type-qualified
statics (`Buffer.new`): a pure stdlib module has no `Type.fn` static-call form — that is
reserved for compiler builtins like `Cell.new`. This mirrors `@std.view`'s `view.from`.

```tw
// construction / lifetime
buffer.new(nbytes: Int) Buffer
buffer.from_bytes(bytes: Vector<Byte>) Buffer      // alloc + copy in
buf.free()                                          // release the region
buf.len() Int                                       // byte length
buf.to_bytes() Vector<Byte>                         // copy out

// raw access — BYTE-addressed (offset is a byte offset), little-endian, unaligned ok
buf.get_u8(off: Int) Byte        buf.set_u8(off: Int, v: Byte)
buf.get_i64(off: Int) Int        buf.set_i64(off: Int, v: Int)
buf.get_f64(off: Int) Float      buf.set_f64(off: Int, v: Float)

// typed views — ELEMENT-indexed handles over a region, no extra allocation
buf.view_u8(byte_off: Int, count: Int)             // -> U8View
buf.view_i64(byte_off: Int, count: Int)            // -> I64View
buf.view_f64(byte_off: Int, count: Int)            // -> F64View
v.at(i: Int) T         v.set(i: Int, x: T)         // element i  (read / write)
v.len() Int                                         // element count  (with at: satisfies IndexRead)
v.slice(lo: Int, hi: Int)                          // sub-view, shares backing, no alloc
v.iter() Iterator<T>                               // for x in v.iter() { ... }
v[lo..hi]                                           // slice sugar -> v.slice (Sliceable; works on concrete)
```

**Method names track the contracts:** `IndexRead` is `at(self, Int) E` + `len(self) Int`
(not `get`). Providing both makes each view *satisfy* `IndexRead<T>`, so views compose
and can be passed to `IndexRead`-bounded generics (e.g. `view.from`, `view.fold`) — the
same way `@std.view` does. See *Index/iteration sugar* below for why this does **not**
imply `v[i]` element sugar.

### Semantics

- **Raw `Buffer` = byte offsets; views = element indices.** On a raw buffer the width
  is explicit per call (`get_u8`/`get_i64`/`get_f64`) and the argument is a byte
  offset; on a view the argument is an element index and the width is fixed by the view
  type. This split keeps both honest.
- **Endianness / alignment:** little-endian (native Wasm), unaligned access allowed.
- **Bounds:** access is **unchecked** against a buffer/view's logical length; only
  Wasm's whole-memory bound traps. Indexing past `len` but inside the memory reads
  garbage or corrupts a neighboring buffer (logically wrong, sandbox-safe). This
  preserves the bare-load speed that motivates linear memory.
- **Index / iteration sugar (scoped down after verifying the checker):** element reads
  use the explicit method **`v.at(i)`**, *not* `v[i]`. `synth_index` only lowers `c[i]`
  to a contract `at` call for **type variables** bounded `IndexRead` (and direct for the
  builtins `Vector`/`Dict`/`String`); a **concrete** named type — exactly what a view is
  — falls through to `NotIndexable`. So `v[i]` element sugar would require new
  checker/lowering work (extend `synth_index`'s concrete-receiver arm) and is **deferred
  out of M2** (it would also light up `@std.view`'s own `v[i]`, so it is a general
  improvement worth its own change, not a buffer detail). Likewise `for x in v` does not
  fire for a concrete type; M2 ships **`v.iter()`** (a builtin `Iterator<T>` built via
  `Iterator.unfold`) so `for x in v.iter()` works with zero checker changes.
  - **Slice sugar `v[lo..hi]` *does* work** on concrete views: `synth_slice` accepts a
    concrete receiver with an inherent `slice` method and lowering emits a uniform
    `Sliceable` contract call. So views get range-slice sugar for free; only element
    `[i]` does not.
  - Writes stay explicit `v.set(i, x)`: no `IndexWrite`, because `arr[i] = v` desugars to
    *rebind-and-build-new* for `Vector`, which would silently conflict with `Buffer`'s
    mutate-in-place contract. Raw `Buffer` stays methods-only (no `[]`, multi-width).
- **Double-free / use-after-free:** documented-undefined — corrupts the allocator's
  bookkeeping but stays within the sandbox. No runtime guard in M2.

### Representation

- `Buffer = .{ ptr: Int, size: Int }` — a GC record; `ptr` is the linear-memory offset,
  `size` the byte length (the field is `size`, not `len`, because a record field and the
  inherent `len()` method may not share a name); the public surface is `buf.len()`.
  `len` the byte length. Being an ordinary GC ref is what makes it first-class
  (storable/returnable).
- **Three concrete view types** — `U8View`, `I64View`, `F64View` — each a thin
  `.{ ptr: Int, byte_off: Int, count: Int }` record, **not** a single generic
  `View<T>`. Without traits, one generic view cannot dispatch `at`/`set` to per-width
  load/store intrinsics nor vary its return type by `T`; three concrete types are the
  honest no-trait expression.
- **Each view type lives in its own submodule, transparently re-exported from
  `@std.buffer` (the `@std.tuple` / `@std.tuple.triple` precedent).** A Twinkle module's
  function namespace has no overloading (`resolver.tw` raises `DuplicateName` on a repeated
  `pub fn`, and inherent dispatch uses `method_name = f.name`), so a *single* module
  cannot hold three `at`/`set`/`len` witnesses — exactly the constraint `tuple.tw` notes
  ("a single module cannot hold two `to_string` witnesses"). So:
  - `boot/stdlib/buffer/u8view.tw` → `@std.buffer.u8view` defines `U8View` + its `at`/
    `set`/`len`/`slice`/`iter`; likewise `i64view.tw` and `f64view.tw`.
  - `boot/stdlib/buffer.tw` → `@std.buffer` re-exports them transparently
    (`use .buffer.i64view as i64view_mod` + `pub type I64View = i64view_mod.I64View`), so
    one import surface (`use @std.buffer.{Buffer, U8View, I64View, F64View}`) names all
    four types while each view's methods resolve via its nominal home.
  - The view submodules hold a raw `ptr: Int` (not a `Buffer`), so they do **not** import
    `@std.buffer` — the dependency is one-directional (`buffer` → view submodules), no
    cycle. They reach `__buf_*` through `add_internal_host_builtins` like any stdlib module.
- **Handles are forgeable — accepted under the sandboxed-low-level model.** A `pub type
  Buffer = .{ ptr, size }` exposes its fields, so a user can construct an arbitrary
  `Buffer.{ ptr: 999, size: 8 }` or rebind `buf.ptr`. Twinkle has no record-field privacy,
  and this is consistent with the model: a forged handle can still only touch the
  sandboxed linear memory (no worse than the already-unchecked `get/set`), and "rebinding"
  a field is immutable-build-new (it cannot corrupt an existing handle's bytes). M2 does
  **not** add an opaque representation; if a future milestone wants real handle integrity,
  the option is to promote `Buffer` to an opaque compiler builtin (recorded, not built).

### Implementation (mostly reuses M1/M3)

1. **`rt.buf` → free-list allocator (contract specified).** Replace the bump-only
   allocator with a free-list + coalescing, since first-class buffers are freed in
   arbitrary (non-LIFO) order. Concrete contract:
   - **Block layout:** each block carries an 8-byte header `[ size: i32 ][ free: i32 ]`
     immediately before its 8-byte-aligned payload; `buf_alloc` returns the payload
     pointer, `buf_free(ptr)` reads the header at `ptr - 8`. (Size lives in the header,
     so `free` needs only the pointer — no caller-supplied length.)
   - **`buf_alloc(nbytes) -> ptr`:** first-fit over the free list, splitting a larger
     free block when the remainder fits another header+payload; otherwise bump the heap
     end, growing via `memory.grow` and **trapping if grow fails**.
   - **`buf_free(ptr)`:** mark the block free and coalesce with adjacent free neighbors.
   - **Edge cases:** `nbytes` is a Twinkle `Int` (i64) narrowed to i32 at the abi
     boundary — trap on negative or on a size that would overflow i32 / exceed the max
     memory. Addresses fit i32 (linear memory < 4 GiB). Double-free/use-after-free
     corrupt this bookkeeping (documented-undefined, still sandboxed).
   - Add `i64`/`f64` load/store runtime funcs (byte funcs `buf_load_u8`/`buf_store_u8`
     already exist from M3).
2. **Intrinsics (3-site recipe).** Add `__buf_free`, `__buf_load_i64`/`__buf_store_i64`,
   `__buf_load_f64`/`__buf_store_f64` (`builtins.tw` rt entry + `builtin_abi` declaring
   the i64↔i32 wasm bridge — not automatic; plus the `new_ctx` `__`-alias gate). **Move
   all `__buf_*` out of the global `builtin_env` into internal-host builtins** reachable
   only from `@std.buffer` — closing the M3 global-surface leak.
3. **`@std.buffer` module + three view submodules.** `boot/stdlib/buffer.tw` holds the
   `Buffer` record, raw accessors, `from_bytes`/`to_bytes` (copy loops), the `view_*`
   constructors, and transparent re-exports of the view types. Each view type is its own
   submodule (`boot/stdlib/buffer/{u8view,i64view,f64view}.tw`) defining that type +
   `at`/`set`/`len` (the `at`+`len` pair makes it satisfy `IndexRead`), `slice` (satisfies
   `Sliceable` → `v[lo..hi]`), and `iter()` (a builtin `Iterator<T>` via `Iterator.unfold`,
   backing `for x in v.iter()`). Pure Twinkle over the intrinsics, per the stdlib-module
   wiring recipe (no Rust stage0 change — `boot/main.tw` does not use `@std.buffer`).
   Element `v[i]` sugar is **not** wired (it needs checker work — see *Index/iteration
   sugar*).
4. **Conditional memory emission (explicit linker/codegen work — does NOT fall out of
   DCE).** Wasm DCE removes unused funcs/imports but **not** memories, globals, or data,
   and today `runtime_modules()` (`codegen.tw`) pushes `rt_buf.module()` unconditionally.
   Worse, `rt.arr`'s M1 dense linear-scratch sort imports `buf_alloc`/`buf_mark`/`buf_reset`,
   so **any `Vector.sort()` pulls in `rt.buf`** independent of `@std.buffer`. Two steps:
   - **Sever `rt.arr → rt.buf`.** Remove the M1 dense linear-scratch merge sort from
     `arr.tw` (it benched at *parity* — the read-wall lever is typed `PVecI64`, not this
     scratch), restoring the recursive-merge path. After this, `rt.buf` is reachable
     **only** through `@std.buffer`'s `__buf_*` intrinsics.
   - **Gate `rt.buf` inclusion at module-assembly time.** Make `runtime_modules()` /
     module assembly include `rt_buf.module()` (and therefore its `MemoryDef` + globals)
     **only when the lowered program references a buffer intrinsic** — a pre-link
     reachability check over the user module's calls, not a post-emit DCE pass. Programs
     that never `use @std.buffer` then emit no linear memory at all.
5. **Remove probe artifacts:** `boot/lib/buf_codec.tw`, `boot/bench/buf_codec_bench.tw`,
   `boot/bench/md5_linear_bench.tw`, `boot/tests/suites/buf_codec_suite.tw`.
6. **Docs + hygiene:** `docs/spec.md` (`Buffer` as the second mutate-in-place type) and
   `docs/API.md` entries; `twk fmt` + `twk lint` on every edited `.tw`.

### Non-goals (M2)

- No arena, second-class/escape safety, or `IndexWrite` sugar (all considered and
  rejected above).
- **No `v[i]` element-index sugar and no bare `for x in v`** — both need new
  concrete-receiver access-contract lowering in the checker; M2 ships `v.at(i)` and
  `v.iter()` instead. (Slice sugar `v[lo..hi]` *is* in, since it already works on
  concrete types.)
- No M3b IR codec or M4 shared-memory/atomics consumer — M2 ships the sandboxed
  low-level type only.
- No leak detection, double-free guard, opaque/forge-proof handle, or
  alignment-required fast paths.
- No change to `Vector` semantics; `Buffer` augments, never replaces.

---

## Strategic case — M3b / M4 (future, untested)

The reasons to want linear memory beyond M3's probe are a single-process **byte codec**
and cross-Worker **shared memory**. Both rest on the same hard constraint:

> **Only flat bytes live in linear memory — never GC objects.** Twinkle values
> (`String`, records, `Vector`, closures) are Wasm-GC heap objects. So linear memory
> helps move/serialize *byte representations* (IR, Wasm, packed columns), not live
> object graphs. A byte codec is the unlock that turns GC artifacts into the flat form
> shared memory can transport.

- **M3b — byte-level IR codec (nearer-term).** A flat linear-memory artifact the
  compiler decodes with O(1) indexing, replacing the O(n·log n) `Vector<Byte>` decode
  that sank Phase G. Needs only the *plain* memory + load/store M1 already provides. The
  lowest-risk first real consumer of the M2 type.
- **M4 — shared-memory worker buffers (the original ask).** Move serialized payloads (a
  compiled Wasm module, an IR blob) between compile Workers through a
  `SharedArrayBuffer`-backed memory, avoiding the structured-clone copies the current
  `postMessage` remote-channel transport pays (`project_parallel_compile_workers`, whose
  next step — "spawn_worker ABI + Wasm codec" — is exactly the flat payload M3b produces).

### IR/runtime gaps M1 did NOT close (needed for M4)

- **`MemoryDef` cannot express `shared`.** Add a `shared: Bool` field; the limits
  encoder (`encode_memory_section_payload`, `wasm.tw`) must emit flag `0x03` (shared
  requires a max). Small additive IR change.
- **No atomics.** `i32/i64.atomic.load/store`, `atomic.rmw.*`,
  `memory.atomic.wait32/notify` — none exist in the `Instr` enum; each is an emit + WAT
  arm like M1's load/store work.
- **Runtime: SAB-backed shared memory.** `runtime.mjs`/`deno_main.mjs` must instantiate
  the memory from a `SharedArrayBuffer` and pass the *same* `WebAssembly.Memory`
  (imported, not module-owned) into every Worker. Needs `crossOriginIsolated`.
- **Conditional memory emission** — also an M2 deliverable (above).
