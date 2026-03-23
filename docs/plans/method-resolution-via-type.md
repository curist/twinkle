# Inherent Method Resolution via Type Origin

Last updated: 2026-03-23

## Problem

After fixing destructured imports to not bring the parent module into
scope, inherent method resolution breaks when only the type is imported:

```tw
use compiler.builtins.{BuiltinRegistry}

reg: BuiltinRegistry = ...
reg.id("foo")  // "No such field" — method resolution fails
```

The workaround is to add a redundant module import:

```tw
use compiler.builtins              // needed just for method resolution
use compiler.builtins.{BuiltinRegistry}
```

This defeats the purpose of destructured imports. Importing a type
should be sufficient to call its inherent methods.

## Root Cause

Method resolution currently requires the defining module to be in the
value environment. The flow:

1. Type checker gets receiver type → extracts `TypeId`
2. Looks up `TypeEnv.methods[(TypeId, method_name)]` → gets qualified
   function name (e.g., `"builtins.id"`)
3. Fetches function signature from `ValueEnv.get_function(func_name)`

Step 2 works — the method is registered in `TypeEnv.methods` during
module compilation (before import filtering). But step 3 fails because
the qualified function name `"builtins.id"` was registered by
`register_module_exports`, which is now skipped for destructured
imports.

The deeper issue: **methods are stored with qualified names that
require the module alias to exist in the value environment**. This
couples method resolution to module import style.

## Current Architecture

### Registration (`src/module/context.rs`)

`register_module_exports(alias, exports)`:
- Adds `alias` to `module_aliases` set
- Registers qualified function names: `"{alias}.{func}"` in `ValueEnv`
- Registers methods: `TypeEnv.methods[(type_id, method)] = "{alias}.{func}"`

`register_import_items(alias, exports, items)`:
- Registers only listed names **unqualified** in `ValueEnv`
- Does NOT register methods or the module alias

### Resolution (`src/types/check.rs`)

`try_synth_registered_method_call()`:
1. `method_receiver_type_id(base_ty)` → `TypeId`
2. `type_env.get_method_function(type_id, method)` → qualified func name
3. `value_env.get_function(func_name)` → `FunctionSignature`

Step 3 fails because the qualified function name was never registered.

### Key Data Structures (`src/types/env.rs`)

```
TypeEnv.methods: HashMap<(TypeId, String), String>
  Key: (TypeId, method_name)
  Value: qualified function name (e.g., "builtins.id")
```

## Proposed Fix

### Option A: Register methods during destructured import (recommended)

When `register_import_items` processes a destructured import that
includes a type, also register any inherent methods for that type.

**Algorithm:**

In `register_import_items(alias, exports, items)`, after registering
each type item:
1. Find all methods in `exports` whose receiver type matches the
   imported type's `TypeId`
2. For each such method, register it in `TypeEnv.methods` with an
   unqualified function name
3. Also register the method function in `ValueEnv` under its
   unqualified name (if not already imported)

This means `use foo.{MyType}` implicitly brings `MyType`'s inherent
methods into the value environment — not as user-visible names, but
as targets for method dispatch.

**Changes:**

1. `src/module/context.rs` — `register_import_items`:
   - After importing a type, scan `exports.public_functions` for
     functions whose first parameter type matches the imported TypeId
   - Register each as a method in `TypeEnv` and as a function in
     `ValueEnv` (using an internal qualified name to avoid polluting
     user namespace)

2. `ModuleExports` — add a `methods_by_type` index:
   - `HashMap<TypeId, Vec<(String, FunctionSignature)>>` — pre-computed
     during module compilation so `register_import_items` doesn't need
     to scan all functions

**Pros:** Minimal change, method resolution "just works" when you
import a type. No visible namespace pollution.

**Cons:** Importing a type silently makes its methods available, which
is a slight expansion of what "importing a name" means.

### Option B: Resolve methods by type origin, bypassing ValueEnv

Change method resolution to not go through `ValueEnv` at all. Instead,
store the full `FunctionSignature` directly in `TypeEnv.methods` (or a
parallel structure), so step 3 doesn't need a name lookup.

**Changes:**

1. `TypeEnv.methods` value type changes from `String` (func name) to
   a struct containing `FunctionSignature` + `FuncId`
2. `try_synth_registered_method_call` uses the stored signature
   directly instead of looking it up by name
3. `register_module_exports` stores the full signature at registration
   time

**Pros:** Completely decouples method resolution from module imports.
Clean separation of concerns.

**Cons:** Larger refactor. `TypeEnv.methods` becomes heavier. Need to
update IR lowering too (currently uses the qualified name to resolve
`FuncId`).

### Option C: Always register methods, never register the module alias

A middle ground: `register_import_items` always registers methods for
all types defined in the imported module (regardless of which specific
names were imported), but still doesn't add the module alias to
`module_aliases`.

**Pros:** Simple — just move the method registration loop from
`register_module_exports` into a shared helper called by both paths.

**Cons:** Registers methods for types you didn't import, which could
cause surprising resolution in edge cases.

## Recommendation

**Option A** is the best balance of correctness and minimal change.
It follows the principle of least surprise: if you imported a type,
its methods work. If you didn't import it, they don't.

Option B is the "correct" long-term architecture but is a larger
refactor that can be done later if needed.

## Impact on Boot Compiler

Once this fix lands, the workaround `use .foo` + `use .foo.{Bar}`
lines added in the destructured import fix can be simplified back to
just `use .foo.{Bar}` in files that only need the type + its methods.

Affected files (currently using the workaround):
- `boot/compiler/lexer.tw`
- `boot/compiler/parser.tw`
- `boot/compiler/lower_core.tw`
- `boot/compiler/opt/pipeline.tw`
- `boot/tests/suites/builtins_suite.tw`
- `boot/tests/suites/core_ir_suite.tw`

## Test Plan

1. Remove the `use compiler.builtins` workaround from a boot file,
   keep only `use compiler.builtins.{BuiltinRegistry}`
2. Verify `reg.id("foo")` resolves correctly
3. Verify `BuiltinRegistry.id(reg, "foo")` qualified syntax still
   works (this goes through a different path)
4. Verify methods for non-imported types do NOT resolve
5. Add a module test: import only a type via destructuring, call its
   inherent method
