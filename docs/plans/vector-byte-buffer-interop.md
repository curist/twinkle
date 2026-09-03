# `Vector<Byte>` ↔ `Buffer` Interop — Placeholder

> **Status: forward-work stub, now unblocked.** This is not an executable plan; it
> records *what changes once `Vector<Byte>` is unboxed* and the one honest ceiling
> on "fast conversion," so the work is scoped correctly when picked up. **The hard
> prerequisite `PVecByte`** (the typed `array i8` family —
> [`mutvec-later-slices.md`](mutvec-later-slices.md) Phase 4) **has landed**, so
> the two narrow follow-ups below (typed-leaf conversion path, and re-benching the
> codec go/no-go against the unboxed baseline) are now actionable rather than
> blocked. The AWFY suite already reflects the upstream half of this: on Sieve the
> unboxed persistent path beat `@std.buffer`, so the Buffer variant was dropped
> there; Buffer's durable value narrows to FFI / shared-memory / hottest-loop
> codecs, exactly as scoped below.

## Why this exists

`@std.buffer` (archived design: [`archive/buffer-linear-memory.md`](archive/buffer-linear-memory.md))
and the Byte typed family draw a *conceptual* boundary — "Byte complements
`@std.buffer`, does not replace it." That boundary is already documented. What is
**not** yet captured is the sharper question: once `Vector<Byte>` is stored
**unboxed** (`PVecByte` = `array i8`, native `array.get_u`), which part of
Buffer's job actually erodes, and what does a *fast* conversion between the two
representations realistically look like?

## What PVecByte changes for Buffer

The M3 probe measured ~30× (LEB128 decode) for linear memory over a boxed
`Vector<Byte>`. The archived doc explains the size: each boxed byte index pays a
persistent-trie walk **plus** a GC ref-cast/unbox per byte. `PVecByte` deletes
the ref-cast/unbox half. So the gap collapses toward the *purely structural*
advantage — a log₃₂ n leaf-boundary walk vs. a single linear-memory load —
single-digit×, not 30×.

**Measured 2026-09-03, now that `PVecByte` has landed** (200k-byte scan × 40 vs
`@std.buffer`; read path confirmed in WAT). The prediction is directionally right
but the collapse is **not** primarily `PVecByte`'s doing, and it only reaches
part of the byte-read surface:

| `Vector<Byte>` read site | compiles to | × vs Buffer |
|---|---|---|
| non-escaping local (`collect`, read in place) | `rt_arr__get_byte` on `PVecByte` (unboxed) | **~4.0×** |
| passed to a decode function (realistic codec) | `rt_arr__get` (boxed `anyref`) | **~8.2×** |
| `@std.buffer` | `get_u8` | 1× |

Two corrections this forces:

1. **The 30×→single-digit× collapse is mostly general improvement, not
   `PVecByte`.** Even the *still-boxed* codec path is already ~8×, not 30×.
   `PVecByte`'s specific contribution is the further ~2× (8×→4×) it buys by
   deleting the per-byte unbox — real, but a smaller slice than "deletes the
   ref-cast/unbox half → collapse" implies.
2. **The unboxed path only reaches typed storage sites.** A `Vector<Byte>` that
   crosses a function boundary (exactly what a decode/parse routine does) reads
   through the boxed `rt_arr__get` — there is no typed-parameter ABI (the same
   wall the vector/sort read-wall hit). So "once `Vector<Byte>` is stored
   unboxed" is load-bearing: true for non-escaping locals, **false across call
   boundaries**. A codec only gets the ~4× floor if it is fully inlined/local.

**What stays uniquely Buffer's, even after PVecByte:**

- **Addressability** — GC arrays are not addressable, so FFI, `SharedArrayBuffer`,
  and M4 cross-Worker transport can *only* be Buffer. This never moves.
- **Truly contiguous layout** — no 32-element leaf seams, for the hottest codec
  inner loops where even the leaf-boundary branch costs.
- **First-class mutate-in-place at arbitrary offsets** — MutVec covers only owned,
  locally-born regions; Buffer is a freely-aliased mutable handle.

So the ground that shifts is Buffer's **general-purpose in-language byte** use
case — casual parsing and codecs where single-digit× is not decisive. That is
exactly the ground `v[i]` / `v[a..b]`-sugared unboxed `Vector<Byte>` is meant to
take. Buffer narrows toward FFI / shared-memory / absolute-hottest-loop territory.

## The hard constraint (write it down so nobody chases it)

**There is no Wasm-GC instruction that bulk-copies between a GC array and linear
memory** — `array.copy` is array→array only. Therefore `from_bytes` / `to_bytes`
are *permanently* O(n) element loops. There is no memcpy bridge, and there never
will be under the current Wasm-GC surface. "Fast conversion" can only mean *cheap
per-byte loop*, never *constant-time transfer*.

## The narrow future work this actually names

1. **Typed-leaf conversion path.** Once `PVecByte` exists, rewrite
   `buffer.from_bytes` / `buf.to_bytes` to walk the typed `i8` leaves — each
   iteration a near-native `array.get_u` / `array.set` instead of a boxed
   trie+unbox. Still O(n), but a much lower constant. (Today's copy loops iterate
   the boxed `anyref` path.)
2. **Re-bench the go/no-go.** The original M3 probe answered "is Buffer worth it
   for codecs?" *against a boxed `Vector<Byte>` baseline that will no longer
   exist.* Re-run the codec A/B against the **unboxed** `PVecByte` baseline to
   confirm Buffer still earns its complexity for byte-codec workloads, and to
   size the residual gap that justifies keeping it.

## Non-goals

- No new IR / no attempt at an array↔memory bulk-copy primitive (does not exist).
- No change to the Buffer surface or the `Vector<Byte>` / `Buffer` boundary as
  documented — this only makes the *crossing* between them cheap and re-validates
  the split with real numbers.
- (Historical) was gated on `PVecByte` (mutvec-later-slices Phase 4 typed-storage
  prerequisite); that has since landed, so this gate is cleared.
