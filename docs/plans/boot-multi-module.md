# Phase E: Multi-Module Compilation for Boot Compiler

## Status

Partially landed.

Implemented:

- import scanning and dependency planning
- stdlib / relative / absolute import resolution
- prelude auto-injection for non-internal modules
- module export extraction and env merging
- recursive multi-module compilation with cache and cycle detection
- qualified module calls through resolver/checker/lowerer
- Core IR linking across compiled modules
- import and end-to-end multi-module test coverage

Not yet implemented:

- pipeline entrypoint for multi-module file compilation
- `boot/main.tw` integration with the multi-module pipeline

This document reflects the current repository state rather than the original
greenfield plan.

## Current State

The boot compiler no longer behaves as a strict single-file compiler at the
frontend level when you go through
[`module_compiler.compile_entry`](../../boot/compiler/module_compiler.tw).

For a chosen entry file, it can:

1. parse the entry module
2. scan `use` imports
3. resolve stdlib, relative, and absolute module paths
4. auto-inject prelude modules for non-internal modules
5. resolve transitive dependencies recursively
6. compile each dependency against the builtin/base env
7. project dependency exports into the importing module env
8. resolve, check, and lower each module
9. link compiled modules into one Core IR module

That work lives in:

- [`boot/compiler/imports.tw`](../../boot/compiler/imports.tw)
- [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw)
- [`boot/compiler/module_compiler.tw`](../../boot/compiler/module_compiler.tw)
- [`boot/compiler/core_linker.tw`](../../boot/compiler/core_linker.tw)
- [`boot/compiler/checker.tw`](../../boot/compiler/checker.tw)
- [`boot/compiler/lower_core.tw`](../../boot/compiler/lower_core.tw)

The remaining integration gap is that the ordinary boot file pipeline still goes
through [`boot/compiler/pipeline.tw`](../../boot/compiler/pipeline.tw)
`compile_path(...)`, and
[`boot/main.tw`](../../boot/main.tw) still calls that single-file path for
commands like `ir`.

## Landed Work

### Step 1: Import Scanner

Implemented in [`boot/compiler/imports.tw`](../../boot/compiler/imports.tw).

Current behavior:

- scans `Item.Use(decl)` entries
- resolves:
  - stdlib imports via `resolve_stdlib_module_path`
  - relative imports via `resolve_relative_module_path`
  - absolute imports via `resolve_module_path(project_root, ...)`
- computes import aliases from explicit `as` or last path segment
- auto-injects prelude modules for non-internal modules
- skips prelude injection for `prelude/` and `stdlib/`
- deduplicates prelude modules against explicit imports
- canonicalizes `boot/prelude` and `boot/stdlib` symlinked paths against the
  real top-level `prelude/` and `stdlib/`

Important cleanup already made:

- the temporary boot-specific absolute-import-root remap was removed
- callers/tests now use the actual boot project root instead of relying on
  repo-root special casing

### Step 2: Module Exports and Environment Merging

Implemented in [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw).

Implemented pieces:

- `ExportedType`
- `ModuleExports`
- `extract_exports`
- `merge_module_exports`
- `merge_selective_imports`
- `merge_prelude_exports`

Behavior now supported:

- exporting `pub` types and functions from a module
- registering qualified imported names like `math.add` / `gfx.Point`
- registering unqualified selective imports
- registering prelude exports invisibly
- preserving inherent methods for imported / prelude-visible types
- resolving qualified type paths through the merged type namespace

### Step 3: Recursive Multi-Module Compilation

Implemented in
[`boot/compiler/module_compiler.tw`](../../boot/compiler/module_compiler.tw).

Implemented pieces:

- `CompileState`
- `compile_entry`
- `compile_module`
- export cache keyed by canonical module path
- circular import detection via `importing_stack`
- dependency isolation: deps compile against the base env, not the parent's
  merged env

Current compile flow:

1. find project root from the entry file directory
2. parse the module
3. plan dependencies
4. recursively compile dependencies
5. merge dependency exports into a fresh env derived from builtins
6. resolve/check/lower the current module
7. cache exports and accumulate the lowered module

Additional support landed outside `module_compiler.tw`:

- [`boot/compiler/lower_core.tw`](../../boot/compiler/lower_core.tw) assigns
  `FuncId`s for visible imported functions, so qualified imported calls lower
  correctly
- [`boot/compiler/checker.tw`](../../boot/compiler/checker.tw) and
  [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw) support
  module-qualified call/type resolution

### Step 4: Core IR Linking

Implemented in [`boot/compiler/core_linker.tw`](../../boot/compiler/core_linker.tw)
and wired from
[`boot/compiler/module_compiler.tw`](../../boot/compiler/module_compiler.tw).

Behavior now supported:

- combines compiled modules into one linked `CoreModule`
- assigns globally unique `FuncId`s across modules
- remaps cross-module `GlobalFunc(...)` / closure references
- builds combined init ordering across module init functions
- runs lightweight reachability filtering before monomorphization

Current evidence:

- `module_compiler.compile_entry()` links `state.compiled_modules` before
  monomorphization / ANF / optimization
- the multi-module suite asserts linked output contains functions from multiple
  modules

Important nuance:

- there is also a later Wasm linker in
  [`boot/compiler/codegen/linker.tw`](../../boot/compiler/codegen/linker.tw)
- that linker is downstream of Core IR linking and solves a different problem

## Remaining Work

### Step 5: Pipeline and CLI Integration

Not implemented.

Still missing:

- `pipeline.compile_entry_path(...)` or equivalent multi-module file entrypoint
- pipeline selection between single-file source compilation and multi-module
  file compilation
- wiring `boot/main.tw` `ir` / `run` / `build` to the multi-module path

Current state:

- [`boot/compiler/pipeline.tw`](../../boot/compiler/pipeline.tw) still exposes
  `compile_source` and `compile_path`
- [`boot/main.tw`](../../boot/main.tw) still calls
  `pipeline.compile_path(file)`

## Tests

Implemented test coverage:

- [`boot/tests/suites/imports_suite.tw`](../../boot/tests/suites/imports_suite.tw)
- [`boot/tests/suites/multi_module_suite.tw`](../../boot/tests/suites/multi_module_suite.tw)

These currently cover:

- import planning
- prelude injection behavior
- stdlib / relative / absolute import resolution
- selective import preservation
- single-module compile via `compile_entry`
- relative imports
- transitive dependency chains
- circular import detection
- linked Core IR output spanning multiple modules

## Verified State

Currently verified passing:

```bash
cargo run --release -- run -i boot/tests/test_api.tw
cargo run --release -- run boot/tests/test_api.tw
```

Those include the landed import and multi-module suites.

## Recommended Next Batch

1. Add `pipeline.compile_entry_path(...)` or equivalent.
2. Route `boot/main.tw` file-based commands through the multi-module entry
   pipeline.
3. Add end-to-end tests that exercise the CLI / pipeline integration path
   rather than only calling `module_compiler.compile_entry(...)` directly.
4. Remove any stale comments/docs that still describe Core IR linking as
   missing.

## File Reality Check

The original plan described several files as new. That is no longer true.

Already present and active:

- [`boot/compiler/imports.tw`](../../boot/compiler/imports.tw)
- [`boot/compiler/module_compiler.tw`](../../boot/compiler/module_compiler.tw)
- [`boot/compiler/core_linker.tw`](../../boot/compiler/core_linker.tw)
- [`boot/tests/suites/imports_suite.tw`](../../boot/tests/suites/imports_suite.tw)
- [`boot/tests/suites/multi_module_suite.tw`](../../boot/tests/suites/multi_module_suite.tw)

Still effectively missing relative to the plan:

- multi-module pipeline entrypoint in
  [`boot/compiler/pipeline.tw`](../../boot/compiler/pipeline.tw)
- CLI wiring in [`boot/main.tw`](../../boot/main.tw)
