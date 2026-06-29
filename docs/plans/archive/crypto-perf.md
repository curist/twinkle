# Crypto Performance — Routes to Native Speed

Status: **CLOSED (2026-06-29).** A catalogue of directions that could make
`@std.crypto`/base64 faster. Two things were settled here:

1. **Tabling alone is a dud.** Route 1 (hash inline+table) buys ~10–13%; a
   table-driven base64 (the original Route 3) buys ~3% on the roundtrip. On a
   V8-class engine the JIT already inlines the small helpers, so the char↔value
   mapping a table optimizes is not where the time goes.
2. **The value boundary is where the time goes — and one boundary was killed.**
   The remaining cost is the language's GC-value boundary work: the string
   builder, `Vector<Byte>` append, `from_utf8` validation, per-element reads.
   Tables can't touch those, but a *bridge primitive* can. This work shipped
   `String.from_mem` / `buffer.to_string` (write encoded bytes into a linear-memory
   `Buffer`, materialize the `String` in one pass, no UTF-8 validation) plus
   `buffer.set_byte` (raw store, no `Byte` box). **`base64_encode` rewritten onto
   that bridge: encode ~50µs → ~30µs (~40%), roundtrip ~99µs → ~75µs (~24%).**

What remains is the **decode** half: it builds a `Vector<Byte>` via `.append` and
that loop now dominates the roundtrip. Closing it needs the *dual* bridge — a
bulk `Buffer`→`Vector<Byte>` construct or a `base64_decode_buf` returning a
`Buffer` — which is Route 5 (buffer-native I/O) territory and not scheduled.
SHA-256 stays silicon-bound (~15–20×, no Wasm crypto intrinsics). Archived: the
shippable software win on this surface has been taken; the rest waits on Route 2/5.

Builds on the shipped, archived in-buffer-crypto work
([archive/in-buffer-crypto.md](archive/in-buffer-crypto.md)): `*_bytes` digests
already route through a transient linear-memory `Buffer` scratch (word loads +
in-place schedule), and `base64_decode` already dropped its per-char `Option`
allocation.

## How to measure

```bash
make bundle-cli                              # target/twk must reflect stdlib changes
bash examples/crypto-bench/run.sh            # twinkle + node + python + go, 4 KiB cases
```

Per-op µs is `(ms * 1000) / iters`. Compare the Twinkle `_bytes`/`_buf` rows
against the native runtimes. **Caveat:** at microsecond scale a chunk of the
measured time is fixed per-call overhead (the call, the `Digest` record alloc,
`to_bytes()[0]` in the bench), *not* throughput. For a real verdict on a route,
also bench a large (≥1 MiB) input where throughput dominates.

## Current state (4 KiB, µs/op, this machine)

| | default `_bytes` | `_buf` reused | best native | gap (reused vs native) |
|---|---|---|---|---|
| MD5 | ~64 | ~40 | ~4.5 | ~9× |
| SHA-1 | ~35 | ~14 | ~1.2 | ~11× |
| SHA-256 | ~74 | ~50 | ~1.2 | ~40× |
| base64 roundtrip | ~102 | — | ~1.9 | ~54× |

## The gap is two different things

The decisive fact: **native MD5 and base64 use no special hardware** — they are
just well-compiled C with registers and no bounds checks. **Native SHA-1 and
SHA-256 use CPU crypto instructions** (x86 SHA-NI — `sha256rnds2` runs two rounds
in one instruction; ARMv8 has equivalents). That is *why* native SHA-256 (~1.2µs)
matches native SHA-1 despite doing more work: the silicon does it.

Wasm has **no access to those instructions** — even the 128-bit Wasm SIMD proposal
(`v128`) does not include SHA/AES ops. So part of the gap is software overhead we
can close, and part is silicon we cannot reach. The closeable-ness ranking is the
*inverse* of the current gap:

| | native uses silicon? | realistic floor vs native | why |
|---|---|---|---|
| **MD5** | no | **~2–3×** | native is plain ALU; we can match the algorithm, not the codegen |
| **base64** | partly (SIMD) | **~5–10×** scalar, ~native with SIMD | table/SIMD codec |
| **SHA-1** | yes (SHA-NI) | **~3–5×** | partial hardware dependence |
| **SHA-256** | yes, heavily | **~15–20×** | mostly irreducible without Wasm crypto intrinsics (which don't exist) |

## Where our cycles go (grounded in the generated WAT)

Inspecting `sha256_buf`'s `digest_buf` in emitted WAT:

- **The round helpers are not inlined.** The hot function carries ~40 `call`
  instructions (`big_sigma0/1`, `small_sigma0/1`, `ch`, `maj`, `rotr32`, `u32`,
  `k`), so every one of the 64 rounds pays multiple function calls.
- **`u32()` masking is everywhere.** `Int` is i64 but SHA is 32-bit math, so
  nearly every operation carries an extra `& 0xffffffff`.
- The message reads and schedule are already buffer-backed (the in-buffer work),
  so the remaining cost is overwhelmingly the round arithmetic, not the I/O.

## Routes, in order of ROI

### Route 1 — Inline the hot loop + table the constants
**Effort: low. No new language features. Touches: `boot/stdlib/crypto/*.tw`.**
**Status: PROTOTYPED on MD5 (2026-06-26). The inlining premise did not hold; the
real lever turned out to be mask reduction. Not implemented — deferred pending a
decision on whether the measured ~9–13% is worth the churn on a legacy hash.**

The original idea: eliminate the per-round calls by inlining `rotl32`/`ch`/`maj`/
the sigmas/`round_f` into the round body, and replace `k(i)`/`s(i)` (function calls
over a `case`) with a single load from a constant table.

**What the MD5 prototype actually measured** (4 KiB, `digest_buf`, best-of-5,
relative to current; the runtime is V8 via `runtime.mjs`):

| change | speedup | note |
|---|---|---|
| inline helpers + 4-group unroll, keep `k()` | **~3%** | nearly nothing — V8 already inlines small wasm fns in hot loops |
| inline `rotl32` + drop one redundant `& 0xffffffff`/round | **~9%** | the surprise winner; pure arithmetic, no new state |
| + k-table (module-global `Buffer`, linear-memory load vs 64-arm `case`) | **~12%** | k-table adds ~3% over mask-drop alone |
| + 4-group unroll on top | **~13%** | unroll buys only ~1%, not worth the code |

The decisive correction: **inlining the round helpers is ~a no-op on a V8-class
engine** — the gap the WAT's `call` instructions implied is closed by the JIT, not
by us. The genuine software-only lever is **reducing the i64→u32 masking**:
addition's low 32 bits depend only on the operands' low 32 bits, so the rotate's
own mask can be folded into the final `& 0xffffffff` (3 masks/round → 2). The
k-table helps a little more by turning a branchy 64-arm `case` into one linear load
(needs a module-global `Buffer`, verified to initialize at import).

So Route 1's realistic ceiling is **~10–13%, not ~1.5–2.5×**, and it points at
Route 2 (drop the masking entirely via an i32-typed path) as the real arithmetic
lever. Re-derive the SHA estimates from this before scheduling them.

Risk: hand-inlining duplicates the round constants/logic that the helper functions
currently centralize — keep the functional `digest_bytes` as the equivalence
reference (already wired into the crypto suite) so a transcription slip is caught.
(The prototype cross-checked every variant byte-for-byte against `digest_buf`.)

### Route 2 — 32-bit-typed hot path
**Effort: medium. Language/backend lever. Ties into typed-vector/typed-arithmetic work.**

Do the round math in i32 to drop the pervasive i64→u32 masking. Worth
**~1.3–1.8×** on arithmetic-bound hashes (SHA-256 benefits most — it is the most
ALU-heavy). Depends on the backend exposing i32 arithmetic on a typed path; see
the typed-vector representation effort for the adjacent machinery.

### Route 3 — Buffer-native base64
**Effort: medium. Touches: `boot/stdlib/crypto.tw`, `boot/stdlib/buffer.tw`, the
boot/stage0 intrinsic surface.**
**Status: ENCODE LANDED via a new value-boundary bridge (2026-06-29); decode
remains. The table-driven shape was prototyped first and dropped (~3%); the real
lever was the missing `Buffer`→`String` bridge.**

The original idea (a `Buffer`-resident decode/encode lookup table, 4↔3 transform
through linear-memory loads/stores) was prototyped and **measured ~3% on the
roundtrip** — see the table below. The table is not the lever; the conversions
back to the GC API types (`String` builder, `Vector<Byte>` append, `from_utf8`
validation) are. Adding the missing bridge primitive is what paid off:

- **Shipped:** `String.from_mem(ptr, len)` — a GC intrinsic that copies a
  linear-memory byte range straight into a fresh `String` with **no UTF-8
  validation** (mirrors the bridge's `bulk_string_new`, but from `ptr+i` instead of
  a fixed scratch). Wrapped as `buffer.to_string(off, len)`. Plus `buffer.set_byte`
  (raw `__buf_store_u8`, skips the `Byte` box on the hot store). Boot codegen +
  stage0 (the stage0 path has no guest memory for buffers, so it lowers to a trap;
  the boot compiler never calls `to_string` at compile time, so this is sound).
- **`base64_encode` rewritten** to write ASCII into a `Buffer` and emit one
  `to_string`: **~50µs → ~30µs (~40%)**, roundtrip **~99µs → ~75µs (~24%)**. All
  crypto + boot tests green.
- **Decode untouched** — it builds a `Vector<Byte>` via `.append` and that loop now
  dominates the roundtrip. Closing it needs the *dual* bridge (bulk
  `Buffer`→`Vector<Byte>`, or a `base64_decode_buf` returning a `Buffer`), i.e.
  Route 5; not scheduled.

**What the prototype actually measured** (4 KiB, 501 iters, best-of, `target/twk`;
correctness cross-checked byte-for-byte against the current implementation,
including the 1- and 2-byte tails):

| variant | µs/op | vs current |
|---|---|---|
| **encode** current (per-char `cond` + `from_byte` + concat) | ~50 | — |
| **encode** 12-bit/2-char module-global string table, 2 concats/group | ~43 | **~15% faster** |
| **encode** build `Vector<Byte>` of ASCII + `String.from_utf8` once | ~106 | **2× slower** (re-confirms the trap below) |
| **decode** current (per-char `cond`, `.append` output) | ~41.5 | — |
| **decode** 256-entry `Buffer` lookup table replacing the `cond` | ~44 | **wash / slight regression** |
| **roundtrip** (encode+decode) current | ~97 | — |
| **roundtrip** table-driven (best encode + table decode) | ~94 | **~3%** |

The decisive findings:

- **Encode's only real lever is collapsing concats, not tabling the char map.** A
  12-bit→2-char table (one module-global `Vector<String>`, built once at import)
  emits two `concat`s per 3 bytes instead of four and is ~15% faster. Replacing the
  `b64_char` `cond` with a `Buffer` byte-load on its own does ~nothing — same story
  as Route 1's "inlining is a no-op."
- **Decode does not improve at all.** The cost is the `Vector<Byte>` `.append`
  output loop, not the `b64_value` `cond`; a `Buffer` lookup table is a wash-to-
  regression (the `get_u8` + `to_int` per char is no cheaper than the branch).
- **The buffer-native "single conversion at each end" shape is blocked by the API
  boundary.** There is no `Buffer`→`String` bridge — the only many-byte `String`
  constructor is `String.from_utf8(Vector<Byte>)`, which runs a full UTF-8
  validation pass and was measured at ~106µs (2× slower), exactly the trap recorded
  for the earlier `Vector<Byte>` reassembly attempt. So encode is stuck on the
  transient string builder and decode is stuck on `Vector<Byte>` append; linear
  memory can speed the *interior* transform but the conversions back to the API
  types dominate and erase the win.

The honest verdict: tabling *inside* the current API is a ~3% change, but adding
the missing `Buffer`→`String` bridge primitive turned the encode half into a real
~40% win (roundtrip ~24%) — and that primitive (`String.from_mem`/`buffer.to_string`)
is general, not base64-specific. The decode half still wants the dual bridge
(**Route 5**) or a typed path (**Route 2**) to drop its `Vector<Byte>` append.

### Route 4 — Wasm SIMD (`v128`) backend support
**Effort: large. Strategic. Touches: the backend (new `v128` type + intrinsics + codegen).**

Teach the Twinkle backend to emit 128-bit SIMD. It will **not** touch the SHA-NI
gap, but it unlocks **SIMD base64** (the technique node/Go use — could approach
native), vectorized message scheduling for the hashes, and broadly benefits the
whole language (bulk vector ops, memcpy, parsing). Highest effort, broadest payoff;
justify it on the language-wide win, not crypto alone.

### Route 5 — Buffer-native I/O (skip the copy)
**Effort: medium. Ecosystem direction, not hash-speed. Touches: `@std.fs` / sockets.**

Make file/socket reads return `Buffer` instead of `Vector<Byte>`, so hashing data
that came from disk/network never pays the ~19µs copy-in — the `_buf` path becomes
the natural one rather than an opt-in. This is plumbing, not arithmetic, but it is
where real workloads (hashing files) actually get their win.

## Explicitly out of reach

- **SHA/AES hardware instructions** — not in Wasm (not even in Wasm SIMD). This is
  the hard floor on SHA-256 (~15–20×) and part of SHA-1.
- **JIT-quality register allocation / no bounds checks** — native C keeps all
  working state in registers; we are at the mercy of the Wasm engine's codegen.

## Suggested sequencing

1. ~~Route 1 on MD5~~ DONE (prototyped, not implemented) — the inlining half is a
   no-op on V8; only the **mask-reduction** sub-lever (~9%) and k-table (~12% total)
   pay off. Not worth the churn on a legacy hash.
2. ~~Route 3 base64~~ ENCODE DONE — the table form was dropped (~3%); the
   `Buffer`→`String` bridge (`String.from_mem`/`buffer.to_string` + `buffer.set_byte`)
   landed encode at ~40% (roundtrip ~24%). Decode still wants the dual bridge
   (Route 5).
3. Route 2 once the typed-arithmetic path exists — this is where the masking the
   Route 1 prototype could only *reduce* gets dropped entirely; the real ALU lever.
4. Route 5 alongside any buffer-native I/O work — the home of the base64 **decode**
   win (bulk `Buffer`→`Vector<Byte>`), and the way the `_buf` digest path becomes
   the natural one for file/socket data.
5. Route 4 only when SIMD is wanted language-wide.

Realistic end state, corrected by the prototypes and the shipped bridge: tabling
*inside* the API is a dud (~10–13% per hash via Route 1; ~3% on base64), because the
JIT already inlines the helpers and the GC value-boundary work dominates. Killing
*one* boundary with a bridge primitive is what pays — base64 **encode ~40%** once
`String.from_mem` existed. The rest of the gap is more boundaries: base64 decode
(Route 5 dual bridge) and the hashes (Route 2 typed path). **MD5 stays ~8–9×**,
**base64 roundtrip now ~30–35×** (was ~45×) and drops further only when decode gets
its bridge; **SHA-256 stays ~15–20×** until Wasm gains crypto intrinsics that do not
currently exist.
