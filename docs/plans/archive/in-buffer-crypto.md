# In-Buffer Crypto (`Buffer` word accessors + `crypto.*_buf`)

Status: **DONE.** A focused follow-on to the shipped linear-memory
`Buffer` ([buffer-linear-memory.md](buffer-linear-memory.md)). Boot-only.

## Goal

Make `@std.crypto` competitive in `examples/crypto-bench/` by hashing data that already
lives in a `Buffer`, attacking **both** dominant costs of the current pure-Twinkle hashes
— the per-byte `Vector<Byte>` reads **and** the functional schedule-record churn — without
touching the existing `_bytes` paths or the conditional linear-memory-emission property.

Success is **a measured win** on the bench's 4 KiB hash cases, not just a new API.

## Why (the measured motivation)

A faithful MD5 spike (3 runs, 64 KiB input, identical digests) established the shape:

| path | what | ms/op |
|---|---|---|
| A | hash over `Vector<Byte>` | 1.22 |
| B | copy `Vector`→`Buffer` then hash, per call | 1.24 (wash) |
| C | hash from a **pre-loaded** `Buffer` | 0.76 (~1.6×) |
| D | copy-in alone (`from_bytes`) | 0.48 |

The copy (D) negates the in-buffer speedup (C) for a *single-shot* `Vector` input, so
migrating the `_bytes` API behind a `Buffer` is a wash. The win is real only when the
bytes already live in a `Buffer` — i.e. **path C**. The crypto-bench's 4 KiB input is
*reused* across 501 iterations, so converting it to a `Buffer` once amortizes the copy and
every iteration runs at path-C speed.

Reading the actual `@std.crypto` code surfaced a second, source-independent cost: the
shared schedule (`crypto.schedule`) is a **functional 16-field record**, and the block loop
does `w = sched.set(w, j, ...)` — **each `set` rebuilds the whole 16-field record**. MD5
does ~16 rebuilds per 64-byte block (it reads the 16 words once and only permutes their
order). **SHA-1 and SHA-256 both expand the schedule across the rounds**, doing a
`sched.set` per expansion step: SHA-1 updates its 16-word ring for rounds 16..79
(`sha1.tw:118-125`) and SHA-256 expands to 64 words for rounds 16..63 — so **~80 / ~64
record rebuilds per block** (thousands of allocations for one 4 KiB digest). This GC churn
is independent of the byte source, so it must be attacked too or the read win is diluted.

So the in-buffer path uses **two levers**: word-load the message (Lever A) **and** store the
schedule in an in-place `Buffer` scratch (Lever B).

## Non-goals

- `hmac_*_buf`; `get_u32_be`/`set_u32_be` on `Buffer`; explicit offset/length hashing of a
  sub-range.
- File-I/O-into-buffer (`read_file_buf`, `crypto.digest_file`, exporting `rt.buf` memory) —
  a separate later milestone; it does **not** affect this bench (which reads no files).
- Changing, replacing, or optimizing the existing `_bytes` paths or the string/small-input
  hashing.
- Any `src/` (Rust stage0) change — `boot/main.tw` uses none of this.

## Design

### 1. `Buffer` word accessors

Add native 32-bit little-endian word load/store to `@std.buffer`, wired through `rt.buf` +
internal `__buf_*` (the same 3-site recipe as `get_i64`):

```tw
buf.get_u32(off: Int) Int      // unsigned 32-bit LE load, zero-extended to Int (0 .. 2^32)
buf.set_u32(off: Int, v: Int)  // 32-bit LE store (low 32 bits of v)
```

- Byte-addressed (`off` is a byte offset), little-endian, **unchecked** against `len` —
  consistent with `get_u8`/`get_i64`/`get_f64`.
- `get_u32` is an unsigned load (zero-extended), so a word with the high bit set is a
  positive `Int` in `0 .. 2^32`. Implementation: reuse the existing `I32Load` +
  `I64ExtendI32U` route (no new IR variant); only add a dedicated `i64.load32_u` `Instr`
  if a measured need appears.
- `Buffer` stays endianness-neutral: these are the Wasm-native LE ops. Crypto owns its own
  byte order (§3). No `_be` accessor on `Buffer`.

### 2. `crypto.{md5,sha1,sha256}_buf(buf: Buffer) Digest`

New `pub fn`s in `crypto.tw` (the umbrella) delegating to a `digest_buf(buf) Vector<Byte>`
added in each submodule (`crypto/{md5,sha1,sha256}.tw`) **alongside the untouched
`digest_bytes`**. Contract: hashes the **entire buffer** — `buf.len()` bytes. Returns the
same `Digest`, so `.hex()`/`.base64()`/`.to_bytes()`/`.to_string()` work unchanged.

Each `digest_buf`:

- **Message length** = `buf.len()`. Padding for `pos ≥ len` (the `0x80` terminator, zero
  fill, and the 64-bit length) is **computed**, not stored — the buffer holds only the raw
  message, no over-allocation.
- **Lever B — schedule scratch.** Allocate a small scratch `Buffer`: 64 B for MD5's 16
  words and SHA-1's 16-word ring; 256 B for SHA-256's 64-word schedule. Replace the
  functional `Schedule` record with **in-place** `set_u32`/`get_u32` by computed index —
  zero GC churn:
  - MD5 fills the 16 words once, then reads them permuted via `get_u32(word_index(i)*4)`.
  - SHA-1 fills 16 words, then **updates the ring in place** for rounds 16..79
    (`set_u32((i & 15)*4, rotl(...))`), reading neighbours with `get_u32`.
  - SHA-256 fills 16 words, then **expands in place** to 64 words for rounds 16..63.

  Free the scratch before returning.
- **Lever A — message reads.** Fill each block's 16 words from the message. The precise
  rule (correctness-critical): use the fast `get_u32(base)` **only when `base + 4 <= len`**
  — the entire word lies inside the real message. **Every other word must be synthesized**
  per byte (real bytes via `get_u8` for `pos < len`, then the `0x80` terminator at
  `pos == len`, zero fill, and the 64-bit length) — the existing `padded_byte` logic over
  the buffer. This is not merely a "trailing word or two": a block-aligned input (e.g. the
  4 KiB bench, an exact multiple of 64) appends an **entire synthetic padding block** whose
  every word is synthesized. Reading `get_u32` past `len` is forbidden — it would read the
  uninitialized, 8-byte-rounded allocation tail. The fast path still covers ~all of the
  real message a word at a time; only the boundary/padding words take the slow path.
- **Shared math.** The pure round helpers (`u32`, `rotl32`, `not32`, `s`, `k`, `round_f`,
  `word_index`, SHA `sigma`s) are reused from the existing submodule; only the ~20–30-line
  block/schedule loop is duplicated (no closure indirection in the hot loop).
- Output via the existing `write_*_word` into a `Vector<Byte>`.

Because only `digest_buf` (reachable solely via `crypto.*_buf`) allocates buffers, the
**conditional-emission property holds**: programs that never call `_buf` emit no linear
memory. Programs that do already provided a `Buffer` message, so nothing new is pulled in.

### 3. Endianness

`Buffer` exposes only LE word access. MD5 reads little-endian message words → `get_u32`
directly. SHA-1/SHA-256 read big-endian → a small `bswap32(x)` helper in the submodule
(`get_u32` + shifts/masks). The schedule scratch stores already-parsed `Int` words, so its
storage byte order is irrelevant as long as `set_u32`/`get_u32` agree (they do).

### 4. Scratch lifetime

The scratch `Buffer` is allocated and freed inside each `digest_buf` call; callers never
see it. Across the bench's 501 iterations the free-list reuses the same block, so alloc/
free is negligible.

### 5. Bench changes (`examples/crypto-bench/twinkle/main.tw`)

- Build `large_buf := buffer.from_bytes(large)` once (copy amortized over 501 iters).
- **Warm up** the `_buf` paths after building `large_buf` (one `crypto.*_buf(large_buf)`
  call each), mirroring the existing warm-up block, so timing is comparable and the scratch
  free-list slot is primed before the timed loops.
- Add `md5_4k_buf`, `sha1_4k_buf`, `sha256_4k_buf` cases calling `crypto.*_buf(large_buf)`.
- **Keep** the existing `_bytes` 4 KiB cases for the side-by-side delta.
- Free `large_buf` at the end.

The `_buf` cases are the apples-to-apples comparison against the native node/python/go
baselines (which hash a native buffer); the `_bytes` cases show Twinkle's naive path.

### 6. Testing

- `boot/tests/suites/stdlib_buffer_suite.tw`: `get_u32`/`set_u32` round-trip and LE byte
  layout (e.g. `set_u32(0, 0x04030201)` ⇒ `get_u8(0)==1 … get_u8(3)==4`; high-bit word
  round-trips as a positive `Int`).
- Crypto cross-equivalence (new or existing crypto suite): for each hash, **allocate a
  buffer, hash it, free it**, and assert it matches the `_bytes` digest — i.e.
  `b := buffer.from_bytes(v); d := crypto.X_buf(b); b.free()` then `d == crypto.X_bytes(v)`.
  (`digest_buf` must **not** free a caller-owned buffer.) Cover edge lengths: empty, `< 4`,
  exactly 4, the 55/56/64-byte padding boundaries, a non-4-aligned length, and 4 KiB.

### 7. Success criterion

Run `examples/crypto-bench/run.sh` (or just the Twinkle bench). `md5_4k_buf` /
`sha1_4k_buf` / `sha256_4k_buf` should be **meaningfully faster** than their `_bytes`
counterparts and narrow the gap to the native baselines. If the 4 KiB win is small
(per-block setup still dominates even after Lever B), record it and consider adding a larger
bench input — but no algorithm changes beyond the two levers.

**Result (measured, 4 KiB, median µs/op):**

| case | `_bytes` | `_buf` | speedup |
|---|---|---|---|
| md5_4k | ~68.8 | ~46.1 | ~1.5× |
| sha1_4k | ~66.0 | ~17.9 | ~3.7× |
| sha256_4k | ~80.8 | ~57.5 | ~1.4× |

SHA-1 gains the most: the in-place 16-word ring (Lever B) eliminates its per-round
schedule churn outright. MD5 and SHA-256 gains are bounded by their round-function cost —
MD5 never expands its schedule (so Lever B saves little; the win is mostly Lever A's word
loads), and SHA-256's compression dominates the per-block time. The wins are real without a
larger bench input.

**Implementation note (not in the original design):** SHA-1 and SHA-256 encode the trailing
64-bit length field **big-endian**; the in-buffer `buf_padded_byte` must therefore use
`shift = (7 - (pos - len_start)) * 8`, not MD5's little-endian `(pos - len_start) * 8`.
Empty input hides the bug (zero length is endian-agnostic).

## File touch map (for the plan)

- `boot/compiler/codegen/runtime/buf.tw` — `buf_load_u32` (`I32Load` + `I64ExtendI32U` →
  i64) / `buf_store_u32` (`I32Store`, value narrowed at the ABI like `store_u8`) funcs +
  exports. No new `Instr` variant needed (both already exist in `wasm_ir.tw`).
- `boot/compiler/builtins.tw` — `builtin_abi` + `builtin_specs` for the two ops.
- `boot/compiler/base_env.tw` — `__buf_load_u32`/`__buf_store_u32` in
  `add_internal_host_builtins`.
- `boot/stdlib/buffer.tw` — `pub fn get_u32`/`set_u32`.
- `boot/stdlib/crypto.tw` — `pub fn md5_buf`/`sha1_buf`/`sha256_buf`.
- `boot/stdlib/crypto/{md5,sha1,sha256}.tw` — `digest_buf` + `bswap32` (SHA only).
- `boot/tests/suites/stdlib_buffer_suite.tw`, crypto suite — tests.
- `examples/crypto-bench/twinkle/main.tw` — `_buf` bench cases.
- `docs/API.md` — `Buffer.get_u32`/`set_u32`, `crypto.*_buf`.
