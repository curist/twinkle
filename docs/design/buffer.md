# Linear-Memory `Buffer`

`@std.buffer`'s `Buffer` is Twinkle's second mutate-in-place reference type,
alongside `Cell<T>`. It is a sandboxed, manually-managed region of Wasm **linear
memory** — an opt-in escape hatch for workloads where GC-managed `Vector<Byte>`
(or any persistent collection) is too slow.

This document is the design rationale and semantic model. The full API surface
(`buffer.new`, the byte-addressed accessors, and the `U8View`/`I64View`/`F64View`
element views) lives in [docs/API.md](../API.md#stdbuffer).

## Why linear memory at all

Twinkle is entirely Wasm-GC: every collection is a GC object, and the only mutable
flat storage (the PVec backing behind `rt.arr`) is unreachable from user code. The
cost of having no raw, in-place, unboxed buffer recurs across the codebase — the
typed-`Vector<Int>` read wall (persistent-trie random access is O(log₃₂ n)), byte
codecs and decoders over `Vector<Byte>`, and dense numeric arrays.

Linear memory provides what is otherwise missing: **O(1) indexed, unboxed,
cache-local mutable storage**. The win is decisive exactly where the pain is —
byte-indexing and codec/decode workloads (a LEB128 decoder over linear memory ran
~30× faster than the same decoder over `Vector<Byte>`, because each `Vector<Byte>`
index pays a trie walk *plus* a GC ref-cast/unbox per byte, versus a single near-native
byte load). It does **not** help comparator-bound work like sorting, which reads each
slot roughly once; that read wall is a separate lever (typed `PVecI64` storage).

## `Buffer` augments `Vector`, never replaces it

A hard constraint shapes the whole feature: linear memory can hold **only unboxed
primitives** (`Int`/i64, `Float`/f64, `Byte`/u8, `Bool`). GC references — `String`,
records, closures, nested collections — cannot live in it. This aligns cleanly with
where the pain is (primitive numeric arrays and byte buffers), so `Buffer` is an
augmentation: GC-element collections keep using the persistent `Vector`, and only
primitive-heavy hot paths reach for a `Buffer`.

## Semantic model

* **Mutate-in-place.** Unlike every ordinary Twinkle value, a `Buffer`'s bytes are
  mutated in place. Two names bound to the same `Buffer` observe each other's writes,
  exactly like two names bound to the same `Cell`. This is the deliberate, visible
  exception to Twinkle's value semantics (see [immutability.md](immutability.md)) —
  it is opt-in and namespaced under `@std.buffer`, never implicit.
* **Manual lifetime.** A `Buffer` is explicitly allocated (`buffer.new`) and
  explicitly released (`buf.free()`). There is no GC reclamation of the underlying
  region and no automatic drop. Correctness — calling `free` exactly once, not using
  after free, not double-freeing — is the **programmer's responsibility, like C**.
* **Views are windows, not owners.** `U8View`/`I64View`/`F64View` are O(1) handles
  over a slice of a buffer. They share the backing region; they do not allocate, copy,
  or own it, and freeing the buffer invalidates every view over it.

## Safety model: sandboxed, not memory-safe

`Buffer` is low-level by design. Accessors and views are **unchecked** against the
logical length — only the whole-linear-memory bound is enforced by Wasm itself. The
consequence:

* The worst case of a misuse (out-of-range index, use-after-free, double-free) is
  **corrupting another buffer's bytes or trapping** — never an escape from the Wasm
  sandbox, and never a violation of the host or of GC-managed values, which live in a
  separate heap the buffer cannot address.
* So the safety floor is the Wasm sandbox, not language-level memory safety. Treat a
  `Buffer` the way you would treat a raw pointer in C, kept inside a small, audited
  module.

## Relationship to the two mutable reference types

| | `Cell<T>` | `Buffer` (`@std.buffer`) |
|---|---|---|
| Stores | one GC value of any type `T` | a flat region of unboxed primitives |
| Lifetime | GC-managed | manual (`new` / `free`) |
| Access | `get`/`set`/`update` | byte accessors + typed element views |
| Purpose | shared mutable state | dense unboxed storage / codecs |

Both are the *only* mutate-in-place reference types, and both are opt-in. Everything
else in Twinkle remains immutable with value semantics.
