# Boot Resolver — Method Registry

**File:** `boot/compiler/resolver.tw`
**Depends on:** resolver already functional (types, functions, arity checks)
**Blocks:** `boot/compiler/checker.tw` — string interpolation validation, method call type checking

---

## Problem

The boot resolver (`ResolvedEnv`) currently tracks types and function signatures but has no method registry. In stage0, `TypeEnv` maintains a `methods: HashMap<(TypeId, String), String>` mapping `(type_id, method_name) → function_name`, which is used by:

1. **Type checker** — validating string interpolation (`${x}` requires `x`'s type to have `to_string`)
2. **Type checker** — typing method calls (`x.method(args)` → look up method, check receiver type)
3. **Lowerer** — desugaring method calls to global function calls

Without this, the boot checker:
- Cannot validate interpolation types (currently allows all `Named` types, should check for `to_string`)
- Cannot type-check method calls (currently relies on the resolver pre-resolving them to global calls, which only works for user-defined functions in the same module)

### Why wasn't this in the original resolver plan?

The resolver was designed as a minimal two-pass system: collect names (Pass 1), resolve type references (Pass 2). It handles single-module resolution — type declarations and function signatures within one file.

Method registration is inherently a **cross-module** concern: `x.push(1)` on `Vector<Int>` resolves to a function defined in the prelude, not in the current module. The original resolver plan deferred multi-module features to Phase E (Integration). But the type checker needs method awareness even for single-module checking — user-defined types with methods in the same file, and builtin type methods for interpolation.

---

## Design

### New field in `ResolvedEnv`

```tw
pub type ResolvedEnv = .{
  types: Vector<TypeEntry>,
  type_names: Vector<String>,
  functions: Vector<FunctionSig>,
  methods: Dict<String, Vector<MethodEntry>>,  // type_name → methods
}

pub type MethodEntry = .{
  method_name: String,
  function_name: String,     // the global function this resolves to
}
```

Using `Dict<String, Vector<MethodEntry>>` keyed by type name (not TypeId) because:
- Builtin types (Vector, String, Dict, Cell) don't have TypeIds in the current system
- Type name is what the checker has when resolving `x.method()` — it zonks the type and checks the name

### Method lookup

```tw
pub fn lookup_method(env: ResolvedEnv, type_name: String, method_name: String) String?
```

Returns the global function name if found.

### Registration

Methods are registered in two ways:

1. **Builtin methods** — pre-populated in `test_env()` and eventually in the real compilation env. These map builtin type methods like `Vector.push`, `String.len`, `Dict.keys`, etc.

2. **User-defined inherent methods** — a function whose first parameter type matches a type defined in the same module is an inherent method. The resolver can detect this in Pass 2 after types and functions are both resolved.

---

## Milestones

### M1 — Data structures and lookup ✅

Add `MethodEntry` type, `methods` field to `ResolvedEnv`, and `lookup_method` function. Update `empty_env()`. No behavioral change — methods dict starts empty.

**Tests:**
- `lookup_method` on empty env returns `.None`
- Manual method registration + lookup returns function name

### M2 — Builtin method registration for tests ✅

Add a `register_builtin_methods(env)` helper that populates the method registry for all builtin/prelude types. Covers all dot-callable methods from docs/API.md:
- `String`: core (`len`, `get`, `slice`, `concat`, `char_code_at`, `utf8_bytes`, `to_string`, `compare`) + prelude (`index_of`, `contains`, `starts_with`, `ends_with`, `split`, `trim`) + unicode (`chars`, `char_len`, `code_point_at`, `graphemes`)
- `Vector`: `len`, `push`, `get`, `set`, `concat`, `slice`, `map`, `filter`, `fold`, `find`, `any`, `all`, `contains`, `reverse`, `sort_by`, `join`
- `Dict`: `len`, `has`, `keys`, `values`, `get`, `set`, `remove`
- `Cell`: `get`, `set`, `update`
- `Int`: `to_string`, `to_float`, `compare`
- `Float`: `to_string`, `to_int`, `compare`
- `Bool`: `to_string`
- `Byte`: `to_string`, `to_int`, `compare`
- `Option`: `map`, `and_then`, `ok_or`, `ok_or_else`, `transpose`
- `Result`: `map`, `and_then`, `transpose`
- `Iterator`: `next`, `map`, `filter`, `take`, `to_vector`

Update `test_env()` in `checker_suite.tw` to call this.

**Tests:**
- `lookup_method(env, "String", "len")` returns `Some("string_len")`
- `lookup_method(env, "Vector", "push")` returns `Some("vector_push")`

### M3 — Checker uses method registry for interpolation ✅

Update `is_interpolatable` in `checker.tw` to check the method registry: a `Named` type is interpolatable only if it has a `to_string` method registered. Primitives (Int, Float, Bool, Byte, String) remain always-interpolatable.

**Tests:**
- `"${n}"` where `n: Int` → ok
- `"${p}"` where `p: Point` (no to_string) → diagnostic
- `"${p}"` where `p: Point` (has to_string registered) → ok

### M4 — Auto-detect inherent methods in same module ✅

In Pass 2, after resolving functions: for each function whose first parameter is a `Named(tid, ...)` type, register it as a method on that type. This handles user-defined inherent methods within the same module.

**Tests:**
- `type Point = .{ x: Int }\nfn to_string(p: Point) String { "" }` → `to_string` registered on Point
- Method not registered if first param isn't a named type
- Auto-detect method on sum type

### M5 — Checker validates method calls (stretch)

Currently, `x.method(args)` is resolved by the parser/resolver to a `Call(GlobalFunc, [x, ...args])`. The checker types this as a regular function call. For same-module functions this works. For cross-module methods (like prelude `Vector.push`), the resolver would need multi-module support.

This milestone is deferred to when multi-module resolution is implemented. The current approach (resolver resolves method calls to known functions) is sufficient for single-module checking.

---

## Current Checker Gaps Addressed

| Gap | Milestone |
|-----|-----------|
| Interpolation accepts any Named type | M3 — validates to_string exists |
| String interpolation tests can't run | M3 — tests use test_env with registered methods |
| User type to_string not detected | M4 — auto-registers from same-module functions |
| Cross-module method calls | Deferred (M5 / Phase E) |

---

## Files to Modify

- **Modify:** `boot/compiler/resolver.tw` — add `MethodEntry`, `methods` to `ResolvedEnv`, `lookup_method`, auto-detection in Pass 2
- **Modify:** `boot/compiler/checker.tw` — update `is_interpolatable` to use method registry
- **Modify:** `boot/tests/suites/checker_suite.tw` — update `test_env()` with method registration, add interpolation tests
- **Modify:** `boot/tests/suites/resolver_suite.tw` — method registry tests
