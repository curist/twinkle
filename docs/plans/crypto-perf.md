# Crypto Performance — Routes to Native Speed

Status: **ROUTES / RESEARCH (not scheduled).** A living catalogue of the directions
that could make `@std.crypto` (and the related codecs) faster, with grounded
measurements, realistic ceilings, and the effort/payoff of each route. Pick items
from here into their own implementation plans when scheduled.

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

### Route 3 — Buffer-native, table-driven base64
**Effort: medium. Self-contained. Touches: `boot/stdlib/crypto.tw` (+ a `Buffer`).**

A 256-entry decode lookup table and an encode table living in a `Buffer`, with the
4↔3 byte transform done through linear-memory loads/stores and a single conversion
at each end (input `String`→bytes once, output bytes→`String` once). Scalar
table-driven base64 is the standard fast technique.

Expected **base64 roundtrip ~102µs → ~15–25µs** (~5–10× of native). Good standalone
project. **Note the trap already learned:** do *not* rebuild `base64_encode` as a
`Vector<Byte>` + `String.from_utf8` reassembly — that measured *slower* (54→91µs)
because the existing `out.concat(...)` loop already lowers to the transient string
builder and `from_utf8` adds a full UTF-8 validation pass. A buffer + table is a
different, genuinely faster shape; re-measure encode specifically.

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

1. ~~Route 1 on MD5~~ DONE (prototyped) — the inlining half is a no-op on V8; only
   the **mask-reduction** sub-lever (~9%) and k-table (~12% total) pay off. Roll the
   mask-reduction idea to SHA-1/SHA-256 only if a ~10% win there is worth the churn.
2. Route 3 base64 (self-contained, large relative win).
3. Route 2 once the typed-arithmetic path exists — this is where the masking the
   Route 1 prototype could only *reduce* gets dropped entirely; the real ALU lever.
4. Route 5 alongside any buffer-native I/O work.
5. Route 4 only when SIMD is wanted language-wide.

Realistic end state, corrected by the MD5 prototype: software-only inlining/tabling
buys only **~10–13%** per hash (the round helpers are already JIT-inlined), so
**MD5 stays ~8–9×** of native without the i32-typed path (Route 2); **base64
~5–10×** via Route 3; **SHA-256 stays ~15–20×** until Wasm gains crypto intrinsics
that do not currently exist.
