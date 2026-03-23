# Inherent Method Resolution via Type Origin

Last updated: 2026-03-23
Status: **Done** — Option B implemented

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

Additionally, transitive method resolution must work: if module C uses
module B which uses module A, and C receives a value of A's type from
B, C should be able to call A's inherent methods on that value without
ever importing A.

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

## Options Considered

### Option A: Register methods during destructured import

When `register_import_items` processes a type, also register its
methods in `ValueEnv` under internal qualified names.

**Rejected because:**
- Internal names (e.g. `TypeName.__method`) leak into ValueEnv and
  may appear in error messages
- Does not solve transitive resolution: if C gets a type from B
  without importing A, there's no import event to trigger registration
- Couples method availability to import style rather than type identity

### Option B: Resolve methods by type origin, bypassing ValueEnv (chosen)

Store `FunctionSignature` directly in `TypeEnv.methods` so the type
checker never needs ValueEnv for method resolution. Methods become a
property of the type itself, not of the import scope.

**Why Option B:**
- Completely decouples method resolution from module imports
- Transitive resolution works naturally: once a type's methods are
  registered (during its defining module's compilation), any module
  that receives a value of that type can call its methods
- Clean separation: TypeEnv owns method signatures, ValueEnv owns
  user-visible bindings
- No namespace pollution — no internal names leak into ValueEnv

### Option C: Always register methods for all types in the module

Rejected — registers methods for types you didn't import.

## Implementation

### Data model change (`src/types/env.rs`)

`TypeEnv.methods` value type changed from `String` to `MethodInfo`:

```rust
pub struct MethodInfo {
    pub func_name: String,                    // for lowerer FuncId resolution
    pub signature: Option<FunctionSignature>,  // None for builtins registered early
}
```

`Option<FunctionSignature>` because builtin methods (Vector, String,
etc.) are registered in `TypeEnv::new()` before their signatures exist
in ValueEnv. These fall back to ValueEnv lookup at call sites.

### Type checker changes (`src/types/check.rs`)

`try_synth_registered_method_call` and `synth_method_value_ref` now
use `info.signature` directly when `Some`, falling back to
`ValueEnv.get_function(&info.func_name)` for builtins.

### Import registration (`src/module/context.rs`)

`register_import_items`: when a type is imported via destructuring,
scans `exports.public_functions` for inherent methods and registers
them in `TypeEnv.methods` with their full signature + FuncId mapping.

### Transitive resolution

Two mechanisms ensure methods persist across module boundaries:

1. **TypeEnv snapshot/restore** — `restore_bindings` preserves new
   method entries for user-defined types (non-builtin TypeIds) that
   were added during dependency compilation. Builtin type methods are
   fully restored to maintain prelude isolation for stdlib modules.

2. **`method_func_targets`** — a persistent `HashMap<String,
   ExternalFuncRef>` on `CompileState` (NOT snapshot/restored) that
   accumulates FuncId mappings for user-defined type methods. Merged
   into `qualified_func_targets` when constructing `LowerInput` so the
   lowerer can resolve transitive method FuncIds.

### Boot compiler cleanup

Removed redundant workaround imports from 4 files:
- `boot/compiler/lower_core.tw` — removed `use .builtins`
- `boot/compiler/opt/pipeline.tw` — removed `use compiler.builtins`
- `boot/tests/suites/builtins_suite.tw` — removed `use compiler.builtins`
- `boot/tests/suites/core_ir_suite.tw` — removed `use compiler.builtins`

3 files retain both imports because the plain import is genuinely used
for qualified calls (not just method resolution):
- `boot/compiler/lexer.tw` — `tokens.make(...)`, `tokens.eof(...)`
- `boot/compiler/parser.tw` — `cursor.new(...)`
- `boot/compiler/lower_core.tw` — `checker.MethodCallInfo`

## Tests

| Test | Coverage |
|------|----------|
| `method_via_type_import` | Basic: `use vec2.{Vec2}` → `a.add(b)` |
| `method_via_type_import_negative` | Only imported type's methods resolve |
| `method_via_type_import_aliased` | `use vec2.{Vec2 as V}` → methods work |
| `method_via_type_import_multi` | `use shapes.{Circle, Rect}` — both get methods |
| `method_via_type_import_first_class` | `f := p.magnitude_sq` — method as value |
| `method_transitive_module` | C calls `.translate()` on A's type without importing A |
