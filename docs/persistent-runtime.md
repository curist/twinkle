# Persistent Runtime Abstraction

This note captures how to keep Twinkle’s immutable data model pluggable across backends. Today’s focus stays on Wasm GC, but the compiler should target a small, stable surface so other runtimes (JS, C, JVM/CLR, etc.) can slot in later without touching front-end semantics.

## Goals
- Preserve value semantics and immutability regardless of backend.
- Minimize the “core API” the compiler emits (array/dict/record/string basics).
- Allow per-target implementations (persistent libs or native GC types) behind a shared contract.
- Keep desugarings (for/collect, field/index updates) target-agnostic.

## Core Surface (what the compiler emits)
- **Record update op**: conceptual `RecordUpdate(r, field, expr)` lowering for field updates.
- **Array**: `new`, `len`, `get`, `set`, `append`, `concat`, `slice`.
- **Dict**: `new`, `len`, `get`, `set`, `remove`, `has`, `keys`.
- **String**: `concat`, `substring`, `of_int`, `of_float`, `of_bool`.
- **Option/Result**: nominal ADTs as in the language (no runtime tricks assumed).
- **Iterator helpers**: whatever minimal support `for`/`collect` lowering needs.

All sugar (`arr[i] = v`, `r.field = v`, `collect`, `for`) should be lowered to these calls before codegen.

## Contract (per backend)
- Operations are pure: inputs stay usable; outputs may share structure internally.
- Traps/Errors match language semantics (OOB, div-by-zero, explicit `error`).
- Types/shapes exposed to user code stay consistent (no leaking backend-specific structures).
- Structural sharing is allowed but not observable.

## Backend Sketches
- **Wasm GC (default)**: use native `struct`/`array`/nullable refs; implement the core surface directly. Small shim only.
- **JS** (future): wrap a persistent lib (e.g., immer.js/immutable.js) behind the same `array`/`dict` modules; ensure operations are pure and match traps.
- **C/C++** (future): wrap Immer (or similar) for `array`/`dict`; provide a tiny runtime for strings/options/results/iterators.
- **JVM/CLR** (future): lower to classes/records + persistent collections; nullable refs for Option over ref types.
- **Fallback**: a naïve copy-on-write reference impl is acceptable for portability tests if no persistent lib is available.

## Compiler Hooks
- Target selection (e.g., `--target wasm-gc | js-immer | c-immer | jvm`).
- A mapping table from core surface ops to backend symbols/imports.
- One lowering pipeline that emits only the core surface; backends just supply implementations.

## Testing Strategy
- Cross-backend conformance suite over the core surface (array/dict/string/record update/for/collect).
- Golden tests for traps/errors to ensure backend parity.
- Perf tests can be backend-specific but should not affect semantics.
