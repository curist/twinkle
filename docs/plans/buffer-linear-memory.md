# Linear-Memory `Buffer` (foundation-first)

Status: Design approved; M1 not started. Branch: `buffer-linear-memory` (off `main`).

## Motivation

Twinkle is entirely Wasm-GC: every collection is a GC object, and the only
mutable flat storage (`rt_types__Array`, the PVec backing) is reachable solely
inside `rt.arr`. The recurring cost of having no raw, in-place, unboxed buffer
shows up across the codebase:

- the typed-`Vector<Int>` "read wall" (PVec random access is O(log₃₂ n));
- the reverted native dense sort (`b915637`): a GC-array `Scratch<T>` merge sort
  regressed plain `Vector<Int>` sort ~16% because every touch paid an `anyref`
  cast + call, plus two extra copies;
- columnar workloads (dataframe `Vector<Float>` columns, gather cliffs);
- the Phase G result-decode wall: a binary IR codec was abandoned because a
  byte-level decoder over `Vector<Byte>` is O(n·log n) — GC arrays have O(log n)
  random indexing.

Linear memory provides exactly what is missing: **O(1) indexed, unboxed,
cache-local mutable storage**, with `SharedArrayBuffer` as a much-later door to
shared-memory parallelism. This effort introduces that capability foundation-first.

## Scope and driver

The driver is **general raw-buffer performance**, not parallelism. The fast IR
codec and any shared-memory parallelism are deferred consumers that become
natural once the primitive exists and is proven.

### Hard constraint that shapes everything

Linear memory can hold **only unboxed primitives** (`Int`/`i64`, `Float`/`f64`,
`Byte`/`u8`, `Bool`). GC references (`String`, records, closures) cannot live in
it. This aligns with where the pain is — primitive numeric arrays and byte
buffers — so `Buffer` *augments* `Vector`; it never replaces it. GC-element
collections keep using PVec.

## The `Buffer` primitive (target design)

- **`Buffer`** — a byte-addressed, mutable, linear-memory region.
  Conceptually: `Buffer.new(nbytes)`, `get_u8/set_u8`, `get_i64/set_i64`,
  `get_f64/set_f64`, `len()` (bytes). It mutates in place.
- **Typed views** — thin GC handles (`{ buffer, byte_off, count }`, no extra
  linear allocation) giving ergonomic element-indexed access:
  `v := buf.view_i64(off, n)` → `v.get(i)`, `v.set(i, x)`, `v.len()` (elements).
  Everyday numeric code lives on views; raw bytes serve future codec / shared-memory uses.
- **`Buffer` and `Cell` are Twinkle's only two mutate-in-place types.** Like
  `Cell`, a `Buffer` is a reference: aliasing means shared mutation, and that is
  the contract. No uniqueness or linear-type machinery is required — the rest of
  the language stays immutable, and these two are the explicit escape hatches.
- **Lifetime: arena-scoped.** Wasm GC has no finalizers, so a `Buffer`'s region
  is not reclaimed when its handle becomes garbage. Buffers allocate from an
  arena that is bulk-reset at a boundary (deterministic, trivial allocator, zero
  per-buffer cost). The trade-off — a buffer must not outlive its arena — is
  acceptable for transient/scratch use and is the M1 scope.

## Roadmap

The work is staged so each milestone proves its predecessor before adding surface.

- **M1 — internal-first (this spec's detailed scope).** Land the linear-memory
  infrastructure (wider load/store, a program memory, a bump/arena allocator) as
  internal compiler-runtime machinery with **no user-facing `Buffer` type**, and
  prove it on the dense sort that was already tried and reverted. Defers the
  hardest design question (user-facing arena scoping + escape safety).
- **M2 — user-facing `Buffer` + typed views.** Surface syntax for arenas
  (`with_arena { ... }` or an explicit `Arena` value), the view API, escape
  discipline, and codegen for user code. Only after M1 proves the primitive pays off.
- **M3 — fast IR codec.** A flat linear-memory artifact format with O(1) decode,
  attacking the Phase G result-decode wall.
- **M4 (speculative) — shared-memory parallelism.** `SharedArrayBuffer`-backed
  buffers across Workers. Only meaningful once artifacts live in linear memory.

Each milestone is additive on proven infrastructure.

## M1 — Linear-memory infrastructure, proven on the dense sort

### What already exists (de-risks M1)

- `WasmModule.memories` + `MemoryDef { name, min_pages, max_pages? }`, memory-section
  encoding (`encode_memory_section_payload` in `wasm.tw`), and linker memory concat
  (`linker.tw` concatenates each module's `memories`).
- Byte instructions: `I32Load8U`, `I32Store8`, `MemoryGrow`, with emit + WAT.
- The separate `bridge.wasm` already declares a `"staging"` memory and uses byte
  load/store for host data staging — proof a memory validates and runs under V8.
- The **output program module currently declares no memory** (`emit.tw: memories: []`);
  it gets its own single memory at index 0, so no multi-memory proposal is needed.

### New work (all boot-codegen; no stage0 parity needed)

The linear-memory machinery is boot codegen that *constructs* wasm IR. The new
Instr variants are ordinary enum additions to `wasm_ir.tw`; stage0 compiles the
boot source that uses them like any other enum, so this follows the established
"no-stage0-parity for boot-codegen" rule.

1. **Wider load/store instructions.** Add `I32Load`/`I32Store`, `I64Load`/`I64Store`,
   `F64Load`/`F64Store` (each carrying an align + offset memarg) to `wasm_ir.tw`,
   with opcode emit in `wasm.tw` and WAT rendering in `wat.tw`. Byte ops already exist.
2. **Program memory + bump/arena allocator.** A new `boot/compiler/codegen/runtime/buf.tw`:
   - contributes one `MemoryDef` for the program module (flows through the linker concat);
   - a mutable `i32` global `buf_heap_ptr`;
   - `buf_alloc(nbytes) -> ptr` that bumps the pointer and grows the memory via
     `MemoryGrow` when the region would overflow the current pages;
   - arena `buf_mark() -> i32` and `buf_reset(mark)` = save/restore `buf_heap_ptr`
     (high-water reset).
   These are internal runtime functions only — no user-facing `Buffer` type, no
   surface syntax.
3. **Dense typed sort over linear scratch.** Near `sort_typed_fn` in `arr.tw`,
   add a merge sort that, for `i64`/`f64` element vectors: gathers the vector into
   an arena-allocated linear scratch region, merges through linear memory using the
   new O(1) load/store (no `anyref` casts, no GC-array copies — the exact costs that
   sank the reverted `Scratch<T>`), scatters the sorted result back into a fresh
   typed vector, then `buf_reset`s the arena before returning. Non-primitive element
   types keep the existing recursive-merge path.

### Validation / success criteria

- **Hard gates:**
  - emitted wasm validates and runs under Deno with the program memory + new load/store;
  - the **self-host fixed point holds** (stage3 == stage4);
  - the full boot test suite passes.
- **The proof (go/no-go for the whole direction):** the sort benches in
  `examples/sort-bench/` (e.g. `sort_repeat_probe.tw`, `sort_by_component_probe.tw`)
  show the linear-memory dense sort **beats both** the current recursive merge and
  the reverted GC-array `Scratch<T>` on plain `Vector<Int>` sort — the workload
  `Scratch<T>` regressed ~16%. Report before/after; if linear memory does not clearly
  win here, the direction stops at M1.

### Risks

- **GC ↔ linear coexistence** in one program module under V8 — low: the bridge
  already proves a memory validates and runs; M1 just adds one to the program module.
- **Gather/scatter overhead.** Copying a GC `Vector<Int>` into linear scratch and
  the sorted result back is an extra pass. The win must exceed it; the bench decides.
- **Memory growth.** The bump allocator must grow pages on demand and the arena
  reset must reclaim the high-water mark so repeated sorts do not leak within a run.

## Non-goals (M1)

- No user-facing `Buffer` type, view API, or arena syntax (that is M2).
- No long-lived / columnar / escaping buffers (transient/scratch only).
- No IR codec or shared-memory work (M3/M4).
- No change to `Vector` semantics; `Buffer` augments, never replaces.
- No stage0 language-level linear-memory support.
