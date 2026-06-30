# Host ABI Reference

Twinkle compiles to WebAssembly GC modules that import every host capability from
a single namespace, **`"twinkle_runtime"`**. Any host (browser, Node.js, Deno,
Wasmtime) must provide the imports a given module actually references to run it.
A module only imports what it uses, so most programs reference a small subset.

The `twinkle_runtime` namespace holds two kinds of functions, but they share one
import module:

- **Core builtins** — console I/O, float formatting, and numeric parsing — are
  emitted directly by the compiler (from `rt.core`, `rt.str`, and the intrinsics
  module). They are not user-declarable. The stage0 (Rust) compiler also emits a
  host-side linear-memory buffer shim (`buf_*`) here; the boot compiler
  implements those in Wasm instead.
- **OS / process / filesystem / stdin capabilities** are declared in stdlib
  source with `extern twinkle_runtime { … }` (see `@std.fs`, `@std.io`,
  `@std.proc`, `@std.time`) and auto-bridged by the JS runtime.

A separate `"task"` namespace carries the cooperative-concurrency intrinsics
(`task_create`, `suspend_await`, `channel_*`, …) and appears only when a program
uses `Task`/`Channel`; it is provided by the JSPI scheduler in the JS runtime.

The reference host implementation lives in `tools/js_runtime/runtime.mjs`
(`makeHostImports`). The Rust compiler only builds Wasm; it does not run it, so
there is no Rust reference host.

> Historical note: OS functions were once compiler-internal `__host_*`
> intrinsics, and core builtins imported under a separate `"host"` module. Both
> are gone — every host import is now an ordinary `twinkle_runtime` symbol.

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

Extern functions must declare a return type (there is no body to infer one from);
use `Void` for fire-and-forget calls.

The compiler maps each extern parameter/result to its Wasm boundary type and,
where a parameter or result is a `Vector<…>`, inserts the `$PVec`↔`$Array`
conversions (`rt_arr__to_array` / `rt_arr__from_array`) around the call so the
host always sees the flat `$Array` representation. `Vector<Byte>!String`
(the `read_file` shape) crosses as a `$Variant` and is rebuilt into the typed
`Result` after the call. Diverging fns may be declared `Never` (emits no result).

The JS runtime provides all of `twinkle_runtime.*` from `makeHostImports`. The
generic extern bridge can also marshal `bytes`/`strvec`/`readfile` kinds for
*externally-provided* imports, but `makeHostImports` pre-populates these entries,
so it deliberately bypasses that path (see `bridgeExternImports`).

---

## Type Conventions

All string arguments and return values use the runtime string type
(`ref null $rt_types__String`), which is `(array (mut i8))` — a mutable byte
array holding UTF-8 data.

Array values use `ref null $rt_types__Array` (`(array (mut anyref))`).

---

## Core builtins

Emitted by the compiler (not declared in source).

### Console I/O

| Import | Signature | Description |
|---|---|---|
| `twinkle_runtime.print` | `(ref null $String) → ()` | Write string to stdout (no newline) |
| `twinkle_runtime.println` | `(ref null $String) → ()` | Write string to stdout with newline |
| `twinkle_runtime.eprint` | `(ref null $String) → ()` | Write string to stderr (no newline) |
| `twinkle_runtime.eprintln` | `(ref null $String) → ()` | Write string to stderr with newline |
| `twinkle_runtime.error` | `(ref null $String) → ()` | Trap with error message (must not return) |

Source: `src/runtime/core.rs` (stage0), `boot/compiler/codegen/runtime/core.tw` (boot)

### String conversion

| Import | Signature | Description |
|---|---|---|
| `twinkle_runtime.f64_to_string` | `(f64) → (ref $String)` | Format a float as a decimal string |

Needed because float-to-string formatting is complex to implement in pure Wasm.
Integer and boolean conversions are handled entirely in the runtime module (`rt_str`).

### Numeric parsing

| Import | Signature | Description |
|---|---|---|
| `twinkle_runtime.parse_float` | `(ref null $String) → (f64, i32)` | Parse float from string; returns `(value, ok)` where `ok=1` on success |

The public APIs are `Int.from_string` and `Float.from_string`. Integer parsing is
implemented in pure Wasm (no host import needed); float parsing delegates here
because decimals, exponents, and special values are impractical in inline Wasm.

### Linear-memory buffer shim (stage0 only)

The stage0 (Rust) compiler emits `buf_alloc` / `buf_free` / `buf_load_*` /
`buf_store_*` as host imports, used only when the guest exports no memory of its
own. The boot compiler implements these as ordinary Wasm functions, so
boot-compiled programs never import them.

---

## OS / process / filesystem / stdin

Declared via `extern twinkle_runtime` in the stdlib.

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
| `twinkle_runtime.stderr_write_bytes` | `(ref null $Array) → ()` | Write raw bytes to stderr |
| `twinkle_runtime.is_terminal` | `(i64) → (i32)` | True (1) when standard fd `0`, `1`, or `2` is attached to a terminal |

**Async (JSPI):** `sleep`, `stdin_read_chunk`, `stdin_read_timeout`, and
`run_wasm` are Promise-suspending under the JSPI runtime. The JS runtime installs
their suspending implementations on `twinkle_runtime` after the extern imports are
bridged (see the `hasJspi` block in `runtime.mjs`). Under a synchronous runtime,
`sleep` traps (sleeping requires the async runtime).

| Import | Signature | Description |
|---|---|---|
| `twinkle_runtime.sleep` | `(i64) → ()` | Suspend for at least `ms` milliseconds (JSPI only) |

---

## Notes for Host Implementors

- **Conditional imports:** Not all programs import all functions. The host only
  needs to provide functions the specific module actually imports. Console I/O and
  `f64_to_string` are present in nearly every program; `twinkle_runtime.*` OS
  imports appear only when the corresponding stdlib APIs are used.

- **`twinkle_runtime.error` must not return.** It should trap/abort the Wasm
  instance. The same applies to `twinkle_runtime.exit`.

- **`twinkle_runtime.env` returns an array**, not a nullable string, to avoid
  needing Option encoding at the host boundary. Empty array = not set.

- **`twinkle_runtime.read_file` should not trap for ordinary I/O failures.**
  Return `Result.Err(String)` instead. `Result.Ok` carries `Vector<Byte>`; hosts
  should avoid implicit UTF-8 decoding at this boundary.

- **`twinkle_runtime.parse_float` uses multi-value return:** `(f64, i32)` where
  `i32` is 1 on success, 0 on failure. On failure the `f64` value is unspecified.
