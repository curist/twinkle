# Stage 10 — Self-Hosted Compiler

**Goal:** Re-implement the compiler pipeline in Twinkle, use the stage0 Rust compiler to
compile it to `twc.wasm`, then run and verify the Twinkle-hosted compiler.

**Bootstrapping sequence:**

1. Write the compiler in Twinkle under `compiler/` (lexer, parser, type checker, Core IR
   lowering, ANF lowering, optimizer, WAT emitter, Runtime IR + linker).
2. Stage0 Rust: `twk build compiler/main.tw -o twc.wasm`.
3. Verify: run `twc.wasm` under Wasmtime on `hello.tw`; output must match stage0 output.
4. Self-hosting round: compile `compiler/main.tw` with `twc.wasm` → new `twc.wasm`; verify
   the two are behaviorally equivalent on the compatibility suite.

**Prerequisites:** The Twinkle language must be expressive enough to write a compiler.
File I/O (reading source files) is provided by the host via WASI or a custom import — the
compiler sources import it as an abstract interface. String manipulation, arrays, and dicts
(already in the runtime) are sufficient for symbol tables and AST representations.

**Porting note:** The Runtime IR + Linker (`src/wasm/`) is implemented in Rust for stage0 but
must be ported to Twinkle for self-hosting. It is the largest self-hosting prerequisite beyond
the compiler pipeline itself.

**Compatibility suite:**

A set of `.tw` programs compiled by both stage0 (Rust) and stage1 (Twinkle self-hosted);
outputs (Wasm execution results) must be identical.

Deliverables:

* `twc.wasm` produced by stage0 can compile real Twinkle programs.
* `twc.wasm` produced by itself compiles the same programs to equivalent results.
* Stage0 Rust implementation frozen as a reference and bootstrap tool.
