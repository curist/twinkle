# Fast `fs.read_text` via a direct-to-String host op

## Summary

Reading a file as text currently routes disk bytes through a `Vector<Byte>`
persistent trie before decoding to a `String`. Building that trie over millions
of bytes is the dominant cost — not disk I/O, not UTF-8 validation. Adding a
privileged `twinkle_runtime.read_file_string` host op that reads a file in one
call and returns a GC `String` directly (validated in the host) skips the trie
entirely and makes `fs.read_text` ~4× faster, for the compiler frontend, the
LSP, `fmt`, `lint`, and every user program.

## Motivation and measurements

The compiler loads source with `fs.read_text`, whose path is:

```
host readFile → bulk_bytes_new (flat $Array)
             → rt_arr__from_read_file_result (rebuild flat $Array into a $PVec)   ← Vector<Byte>
             → String.from_utf8 (validate + walk the PVec into a String)
```

Spikes reading the whole boot tree (663 files, ~6 MB, average of 5 iterations,
in-Twinkle via `target/twk run`):

| Path | What it does | ms/iter |
|------|--------------|--------:|
| A — `read_text` | `Vector<Byte>` + `from_utf8` (current) | 99.7 |
| D — `read_bytes` only | `Vector<Byte>`, no validation | 89.2 |
| B — `read_buffer` + `to_string`/`from_mem` | disk → linear mem → String | 24.5 |
| E — `read_buffer` only | disk → linear mem | 22.5 |

Pure host-side floor (JS `readFileSync` + one bridge op, no Twinkle):

| Op | ms/iter |
|----|--------:|
| → `bulk_string_new` (String) | 6.2 |
| → `bulk_bytes_new` (`Vector<Byte>`) | 7.2 |

Attribution: disk + bridge floor ~6–7 ms; `from_utf8` only ~10 ms (A − D); the
**`Vector<Byte>` trie construction is ~65 ms (D − E)**. The bridge op itself is
cheap (7 ms host-side) — the ~82 ms in-Twinkle gap for the byte path is the
`rt_arr__from_read_file_result` flat-`$Array`→`$PVec` rebuild. Any path that
produces a `String` directly and never materializes `Vector<Byte>` wins ~4×.

JSPI / async is not the lever here: the cost is CPU-bound marshaling, not blocked
I/O.

### Honest magnitude

The compiler wall is ~20 s, dominated (~13.5 s) by the two sound-uniqueness
ownership phases; the frontend `load` phase is ~159 ms (disk reads plus the
embedded `core_lib` strings). This change trims roughly ~75 ms — about 0.4 % of
the compiler wall. Its real value is the ~4× speedup on *every* file read across
the LSP, `fmt`, `lint`, and user programs, delivered by a small self-contained
change.

## Design

Add `twinkle_runtime.read_file_string(path: String) String!String`, a privileged
host extern in the same family as `read_file`. It reads the file in a single
call, validates UTF-8 in the host, and returns the text as a GC `String`.

Because the return is `Result<String, String>`, the Ok payload is already a GC
`String` — it needs **no** `from_read_file_result`-style rewrite and passes
straight through `emit_host_result_shim` (the generic host-call path returns
`buf` unchanged for anything that is neither vector-returning nor `read_file`).
This is both the source of the win and what keeps the codegen change minimal.

`fs.read_text` becomes a thin wrapper over the new op. `fs.read_bytes` (and the
`Vector<Byte>` path) stays for binary callers. The typed
`FsError.InvalidUtf8` distinction is preserved via a fixed `"invalid-utf8"`
error token the host returns and `fs.read_text` recognizes; any other host error
maps to `FsError.Other`.

### Error protocol

The host op returns `String!String`, so the failure channel is a single string.
`fs.read_text` maps it back to the typed `FsError`:

- host invalid-UTF-8 → `.Err("invalid-utf8")` → `FsError.InvalidUtf8`
- host I/O error → `.Err("<message>")` → `FsError.Other(message)`

The `"invalid-utf8"` token is an internal protocol between the host op and its
sole wrapper (`fs.read_text`); it is not part of the public surface.

## Wiring surface

Both compilers are touched and reconcile by canonical name (boot is primary;
stage0 is the correctness mirror and must at least *compile* the op, since boot
source uses `fs.read_text` and the bootstrap compiles boot through stage0).

**Host runtime**
- `tools/js_runtime/runtime.mjs`: add `read_file_string` to the
  `twinkle_runtime` object — `runtime.host.readFile` (already provided by every
  host adapter), then `TextDecoder({ fatal: true })` to validate; on success copy
  bytes to linear memory and `bulk_string_new` → `makeResultOk`; on decode
  failure `makeResultErr(b, encodeString(b, "invalid-utf8"))`; on I/O error
  `makeResultErr` with the exception message. No per-adapter (`node_host`,
  `web`, `deno_main`) changes needed.

**Boot compiler**
- `boot/compiler/resolver.tw` — `is_extern_safe_type`: add
  `.Result(.String, .String) => true` (currently only
  `Result<Vector<Byte>, String>` is allowlisted, which is why a `String!String`
  extern is rejected today).
- `boot/compiler/builtins.tw` — register
  `rt("host_read_file_string", "twinkle_runtime", "read_file_string", .None)`
  and add the ABI arm `"host_read_file_string" => abi([str_n()], [variant_null()])`
  (identical shape to `host_read_file`).

**Stage0 (Rust) mirror**
- `src/types/resolve.rs` — `validate_extern_safe_type`: allow
  `Result<String, String>`.
- Stage0 builtin registration + ABI for `host_read_file_string`, mirroring
  `host_read_file` but **without** the `from_read_file_result` conversion (the
  String payload needs no rewrite).

**stdlib + docs**
- `boot/stdlib/fs.tw` — declare the extern; rewrite `read_text` to call it and
  apply the error mapping above. Keep `read_bytes` unchanged.
- Regenerate `core_lib` (`python3 tools/generate_core_lib.py`) — `fs.tw` is
  embedded.
- `docs/API.md` — update the `read_text` row to note it no longer round-trips
  through `Vector<Byte>`; `read_bytes` remains for raw bytes.

## Implementation phases

Each phase ends green before the next begins. **Ordering is bootstrap-driven:**
`make stage2` starts with stage0 (Rust) compiling boot source → stage1
(`Makefile`), and `fs.tw` (embedded in `core_lib`, imported by the compiler)
will declare/use the new extern. So stage0 must accept the extern *before* the
`fs.tw` usage is compiled — stage0 is a hard prerequisite, not a follow-on
mirror. Append the new builtin at the end of its registry so existing FuncIds do
not renumber.

1. **Host op.** Add `read_file_string` to `runtime.mjs` (single `readFile` +
   `TextDecoder({fatal:true})` + `bulk_string_new`, error-token protocol). The
   host-side floor is already spiked (~6 ms); this phase just lands the op.
2. **Stage0 (Rust) — prerequisite.** `src/types/resolve.rs`
   `validate_extern_safe_type` allows `Result<String,String>`; stage0 builtin
   registration + ABI mirroring `host_read_file` **without** the
   `from_read_file_result` conversion. `cargo build --release`. This phase
   validates the one open codegen question end-to-end: **a variant-returning host
   extern with no payload rewrite emits and casts correctly at the call
   boundary** (`read_file` is the only variant-returning host op today and it is
   special-cased). Prove it by compiling a small `.tw` that declares the extern
   (in a stdlib-hosted module, since user-file externs are gated to primitive
   returns) with stage0 and running the emitted wasm on the JS runtime.
3. **Boot compiler wiring.** `boot/compiler/resolver.tw` `is_extern_safe_type`
   adds `.Result(.String, .String) => true`; `boot/compiler/builtins.tw`
   registers `host_read_file_string` + ABI. Mirror any call-boundary emit tweak
   found in phase 2.
4. **stdlib rewrite.** `boot/stdlib/fs.tw`: declare the extern; rewrite
   `read_text` + error mapping; regenerate `core_lib`
   (`python3 tools/generate_core_lib.py`); `fmt` + `lint` `fs.tw`.
5. **Full gate + docs.** `make stage2` (fixed point reached), `make bundle-cli`,
   `make boot-test`, targeted `cargo test`, `docs/API.md`. Re-run the read spike
   to confirm the load-path drop end-to-end.

## Verification

`boot.wasm` is **not** byte-identical here — `read_text`'s body changes and the
embedded `fs.tw` source string changes — so the gate is behavioral, not diff:

- **Self-host fixed point**: `make stage2` reaches stage3 == stage4 ("Fixed
  point reached"), proving the compiler still compiles itself after the change.
- **Suites**: `make boot-test`; targeted Rust tests for the stage0 extern-safety
  and host-ABI changes.
- **Functional correctness**: a `.tw` spike reads a known file and gets the exact
  content; a file with invalid UTF-8 yields `FsError.InvalidUtf8`; a missing file
  yields `FsError.Other`/`NotFound`.
- **Spike re-run**: the boot-tree read spike shows `read_text` dropping from
  ~100 ms toward the ~6–24 ms range.

## Risks / open items

- **Call-boundary cast for a plain variant return** (phase 1). `read_file` is the
  only existing variant-returning host op and it is special-cased; the generic
  path may need a small cast addition. De-risked in phase 1 before any deeper
  wiring.
- **Error-token coupling.** `fs.read_text` recognizes the `"invalid-utf8"`
  token; documented as an internal host↔wrapper protocol. Acceptable given the
  op has a single wrapper caller.

## Relationship to other plans

Complementary to `mutvec-later-slices.md` Phase 4 (`PVecByte`), which unboxes the
`Vector<Byte>` result of `read_bytes` for **binary** reads. This plan removes
`Vector<Byte>` from the **text** path entirely; the two do not overlap and can
land independently.
