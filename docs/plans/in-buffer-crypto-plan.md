# In-Buffer Crypto Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `Buffer.get_u32`/`set_u32` word accessors and `crypto.{md5,sha1,sha256}_buf(Buffer)` hash variants that hash data already in linear memory (word-loaded message + an in-place `Buffer` schedule scratch), to win the 4 KiB crypto-bench cases.

**Architecture:** A native 32-bit LE load/store is added to `rt.buf` and surfaced as `Buffer.get_u32`/`set_u32`. Each `crypto/*.tw` submodule gains a `digest_buf(buf)` beside the untouched `digest_bytes`: it reads the message a word at a time (falling back to per-byte synthesis at the padding boundary) and stores the round schedule in a small scratch `Buffer` mutated in place, eliminating the functional-record GC churn. The `_bytes` paths and conditional memory emission are untouched.

**Tech Stack:** Boot compiler only (`boot/`), Twinkle (`.tw`), hand-written Wasm IR (`.Instr` DSL). No `src/`/stage0 changes — `boot/main.tw` uses none of this; the self-host fixed point + boot suite are the gates.

**Design doc:** `docs/plans/in-buffer-crypto.md` — read it first.

---

## Orientation (read once before Task 1)

**Key files:**
- `boot/compiler/codegen/runtime/buf.tw` — `rt.buf` runtime module. Model new funcs on the existing `buf_load_i64`/`buf_store_i64` (byte address = `base + off`, memarg `(0,0)`).
- `boot/compiler/builtins.tw` — `builtin_abi(name)` (~line 117, declares wasm types) and `builtin_specs()` (~line 445, `rt(...)` entries). The existing `buf_*` arms are the template.
- `boot/compiler/base_env.tw` — `add_internal_host_builtins()` (~line 436) holds the internal `__buf_*` signatures. `lower_core/context.tw:44` already aliases any `buf_*` runtime func to `__buf_*` (no change needed).
- `boot/stdlib/buffer.tw` — `@std.buffer`; add `get_u32`/`set_u32` beside `get_i64`/`set_i64`.
- `boot/stdlib/crypto.tw` — umbrella; `md5_bytes` is `.{ bytes: md5_impl.digest_bytes(input) }`. Add `*_buf` siblings.
- `boot/stdlib/crypto/{md5,sha1,sha256}.tw` — the hash impls. Each has `padded_byte`, `read_le_word`/`read_be_word`, `digest_bytes`, and shared helpers (`u32`, `rotl32`, `not32`, `s`, `k`, `round_f`, `word_index` for md5; the `sigma`s for sha) you will REUSE.
- `boot/tests/suites/stdlib_buffer_suite.tw` and `boot/tests/suites/stdlib_crypto_suite.tw` — add tests here (both already registered in `boot/tests/main.tw`).
- `examples/crypto-bench/twinkle/main.tw` — the bench.

**Build / verify (the trap — stale CLI uses old codegen/stdlib):**
```bash
python3 tools/generate_core_lib.py            # regen embedded stdlib (gitignored; needed for ANY boot/stdlib or codegen change)
cargo build --release                          # stage0
./target/release/twk build boot/main.tw -o /tmp/stage1.wasm   # stage0 -> boot v1 (reflects your changes)
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs run <prog-or-boot/tests/main.tw>
```
The bundled `target/twk` is STALE for `run`/`build` until `make bundle-cli`; it is fine for `fmt`/`lint`. Full gate (Task 6): `make bundle-cli` ("Fixed point reached") + `make boot-test`.

**Commit conventions:** one commit per task; short imperative subject + what/why/how body; no count metrics. Follow `CLAUDE.md`/`AGENTS.md` trailer guidance — **do not add a `Co-Authored-By` trailer**. Run `target/twk fmt <file>` + `target/twk lint <entry>` on edited `.tw` before committing.

---

## Task 1: `Buffer.get_u32` / `set_u32`

Native 32-bit little-endian word load/store. Foundation for all `_buf` hashes.

**Files:**
- Modify: `boot/compiler/codegen/runtime/buf.tw`
- Modify: `boot/compiler/builtins.tw`
- Modify: `boot/compiler/base_env.tw`
- Modify: `boot/stdlib/buffer.tw`
- Test: `boot/tests/suites/stdlib_buffer_suite.tw`

- [ ] **Step 1: Write the failing test**

In `boot/tests/suites/stdlib_buffer_suite.tw`, add a test to the suite chain (after an existing `.test(...)`, mind commas):
```tw
    .test(
      "u32 word load/store, little-endian, unsigned",
      fn() {
        b := buffer.new(16)
        b.set_u32(0, 0x04030201)
        try assert.equal(b.get_u32(0), 0x04030201)
        // little-endian byte layout
        try assert.equal(b.get_u8(0).to_int(), 1)
        try assert.equal(b.get_u8(1).to_int(), 2)
        try assert.equal(b.get_u8(2).to_int(), 3)
        try assert.equal(b.get_u8(3).to_int(), 4)
        // high-bit word round-trips as a positive Int (unsigned, zero-extended)
        b.set_u32(8, 0x80000000)
        try assert.equal(b.get_u32(8), 2147483648)
        b.free()
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run it; verify it fails**

Run:
```bash
python3 tools/generate_core_lib.py && cargo build --release && \
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm && \
  BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
    tools/js_runtime/deno_main.mjs run boot/tests/main.tw 2>&1 | grep -iE 'get_u32|set_u32|undefined|fail' | head
```
Expected: a typecheck error — `get_u32`/`set_u32` undefined.

- [ ] **Step 3: Add the rt.buf runtime funcs**

In `boot/compiler/codegen/runtime/buf.tw`, add two funcs (model on `load_i64_fn`/`store_i64_fn`) and register them in the `funcs:` list and as `ExportDef`s:
```tw
// buf_load_u32(base, off) -> i64 : unsigned 32-bit LE load, zero-extended.
fn load_u32_fn() FuncDef {
  .{
    name: "buf_load_u32",
    params: [.I32, .I32],
    results: [.I64],
    locals: [],
    body: [.LocalGet(0), .LocalGet(1), .I32Add, .I32Load(0, 0), .I64ExtendI32U],
  }
}

// buf_store_u32(base, off, v) — stores the low 32 bits of v at base+off (LE).
fn store_u32_fn() FuncDef {
  .{
    name: "buf_store_u32",
    params: [.I32, .I32, .I32],
    results: [],
    locals: [],
    body: [.LocalGet(0), .LocalGet(1), .I32Add, .LocalGet(2), .I32Store(0, 0)],
  }
}
```
Add `load_u32_fn(), store_u32_fn()` to the `funcs:` list and add:
```tw
      ExportDef.{ wasm_name: "buf_load_u32", func_sym: "buf_load_u32" },
      ExportDef.{ wasm_name: "buf_store_u32", func_sym: "buf_store_u32" },
```
to the `exports:` list.

- [ ] **Step 4: Wire the ABI + rt specs**

In `boot/compiler/builtins.tw`, in `builtin_abi` (beside the other `buf_*` arms):
```tw
    "buf_load_u32" => abi([.I32, .I32], [.I64]),
    "buf_store_u32" => abi([.I32, .I32, .I32], []),
```
In `builtin_specs()` (beside the other `rt("buf_*", ...)` entries):
```tw
    rt("buf_load_u32", "rt.buf", "buf_load_u32", .None),
    rt("buf_store_u32", "rt.buf", "buf_store_u32", .None),
```

- [ ] **Step 5: Register the internal builtins**

In `boot/compiler/base_env.tw`, in `add_internal_host_builtins()` (beside the other `__buf_*` chain):
```tw
    .add_function(builtin_sig("__buf_load_u32", [], ["base", "i"], [.Int, .Int], .Some(.Int)))
    .add_function(builtin_sig("__buf_store_u32", [], ["base", "i", "v"], [.Int, .Int, .Int], .Some(.Void)))
```

- [ ] **Step 6: Add the `@std.buffer` methods**

In `boot/stdlib/buffer.tw`, beside `get_i64`/`set_i64`:
```tw
/// 32-bit unsigned little-endian read at byte offset `off` (unaligned ok).
pub fn get_u32(b: Buffer, off: Int) Int {
  __buf_load_u32(b.ptr, off)
}

/// 32-bit little-endian write (low 32 bits of `v`) at byte offset `off`.
pub fn set_u32(b: Buffer, off: Int, v: Int) {
  __buf_store_u32(b.ptr, off, v)
}
```

- [ ] **Step 7: Run the test; verify it passes**

Run the Step 2 command again, then:
```bash
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs run boot/tests/main.tw 2>&1 | grep -iE 'buffer|fail|Ran [0-9]+ tests'
```
Expected: "stdlib buffer" passes; final line "Ran N tests: N passed".

- [ ] **Step 8: Format, lint, commit**

```bash
target/twk fmt boot/compiler/codegen/runtime/buf.tw boot/compiler/builtins.tw \
  boot/compiler/base_env.tw boot/stdlib/buffer.tw boot/tests/suites/stdlib_buffer_suite.tw
git add -A
git commit -m "buffer: add get_u32/set_u32 native 32-bit LE word accessors

Foundation for in-buffer crypto: a single i32 load/store instead of four
byte ops. get_u32 zero-extends to a positive Int; value narrowed at the
ABI like store_u8."
```

---

## Task 2: MD5 `digest_buf` + `crypto.md5_buf` (the template)

MD5 is the simplest: little-endian, no schedule expansion (the 16 words are read in a permuted order). This establishes the pattern reused by SHA.

**Files:**
- Modify: `boot/stdlib/crypto/md5.tw`
- Modify: `boot/stdlib/crypto.tw`
- Test: `boot/tests/suites/stdlib_crypto_suite.tw`

- [ ] **Step 1: Write the failing cross-equivalence test**

In `boot/tests/suites/stdlib_crypto_suite.tw`, add a `use @std.buffer` at the top (beside the existing imports) and a test. Add this helper near the top of the file (after imports) if no equivalent exists:
```tw
fn make_bytes(n: Int) Vector<Byte> {
  out: Vector<Byte> = []
  i := 0

  for i < n {
    out = .append(Byte.from_int(i * 31 + 7 & 0xff).unwrap())
    i = i + 1
  }

  out
}

fn md5_equiv(v: Vector<Byte>) Result<Void, String> {
  b := buffer.from_bytes(v)
  got := crypto.md5_buf(b).to_bytes()
  b.free()
  assert.equal(got, crypto.md5_bytes(v).to_bytes())
}
```
And the test (in the suite chain):
```tw
    .test(
      "md5_buf matches md5_bytes across edge lengths",
      fn() {
        try md5_equiv([])
        try md5_equiv(make_bytes(3))
        try md5_equiv(make_bytes(4))
        try md5_equiv(make_bytes(55))
        try md5_equiv(make_bytes(56))
        try md5_equiv(make_bytes(64))
        try md5_equiv(make_bytes(100))
        try md5_equiv(make_bytes(4096))
        .Ok({})
      },
    )
```
(If `stdlib_crypto_suite.tw` does not already `use @std.crypto`, add it. Confirm `assert.equal` returns `Result<Void,String>` so `try` works — it does in this suite style.)

- [ ] **Step 2: Run it; verify it fails**

Run:
```bash
python3 tools/generate_core_lib.py && cargo build --release && \
  ./target/release/twk build boot/main.tw -o /tmp/stage1.wasm && \
  BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env \
    tools/js_runtime/deno_main.mjs run boot/tests/main.tw 2>&1 | grep -iE 'md5_buf|undefined|fail' | head
```
Expected: `crypto.md5_buf` undefined.

- [ ] **Step 3: Add `digest_buf` to `md5.tw`**

In `boot/stdlib/crypto/md5.tw`, add the imports at the top (beside the existing ones):
```tw
use @std.buffer
use @std.buffer.{Buffer}
```
Then add these functions (they REUSE the existing `u32`, `rotl32`, `round_f`, `word_index`, `s`, `k`, `padded_len`, `write_le_word`):
```tw
// Byte at message position `pos`, synthesizing MD5 padding past `len`.
fn buf_padded_byte(buf: Buffer, len: Int, msg_len: Int, pos: Int) Int {
  if pos < len {
    return buf.get_u8(pos).to_int()
  }
  if pos == len {
    return 0x80
  }

  len_start := msg_len - 8
  if pos < len_start {
    return 0
  }

  bit_len := len * 8
  shift := (pos - len_start) * 8
  bit_len >> shift & 0xff
}

// 32-bit LE message word at byte position `base`. Fast single load only when
// the whole word is inside the real message; otherwise synthesize per byte.
fn buf_le_word(buf: Buffer, len: Int, msg_len: Int, base: Int) Int {
  if base + 4 <= len {
    return buf.get_u32(base)
  }

  b0 := buf_padded_byte(buf, len, msg_len, base)
  b1 := buf_padded_byte(buf, len, msg_len, base + 1)
  b2 := buf_padded_byte(buf, len, msg_len, base + 2)
  b3 := buf_padded_byte(buf, len, msg_len, base + 3)
  u32(b0 | b1 << 8 | b2 << 16 | b3 << 24)
}

/// Raw 16-byte MD5 digest of the entire buffer (`buf.len()` bytes).
pub fn digest_buf(buf: Buffer) Vector<Byte> {
  len := buf.len()
  msg_len := padded_len(len)
  scratch := buffer.new(64)

  a0 := 0x67452301
  b0 := 0xefcdab89
  c0 := 0x98badcfe
  d0 := 0x10325476

  offset := 0
  for offset < msg_len {
    j := 0
    for j < 16 {
      scratch.set_u32(j * 4, buf_le_word(buf, len, msg_len, offset + j * 4))
      j = j + 1
    }

    a := a0
    b := b0
    c := c0
    d := d0

    i := 0
    for i < 64 {
      f := round_f(i, b, c, d)
      g := word_index(i)
      next := d
      d = c
      c = b
      b = u32(b + rotl32(u32(a + f + k(i) + scratch.get_u32(g * 4)), s(i)))
      a = next
      i = i + 1
    }

    a0 = u32(a0 + a)
    b0 = u32(b0 + b)
    c0 = u32(c0 + c)
    d0 = u32(d0 + d)

    offset = offset + 64
  }

  scratch.free()

  out: Vector<Byte> = []
  out = write_le_word(out, a0)
  out = write_le_word(out, b0)
  out = write_le_word(out, c0)
  out = write_le_word(out, d0)
  out
}
```

- [ ] **Step 4: Add `crypto.md5_buf`**

In `boot/stdlib/crypto.tw`, add `use @std.buffer.{Buffer}` at the top (beside the impl imports), and beside `md5_bytes`:
```tw
/// MD5 digest of the bytes already in a `Buffer` (hashes `buf.len()` bytes).
pub fn md5_buf(input: Buffer) Digest {
  .{ bytes: md5_impl.digest_buf(input) }
}
```

- [ ] **Step 5: Run the test; verify it passes**

Re-run the Step 2 command, then the suite run; expected: "stdlib crypto" passes (all `md5_equiv` cases green).

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/stdlib/crypto/md5.tw boot/stdlib/crypto.tw boot/tests/suites/stdlib_crypto_suite.tw
target/twk lint boot/stdlib/crypto.tw
git add -A
git commit -m "crypto: md5_buf — hash bytes already in a Buffer

digest_buf word-loads the message (per-byte synthesis only at the padding
boundary) and stores the round schedule in an in-place Buffer scratch,
dropping the functional 16-field schedule-record churn. md5_bytes
untouched."
```

---

## Task 3: SHA-1 `digest_buf` + `crypto.sha1_buf`

Differs from MD5: big-endian message words (read LE word + byte-swap), and the 16-word ring is **expanded in place** for rounds 16..79.

**Files:**
- Modify: `boot/stdlib/crypto/sha1.tw`
- Modify: `boot/stdlib/crypto.tw`
- Test: `boot/tests/suites/stdlib_crypto_suite.tw`

- [ ] **Step 1: Write the failing test** (in `stdlib_crypto_suite.tw`)
```tw
fn sha1_equiv(v: Vector<Byte>) Result<Void, String> {
  b := buffer.from_bytes(v)
  got := crypto.sha1_buf(b).to_bytes()
  b.free()
  assert.equal(got, crypto.sha1_bytes(v).to_bytes())
}
```
```tw
    .test(
      "sha1_buf matches sha1_bytes across edge lengths",
      fn() {
        try sha1_equiv([])
        try sha1_equiv(make_bytes(3))
        try sha1_equiv(make_bytes(4))
        try sha1_equiv(make_bytes(55))
        try sha1_equiv(make_bytes(56))
        try sha1_equiv(make_bytes(64))
        try sha1_equiv(make_bytes(100))
        try sha1_equiv(make_bytes(4096))
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run it; verify it fails** — same run command as Task 2 Step 2, grepping `sha1_buf`. Expected: undefined.

- [ ] **Step 3: Add `digest_buf` to `sha1.tw`**

Add imports (`use @std.buffer` + `use @std.buffer.{Buffer}`) and these functions (REUSE existing `u32`, `rotl32`, `not32`, `padded_len`, `write_be_word`):
```tw
fn bswap32(x: Int) Int {
  u32((x & 0xff) << 24 | (x >> 8 & 0xff) << 16 | (x >> 16 & 0xff) << 8 | (x >> 24 & 0xff))
}

fn buf_padded_byte(buf: Buffer, len: Int, msg_len: Int, pos: Int) Int {
  if pos < len {
    return buf.get_u8(pos).to_int()
  }
  if pos == len {
    return 0x80
  }

  len_start := msg_len - 8
  if pos < len_start {
    return 0
  }

  bit_len := len * 8
  shift := (pos - len_start) * 8
  bit_len >> shift & 0xff
}

// 32-bit BE message word at byte position `base`. Fast load + byte-swap only
// when the whole word is inside the real message; else synthesize per byte.
fn buf_be_word(buf: Buffer, len: Int, msg_len: Int, base: Int) Int {
  if base + 4 <= len {
    return bswap32(buf.get_u32(base))
  }

  b0 := buf_padded_byte(buf, len, msg_len, base)
  b1 := buf_padded_byte(buf, len, msg_len, base + 1)
  b2 := buf_padded_byte(buf, len, msg_len, base + 2)
  b3 := buf_padded_byte(buf, len, msg_len, base + 3)
  u32(b0 << 24 | b1 << 16 | b2 << 8 | b3)
}

/// Raw 20-byte SHA-1 digest of the entire buffer (`buf.len()` bytes).
pub fn digest_buf(buf: Buffer) Vector<Byte> {
  len := buf.len()
  msg_len := padded_len(len)
  scratch := buffer.new(64)

  h0 := 0x67452301
  h1 := 0xefcdab89
  h2 := 0x98badcfe
  h3 := 0x10325476
  h4 := 0xc3d2e1f0

  offset := 0
  for offset < msg_len {
    j := 0
    for j < 16 {
      scratch.set_u32(j * 4, buf_be_word(buf, len, msg_len, offset + j * 4))
      j = j + 1
    }

    a := h0
    b := h1
    c := h2
    d := h3
    e := h4

    i := 0
    for i < 80 {
      f := cond {
        i < 20 => b & c | not32(b) & d,
        i < 40 => b ^ c ^ d,
        i < 60 => b & c | b & d | c & d,
        _ => b ^ c ^ d,
      }
      kk := cond {
        i < 20 => 0x5a827999,
        i < 40 => 0x6ed9eba1,
        i < 60 => 0x8f1bbcdc,
        _ => 0xca62c1d6,
      }
      wi := if i < 16 {
        scratch.get_u32((i & 15) * 4)
      } else {
        next := rotl32(
          scratch.get_u32(((i - 3) & 15) * 4)
            ^ scratch.get_u32(((i - 8) & 15) * 4)
            ^ scratch.get_u32(((i - 14) & 15) * 4)
            ^ scratch.get_u32(((i - 16) & 15) * 4),
          1,
        )
        scratch.set_u32((i & 15) * 4, next)
        next
      }
      temp := u32(rotl32(a, 5) + f + e + kk + wi)
      e = d
      d = c
      c = rotl32(b, 30)
      b = a
      a = temp
      i = i + 1
    }

    h0 = u32(h0 + a)
    h1 = u32(h1 + b)
    h2 = u32(h2 + c)
    h3 = u32(h3 + d)
    h4 = u32(h4 + e)
    offset = offset + 64
  }

  scratch.free()

  out: Vector<Byte> = []
  out = write_be_word(out, h0)
  out = write_be_word(out, h1)
  out = write_be_word(out, h2)
  out = write_be_word(out, h3)
  out = write_be_word(out, h4)
  out
}
```
(The ring indices use explicit `((i - 3) & 15) * 4` parentheses to avoid any operator-precedence ambiguity. The `& 15` keeps the index inside the 16-word ring; `* 4` converts a word index to a byte offset.)

- [ ] **Step 4: Add `crypto.sha1_buf`** (in `boot/stdlib/crypto.tw`, beside `sha1_bytes`):
```tw
/// SHA-1 digest of the bytes already in a `Buffer` (hashes `buf.len()` bytes).
pub fn sha1_buf(input: Buffer) Digest {
  .{ bytes: sha1_impl.digest_buf(input) }
}
```

- [ ] **Step 5: Run the test; verify it passes** — same as Task 2 Step 5, for `sha1_equiv`.

- [ ] **Step 6: Format, lint, commit**
```bash
target/twk fmt boot/stdlib/crypto/sha1.tw boot/stdlib/crypto.tw boot/tests/suites/stdlib_crypto_suite.tw
git add -A
git commit -m "crypto: sha1_buf — hash a Buffer (BE word loads + in-place ring)

Reads big-endian message words (single load + byte-swap) and expands the
16-word schedule ring in place in a Buffer scratch for rounds 16..79.
sha1_bytes untouched."
```

---

## Task 4: SHA-256 `digest_buf` + `crypto.sha256_buf`

Big-endian message words; the schedule **expands to 64 words** (rounds 16..63) in a 256-byte scratch.

**Files:**
- Modify: `boot/stdlib/crypto/sha256.tw`
- Modify: `boot/stdlib/crypto.tw`
- Test: `boot/tests/suites/stdlib_crypto_suite.tw`

- [ ] **Step 1: Write the failing test** (in `stdlib_crypto_suite.tw`)
```tw
fn sha256_equiv(v: Vector<Byte>) Result<Void, String> {
  b := buffer.from_bytes(v)
  got := crypto.sha256_buf(b).to_bytes()
  b.free()
  assert.equal(got, crypto.sha256_bytes(v).to_bytes())
}
```
```tw
    .test(
      "sha256_buf matches sha256_bytes across edge lengths",
      fn() {
        try sha256_equiv([])
        try sha256_equiv(make_bytes(3))
        try sha256_equiv(make_bytes(4))
        try sha256_equiv(make_bytes(55))
        try sha256_equiv(make_bytes(56))
        try sha256_equiv(make_bytes(64))
        try sha256_equiv(make_bytes(100))
        try sha256_equiv(make_bytes(4096))
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run it; verify it fails** — grep `sha256_buf`. Expected: undefined.

- [ ] **Step 3: Add `digest_buf` to `sha256.tw`**

First read the existing `digest_bytes` in `boot/stdlib/crypto/sha256.tw` to copy its exact compression body (the `big_sigma`/`small_sigma`/`ch`/`maj` helpers, the `h0..h7` init constants, and the round-constant function — REUSE them; do not retype the constants). Add imports (`use @std.buffer` + `use @std.buffer.{Buffer}`) and:
```tw
fn bswap32(x: Int) Int {
  u32((x & 0xff) << 24 | (x >> 8 & 0xff) << 16 | (x >> 16 & 0xff) << 8 | (x >> 24 & 0xff))
}

fn buf_padded_byte(buf: Buffer, len: Int, msg_len: Int, pos: Int) Int {
  if pos < len {
    return buf.get_u8(pos).to_int()
  }
  if pos == len {
    return 0x80
  }

  len_start := msg_len - 8
  if pos < len_start {
    return 0
  }

  bit_len := len * 8
  shift := (pos - len_start) * 8
  bit_len >> shift & 0xff
}

fn buf_be_word(buf: Buffer, len: Int, msg_len: Int, base: Int) Int {
  if base + 4 <= len {
    return bswap32(buf.get_u32(base))
  }

  b0 := buf_padded_byte(buf, len, msg_len, base)
  b1 := buf_padded_byte(buf, len, msg_len, base + 1)
  b2 := buf_padded_byte(buf, len, msg_len, base + 2)
  b3 := buf_padded_byte(buf, len, msg_len, base + 3)
  u32(b0 << 24 | b1 << 16 | b2 << 8 | b3)
}

/// Raw 32-byte SHA-256 digest of the entire buffer (`buf.len()` bytes).
pub fn digest_buf(buf: Buffer) Vector<Byte> {
  len := buf.len()
  msg_len := padded_len(len)
  scratch := buffer.new(256)

  // h0..h7 initial hash values — copy from this module's digest_bytes.
  // (left as named locals h0..h7 exactly as in digest_bytes)

  offset := 0
  for offset < msg_len {
    // load 16 BE words, then expand to 64 in place
    j := 0
    for j < 16 {
      scratch.set_u32(j * 4, buf_be_word(buf, len, msg_len, offset + j * 4))
      j = j + 1
    }
    j = 16
    for j < 64 {
      s0 := small_sigma0(scratch.get_u32((j - 15) * 4))
      s1 := small_sigma1(scratch.get_u32((j - 2) * 4))
      scratch.set_u32(
        j * 4,
        u32(scratch.get_u32((j - 16) * 4) + s0 + scratch.get_u32((j - 7) * 4) + s1),
      )
      j = j + 1
    }

    // compression: copy the working-variable round loop from digest_bytes,
    // replacing every `sched.get(w, i)` with `scratch.get_u32(i * 4)`.

    offset = offset + 64
  }

  scratch.free()

  // output h0..h7 via write_be_word, exactly as digest_bytes does.
}
```
Fill the three "copy from digest_bytes" regions (h0..h7 init, the working-variable round loop with `sched.get(w, i)` → `scratch.get_u32(i * 4)`, and the `write_be_word` output) verbatim from this module's existing `digest_bytes`, and confirm the helper names (`small_sigma0`/`small_sigma1`/`big_sigma0`/`big_sigma1`/`ch`/`maj`/the K-constant fn) match the file. The cross-equivalence test is the correctness gate.

- [ ] **Step 4: Add `crypto.sha256_buf`** (in `boot/stdlib/crypto.tw`, beside `sha256_bytes`):
```tw
/// SHA-256 digest of the bytes already in a `Buffer` (hashes `buf.len()` bytes).
pub fn sha256_buf(input: Buffer) Digest {
  .{ bytes: sha256_impl.digest_buf(input) }
}
```

- [ ] **Step 5: Run the test; verify it passes** — for `sha256_equiv`.

- [ ] **Step 6: Format, lint, commit**
```bash
target/twk fmt boot/stdlib/crypto/sha256.tw boot/stdlib/crypto.tw boot/tests/suites/stdlib_crypto_suite.tw
git add -A
git commit -m "crypto: sha256_buf — hash a Buffer (BE loads + 64-word buffer schedule)

Expands the message schedule to 64 words in place in a Buffer scratch
instead of rebuilding the functional schedule record per round.
sha256_bytes untouched."
```

---

## Task 5: Bench cases + measurement

**Files:**
- Modify: `examples/crypto-bench/twinkle/main.tw`

- [ ] **Step 1: Add the Buffer input + warm-up + bench cases**

In `examples/crypto-bench/twinkle/main.tw`:
- At the top, add `use @std.buffer`.
- After `large := make_bytes(4096)`, add `large_buf := buffer.from_bytes(large)`.
- In the warm-up block (where `warm_md5`/etc. are computed), add warm-up calls:
  ```tw
  warm_md5b := crypto.md5_buf(large_buf)
  warm_sha1b := crypto.sha1_buf(large_buf)
  warm_sha256b := crypto.sha256_buf(large_buf)
  ```
  and fold `first_byte(warm_md5b) ^ first_byte(warm_sha1b) ^ first_byte(warm_sha256b)` into `warm_sink`.
- After the existing `sha256_4k` timed block, add three timed loops mirroring the `_4k` ones but calling the `_buf` variant on `large_buf`:
  ```tw
  sink = 0
  start = date.now()
  i = 0
  for i < iters_large {
    sink = sink ^ first_byte(crypto.md5_buf(large_buf))
    i = i + 1
  }
  print_result("md5_4k_buf", iters_large, date.now() - start, sink)
  ```
  and likewise `sha1_4k_buf` (`crypto.sha1_buf`) and `sha256_4k_buf` (`crypto.sha256_buf`).
- At the very end of the file (after the last `print_result`), add `large_buf.free()`.

- [ ] **Step 2: Run the bench; record the delta**

```bash
make bundle-cli   # the bench runs through target/twk; it must reflect the new stdlib
target/twk run examples/crypto-bench/twinkle/main.tw
```
Expected: lines for `md5_4k`, `md5_4k_buf`, `sha1_4k`, `sha1_4k_buf`, `sha256_4k`, `sha256_4k_buf`. The `_buf` lines should have a smaller `ms` than their `_bytes` counterparts. Record the numbers in the commit body (the comparison, not a vanity count).

- [ ] **Step 3: Commit**
```bash
target/twk fmt examples/crypto-bench/twinkle/main.tw
git add examples/crypto-bench/twinkle/main.tw
git commit -m "crypto-bench: add Buffer-native 4k hash cases

Hash a once-converted Buffer via crypto.*_buf to measure the in-buffer
win against the existing Vector<Byte> _bytes cases. <paste md5_4k vs
md5_4k_buf and sha*_4k vs _buf ms here>."
```

---

## Task 6: Docs + full gate + closeout

**Files:**
- Modify: `docs/API.md`
- Modify: `docs/plans/README.md`, `docs/plans/in-buffer-crypto.md` (closeout)

- [ ] **Step 1: Document the new surface in `docs/API.md`**

- In the `@std.buffer` accessor table, add `buf.get_u32(off) Int` (32-bit unsigned LE read, unchecked) and `buf.set_u32(off, v)` (32-bit LE write).
- In the `@std.crypto` table, add `crypto.md5_buf` / `sha1_buf` / `sha256_buf` `fn(input: Buffer) Digest`, noting they hash the whole buffer (`buf.len()` bytes) and pair with `buffer.from_bytes`.

- [ ] **Step 2: Full gate**

```bash
make bundle-cli    # expect "Fixed point reached"
make boot-test     # expect all suites pass, incl. stdlib buffer + stdlib crypto
```

- [ ] **Step 3: Closeout**

- In `docs/plans/README.md`, update the In-buffer crypto row status to "Done" with the measured win (or remove the row and archive per the repo convention — match how sibling completed efforts are handled).
- In `docs/plans/in-buffer-crypto.md`, flip the top status to DONE and record the measured 4k deltas in §7.
- `git mv docs/plans/in-buffer-crypto-plan.md docs/plans/archive/in-buffer-crypto-plan.md`.

- [ ] **Step 4: Commit**
```bash
git add -A
git commit -m "crypto: in-buffer hashing done — docs, gate, closeout

Buffer.get_u32/set_u32 + crypto.*_buf shipped; self-host fixed point and
full boot suite green. Measured 4k bench win: <paste>."
```

---

## Notes / risks for the implementer

- **No `src/`/stage0 changes.** `boot/main.tw` uses none of this; the self-host fixed point is the proof. The `__buf_*` are internal-only (Task 1 puts them in `add_internal_host_builtins`), so user programs still can't call them directly.
- **Correctness is gated by the cross-equivalence tests** (`*_buf` must byte-match `*_bytes`). They cover the empty input, the 55/56/64 padding boundaries (where exact-block inputs add a whole synthetic padding block), and non-4-aligned lengths — exactly where the `base + 4 <= len` fast/slow split and the padding synthesis are easy to get wrong. If a hash mismatches, suspect the boundary word handling or a bswap.
- **Never `get_u32` past `len`.** The fast path is guarded by `base + 4 <= len`; the buffer's allocation is 8-byte rounded, so bytes past `len` are uninitialized garbage.
- **`make bundle-cli` before running the bench** (Task 5) and before the final gate — `target/twk` is stale otherwise. During TDD use the stage1 path.
- **`digest_buf` allocates a scratch and frees it; it must NOT free the caller's input buffer** (the tests free their own buffer).
- Verify helper names against the actual files before relying on them: SHA-256's `small_sigma0/1`, `big_sigma0/1`, `ch`, `maj`, and K-constant function names; `assert.equal`'s `Result<Void,String>` shape; `Digest.to_bytes()`.
