# Phase E: Multi-Module Compilation for Boot Compiler

## Status

Partially landed.

Implemented:
- Import scanning and dependency planning
- Module export extraction and env merging
- Recursive multi-module compilation with cache and cycle detection
- Qualified module calls through checker/lowerer
- Import and end-to-end multi-module test coverage

Not yet implemented:
- Core IR linking across compiled modules
- Pipeline entrypoint for multi-module compilation
- `boot/main.tw` integration with the multi-module pipeline

This doc now reflects current repository state instead of the original greenfield plan.

## Current State

The boot compiler no longer behaves as a strict single-file compiler at the frontend level.
For a chosen entry file, it can:

1. Parse the entry module
2. Scan `use` imports
3. Resolve transitive dependencies recursively
4. Compile each dependency against the builtin/base env
5. Project dependency exports into the importing module env
6. Resolve, check, and lower each module

That work lives in:
- [imports.tw](/Users/curist/playground/rust/twinkle/boot/compiler/imports.tw)
- [resolver.tw](/Users/curist/playground/rust/twinkle/boot/compiler/resolver.tw)
- [module_compiler.tw](/Users/curist/playground/rust/twinkle/boot/compiler/module_compiler.tw)
- [checker.tw](/Users/curist/playground/rust/twinkle/boot/compiler/checker.tw)
- [lower_core.tw](/Users/curist/playground/rust/twinkle/boot/compiler/lower_core.tw)

The missing piece is that the compiled modules are not yet linked into one global Core IR module. `module_compiler.compile_entry()` still returns the entry module's lowered Core IR and leaves linking deferred.

## Landed Work

### Step 1: Import Scanner

Implemented in [imports.tw](/Users/curist/playground/rust/twinkle/boot/compiler/imports.tw).

Current behavior:
- Scans `Item.Use(decl)` entries
- Resolves:
  - stdlib imports via `resolve_stdlib_module_path`
  - relative imports via `resolve_relative_module_path`
  - absolute imports via `resolve_module_path(project_root, ...)`
- Computes import aliases from explicit `as` or last path segment
- Auto-injects prelude modules for non-internal modules
- Skips prelude injection for `prelude/` and `stdlib/`
- Deduplicates prelude modules against explicit imports
- Canonicalizes `boot/prelude` and `boot/stdlib` symlinked paths against the real top-level `prelude/` and `stdlib/`

Important cleanup already made:
- The temporary boot-specific absolute-import-root remap was removed
- Callers/tests now use the actual boot project root instead of relying on repo-root special casing

### Step 2: Module Exports and Environment Merging

Implemented in [resolver.tw](/Users/curist/playground/rust/twinkle/boot/compiler/resolver.tw).

Implemented pieces:
- `ExportedType`
- `ModuleExports`
- `extract_exports`
- `merge_module_exports`
- `merge_selective_imports`
- `merge_prelude_exports`

Behavior now supported:
- Exporting `pub` types and functions from a module
- Registering qualified imported names like `math.add` / `gfx.Point`
- Registering unqualified selective imports
- Registering prelude exports invisibly
- Preserving inherent methods for imported/prelude-visible types
- Resolving qualified type paths through the merged type namespace

### Step 3: Recursive Multi-Module Compilation

Implemented in [module_compiler.tw](/Users/curist/playground/rust/twinkle/boot/compiler/module_compiler.tw).

Implemented pieces:
- `CompileState`
- `compile_entry`
- `compile_module`
- export cache keyed by canonical module path
- circular import detection via `importing_stack`
- dependency isolation: deps compile against the base env, not the parent's merged env

Current compile flow:
1. Find project root from the entry file directory
2. Parse the module
3. Plan dependencies
4. Recursively compile dependencies
5. Merge dependency exports into a fresh env derived from builtins
6. Resolve/check/lower the current module
7. Cache exports and accumulate the lowered module

Additional support landed outside `module_compiler.tw`:
- [lower_core.tw](/Users/curist/playground/rust/twinkle/boot/compiler/lower_core.tw) now assigns `FuncId`s for visible imported functions, so qualified imported calls lower correctly
- [checker.tw](/Users/curist/playground/rust/twinkle/boot/compiler/checker.tw) and [resolver.tw](/Users/curist/playground/rust/twinkle/boot/compiler/resolver.tw) support module-qualified call/type resolution

## Remaining Work

### Step 4: Core IR Linking

Not implemented.

This remains the main functional gap.

What is missing:
- A boot-side Core IR linker that combines all compiled modules
- Global `FuncId` remapping across modules
- Rewriting inter-module `GlobalFunc(...)` references to globally linked IDs
- Producing a linked init/entry arrangement instead of just returning the entry module's Core IR

Current evidence:
- [module_compiler.tw](/Users/curist/playground/rust/twinkle/boot/compiler/module_compiler.tw) still contains the note `linking deferred to Step 4`
- `compiled_modules` are accumulated but not linked
- `compile_entry()` currently returns the last compiled module's `core`

Important nuance:
- There is already a Wasm linker in [codegen/linker.tw](/Users/curist/playground/rust/twinkle/boot/compiler/codegen/linker.tw), but that is later in the pipeline and is not the Core IR linker described by this plan

### Step 5: Pipeline and CLI Integration

Not implemented.

Still missing:
- `pipeline.compile_entry_path(...)`
- pipeline selection between single-file and multi-module entry compilation
- wiring `boot/main.tw` `ir`/`run`/`build` to the multi-module path

Current state:
- [pipeline.tw](/Users/curist/playground/rust/twinkle/boot/compiler/pipeline.tw) still exposes only `compile_source` and `compile_path`
- [main.tw](/Users/curist/playground/rust/twinkle/boot/main.tw) still calls `pipeline.compile_path(file)`

## Tests

Implemented test coverage:
- [imports_suite.tw](/Users/curist/playground/rust/twinkle/boot/tests/suites/imports_suite.tw)
- [multi_module_suite.tw](/Users/curist/playground/rust/twinkle/boot/tests/suites/multi_module_suite.tw)

These currently cover:
- import planning
- prelude injection behavior
- stdlib/relative/absolute import resolution
- selective import preservation
- single-module compile via `compile_entry`
- relative imports
- transitive dependency chains
- circular import detection

## Verified State

Currently verified passing:

```bash
cargo run --release -- run -i boot/tests/test_api.tw
cargo run --release -- run boot/tests/test_api.tw
```

Those include the landed import and multi-module suites.

## Recommended Next Batch

1. Add a boot Core IR linker, likely in `boot/compiler/core_linker.tw`
2. Change `module_compiler.compile_entry()` to link `compiled_modules` instead of returning the entry module directly
3. Add `pipeline.compile_entry_path(...)`
4. Update `boot/main.tw` to use the multi-module entry pipeline for file-based commands
5. Add end-to-end tests that require true cross-module linking rather than just successful per-module lowering

## File Reality Check

The original plan described several files as new. That is no longer true.

Already present and active:
- [imports.tw](/Users/curist/playground/rust/twinkle/boot/compiler/imports.tw)
- [module_compiler.tw](/Users/curist/playground/rust/twinkle/boot/compiler/module_compiler.tw)
- [imports_suite.tw](/Users/curist/playground/rust/twinkle/boot/tests/suites/imports_suite.tw)
- [multi_module_suite.tw](/Users/curist/playground/rust/twinkle/boot/tests/suites/multi_module_suite.tw)

Still effectively missing relative to the plan:
- `boot/compiler/core_linker.tw`
- multi-module pipeline entrypoint in [pipeline.tw](/Users/curist/playground/rust/twinkle/boot/compiler/pipeline.tw)
- CLI wiring in [main.tw](/Users/curist/playground/rust/twinkle/boot/main.tw)
