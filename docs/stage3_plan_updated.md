# Stage 3: Core IR Design & Lowering - Updated Plan

## Status: Phase 0 Complete ✅

**Last Updated:** 2024-12-13
**Completion:** Phase 0 infrastructure complete, ready for Phase 1

---

## Overview

Implement Core IR (Intermediate Representation) that captures Twinkle semantics with a minimal set of constructs, and lower typed AST into Core IR. This establishes the canonical semantic representation of Twinkle programs.

## Key Design Decisions (Finalized)

### 1. No InherentMethod IR Node ✅
- Inherent method calls lower to ordinary `Call` nodes
- `Call { callee: GlobalFunc(method_func_id), args: [receiver, ...] }`
- No string-based dispatch in IR
- Methods resolved to `FuncId` before/during lowering

### 2. Field-First Resolution Rule ✅
1. If `x.name` has a field named `name` → field access
2. Otherwise, if inherent method named `name` exists → method call
3. If both exist → **compile error** (FieldMethodCollision)

### 3. Immutable Arrays ✅
- `Array.append(arr, x) -> Array<T>` returns new array
- Collect loops use rebinding: `acc = acc.append(val)`

### 4. String Interpolation (Right-Nested) ✅
```tw
"a${x}b${y}c"
→ string::concat("a",
    string::concat(int::to_string(x),
      string::concat("b",
        string::concat(int::to_string(y), "c"))))
```

### 5. FuncId Numbering (Flat, Deterministic) ✅
```
FuncId(0) = __main (if exists)
FuncId(1..N) = prelude/core functions (fixed order)
FuncId(N+1..) = user functions (source order)
```

### 6. Result Type in Prelude ✅
- `type Result<T, E> = { Ok(T), Err(E) }`
- Implicitly available in all modules
- Shadowing Result is compile error
- Variant IDs: Ok=0, Err=1

---

## Phase 0: Type Annotation Infrastructure ✅ COMPLETE

**Duration:** 2 days
**Status:** All items complete, all tests passing

### Completed Items ✅

1. **ExprId in AST**
   - Added `ExprId(u32)` to uniquely identify expressions
   - Parser allocates IDs via `alloc_expr_id()`
   - All Expr::new call sites updated (20+ locations)
   - Snapshots updated

2. **TypeMap Structure**
   - Created `src/types/type_map.rs`
   - Fields: `expr_types: HashMap<ExprId, MonoType>`
   - Fields: `method_calls: HashMap<ExprId, FuncId>` (infrastructure)
   - API: get/set for types and method calls

3. **TypeChecker Integration**
   - Added `type_map: TypeMap` field to TypeChecker
   - `synth_expr` records all inferred types
   - `check_expr` records validated types
   - Return type: `Result<(TypeMap, TypeEnv), Vec<TypeError>>`

4. **Method Infrastructure**
   - TypeEnv has `methods` HashMap
   - Helpers: `has_method()`, `has_field()`, `add_method()`
   - Collision detection in `synth_field_access`
   - `TypeError::FieldMethodCollision` added

5. **Core IR Cleanup**
   - Removed `InherentMethod` variant
   - Added documentation comments

### Deferred to Early Stage 3

Items documented in `docs/phase0_completion_status.md`:

1. **Method Registration** - Needs FuncId allocation design
2. **Loop Typing** - Critical, first task in Phase 1
3. **Try/Collect/For Type Checking** - Add as needed
4. **ValueEnv Return** - Design decision pending
5. **Result Shadowing Check** - Low priority

---

## Phase 1: Core Data Structures (Days 3-4) ⏳ NEXT

**Status:** Not started
**Prerequisites:** Phase 0 complete ✅

### Tasks

1. **Create Core IR Types** (`src/ir/core.rs`)
   - Define `CoreExpr`, `CoreExprKind` enums
   - Define `CorePattern` enum
   - Define `FunctionDef`, `CoreModule` structs
   - Define ID types: `LocalId`, `FuncId`, `FieldId`, `VariantId`

2. **Create LocalAllocator** (`src/ir/local_allocator.rs`)
   - Per-function instance (reset for each function)
   - Methods: `alloc()`, `bind()`, `lookup()`, `push_scope()`, `pop_scope()`
   - Handle shadowing correctly

3. **Create LowerError** (`src/ir/error.rs`)
   - Define error variants for lowering failures
   - Include span information for reporting

4. **Update Module Exports** (`src/ir/mod.rs`)
   - Re-export core types
   - Ensure IR module compiles

5. **Add Unit Tests**
   - LocalAllocator scope tests
   - ID allocation tests

**Milestone:** IR types compile, LocalAllocator tested

---

## Phase 2: Simple Lowering (Days 5-6) ⏳ PENDING

**Prerequisites:** Phase 1 complete, Loop typing implemented

### Tasks

1. **Implement Loop Typing** (CRITICAL)
   - Add to TypeChecker before lowering starts
   - Algorithm: collect break types, unify
   - Document in check.rs

2. **Create Lowering Infrastructure**
   - `src/ir/lower.rs` - Main orchestration
   - `src/ir/lower_expr.rs` - Expression lowering
   - Lowerer struct with TypeMap, TypeEnv, LocalAllocator

3. **Implement Simple Expressions**
   - Literals: Int, Float, Bool, String, Void
   - Variables: Local(LocalId), GlobalFunc(FuncId)
   - Binary/Unary operations (reuse ast::BinOp, ast::UnOp)
   - Function calls
   - If expressions

4. **Implement Simple Patterns**
   - Wildcard, Var, Literal patterns
   - Pattern lowering with LocalAllocator

5. **Add Tests**
   - `tests/lower_core/literals.tw`
   - `tests/lower_core/bindings.tw`

**Milestone:** Simple expressions lower correctly

---

## Phase 3: Block Desugaring (Day 7) ⏳ PENDING

### Tasks

1. **Statement Sequences → Let Chains**
   ```
   { a := 1; b := 2; a + b }
   →
   Let(a, 1, Let(b, 2, BinOp(Add, Local(a), Local(b))))
   ```

2. **No Implicit Returns**
   - Final expression is the body value
   - Return nodes only for early returns

3. **Tests**
   - `tests/lower_core/blocks.tw`

**Milestone:** No Block nodes in IR, all nested Lets

---

## Phase 4: Data Structures (Day 8) ⏳ PENDING

### Tasks

1. **Record Literals + Fields**
   - Resolve type_id from TypeMap
   - Get field_id from TypeEnv
   - Lower: `Record { type_id, fields: Vec<(FieldId, CoreExpr)> }`

2. **Variant Literals**
   - Resolve type_id, variant_id via TypeMap/TypeEnv
   - Lower: `Variant { type_id, variant, args }`

3. **Array Literals + Indexing**
   - Lower: `ArrayLit { elements }`
   - Lower: `Index { base, index }`

4. **Variant Patterns**
   - Pattern destructuring with LocalAllocator

5. **Tests**
   - `tests/lower_core/records.tw`
   - `tests/lower_core/variants.tw`
   - `tests/lower_core/arrays.tw`

**Milestone:** Data structures lower with resolved IDs

---

## Phase 5: Control Flow (Day 9) ⏳ PENDING

### Tasks

1. **Match/Case**
   - Pattern matching with arms
   - Exhaustiveness already checked by typechecker

2. **Loop/Break/Continue**
   - Loop type from TypeMap (typechecker inferred)
   - Break with optional value
   - Continue (no value)

3. **Return**
   - Only for early returns
   - Not for final function value

4. **Tests**
   - `tests/lower_core/control_flow.tw`

**Milestone:** Pattern matching and loops work

---

## Phase 6: Desugarings - Try (Day 10) ⏳ PENDING

### Tasks

1. **Try → Match over Result**
   ```rust
   y := try foo()

   →

   Match scrutinee=Call(foo, []) {
     PatVariant(Result, Ok, [PatVar(v)]) =>
       Let(y, Local(v), <continuation>),
     PatVariant(Result, Err, [PatVar(e)]) =>
       Return(Some(Variant(Result, Err, [Local(e)])))
   }
   ```

2. **Result Type Lookup**
   - `type_env.lookup_type("Result")`
   - Error if not found: `LowerError::MissingResultType`

3. **Tests**
   - `tests/lower_core/try_desugar.tw`

**Milestone:** Try lowers to Match over Result

---

## Phase 7: Desugarings - Collect (Day 11) ⏳ PENDING

### Tasks

1. **Collect → Index-Based Loop**
   ```rust
   collect x in arr { x * 2 }

   →

   Let(arr_local, <arr>,
     Let(acc, ArrayLit([]),
       Let(idx, LitInt(0),
         Let(len, Call(array::len, [Local(arr_local)]),
           Loop {
             If {
               cond: BinOp(Gte, Local(idx), Local(len)),
               then: Break(Some(Local(acc))),
               else: Let(elem, Index(arr_local, idx),
                       Let(val, <body>,
                         Let(acc2, Call(array::append, [acc, val]),
                           Let(acc, acc2,
                             Let(idx2, BinOp(Add, idx, 1),
                               Let(idx, idx2, Continue))))))
             }
           }))))
   ```

2. **Immutable Append**
   - `Array.append` returns new array
   - Accumulator rebinding required

3. **Tests**
   - `tests/lower_core/collect_desugar.tw`

**Milestone:** Collect lowers to Loop + accumulator

---

## Phase 8: Desugarings - For Loops (Day 12) ⏳ PENDING

### Tasks

1. **For Cond**
   ```rust
   for cond { body }
   →
   Loop { If { cond: Not(cond), then: Break(None), else: body } }
   ```

2. **For Array Iteration**
   ```rust
   for x in arr { body }
   →
   // Index-based loop similar to collect, but no accumulator
   ```

3. **Tests**
   - `tests/lower_core/for_loops.tw`

**Milestone:** For loops lower to Loops

---

## Phase 9: String Interpolation (Day 13) ⏳ PENDING

### Tasks

1. **Interpolation → concat + to_string**
   - Right-nested structure
   - Type-specific to_string calls
   - Supported types: Int, Float, Bool, String

2. **Tests**
   - `tests/lower_core/string_interpolation.tw`

**Milestone:** Interpolation lowers correctly

---

## Phase 10: Lambda Support (Day 14) ⏳ PENDING

### Tasks

1. **Non-Capturing Lambdas**
   - Detect captures with `detect_captures()`
   - Algorithm documented in plan

2. **Reject Capturing Lambdas**
   - Error: `LowerError::UnsupportedFeature { feature: "lambda captures" }`

3. **Tests**
   - `tests/lower_core/lambda_simple.tw`
   - `tests/lower_core/lambda_capture_unsupported.tw`

**Milestone:** Lambdas lower, captures error

---

## Phase 11: CLI Integration (Day 15) ⏳ PENDING

### Tasks

1. **Create `twk lower` Command**
   - `src/cli/lower.rs`
   - Formats: `debug`, `json`, `pretty`
   - Default: debug to stdout

2. **Update Main**
   - Add command to CLI

3. **Tests**
   - Manual testing of CLI

**Milestone:** CLI works

---

## Phase 12: Pretty Printer (Day 16) ⏳ PENDING

### Tasks

1. **Implement Display**
   - `src/ir/display.rs`
   - Readable Core IR output

2. **Tests**
   - Visual inspection

**Milestone:** Readable IR output

---

## Phase 13: Edge Cases & Refinement (Day 17) ⏳ PENDING

### Tasks

1. **Add Edge Case Tests**
   - `tests/lower_core/edge_cases.tw`

2. **Run Full Test Suite**
   - `cargo test`
   - All snapshots stable

3. **Documentation**
   - Final review of all docs

**Milestone:** End-to-end stable snapshots

---

## Success Criteria

### Phase 0 ✅
- [x] ExprId in AST
- [x] TypeMap structure created
- [x] TypeChecker populates TypeMap
- [x] TypeError variants added
- [x] Method infrastructure ready
- [x] InherentMethod removed from IR
- [x] All tests passing

### Phase 1-13 ⏳
- [ ] Core IR types defined
- [ ] Loop typing in typechecker
- [ ] All desugarings implemented
- [ ] Method calls resolve to FuncId
- [ ] All tests pass with deterministic snapshots
- [ ] CLI command works
- [ ] Ready for Stage 4 (Interpreter)

---

## Critical Files

### Existing ✅
- `src/syntax/ast.rs` - AST with ExprId
- `src/syntax/parser.rs` - Allocates ExprIds
- `src/types/check.rs` - Populates TypeMap
- `src/types/type_map.rs` - Type storage
- `src/types/env.rs` - Method infrastructure
- `src/types/error.rs` - FieldMethodCollision error

### To Create ⏳
- `src/ir/core.rs` - Core IR data structures
- `src/ir/local_allocator.rs` - LocalId allocation
- `src/ir/lower.rs` - Main lowering orchestration
- `src/ir/lower_expr.rs` - Expression lowering
- `src/ir/lower_pattern.rs` - Pattern lowering
- `src/ir/lower_desugar.rs` - Desugaring transformations
- `src/ir/error.rs` - Lowering errors
- `src/ir/display.rs` - Pretty printing
- `src/cli/lower.rs` - CLI command
- `tests/lower_test.rs` - Integration tests
- `tests/lower_core/*.tw` - Test cases

---

## Built-in Functions & Methods

### Prelude Functions (GlobalFunc)
- `print(String) -> Void` - FuncId(1)
- `println(String) -> Void` - FuncId(2)
- `error(String) -> Never` - FuncId(3)

### Inherent Methods (Lower to Call with GlobalFunc)

**Int** (module: int)
- `to_string() -> String` - FuncId(4)

**Float** (module: float)
- `to_string() -> String` - FuncId(5)

**Bool** (module: bool)
- `to_string() -> String` - FuncId(6)

**String** (module: string)
- `len() -> Int` - FuncId(7)
- `concat(String) -> String` - FuncId(8)
- `to_string() -> String` - FuncId(9) (identity)

**Array<T>** (module: array)
- `len() -> Int` - FuncId(10)
- `append(T) -> Array<T>` - FuncId(11) (immutable)

---

## Dependencies

### Stage 2 → Stage 3
- ✅ ExprId in AST
- ✅ TypeMap populated
- ⏳ Loop typing (first task Phase 2)
- ⏳ Method FuncId resolution
- ⏳ Try/Collect/For validation

### Stage 3 → Stage 4
- Complete Core IR representation
- All desugarings working
- Deterministic snapshots
- Lowering tested

---

## Notes

1. **Method FuncId Allocation**
   - Design decision needed early in Phase 1
   - Options: during resolution, type checking, or dedicated pass
   - Infrastructure ready in TypeEnv

2. **Loop Typing Critical Path**
   - Must be implemented before Phase 2 lowering
   - Algorithm documented in `docs/stage2_3_integration.md`
   - Belongs in TypeChecker, not Lowerer

3. **Context Management**
   - Phase 0 used 123k/200k tokens (61%)
   - Recommend fresh sessions for major phases
   - Keep implementations focused

4. **Test Strategy**
   - Snapshot testing with insta
   - JSON output for determinism
   - One .tw file per feature

---

## Timeline Summary

- **Phase 0:** 2 days ✅ COMPLETE
- **Phase 1-2:** 4 days ⏳ (includes loop typing)
- **Phase 3-8:** 6 days ⏳
- **Phase 9-13:** 5 days ⏳
- **Total:** ~17 days

**Current Status:** Phase 0 complete, ready to start Phase 1

---

*Last updated: 2024-12-13*
*Status: Phase 0 complete, all tests passing, ready for Phase 1*
