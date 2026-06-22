# Linear-Memory `Buffer`

Status: **M1 + M3 validated (GO).** The linear-memory direction is proven on the
workload it is actually for — dense byte indexing / codecs — where it is decisively
faster than GC `Vector<Byte>`. **M2 (the user-facing `Buffer` type) is the active
milestone; its design is below.** Branch: `buffer-linear-memory` (off `main`).

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
- **M2 — user-facing `Buffer` + typed views (ACTIVE — design below).** A clean, safe,
  ergonomic type as the abstraction over linear memory. Success is the *type*, not a
  perf number; the perf consumers come later.
- **M3b — fast IR codec (future).** A flat linear-memory artifact format with O(1)
  decode, attacking the Phase G result-decode wall — the first real perf consumer.
- **M4 — shared-memory parallelism (future, the original ask).**
  `SharedArrayBuffer`-backed byte buffers across compile Workers. Needs `shared` memory
  + atomics + SAB runtime backing on top of M1's IR (see *Strategic case* below).

---

## M2 — user-facing `Buffer` + typed views (active design)

### Philosophy and safety model

`Buffer` is an **opt-in, low-level, manually-managed** linear-memory region — Twinkle's
**second mutate-in-place reference type alongside `Cell`**, and the explicit escape
hatch from an otherwise-immutable language. It is **fully first-class**: returnable,
storable in records/collections, freely captured. Correctness — calling `free`, not
using after free — is the **programmer's responsibility, like C**. The one safety floor
Wasm gives for free: all access is sandboxed within the linear memory, so the worst
case is reading/corrupting *another buffer's* bytes or trapping at the memory edge —
never true UB or an escape from the sandbox.

This deliberately rejects the heavier alternatives considered (arena scoping,
second-class/non-escaping `Buffer`, escape analysis). Manual management gives maximum
leverage and is markedly simpler to build, and it matches the intent: linear memory is
a low-level construct, so the surface should expose precise control of it.

### Lifetime

Manual `Buffer.new` / `buf.free()`. The idiomatic scope hook is **`defer`**:

```tw
buf := Buffer.new(1024)
defer buf.free()              // runs at block exit, LIFO, captures the handle by value
buf.set_i64(0, 42)
```

`defer` is tied to the nearest enclosing `{ }` block, runs on every exit except trap,
fires LIFO, and captures by value — so `defer buf.free()` is correct and composes with
nested allocations. It is the common pattern, not the only legal one (first-class
buffers may also be freed wherever their owner decides).

### Public API (`use @std.buffer`)

```tw
use @std.buffer.{Buffer}

// construction / lifetime
Buffer.new(nbytes: Int) Buffer
Buffer.from_bytes(bytes: Vector<Byte>) Buffer      // alloc + copy in
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
v.get(i: Int) T        v.set(i: Int, x: T)         // element i
v.len() Int                                         // element count
v.slice(lo: Int, hi: Int)                          // sub-view, shares backing, no alloc
v[i]                                                // read sugar -> v.get(i) (IndexRead)
for x in v { ... }                                  // IntoIterator
```

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
- **Index sugar:** views satisfy **`IndexRead`** so `v[i]` reads (lowering to
  `v.get(i)`) — essentially free once `get` exists. Writes stay explicit `v.set(i, x)`:
  no `IndexWrite`, because `arr[i] = v` desugars to *rebind-and-build-new* for `Vector`,
  which would silently conflict with `Buffer`'s mutate-in-place contract. Raw `Buffer`
  stays methods-only (no `[]`, since it is multi-width).
- **Double-free / use-after-free:** documented-undefined — corrupts the allocator's
  bookkeeping but stays within the sandbox. No runtime guard in M2.

### Representation

- `Buffer = .{ ptr: Int, len: Int }` — a GC record; `ptr` is the linear-memory offset,
  `len` the byte length. Being an ordinary GC ref is what makes it first-class
  (storable/returnable).
- **Three concrete view types** — `U8View`, `I64View`, `F64View` — each a thin
  `.{ ptr: Int, byte_off: Int, count: Int }` record, **not** a single generic
  `View<T>`. Without traits, one generic view cannot dispatch `get`/`set` to per-width
  load/store intrinsics nor vary its return type by `T`; three concrete types are the
  honest no-trait expression.

### Implementation (mostly reuses M1/M3)

1. **`rt.buf` → free-list allocator.** Replace the bump-only allocator with a free-list
   + coalescing (`buf_alloc(nbytes) -> ptr`, `buf_free(ptr)`), since first-class
   buffers are freed in arbitrary (non-LIFO) order. Add `i64`/`f64` load/store runtime
   funcs (byte funcs `buf_load_u8`/`buf_store_u8` already exist from M3).
2. **Intrinsics (3-site recipe).** Add `__buf_free`, `__buf_load_i64`/`__buf_store_i64`,
   `__buf_load_f64`/`__buf_store_f64` (`builtins.tw` rt entry + `builtin_abi` declaring
   the i64↔i32 wasm bridge — not automatic; plus the `new_ctx` `__`-alias gate). **Move
   all `__buf_*` out of the global `builtin_env` into internal-host builtins** reachable
   only from `@std.buffer` — closing the M3 global-surface leak.
3. **`@std.buffer` module.** The `Buffer` record + three view records, all methods,
   `from_bytes`/`to_bytes` (copy loops), `slice`, the `IndexRead` satisfier for `v[i]`,
   and the `IntoIterator` satisfier for `for x in v`. Pure Twinkle over the intrinsics,
   per the stdlib-module wiring recipe (little/no Rust stage0 change expected).
4. **Conditional memory emission.** Emit the `rt.buf` memory + funcs **only when
   reachable after DCE**, so programs that don't `use @std.buffer` pay zero (today the
   ~1 MiB memory is emitted on every module). This falls out naturally from the opt-in
   module: no import → no `__buf_*` reachable → no memory.
5. **Remove probe artifacts:** `boot/lib/buf_codec.tw`, `boot/bench/buf_codec_bench.tw`,
   `boot/bench/md5_linear_bench.tw`, `boot/tests/suites/buf_codec_suite.tw`.
6. **Docs + hygiene:** `docs/spec.md` (`Buffer` as the second mutate-in-place type) and
   `docs/API.md` entries; `twk fmt` + `twk lint` on every edited `.tw`.

### Non-goals (M2)

- No arena, second-class/escape safety, or `IndexWrite` sugar (all considered and
  rejected above).
- No M3b IR codec or M4 shared-memory/atomics consumer — M2 ships the safe type only.
- No leak detection, double-free guard, or alignment-required fast paths.
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
