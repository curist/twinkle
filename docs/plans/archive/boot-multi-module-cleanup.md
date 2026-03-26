# Boot Multi-Module Cleanup Plan

Addresses workarounds and code smells identified in Steps 1–3 (commit 44f7e81).

## Issue 1: Duplicated helpers (`vec_contains`, `fs_error_message`)

**Problem:**
`vec_contains` is duplicated verbatim in `imports.tw` and `module_compiler.tw`.
`fs_error_message` is duplicated in `pipeline.tw` (private) and `module_compiler.tw`.
Any bug fix or new `FsError` variant requires updating two places.

**Fix:**
Create `boot/compiler/util.tw` with both helpers exported:

```tw
pub fn vec_contains(v: Vector<String>, item: String) Bool { ... }
pub fn fs_error_message(err: fs.FsError) String { ... }
```

Update `imports.tw`, `module_compiler.tw`, and `pipeline.tw` to import from `compiler.util`.

**Files:** `boot/compiler/util.tw` (new), `boot/compiler/imports.tw`, `boot/compiler/module_compiler.tw`, `boot/compiler/pipeline.tw`

---

## Issue 2: `merge_selective_imports` leaks full module namespace

**Problem:**
`merge_selective_imports` unconditionally calls `merge_module_exports(alias, exports)` on line 370, registering the full qualified namespace (`alias.func`, `alias.Type`) before adding the selected unqualified names. This means `use foo.bar.{fn1}` makes both `bar.fn1` and `fn1` visible.

The Rust stage0 (`register_import_items` in `src/module/context.rs:245`) does NOT register the full qualified alias — only the selected items under their unqualified (or aliased) names. The boot compiler is therefore more permissive and will accept code that stage0 rejects.

**Fix:**
Rewrite `merge_selective_imports` to register only the selected items without calling `merge_module_exports`. The function should:

1. For each `Value(item)`: look up `item.name` in `exports.functions`, register under `import_binding_name(item.name, item.alias)`.
2. For each `Type(item)`: look up `item.name` in `exports.types`, register under `import_binding_name(item.name, item.alias)`. Copy associated inherent methods.

No qualified `alias.*` names should be registered.

**Files:** `boot/compiler/resolver.tw`

**Test:** Add a test in `resolver_suite.tw` that asserts a selectively imported module's non-selected names are NOT visible in the env.

---

## Issue 3: Cache key mismatch between entry path and dependency paths

**Problem:**
`compile_module` in `module_compiler.tw:118` uses `path.normalize(file_path)` as the cache key. But `plan_dependencies` remaps symlinked `boot/prelude`/`boot/stdlib` paths through `canonical_module_path`. If an internal module is both the entry point and a transitive dependency, the cache keys won't match, causing double compilation.

**Fix:**
Use `canonical_module_path` (from `imports.tw`) as the cache key in `compile_module` instead of bare `path.normalize`. This requires either:
- Exporting `canonical_module_path` from `imports.tw`, or
- Extracting path canonicalization into `util.tw`.

Since `canonical_module_path` needs a `project_root`, and `compile_module` already has `state.project_root`, this is straightforward:

```tw
canonical := imports.canonical_module_path(file_path, state.project_root)
```

Also apply the same canonicalization to `entry_path` in `compile_entry` before passing it to `compile_module`.

**Files:** `boot/compiler/imports.tw` (export `canonical_module_path`), `boot/compiler/module_compiler.tw`

---

## Issue 4: `canonical_module_path` recomputes roots on every call

**Problem:**
`canonical_module_path` internally calls `loader.resolve_prelude_root` and `loader.resolve_stdlib_root` on every invocation. In `plan_dependencies`, it is called once per explicit import plus once per prelude module, each time recomputing the same root paths.

**Fix:**
Introduce a `CanonicalRoots` record that pre-computes the normalized roots once:

```tw
type CanonicalRoots = .{
  prelude_root: String,
  stdlib_root: String,
  parent_prelude: String,
  parent_stdlib: String,
}

pub fn make_canonical_roots(project_root: String) CanonicalRoots { ... }
pub fn canonical_module_path(path_str: String, roots: CanonicalRoots) String { ... }
```

`plan_dependencies` creates the roots once at the top and passes them through. This also benefits Issue 3 since `compile_module` can construct roots once from `state.project_root`.

**Files:** `boot/compiler/imports.tw`

---

## Issue 5: Comment the snapshot/restore pattern in `src/module/mod.rs`

**Problem:**
The double-restore loop (restore to clean base before recursion, restore to accumulated projection after) is correct but non-obvious. A reader could mistake it for a bug.

**Fix:**
Add a comment block before the loop explaining the two-phase restore:

```rust
// Two-phase snapshot/restore:
//
// compile_snapshot: clean env from before any deps. Each dep compiles
// against this (isolation — dep N cannot see projections from deps 1..N-1).
//
// projected_snapshot: accumulated env after projecting deps 1..N-1.
// Restored after each dep's recursive compilation returns, so we can
// project dep N's exports on top of the accumulated env.
//
// Fields outside the snapshot (next_func_id, module_hashes, etc.)
// accumulate naturally across both phases.
```

**Files:** `src/module/mod.rs`

---

## Issue 6: Only first diagnostic reported per stage

**Problem:**
`module_compiler.tw` uses `diagnostics[0].message` at each stage, swallowing all but the first error. This matches `pipeline.tw` and is consistent, but loses information.

**Fix:**
Low priority. When the boot compiler gains a proper diagnostics renderer, switch to collecting all diagnostics. For now, keep the single-error pattern but add a count hint:

```tw
msg := resolved.diagnostics[0].message
if resolved.diagnostics.len() > 1 {
  extra := resolved.diagnostics.len() - 1
  return .Err("resolve '${file_path}': ${msg} (and ${extra} more)")
}
return .Err("resolve '${file_path}': ${msg}")
```

This is a minor improvement that can be deferred.

**Files:** `boot/compiler/module_compiler.tw`

---

## Issue 7: `compile_entry` discards dependency modules (Step 4 gap)

**Problem:**
`compile_entry` compiles all dependencies into `compiled_modules` but then takes only the last one (the entry module) and runs monomorphize/lower-ANF on it. Imported functions referenced by `GlobalFunc(id)` in the entry module's Core IR point to `FuncId`s that only exist in discarded dependency modules. The multi_module_suite tests don't execute the output, so this is invisible.

**Fix:**
This is the Step 4 (Core IR linking) gap documented in `docs/plans/boot-multi-module.md`. No action here — just ensure the existing plan's Step 4 addresses:

1. Merge all `compiled_modules` into a single `CoreModule`
2. Remap `FuncId`s globally so cross-module references are consistent
3. Produce a linked init sequence (dependency init before entry)

The multi_module_suite should gain execution tests once linking lands.

**Not addressed by this plan.** See `docs/plans/boot-multi-module.md` Step 4.

---

## Execution Order

Issues are independent except where noted. Suggested order by impact:

1. **Issue 2** (selective import namespace leak) — behavioral correctness divergence from stage0
2. **Issue 3 + 4** (cache key + canonical roots) — do together since both touch `canonical_module_path`
3. **Issue 1** (duplicate helpers) — straightforward extraction
4. **Issue 5** (comment) — trivial
5. **Issue 6** (diagnostic count hint) — optional polish
6. **Issue 7** — deferred to Step 4
