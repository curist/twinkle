# `Vector<Byte>` ↔ `Buffer` Interop — Placeholder

> **Status: placeholder / deferred.** This is a forward-work stub, not an
> executable plan. It records *what changes once `Vector<Byte>` is unboxed* and
> the one honest ceiling on "fast conversion," so the work is scoped correctly
> when it is picked up. **Hard prerequisite: `PVecByte`** (the typed `array i8`
> family — see [`mutvec-later-slices.md`](mutvec-later-slices.md) Phase 4). None
> of the wins below exist until `Vector<Byte>` is stored unboxed.

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
- Not startable before `PVecByte` (mutvec-later-slices Phase 4 typed-storage
  prerequisite) lands.
