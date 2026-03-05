# Stage 9 — Host Integration & Validation

**Goal:** Implement the Wasmtime host shell that satisfies the runtime's host import interface,
run compiled programs end-to-end via Wasm, and validate correctness against the interpreter
via differential testing.

**Host shell design:**

The host is a thin Rust + Wasmtime layer. It is *not* the compiler — it merely provides
the host import functions (`host.print`, `host.println`, `host.error`) and instantiates the
linked Wasm module. The compiler pipeline remains in Rust at this stage, but the host interface
is deliberately minimal so any other host (Node, browser shim) can implement it identically.

WASI is a host concern: the Wasmtime host implements `host.read_file`, `host.write_file`,
`host.write_bytes`, `host.mkdirp`, `host.list_dir`, `host.exists` using WASI or native
calls. `twc.wasm` imports file I/O abstractly — it is not aware of WASI. This keeps
`twc.wasm` host-agnostic.

At Stage 9, the host shell only needs console imports (`host.print`, `host.println`,
`host.error`) since the compiler pipeline is still in Rust. File I/O imports become live
in Stage 10 when `twc.wasm` itself reads source files.

**Status (2026-03-04):**

Stage 9 deliverables are complete in the current workspace state.

Implemented:

* Wasmtime host shell in `src/cli/run_wasm.rs` with typed host imports for:
  * Console: `print`, `println`, `eprint`, `eprintln`, `error`
  * File I/O: `read_file`, `write_file`, `write_bytes`, `mkdirp`, `list_dir`, `exists`
  * Process: `args`, `env`, `cwd`, `exit`
* End-to-end CLI flow:
  * `twk run-wasm file.tw` (compile + link + run via Wasmtime)
  * `twk build file.tw -o output.wasm` (compile + link only)
* Differential parity:
  * `tests/differential_test.rs` now has an empty wasm skip list.
  * `differential_interp_vs_wasm` passes for all discovered fixtures
    (`45 passed, 0 skipped` at last run).
* Tail-call emission:
  * Tail-position calls emit `return_call` / `return_call_ref` in `src/codegen/emit.rs`.
  * `Instr::ReturnCall` / `Instr::ReturnCallRef` are emitted by the WAT backend and validated
    by codegen unit tests.
* Additional Stage 9 parity fixes landed while closing differential gaps:
  * `string_methods` substring bounds fix.
  * `defer_capture` capture-by-value snapshot fix.
  * Host/process intrinsic parity fixes for `twinkle_typechecker` and `stdlib_proc`.

CLI:

```bash
twk run file.tw                  # interpreter backend (unchanged)
twk run-wasm file.tw             # compile → link → run via Wasmtime
twk build file.tw -o output.wasm # compile + link only
```

**Differential testing (`tests/differential_test.rs`):**

For every program in `tests/run/`:

1. Run via interpreter → capture stdout.
2. Compile + link + run via Wasmtime → capture stdout.
3. Assert outputs are identical.

Any divergence is a regression in the WAT emitter or runtime. The interpreter remains the
reference semantic oracle.

**Wasm 3.0 — Tail Calls:** The WAT emitter now emits `return_call $f` / `return_call_ref
$ClosureFunc` for eligible calls in tail position.

Tail calls matter for:

* The recursive-descent parser in the self-hosted compiler (Stage 10).
* Mutually-recursive functions that otherwise hit Wasm's call stack limit on large inputs.

This remains a Stage 10 safety gate for deep recursion correctness.

Deliverables:

* [x] All `tests/run/*.tw` fixtures in differential scope produce matching output via Wasmtime.
* [x] Differential test suite passing.
* [x] Tail-position calls emitted as `return_call` / `return_call_ref`.
* [~] Host interface documentation exists but is still split across plan/docs and code.

**Post-Stage 9 follow-ups (non-blocking):**

* Consolidate host ABI docs into one canonical reference (imports, signatures, behavior).
* Add explicit stdin-injection support in `run_wasm_capture` for deterministic
  `__debug_stdin_read_all` testing (currently uses non-blocking empty-input fallback).
* Add a deep-recursion fixture to stress tail-call behavior end-to-end.
