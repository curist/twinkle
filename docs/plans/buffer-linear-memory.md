# Linear-Memory `Buffer` (foundation-first)

Status: **M1 built. The dense sort — M1's chosen *proxy* — came back at parity, so it is not a viable perf lever. That verdict is about the sort, NOT about the linear-memory direction:** the strategic case (M3 fast IR codec, M4 shared-memory worker buffers) was never on trial here, and the foundational load/store IR stands regardless. Branch: `buffer-linear-memory` (off `main`).

### M1 result (2026-06-22)

All hard gates pass: emitted wasm validates and runs under V8 with the program
memory + new load/store; the self-host fixed point holds (stage3 == stage4); the
full boot suite is green. The infrastructure is sound and the linear-memory
primitive does **not** trip any GC↔linear coexistence problem under V8.

**The perf proof did not land.** `examples/sort-bench/sort_repeat_probe.tw`,
`native xs.sort()` on 1,000,000 ints, warm (run2/run3 after V8 tier-up), measured
on the same machine with self-host-rebuilt CLIs:

| build | warm `native xs.sort()` | cold (run1) |
|---|---|---|
| `main` (GC-array merge) | ~59–66 ms | ~125–129 ms |
| this branch (dense linear-memory scratch) | ~59–63 ms | ~124 ms |

The dense linear-memory sort lands at **parity** — inside run-to-run noise, no
clear win. The good news vs. the reverted GC-array `Scratch<T>` (`b915637`, which
regressed ~16%): linear memory does **not** regress, so the gather/scatter +
`i64.load`/`i64.store` path fully absorbs its own overhead. But parity is not the
win the go/no-go called for.

**Caveat — cost imposed on all programs:** `rt.buf` adds a 16-page (`1 MiB`)
linear memory to *every* emitted module (the memory section survives DCE even when
`sort_i64` is eliminated), so non-sorting programs pay a baseline footprint for a
feature that is, at best, parity for the one workload that uses it.

**What this does and does not decide.** The sort was picked as M1's go/no-go
because it is a cheap, self-contained bellwether — *"if linear memory can't win the
one workload we already know is read-bound, be skeptical."* It didn't win. So:

- **Settled:** a linear-memory scratch sort is *not* the lever for the typed
  `Vector<Int>` read wall. That lever remains typed `PVecI64` storage
  (`project_typed_vector_repr`), not the sort's scratch mechanism. Don't re-run this
  experiment expecting a win.
- **NOT settled (never tested here):** whether linear memory pays off for its
  *actual* motivations — the **M3 fast IR codec** (a flat byte artifact with O(1)
  decode, attacking the Phase G result-decode wall) and **M4 shared-memory worker
  buffers** (`SharedArrayBuffer`-backed byte payloads moved between compile Workers
  without structured-clone copies). The sort touches none of that. Judging the
  direction by the sort conflated a tactical proof with the strategic goal.

**What M1 leaves behind, by reusability:**

- *Foundational, reusable for M3/M4 regardless of the sort result:* the wider
  load/store IR (`i32/i64/f64.load/store`, `memory.size/grow`) and the program
  memory-section emit + linker wiring. These are prerequisites for any
  linear-memory use. They earn their keep independent of the sort.
- *Tactical, proved only coexistence:* the `rt.buf` bump allocator and the dense
  `sort_i64`. They de-risked "GC + a linear memory validate and run together under
  V8," but are not what an M3 codec or M4 shared transport would reuse.

**Caveat to fix before any merge:** `rt.buf` adds a 16-page (`1 MiB`) linear memory
to *every* emitted module (the memory section survives DCE even when `sort_i64` is
eliminated). A real adoption must make the program memory **conditional on actual
use**, so non-sorting / non-buffer programs pay nothing.

**Decision:** keep the branch unmerged for reference; do not adopt the dense sort.
The forward question is not "did the sort win" but "do we invest in linear memory as
the substrate for M3/M4" — see *Post-M1: the strategic case (untested)* below.

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
  attacking the Phase G result-decode wall. *Recommended go/no-go for the whole
  direction* (the sort was the wrong proxy — see Post-M1 below).
- **M4 — shared-memory parallelism (the original motivation).**
  `SharedArrayBuffer`-backed byte buffers across compile Workers, avoiding
  structured-clone copies in the parallel-compile transport. Unproven and unbuilt,
  but it is *why* linear memory was wanted; needs `shared` memory + atomics + SAB
  runtime backing on top of M1's IR (none of which M1 added). See Post-M1 below.

Each milestone is additive on proven infrastructure.

## Post-M1: the strategic case (untested)

M1's sort proxy failed, but the reasons to want linear memory are M3 and M4, and
neither was exercised. This section scopes what they actually require so the
direction can be judged on its real merits — and to be explicit that M1, despite
naming a memory, does **not** yet deliver the pieces these need.

### Two distinct targets (different urgency, shared substrate)

- **M3 — byte-level IR codec (single-process, nearer-term).** A flat
  linear-memory artifact format the compiler decodes with O(1) random indexing,
  replacing the O(n·log n) `Vector<Byte>` decode that sank Phase G. Needs only a
  *plain* (non-shared) memory + byte/word load/store — which M1's IR already
  provides. This is the lowest-risk way to actually test whether linear memory
  pays off, on a workload that genuinely indexes bytes (unlike the sort, which only
  used linear memory as scratch). **If we want one proof of the direction, this is
  the one to run, not the sort.**
- **M4 — shared-memory worker buffers (the original ask).** Move serialized
  payloads (a compiled Wasm module, an IR blob) between compile Workers through a
  `SharedArrayBuffer`-backed memory, avoiding the structured-clone copies the
  current `postMessage` remote-channel transport pays. Ties directly into the
  parallel-compile-workers roadmap, whose next step is already *"spawn_worker ABI +
  Wasm codec"* (`project_parallel_compile_workers`) — and that Wasm codec is
  exactly the flat byte payload M3 produces. So M3 is effectively a dependency of a
  *fast* M4.

### Hard constraint that gates both

**Only flat bytes live in linear memory — never GC objects.** Twinkle values
(`String`, records, `Vector`, closures) are Wasm-GC heap objects and cannot be
placed in or shared through a linear memory. So linear memory helps move/serialize
*byte representations* (IR, Wasm, packed columns), not live object graphs. This is
why a byte codec (M3) is the unlock: it's what turns GC artifacts into the flat
form that shared memory (M4) can transport.

### Concrete IR/runtime gaps M1 did NOT close

What M1 built (load/store + a plain per-instance memory) is necessary but not
sufficient for M4. To express and run *shared* memory we still need:

- **`MemoryDef` cannot express `shared`.** Today
  `MemoryDef = { name, min_pages, max_pages? }` and the limits encoder
  (`encode_memory_section_payload`, `wasm.tw`) emits only flag `0x00` (min) or
  `0x01` (min+max). A shared memory requires a `shared: Bool` field and limits flag
  `0x03` (shared **must** carry a max). Small, additive IR change — but real.
- **No atomics.** Cross-worker coordination needs `i32/i64.atomic.load/store`,
  `atomic.rmw.*`, and `memory.atomic.wait32/notify`. None exist in the `Instr`
  enum; each is an emit + WAT arm like M1's load/store work.
- **Runtime: SAB-backed shared memory.** `runtime.mjs`/`deno_main.mjs` must
  instantiate the memory from a `SharedArrayBuffer` and pass the *same*
  `WebAssembly.Memory` into every Worker instance (imported, not module-owned).
  Needs `crossOriginIsolated` (COOP/COEP headers in the browser; fine under Deno).
- **Conditional memory emission.** Per the M1 caveat, a shared (or any) program
  memory must be emitted only when actually used, not unconditionally on every
  module.

### Recommended next probe (if we pursue this)

Run **M3 (byte codec) as the honest go/no-go**, not another sort. Build a minimal
flat IR (or Wasm-module) codec over the M1 load/store IR, decode it with O(1)
indexing, and compare against the current `Vector<Byte>` decode on a realistic
artifact. That measures the property linear memory is actually good at (dense byte
indexing). Only if M3 wins is M4 (add `shared` + atomics + SAB transport) worth the
larger lift. The dense sort stays parked as proof-of-coexistence, nothing more.

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
