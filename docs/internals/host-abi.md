# Host ABI Reference

Twinkle compiles to WebAssembly GC modules that import host functions from two
namespaces. Any host (Wasmtime, browser, Node.js) must provide the imports a
given module actually references to run it.

- **`"host"`** — a small fixed set of core runtime builtins (console I/O) and
  pure helpers (float formatting/parsing) emitted directly by the compiler.
  These are not user-declarable; the compiler emits them as needed.
- **`"twinkle_runtime"`** — the OS / process / filesystem / stdin capability
  surface. These are declared in stdlib source with `extern twinkle_runtime { … }`
  (see `@std.fs`, `@std.io`, `@std.proc`, `@std.time`) and auto-bridged by the
  JS runtime. A non-JS host provides them the same way it provides `host.*`.

The reference implementations live in `tools/js_runtime/runtime.mjs` (JS) and
`src/cli/run_wasm.rs` (the Rust reference host).

> Historical note: the OS functions below were previously compiler-internal
> `__host_*` intrinsics that imported under `host.*`. They are now ordinary
> `extern twinkle_runtime` declarations; the `__host_*` surface no longer exists.

---

## Declaring runtime imports (`extern twinkle_runtime`)

stdlib modules declare runtime capabilities directly:

```tw
extern twinkle_runtime {
  fn read_file(path: String) Vector<Byte>!String
  fn write_file(path: String, text: String) Void
  fn exit(code: Int) Never
}
```

The compiler maps each extern parameter/result to its Wasm boundary type and,
where a parameter or result is a `Vector<…>`, inserts the `$PVec`↔`$Array`
conversions (`rt_arr__to_array` / `rt_arr__from_array`) around the call so the
host always sees the flat `$Array` representation. `Vector<Byte>!String`
(the `read_file` shape) crosses as a `$Variant` and is rebuilt into the typed
`Result` after the call. Diverging fns may be declared `Never` (emits no result).

The JS runtime exposes `twinkle_runtime.*` from the same implementations that back
`host.*` (see `makeHostImports`), so a fn like `read_file` is shared, not
duplicated. The generic extern bridge can also marshal `bytes`/`strvec`/`readfile`
kinds for *externally-provided* imports, but the stdlib pre-populates these
entries, so it deliberately bypasses that path (see `bridgeExternImports`).

---

## Type Conventions

All string arguments and return values use the runtime string type
(`ref null $rt_types__String`), which is `(array (mut i8))` — a mutable byte
array holding UTF-8 data.

Array values use `ref null $rt_types__Array` (`(array (mut anyref))`).

---

## `host.*` — core builtins and pure helpers

These are emitted by the compiler (not declared in source) and remain on the
`host` namespace.

### Console I/O

| Import | Signature | Description |
|---|---|---|
| `host.print` | `(ref null $String) → ()` | Write string to stdout (no newline) |
| `host.println` | `(ref null $String) → ()` | Write string to stdout with newline |
| `host.eprint` | `(ref null $String) → ()` | Write string to stderr (no newline) |
| `host.eprintln` | `(ref null $String) → ()` | Write string to stderr with newline |
| `host.error` | `(ref null $String) → ()` | Trap with error message (must not return) |

Source: `src/runtime/core.rs`

### String Conversion

| Import | Signature | Description |
|---|---|---|
| `host.f64_to_string` | `(f64) → (ref $String)` | Format a float as a decimal string |

Needed because float-to-string formatting is complex to implement in pure Wasm.
Integer and boolean conversions are handled entirely in the runtime module (`rt_str`).

Source: `src/runtime/str.rs`

### Numeric Parsing

| Import | Signature | Description |
|---|---|---|
| `host.parse_float` | `(ref null $String) → (f64, i32)` | Parse float from string; returns `(value, ok)` where `ok=1` on success |

The public APIs are `Int.from_string` and `Float.from_string`. Integer parsing is
implemented in pure Wasm (no host import needed); float parsing delegates here
because decimals, exponents, and special values are impractical in inline Wasm.

Source: `src/codegen/emit.rs` (`ensure_host_parse_float_import`)

---

## `twinkle_runtime.*` — OS / process / filesystem / stdin

Declared via `extern twinkle_runtime` in the stdlib. The JS runtime provides
these in `makeHostImports`.

### File I/O

| Import | Signature | Description |
|---|---|---|
| `twinkle_runtime.read_file` | `(ref null $String) → (ref null $Variant)` | Read file as bytes; returns `Result<Vector<Byte>, String>` |
| `twinkle_runtime.write_file` | `(ref null $String, ref null $String) → ()` | Write string to file (path, content) |
| `twinkle_runtime.write_bytes` | `(ref null $String, ref null $Array) → ()` | Write byte array to file (path, bytes) |
| `twinkle_runtime.mkdirp` | `(ref null $String) → ()` | Create directory and parents |
| `twinkle_runtime.list_dir` | `(ref null $String) → (ref $Array)` | List directory entries as array of strings |
| `twinkle_runtime.exists` | `(ref null $String) → (i32)` | Check if path exists (1=yes, 0=no) |

### Buffer (linear-memory) file I/O

Used by `@std.fs`'s `read_buffer`/`write_buffer` to move bytes directly through
linear memory. `ptr`/`len` index the guest's exported memory (or a host-side
shim when the guest exports none).

| Import | Signature | Description |
|---|---|---|
| `twinkle_runtime.read_buffer_len_raw` | `(ref null $String) → (i64)` | File length in bytes, or `-1` if missing |
| `twinkle_runtime.read_buffer_raw` | `(ref null $String, i64, i64) → (i64)` | Read file into `[ptr, ptr+len)`; returns bytes read or `-1` |
| `twinkle_runtime.write_buffer_raw` | `(ref null $String, i64, i64) → ()` | Write `[ptr, ptr+len)` to file |

### Process

| Import | Signature | Description |
|---|---|---|
| `twinkle_runtime.args` | `() → (ref $Array)` | Command-line arguments as array of strings |
| `twinkle_runtime.env` | `(ref null $String) → (ref $Array)` | Get environment variable; returns 1-element array or empty |
| `twinkle_runtime.cwd` | `() → (ref $String)` | Current working directory |
| `twinkle_runtime.exit` | `(i64) → ()` | Exit process with given code (declared `Never`; must not return) |
| `twinkle_runtime.now` | `() → (f64)` | Milliseconds since the runtime time origin |
| `twinkle_runtime.run_wasm` | `(ref null $Array, ref null $Array) → (i64)` | Run a child Wasm module with argv; returns its exit code |

### Stdin / stdout (byte streams)

| Import | Signature | Description |
|---|---|---|
| `twinkle_runtime.stdin_read_chunk` | `(i64) → (ref $Array)` | Read up to `max_bytes`; empty array at EOF |
| `twinkle_runtime.stdin_read_timeout` | `(i64, i64) → (ref $Array)` | Read up to `max_bytes`, waiting at most `timeout_ms` |
| `twinkle_runtime.stdin_eof` | `() → (i32)` | True (1) after stdin reaches EOF |
| `twinkle_runtime.stdout_write_bytes` | `(ref null $Array) → ()` | Write raw bytes to stdout |

**Async (JSPI):** `sleep`, `stdin_read_chunk`, `stdin_read_timeout`, and
`run_wasm` are Promise-suspending under the JSPI runtime. The JS runtime installs
their suspending implementations after the extern imports are bridged and
re-points the `twinkle_runtime` aliases at them (see the `hasJspi` block in
`runtime.mjs`). Under a synchronous runtime, `sleep` traps (sleeping requires the
async runtime).

| Import | Signature | Description |
|---|---|---|
| `twinkle_runtime.sleep` | `(i64) → ()` | Suspend for at least `ms` milliseconds (JSPI only) |

---

## Notes for Host Implementors

- **Conditional imports:** Not all programs import all host functions. The host
  only needs to provide functions that the specific module actually imports.
  Console I/O and `f64_to_string` are always present (from the runtime modules);
  `twinkle_runtime.*` imports appear only when the corresponding stdlib APIs are
  used.

- **`host.error` must not return.** It should trap/abort the Wasm instance. The
  same applies to `twinkle_runtime.exit`.

- **`twinkle_runtime.env` returns an array**, not a nullable string, to avoid
  needing Option encoding at the host boundary. Empty array = not set.

- **`twinkle_runtime.read_file` should not trap for ordinary I/O failures.**
  Return `Result.Err(String)` instead. `Result.Ok` carries `Vector<Byte>`; hosts
  should avoid implicit UTF-8 decoding at this boundary.

- **`host.parse_float` uses multi-value return:** `(f64, i32)` where `i32` is
  1 on success, 0 on failure. On failure the `f64` value is unspecified.
