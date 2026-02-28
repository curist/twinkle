# Query-Friendly Pipeline Refactor

## Motivation

The current pipeline is a linear, mutation-through-shared-context design. Every stage
reaches into `CompilationContext` and mutates it. This works fine for batch compilation
but creates friction for:

- **LSP**: needs to re-check a single file after a keystroke, not re-run the whole pipeline
- **Formatter / linter**: need to invoke only parse or parse+typecheck independently
- **Testing**: stages can't be exercised in isolation without constructing a full context
- **Self-hosting**: the eventual Twinkle-written compiler must replicate the architecture — a Rust-specific framework (like Salsa) cannot be ported

The goal of this refactor is to reshape each stage into a **pure function with explicit
inputs and outputs**, with a separate linking step that combines per-module artifacts.
No new framework dependency. The pattern is language-agnostic and translates directly
to Twinkle when the time comes.

---

## Current State

`CompilationContext` is a mutable god object that accumulates everything:

```rust
pub struct CompilationContext {
    pub type_env: TypeEnv,                              // grows across modules
    pub value_env: ValueEnv,                            // grows across modules
    pub func_table: HashMap<String, FuncId>,            // bare + qualified names
    pub next_func_id: u32,                              // global monotonic counter
    pub all_functions: Vec<FunctionDef>,                // accumulates FuncDefs
    pub module_registry: HashMap<String, ModuleExports>,
    pub module_aliases: HashSet<String>,
    pub module_cache: HashMap<PathBuf, ModuleExports>,  // only existing cache
    pub all_init_func_ids: Vec<FuncId>,
    pub init_func_id: Option<FuncId>,
    pub next_global_local_id: u32,                      // for module-level lets
    pub qualified_value_globals: HashMap<String, LocalId>,
}
```

`compile_module` drives everything by mutating this context throughout:
resolve → typecheck → lower → accumulate into ctx. The function signature says nothing
about what it needs or what it produces.

**Problems for incremental / independent use:**

- FuncIds are counter-based (order-dependent, not stable across re-compilations)
- No way to re-run just the type checker for one file — it needs a fully populated ctx
- Tests must run the full pipeline to exercise any single stage
- Module cache is per-invocation only; nothing persists across `twk` runs

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

The function signature IS the interface contract. No hidden mutation through a shared object.

### Per-Module Artifact Structs

```rust
// What resolver produces
struct ResolvedModule {
    type_env: TypeEnv,        // this module's type definitions
    value_env: ValueEnv,      // this module's function signatures
    func_ids: HashMap<String, FuncId>,  // pre-assigned, module-local
    exports: ModuleExports,
}

// What type checker produces
struct TypedModule {
    type_map: TypeMap,        // ExprId → MonoType for this file
    // carries resolved by reference; no duplication
}

// What lowerer produces (module-local FuncIds: 0, 1, 2, ...)
struct LoweredModule {
    functions: Vec<FunctionDef>,
    init_func: Option<FunctionDef>,
    module_id: ModuleId,
}

// What the linker produces (FuncIds remapped with module offsets)
struct LinkedProgram {
    functions: Vec<FunctionDef>,
    type_env: TypeEnv,         // merged
    init_order: Vec<FuncId>,
}
```

### Stable FuncId Assignment

Currently FuncIds are assigned from a global counter — the 15th function encountered
across all modules gets `FuncId(15)`. This breaks caching because the same function
gets a different FuncId depending on import order.

**Target**: each module assigns FuncIds locally (0, 1, 2...). The linker applies a
per-module base offset when combining. Same model as object files with relocations.

The prelude retains fixed FuncIds (as today). User-module FuncIds become
`module_base_offset + local_index`. The base offset is determined by topological order
of dependencies, computed once by the linker.

---

## Stage-by-Stage Changes

### resolve

Currently, `compile_module` does significant work around the resolver call that must
move into (or alongside) `resolve` itself:

- **Before** calling `Resolver::resolve_with_context`: pre-assigns FuncIds for this
  module's functions into `ctx.func_table` and increments `ctx.next_func_id`.
- **After**: manually registers inherent methods into `ctx.type_env` / `ctx.value_env`.

These steps are part of name resolution semantically. After the refactor, they become
part of `resolve`'s internal logic; `ResolvedModule` carries the pre-assigned `func_ids`
and the registered method table alongside `type_env` and `value_env`.

Multi-module: the caller passes in `deps: &[ModuleExports]` (already-resolved imports)
instead of a mutable context. Resolution itself stays a two-pass scan of the AST.

### typecheck

Two entry points currently exist: `check_module(ast, type_env, value_env)` (already
takes explicit args) and `check_module_with_context(&mut ctx)` (uses shared state,
swapping envs in/out with `mem::replace`). The refactor unifies them behind a single
pure interface: `typecheck(ast, resolved: &ResolvedModule) -> TypedModule`.

The `ResolvedModule` carries `type_env` and `value_env` — so `check_module` is already
close to the target shape. The main work is eliminating `check_module_with_context` and
ensuring no ctx mutation leaks into the type checker.

### lower

Receives `(ast, typed, resolved)` explicitly. Assigns module-local FuncIds starting
from 0. Returns `LoweredModule` with those local IDs.

`LocalAllocator` stays per-function (no change needed).

### link (new step)

Currently implicit: `ctx.all_functions.extend(...)` in `compile_module`.

The linker becomes an explicit step:
1. Topological-sort modules by dependency order
2. Assign base offsets: `module[i].base = sum of prev modules' function counts`
3. Remap all FuncId references in FunctionDefs from local → global
4. Merge TypeEnvs
5. Return `LinkedProgram`

The existing `all_init_func_ids: Vec<FuncId>` ordering logic moves here.

**Open design question — module-level globals**: `CompilationContext` currently tracks
`next_global_local_id` and `qualified_value_globals` to give module-level `let` bindings
stable `LocalId`s accessible across functions. These have no direct equivalent in a
per-module `LoweredModule`. Options:
- Give each `LoweredModule` its own `globals: Vec<(LocalId, CoreExpr)>` with module-local
  IDs; the linker remaps them just like FuncIds.
- Or make the linker assign GlobalIds in a separate namespace (separate from LocalIds)
  so cross-module global references are unambiguous.

This is the most non-trivial design question in the refactor and should be resolved
before starting the lower stage changes.

### compile_module (orchestrator)

After the refactor, `compile_module` becomes a thin coordinator:

```rust
fn compile_module(path, deps: &[ModuleExports]) -> Result<LoweredModule> {
    let source = fs::read_to_string(path)?;
    let ast = parse(&source)?;
    let resolved = resolve(&ast, deps)?;
    let typed = typecheck(&ast, &resolved)?;
    lower(&ast, &typed, &resolved)
}
```

No mutable shared state. The caller (CLI or future LSP) decides what to cache
and when to invalidate.

---

## Optional: Content-Hash Caching

Once stages are pure functions, adding a cache layer is straightforward:

```rust
struct StageCache {
    // key: hash of (source_text + deps_hashes)
    parsed:  HashMap<u64, SourceFile>,
    resolved: HashMap<u64, ResolvedModule>,
    typed:   HashMap<u64, TypedModule>,
    lowered: HashMap<u64, LoweredModule>,
}
```

A file that hasn't changed (same source hash, same deps hashes) skips all stages.
This can be persisted to disk across `twk` runs.

This is NOT needed immediately — the pure-function shape is the prerequisite. Add
caching only when batch compilation speed becomes a real issue.

---

## Why Not Salsa

[Salsa](https://github.com/salsa-rs/salsa) (used by rust-analyzer) would give us
automatic memoization, fine-grained invalidation, and cycle detection. However:

- It uses Rust-specific macros, trait objects, and proc-macro infrastructure
- A self-hosted Twinkle compiler cannot use Salsa — it's unportable
- Option B gives us the same *shape* (pure functions, explicit deps) without the framework
- If Salsa is ever desirable (e.g. for a long-lived Rust LSP server before self-hosting),
  the functions-with-explicit-deps shape makes it easy to slot in — adding
  `#[salsa::tracked]` attributes is mechanical once the architecture is right

---

## Benefits

**Testing**: each stage can be unit-tested by constructing its explicit inputs directly.
No need to build a full `CompilationContext` to test the type checker.

**Formatter / linter**: can call `parse()` or `resolve()` independently without touching
the rest of the pipeline. See [tooling.md](tooling.md).

**LSP**: on a keystroke, re-run only the stages affected by the changed file. Upstream
modules (already compiled, hash unchanged) return cached artifacts immediately.

**Module boundaries**: the function signature documents exactly what each stage depends
on. No hidden coupling through shared mutable state.

**Self-hosting**: the same architecture (pure function per stage, explicit structs, linker
step) is straightforward to implement in Twinkle. No framework, no macros.

---

## Implementation Order

This refactor can be done incrementally without breaking the existing test suite:

1. **Define the artifact structs** (`ResolvedModule`, `TypedModule`, `LoweredModule`,
   `LinkedProgram`) — initially just wrappers around existing types
2. **Refactor `resolve`** to return `ResolvedModule` instead of mutating `ctx`
3. **Refactor `typecheck`** to take explicit inputs, return `TypedModule`
4. **Refactor `lower`** to use module-local FuncIds, return `LoweredModule`
5. **Extract `link`** as an explicit step from what `compile_module` currently does
   implicitly
6. **Slim down `CompilationContext`** — most of it dissolves into the artifact structs;
   only the module loader cache (dedup) and import stack (circular detection) remain
7. **Update CLI commands** to call stages explicitly rather than through `ctx`

Each step is independently testable. The existing integration tests (`tests/run/`,
`tests/modules/`) validate correctness throughout.
