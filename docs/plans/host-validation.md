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

**Status (2026-03-03):**

Much of Stage 9 is already implemented as part of Stage 8d/8e:

* Wasmtime host shell exists in `src/cli/run_wasm.rs` with all host imports implemented:
  console (`print`, `println`, `eprint`, `eprintln`, `error`), file I/O (`read_file`,
  `write_file`, `write_bytes`, `mkdirp`, `list_dir`, `exists`), and process (`args`, `env`,
  `cwd`, `exit`).
* `twk run-wasm file.tw` compiles and runs programs end-to-end via Wasmtime.
* `twk build file.tw -o output.wasm` compiles and links to `.wasm`.
* Wasm run fixture tests exist in `tests/run_wasm_test.rs` for a subset of programs.

**Remaining work:**

* Differential testing: systematically run *all* `tests/run/*.tw` through both interpreter
  and Wasmtime, asserting identical stdout. Currently only a subset has wasm coverage.
* Tail calls: emit `return_call` / `return_call_ref` for calls in tail position. The
  `Instr::ReturnCall` and `Instr::ReturnCallRef` variants in `src/wasm/ir.rs` are available.
  Identify tail-position calls in ANF IR (a `Return(ACall(...))` pattern) and emit the
  tail-call form. This is a safety gate for Stage 10.
* Host interface documentation: the exact set of imports, their types, and observable behavior.
  This is the stability boundary for future hosts (Node.js, browser).

CLI:

```bash
twk run file.tw                  # interpreter backend (unchanged)
twk run-wasm file.tw             # compile → link → run via Wasmtime
twk build file.tw -o output.wasm # compile + link only
```

**Differential testing (`tests/wasm_test.rs`):**

For every program in `tests/run/`:

1. Run via interpreter → capture stdout.
2. Compile + link + run via Wasmtime → capture stdout.
3. Assert outputs are identical.

Any divergence is a regression in the WAT emitter or runtime. The interpreter remains the
reference semantic oracle.

**Wasm 3.0 — Tail Calls:** The WAT emitter should emit `return_call $f` / `return_call_ref
$ClosureFunc` for calls in tail position. Tail calls matter for:

* The recursive-descent parser in the self-hosted compiler (Stage 10).
* Mutually-recursive functions that otherwise hit Wasm's call stack limit on large inputs.

The `Instr::ReturnCall` and `Instr::ReturnCallRef` variants in `src/wasm/ir.rs` are available
for the emitter to use. Identify tail-position calls in ANF IR (a `Return(ACall(...))` pattern)
and emit the tail-call form. This is a safety gate for Stage 10 correctness.

Deliverables:

* All `tests/run/*.tw` programs produce correct output via Wasmtime.
* Differential test suite passing.
* Host interface documented: the exact set of imports `twc.wasm` requires from the host,
  their types, and their observable behavior. This is the stability boundary for future hosts.
* Tail-position calls emitted as `return_call` / `return_call_ref`; verified on a
  deeply-recursive test program (e.g. Fibonacci with large N).
