# Host ABI Reference

Twinkle compiles to WebAssembly GC modules that import a set of host functions
under the `"host"` namespace. Any host (Wasmtime, browser, Node.js) must provide
these imports to run a compiled `.wasm` module.

The reference implementation lives in `src/cli/run_wasm.rs`.

---

## Compiler Internal `__host_*` Intrinsics

`__host_*` names are Twinkle internal builtins used by stdlib modules (`@std.fs`, `@std.proc`).
They map 1:1 to host imports:

| Internal Intrinsic | Host Import | Twinkle Type |
|---|---|---|
| `__host_read_file` | `host.read_file` | `fn(String) Vector<Byte>!String` |
| `__host_write_file` | `host.write_file` | `fn(String, String) Void` |
| `__host_write_bytes` | `host.write_bytes` | `fn(String, Vector<Int>) Void` |
| `__host_mkdirp` | `host.mkdirp` | `fn(String) Void` |
| `__host_list_dir` | `host.list_dir` | `fn(String) Vector<String>` |
| `__host_exists` | `host.exists` | `fn(String) Bool` |
| `__host_args` | `host.args` | `fn() Vector<String>` |
| `__host_env` | `host.env` | `fn(String) Vector<String>` |
| `__host_cwd` | `host.cwd` | `fn() String` |
| `__host_exit` | `host.exit` | `fn(Int) Never` |

Note: this list is only the stdlib bridge layer. The full host import surface also includes runtime imports such as `host.print` and `host.f64_to_string`.

---

## Type Conventions

All string arguments and return values use the runtime string type
(`ref null $rt_types__String`), which is `(array (mut i8))` — a mutable byte
array holding UTF-8 data.

Array values use `ref null $rt_types__Array` (`(array (mut anyref))`).

---

## Console I/O

| Import | Signature | Description |
|---|---|---|
| `host.print` | `(ref null $String) → ()` | Write string to stdout (no newline) |
| `host.println` | `(ref null $String) → ()` | Write string to stdout with newline |
| `host.eprint` | `(ref null $String) → ()` | Write string to stderr (no newline) |
| `host.eprintln` | `(ref null $String) → ()` | Write string to stderr with newline |
| `host.error` | `(ref null $String) → ()` | Trap with error message (must not return) |

Source: `src/runtime/core.rs`

---

## String Conversion

| Import | Signature | Description |
|---|---|---|
| `host.f64_to_string` | `(f64) → (ref $String)` | Format a float as a decimal string |

Needed because float-to-string formatting is complex to implement in pure Wasm.
Integer and boolean conversions are handled entirely in the runtime module (`rt_str`).

Source: `src/runtime/str.rs`

---

## Numeric Parsing

| Import | Signature | Description |
|---|---|---|
| `host.parse_float` | `(ref null $String) → (f64, i32)` | Parse float from string; returns `(value, ok)` where `ok=1` on success |

The public APIs are `Int.from_string` and `Float.from_string`.
Internally, integer parsing is implemented in pure Wasm (no host import needed),
while float parsing delegates to this host function because decimals, exponents,
and special values are impractical in inline Wasm.

Source: `src/codegen/emit.rs` (`ensure_host_parse_float_import`)

---

## File I/O

| Import | Signature | Description |
|---|---|---|
| `host.read_file` | `(ref null $String) → (ref null $Variant)` | Read file as bytes and return `Result<Vector<Byte>, String>` |
| `host.write_file` | `(ref null $String, ref null $String) → ()` | Write string to file (path, content) |
| `host.write_bytes` | `(ref null $String, ref null $Array) → ()` | Write byte array to file (path, bytes) |
| `host.mkdirp` | `(ref null $String) → ()` | Create directory and parents |
| `host.list_dir` | `(ref null $String) → (ref $Array)` | List directory entries as array of strings |
| `host.exists` | `(ref null $String) → (i32)` | Check if path exists (1=yes, 0=no) |

Source: `src/codegen/prelude.rs`

---

## Process

| Import | Signature | Description |
|---|---|---|
| `host.args` | `() → (ref $Array)` | Command-line arguments as array of strings |
| `host.env` | `(ref null $String) → (ref $Array)` | Get environment variable; returns 1-element array or empty |
| `host.cwd` | `() → (ref $String)` | Current working directory |
| `host.exit` | `(i64) → ()` | Exit process with given code |

Source: `src/codegen/prelude.rs`

---

## Notes for Host Implementors

- **Conditional imports:** Not all programs import all host functions. The host
  only needs to provide functions that the specific module actually imports.
  Console I/O and `f64_to_string` are always present (from the runtime modules).
  File I/O and process imports are only present when used.

- **`host.parse_int` is currently accepted by the reference host linker but is
  not emitted by the current compiler pipeline.** You generally do not need to
  provide it unless you are running hand-written/legacy Wasm modules.

- **`host.error` must not return.** It should trap/abort the Wasm instance.

- **`host.env` returns an array**, not a nullable string, to avoid needing
  Option encoding at the host boundary. Empty array = not set.

- **`host.read_file` should not trap for ordinary I/O failures.** Return
  `Result.Err(String)` instead. `Result.Ok` carries `Vector<Byte>`; hosts should
  avoid implicit UTF-8 decoding at this boundary.

- **`host.parse_float` uses multi-value return:** `(f64, i32)` where `i32` is
  1 on success, 0 on failure. On failure the `f64` value is unspecified.
