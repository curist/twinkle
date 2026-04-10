# Twinkle Runtime/Library Import Boundary

## Goal

Provide a mechanism by which Twinkle-authored library modules (`boot/lib`)
can bind and call into runtime substrate symbols (e.g. `rt.arr.*`,
`rt.str.*`), so that semantic library implementations can be written in
Twinkle rather than maintained as parallel Rust logic.

This plan is specifically about the boundary between two different
Twinkle-authored layers:

- ordinary Twinkle in `boot/lib`, where semantic collection logic should live
- compiler-owned runtime modules, which may themselves be authored in Twinkle
  via the Wasm IR DSL (`boot/compiler/codegen/wasm_ir.tw`)

This plan is a prerequisite for:

- [boot-lib-vector-consumption.md](boot-lib-vector-consumption.md)

## Why This Plan Exists

`boot-lib-vector-consumption.md` describes an ABI boundary where stage0
consumes a Twinkle-authored `Vector<Int>` implementation from `boot/lib`.
That plan assumes `boot/lib` code can import `rt.arr` substrate helpers.

Today, no such capability exists:

- Twinkle has no `extern`/`foreign` syntax for declaring external functions.
- All runtime calls go through a hardcoded builtin registry with fixed
  FuncIds.
- There is no way for a `.tw` file to name or bind a raw Wasm import like
  `rt.arr::set_i64`.
- The linker has no notion of compiler-owned library artifacts distinct from
  runtime modules.

Until this boundary exists, plans that depend on Twinkle-authored library
modules consuming runtime substrate are blocked.

This is not the same as saying low-level helpers must remain Rust-owned.
Stage0 and boot may continue to move runtime substrate into Twinkle-authored
Wasm IR modules. The missing piece is the call/import/link boundary from
ordinary `boot/lib` Twinkle into those substrate modules.

## Scope

In scope:

- A representation (syntax, IR, or both) for Twinkle code to declare
  dependence on named runtime symbols
- Resolver/type-checker support for such declarations
- Linker/build-pipeline support for compiler-owned library artifacts that
  import runtime modules
- Enough infrastructure for the first `boot/lib` vector module to call
  `rt.arr` typed helpers

Out of scope:

- General-purpose Wasm FFI (arbitrary host imports, multi-value returns)
- Changes to the public user-facing language surface
- Moving all runtime functions behind this boundary at once
- Replacing the compiler/runtime Wasm IR layer with ordinary `boot/lib`
  source; low-level substrate may remain expressed through compiler-owned
  Wasm IR modules

## Design Space

### Option A: Extern Declaration Syntax

Add a language-level `extern` or `@import` form:

```tw
extern "rt.arr" {
  fn make_i64(len: Int, fill: Int) Vector<Int>
  fn get_i64(arr: Vector<Int>, idx: Int) Int
  // ...
}
```

Pros:

- Explicit, type-checked at the Twinkle level
- Naturally extends to user-authored Wasm interop later
- No magic — the source declares exactly what it imports

Cons:

- Parser, resolver, and type checker changes
- ABI mismatch handling (Twinkle Int = i64, runtime params may be i32)
- Larger surface area for the first landing

### Option B: IR-Level Runtime Binding

Keep the source language unchanged. Instead, teach the lowering/codegen
pipeline to recognize a set of well-known library modules and wire their
calls to runtime imports:

- `boot/lib/vector_i64.tw` defines functions with real Twinkle bodies
  (stubs or partial logic)
- A compiler-internal binding table maps specific calls in those modules to
  runtime symbols
- The linker resolves those bindings against `rt.arr` exports

Pros:

- No parser/syntax changes
- Can be done incrementally
- Keeps the user-facing language simple

Cons:

- Magic binding table maintained in Rust
- Harder to extend to new modules
- Still requires linker changes

### Option C: Builtin Registry Extension

Extend the existing builtin registry to support library-internal builtins
that are not part of the prelude. A `boot/lib` module could declare
functions whose bodies are replaced at compile time with calls to registered
runtime symbols.

Pros:

- Minimal new infrastructure — reuses the existing builtin path
- Works today for the boot compiler (which already has a builtin registry)

Cons:

- Conflates "compiler builtin" with "library import"
- Registry grows unboundedly as more substrate calls are needed
- Does not generalize to user-authored libraries

## Chosen Approach: Option C (Builtin Registry Extension)

Option C is the right fit for the current scope:

- The builtin registry already exists in both stage0
  (`src/intrinsics/registry.rs`) and the boot compiler
  (`boot/compiler/builtins.tw`)
- The binding set is small and known (roughly a dozen `Vector<Int>` ops)
- No parser/syntax changes needed — avoids churn for what is fundamentally
  a compiler-internal concern
- Incremental: add library-internal builtins alongside existing prelude
  builtins

The cons (registry growth, conflation with prelude builtins) are manageable
at this scale. If the pattern proves too rigid as more library modules
appear, Option A (extern syntax) can be introduced later as a generalization.

### Concrete Shape

1. Add a new builtin category (e.g. `LibraryInternal`) distinct from
   `Runtime` and `Intrinsic`, so library-internal builtins are not exposed
   to user code through the prelude.

2. Register `Vector<Int>` library ABI entries that delegate to `rt.arr`
   substrate:

   | Library ABI name       | Targets            |
   |------------------------|--------------------|
   | `vector_i64_make`      | `rt.arr::make_i64` |
   | `vector_i64_get`       | `rt.arr::get_i64`  |
   | `vector_i64_set`       | `rt.arr::set_i64`  |
   | `vector_i64_len`       | `rt.arr::len_i64`  |
   | `vector_i64_push`      | `rt.arr::push_i64` |
   | `vector_i64_concat`    | `rt.arr::concat_i64` |
   | `vector_i64_slice`     | `rt.arr::slice_i64` |
   | `vector_i64_builder_new` | `rt.arr::builder_new` |
   | `vector_i64_builder_from` | `rt.arr::builder_from_i64` |
   | `vector_i64_builder_push` | `rt.arr::builder_push_i64` |
   | `vector_i64_builder_freeze` | `rt.arr::builder_freeze_i64` |

3. Retarget stage0 codegen: when emitting `Vector<Int>` ops, call through
   the library ABI names. The linker resolves these against `rt.arr`
   exports (possibly via a thin forwarding module or direct aliasing).

4. A `boot/lib/vector_i64.tw` module provides stub function signatures
   whose bodies are replaced by the registry bindings at compile time. This
   gives the library ABI a Twinkle-level identity even though the
   implementation is substrate-provided.

Later, stub bodies can be replaced with real Twinkle implementations that
call substrate helpers directly (once the function-body-replacement path is
proven). Those substrate helpers may themselves be Rust-authored or
Twinkle-authored Wasm IR modules; this plan only cares that `boot/lib`
can bind them as compiler-owned runtime artifacts.

## Milestones

### Milestone 1: Library-Internal Builtin Category

Add a `LibraryInternal` dispatch kind to the intrinsics registry, distinct
from prelude builtins. Register the `Vector<Int>` library ABI entries.

### Milestone 2: Codegen Retarget

Have stage0 `Vector<Int>` codegen call library ABI names instead of `rt.arr`
names directly. The linker resolves them against `rt.arr` exports.

### Milestone 3: First boot/lib Stub Module

Create `boot/lib/vector_i64.tw` with stub signatures. Compile it through
stage0 and verify the linked output uses library ABI symbols.

Milestone 3 is the entry condition for `boot-lib-vector-consumption.md`
Milestone 2.

## Exit Criteria

This plan is complete when:

1. Library-internal builtins are registered and distinct from prelude
   builtins.
2. Stage0 codegen emits calls to library ABI names for `Vector<Int>` ops.
3. The linker resolves those names against `rt.arr` substrate exports.
4. A `boot/lib` stub module exercises the full path end-to-end.
