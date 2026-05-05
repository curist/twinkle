# Compiler Architecture

This document is the durable architecture reference for Twinkle compiler
contributors. It captures the current stage0 implementation model and the
self-hosting direction.

---

## Goal

Build a self-hosting Twinkle compiler whose canonical artifact is `twc.wasm`,
a single WebAssembly module that can run in any compliant host.

The Rust `twk` binary is the stage0 bootstrap host used for fast iteration and
for compiling `boot/main.tw` into `twc.wasm`.

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
       - Core IR interpreter (semantic oracle)
       - Wasm GC backend → Runtime IR + Linker → linked.wat → output.wasm
```

Runtime / distribution:

```text
runtime modules (src/runtime/* + src/wasm/*)                    ┐
stdlib modules  (compiled from stdlib/*.tw via Wasm GC backend) ├─→ Linker → output.wasm
user module IR                                                   ┘

boot compiler sources (boot/main.tw + boot/lib/*)
  └─(compiled by stage0 twk build)─→ twc.wasm
```

Operational notes:

* `twk` exposes `parse`, `check`, `lower`, `lower-anf`, `opt`, `run`, `build`,
  `runtime-dump`, and `lsp`.
* `run` defaults to Wasm execution; `run -i` uses the Core IR interpreter.
* `build` emits Wasm output and can optionally emit sibling WAT (`--emit-wat`).
* `lower-anf`, `opt`, and `build` operate on monomorphized Core IR before ANF
  lowering.
* Self-hosting target remains: compile `boot/main.tw` into `twc.wasm`, then
  use `twc.wasm` as the compiler artifact.

Host interface expectations (deterministic, no clock/random/process spawning):

* console: `host.print`, `host.println`, `host.eprint`, `host.eprintln`,
  `host.error`
* filesystem: `host.read_file`, `host.write_file`, `host.write_bytes`,
  `host.mkdirp`, `host.list_dir`, `host.exists`
* process/environment: `host.args`, `host.env`, `host.cwd`, `host.exit`

---

## Design Principles

1. **Pure compiler core**
   Keep compiler stages as pure transformations over in-memory values; isolate
   host I/O in thin edges.

2. **Core IR as semantic backbone**
   Lower language constructs (`collect`, `try`, variants, `for`) into a small,
   explicit Core IR.

3. **ANF for backend friendliness**
   Preserve explicit evaluation order and backend-oriented structure through ANF.

4. **Interpreter as semantic oracle**
   Maintain interpreter behavior parity to validate semantics quickly.

5. **Wasm backend as production path**
   Treat Wasm emission/linking as the primary execution artifact while
   preserving interpreter parity checks.

6. **Deliberate self-hosting progression**
   Keep `boot/` progression incremental and continuously bootstrap-able from
   stage0.

7. **Shared compiler helper boundaries**
   Keep semantic type helpers independent from backend facts and Wasm encoding
   details. In the boot compiler, `compiler.type_util` owns pure `MonoType`
   traversal/substitution, `compiler.backend.facts` owns prepared backend IR
   fact accessors, and `compiler.codegen.wasm_type_util` owns format-neutral
   Wasm IR type comparisons.

---

## Repository Layout (Current)

```text
twinkle/
  src/
    main.rs               # CLI entry (twk)
    cli/                  # parse/check/lower/lower-anf/opt/run/build/lsp/runtime-dump
    syntax/               # lexer, parser, AST, spans, pretty-printer
    types/                # check/resolve/patterns/type env
    ir/                   # core IR, monomorphization, ANF lowering
    interp/               # interpreter
    opt/                  # optimization pipeline/passes
    codegen/              # backend planning and WAT/Wasm emission
    runtime/              # runtime modules (arr/dict/str/core/types)
    wasm/                 # linker/runtime IR emit
  prelude/                # auto-imported language APIs
  stdlib/                 # @std modules
  boot/                   # self-hosted compiler work (Stage 10)
  tests/
  docs/
    spec.md
    API.md
    design/
    internals/
    plans/
```

