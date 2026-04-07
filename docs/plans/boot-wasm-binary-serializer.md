# Boot Wasm IR → Wasm Binary Serializer

## Goal

Enable the boot compiler to emit real `.wasm` binaries from
`boot/compiler/codegen/wasm_ir.tw`, so a standalone Node.js Twinkle toolchain
can eventually compile Twinkle end to end without requiring the Rust `twk` CLI
as a hard dependency.

This plan is intentionally scoped to:

- **serializing the Wasm IR that Twinkle actually emits today**
- not implementing a general-purpose WAT parser or wasm assembler
- not supporting arbitrary wasm features outside the boot compiler/runtime
  surface

## Why This Plan Exists

The missing foundation for a standalone Node.js compiler/runtime is not the
bridge module by itself. It is the lack of a boot-side path from:

- Twinkle source
- → boot compiler pipeline
- → `boot/compiler/codegen/wasm_ir.tw`
- → final `.wasm` bytes

As long as the last step depends on the Rust stage0 emitter, Node cannot become
an end-to-end compiler/runtime on its own.

## Design Position

### 1. Twinkle Is The Intended Implementation Language

The serializer should be implemented in Twinkle, not maintained permanently as a
parallel JS or Rust backend.

Reasoning:

- avoids duplicating backend logic in another language
- aligns with the long-term self-hosting direction
- builds on the existing `wasm_ir.tw` abstraction rather than bypassing it
- moves the Node toolchain toward true independence from Rust `twk`

### 2. A JS Spike Is Allowed Only As A Short-Lived Feasibility Tool

A tiny JS-side proof of concept is acceptable only if it derisks one of the
following:

- modern Wasm GC binary encodings used by Twinkle
- current Node/Bun acceptance of emitted binaries
- exact opcode/type encodings for the subset Twinkle uses

A JS serializer is **not** the target architecture and must not become a second
maintained emitter.

### 3. Support Only The IR Subset Boot Actually Emits

This project is feasible because it does **not** need to be a general assembler.
It only needs to serialize:

- the runtime modules the compiler emits
- the user-module Wasm IR the boot compiler emits
- the GC/reference/instruction subset Twinkle currently uses

Unsupported IR forms should fail clearly rather than silently emitting invalid
wasm.

## Scope

In scope:

- a binary serializer for the current `wasm_ir.tw` model
- byte-buffer building utilities needed by that serializer
- section encoding for the subset of wasm used by Twinkle
- modern GC/reference type encoding needed by current runtime/user modules
- validation against Node/Bun and stage0-generated behavior

Out of scope:

- parsing WAT
- accepting arbitrary external wasm IR/text
- supporting all wasm proposals
- broad Binaryen/WABT compatibility work
- changing Twinkle surface syntax or general compiler architecture

## Dependencies and Inputs

Primary source of truth:

- `boot/compiler/codegen/wasm_ir.tw`

Important consumers/producers to audit:

- `boot/compiler/codegen/runtime/*.tw`
- `boot/compiler/codegen/emit.tw`
- stage0 runtime/codegen output and snapshots
- runtime modules such as `src/runtime/*.rs` as reference behavior/shape

The serializer must match the **current emitted IR and runtime conventions**,
not an older planned shape.

## Key Unknown: Is Twinkle Comfortable Enough For Binary Work?

The main project risk is not conceptual complexity. It is whether current
Twinkle ergonomics are good enough for binary-heavy code.

Areas to test early:

- byte-buffer building
- byte appends and concatenation
- unsigned/signed LEB128 encoding
- little-endian fixed-width integer emission
- bit operations and masking ergonomics
- file output for raw bytes

This motivates an explicit feasibility spike before the full serializer.

## Proposed Architecture

### Layer A: Byte Encoding Utilities

A small Twinkle utility layer for binary emission, likely under boot compiler
infrastructure.

Expected responsibilities:

- mutable or efficiently persistent byte buffer construction
- `emit_u8`
- `emit_bytes`
- `emit_u32_leb`
- `emit_i32_leb`
- `emit_i64_leb`
- fixed-width little-endian encoding where needed
- section framing helpers
- wasm vector/string encoding helpers

### Layer B: Wasm IR Serializer

A serializer from `WasmModule` / related IR nodes to bytes.

Expected responsibilities:

- encode module header
- encode sections in canonical order
- map IR types to wasm binary types
- map IR instructions to wasm binary opcodes/immediates
- reject unsupported IR constructs clearly

### Layer C: Compiler Integration

Boot compiler path that produces `.wasm` bytes from emitted `WasmModule` IR.

Later, a standalone Node CLI can consume that compiler path directly.

## Minimal Supported Surface

The first implementation should support only what current boot/runtime emission
needs.

### Sections

Minimum likely set:

- type
- import
- function
- global
- export
- start
- code
- data

Include table/element sections only if current emitted IR actually requires
those forms.

### Types

Support only currently used types, likely including:

- `i32`, `i64`, `f64`
- `anyref`, `eqref`, named refs, nullable refs
- function types
- struct types
- array types
- packed field types such as `i8`

### Instructions

Only implement the instruction variants currently constructed by the compiler and
runtime builders.

This includes ordinary control flow plus the GC/reference instructions Twinkle
already emits.

## Phasing

### Phase 0: Feasibility Spike

Goal:

- determine whether current Twinkle is pleasant enough for binary emission work

Deliverable:

- tiny Twinkle program that emits a minimal valid wasm module binary

Suggested target module:

- one exported function returning a small constant

Acceptance:

- Twinkle code can build byte buffers sanely
- LEB128 helpers are manageable
- emitted module validates/runs under Node or another wasm runtime

Decision gate:

- if Twinkle ergonomics are acceptable, continue in Twinkle
- if not, identify the minimal Twinkle/runtime/library improvements needed
  before continuing
- only if uncertainty around encodings remains high should a tiny JS spike be
  used as temporary validation scaffolding

### Phase 1: IR Surface Audit

Goal:

- enumerate the actual IR/types/instructions/sections that must be supported

Tasks:

- inspect `boot/compiler/codegen/wasm_ir.tw`
- inventory constructors and variants
- identify which are actually emitted by current boot compiler/runtime code
- derive the minimal serializer matrix

Acceptance:

- explicit list of supported IR forms
- unsupported forms identified and marked for hard failure

### Phase 2: Byte Utility Layer

Goal:

- establish reusable Twinkle primitives for binary output

Tasks:

- choose byte buffer representation
- implement append and bulk-write helpers
- implement LEB128 encoders
- implement section framing helpers
- add tests for all primitive encoders

Acceptance:

- utilities can encode standalone byte sequences correctly
- tests cover corner cases for integer encodings

### Phase 3: Minimal Module Serializer

Goal:

- serialize a tiny subset of `WasmModule` into valid `.wasm`

Tasks:

- encode magic/version
- encode a minimal type/function/export/code pipeline
- encode simple function bodies and locals
- validate output against actual engines

Acceptance:

- minimal modules produced from Twinkle IR execute correctly

### Phase 4: Runtime Module Coverage

Goal:

- support the subset needed by compiler-owned runtime modules

Suggested first targets:

- `rt.types`
- a small runtime module with simple funcs/exports
- then more GC-heavy modules such as `rt.core` / `rt.str`

Tasks:

- add type encodings used by runtime modules
- add GC-specific type/instruction support actually required by those modules
- compare emitted binaries/behavior with stage0 references

Acceptance:

- selected runtime modules serialize and validate successfully

### Phase 5: Full User-Module Coverage

Goal:

- support the IR used by normal boot-compiled user modules

Tasks:

- implement remaining instruction encodings actually emitted by boot codegen
- validate on real Twinkle programs
- close gaps discovered by boot tests and realistic workloads

Acceptance:

- boot compiler can emit runnable `.wasm` for representative user programs

### Phase 6: End-to-End Compiler Path

Goal:

- make the boot compiler produce final `.wasm` bytes as a normal output path

Tasks:

- integrate serializer into the boot compilation flow
- add file-writing path for raw wasm bytes
- validate against runtime execution under Node

Acceptance:

- boot compiler can compile Twinkle source to `.wasm` without relying on the
  Rust emitter backend

## Validation Strategy

### Correctness

- byte utility tests for primitive encodings
- golden tests for tiny modules
- runtime validation through Node/Bun/WebAssembly engines
- comparisons against stage0-generated binaries where useful

### Scope discipline

- serializer fails loudly on unsupported IR nodes
- no silent widening into “best effort” encoding

### Integration

- boot test suites continue to pass where serializer-backed compilation is used
- representative Twinkle programs compile and run under Node

## Risks

- Twinkle may currently be awkward for low-level byte manipulation
- GC/reference type binary encodings must match current engine expectations
- the emitted IR surface may be larger than initially expected
- debugging binary mismatches can be time-consuming without good byte-level
  diagnostics
- performance of byte-buffer construction may require utility/runtime work

## Risk Mitigations

- start with a tiny feasibility spike before full serializer work
- implement only the emitted subset, not a general assembler
- test early against Node/Bun engine validation
- compare with stage0/runtime outputs and snapshots where practical
- add byte-dump/debug helpers if needed

## Exit Criteria

This plan is complete when all are true:

1. The boot compiler has a Twinkle-implemented serializer from `wasm_ir.tw` to
   `.wasm` bytes.
2. The serializer supports the IR subset needed by current boot/runtime and user
   module emission.
3. Representative Twinkle programs compile to valid `.wasm` without relying on
   the Rust emitter backend.
4. A standalone Node.js Twinkle toolchain is no longer blocked on wasm binary
   emission.

## Follow-On Work

After this lands:

1. build the standalone Node.js compiler/runtime entrypoint around the boot
   compiler + serializer
2. keep the bridge module in textual / Twinkle-authored form rather than as a
   committed opaque binary
3. improve Twinkle binary ergonomics further if the serializer exposed rough
   edges
4. eventually remove Rust `twk` as a hard requirement for the Node-based toolchain
