# Prelude Stdlib — Auto-Available Inherent Methods

## Goal

Make core stdlib modules (`vector`, `string_ext`, `numeric`, `dict_ext`)
available without explicit `use` imports. Their functions should work as
inherent methods on built-in types (e.g. `xs.map(f)`, `s.trim()`) and as
qualified calls (e.g. `Int.to_float(3)`) — same as today, but without
requiring `use @std.vector` etc.

These modules are conceptually part of the language prelude, unlike
`@std.fs`/`@std.path`/`@std.proc` which are optional system-facing modules.

---

## Current State

- `stdlib/vector.tw`, `stdlib/string_ext.tw`, `stdlib/numeric.tw`,
  `stdlib/dict_ext.tw` exist and work correctly with explicit `use @std.X`.
- Built-in methods (`.len()`, `.push()`, `.get()`, etc.) are hardcoded in the
  compiler as prelude intrinsics — no import needed.
- The stdlib-authored methods (`.map()`, `.filter()`, `.trim()`, etc.) require
  `use` because the module system compiles and registers them on demand.
- The linker has no dead-code elimination — all functions from all compiled
  modules are included in the output.

## Problem

A naive eager auto-import (compile all four modules for every file) causes:

1. **Output bloat**: every program includes ~20+ stdlib functions even if unused.
   `hello.tw` goes from 1 function to 12+ functions in ANF output.
2. **FuncId instability**: auto-imported functions consume FuncId slots before
   user functions, shifting all user FuncIds and breaking snapshot tests and
   any tooling that depends on stable IDs.
3. **No DCE**: the linker includes everything, so unused auto-imported functions
   persist all the way to the wasm output.

---

## Design Options

### Option A: Lazy auto-import (demand-driven)

Register module aliases and type signatures at startup without compiling the
modules. On first actual use (method call or qualified reference), trigger
compilation and linking of the required module.

**Pros**: no bloat, no wasted compilation.
**Cons**: requires splitting registration (types/methods) from compilation
(lowering/linking), which the pipeline doesn't currently support. Method
resolution happens during type-checking but module compilation needs the full
pipeline. Would need a two-phase approach or deferred compilation hooks.

### Option B: Prelude folder with special treatment

Move prelude stdlib files to `stdlib/prelude/` (or mark them with metadata).
Compile them once at startup and register their exports, but only link
functions that are actually referenced by the entry module.

**Pros**: clean separation between prelude and optional stdlib. Simple to
implement if combined with a basic DCE pass.
**Cons**: requires dead-code elimination in the linker to avoid bloat.

### Option C: DCE first, then eager auto-import

Add a dead-code elimination pass to the linker (reachability from `__init__`).
Then eagerly auto-import all prelude modules without worrying about bloat.

**Pros**: DCE is independently valuable. Auto-import becomes trivial once
unreferenced functions are pruned.
**Cons**: DCE is a nontrivial addition (must handle indirect calls via
closures, method tables, etc.).

### Option D: Compile-to-signature-only for prelude modules

At startup, parse and typecheck prelude modules to extract signatures and
method registrations, but skip lowering/codegen. When the linker encounters
an unresolved external reference to a prelude function, compile that module
on demand.

**Pros**: no bloat, signatures available early, compilation only when needed.
**Cons**: requires changes to linker to handle deferred compilation. More
complex than Option C.

---

## Recommended Approach: Option C (DCE + eager auto-import)

DCE is the right foundation because:

- It solves bloat for all cases, not just prelude auto-import.
- It's independently valuable (smaller wasm output for all programs).
- Once DCE exists, auto-import is trivial — just compile and register the
  modules; unreferenced functions get pruned automatically.
- The earlier auto-import prototype proved correctness — the only issue was
  bloat/FuncId instability, both solved by DCE.

---

## Implementation Plan

### Phase 1: Linker DCE

Add a reachability pass to `link()` in `src/module/mod.rs`:

1. Start from the entry module's `__init__` function.
2. Walk all `GlobalFunc(id)` references transitively.
3. Only include reachable functions in `linked_functions`.
4. Renumber FuncIds based on reachable set only.

Edge cases:
- `MakeClosure` references must be traced (closures are indirect calls).
- Module-level `__init__` functions for dependencies must be included
  if they have side effects (they initialize module globals).

Tests:
- Existing tests should continue to pass (all currently-used functions are
  reachable).
- New test: compile a file that imports a module but only uses some of its
  functions; verify unused ones are absent from output.

### Phase 2: Prelude auto-import

After DCE is in place:

1. Move prelude stdlib to `stdlib/prelude/` (or use a list in the compiler).
2. In `compile_module_with_adapter`, after explicit imports, auto-compile
   prelude modules and register their exports (same as the reverted prototype).
3. Guard: skip auto-import for files inside stdlib (avoid circular deps).
4. Guard: skip if the alias was already explicitly imported.

Tests:
- `xs.map(f)` works without `use @std.vector`.
- `s.trim()` works without `use @std.string_ext`.
- `3.to_float()` and `Int.to_float(3)` work without `use @std.numeric`.
- `d.values()` and `Dict.values(d)` work without `use @std.dict_ext`.
- Explicit `use @std.vector` still works and doesn't double-register.
- Output size: `hello.tw` still produces minimal wasm (DCE prunes unused).

### Phase 3: Cleanup

- Update `docs/design/stdlib.md` to document prelude vs optional distinction.
- Update `docs/API.md` to note that vector/string/numeric/dict methods are
  available without import.
- Remove `use @std.vector` etc. from test files and self-hosting code where
  no longer needed.

---

## File Layout (proposed)

```
stdlib/
  prelude/          # auto-imported, no `use` needed
    vector.tw
    string_ext.tw
    numeric.tw
    dict_ext.tw
  fs.tw             # optional, requires `use @std.fs`
  path.tw           # optional, requires `use @std.path`
  proc.tw           # optional, requires `use @std.proc`
```

---

## Risks

- **DCE correctness**: must handle all indirect call patterns (closures,
  function references stored in data structures). Missing a reference path
  would silently drop needed functions.
- **Init ordering**: module `__init__` functions may have side effects.
  DCE must preserve dependency init order even if no user function explicitly
  calls into the module.
- **FuncId stability across edits**: with DCE, adding/removing a function
  in one module could renumber others. This is already the case today but
  would be more visible with auto-imports. Not a correctness issue but
  affects debugging and snapshot tests.
