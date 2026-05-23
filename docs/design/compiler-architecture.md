# Compiler Architecture

This document is the durable architecture reference for Twinkle compiler
contributors. It captures the current self-hosted compiler model and the Rust
stage0 bootstrap role.

---

## Goal

Twinkle's canonical compiler is the self-hosted compiler in `boot/`, built as a
WebAssembly GC module (`target/boot.wasm`) and bundled into the standalone
`target/twk` CLI.

The Rust compiler in `src/` is stage0. Its job is to bootstrap the self-hosted
compiler from source and to remain a compact correctness/reference
implementation for the build pipeline. Day-to-day compiler behavior belongs in
`boot/`.

---

## High-Level Architecture

Both stage0 and the boot compiler follow the same broad pipeline:

```text
Twinkle source
  → Lexer
  → Parser (AST with spans)
  → Resolver / type checker (bidirectional, Damas–Milner style)
  → Core IR
  → Monomorphized Core IR
  → ANF IR
  → Optimization passes
  → Wasm GC backend
  → linker/runtime support
  → .wasm or .wat output
```

Boot compiler distribution:

```text
boot/main.tw + boot/compiler/* + boot/lib/*
  └─ stage0 build ─→ target/boot-stage1.wasm
       └─ stage1 builds stage2 ─→ target/boot.wasm
            └─ stage2/stage3 fixed-point check
                 └─ bundled with JS runtime ─→ target/twk
```

Runtime / library inputs:

```text
prelude/*.tw + stdlib/*.tw
  └─ generated into boot/lib/module/core_lib.tw for bootstrap loading

boot/compiler/codegen/runtime/* + bridge/runtime helpers
  └─ linked with user modules by the Wasm backend
```

---

## Operational Model

### Rust stage0 (`src/`)

Stage0 exists to get the boot compiler running from a clean checkout. It should
stay focused on build/check/bootstrap functionality and avoid accumulating
product-facing features.

Current stage0 CLI surface is intentionally smaller than the boot CLI:

```text
parse
check
lower
lower-anf
opt
run
build
runtime-dump
```

The Rust LSP implementation has been removed; LSP support lives in the boot
compiler.

Longer-term direction: stage0 should become even smaller, ideally a compiler that
can build Wasm artifacts without also acting as the normal Twinkle runner.

### Self-hosted CLI (`target/twk`)

The self-hosted compiler is the user-facing compiler. Its CLI is defined in
`boot/main.tw` and currently exposes:

```text
run
check
build
ir
parse
lsp
```

`run`, `check`, `build`, `ir`, and LSP behavior should be implemented here first.
Rust stage0 should only be updated when needed to keep bootstrapping possible or
to preserve a useful reference implementation.

### Bootstrap targets

The Makefile is the source of truth for bootstrap orchestration:

```text
make stage2       # rebuild target/boot.wasm through the self-host loop
make bundle-cli   # rebuild target/boot.wasm, then build target/twk
make playground   # build the playground assets using the boot payload
make test         # Rust tests + boot compiler tests
```

The self-host loop builds through multiple boot-compiled stages and compares the
final outputs to ensure a fixed point.

---

## Design Principles

1. **Boot compiler is canonical**
   Implement new compiler behavior in `boot/` first. Treat Rust stage0 as a
   bootstrap/reference path, not the product compiler.

2. **Pure compiler core**
   Keep compiler stages as pure transformations over in-memory values; isolate
   host I/O in thin command/runtime edges.

3. **Core IR as semantic backbone**
   Lower language constructs (`collect`, `try`, variants, `for`, update syntax)
   into a small, explicit Core IR before backend-specific concerns.

4. **ANF for backend friendliness**
   Preserve explicit evaluation order and backend-oriented structure through ANF.

5. **Wasm backend as production path**
   Treat Wasm GC emission/linking as the production artifact. WAT output is a
   debugging format selected by using a `.wat` output path.

6. **Persistent runtime is deliberate**
   `Vector` and `Dict` are persistent collections implemented by Twinkle's own
   runtime structures, not assumptions about host-level immutable arrays/maps.

7. **Incremental self-hosting**
   Keep `boot/` continuously bootstrap-able from stage0 and verify the
   boot-compiled fixed point.

8. **Shared helper boundaries**
   Keep semantic type helpers independent from backend facts and Wasm encoding
   details. In the boot compiler, `compiler.type_util` owns pure `MonoType`
   traversal/substitution, `compiler.backend.facts` owns prepared backend IR
   fact accessors, and `compiler.codegen.wasm_type_util` owns format-neutral
   Wasm IR type comparisons.

---

## Repository Layout

```text
twinkle/
  boot/                   # self-hosted compiler and user-facing CLI
    main.tw               # boot CLI entry
    commands/             # run/check/build/ir/parse/lsp commands
    compiler/             # lexer/parser/resolver/checker/IR/opt/codegen
    lib/                  # compiler support libs, argparse, LSP, source utils
    tests/                # boot compiler tests

  src/                    # Rust stage0 bootstrap compiler
    main.rs               # stage0 CLI entry
    cli/                  # stage0 commands
    syntax/               # lexer, parser, AST, spans, pretty-printer
    types/                # check/resolve/patterns/type env
    ir/                   # Core IR, monomorphization, ANF lowering
    opt/                  # optimization pipeline/passes
    codegen/              # backend planning and WAT/Wasm emission
    runtime/              # runtime modules (arr/dict/str/core/types)
    wasm/                 # linker/runtime IR emit

  prelude/                # auto-imported language APIs
  stdlib/                 # @std modules
  playground/             # browser playground
  tree-sitter-twinkle/    # syntax grammar and highlighting queries
  tools/                  # JS runtime, Deno bundling, generators
  tests/                  # Rust stage0 tests
  docs/                   # spec, design notes, internals, plans
```
