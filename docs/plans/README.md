# Twinkle Implementation Plan

## Goal

Build a self-hosting Twinkle compiler whose canonical artifact is `twc.wasm` — a single
WebAssembly module that can run in any compliant host. The Rust `twk` binary is the first
host shell; browser and npm hosts follow naturally from the same interface.

* A small Rust **stage0** implementation (`twk`) for fast iteration.
* A clear internal pipeline: Source → AST → Typed AST → Core IR → ANF → Wasm GC backend.
* An **interpreter-first** path (Core IR interpreter) as the semantic oracle.
* A **Wasm GC backend** that emits code calling into a persistent-data-structure runtime.
* `twc.wasm` as the stable, host-agnostic compiler artifact.

---

## High-Level Architecture

Compiler pipeline:

```text
Twinkle source
  → Lexer
  → Parser (AST with spans)
  → Typechecker (bidirectional, Damas–Milner)
  → Core IR (expression+block, loops, match, variants)
  → ANF IR (backend-oriented, with optimization passes)
  → Backend(s):
       - Core IR Interpreter (stage0, semantic oracle)
       - Wasm GC backend → Runtime IR + Linker → linked.wat → output.wasm
```

Runtime / distribution:

```text
Runtime modules (rt.types, rt.arr, rt.dict, rt.str, rt.core)   ┐
Stdlib modules  (compiled from stdlib/*.tw via Wasm GC backend) ├─→ Linker → twc.wasm
Compiler modules (compiled from compiler/*.tw via stage0)       ┘

                                                                  ┌── (stdlib + runtime ModuleIR
                                                                  │    embedded in twc.wasm)
user source files → twc.wasm (running in host) → user ModuleIR → Linker → output.wasm
```

* **`twc.wasm`** bundles three things: the compiler, the stdlib, and the runtime — all linked
  together by the same Runtime IR + Linker pipeline. It is `output.wasm` when the sources are
  `compiler/main.tw` + `stdlib/*.tw` + the runtime modules.
* **Stdlib is embedded**, not loaded from disk. Stdlib `.tw` sources are compiled via the
  Wasm GC backend to `ModuleIR` and linked into `twc.wasm` at build time. The host only needs
  to provide FS access for user source files and build outputs — not for the stdlib.
* When compiling a user program, `twc.wasm` carries the pre-compiled stdlib and runtime
  `ModuleIR` internally. It emits the user's `ModuleIR`, then links it together with those
  embedded artifacts to produce `output.wasm`.
* Once self-hosted, the **host shell drives `twc.wasm`**: it provides file I/O (reading user
  source files, writing output) and instantiates `twc.wasm`, which executes the full compiler
  pipeline internally. The compiler pipeline diagram above describes what runs *inside* `twc.wasm`.
* The Rust host (Wasmtime) is a replaceable shell; browser and npm hosting implement the
  same host import interface.
* **Host interface** (what any host must provide):
  * Console: `host.print`, `host.println`, `host.eprint`, `host.eprintln`, `host.error`.
  * File I/O (for reading user source files and writing build outputs; stdlib is embedded):
    `host.read_file`, `host.write_file`, `host.write_bytes`, `host.mkdirp`, `host.list_dir`,
    `host.exists`.
  * Process/environment (for `@std.proc`):
    `host.args`, `host.env`, `host.cwd`, `host.exit`.
  * Paths are logical (`/`-separated); the host maps them to OS paths or virtual FS.
  * No clock, no randomness, no process spawning — compiler output is deterministic.
* In development: `twk` (Rust binary) with subcommands
  `parse`, `check`, `run`, `lower`, `lower-anf`, `opt`, `build`, `runtime-dump`.
* `run` uses linked Core IR in the interpreter.
* `lower-anf`, `opt`, and `build` are backend-oriented paths and operate on
  monomorphized Core IR before ANF lowering.
* Self-hosted: `twc.wasm` compiled by stage0, then compiles itself.

## Plan Lifecycle

To keep this directory actionable:

* `docs/plans/` top level contains active WIP/planned documents.
* completed plans are moved to `docs/plans/archive/`.
* archived stage/history indexes live in [archive/README.md](archive/README.md).

---

## Design Principles

1. **Pure compiler core**
   Compiler modules operate only on in-memory data:

   * `String → Vec<Token> → AST → Typed AST → IR`.
   * File I/O, CLI, and host integration live in thin wrappers.

2. **Core IR as semantic backbone**
   Twinkle features (`collect`, `try`, `.Variant`, `for x in`, etc.) are desugared into a **Core IR** that directly expresses the semantics in a small set of constructs (spec §7.5, §12, §13, §18):

   * `Let`, `If`, `Match`, `Loop`, `Call`, `Record`, `Variant`, etc.

3. **ANF for backend friendliness**
   A simple **ANF IR** can be derived from Core IR:

   * Every intermediate result bound to a `let`.
   * Evaluation order explicit, easy to map to WAT/Wasm.
   * Optional at first; used primarily by the backend.

4. **Interpreter-first**
   Stage0 backend is an interpreter for Core IR:

   * Fast path to a usable language.
   * Provides a reference semantics engine.
   * Makes it easier to validate language design before committing to Wasm details.

5. **WAT/Wasm is a later backend**
   A WAT/Wasm backend is added after Core IR and interpreter are solid:

   * Emit `.wat` text from IR/ANF.
   * Use Wasmtime (or another runtime) as an external tool at first.
   * Eventually integrate Wasmtime into CLI and move toward self-hosting.

6. **Self-hosting as a deliberate phase**
   Once the language and compiler pipeline stabilize:

   * Re-implement the compiler in Twinkle.
   * Use stage0 Rust compiler to compile the Twinkle compiler to WAT/Wasm.
   * Distribute the Twinkle-implemented compiler as a `.wasm` artifact.

---

## Repository Layout (Stage0-Oriented)

Suggested Rust layout:

```text
twinkle/
  src/
    main.rs               # CLI entry (twk)
    cli/                  # CLI commands (parse/check/run/build)
    syntax/
      lexer.rs
      tokens.rs
      parser.rs
      ast.rs
      span.rs
    types/
      ty.rs               # type representation
      unify.rs            # unification
      infer.rs            # type inference / checking
      env.rs              # type and value environments
    ir/
      core.rs             # Core IR definitions
      lower_core.rs       # AST → Core IR lowering
      anf.rs              # ANF IR definitions
      lower_anf.rs        # Core IR → ANF lowering (later)
    interp/
      value.rs
      eval_core.rs        # Core IR interpreter
    codegen/
      wat.rs              # IR/ANF → WAT backend (later)
  tests/
    parser/
    typecheck/
    ir/
    run/
  docs/
    plans/                # implementation plan (this directory)
    ir.md                 # Core IR & ANF spec (later)
    lang-spec.md          # language spec
```

This keeps the front end, IR, interpreter, and backend clearly separated.

---

## Active Plan Index

Historical/completed indexes are in [archive/README.md](archive/README.md).

### Stage-aligned active plans

| Stage | Description | Status | Details |
|-------|-------------|--------|---------|
| 10 | Self-Hosted Compiler (`boot/`) | In Progress | [self-hosting.md](self-hosting.md) |
| Later | Tooling & Ecosystem | Planned | [tooling.md](tooling.md) |

### Active cross-cutting plans

| Plan | Description |
|------|-------------|
| [string-interning.md](string-interning.md) | Reduce duplicate string allocations with literal/runtime interning |
| [persistent-vector.md](persistent-vector.md) | Move vector runtime from flat COW arrays to persistent tree structure |
| [persistent-dict.md](persistent-dict.md) | Replace linear dict runtime with persistent HAMT |
| [pre-selfhost-cleanup.md](pre-selfhost-cleanup.md) | Refactoring and cleanup before Stage 10 self-hosting |
| [boot-foundation-libs.md](boot-foundation-libs.md) | Stage 10 support libs in `boot/lib` (`source`, `module`, `graph`, `query`) |
| [lsp-diagnostics-completion.md](lsp-diagnostics-completion.md) | Phase 2 plan for `twk lsp` diagnostics publishing, completion, and simple `///` doc comments |
