# Plan: `Stringify` Contract for `Vector<T>`

## Goal

Make `${vec}` string interpolation work for `Vector<T>` when `T` conforms to
`Stringify` (has a zero-arg `to_string() -> String`).

Expected output:
```tw
println("${[1, 2, 3]}")   // [1, 2, 3]
println("${["a", "b"]}")  // [a, b]
```

---

## Background

The `Stringify` contract requires `to_string(self) -> String` on the receiver
type. Currently supported for primitives (`Int`, `Float`, `Bool`, `Byte`,
`String`) and user `Named` types. `Vector<T>` is not handled — the type
checker rejects `${vec}` even when the element type is stringifiable.

String interpolation `${expr}` touches two compiler passes:

1. **Typecheck** (`types/check.rs` — `validate_interpolation_to_string`):
   verifies that the expression type conforms to `Stringify`.
2. **Lowering** (`ir/lower.rs`): desugars `${expr}` to `expr.to_string()` and
   emits Core IR.

The prelude already has the implementation:

```tw
// prelude/vector.tw — already in place
pub fn to_string<T>(xs: Vector<T>, f: fn(T) String) String {
  "[" + xs.map(f).join(", ") + "]"
}
```

The challenge is wiring `${vec}` to call this with the right element-level
`to_string` function synthesized automatically, without introducing generic
bounds in the source language.

---

## Design

### Step 1 — Register `Vector.to_string` as a builtin method

**File:** `src/types/env.rs`

Add to the `builtin_methods` table in `TypeEnv::new`:

```rust
(BUILTIN_VECTOR_TYPE_ID, "to_string", "Vector.to_string"),
```

This makes `type_env.get_method(BUILTIN_VECTOR_TYPE_ID, "to_string")` resolve,
which is required both for typecheck (step 2) and for lowering method dispatch
(step 3) to reach the intercept point.

### Step 2 — Typecheck: extend `validate_interpolation_to_string`

**File:** `src/types/check.rs`

Add a `MonoType::Vector(elem)` arm to `validate_interpolation_to_string`:

```rust
MonoType::Vector(elem_ty) => match elem_ty.as_ref() {
    MonoType::Vector(_) => {
        self.errors.push(TypeError::UnsupportedFeature {
            feature: "string interpolation",
            span: expr.span,
            note: "Nested Vector<Vector<T>> interpolation is not yet supported".to_string(),
        });
        Err(())
    }
    other => self.validate_interpolation_to_string(expr, other),
},
```

MVP accepts flat `Vector<T>` where `T` is stringifiable; rejects nested vectors
with a clear diagnostic rather than accepting them at typecheck and panicking in
the lowerer. This preserves the invariant that a successfully-typechecked
program can always be lowered.

### Step 3 — Lowering: `synthesize_stringify_fn`

**File:** `src/ir/lower.rs`

Add a helper that, given a concrete element type, returns a `CoreExpr` of type
`fn(T) String` pointing to the right `to_string` function. Each synthesized
node must carry an explicit `fn(T) String` type annotation since these nodes
have no `type_map` entry.

| `ty` | synthesized expr |
|------|-----------------|
| `MonoType::Int` | `GlobalFunc(INT_TO_STRING)` with ty `fn(Int) String` |
| `MonoType::Float` | `GlobalFunc(FLOAT_TO_STRING)` with ty `fn(Float) String` |
| `MonoType::Bool` | `GlobalFunc(BOOL_TO_STRING)` with ty `fn(Bool) String` |
| `MonoType::String` | `GlobalFunc(STRING_TO_STRING)` with ty `fn(String) String` |
| `MonoType::Byte` | `GlobalFunc(BYTE_TO_STRING)` with ty `fn(Byte) String` |
| `MonoType::Named { type_id, .. }` | resolve FuncId via `type_env.get_method(type_id, "to_string")` then `resolve_named_func_id`; wrap as `GlobalFunc` with ty `fn(Named{..}) String` |
| anything else | `None` (unsupported — lowering should emit an `InternalError`) |

For the `Named` case: `get_method` returns a `MethodInfo` with a qualified
`func_name` (e.g. `"MyModule.to_string"`). That name must be passed through
`resolve_named_func_id` to get the concrete `FuncId` — the FuncId is not
directly in `MethodInfo`. The synthesized `GlobalFunc` node's `ty` must be
constructed explicitly as `MonoType::Function { params: vec![ty.clone()], ret:
Box::new(MonoType::String) }`.

### Step 4 — Lowering: intercept `to_string` in Vector method dispatch

**File:** `src/ir/lower.rs`

When lowering a method call `expr.to_string()` where `expr` has type
`MonoType::Vector(elem_ty)`, intercept it after the method lookup resolves
`Vector.to_string` (which now works due to step 1) but before the argument list
is constructed. Emit:

```
Call(
  GlobalFunc(vector_to_string_func_id),
  [expr, synthesize_stringify_fn(elem_ty)]
)
```

The normal one-argument dispatch would produce a type-mismatch; the intercept
injects the synthesized second argument. This is consistent with how the user
would write `vec.to_string(Int.to_string)` manually.

### Step 5 — Monomorphization (no changes needed)

Once the lowerer emits concrete calls to `Vector.to_string` with a specific
element function, the existing mono pass specializes the generic prelude
function automatically — the same way `map`, `filter`, etc. are handled today.

---

## Files to touch

| File | Change |
|------|--------|
| `prelude/vector.tw` | Already done: `pub fn to_string<T>(xs, f) String` |
| `src/types/env.rs` | Register `(BUILTIN_VECTOR_TYPE_ID, "to_string", "Vector.to_string")` in builtin methods |
| `src/types/check.rs` | `validate_interpolation_to_string`: add `MonoType::Vector` arm with nested guard |
| `src/ir/lower.rs` | Add `synthesize_stringify_fn`; intercept `to_string` on `Vector` in method dispatch |

---

## Scope

**MVP (this plan):**
- Flat `Vector<T>` where `T` is a primitive or `Named` type with `to_string`.
- `Vector<Vector<T>>` produces a clear unsupported-feature diagnostic.

**Follow-up:**
- Nested `Vector<Vector<T>>`: requires hoisting a helper `FunctionDef` during
  lowering to represent `fn(xs: Vector<inner>) String` as a first-class value,
  then remove the nested-vector guard in step 2.
- `Dict<K, V>`: analogous design, same four-step approach, `to_string` renders
  as `{k: v, ...}`.

---

## Test plan

**Happy path** — add to `tests/run/stdlib_vector_string_ext.tw`:

```tw
println("${[1, 2, 3]}")          // [1, 2, 3]
println("${([] : Vector<Int>)}") // []
println("${["a", "b", "c"]}")    // [a, b, c]
println("${[true, false]}")      // [true, false]

// user Named type with to_string defined
type Point = { x: Int, y: Int }
pub fn to_string(p: Point) String { "(${p.x}, ${p.y})" }
pts := [Point{ x: 1, y: 2 }, Point{ x: 3, y: 4 }]
println("${pts}")                // [(1, 2), (3, 4)]
```

**Error path** — add a typecheck error test:

```tw
// Vector<T> where T has no to_string should produce E_UNSUPPORTED_FEATURE
// Vector<Vector<Int>> should produce "not yet supported" diagnostic
```

---

## Relation to `contracts.md`

This plan is a historical sketch for making `Stringify` work for a generic
container. In the newer contracts direction, the type checker extension in step
2 would be a contract satisfaction check, and the lowering in steps 3–4 would
be contract-backed lowering for interpolation.

The capability-record function (`f: fn(T) String`) discussed here reflects the
older design exploration and is not the preferred contracts-based direction.

See [contracts.md](contracts.md) for the current design vocabulary and model.
