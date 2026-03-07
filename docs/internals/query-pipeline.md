# Query-Friendly Pipeline

This document describes the refactoring of the compiler pipeline from a
mutation-through-shared-context design into pure functions with explicit
inputs and outputs.

---

## Motivation

The original pipeline uses a mutable `CompilationContext` god object that
accumulates everything. This works for batch compilation but creates friction
for:

- **LSP**: needs to re-check a single file, not re-run the whole pipeline
- **Formatter / linter**: need to invoke only parse or parse+typecheck
- **Testing**: stages can't be exercised in isolation
- **Self-hosting**: Rust-specific frameworks (like Salsa) cannot be ported

The goal is to reshape each stage into a pure function with a separate linking
step that combines per-module artifacts. No framework dependency.

---

## Target Architecture

Each stage becomes a pure function:

```
parse(source) -> SourceFile

resolve(ast, deps: &[ModuleExports]) -> ResolvedModule

typecheck(ast, resolved: &ResolvedModule) -> TypedModule

lower(ast, typed: &TypedModule) -> LoweredModule

link(modules: &[LoweredModule]) -> LinkedProgram
```

The function signature IS the interface contract. No hidden mutation.

### Per-Module Artifact Structs

```rust
struct ResolvedModule {
    type_env: TypeEnv,
    value_env: ValueEnv,
    func_ids: HashMap<String, FuncId>,
    exports: ModuleExports,
}

struct TypedModule {
    type_map: TypeMap,    // ExprId → MonoType for this file
}

struct LoweredModule {
    functions: Vec<FunctionDef>,
    init_func: Option<FunctionDef>,
    module_id: ModuleId,
}

struct LinkedProgram {
    functions: Vec<FunctionDef>,
    type_env: TypeEnv,        // merged
    init_order: Vec<FuncId>,
}
```

### Stable FuncId Assignment

Each module assigns FuncIds locally (0, 1, 2...). The linker applies a
per-module base offset when combining — same model as object files with
relocations. Prelude FuncIds remain fixed.

**Status (Stage 6b)**: Module-local user FuncIds are implemented in lowering,
and the linker remaps all references using deterministic module ordering.

---

## Stage-by-Stage Changes

### resolve

Pre-assign module-local FuncIds and register inherent methods — these are part
of name resolution semantically. `ResolvedModule` carries the assigned `func_ids`
and method table alongside `type_env` and `value_env`. Multi-module: the caller
passes `deps: &[ModuleExports]` instead of a mutable context.

### typecheck

Runs with explicit `ResolvedModule` input, returns `TypedModule`. No env swap
pattern.

### lower

Receives `(ast, typed, resolved)` explicitly. Assigns module-local FuncIds
starting from 0. `LocalAllocator` stays per-function.

### link

Explicit step (previously implicit `ctx.all_functions.extend(...)`):

1. Topological-sort modules by dependency order
2. Assign base offsets per module
3. Remap all FuncId references from local → global
4. Merge TypeEnvs
5. Return `LinkedProgram`

### compile_module (orchestrator)

After the refactor, a thin coordinator:

```rust
fn compile_module(path, deps: &[ModuleExports]) -> Result<LoweredModule> {
    let source = fs::read_to_string(path)?;
    let ast = parse(&source)?;
    let resolved = resolve(&ast, deps)?;
    let typed = typecheck(&ast, &resolved)?;
    lower(&ast, &typed, &resolved)
}
```

No mutable shared state. The caller decides what to cache and when to invalidate.

---

## Content-Hash Caching

Once stages are pure functions, caching is straightforward:

```rust
struct StageCache {
    parsed:   HashMap<u64, SourceFile>,
    resolved: HashMap<u64, ResolvedModule>,
    typed:    HashMap<u64, TypedModule>,
    lowered:  HashMap<u64, LoweredModule>,
}
```

A file with unchanged source hash and deps hashes skips all stages.

Current status: in-process cache is implemented for parse/resolve/typecheck/lower.
On-disk persistence is out of scope for now.

---

## Why Not Salsa

[Salsa](https://github.com/salsa-rs/salsa) would give automatic memoization
and fine-grained invalidation, but it uses Rust-specific macros and proc-macro
infrastructure that cannot be ported to a self-hosted Twinkle compiler.

The pure-functions-with-explicit-deps shape gives the same architecture without
the framework. If Salsa is ever desirable (e.g. for a long-lived Rust LSP before
self-hosting), adding `#[salsa::tracked]` attributes is mechanical once the
architecture is right.

---

## Benefits

- **Testing**: each stage can be unit-tested with explicit inputs
- **Formatter / linter**: call `parse()` or `resolve()` independently
- **LSP**: re-run only stages affected by the changed file
- **Module boundaries**: function signatures document dependencies
- **Self-hosting**: same architecture translates directly to Twinkle
