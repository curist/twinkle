# Prelude Stdlib — Auto-Available Inherent Methods

**Status: Implemented** (2026-03-09)

## Goal

Make core prelude modules (`vector`, `string`, `int`, `float`, `dict`)
available without explicit `use` imports. Their functions work as inherent
methods on built-in types (e.g. `xs.map(f)`, `s.trim()`) and as qualified
calls via canonical builtin aliases (e.g. `Int.to_float(3)`,
`Vector.map(xs, f)`) — no imports needed.

These modules are conceptually part of the language prelude, unlike
`@std.fs`/`@std.path`/`@std.proc` which are optional system-facing modules
that require explicit `use`.

---

## Architecture

### File layout

```
prelude/              # top-level, auto-imported, invisible to users
  vector.tw           (Vector methods: map, filter, fold, find, any, all, …)
  string.tw           (String methods: trim, split, index_of, contains, …)
  int.tw              (Int methods: to_float)
  float.tw            (Float methods: to_int)
  dict.tw             (Dict methods: values)
stdlib/               # optional, requires `use @std.X`
  fs.tw
  path.tw
  proc.tw
```

`prelude/` is a top-level directory, separate from `stdlib/`. Users cannot
import prelude modules directly — `@std.prelude.vector` resolves to the
non-existent `stdlib/prelude/vector.tw`, producing a normal "Cannot resolve
module" error with no mention of the real `prelude/` location.

### Linker DCE (`src/module/dce.rs`)

A reachability pass runs after `link()` on the linked `CoreModule`:

1. **Reference graph**: walks all function bodies collecting `GlobalFunc(id)`
   and `MakeClosure { func_id }` edges into an adjacency list.
2. **BFS from roots**: seeds with all `__init__` FuncIds from
   `all_init_func_ids`, then follows edges to compute the reachable set.
   Prelude intrinsic FuncIds (1–40) are always available and not tracked.
3. **Filter and renumber**: drops unreachable functions, then assigns compact
   sequential FuncIds (starting from `USER_FUNC_START`) sorted by original
   FuncId to preserve the linker's ID assignment order.
4. **Remap**: walks all remaining function bodies, `init_func_id`, and
   `all_init_func_ids` through the old-to-new FuncId mapping.

DCE is general-purpose — it benefits all modules, not just prelude.

### Prelude auto-import

In `compile_module_with_adapter`, after processing explicit `use` imports
and before computing dependency hashes:

1. **Enumerate** prelude modules via `ModuleSourceAdapter::list_prelude_modules()`
   (adapter-driven for both filesystem and source-map pipelines).
2. **Skip** if the current module is inside `stdlib/` or `prelude/` itself
   (avoids cycles).
3. **Canonical-path dedupe**: skip any prelude module already in the
   dependency list (by canonical path, not alias name).
4. **Compile and register** each prelude module under an internal alias
   (`__prelude_vector`, etc.) via `register_module_exports()`. This
   registers inherent methods for dot syntax and canonical builtin aliases
   (`Vector.map`, `Int.to_float`, etc.).
5. **Dependency tracking**: prelude modules are included in
   `dep_canonical_paths` and `dep_hash_entries` so cache invalidation
   works correctly.

### ModuleSourceAdapter

The trait has `list_prelude_modules()`, `stdlib_root()`, and `prelude_root()`
methods:

- **FsModuleSourceAdapter**: lists `prelude/*.tw` from disk via
  `resolve_prelude_root_default()`.
- **SourceMapModuleAdapter**: lists `*.tw` entries under the prelude root
  (derived as sibling of `stdlib_root`) from its in-memory source map.

### Path resolution

- `resolve_prelude_root_default()`: `TWINKLE_ROOT/prelude` or
  `CARGO_MANIFEST_DIR/prelude`.
- `resolve_stdlib_module_path_from_root()`: resolves `@std.*` imports to
  `stdlib/*.tw` only. The `prelude` segment is not special-cased — it
  naturally resolves to `stdlib/prelude/*.tw` which doesn't exist.

---

## Migration from previous layout

- `stdlib/vector.tw` → `prelude/vector.tw`
- `stdlib/string_ext.tw` → `prelude/string.tw` (renamed)
- `stdlib/dict_ext.tw` → `prelude/dict.tw` (renamed)
- `stdlib/numeric.tw` → split into `prelude/int.tw` + `prelude/float.tw`
  (removed, no compatibility shim)
- `use @std.vector`, `use @std.string_ext`, `use @std.dict_ext`,
  `use @std.numeric` are no longer needed and the old files no longer exist.

---

## Tests

- `tests/run/prelude_auto_import.tw` — dot-syntax methods without imports
  (Vector.map, String.trim, Int.to_float round-trip)
- `tests/run/prelude_qualified.tw` — canonical qualified calls without imports
  (Vector.map, String.trim, Int.to_float)
- `tests/dce_test.rs` — DCE removes unused imported functions, renumbers
  FuncIds compactly, preserves program correctness
- `src/module/dce.rs` unit tests — unreachable removal, renumbering, closure
  reachability, transitive reachability, init remapping, prelude refs
- All existing tests pass unchanged (DCE is a no-op when all functions are
  reachable; FuncId renumbering preserves original sorted order)
