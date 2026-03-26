# Phase E: Multi-Module Compilation for Boot Compiler

## Context

The boot compiler (self-hosted in Twinkle) currently compiles only single files. The resolver skips `UseDecl` items, `builtin_env()` is the only source of bindings, and the pipeline takes a source string — not a module graph. Phase E adds multi-module support so the boot compiler can compile programs with `use` imports.

Foundation libraries are ready: module loader (`boot/lib/module/loader.tw`), dependency graph (`boot/lib/graph/dependency.tw`), query keys (`boot/lib/query/keys.tw`).

## Architecture Overview

The Rust stage0 pattern:
1. **Plan**: scan AST for imports → list of `PlannedDependency`
2. **Compile recursively**: for each dep, snapshot env → compile → restore → project exports
3. **Link**: merge Core IR modules, remap FuncIds to global namespace

The boot compiler will follow the same pattern, adapted to its functional env-threading style.

## Implementation Plan

### Step 1: Import Scanner (`boot/compiler/imports.tw`)

New file. Scans parsed AST for `UseDecl` items, resolves file paths, produces a dependency plan.

**Types:**
```
type DependencyKind = { Import, Prelude }

type PlannedDep = .{
  canonical_path: String,
  alias: String,
  kind: DependencyKind,
  items: Vector<ImportItem>?,  // selective imports
}

type DependencyPlan = .{
  dependencies: Vector<PlannedDep>,
}
```

**Functions:**
- `fn plan_dependencies(module: Module, file_path: String, project_root: String) Result<DependencyPlan, String>`
  - Iterates `module.items`, extracts `Item.Use(decl)` entries
  - For each UseDecl:
    - `is_stdlib` → resolve via `loader.resolve_stdlib_module_path()`
    - `is_relative` → resolve via `loader.resolve_relative_module_path()`
    - else → resolve via `loader.resolve_module_path()` from project root
  - Compute alias from `decl.alias` or last path segment
  - Auto-add prelude modules (for non-prelude/stdlib files) via `loader.list_prelude_modules()`
  - Deduplicate prelude against explicit imports

**Key files:**
- `boot/compiler/ast.tw:16-40` — `UseDecl`, `ImportItem` types (already exist)
- `boot/lib/module/loader.tw` — path resolution functions

### Step 2: Module Exports & Environment Merging (extend `boot/compiler/resolver.tw`)

Add types and functions for extracting what a module exports and merging those into an importing module's env.

**New types in resolver.tw:**
```
type ModuleExports = .{
  types: Vector<ExportedType>,
  functions: Vector<FunctionSig>,
  methods: Dict<String, Vector<MethodEntry>>,
}

type ExportedType = .{
  name: String,
  entry: TypeEntry,
}
```

**New functions in resolver.tw:**
- `fn extract_exports(env: ResolvedEnv, module: Module) ModuleExports`
  - Scan module items for `pub` types and functions
  - Collect their resolved entries from env
  - Include inherent methods for exported types

- `fn merge_module_exports(env: ResolvedEnv, alias: String, exports: ModuleExports) ResolvedEnv`
  - For each exported type: `add_type(entry, "alias.TypeName")`
  - For each exported function: `add_function(sig_with_qualified_name)`
  - Register in methods dict: `methods["alias"] += [MethodEntry.{ method_name: "func", function_name: "alias.func" }]`
  - Register inherent methods for imported types (so dot syntax works)

- `fn merge_selective_imports(env: ResolvedEnv, alias: String, exports: ModuleExports, items: Vector<ImportItem>) ResolvedEnv`
  - First call `merge_module_exports` for qualified access
  - Then for each ImportItem: add unqualified binding too

- `fn merge_prelude_exports(env: ResolvedEnv, exports: ModuleExports) ResolvedEnv`
  - Register functions directly (unqualified) — prelude functions are globally available
  - Register inherent methods for prelude types
  - No module alias (prelude is invisible)

**Fix `resolve_type_path` for qualified types:**
- Currently errors on multi-segment paths (line 646: "qualified type paths are not yet supported")
- Change to: join segments with "." → look up `"alias.TypeName"` in `type_index`

### Step 3: Multi-Module Compiler (`boot/compiler/module_compiler.tw`)

New file. Orchestrates recursive module compilation with caching and cycle detection.

**Types:**
```
type CompileState = .{
  cache: Dict<String, ModuleExports>,     // canonical_path → exports
  compiled_modules: Vector<CompiledModule>,  // for linking
  project_root: String,
}

type CompiledModule = .{
  path: String,
  core: CoreModule,
  env: ResolvedEnv,
}
```

**Functions:**
- `fn compile_entry(path: String) Result<PipelineArtifacts, String>`
  - Find project root via `loader.find_project_root()`
  - Initialize CompileState with empty cache
  - Call `compile_module()` for entry file
  - Link all compiled modules
  - Mono → ANF → optimize the linked result

- `fn compile_module(file_path: String, alias: String, base_env: ResolvedEnv, state: CompileState, importing_stack: Vector<String>) Result<CompileModuleResult, String>`
  - Canonicalize path, check cache → return cached exports
  - Check `importing_stack` for cycles → error
  - Read & parse source file
  - Plan dependencies via `imports.plan_dependencies()`
  - For each dependency:
    - Recursively `compile_module(dep_path, dep_alias, base_env, state, stack.push(file_path))`
    - Merge dep's exports into current env (qualified + selective)
  - Resolve module with accumulated env
  - Type-check
  - Lower to Core IR
  - Extract exports
  - Cache exports, accumulate CompiledModule
  - Return exports

**Key insight from Rust stage0:** Dependencies are compiled against the **base** env (builtins only), not the parent's accumulated env. Each dependency gets a clean env plus its own transitive deps. Only the dependency's exports are projected into the parent.

### Step 4: Core IR Linking

New file or extend `boot/compiler/core_linker.tw`.

- `fn link_core_modules(modules: Vector<CompiledModule>, entry_path: String, builtins: BuiltinRegistry) CoreModule`
  - Topo-sort modules by dependency order
  - For each module, remap local FuncIds to globally unique IDs
  - Merge all function definitions
  - Wire up entry module's init as the linked init

**FuncId remapping approach (from Rust stage0 `src/module/mod.rs:856+`):**
- Each module has local FuncIds starting at `USER_FUNC_START`
- Linker assigns global IDs sequentially across modules (in topo order)
- Builds `local_to_global: Dict<Int, Int>` per module
- Walks all function bodies, remapping `GlobalFunc(local_id)` → `GlobalFunc(global_id)`

### Step 5: Pipeline Integration

Update `boot/compiler/pipeline.tw`:
- Add `compile_entry_path(path: String) Result<PipelineArtifacts, String>` that uses module_compiler
- Keep `compile_source()` and `compile_path()` for single-file (test) usage

Update `boot/main.tw`:
- Wire `ir` command to use `compile_entry_path` when multi-module is needed
- Eventually wire `run` and `build` commands

## File Changes Summary

| File | Action |
|------|--------|
| `boot/compiler/imports.tw` | **New** — import scanning & dependency planning |
| `boot/compiler/resolver.tw` | **Modify** — add ModuleExports, merge functions, fix qualified type paths |
| `boot/compiler/module_compiler.tw` | **New** — recursive compile loop |
| `boot/compiler/core_linker.tw` | **New** — Core IR linking with FuncId remapping |
| `boot/compiler/pipeline.tw` | **Modify** — add `compile_entry_path()` |
| `boot/main.tw` | **Modify** — wire multi-module compilation |
| `boot/tests/suites/imports_suite.tw` | **New** — tests for import scanning |
| `boot/tests/suites/multi_module_suite.tw` | **New** — end-to-end multi-module tests |

## Phasing

**Batch 1 (Steps 1-2):** Import scanner + exports/merging. Testable in isolation with unit tests.

**Batch 2 (Step 3):** Recursive compile loop. Testable with simple two-file programs.

**Batch 3 (Steps 4-5):** Core IR linking + pipeline integration. Full end-to-end multi-module compilation.

## Verification

```bash
# Run existing tests (should still pass — no regressions)
cargo run --release -- run boot/tests/test_api.tw

# Run new import/multi-module tests
cargo run --release -- run boot/tests/test_api.tw
# (after adding new suites to test_api.tw)

# Manual test: create a two-file program and compile via boot compiler
# boot/tests/fixtures/multi/a.tw imports boot/tests/fixtures/multi/b.tw
```

## Key Reference Files

- `src/module/planner.rs` — Rust import scanning (reference for Step 1)
- `src/module/context.rs` — Rust CompileState, export registration (reference for Step 2-3)
- `src/module/mod.rs:412-696` — Rust compile_module loop (reference for Step 3)
- `src/module/mod.rs:856-955` — Rust link() (reference for Step 4)
- `boot/compiler/checker.tw:2516-2541` — `collect_module_aliases` (already reads UseDecl!)
- `boot/compiler/checker.tw:746-773` — `try_synth_module_qualified_call` (uses method table for module calls)
- `boot/compiler/lower_core.tw:678-717` — lowerer handles module-qualified calls via `method_calls` + `func_table`
