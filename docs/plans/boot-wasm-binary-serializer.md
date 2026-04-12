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

This has also become a concrete self-hosting issue, not just an architectural
gap. The current bootstrap loop can now drive second-generation self-hosted
builds far enough to emit `boot/main.wat`, but closing the loop still depends on
an external WAT parser/assembler. That creates an avoidable failure surface:

- WAT tooling support for newer GC/reference text syntax is uneven
- bootstrap progress can be blocked by text-format compatibility instead of
  compiler correctness
- the fixed-point self-host loop is more fragile than a direct wasm emission
  path

A boot-side binary serializer removes that entire WAT bridge from the critical
path.

## Design Position

### 1. Twinkle Is The Intended Implementation Language

The serializer should be implemented in Twinkle, not maintained permanently as a
parallel JS or Rust backend.

Reasoning:

- avoids duplicating backend logic in another language
- aligns with the long-term self-hosting direction
- builds on the existing `wasm_ir.tw` abstraction rather than bypassing it
- moves the Node toolchain toward true independence from Rust `twk`

### 2. Keep External Validation Temporary And Narrow

If an external script is useful while bringing up a particular encoding detail,
it should be used only as a short-lived validation aid for:

- modern Wasm GC binary encodings used by Twinkle
- current Node/Bun acceptance of emitted binaries
- exact opcode/type encodings for the subset Twinkle uses

The maintained implementation remains the Twinkle serializer. External helpers
should only help confirm encodings during bring-up, not become a second backend.

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

## Useful Existing Building Blocks

The serializer should lean on the pieces Twinkle already has instead of
introducing a large custom substrate up front.

Immediately useful pieces:

- `prelude/vector.tw::join` already demonstrates the core serializer pattern:
  build a `Vector<Byte>` buffer, append bytes incrementally, and convert at the
  boundary when needed
- `@std.fs.write_bytes(path, bytes)` now uses the natural serializer-facing API:
  `fn(path: String, bytes: Vector<Byte>) !FsError`
- `String.utf8_bytes()` provides the right boundary for wasm string/name
  encoding
- the current self-host loop already provides a concrete integration target:
  replace the WAT bridge with direct `.wasm` emission

That means the initial implementation can start from plain `Vector<Byte>`
construction and only introduce more specialized byte-building helpers when they
pay for themselves.

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

Useful existing building blocks and patterns to reuse:

- `Vector<Byte>` accumulation style already used by `prelude/vector.tw::join`
- `@std.fs.write_bytes(path, bytes)` as the final output boundary for raw wasm
  bytes
- `String.utf8_bytes()` for wasm name/string payload encoding

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

### Phase 0: Audit The Actual Emitted Surface

Goal:

- lock the serializer scope to the Wasm IR the boot compiler already emits

Tasks:

- inspect `boot/compiler/codegen/wasm_ir.tw`
- inventory section, type, and instruction variants
- identify which variants are actually constructed by current runtime builders
  and user-module codegen
- produce the first supported-surface checklist for the serializer

Acceptance:

- explicit list of sections, type forms, and instruction forms to implement
- unsupported IR forms identified up front and marked for hard failure

### Phase 1: Byte Utilities On `Vector<Byte>`

Goal:

- establish the byte-writing primitives the serializer will use everywhere

Tasks:

- adopt `Vector<Byte>` as the initial byte buffer representation
- implement `emit_u8`
- implement `emit_bytes`
- implement unsigned and signed LEB128 helpers
- implement fixed-width little-endian helpers where needed
- implement wasm string/vector length-prefix helpers
- add tests for all primitive encoders

Notes:

- use the same append-oriented style already demonstrated by
  `prelude/vector.tw::join`
- keep the first version simple and explicit
- introduce a specialized byte builder only if repeated serializer code makes it
  clearly worthwhile

Acceptance:

- primitive encoders produce correct byte sequences
- tests cover boundary values and common section payload shapes

### Phase 2: Minimal End-To-End Module

Goal:

- prove the end-to-end path from Twinkle serializer code to a runnable wasm
  binary

Tasks:

- encode the wasm magic and version
- encode the smallest useful section pipeline: type, function, export, code
- encode a tiny function body with locals and `end`
- write the result through `@std.fs.write_bytes`
- validate and run the output under Node

Suggested target module:

- one exported function returning a small constant

Acceptance:

- Twinkle emits a valid `.wasm` binary directly
- the binary validates and runs under the Node wasm runner

### Phase 3: Serializer Core For Current `WasmModule`

Goal:

- move from the tiny bring-up module to the real boot `WasmModule` structure

Tasks:

- map `WasmModule` fields to binary sections in canonical order
- encode indices, names, locals, globals, exports, start, code, and data payloads
- wire section omission rules for empty sections
- keep unsupported forms as explicit hard errors

Acceptance:

- a real `WasmModule` value from Twinkle code can be serialized to bytes
- section ordering and payload framing match engine expectations

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

### Phase 6: End-To-End Compiler Path

Goal:

- make direct `.wasm` emission a normal boot compiler output path

Tasks:

- integrate the serializer into the boot compilation flow after `wasm_ir.tw`
- add a raw wasm output path that writes bytes with `@std.fs.write_bytes`
- preserve the existing WAT/debug path where it is still useful for inspection
- validate emitted binaries by running them under Node
- switch the self-host fixed-point loop to use emitted `.wasm` artifacts
  directly instead of going through WAT parsing

Acceptance:

- boot compiler can compile Twinkle source to `.wasm` without relying on the
  Rust emitter backend
- self-host fixed-point compilation no longer depends on a WAT-to-wasm bridge

## Validation Strategy

### Encoding correctness

- byte utility tests for primitive encodings
- focused checks for section framing, LEB128 lengths, names, and locals
- tiny end-to-end module tests that validate and execute under Node
- comparisons against stage0-generated binaries or structure dumps where useful

### Serializer discipline

- serializer fails loudly on unsupported IR nodes
- no silent widening into “best effort” encoding
- each newly supported IR form gets a direct validation case when practical

### Integration and self-hosting

- representative runtime modules serialize and validate successfully
- representative Twinkle programs compile and run under Node
- the self-host loop can use emitted `.wasm` artifacts directly
- fixed-point checking compares serializer-backed generations without a WAT
  conversion step

## Execution Notes

To keep the work moving toward a shippable result:

- start from `Vector<Byte>` and existing stdlib helpers instead of designing a
  custom buffer system first
- complete one valid end-to-end `.wasm` emission path early, then widen support
- keep the supported surface tied to the IR the boot compiler actually emits
- validate early against Node engine loading and execution
- add byte-dump and section-dump helpers whenever they speed up bring-up
- preserve WAT output as a debugging aid, but remove it from the critical
  self-host path
- optimize byte construction only after the serializer is functionally complete

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
