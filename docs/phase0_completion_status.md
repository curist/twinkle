# Phase 0 Completion Status

## ✅ Completed Infrastructure

### 1. ExprId System
- ✅ `ExprId(u32)` type added to AST
- ✅ Parser allocates sequential IDs via `alloc_expr_id()`
- ✅ All 20+ `Expr::new` call sites updated
- ✅ Snapshots updated

### 2. TypeMap Structure
- ✅ Created `src/types/type_map.rs`
- ✅ Fields: `expr_types`, `method_calls` (infrastructure ready)
- ✅ API: `set_expr_type()`, `get_expr_type()`, `set_method_call()`, `get_method_call()`

### 3. TypeChecker Integration
- ✅ TypeChecker has `type_map: TypeMap` field
- ✅ `synth_expr` populates `expr_types` for all expressions
- ✅ `check_expr` populates `expr_types` for checked expressions
- ✅ Signature updated: `check_module() -> Result<(TypeMap, TypeEnv), Vec<TypeError>>`
- ✅ All call sites updated

### 4. Method Resolution Infrastructure
- ✅ TypeEnv has `methods: HashMap<(TypeId, String), String>` field
- ✅ Helper methods: `add_method()`, `has_method()`, `get_method_function()`, `has_field()`
- ✅ Collision detection in `synth_field_access`
- ✅ `TypeError::FieldMethodCollision` variant added

### 5. Core IR Cleanup
- ✅ Removed `InherentMethod` variant (contradicted plan)
- ✅ Added comment explaining methods lower to `Call` nodes

## ⏳ Deferred to Early Stage 3

### 1. Method Registration
**Why deferred:** Requires FuncId allocation design
- Need to decide when/how FuncIds are assigned
- Options: during resolution, during type checking, or dedicated pass
- TypeEnv has infrastructure ready (`add_method()`)

**Current state:** Methods registry exists but is never populated

### 2. TypeMap.method_calls Population
**Why deferred:** Depends on method registration above
- Infrastructure exists in TypeMap
- Collision detection checks for methods
- Actual FuncId recording needs allocation strategy

### 3. Loop Typing
**Why deferred:** Requires AST walking for break collection
- Algorithm specified in docs/stage2_3_integration.md
- Critical for Stage 3 lowering
- Should be implemented early in Phase 1

### 4. Try/Collect/For Type Checking
**Why deferred:** Not critical for initial lowering
- Currently return `UnsupportedFeature` errors
- Type checking logic documented
- Can be added as lowering implements these constructs

### 5. ValueEnv Return
**Why deferred:** Need to clarify Stage 3 requirements
- Current: `check_module() -> (TypeMap, TypeEnv)`
- Proposed: `-> (TypeMap, TypeEnv, ValueEnv)`
- OR: Embed function info in TypeEnv
- Decision needed before lowering starts

### 6. Result Prelude Shadowing
**Why deferred:** Low priority, simple to add later
- One-line check in resolver
- Not blocking any implementation

## Test Status

✅ **All existing tests passing**
- 33 unit tests
- 5 integration tests
- 2 typecheck tests

⚠️ **Known test issue:**
- `field_method_collision.tw` exists but will pass incorrectly
- Reason: Methods not registered yet
- Will fail correctly once method registration implemented

## Summary

**Phase 0 achieved its goal:** Provide architectural foundation for type-annotated IR lowering.

**What works:**
- Every expression has a unique ID
- Every expression's type is recorded in TypeMap
- TypeChecker returns type information for lowering
- Collision detection infrastructure ready

**What's left:**
- Populating method registry (needs FuncId design)
- Loop typing (critical, should be first task in Phase 1)
- Control flow type checking (try/collect/for)
- Minor cleanups (ValueEnv, Result shadowing)

**Recommendation:** Proceed to Stage 3 Phase 1 (Core Data Structures). Implement loop typing and method registration in parallel with IR design.

## Context Usage Note

Phase 0 consumed ~121k/200k tokens (60%). Remaining work should be done incrementally to manage context efficiently.
