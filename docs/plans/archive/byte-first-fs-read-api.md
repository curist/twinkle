# Byte-First File Read API & Host Contract

**Status:** Implemented (Phase 1 scope)  
**Last updated:** 2026-03-10

## Goal

Align file-reading APIs with Twinkle's byte-first model:

* `Byte` is explicit.
* `String` is UTF-8 data with explicit decoding boundaries.
* Host runtime exposes raw bytes, not text policy.

Target layering:

```text
host.read_file(path) -> Result<Vector<Byte>, String>
        ↓
@std.fs.read_bytes(path)
        ↓
@std.fs.read_text(path) via String.from_utf8(...)
```

## Why this change

Current behavior still decodes in the host (`read_to_string`), which:

* bakes UTF-8 policy into the host boundary,
* rejects non-UTF-8 files before Twinkle code can decide behavior,
* conflicts with byte-first semantics introduced by `Byte` and updated string APIs.

## Previous state (mismatch summary)

Before this change, `__host_read_file` was treated as text:

* type env: `fn(String) -> String`
* prelude import signature: `host.read_file: (ref $String) -> (ref $String)`
* Wasmtime host implementation uses `std::fs::read_to_string`
* `@std.fs.read_text` directly forwards host output

## Target API and ABI

### Host import contract

Primary primitive:

```tw
host.read_file(path: String) -> Vector<Byte>!String
```

Wasm GC shape:

* param: runtime string ref
* result: runtime variant ref (`ref $Variant`) carrying `Result<Vector<Byte>, String>`

### Stdlib contract

`@std.fs` exposes:

```tw
pub fn read_bytes(path: String) Vector<Byte>!FsError
pub fn read_text(path: String) String!FsError
```

`read_text` performs UTF-8 decode in Twinkle:

* `read_bytes(path)` first
* `String.from_utf8(bytes)`
* decode failure maps to an fs error variant (recommended: `InvalidUtf8`)

Optional future convenience:

```tw
pub fn read_text_lossy(path: String) String!FsError
```

## Implementation plan

### Phase A: ABI/type plumbing

1. Change internal host builtin type:
   * `__host_read_file: fn(String) -> Vector<Byte>!String`
2. Update prelude runtime import signature for `HOST_READ_FILE` to return runtime variant ref.
3. Update host ABI documentation to declare `host.read_file` as bytes.

Files:

* `src/types/env.rs`
* `src/ir/lower.rs` (comments/signature docs)
* `src/codegen/prelude.rs`
* `docs/internals/host-abi.md`

### Phase B: Wasmtime host behavior

1. Replace `std::fs::read_to_string` with `std::fs::read`.
2. Encode host return as `Result` variant:
   * `.Ok(Vector<Byte>)` on success
   * `.Err(String)` on I/O failure
3. Keep existing path resolution/sandbox behavior unchanged.

Files:

* `src/cli/run_wasm.rs`

### Phase C: stdlib API layering

1. Add `fs.read_bytes` as direct host wrapper.
2. Refactor `fs.read_text` to decode via `String.from_utf8`.
3. Add/adjust fs error variant for decode failure (`InvalidUtf8` recommended).

Files:

* `stdlib/fs.tw`
* `docs/API.md`
* `docs/design/stdlib.md`

### Phase D: validation and tests

1. Update host-import signature tests to expect variant result for `read_file`.
2. Extend stdlib fs wasm test:
   * verify `read_text` for UTF-8 text file,
   * verify `read_bytes` for binary file.
3. Add negative case (invalid UTF-8 + `read_text` returns decode error) once fs error variant is added.

Files:

* `src/cli/run_wasm.rs` (unit tests in module)
* `tests/stdlib_fs_wasm_test.rs`

## Acceptance criteria

1. No host-side UTF-8 decoding during file reads.
2. `host.read_file` returns `Result<Vector<Byte>, String>` and does not trap for ordinary I/O failures.
3. `@std.fs.read_text` is implemented as byte decode adapter.
4. Host ABI docs and public API docs match implementation.
5. Existing text read behavior remains correct for valid UTF-8 files.

## Compatibility and migration notes

* Source compatibility can be preserved by keeping `fs.read_text` stable while adding `fs.read_bytes`.
* `fs.write_bytes` can remain unchanged in this plan and be harmonized to `Vector<Byte>` in a separate follow-up.
* Adding `FsError.InvalidUtf8` is additive at type-definition level but may require exhaustive `case` updates in downstream code.

## Out of scope

* Streaming file APIs (`open/read/close`).
* JSON/TOML/text convenience readers.

## Follow-up candidates

* Add lossy UTF-8 decode helper in stdlib.
* Unify byte-vector APIs (`write_bytes` + potential append/stream primitives).
