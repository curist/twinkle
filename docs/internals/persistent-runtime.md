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

## Semantic Surface

This is the smallest collection API a backend should model semantically, even if the
current boot compiler routes through a few extra helper symbols:

- **Record update**: `RecordUpdate(r, field, expr)` — functional field replacement.
- **Vector**: `new`, `len`, trap-on-OOB index read, safe `get`, persistent `set`,
  `push`, `concat`, `slice`.
- **Dict**: `new`, `len`, safe `get`, `set`, `remove`, `has`, `keys`.
- **String**: `concat`, `substring`, byte indexing helpers, `of_int`, `of_float`,
  `of_bool`.
- **Option/Result**: nominal ADTs as in the language.

## Current Boot Compiler Surface

To plug into the current boot compiler without changing lowering/codegen, a backend
needs a slightly larger operational surface than the semantic one above.

User-visible collection methods are partly implemented as compiler intrinsics and
partly as runtime calls:

- **Vector runtime helpers**:
  - `vector_len`
  - `vector_set_unsafe`
  - `vector_concat`
  - `vector_slice`
  - `vector_builder_new`
  - `vector_builder_from`
  - `vector_builder_push`
  - `vector_builder_freeze`
- **Vector intrinsics**:
  - `vector_push`
  - `vector_get`
  - `vector_set`
  - `vector_make`
  - `vector_set_in_place` (optimizer fast path)
- **Dict runtime helpers**:
  - `dict_new`
  - `dict_get` (`Option`-returning lookup)
  - `dict_len`
  - `dict_has`
  - `dict_keys`
  - `dict_set`
  - `dict_remove`
  - `dict_set_in_place` (optimizer fast path)
  - `dict_remove_in_place` (optimizer fast path)
- **Optional internal helper**:
  - `dict_get_unsafe` if lowering/codegen still wants raw lookup without `Option`

These extra helpers exist for current lowering and optimizer structure:

- `collect` lowers through vector builder ops.
- loop-region uniqueness rewrites target vector builder ops and in-place helpers.
- `arr[i] = v` lowers to raw persistent update (`vector_set_unsafe`).
- `m[k] = v` lowers to `dict_set`.
- dict iteration currently depends on `Dict.keys`, so key order is observable through
  `for`, `collect`, and prelude helpers like `Dict.values`. Backends must preserve the
  language-level dict-order contract, not just a deterministic traversal.

---

## Backend Contract

- Operations are pure: inputs stay usable; outputs may share structure internally.
- Traps/errors match language semantics (OOB, div-by-zero, explicit `error`).
- Types/shapes exposed to user code stay consistent (no leaking backend internals).
- Structural sharing is allowed but not observable.
- Optimizer-only in-place helpers are allowed, but they must only be used when the
  compiler has already proved uniqueness. They are ABI hooks, not user-visible
  semantic operations.

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
- A mapping table from semantic ops to backend symbols/imports.
- A second mapping layer, if needed, from current boot operational helpers
  (`vector_builder_*`, `*_set_in_place`, etc.) to the backend's implementation.
- One long-term lowering pipeline should ideally emit only the semantic surface, but
  the current boot compiler still emits the larger operational surface listed above.

---

## Testing Strategy

- Cross-backend conformance suite over the semantic surface.
- Golden tests for traps/errors to ensure backend parity.
- Backend-integration tests for the current operational hooks:
  - builder-driven `collect`
  - indexed assignment
  - optimizer rewrites to in-place helpers
  - dict iteration order via `keys`
- Perf tests can be backend-specific but must not affect semantics.
