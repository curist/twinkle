# Stage 2/3 Integration Requirements

This document tracks the gaps between current Stage 2 (type checking) implementation and what Stage 3 (Core IR lowering) requires.

## Status: Phase 0 Complete, Gaps Documented

Phase 0 successfully added the **infrastructure** for type annotation (ExprId, TypeMap), but several **Stage 2 features** needed by Stage 3 are not yet implemented. This is intentional - they will be added as part of Phase 1-2 of Stage 3.

---

## 1. Method Resolution (Not Yet Implemented)

### Current State
- `TypeMap::method_calls` field exists but is **never populated**
- `synth_field_access` in `check.rs` only handles record fields
- No inherent method resolution logic in typechecker
- Test `field_method_collision.tw` exists but incorrectly passes (treats `p.len` as field)

### What's Needed (Stage 3 Phase 1-2)

Before lowering can begin, **one of these must be implemented**:

#### Option A: TypeChecker Annotates Method Calls (Preferred)
1. During type checking, when encountering `receiver.name(args)`:
   - Determine if it's a field access or method call
   - If method: resolve to `FuncId` and call `type_map.set_method_call(call_expr_id, func_id)`
2. TypeEnv needs: `fn has_method(&self, ty: &MonoType, name: &str) -> bool`
3. TypeEnv needs: `fn get_method_func_id(&self, ty: &MonoType, name: &str) -> Option<FuncId>`
4. Collision check: if both field and method exist, emit `TypeError::FieldMethodCollision`

#### Option B: Separate Resolution Pass
- Add a "resolve methods" pass between type checking and lowering
- Populates `TypeMap::method_calls` by walking the typed AST
- Same helpers needed from TypeEnv

**Decision required:** Pick Option A or B before Phase 1.

---

## 2. Field vs Method Collision Detection (Not Implemented)

### Current State
- `TypeError::FieldMethodCollision` exists
- No logic in `synth_field_access` to detect collisions
- TypeEnv has no method registry

### What's Needed
Implement in `synth_field_access` (or equivalent):

```rust
fn synth_field_access(&mut self, base: &Expr, name: &str, span: Span) -> Result<MonoType, ()> {
    let recv_ty = self.synth_expr(base)?;

    let has_field = self.type_env.has_field(&recv_ty, name);
    let has_method = self.type_env.has_method(&recv_ty, name);

    if has_field && has_method {
        self.errors.push(TypeError::FieldMethodCollision {
            type_name: recv_ty.to_string(),
            name: name.to_string(),
            span,
        });
        return Err(());
    }

    if has_field {
        // existing field logic
    } else if has_method {
        // resolve method FuncId, record in TypeMap
    } else {
        // error: no such field or method
    }
}
```

---

## 3. Loop Typing (Not Implemented)

### Current State
- Loops return `UnsupportedFeature` error in typechecker
- `Break`/`Continue` not type-checked
- No loop type inference

### What's Needed (Per Updated Plan)
Move loop typing to typechecker. Algorithm:

```rust
fn synth_loop(&mut self, body: &Block, span: Span) -> Result<MonoType, ()> {
    // Collect all Break expressions in body
    let break_types = self.collect_break_types(body)?;

    if break_types.is_empty() {
        // No breaks: loop is Void (non-terminating or side-effect only)
        Ok(MonoType::Void)
    } else {
        // Unify all break types
        let unified = self.unify_all(&break_types, span)?;
        Ok(unified)
    }
}

fn check_break(&mut self, value: Option<&Expr>, span: Span) -> Result<MonoType, ()> {
    match value {
        None => Ok(MonoType::Void),
        Some(expr) => self.synth_expr(expr),
    }
}
```

Lowering will then **trust** the loop type from TypeMap.

---

## 4. Try/Collect/For Desugaring (Typechecker Gaps)

### Current State
- `try`, `collect`, `for` all return `UnsupportedFeature`

### What's Needed
These are **surface syntax** that lower to Core IR, but the typechecker needs to validate them:

#### Try
```rust
fn synth_try(&mut self, expr: &Expr, span: Span) -> Result<MonoType, ()> {
    let ty = self.synth_expr(expr)?;

    // Must be Result<T, E>
    match ty {
        MonoType::Named { name, args, .. } if name == "Result" && args.len() == 2 => {
            // Return T (the Ok value type)
            Ok(args[0].clone())
        }
        _ => {
            self.errors.push(TypeError::TryRequiresResult { ty, span });
            Err(())
        }
    }
}
```

#### Collect/For
- Type check iterator expression
- Ensure it's an `Array<T>`
- Bind pattern variable with type `T`
- Type check body

---

## 5. ValueEnv Not Returned (Minor Gap)

### Current State
```rust
pub fn check_module(...) -> Result<(TypeMap, TypeEnv), Vec<TypeError>>
```

### What's Needed
Stage 3 lowering needs function signatures to build the function table. Either:

**Option A:** Return ValueEnv:
```rust
pub fn check_module(...) -> Result<(TypeMap, TypeEnv, ValueEnv), Vec<TypeError>>
```

**Option B:** Embed function signatures in TypeEnv:
- TypeEnv already has access to function metadata
- Lowering reads from TypeEnv only

**Decision:** Option B is cleaner (one source of truth).

---

## 6. Result Type Location (Clarified)

Per plan update:
- `Result<T, E>` is defined in **prelude**
- Implicitly available in all modules
- Shadowing `Result` is a **compile error**

Add to resolver:
```rust
const PRELUDE_TYPES: &[&str] = &["Result"];

fn check_type_shadowing(&mut self, name: &str, span: Span) {
    if PRELUDE_TYPES.contains(&name) {
        self.errors.push(TypeError::ShadowingPreludeType { name, span });
    }
}
```

---

## Implementation Order

### Before Phase 1 (Core Data Structures)
1. ✅ Phase 0 infrastructure complete (ExprId, TypeMap, error types)

### During Phase 1-2 (Parallel to IR Design)
2. Add method resolution to TypeEnv (has_method, get_method_func_id)
3. Implement field/method collision detection
4. Add loop type inference to typechecker
5. Add try/collect/for type checking
6. Add Result shadowing check to resolver

### Phase 3+ (Lowering Implementation)
7. Lowering reads TypeMap and trusts all type annotations
8. Method calls read FuncId from TypeMap.method_calls
9. Loops read type from TypeMap (no re-inference)

---

## Current Test Status

- ✅ `field_method_collision.tw` created (will fail once methods implemented)
- ⚠️  Test currently **passes** (p.len treated as field, not method)
- ⚠️  This is **expected** until method resolution is added

---

## Summary

Phase 0 laid the **architectural foundation** (IDs, type maps, error types) but intentionally deferred **feature implementation** to avoid conflicts with the full Stage 3 design.

The gaps above are **documented blockers** that must be addressed in Phase 1-2 before lowering can consume the TypeMap.

This is a **clean separation**:
- Phase 0 = infrastructure ✅
- Phase 1-2 = complete Stage 2 features needed by lowering ⏳
- Phase 3+ = implement lowering with all prereqs met ⏳
