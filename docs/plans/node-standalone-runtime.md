# Standalone Node.js Runtime and Toolchain

## Goal

Build a standalone Node.js Twinkle toolchain entry that can:

- compile Twinkle source end to end
- run the resulting Wasm GC module under Node.js
- avoid making the Rust `twk` CLI a hard requirement

This plan is **not** about integrating Node as an extra backend flag into the
Rust CLI. Node.js is treated as its own toolchain/runtime entry.

## Why This Plan Exists

The current Node support proves that emitted Twinkle Wasm GC modules can run
under Node/Bun with a JS host implementation. But today that support is still:

- tool-script oriented
- not a standalone compiler/runtime toolchain
- dependent on the Rust compiler path for actual Twinkle compilation

If Node is to become a primary future runtime/toolchain path, it must have its
own entrypoint and must not depend on Rust `twk` as a hard prerequisite.

## Design Position

### 1. Node.js Is A Separate Toolchain Entry

This plan does **not** target:

- `twk run --backend node`

Instead it targets a dedicated Node-side entry, for example:

- `node cli/twk.mjs run file.tw`
- or an eventual npm bin such as `twinkle run file.tw`

The exact naming can wait, but the architecture should assume a standalone Node
entry rather than Rust CLI integration.

### 2. Rust `twk` Must Not Be A Hard Requirement

The desired end state is:

- Node toolchain can compile Twinkle source to `.wasm`
- Node toolchain can run that `.wasm`
- Rust `twk` may remain useful for development, reference behavior, and stage0,
  but it is not required for the Node path to function

This makes the boot compiler + wasm serializer the key enabling dependency.

### 3. Twinkle Remains The Source Of Truth For Compilation Logic

The Node toolchain should avoid growing a parallel JS compiler implementation.

Preferred architecture:

- Twinkle boot compiler handles frontend/lowering/codegen
- Twinkle-side wasm serializer emits final binaries
- Node provides orchestration, file/project integration, host runtime, and CLI

A small amount of JS glue is expected. A second maintained compiler backend is
not.

### 4. Runtime Support Should Stay Textual / Auditable

For helper modules such as the JS↔Wasm GC bridge, prefer:

- textual source
- ideally Twinkle-authored or Twinkle-Wasm-IR-authored definitions

rather than treating committed opaque binaries as the primary source of truth.

## Relationship To Other Plans

This plan depends directly on:

- [`boot-wasm-binary-serializer.md`](boot-wasm-binary-serializer.md)

It is also informed by:

- `tools/run_wasm_node.mjs` as the current proof of runtime feasibility
- the persistent vector/dict/runtime plans that define the emitted runtime shape

If the serializer plan is blocked, this plan is blocked as an end-to-end
compiler path.

## Current State

### What already works

- a JS host runtime exists in `tools/run_wasm_node.mjs`
- Twinkle-emitted Wasm GC modules can run under Node.js
- benchmark and boot-test experiments show Node is a viable execution runtime

### What is still missing

- standalone Node compiler entrypoint
- Node toolchain path from `.tw` source to final `.wasm`
- formal runtime host contract/documentation
- packaging/distribution shape for a real Node CLI
- removal of Rust `twk` as a hard dependency for the Node path

## Desired End State

A user can do something like:

```bash
twinkle run src/main.tw
twinkle build src/main.tw -o out.wasm
```

where the command is provided by a Node.js package/entrypoint and the path does
not require the Rust CLI to perform compilation.

Under the hood, the Node toolchain is responsible for:

- project/module discovery
- invoking the boot compiler path
- obtaining `.wasm` bytes from the boot serializer
- providing host imports for execution
- surfacing stdout/stderr/exit behavior consistently

## Proposed Architecture

### Layer A: Node CLI / Orchestration

Responsibilities:

- parse command-line args
- resolve project root / files / outputs
- invoke the Twinkle compiler path
- invoke execution runtime for `.wasm`
- handle diagnostics / exit codes / file IO integration

This layer should remain thin.

### Layer B: Twinkle Compiler Path

Responsibilities:

- parse/resolve/check/lower/optimize Twinkle source
- emit `wasm_ir.tw` structures
- serialize them to final `.wasm` bytes

This is where most language logic should live.

### Layer C: Node Host Runtime

Responsibilities:

- instantiate bridge/helper modules
- instantiate user/runtime wasm modules
- provide the Twinkle host ABI
- handle strings/arrays/variants/byte arrays across JS↔Wasm boundaries
- execute modules and propagate exit behavior

This is the natural evolution of `tools/run_wasm_node.mjs`.

## Node Runtime Contract

The standalone Node runtime needs a documented host ABI matching Twinkle's
runtime expectations.

Minimum host capabilities include:

- stdout/stderr printing
- trap/error path
- process args / env / cwd / exit
- file reads/writes and byte writes
- directory listing and existence checks
- numeric parsing helpers
- runtime string / byte array / variant bridging

This contract should be documented and tested explicitly rather than only living
inside one helper script.

## Bridge Module Strategy

The JS↔Wasm GC bridge remains necessary because JS cannot directly construct and
inspect all Twinkle GC runtime values in the needed shape.

Preferred long-term approach:

- keep bridge logic in textual / structured form
- ideally author it in Twinkle or Twinkle Wasm IR
- compile it through the same serializer/toolchain direction as the rest of the
  system when practical

Avoid treating an opaque committed `bridge.wasm` as the long-term source of
truth.

## Phasing

### Phase 0: Runtime Proof Consolidation

Goal:

- stabilize the current Node runtime proof into a reusable module structure

Tasks:

- factor the current helper script into clearer runtime/CLI pieces
- document the current host ABI surface
- preserve current working execution behavior under Node

Acceptance:

- runtime host code is reusable beyond a one-off tool script

### Phase 1: Standalone `.wasm` Runner Entry

Goal:

- provide a real Node CLI path for already-compiled `.wasm`

Tasks:

- define the standalone Node CLI package/entry structure
- support `run file.wasm`
- forward args/stdout/stderr/exit codes correctly
- add tests for runtime host behavior

Acceptance:

- Node toolchain can run Twinkle-produced `.wasm` without going through the
  Rust CLI entrypoint

### Phase 2: Boot Compiler → Wasm Serializer Foundation

Goal:

- remove the biggest blocker to end-to-end Node compilation

Dependency:

- progress on `boot-wasm-binary-serializer.md`

Acceptance:

- boot compiler can produce final `.wasm` bytes from Twinkle source

### Phase 3: Source Compilation In Node Toolchain

Goal:

- support `run file.tw` and `build file.tw` in the Node toolchain

Tasks:

- wire Node CLI to the boot compiler path
- invoke boot-side compilation and serialization
- cache/intermediate handling as needed
- expose diagnostics sensibly to users

Acceptance:

- Node toolchain can compile `.tw` to `.wasm` end to end without Rust `twk`

### Phase 4: Project / Module / Test Workflow

Goal:

- make the Node toolchain practical for real Twinkle projects

Tasks:

- support project-root/module discovery equivalent to current compiler behavior
- support runtime tests / boot tests / representative project flows
- decide initial command surface beyond `run` and `build`

Acceptance:

- representative Twinkle workloads can use the Node toolchain directly

### Phase 5: Packaging and Promotion

Goal:

- turn the standalone Node toolchain into a first-class supported path

Tasks:

- choose package/bin naming
- document install and runtime requirements
- add CI coverage for Node toolchain execution
- decide whether Node becomes the recommended runtime/toolchain path

Acceptance:

- Node toolchain is documented, packaged, and supportable as a primary path

## Packaging Shape

Suggested eventual repository/package structure:

- dedicated Node package or directory, not an ad hoc `tools/` helper
- reusable runtime library code separated from CLI entry code
- clear bin/package metadata for npm distribution later

Exact directory naming can be decided later, but the plan should assume this is
product surface, not just internal tooling.

## Validation

### Runtime parity

- representative programs produce the same output/exit behavior as current
  reference execution
- host IO behavior matches expectations
- boot tests or equivalent representative suites run successfully

### Toolchain independence

- Node path can compile and run Twinkle code without Rust `twk`
- any remaining Rust dependencies are optional/dev-facing, not hard runtime
  requirements

### Packaging readiness

- CLI behavior is stable enough to document
- dependency/install story is clear

## Risks

- the boot compiler serializer is the critical path and may expose Twinkle
  binary-ergonomics gaps
- JS↔Wasm GC bridge maintenance may still be subtle
- project-root/module-resolution behavior must remain consistent with existing
  Twinkle expectations
- premature CLI polish before compiler independence would create churn

## Risk Mitigations

- keep the Node CLI thin and defer major UX expansion until the serializer path
  is real
- make the runtime host contract explicit and test it
- keep Twinkle as the source of truth for compilation logic
- treat JS-only implementations as temporary spikes, not architecture

## Exit Criteria

This plan is complete when all are true:

1. A standalone Node.js Twinkle entrypoint exists.
2. It can compile Twinkle source to `.wasm` end to end without requiring Rust
   `twk`.
3. It can run the resulting Wasm GC modules with a maintained Node host runtime.
4. The Node path is viable as a primary future runtime/toolchain for Twinkle.

## Follow-On Work

After this lands:

1. decide whether Node becomes the recommended default toolchain/runtime path
2. extend the same architecture to Bun or browser-adjacent toolchains if useful
3. continue reducing any remaining stage0/Rust-only assumptions from the Node
   path
