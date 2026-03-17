# Bug: Record Field Type Leak Across Functions

**Severity:** Blocking (prevents boot type checker implementation)
**Component:** Stage0 type checker (`src/types/check.rs`)

---

## Symptom

When two or more functions construct the same record type (`InferCtx` with 7 fields), the type checker misassigns field types in the second function. Specifically:

- `ctx.type_var_scope` (declared `Vector<String>`) is typed as `Option<Vector<Dict<String, MonoType>>>` — the Optional-wrapped type of the `locals` field
- The "expected" type shown for `type_var_scope` is `Vector<Dict<String, MonoType>>` — the type of the `locals` field, not `type_var_scope`
- The first function checking the same record construction pattern works fine

## Reproduction

```tw
pub type InferCtx = .{
  env: ResolvedEnv,
  locals: Vector<Dict<String, MonoType>>,
  subst: Dict<Int, MonoType>,
  next_meta: Int,
  current_ret: MonoType?,
  type_var_scope: Vector<String>,
  type_map: Dict<Int, MonoType>,
}

// This works:
fn push_scope(ctx: InferCtx) InferCtx {
  InferCtx.{
    env: ctx.env, locals: ctx.locals.push(empty_scope()),
    subst: ctx.subst, next_meta: ctx.next_meta,
    current_ret: ctx.current_ret, type_var_scope: ctx.type_var_scope,
    type_map: ctx.type_map,
  }
}

// This fails with type mismatch on type_var_scope:
fn pop_scope(ctx: InferCtx) InferCtx {
  InferCtx.{
    env: ctx.env, locals: ctx.locals,
    subst: ctx.subst, next_meta: ctx.next_meta,
    current_ret: ctx.current_ret, type_var_scope: ctx.type_var_scope,
    type_map: ctx.type_map,
  }
}
```

Error:
```
type_var_scope: ctx.type_var_scope,
               ^--------------------------
note: Expected: Vector<Dict<String, MonoType>>
Actual:   Option<Vector<Dict<String, MonoType>>>
```

## Key Observations

1. **First function works, second doesn't** — `push_scope` (identical pattern) compiles; `pop_scope` fails
2. **Even delegation fails** — `fn pop_scope(ctx: InferCtx) InferCtx { push_scope(ctx) }` causes errors to appear on `push_scope` instead (the act of adding the second function breaks the first)
3. **Option wrapping** — The `current_ret: MonoType?` field (Optional) seems to "leak" its Optional wrapper onto adjacent field types in subsequent functions
4. **Field type confusion** — The expected type for `type_var_scope` matches the declared type of `locals`, suggesting field index confusion or stale type environment state

## Likely Cause

The stage0 type checker appears to carry stale or corrupted state between function-level type checking passes when records contain:
- Multiple Dict-parameterized fields with imported type arguments
- An Optional field (`MonoType?`)
- 7+ fields

Possible root causes to investigate:
- **MetaVar leakage** between function checking — unification variables from one function polluting the next
- **Field index mismatch** in named record construction — checking by position when field order is reused
- **TypeId/field resolution cache** — cached field types from the first function's checking being reused incorrectly

## Impact

Blocks `boot/compiler/checker.tw` — the type checker needs `InferCtx` constructed in ~10 functions. Only the first construction succeeds.

## Workaround

**Use COW field mutation instead of record construction.** Since Twinkle has COW semantics, mutate fields in place and return the record rather than constructing a new literal:

```tw
// Instead of:
fn push_scope(ctx: InferCtx) InferCtx {
  InferCtx.{
    env: ctx.env, locals: ctx.locals.push(empty_scope()),
    subst: ctx.subst, next_meta: ctx.next_meta,
    current_ret: ctx.current_ret, type_var_scope: ctx.type_var_scope,
    type_map: ctx.type_map,
  }
}

// Do:
fn push_scope(ctx: InferCtx) InferCtx {
  ctx.locals = ctx.locals.push(empty_scope())
  ctx
}
```

This avoids the bug entirely (no record literal construction) and is also more concise and idiomatic for a COW language.
