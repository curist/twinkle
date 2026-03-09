# Prelude Stdlib — Auto-Available Inherent Methods

## Goal

Make core stdlib modules (`vector`, `string`, `int`, `float`, `dict`)
available without explicit `use` imports. Their functions should work as
inherent methods on built-in types (e.g. `xs.map(f)`, `s.trim()`) and as
qualified calls via canonical builtin aliases (e.g. `Int.to_float(3)`,
`Vector.map(xs, f)`) — same as today, but without
requiring `use @std.vector` etc.

These modules are conceptually part of the language prelude, unlike
`@std.fs`/`@std.path`/`@std.proc` which are optional system-facing modules.

---

## Current State

- `stdlib/vector.tw`, `stdlib/string_ext.tw`, `stdlib/numeric.tw`,
  `stdlib/dict_ext.tw` exist and work correctly with explicit `use @std.X`.
  As part of this work, `string_ext` and `dict_ext` will be renamed to
  `string` and `dict` (dropping the `_ext` suffix), and `numeric.tw` will be
  split into `int.tw` + `float.tw` then removed (no compatibility shim).
- Built-in methods (`.len()`, `.push()`, `.get()`, etc.) are hardcoded in the
  compiler as prelude intrinsics — no import needed.
- The stdlib-authored methods (`.map()`, `.filter()`, `.trim()`, etc.) require
  `use` because the module system compiles and registers them on demand.
- The linker has no dead-code elimination — all functions from all compiled
  modules are included in the output.

## Problem

A naive eager auto-import (compile all five prelude modules for every file) causes:

1. **Output bloat**: every program includes ~20+ stdlib functions even if unused.
   `hello.tw` goes from 1 function to 12+ functions in ANF output.
2. **FuncId instability**: auto-imported functions consume FuncId slots before
   user functions, shifting all user FuncIds and breaking snapshot tests and
   any tooling that depends on stable IDs.
3. **No DCE**: the linker includes everything, so unused auto-imported functions
   persist all the way to the wasm output.

---

## Chosen Approach: General DCE + Prelude Folder

Combines Option B's clean folder structure with Option C's general DCE:

- **General DCE** in the linker solves bloat for all modules (not just prelude),
  producing smaller wasm output for every program.
- **`stdlib/prelude/` folder** provides a clean, extensible convention for
  marking which stdlib modules are auto-imported.
- **Late FuncId assignment**: only reachable functions receive FuncIds,
  so adding unused prelude modules does not perturb linked user FuncIds.
  This avoids renumbering gaps and makes snapshot tests less sensitive to
  prelude changes.

---

## Implementation Plan

### Phase 1: Linker DCE

Add a reachability pass after `link()` assembles all functions in
`src/module/mod.rs`. This pass runs on the linked `CoreModule` before
returning it.

#### 1a. Build a reference graph

Walk every function body in `CoreModule.functions` and collect all
`GlobalFunc(id)` and `MakeClosure { func_id, .. }` references. Build a
`HashMap<FuncId, HashSet<FuncId>>` adjacency list.

#### 1b. Compute reachable set

Seed the worklist with:
- The entry module's `__init__` function (the program entry point).
- All dependency `__init__` functions from `all_init_func_ids` (they may
  have side effects like initializing module-level globals).

BFS/DFS from the seeds through the adjacency list. The result is a
`HashSet<FuncId>` of reachable functions.

#### 1c. Filter and renumber

- Drop unreachable functions from `CoreModule.functions`.
- Build a `HashMap<FuncId, FuncId>` old-to-new mapping that assigns
  compact sequential FuncIds (starting from `USER_FUNC_START`) to only
  the reachable functions, in their original deterministic linked order
  (i.e. the order they appear in `CoreModule.functions`).
- Walk all remaining function bodies and remap every `GlobalFunc(id)` and
  `MakeClosure { func_id }` through the old-to-new map.
- Update `init_func_id` and `all_init_func_ids` through the map.

Note: prelude intrinsic FuncIds (1–40) are never remapped — they're
handled by the runtime, not by user-module linking.

#### Edge cases

- **`MakeClosure`**: closures create indirect call references. The
  reachability walk must follow `MakeClosure { func_id }` edges just like
  `GlobalFunc(id)` edges.
- **`__init__` functions**: all module `__init__` functions are seeded as
  roots, since they may have side effects. A future optimization could
  analyze whether an `__init__` is pure and skip it, but that's not needed
  now.

#### Tests

- All existing tests should continue to pass (all currently-used functions
  are reachable by definition).
- New test: import a module, use only one of its functions. Verify that
  unused functions are absent from the linked output (check ANF function
  count or WAT output).

### Phase 2: Prelude auto-import

After DCE is in place, auto-import becomes safe since unused functions
are pruned.

#### 2a. Folder convention

Rename and move prelude stdlib files to `stdlib/prelude/`:
```
stdlib/
  prelude/          # auto-imported, no `use` needed
    vector.tw       (moved from stdlib/vector.tw)
    string.tw       (renamed from stdlib/string_ext.tw)
    int.tw          (split from stdlib/numeric.tw; Int receiver methods)
    float.tw        (split from stdlib/numeric.tw; Float receiver methods)
    dict.tw         (renamed from stdlib/dict_ext.tw)
  fs.tw             # optional, requires `use @std.fs`
  path.tw           # optional, requires `use @std.path`
  proc.tw           # optional, requires `use @std.proc`
```

`stdlib/prelude/numeric.tw` is intentionally **not** kept. `@std.numeric` is
removed as part of this migration.

Canonical builtin aliases `Vector`, `String`, `Int`, `Float`, and `Dict` are
reserved prelude names and are always available for qualified calls.

Prelude modules must not perform observable top-level initialization beyond
declarations; `__init__` should be empty or semantically trivial.

#### 2b. Auto-import in `compile_module_with_adapter`

After processing explicit `use` imports (around line 263 in
`src/module/mod.rs`), enumerate `stdlib/prelude/*.tw` and for each file:

1. **Canonical-path dedupe (not alias dedupe)** — resolve each prelude file
   to canonical path and skip only if that same canonical module path is
   already in the current module's dependency list. Do **not** use alias-name
   checks (`vector`, `string`, etc.) for dedupe, because imports support
   `as` aliases and alias names may refer to unrelated modules.
2. **Skip ambient injection inside stdlib** — prelude auto-import applies to
   user modules, not stdlib internals. Stdlib modules must declare
   dependencies explicitly (which also avoids prelude dependency cycles).
3. **Compile the prelude module** via the same recursive
   `compile_module_with_adapter` call used for explicit imports.
4. **Register exports** via `register_module_exports()` — this makes
   functions available as qualified calls (`Vector.map`) and registers
   inherent methods for dot syntax (`xs.map(f)`).
5. **Do not expose lowercase prelude aliases by default** — auto-imported
   prelude modules should be compiled under reserved internal aliases (for
   example `__prelude_vector`) to avoid collisions with user aliases and to
   keep the public surface to method syntax + canonical builtin aliases only.

Auto-import does not guarantee lowercase aliases.

Important ordering: prelude auto-import must run **before** dependency hashes
are computed for resolve/typecheck/lower cache keys so implicit dependencies are
tracked the same way as explicit imports.

#### 2b.1 Dependency tracking / cache correctness

Treat auto-imported prelude modules as ordinary dependencies:

- Include their canonical paths in `dep_canonical_paths`.
- Include their module hashes in `dep_hash_entries` (thus in `deps_hash` and
  `module_hash`).
- Store them via `with_global_cache(|cache| cache.set_dependencies(...))`.

This ensures edits to prelude modules invalidate and recompile dependents,
including modules that use prelude methods without explicit `use`.

#### 2c. Path resolution

`loader.rs` should understand the new `stdlib/prelude/` layout for compiler
internals and future extensibility, but this plan does not make any new
user-facing guarantees about importing prelude modules directly.

#### 2d. Adapter support (filesystem + source map)

`compile_module_with_adapter` is shared by filesystem compilation and
`compile_entry_from_source_map`, so prelude discovery must be adapter-driven.

- Extend `ModuleSourceAdapter` with a method that returns prelude module
  candidates in deterministic order (sorted by canonical path or alias).
- `FsModuleSourceAdapter`: list `stdlib/prelude/*.tw` from disk.
- `SourceMapModuleAdapter`: list `*.tw` entries under `<stdlib_root>/prelude/`
  from its in-memory source map.

Do not hardcode filesystem-only globbing inside the shared compile path.

#### Tests

- `xs.map(f)` works without `use @std.vector`.
- `Vector.map(xs, f)` works without `use @std.vector`.
- `s.trim()` works without `use @std.string`.
- `String.trim(s)` works without `use @std.string`.
- `3.to_float()` and `Int.to_float(3)` work without `use @std.int`.
- `1.5.to_int()` and `Float.to_int(1.5)` work without `use @std.float`.
- `d.values()` and `Dict.values(d)` work without `use @std.dict`.
- Existing code using `use @std.string_ext` / `use @std.dict_ext` /
  `use @std.numeric` should drop those imports once prelude auto-import
  is working.
- Without explicit import, lowercase qualified calls like `vector.map(xs, f)`
  are not guaranteed; canonical aliases (`Vector.*`, `String.*`, `Int.*`,
  `Float.*`, `Dict.*`) are the guaranteed qualified surface.
- Output size: `hello.tw` still produces minimal wasm (DCE prunes unused
  prelude functions).
- A program that uses no prelude methods should produce identical output
  to today (no extra functions in WAT).
- Cache invalidation: changing `stdlib/prelude/vector.tw` invalidates and
  recompiles a user module that uses `xs.map` without explicit import.
- Source-map pipeline: `compile_entry_from_source_map` auto-imports prelude
  modules the same way as filesystem compilation.

### Phase 3: Cleanup

- Update `docs/API.md` to note that vector/string/int/float/dict methods
  are available without import.
- Remove `use @std.vector` etc. from test files where no longer needed.
- Consider updating error messages to suggest prelude methods when a user
  tries to call an unknown method on a built-in type.

---

## Risks

- **DCE correctness**: must trace all indirect call patterns (closures,
  function refs in `MakeClosure`). Missing a reference edge would silently
  drop a needed function. Mitigation: thorough test coverage including
  closures passed to higher-order functions.
- **Init ordering**: all `__init__` functions are conservatively kept as
  roots. This is safe but means unused prelude modules still run their
  init code. In practice prelude modules have no top-level side effects
  (they only define functions), so this is a non-issue.
- **Snapshot test churn**: DCE + late renumbering will change FuncIds in
  existing snapshot tests. This is a one-time update when DCE lands.
