# Linear-Memory Buffer M3 — Byte-Codec Go/No-Go Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Settle the honest go/no-go the M1 sort proxy never answered — does decoding a binary byte format over **linear memory** (O(1) `i32.load8_u`) beat decoding over a GC `Vector<Byte>` (O(log₃₂ n) random index) on a **decode-dominated** workload?

**Architecture:** Reuse M1's program linear memory + bump allocator (`rt.buf`) and the *already-existing* byte load/store IR (`I32Load8U`/`I32Store8`). Add two tiny `rt.buf` byte accessors and expose the allocator + accessors as internal `__buf_*` builtins. Implement a minimal **LEB128 varint codec** twice — once reading/writing linear memory, once reading/writing a `Vector<Byte>` — prove them equal by round-trip, then A/B their decode speed on a large stream. Varint decode is almost pure byte-indexing (one shift+or+mask per byte), so the indexing cost is *not* diluted by arithmetic the way SHA-1's compression would dilute it.

**Tech Stack:** Twinkle (`.tw`), boot self-hosted compiler, Deno/V8 Wasm-GC runtime. Boot-codegen only; **no stage0 changes** (see "No stage0 work" below).

**Spec:** `docs/plans/buffer-linear-memory.md` (see its "Post-M1: the strategic case" section). M1 plan: `docs/plans/buffer-linear-memory-m1-plan.md`.

---

## Why this design (read once)

- **The sort was the wrong proxy.** M1's dense `Vector<Int>` sort came back at parity because (a) the sort reads each scratch element few times so the GC→linear gather is a wash, and (b) sort cost is dominated by comparator/merge work, not indexing. A *decoder* is the workload linear memory is actually good at: dense, sequential, indexing-dominated byte access.
- **The gather trap still applies — avoid it.** If you stage bytes through a `Vector<Byte>` and then copy them into linear memory, you pay the O(log n)/byte you were trying to avoid. So in the benchmark **the bytes must originate natively in each representation** (encode straight into a `Vector<Byte>` for the baseline; encode straight into linear memory for the linear path). Never cross-gather. This mirrors the real decoder scenario: bytes arrive from I/O directly into a buffer; you never build a `Vector<Byte>` first.
- **Measure decode only.** Pre-encode the stream into each representation *outside* the timed region, then time the decode (sum) pass. That isolates the read-indexing cost, which is the whole question.
- **Varint, not a hash.** LEB128 decode does ~3 ALU ops per byte, so wall time tracks byte-fetch cost. That is the clean signal. (SHA-1 was rejected: ~800 compression ops/block vs 64 byte reads → the indexing delta is ~5% of work and drowns in noise.)
- **Internal surface only.** Like M1, M3 adds **no user-facing `Buffer` type** (that is M2). The `__buf_*` builtins are raw, undocumented, probe-only machinery (the same `__name` convention stdlib uses to reach `__host_read_file`). After the go/no-go they are either promoted into M2 or removed — they are not a committed public API.

---

## No stage0 work

All edits are boot-only (`boot/`). The new `__buf_*` builtins are used **only** by the probe codec, its test suite, and the bench — never by `boot/main.tw`. The self-host loop (`make bundle-cli`) only asks stage0 to compile `boot/main.tw`, which does not reference `__buf_*`, so stage0 (`src/`) needs no parity. This matches M1, which added `rt.buf` boot-only. Do not touch `src/`.

---

## Build & test methodology (read first — same trap as M1)

Two kinds of change, tested differently:

- **Library code a test imports and calls** (the `lib.buf_codec` functions in Task 3, exercised by `buf_codec_suite`). A suite that imports the edited module compiles it into the test program, so the edit takes effect when run — **but** the codec calls `__buf_*` builtins, which the compiler only understands after Tasks 1–2 are compiled **into** the compiler. So once the codec uses `__buf_*`, even its suite must run on a **rebuilt** compiler.
- **Compiler-internal additions** (the `rt.buf` functions in Task 1, the builtin registry + signatures in Task 2). These change what the compiler *accepts and emits*, so they take effect **only after the boot compiler is rebuilt**. The stale bundled `target/twk` does not know `__buf_*` and will reject programs that use them.

**Fast rebuild loop** (use for every check in Tasks 2–4):

```bash
cargo build --release                                          # stage0 (Rust); once, unless you edit src/
./target/release/twk build boot/main.tw -o /tmp/stage1.wasm    # stage0 compiles edited boot → stage1 (knows __buf_*, emits rt.buf accessors)
# run a program or the suite with the freshly-built compiler:
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs run <program-or-suite> <filter>
```

**Exception — shape/signature-only checks that do NOT compile a program using `__buf_*`** (Task 1's `rt_buf.module()` shape assertion) can use the stale `target/twk`, because they only call the edited module function directly. Each task says which loop to use.

After Task 3 registers `buf_codec_suite` in `boot/tests/main.tw`, the **full** suite via the stale `target/twk` will fail to compile (unknown `__buf_*`) until `make bundle-cli` (Task 4) bakes the builtins into `target/twk`. Between Task 3 and Task 4, run the full suite via the stage1 loop above. This is expected.

---

## File structure

- **Modify** `boot/compiler/codegen/runtime/buf.tw` — add `buf_load_u8`/`buf_store_u8` FuncDefs + exports (Task 1).
- **Modify** `boot/tests/suites/codegen_integration_suite.tw` — bump the `rt.buf` shape test to 5 funcs / 5 exports (Task 1).
- **Modify** `boot/compiler/builtins.tw` — append five `rt("buf_*", "rt.buf", ...)` specs (Task 2).
- **Modify** `boot/compiler/base_env.tw` — append five `builtin_sig("buf_*", ...)` signatures (Task 2).
- **Create** `boot/lib/buf_codec.tw` — the LEB128 varint codec: linear + `Vector<Byte>` encode/decode/sum (Task 3).
- **Create** `boot/tests/suites/buf_codec_suite.tw` — round-trip + cross-check correctness (Task 3).
- **Modify** `boot/tests/main.tw` — register the new suite (Task 3).
- **Create** `boot/bench/buf_codec_bench.tw` — the A/B decode bench (Task 4).
- **Modify** `docs/plans/buffer-linear-memory.md` — record the M3 go/no-go result (Task 4).

---

## Task 1: `rt.buf` byte accessors

`buf_alloc`/`buf_mark`/`buf_reset` already exist in `rt.buf` from M1. Add byte-granular read/write. They use the **already-existing** `I32Load8U(align, offset)` / `I32Store8(align, offset)` instructions (byte access uses `align=0`, `offset=0`). Address is `base + i` (1-byte stride). These functions speak the i32 memory ABI; the builtin call boundary bridges Twinkle `Int` (i64) ↔ i32 automatically, exactly as it does for `Vector.get`'s index (`rt.arr get` declares its index param `.I32`) and `String.len`'s `.I32` result.

**Files:**
- Modify: `boot/compiler/codegen/runtime/buf.tw`
- Test: `boot/tests/suites/codegen_integration_suite.tw` (existing `test_rt_buf_module_shape`)

- [ ] **Step 1: Update the failing shape test**

In `boot/tests/suites/codegen_integration_suite.tw`, change `test_rt_buf_module_shape` to expect five funcs and five exports, and to assert the two new export names:

```tw
fn test_rt_buf_module_shape() Result<Void, String> {
  m := rt_buf.module()
  try assert.equal(m.namespace, "rt.buf")
  try assert.equal(m.memories.len(), 1)
  try assert.equal(m.funcs.len(), 5)
  try assert.equal(m.exports.len(), 5)
  names := collect e in m.exports { e.wasm_name }
  try assert.ok(names.contains("buf_load_u8"), "exports buf_load_u8")
  try assert.ok(names.contains("buf_store_u8"), "exports buf_store_u8")
  .Ok({})
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `target/twk run boot/tests/main.tw codegen`
Expected: FAIL — `funcs.len()` is 3, not 5 (and `buf_load_u8` not exported). (Stale `target/twk` is fine here — the test calls `rt_buf.module()` directly.)

- [ ] **Step 3: Add the two accessor FuncDefs**

In `boot/compiler/codegen/runtime/buf.tw`, add these two functions (place them after `reset_fn`):

```tw
// buf_load_u8(base, i) -> u8 zero-extended. addr = base + i (1-byte stride).
fn load_u8_fn() FuncDef {
  .{
    name: "buf_load_u8",
    params: [.I32, .I32],
    results: [.I32],
    locals: [],
    body: [.LocalGet(0), .LocalGet(1), .I32Add, .I32Load8U(0, 0)],
  }
}

// buf_store_u8(base, i, v) — writes the low byte of v at addr = base + i.
fn store_u8_fn() FuncDef {
  .{
    name: "buf_store_u8",
    params: [.I32, .I32, .I32],
    results: [],
    locals: [],
    body: [.LocalGet(0), .LocalGet(1), .I32Add, .LocalGet(2), .I32Store8(0, 0)],
  }
}
```

- [ ] **Step 4: Wire them into the module's funcs + exports**

In the same file's `module()`, change the `funcs:` and `exports:` fields:

```tw
    funcs: [alloc_fn(), mark_fn(), reset_fn(), load_u8_fn(), store_u8_fn()],
```

and add two `ExportDef` entries to the existing `exports:` list:

```tw
      ExportDef.{ wasm_name: "buf_load_u8", func_sym: "buf_load_u8" },
      ExportDef.{ wasm_name: "buf_store_u8", func_sym: "buf_store_u8" },
```

- [ ] **Step 5: Run the shape test to verify it passes**

Run: `target/twk run boot/tests/main.tw codegen`
Expected: PASS (all codegen suites green; exhaustive matches already covered).

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/codegen/runtime/buf.tw
target/twk lint boot/main.tw
git add boot/compiler/codegen/runtime/buf.tw boot/tests/suites/codegen_integration_suite.tw
git commit -m "codegen: add rt.buf byte accessors (buf_load_u8/buf_store_u8)"
```

---

## Task 2: Expose `__buf_*` builtins

Make the allocator + accessors callable from `.tw` as `__buf_alloc`, `__buf_mark`, `__buf_reset`, `__buf_load_u8`, `__buf_store_u8`. The `__`-prefix-to-builtin convention is exactly how `boot/stdlib/io.tw` reaches `__host_stdout_write_bytes` (registered as `host_stdout_write_bytes`).

> **Implementation note (corrected against the actual wiring — committed `d536a09c`).** This plan originally claimed two sites and "automatic" i64↔i32 bridging. That was wrong; the real recipe is **three** sites, and the i64↔i32 bridge is **not** automatic:
> 1. **`boot/compiler/builtins.tw` — `builtin_specs()`**: append the five `rt("buf_*", "rt.buf", ...)` specs at the **end** (FuncIds are positional). *Also* add five entries to **`builtin_abi()`** in the same file declaring the wasm types (`"buf_alloc" => abi([.I32], [.I32])`, `"buf_mark" => abi([], [.I32])`, `"buf_reset" => abi([.I32], [])`, `"buf_load_u8" => abi([.I32, .I32], [.I32])`, `"buf_store_u8" => abi([.I32, .I32, .I32], [])`). These ABI entries are what drive the i64→i32 arg narrowing / i32→i64 result widening at the call boundary — the same way `vector$get`/`host_*` have `builtin_abi` entries. Without them the i64 Twinkle values would mismatch the i32 rt.buf functions.
> 2. **`boot/compiler/base_env.tw` — `builtin_env()`**: register the signatures with `.add_function(builtin_sig("__buf_alloc", [], ["nbytes"], [.Int], .Some(.Int)))` etc. — note the name carries the `__` prefix, and `add_function` (which both registers and binds) is required (a plain `builtin_sig` in the `sig_fns` list does not create a callable binding). Registering in `builtin_env()` (global) rather than `add_internal_host_builtins()` (internal-only) makes `__buf_*` callable from `boot/lib`, the test suites, and the bench. **Caveat:** unlike `__host_*` (internal-only), this exposes raw linear-memory ops to *every* program — an acceptable probe leak, but narrow it (or remove it) when M3 is resolved.
> 3. **`boot/compiler/lower_core/context.tw` — `new_ctx`**: the `__${name}` alias loop gates on `entry.name.starts_with("host_")`; extend it to `or entry.name.starts_with("buf_")` so `__buf_*` source names resolve to the `buf_*` FuncIds.
>
> The original two-site steps below remain as a record of intent; follow the corrected recipe above.

**Files (actual):**
- Modify: `boot/compiler/builtins.tw` (`builtin_specs()` **and** `builtin_abi()`)
- Modify: `boot/compiler/base_env.tw` (`builtin_env()` via `add_function`)
- Modify: `boot/compiler/lower_core/context.tw` (`new_ctx` `__`-alias loop)

- [ ] **Step 1: Write the failing smoke program**

```bash
cat > /tmp/buf_smoke.tw <<'EOF'
mark := __buf_mark()
base := __buf_alloc(64)
__buf_store_u8(base, 0, 65)
__buf_store_u8(base, 1, 66)
println("byte0=${__buf_load_u8(base, 0)} byte1=${__buf_load_u8(base, 1)}")
__buf_reset(mark)
EOF
```

- [ ] **Step 2: Run it to verify it fails**

Run: `target/twk run /tmp/buf_smoke.tw`
Expected: FAIL — unknown function `__buf_alloc` (the stale compiler has no such builtin).

- [ ] **Step 3: Register the builtins**

In `boot/compiler/builtins.tw`, append these to the **end** of the `builtin_specs()` list (after `rt("vector$builder_freeze_i64", ...)`):

```tw
    rt("buf_alloc", "rt.buf", "buf_alloc", .None),
    rt("buf_mark", "rt.buf", "buf_mark", .None),
    rt("buf_reset", "rt.buf", "buf_reset", .None),
    rt("buf_load_u8", "rt.buf", "buf_load_u8", .None),
    rt("buf_store_u8", "rt.buf", "buf_store_u8", .None),
```

- [ ] **Step 4: Declare the type signatures**

In `boot/compiler/base_env.tw`, append these after `builtin_sig("host_stdout_write_bytes", ...)` (signature is `builtin_sig(name, type_params, param_names, param_types, return?)`; all params/results are `.Int`/`.Void` — the call boundary handles i64↔i32):

```tw
    builtin_sig("buf_alloc", [], ["nbytes"], [.Int], .Some(.Int)),
    builtin_sig("buf_mark", [], [], [], .Some(.Int)),
    builtin_sig("buf_reset", [], ["mark"], [.Int], .Some(.Void)),
    builtin_sig("buf_load_u8", [], ["base", "i"], [.Int, .Int], .Some(.Int)),
    builtin_sig("buf_store_u8", [], ["base", "i", "v"], [.Int, .Int, .Int], .Some(.Void)),
```

- [ ] **Step 5: Rebuild the compiler and run the smoke**

```bash
cargo build --release
./target/release/twk build boot/main.tw -o /tmp/stage1.wasm
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs run /tmp/buf_smoke.tw
```
Expected: prints `byte0=65 byte1=66` — the builtins resolve, lower to `rt.buf` calls, and round-trip a byte through linear memory.

- [ ] **Step 6: Confirm the existing suites still pass on the rebuilt compiler**

```bash
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs run boot/tests/main.tw
```
Expected: all tests pass (registering new builtins must not perturb existing dispatch; the `base_env_guardrail`/`builtins` suites confirm registry↔signature consistency).

- [ ] **Step 7: Format, lint, commit**

```bash
target/twk fmt boot/compiler/builtins.tw boot/compiler/base_env.tw
target/twk lint boot/main.tw
git add boot/compiler/builtins.tw boot/compiler/base_env.tw
git commit -m "compiler: expose __buf_* builtins (alloc/mark/reset/load_u8/store_u8)"
```

---

## Task 3: LEB128 varint codec + correctness

Implement the codec twice over the same logic — once on linear memory, once on `Vector<Byte>` — and prove them equal by round-trip and cross-check. Unsigned LEB128: 7 payload bits per byte, high bit set means "more bytes follow". `decode_*` (builds a `Vector<Int>`) is for exact correctness; `sum_*` (allocation-free) is the bench hot path. Values stay below 2³¹ so `>> 7` on the i64 `Int` is always positive.

**Files:**
- Create: `boot/lib/buf_codec.tw`
- Create: `boot/tests/suites/buf_codec_suite.tw`
- Modify: `boot/tests/main.tw` (register the suite)

- [ ] **Step 1: Write the codec module**

Create `boot/lib/buf_codec.tw`:

```tw
//! Probe-only LEB128 varint codec for the M3 linear-memory go/no-go.
//! Two parallel implementations — one over a linear-memory region (via the
//! __buf_* builtins), one over a GC Vector<Byte> — used to A/B decode speed.
//! Not a public API; remove or promote after the go/no-go decision.

// ── Linear-memory region (base offset = byte 0 of the stream) ──────────

// Encode `value` as LEB128 at `base + pos`; return the next write position.
pub fn enc_varint_linear(base: Int, pos: Int, value: Int) Int {
  v := value
  p := pos
  for v >= 128 {
    __buf_store_u8(base, p, (v & 127) | 128)
    v = v >> 7
    p = p + 1
  }
  __buf_store_u8(base, p, v)
  p + 1
}

// Decode `count` varints starting at byte 0 of `base`; return their sum.
pub fn sum_varints_linear(base: Int, count: Int) Int {
  acc := 0
  pos := 0
  n := 0
  for n < count {
    result := 0
    shift := 0
    more := true
    for more {
      b := __buf_load_u8(base, pos)
      pos = pos + 1
      result = result | ((b & 127) << shift)
      shift = shift + 7
      if b < 128 {
        more = false
      }
    }
    acc = acc + result
    n = n + 1
  }
  acc
}

// Decode `count` varints into a vector (exact-correctness oracle).
pub fn decode_varints_linear(base: Int, count: Int) Vector<Int> {
  out: Vector<Int> = []
  pos := 0
  n := 0
  for n < count {
    result := 0
    shift := 0
    more := true
    for more {
      b := __buf_load_u8(base, pos)
      pos = pos + 1
      result = result | ((b & 127) << shift)
      shift = shift + 7
      if b < 128 {
        more = false
      }
    }
    out = out.append(result)
    n = n + 1
  }
  out
}

// ── Vector<Byte> baseline (same logic, GC-array indexing) ──────────────

pub fn enc_varint_vec(out: Vector<Byte>, value: Int) Vector<Byte> {
  bytes := out
  v := value
  for v >= 128 {
    bytes = bytes.append(Byte.from_int((v & 127) | 128).unwrap())
    v = v >> 7
  }
  bytes.append(Byte.from_int(v).unwrap())
}

pub fn sum_varints_vec(bytes: Vector<Byte>, count: Int) Int {
  acc := 0
  pos := 0
  n := 0
  for n < count {
    result := 0
    shift := 0
    more := true
    for more {
      b := bytes[pos].to_int()
      pos = pos + 1
      result = result | ((b & 127) << shift)
      shift = shift + 7
      if b < 128 {
        more = false
      }
    }
    acc = acc + result
    n = n + 1
  }
  acc
}

pub fn decode_varints_vec(bytes: Vector<Byte>, count: Int) Vector<Int> {
  out: Vector<Int> = []
  pos := 0
  n := 0
  for n < count {
    result := 0
    shift := 0
    more := true
    for more {
      b := bytes[pos].to_int()
      pos = pos + 1
      result = result | ((b & 127) << shift)
      shift = shift + 7
      if b < 128 {
        more = false
      }
    }
    out = out.append(result)
    n = n + 1
  }
  out
}
```

- [ ] **Step 2: Write the correctness suite**

Create `boot/tests/suites/buf_codec_suite.tw`:

```tw
use lib.buf_codec.{
  decode_varints_linear, decode_varints_vec, enc_varint_linear, enc_varint_vec, sum_varints_linear,
  sum_varints_vec,
}
use tests.assert
use tests.runner

fn sample_values() Vector<Int> {
  [0, 1, 127, 128, 255, 300, 16383, 16384, 1000000, 2147483647]
}

fn expected_sum(values: Vector<Int>) Int {
  total := 0
  for v in values {
    total = total + v
  }
  total
}

// Encode the samples into a linear region, decode, and cross-check against the
// Vector<Byte> path and the known sum.
fn test_varint_roundtrip_and_crosscheck() Result<Void, String> {
  values := sample_values()

  // Vector<Byte> path: encode natively into a GC array.
  vbytes: Vector<Byte> = []
  for v in values {
    vbytes = enc_varint_vec(vbytes, v)
  }

  // Linear path: encode natively into linear memory (worst case 5 bytes / u32).
  mark := __buf_mark()
  base := __buf_alloc(values.len() * 5)
  pos := 0
  for v in values {
    pos = enc_varint_linear(base, pos, v)
  }

  // Exact decode equality.
  dec_lin := decode_varints_linear(base, values.len())
  dec_vec := decode_varints_vec(vbytes, values.len())
  try assert.equal(dec_lin, values)
  try assert.equal(dec_vec, values)
  try assert.equal(dec_lin, dec_vec)

  // Allocation-free sum (the bench path) agrees with the oracle.
  try assert.equal(sum_varints_linear(base, values.len()), expected_sum(values))
  try assert.equal(sum_varints_vec(vbytes, values.len()), expected_sum(values))

  __buf_reset(mark)
  .Ok({})
}

// Empty stream is a no-op on both paths.
fn test_varint_empty() Result<Void, String> {
  mark := __buf_mark()
  base := __buf_alloc(8)
  try assert.equal(sum_varints_linear(base, 0), 0)
  __buf_reset(mark)
  empty: Vector<Byte> = []
  try assert.equal(sum_varints_vec(empty, 0), 0)
  .Ok({})
}

pub fn suite() runner.Suite {
  runner.suite("buf_codec")
    .test("varint round-trip + linear/vec cross-check", test_varint_roundtrip_and_crosscheck)
    .test("empty varint stream", test_varint_empty)
}
```

- [ ] **Step 3: Register the suite**

In `boot/tests/main.tw`, add the import alongside the other `use .suites.*` lines:

```tw
use .suites.buf_codec_suite
```

and add it to the `runner.run_all([...])` list:

```tw
  buf_codec_suite.suite(),
```

- [ ] **Step 4: Run the new suite on a rebuilt compiler**

The codec calls `__buf_*`, so the suite must run on a compiler that knows them. Do NOT use the stale `target/twk`.

```bash
cargo build --release
./target/release/twk build boot/main.tw -o /tmp/stage1.wasm
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs run boot/tests/main.tw buf_codec
```
Expected: both tests PASS — the linear and `Vector<Byte>` codecs produce identical results and match the known values/sum.

- [ ] **Step 5: Run the full suite on the rebuilt compiler**

```bash
BOOT_WASM=/tmp/stage1.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs run boot/tests/main.tw
```
Expected: all tests pass. (The stale `target/twk` will now fail to compile the full suite because it does not yet know `__buf_*`; that is expected until Task 4's `make bundle-cli`.)

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/lib/buf_codec.tw boot/tests/suites/buf_codec_suite.tw
target/twk lint boot/main.tw
git add boot/lib/buf_codec.tw boot/tests/suites/buf_codec_suite.tw boot/tests/main.tw
git commit -m "probe: LEB128 varint codec over linear memory vs Vector<Byte> + correctness"
```

---

## Task 4: The go/no-go bench + decision

Pre-encode one large varint stream into each representation, then time the decode (`sum`) pass twice (V8 tier-up). The encode is **outside** the timed region and **native to each representation** (no cross-gather). The bench self-checks correctness before timing and traps on mismatch.

**Files:**
- Create: `boot/bench/buf_codec_bench.tw`
- Modify: `docs/plans/buffer-linear-memory.md` (record the result)

- [ ] **Step 1: Write the bench program**

Create `boot/bench/buf_codec_bench.tw`:

```tw
use @std.date
use lib.buf_codec.{enc_varint_linear, enc_varint_vec, sum_varints_linear, sum_varints_vec}

// Deterministic, identical value stream for both representations; kept < 2^31.
fn value_at(i: Int) Int {
  (i * 2654435761) % 2000000000
}

fn expected_sum(count: Int) Int {
  total := 0
  for i in range(count) {
    total = total + value_at(i)
  }
  total
}

count := 4000000

// Encode natively into a Vector<Byte> (baseline origination).
vbytes: Vector<Byte> = []
for i in range(count) {
  vbytes = enc_varint_vec(vbytes, value_at(i))
}

// Encode natively into linear memory (linear origination, no Vector<Byte> gather).
mark := __buf_mark()
base := __buf_alloc(count * 5)
wp := 0
for i in range(count) {
  wp = enc_varint_linear(base, wp, value_at(i))
}

// Correctness gate before timing — trap on any mismatch.
want := expected_sum(count)
if sum_varints_vec(vbytes, count) != want {
  error("vec decode mismatch")
}
if sum_varints_linear(base, count) != want {
  error("linear decode mismatch")
}

// Time decode only, each twice (run1 cold / run2 warm after tier-up).
start1 := date.now()
s1 := sum_varints_vec(vbytes, count)
println("run1 Vector<Byte> decode  ${date.now() - start1}ms (sum ${s1})")

start2 := date.now()
s2 := sum_varints_linear(base, count)
println("run1 linear-memory decode ${date.now() - start2}ms (sum ${s2})")

start3 := date.now()
s3 := sum_varints_vec(vbytes, count)
println("run2 Vector<Byte> decode  ${date.now() - start3}ms (sum ${s3})")

start4 := date.now()
s4 := sum_varints_linear(base, count)
println("run2 linear-memory decode ${date.now() - start4}ms (sum ${s4})")

__buf_reset(mark)
```

- [ ] **Step 2: Run the self-host fixed point so `target/twk` knows `__buf_*`**

```bash
make bundle-cli
```
Expected: ends with `Fixed point reached: stage3 == stage4` and `Built Deno Twinkle CLI: target/twk`. After this, `target/twk` contains the `rt.buf` accessors + `__buf_*` builtins, so it can compile and run the bench (and the full suite) directly again.

- [ ] **Step 3: Run the bench**

Run: `target/twk run boot/bench/buf_codec_bench.tw`
Expected: four timing lines; the two `sum` values per run match (and equal across both decoders — already gated by Step 1's trap). Record the **warm** (run2) `Vector<Byte>` vs `linear-memory` decode milliseconds.

- [ ] **Step 4: Record the decision**

Update the M1-result section of `docs/plans/buffer-linear-memory.md` with the measured warm decode numbers, then decide:

- **If linear-memory decode clearly wins** (meaningfully faster than `Vector<Byte>`, beyond run-to-run noise): the linear-memory direction is validated on the workload it is actually good at. Note the before/after ms and recommend proceeding to M3b (the real flat IR/Wasm codec) and/or M4 (shared-memory worker transport), with the prerequisite IR work (shared `MemoryDef`, atomics, conditional memory emission) from the "Post-M1: the strategic case" section.
- **If it is parity or worse**: linear memory does not pay off even for dense byte indexing in-process. Record the numbers; the direction stops here. (This would be a strong negative signal, since varint decode is the most indexing-favorable workload available.)

Add a short paragraph stating which outcome occurred and the numbers.

- [ ] **Step 5: Commit**

```bash
git add boot/bench/buf_codec_bench.tw docs/plans/buffer-linear-memory.md
git commit -m "buffer M3: byte-codec go/no-go bench + recorded result"
```

---

## Notes for the implementer

- **No new IR.** `I32Load8U`/`I32Store8` predate M1 (`wasm_ir.tw`), with emit + WAT arms already present. M3 only adds two `rt.buf` functions that *use* them, plus builtin wiring.
- **No stage0 work** (see top). Do not touch `src/`.
- **The rebuild trap is the #1 hazard.** Anything that compiles a program using `__buf_*` must run on a freshly built compiler (`/tmp/stage1.wasm`), never the stale `target/twk`, until `make bundle-cli`. The stale compiler silently lacks the builtins.
- **Do not cross-gather in the bench.** Each representation must be encoded into natively. If you ever find yourself building a `Vector<Byte>` and copying it into linear memory (or vice versa), you have reintroduced the O(n·log n) gather and the measurement is invalid.
- **Probe hygiene.** `boot/lib/buf_codec.tw`, the suite, the bench, and the `__buf_*` builtins are probe-only. If the go/no-go is negative, a follow-up should remove them (and reconsider whether the M1 `rt.buf` memory should stay emitted on every module — see the M1 "Caveat to fix before any merge").
- **Run the formatter** (`target/twk fmt <file>`) and **linter** (`target/twk lint boot/main.tw`) on every edited `.tw` before committing.

---

## Self-review (controller checklist)

- **Spec coverage:** the spec's "recommended next probe" asked for "a minimal flat IR (or Wasm-module) codec over the M1 load/store IR, decode it with O(1) indexing, and compare against the current `Vector<Byte>` decode." Tasks 1–2 expose the O(1) byte path; Task 3 implements the codec both ways with correctness; Task 4 is the A/B comparison + decision. Covered.
- **Type consistency:** `enc_varint_linear(base,pos,value)->Int`, `sum_varints_linear(base,count)->Int`, `decode_varints_linear(base,count)->Vector<Int>` and their `_vec` twins are used with matching signatures in the suite and bench. `__buf_alloc:Int->Int`, `__buf_mark:()->Int`, `__buf_reset:Int->Void`, `__buf_load_u8:(Int,Int)->Int`, `__buf_store_u8:(Int,Int,Int)->Void` match `base_env` sigs and `rt.buf` ABI (i64↔i32 bridged at the call boundary).
- **No placeholders:** every code step shows complete code; wiring steps name the exact append sites and analog patterns.
```
