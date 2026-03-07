# Persistent Runtime Abstraction

The compiler targets a small, stable runtime surface for immutable data
operations so that backends beyond Wasm GC (JS, C, JVM/CLR) can slot in
without touching front-end semantics. Today's focus is Wasm GC; this document
defines the contract any backend must satisfy.

---

## Goals

- Preserve value semantics and immutability regardless of backend.
- Minimize the core API the compiler emits (array/dict/record/string basics).
- Allow per-target implementations behind a shared contract.
- Keep desugarings (`for`/`collect`, field/index updates) target-agnostic.

---

## Core Surface

All sugar (`arr[i] = v`, `r.field = v`, `collect`, `for`) is lowered to these
operations before codegen:

- **Record update**: `RecordUpdate(r, field, expr)` — functional field replacement.
- **Array**: `new`, `len`, `get`, `set`, `append`, `concat`, `slice`.
- **Dict**: `new`, `len`, `get`, `set`, `remove`, `has`, `keys`.
- **String**: `concat`, `substring`, `of_int`, `of_float`, `of_bool`.
- **Option/Result**: nominal ADTs as in the language (no runtime tricks).
- **Iterator helpers**: whatever `for`/`collect` lowering needs.

---

## Backend Contract

- Operations are pure: inputs stay usable; outputs may share structure internally.
- Traps/errors match language semantics (OOB, div-by-zero, explicit `error`).
- Types/shapes exposed to user code stay consistent (no leaking backend internals).
- Structural sharing is allowed but not observable.

---

## Backend Sketches

- **Wasm GC (default)**: native `struct`/`array`/nullable refs; implements the
  core surface directly with a small shim.
- **JS** (future): wrap a persistent lib (e.g. immer.js) behind the same
  `array`/`dict` modules.
- **C/C++** (future): wrap Immer (or similar); tiny runtime for strings/options/iterators.
- **JVM/CLR** (future): classes/records + persistent collections; nullable refs
  for Option over ref types.
- **Fallback**: naive copy-on-write reference impl for portability tests.

---

## Compiler Hooks

- Target selection (e.g. `--target wasm-gc | js-immer | c-immer | jvm`).
- A mapping table from core surface ops to backend symbols/imports.
- One lowering pipeline that emits only the core surface; backends supply
  implementations.

---

## Testing Strategy

- Cross-backend conformance suite over the core surface.
- Golden tests for traps/errors to ensure backend parity.
- Perf tests can be backend-specific but must not affect semantics.
