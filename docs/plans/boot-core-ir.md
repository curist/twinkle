# Boot Compiler — Core IR & Lowering

Last updated: 2026-03-21

## Background

Phase A (frontend) is complete: lexer, parser, resolver, and type checker
all produce partial results with diagnostics. The checker outputs
`CheckResult { type_map: Dict<Int, MonoType>, env: ResolvedEnv, diagnostics }`.

Phase B lowers the checked AST into a Core IR suitable for monomorphization
and subsequent ANF lowering. This is the first backend phase and the bridge
between the tree-shaped AST and the flat function list that codegen consumes.

## Design Principles

From [self-hosting.md](self-hosting.md):

- **Pure pipeline**: no `&mut self`; context records are threaded
  functionally and returned as updated copies.
- **No shadow type system**: every Core IR node carries its `MonoType`
  directly; downstream passes derive layout from the type, not from
  side metadata.
- **Flat output**: `CoreModule { functions: Vector<FunctionDef> }` — all
  user functions, hoisted lambdas, and `__init__` are peers.

## Scope

In scope:
- Core IR data types (`boot/compiler/core_ir.tw`)
- AST → Core IR lowering (`boot/compiler/lower_core.tw`)
- Monomorphization pass (`boot/compiler/monomorphize.tw`)
- Tests for each milestone

Out of scope:
- ANF lowering (Phase C)
- Multi-module linking (Phase E)
- Optimization passes

## Prerequisites — Expression Identity

### Problem

The current boot checker keys `type_map` by `span.start`. This is lossy:
for a call like `foo(1)`, both the outer `Call` expression and the callee
`Ident("foo")` share the same `span.start`. The checker writes the callee's
`Function` type at `callee.span.start` (checker.tw:577), then the outer
`synth` writes the call's result type at `expr.span.start` (checker.tw:448)
— same offset, so one overwrites the other. The lowerer cannot reliably
recover callee-only information (needed for method resolution and generic
instantiation).

### Solution

Add a unique `id: Int` field to `Expr` in the boot AST, matching stage0's
`ExprId`. The parser assigns monotonically increasing IDs. The checker keys
`type_map` by `expr.id` instead of `span.start`.

This also unlocks two additional maps that the lowerer and monomorphizer
need (matching stage0's `TypeMap`):

```tw
pub type CheckResult = .{
  type_map: Dict<Int, MonoType>,                // ExprId → type
  method_calls: Dict<Int, String>,              // ExprId → resolved method name
  env: ResolvedEnv,
  diagnostics: Vector<Diagnostic>,
}
```

**Why `method_calls` stores names, not FuncIds:** The checker runs before
FuncId assignment (which happens in the lowering pre-pass). The checker
knows which method name resolved for a call site, but cannot produce a
FuncId. The lowerer maps method names to FuncIds via `func_table` at
lowering time.

**Why no `generic_instantiations`:** Stage0's monomorphizer does not
consume the `generic_instantiations` map from TypeMap. Instead, it
re-derives type substitutions from the concrete types already present on
Core IR nodes — `match_type_against(generic_param_ty, concrete_arg.ty)`
at each call site (monomorphize.rs:190-196). This works because the
lowerer stamps every `CoreExpr.ty` with the fully-resolved concrete type
from the checker's `type_map`. As long as `CoreExpr.ty` is correct, the
monomorphizer can infer all substitutions without a separate map.

**This prerequisite must be implemented before M2.** Without stable per-
expression identity, the lowerer cannot populate `CoreExpr.ty` correctly.

## Core IR Types

### Identity Types

```tw
pub type LocalId = .{ id: Int }
pub type FuncId = .{ id: Int }
pub type FieldId = .{ id: Int }
pub type VariantId = .{ id: Int }
```

Newtypes for clarity. Stage0 uses `u32` wrappers; we use records with a
single `id` field.

### `CoreExpr`

```tw
pub type CoreExpr = .{
  kind: CoreExprKind,
  ty: MonoType,
  span: Span,
}
```

Every node carries its resolved type. The lowerer populates `ty` from the
checker's `type_map` (keyed by `expr.id` — see Prerequisites above).

### `CoreExprKind`

```tw
pub type CoreExprKind = {
  // Literals
  LitInt(Int),
  LitFloat(Float),
  LitBool(Bool),
  LitStr(String),
  LitVoid,

  // Variables
  Local(LocalId),
  GlobalLocal(LocalId),
  GlobalFunc(FuncId),

  // Binding
  Let(LocalId, CoreExpr, CoreExpr),       // let local = value in body
  Assign(LocalId, CoreExpr),              // mutate local (type: Void)

  // Operators
  BinOp(BinOp, CoreExpr, CoreExpr),
  UnOp(UnOp, CoreExpr),

  // Calls & closures
  Call(CoreExpr, Vector<CoreExpr>),
  MakeClosure(FuncId, Vector<LocalId>),   // func_id + captured free vars

  // Control flow
  If(CoreExpr, CoreExpr, CoreExpr),       // cond, then, else
  Match(CoreExpr, Vector<MatchArm>),
  Loop(CoreExpr),
  Break(CoreExpr?),
  Continue,
  Return(CoreExpr?),
  Defer(CoreExpr),                        // erased in ANF defer-elim pass

  // Data
  Record(TypeId, Vector<FieldInit>),      // nominal record construction
  RecordGet(CoreExpr, FieldId),
  RecordUpdate(CoreExpr, FieldId, CoreExpr),
  Variant(TypeId, VariantId, Vector<CoreExpr>),
  ArrayLit(Vector<CoreExpr>),
  Index(CoreExpr, CoreExpr),
}

pub type FieldInit = .{ field: FieldId, value: CoreExpr }

pub type MatchArm = .{ pattern: CorePattern, body: CoreExpr }

pub type CorePattern = {
  Wildcard,
  Var(LocalId),
  LitInt(Int),
  LitBool(Bool),
  LitStr(String),
  Variant(TypeId, VariantId, Vector<CorePattern>),
}
```

### Differences from Stage0

| Stage0 | Boot | Rationale |
|--------|------|-----------|
| No `LitByte` | No `LitByte` (removed) | Twinkle has no byte literal syntax; int literals typed as `Byte` via annotation produce `LitInt` with `ty: MonoType.Byte` |
| `params: Vec<LocalId>` + `param_tys: Vec<MonoType>` (parallel arrays) | `params: Vector<Param>` with `Param = .{ local: LocalId, ty: MonoType }` | Paired data stays together |
| `Defer(Box<CoreExpr>)` | `Defer(CoreExpr)` | Same design — opaque node carried through Core IR, erased in ANF defer-elim pass |
| `LitFloat(f64)` | `LitFloat(Float)` | Twinkle `Float` is f64 |
| `TypeMap` keyed by `ExprId` | `type_map: Dict<Int, MonoType>` keyed by `expr.id` | Same semantics, different representation |

### `FunctionDef` and `CoreModule`

```tw
pub type Param = .{ local: LocalId, ty: MonoType }

pub type FunctionDef = .{
  func_id: FuncId,
  name: String,
  params: Vector<Param>,
  body: CoreExpr,
  return_ty: MonoType,
}

pub type CoreModule = .{
  functions: Vector<FunctionDef>,
  type_env: ResolvedEnv,
  init_func_id: FuncId?,
}
```

All functions (user-defined, hoisted lambdas, `__init__`) live in
`functions`. The `type_env` carries type definitions and function
signatures for downstream passes.

Note: visibility (`pub`) is tracked by the resolver, not in Core IR.

## Lowering Context

Stage0 uses a large mutable `Lowerer` struct. The boot compiler threads
an immutable context record instead:

```tw
type LowerCtx = .{
  type_map: Dict<Int, MonoType>,          // from CheckResult (expr.id → type)
  method_calls: Dict<Int, String>,        // from CheckResult (expr.id → method name)
  env: ResolvedEnv,                       // type defs, function sigs
  func_table: Dict<String, FuncId>,       // name → FuncId
  locals: Vector<Dict<String, LocalId>>,  // scoped frames
  next_local: Int,
  next_func: Int,
  hoisted: Vector<FunctionDef>,           // accumulated lambda lifts
  module_globals: Dict<String, LocalId>,
  next_global: Int,
  current_fn_return_type: MonoType?,      // for try desugaring in lambdas
  errors: Vector<Diagnostic>,
}
```

**Key fields not in the original plan:**
- `method_calls` — resolved method names from the checker. The lowerer
  maps these to FuncIds via `func_table` at lowering time (after the
  FuncId pre-scan). Stage0's lowerer calls
  `resolve_registered_method_func_id` directly; the boot compiler splits
  this into checker (name resolution) + lowerer (FuncId lookup).
- `current_fn_return_type` — saved/restored when entering lambda scopes.
  Required for `try` to know whether to desugar as Option or Result.

**Threading convention:** since closures can appear in any expression and
need to hoist functions, `lower_expr` and all recursive lowering helpers
must return `(CoreExpr, LowerCtx)` — not just `CoreExpr`. The context
carries accumulated hoisted functions, local allocations, and errors.

```tw
fn lower_expr(ctx: LowerCtx, expr: Expr) (CoreExpr, LowerCtx)
fn lower_stmts(ctx: LowerCtx, stmts: Vector<Stmt>, tail: Expr?) (CoreExpr, LowerCtx)
fn lower_block(ctx: LowerCtx, block: Block) (CoreExpr, LowerCtx)

fn alloc_local(ctx: LowerCtx, name: String) (LocalId, LowerCtx)
fn push_scope(ctx: LowerCtx) LowerCtx
fn pop_scope(ctx: LowerCtx) LowerCtx
fn hoist(ctx: LowerCtx, def: FunctionDef) LowerCtx
fn lookup_local(ctx: LowerCtx, name: String) LocalId?
fn lookup_func(ctx: LowerCtx, name: String) FuncId?
fn expr_type(ctx: LowerCtx, expr: Expr) MonoType  // type_map lookup by expr.id
fn emit_error(ctx: LowerCtx, span: Span, msg: String) LowerCtx
```

**Return type of `lower_module`:** returns `LowerResult` — a record
containing the `CoreModule` and any accumulated diagnostics:

```tw
pub type LowerResult = .{
  module: CoreModule,
  diagnostics: Vector<Diagnostic>,
}

pub fn lower_module(module: Module, check_result: CheckResult) LowerResult
```

**Pre-passes:** `lower_module` performs two pre-scans before lowering
function bodies:
1. **Globals scan:** collect module-level `let` bindings, assign `LocalId`s
   to `module_globals`.
2. **FuncId scan:** assign `FuncId`s to all user-defined functions in
   `func_table`.

## Lowering Rules (Summary)

### Literal parsing

AST literals are strings (`IntLit(String)`, `FloatLit(String)`). The
lowerer must parse them to values:
- `IntLit("42")` → `LitInt(42)` (via `Int.parse` or equivalent)
- `IntLit("0xFF")` → `LitInt(255)` (hex literals)
- `FloatLit("3.14")` → `LitFloat(3.14)`
- `BoolLit(true)` → `LitBool(true)` (already a value, no parsing)

### Statements → Let-spine (terminal-aware)

A block `{ s1; s2; ...; tail }` becomes a right-recursive `Let` chain:
```
Let(_, lower(s1), Let(_, lower(s2), ... lower(tail)))
```

**Terminal-aware rule:** when a statement lowers to a terminal expression
(`Return`, `Break`, `Continue`), emit it directly as the block's value.
Do NOT wrap it in `Let(_, terminal, <rest>)` — that would place a
diverging expression in `Let.value`, which the ANF lowerer treats as
malformed Core IR and panics on. Stage0 confirms: `Return`/`Break` are
emitted directly, and subsequent statements in the block are dropped.

Statement lowering:
- `let x = v` → `Let(x_local, lower(v), <rest>)`
- `x = v` (simple rebind) → `Let(_, Assign(x_local, lower(v)), <rest>)`
- `return e` → `Return(lower(e))` ← terminal, no `<rest>`
- `break` / `continue` → `Break` / `Continue` ← terminal, no `<rest>`
- Expression statement → `Let(_, lower(e), <rest>)`

### Complex assignment (lvalue chains)

The checker accepts field assignment (`p.x = 1`), index assignment
(`xs[i] = v`), and dict assignment (`m[k] = v`). These require recursive
lvalue desugaring (matching stage0's `lower_lvalue_chain`):

- `r.field = val` → `Assign(r_local, RecordUpdate(Local(r), field_id, lower(val)))`
- `xs[i] = val` → `Assign(xs_local, Call(VECTOR_SET_UNSAFE, [Local(xs), lower(i), lower(val)]))`
- `m[k] = val` → `Assign(m_local, Call(DICT_SET, [Local(m), lower(k), lower(val)]))`
  (`DICT_SET` returns the updated dict; must rebind, not discard)
- Nested: `a.b.c = x` → recursively build `RecordUpdate` from innermost
  to outermost

All forms use the same recursive `lower_lvalue_chain` pattern: the leaf
`Ident` case returns `(local, rhs)`, and each layer wraps the rhs in
the appropriate update call. The caller emits `Assign(local, final_rhs)`.

### For loops → Loop desugaring

The AST `ForStmt` has optional fields covering all forms. Dispatch is
by inspecting which fields are set and the iterator's type.

**`for x in xs` (Vector):**

Index increment happens BEFORE body so that `continue` in the body
advances to the next element (not infinite-looping on the same one):
```
Let(arr, lower(xs),
  Let(len, Call(vector_len, [arr]),
    Let(idx, LitInt(0),
      Loop(
        If(BinOp(Gte, Local(idx), Local(len)),
          Break(LitVoid),
          Let(x, Index(Local(arr), Local(idx)),
            Let(_, Assign(idx, BinOp(Add, Local(idx), LitInt(1))),
              Let(_, lower(body), Continue))))))))
```

**`for b in s` (String):** same pattern but uses `Call(string_len, [s])`
for length and `Index` yields `Byte`.

**`for x, i in xs` (with index binding):** same as vector form but binds
`i` to the current index value BEFORE the increment, before the body.
Order: bind elem → bind user index → increment → body → Continue.

**`for cond { body }` (condition form):** stage0 negates the condition:
```
Loop(If(UnOp(Not, lower(cond)), Break(LitVoid), Let(_, lower(body), Continue)))
```

**Range form (`for x in range(a, b)`):** dispatch on the iterator
expression's `MonoType`. Uses `RecordGet` on start/end/step fields.
Builds a loop with step-aware comparison (Gte for positive step, Lte for
negative). Same index-before-body order.

**Dict form (`for k in dict` / `for k, v in dict`):** calls `DICT_KEYS`
to get the key vector, then iterates the key vector as a normal vector
loop. The second binder `v` is the dict **value** (not an index) — looked
up via `DICT_GET_UNSAFE(dict, k)` (internal intrinsic, not the safe
`dict_get`). This differs from the vector `for x, i` form where the
second binder is the index.

**Iterator form (`for x in iter`):** calls `ITERATOR_NEXT` in a loop and
pattern-matches on `Option<IterItem<T>>`. `.Some(item)` binds `x` to the
item's value field; `.None` breaks.

### Closures → Hoist + MakeClosure

1. Allocate params, lower body in a new scope
2. Save/restore `current_fn_return_type` around the lambda scope
3. Walk the *lowered* body (not the AST) to collect free variable
   references
4. Hoist a `FunctionDef` with `params: lambda_params` only (free vars
   are NOT prepended to params — they are carried separately)
5. Emit `MakeClosure(hoisted_func_id, free_vars)`

**Free variable collection details** (matching stage0's `collect_local_refs`):
- `Let`-bound locals are added to a `bound` set before recursing into the
  body, preventing inner bindings from being captured as free vars
- `Assign { local }` marks the local as potentially free if not bound
  (captures loop variables correctly)
- `Match` arm patterns extend the bound set per arm
- `MakeClosure { free_vars }` propagates its free vars upward (nested
  closure capture)

### Records

Two AST forms:
- `NamedRecord("Point", entries)` — the type name is in the AST; resolve
  `TypeId` from `env.type_index`.
- `Record(entries)` — anonymous `.{ ... }` literal; the `TypeId` comes
  from the checker's `type_map` (the expected type was propagated during
  type checking).

Both produce `CoreExprKind::Record(type_id, fields)`.

**Type alias resolution:** if the AST or type_map refers to a type alias,
the lowerer must resolve through to the canonical record `TypeId` before
constructing `Record` or looking up field indices. Field IDs are defined
on the canonical type, not the alias.

**Field punning:** `RecordEntry.value` is `Expr?`. When `None`, the entry
`name` is shorthand for `name: name` — the lowerer treats it as
`Ident(entry.name)` and looks up the local.

### Patterns

AST `PatternKind` variants map to `CorePattern`:
- `Wildcard` → `Wildcard`
- `Ident(name)` → `Var(alloc_local(name))`
- `Literal(expr)` → dispatch on the literal expression kind:
  `IntLit` → `LitInt`, `BoolLit` → `LitBool`, `StringLit` → `LitStr`
- `Variant(name, pats)` → resolve `(TypeId, VariantId)` from scrutinee
  type, then lower sub-patterns recursively
- `QualifiedVariant(path, name, pats)` → resolve `TypeId` from the
  qualified path (e.g., `Option.Some`), then same as `Variant`
- `ErrorPattern` → `Wildcard` (best-effort recovery)

### Method calls and module-qualified calls

The checker resolves three call forms and records method names in
`method_calls`. The lowerer maps names to FuncIds via `func_table`.

1. **Receiver method call** (`x.method(args)`) — the lowerer looks up the
   method name from `method_calls` (keyed by the call expr's id), resolves
   it to a `FuncId` via `func_table`, and emits:
   ```
   Call(GlobalFunc(func_id), [lower(x), ...lower(args)])
   ```

2. **Module-qualified call** (`Alias.func(args)`) — the checker records
   the resolved function name in `method_calls`. The lowerer maps to
   FuncId and emits `Call(GlobalFunc(func_id), [...lower(args)])`. For
   Phase B (single-module), this covers prelude calls. Cross-module
   resolution with external FuncId placeholders is deferred to Phase E.

3. **Function-typed record fields** — if `x.f` resolves to a record field
   with function type (not a method), the lowerer emits
   `Call(RecordGet(lower(x), f_field_id), [...lower(args)])` instead.

**First-class method values:** `x.method` without a call — the lowerer
binds the receiver to a temp local (if not already a local), then emits:
```
Let(tmp, lower(x), MakeClosure(method_func_id, [tmp]))
```
`MakeClosure` captures `Vector<LocalId>`, so the receiver must be a
`LocalId`, not a raw `CoreExpr`.

### Try → Match + Return

`try` desugars differently based on the enclosing function's return type
(tracked in `current_fn_return_type`):

**Result form** — `try e` in a `Result`-returning function:
```
Let(tmp, lower(e),
  Match(tmp, [
    MatchArm(.Ok(v), Local(v)),
    MatchArm(.Err(e), Return(Variant(result_tid, err_vid, [Local(e)]))),
  ]))
```

**Option form** — `try e` in an `Option`-returning function:
```
Let(tmp, lower(e),
  Match(tmp, [
    MatchArm(.Some(v), Local(v)),
    MatchArm(.None, Return(Variant(option_tid, none_vid, []))),
  ]))
```

### String interpolation → concat chain

`"hello ${name}!"` becomes a left-fold concat chain. Each interpolated
expression dispatches to a type-specific `to_string` function by
matching the expression's `MonoType`:
- `Int` → `Call(FuncId(4), [expr])` (int_to_string)
- `Float` → `Call(FuncId(5), [expr])` (float_to_string)
- `Bool` → `Call(FuncId(6), [expr])` (bool_to_string)
- `Byte` → `Call(FuncId(1024), [expr])` (byte_to_string)
- `String` → use directly (no conversion)
- fallback → `Call(FuncId(7), [expr])` (string_to_string identity)

```
Call(string_concat, [
  Call(string_concat, [
    LitStr("hello "),
    Call(to_string_for_T, [Local(name)])]),
  LitStr("!")])
```

The `to_string` dispatch uses hardcoded FuncIds rather than method lookup,
which is sufficient for single-module lowering. This may need revisiting
in Phase E if user-defined `to_string` methods should be supported.

### Collect → builder pattern

`collect x in xs { body }` uses the vector builder:
```
Let(builder, Call(builder_new, []),
  Let(_, <for-loop appending to builder>,
    Call(builder_freeze, [Local(builder)])))
```

**Additional collect forms** (matching stage0):
- `collect x, i in xs { body }` — indexed collect, binds `i` to index
- `collect while cond { body }` — condition-form collect
- Dict/iterator/range collect — dispatch by iterator type

### Defer → opaque node (sequenced in Let)

`defer cleanup()` lowers to a `Let`-wrapped `Defer`, matching stage0:
```
Let(tmp, Defer(lower(cleanup())), <rest>)
```

The `Defer` node is opaque in Core IR — the ANF pass handles defer
elimination (inserting the deferred expression at all exit points).

## Monomorphization

### Algorithm

Standard BFS specialization (same as stage0):

1. Identify generic functions (any `Var` in param types or return type)
2. Seed: scan non-generic function bodies for calls to generics; derive
   type substitutions from the concrete types on Core IR nodes (see below)
3. BFS: dequeue `(orig_func_id, subst)`, check `processed` set, clone
   and substitute the body, scan for transitive instantiations
4. Rewrite: walk all non-generic function bodies. At each call site or
   `GlobalFunc` reference to a generic, re-derive the concrete type args
   for that specific site (from argument types in call position, or from
   the parent expression's type in non-call position), then look up the
   specialized FuncId via `spec_map[(orig_id, type_args)]`. One generic
   may have multiple specializations — the rewrite is per-call-site, not
   per-function.
5. Drop original generic functions

**Type arg derivation:** No separate `generic_instantiations` map is
needed. Stage0's monomorphizer derives substitutions entirely from the
concrete types already stamped on Core IR nodes:
- **Call position:** `match_type_against(generic_param_ty, arg.ty)` for
  each argument. The checker has already resolved `arg.ty` to a concrete
  type (e.g., `Int`), so matching against the generic's `Var("T")` param
  type yields `T → Int`.
- **Non-call position** (`let f = id` where `id` is generic): match the
  generic's function type against the `CoreExpr.ty` of the `GlobalFunc`
  node, which the lowerer stamps with the concrete function type from
  `type_map`.
- **Return-context cases** (e.g., `let x: Option<Int> = none()`) work
  because the checker solves type variables during checking and stamps
  the call arguments with concrete types. The monomorphizer reads these
  already-concrete types.

This approach requires that `CoreExpr.ty` is always fully concrete (no
residual `Var` types) for expressions in non-generic function bodies.

### Output consistency

After monomorphization, `CoreModule.functions` contains only concrete
(non-generic) functions. `CoreModule.type_env` is passed through
unchanged — it retains the original generic function signatures. This is
acceptable because downstream passes (ANF lowering, codegen) consume
`FunctionDef` directly and do not look up function signatures from
`type_env`. If a downstream pass needs to look up a specialized function's
signature, it should read from the `FunctionDef.params` / `return_ty`
fields, not from `type_env`.

### Improvements over Stage0

- **Paired params**: `Vector<Param>` instead of parallel `params`/`param_tys`
  arrays eliminates zip-everywhere boilerplate.

Note: the current implementation uses two separate traversals (collect
instantiations, then rewrite calls) matching stage0's approach. A
single-pass optimization is deferred — the two-pass version is simpler
and correctness is easier to verify.

### Types

```tw
type MonoKey = .{ func_id: FuncId, type_args: Vector<MonoType> }

type MonoCtx = .{
  queue: Vector<MonoKey>,
  spec_map: Dict<String, FuncId>,    // serialized (func_id, type_args) → specialized FuncId
  new_functions: Vector<FunctionDef>,
  next_func_id: Int,
}
```

A single `spec_map` serves both deduplication (has this specialization
been created?) and call rewriting (what FuncId replaces the generic?).
The key is a string serialization of `(FuncId, type_args)` since Twinkle
dicts require `String`/`Int`/`Byte` keys. The serialization must use
unambiguous delimiters that cannot appear in type names — e.g.,
`"42|Int\x00String"` using null bytes, or a length-prefixed format.
Stage0 uses `(FuncId, Vec<MonoType>)` tuples in a `HashMap` directly.

## Implementation Plan

### M0 — Expression identity (prerequisite)

Add `id: Int` to the boot AST's `Expr` type. Update the parser to assign
monotonically increasing IDs. Update the checker to key `type_map` by
`expr.id` instead of `expr.span.start`. Add `method_calls` map to
`CheckResult` (stores resolved method names, not FuncIds).

**Tests:**
- Two expressions at the same span.start get distinct IDs
- `type_map` lookup by `expr.id` returns correct type for both callee
  and call expression in `foo(1)`
- `method_calls` populated for `x.method(args)` calls

### M1 — Core IR types and scaffolding

Create `boot/compiler/core_ir.tw` with all types defined above. Create
`boot/compiler/lower_core.tw` with the `LowerCtx` type and scaffolding
for `lower_module`. Create `boot/tests/suites/core_ir_suite.tw`.

**Tests:**
- Construct a `CoreExpr` manually, verify field access
- `FunctionDef` round-trip (create and read back fields)

### M2 — Literals, identifiers, let bindings

Lower literal expressions (including parsing `IntLit(String)` →
`LitInt(Int)` and `FloatLit(String)` → `LitFloat(Float)`), identifier
lookups, and let bindings. Wire up `type_map` lookups to populate
`CoreExpr.ty`. Implement `alloc_local`, `push_scope`, `pop_scope`,
`lookup_local`.

**Tests** (parse → resolve → check → lower, inspect Core IR):
- `fn f() Int { 42 }` → `FunctionDef` with `LitInt(42)` body
- `fn f() Int { 0xFF }` → `LitInt(255)` (hex literal parsing)
- `fn f() Int { x := 5; x }` → `Let(x, LitInt(5), Local(x))`
- `fn f() String { "hello" }` → `LitStr("hello")`
- `fn f() Float { 3.14 }` → `LitFloat(3.14)`

### M3 — Binary/unary operators, if expressions

Lower arithmetic, comparison, logical, and bitwise operators. Lower `if`
expressions (desugar `&&`/`||` to `If` for short-circuit).

**Tests:**
- `1 + 2` → `BinOp(Add, LitInt(1), LitInt(2))`
- `if true { 1 } else { 2 }` → `If(LitBool(true), LitInt(1), LitInt(2))`
- `a && b` → `If(Local(a), Local(b), LitBool(false))`

### M4 — Function calls, global functions, FuncId assignment

Pre-scan module functions to assign `FuncId`s. Lower function calls
(direct calls to named functions and calls to local variables). Wire up
`func_table` lookups.

**Tests:**
- `fn add(a: Int, b: Int) Int { a + b }; fn f() Int { add(1, 2) }` →
  `Call(GlobalFunc(add_id), [LitInt(1), LitInt(2)])`
- Multiple functions get distinct FuncIds

### M5 — Records, variants, pattern matching

Lower record construction (both `NamedRecord` and anonymous `Record`
with type from `type_map`), field punning (`RecordEntry.value == None`),
field access, record update. Lower variant construction. Lower `case`
expressions to `Match` with `CorePattern`. Handle all `PatternKind`
variants: `Wildcard`, `Ident`, `Literal(Expr)`, `Variant`,
`QualifiedVariant`, `ErrorPattern`.

**Tests:**
- `Point.{ x: 1, y: 2 }` → `Record(point_tid, [...])`
- `p: Point = .{ x: 1, y: 2 }` → anonymous record resolved from type_map
- `Point.{ x }` where `x` is in scope → field punning
- `p.x` → `RecordGet(Local(p), x_field_id)`
- `case opt { .Some(x) => x, .None => 0 }` → `Match` with two arms
- `case n { 0 => "zero", _ => "other" }` → literal pattern `LitInt(0)`
- Qualified variant pattern: `Option.Some(x)` → resolves TypeId from path

### M6 — For loops, break/continue, return, assign, defer

Lower all for-loop forms: vector iteration, string iteration (yields
`Byte`), index binding (`for x,i in xs`), condition form (`for cond`),
range, dict, iterator. Lower `break`, `continue`, `return` as terminal
expressions (no Let wrapping). Lower simple rebinding (`x = expr`) to
`Assign`. Lower complex lvalue assignment (`r.field = v`, `xs[i] = v`,
`m[k] = v`) via recursive lvalue chain desugaring. Lower `defer expr` to
`Let(tmp, Defer(lower(expr)), <rest>)`.

**Tests:**
- `for x in xs { ... }` → `Loop(If(... Break ... Let(x, Index(...), ...)))`
  with index increment BEFORE body
- `for b in s { ... }` where `s: String` → element type is `Byte`
- `for x, i in xs { ... }` → `i` bound to index
- `for x > 0 { ... }` → condition-form loop with negated condition
- `return 42` → `Return(LitInt(42))` — terminal, no continuation
- `x = x + 1` → `Assign(x_local, BinOp(Add, ...))`
- `p.x = 1` → `Assign(p_local, RecordUpdate(...))`
- `xs[i] = v` → `Assign(xs_local, Call(VECTOR_SET_UNSAFE, [...]))`
- `defer cleanup()` → `Let(tmp, Defer(Call(cleanup_id, [])), <rest>)`
- Block with `return` mid-body drops subsequent statements

### M7 — Closures, try, string interpolation, arrays, collect

Lower closures with free variable capture and hoisting (free vars in
`MakeClosure`, not prepended to params). Lower `try` to Match + Return
for both Result and Option variants (using `current_fn_return_type`).
Lower string interpolation to left-fold concat chains with type-
dispatched `to_string`. Lower array literals and collect expressions
(including indexed and condition forms).

**Tests:**
- `fn(x: Int) Int { x + y }` where y is captured → `MakeClosure` with
  `free_vars: [y_local]`, hoisted func has only `[x]` as params
- Nested closure captures — inner closure's free vars propagated outward
- `try parse(input)` in Result-returning fn → `Match` with Ok/Err arms
- `try find(x)` in Option-returning fn → `Match` with Some/None arms
- `"${n}"` where `n: Int` → `Call(int_to_string, [Local(n)])`
- `[1, 2, 3]` → `ArrayLit([LitInt(1), LitInt(2), LitInt(3)])`
- `collect x in xs { x * 2 }` → builder pattern
- First-class method value: `xs.push` → `Let(tmp, Local(xs), MakeClosure(push_fid, [tmp]))`

### M8 — Module-level lets, __init__

Lower top-level `let` bindings to `GlobalLocal` references. Synthesize
`__init__` function that evaluates top-level statements.

**Tests:**
- Top-level `x := 42` → `GlobalLocal` in function bodies, `__init__`
  assigns the value
- `CoreModule.init_func_id` is set

### M9 — Monomorphization

Implement `monomorphize(module: CoreModule) CoreModule` as a separate
pass. BFS specialization with two-pass traversal (collect instantiations,
then rewrite calls). Type args derived from concrete types on Core IR
nodes (no external map needed).

**Tests:**
- `fn id<T>(x: T) T { x }; fn f() Int { id(42) }` → `id` specialized
  to `id__Int`, generic `id` dropped
- Transitive: generic calling generic → both specialized
- Non-call-position: `let f = id` with concrete type → specialized
- Return-context driven: `let x: Option<Int> = none()` → specialized
  correctly

### M10 — Integration and wiring

Register `core_ir_suite` in test main. End-to-end test: parse → resolve →
check → lower → monomorphize. Verify the full pipeline produces correct
`CoreModule` for non-trivial programs.

**Tests:**
- Multi-function program with generics, closures, and pattern matching
- Verify function count after monomorphization (generics removed,
  specializations added)

## Files

- **Modify:** `boot/compiler/ast.tw` — add `id: Int` to `Expr` (M0)
- **Modify:** `boot/compiler/parser.tw` — assign `expr.id` (M0)
- **Modify:** `boot/compiler/checker.tw` — key by `expr.id`, add method_calls (M0)
- **Create:** `boot/compiler/core_ir.tw` — Core IR types
- **Create:** `boot/compiler/lower_core.tw` — AST → Core IR lowering
- **Create:** `boot/compiler/monomorphize.tw` — monomorphization pass
- **Create:** `boot/tests/suites/core_ir_suite.tw` — tests
- **Modify:** `boot/tests/main.tw` — register suite

## Risks and Mitigations

- **Risk:** Lowering is the largest single implementation effort in the
  boot compiler (~5000 lines in stage0).
  - **Mitigation:** Milestone-driven with tests at each step. Each
    milestone is independently verifiable.
- **Risk:** Free variable collection for closures is error-prone.
  - **Mitigation:** Dedicated tests for capture scenarios (nested
    closures, shadowing, loop variables). Collect from the lowered Core
    IR body (not the AST), matching stage0's approach.
- **Risk:** Monomorphization key serialization (String-based dict keys)
  may have collisions.
  - **Mitigation:** Use a length-prefixed or null-delimited serialization
    format. Test with types that have similar names (e.g., `Int` +
    `String` vs `IntString`).
- **Risk:** Terminal expressions in Let-value position cause ANF panics.
  - **Mitigation:** `lower_stmts` checks for terminal lowered forms
    before wrapping in `Let`. Dedicated test for `return` mid-block.
- **Risk:** Complex lvalue assignment missed, regressing checker-accepted
  programs.
  - **Mitigation:** M6 explicitly covers field/index/dict assignment with
    tests. Recursive `lower_lvalue_chain` matches stage0's approach.

## Design Decisions

### Lowering is total over partially-checked AST

The lowerer runs on `CheckResult` even when diagnostics are present. It
emits `LitVoid` (or a best-effort fallback) for AST nodes whose types
are missing from `type_map`, and accumulates its own diagnostics. This
matches the boot compiler's philosophy of partial results with
diagnostics throughout the pipeline. Tests should cover both clean and
diagnostic-bearing inputs.

### Method values synthesize closure wrappers

`x.method` in non-call position produces `MakeClosure(method_func_id,
[lower(x)])` — a closure that captures the receiver and calls the method.
No new Core IR variant is needed. This matches stage0's approach.

### Prelude/intrinsic FuncIds are pre-populated

`func_table` is initialized with prelude function names mapped to their
well-known `FuncId`s (e.g., `FuncId(1)` for `print`). These functions
have no corresponding `FunctionDef` in `CoreModule.functions` — they are
provided by the runtime. User-defined function FuncIds start after the
prelude range (`USER_FUNC_START`). The lowerer must not attempt to look
up a `FunctionDef` for intrinsic FuncIds.

## Implementation Status (as of 2026-03-21)

### Completed

M0 (partial), M1, M2, M3, M4, M5 (partial), M6 (partial), M7 (partial),
M8, M9 (partial), M10 (partial).

The core pipeline works: parse → resolve → check → lower → monomorphize
for programs using literals, let-bindings, operators, if/match, function
calls, records, variants, closures, try, for-loops (vector + condition),
collect (vector + condition), defer, module-level lets, and generics.

### Gaps — features not yet implemented

These are plan items with no implementation at all.

**G1. `method_calls` map (M0, blocks M4/M7)**
`CheckResult` lacks the `method_calls: Dict<Int, String>` field.
`LowerCtx` also lacks it. Without this map, the lowerer cannot resolve
dot-call syntax (`x.method(args)`) or module-qualified calls
(`Mod.func(args)`) to `GlobalFunc` — they currently fall through to
`RecordGet`, producing wrong Core IR.

**G2. Method call lowering (M4/M7)**
Depends on G1. Three forms are missing:
- Receiver method call: `x.method(args)` → `Call(GlobalFunc(fid), [x, ...args])`
- Module-qualified call: `Mod.func(args)` → `Call(GlobalFunc(fid), [...args])`
- First-class method value: `x.method` → `Let(tmp, x, MakeClosure(fid, [tmp]))`

**G3. Complex lvalue assignment / `lower_lvalue_chain` (M6)**
Only simple rebinding (`x = expr`) is implemented. Missing:
- `r.field = val` → `Assign(r_local, RecordUpdate(...))`
- `xs[i] = val` → `Assign(xs_local, Call(VECTOR_SET_UNSAFE, [...]))`
- `m[k] = val` → `Assign(m_local, Call(DICT_SET, [...]))`
- Nested chains: `a.b.c = x`

**G4. For-loop type dispatch (M6)**
The `for x in iter` branch always uses the vector pattern (VECTOR_LEN +
Index). Missing type-dispatched forms:
- String iteration: `for b in s` → `string_len` + `Index` yielding `Byte`
- Range iteration: `for x in range(a,b)` → `RecordGet` on start/end/step
- Dict iteration: `for k in dict` → `DICT_KEYS` then vector loop; `for k, v in dict` → value via `DICT_GET_UNSAFE`
- Iterator iteration: `for x in iter` → `ITERATOR_NEXT` loop with Option match

**G5. Collect type dispatch (M7)**
Same gap as G4. Only vector and condition collect forms are implemented.
Missing: dict, iterator, and range collect.

**G6. Type alias resolution in records (M5)**
`lower_named_record` resolves `TypeId` from the name but does not follow
type aliases to the canonical record TypeId. If a user writes
`type Pt = Point` and constructs `Pt.{ x: 1, y: 2 }`, the lowerer uses
the alias TypeId, not Point's — field lookups will fail.

### Discrepancies — implemented but diverging from plan

**D1. Terminal-aware expr-stmts (RISK: ANF panic)**
`lower_stmts` correctly handles `Return`/`Break`/`Continue` as terminal
*statements* (emitting them directly, dropping rest). However, the
`.Expr(es)` case always wraps in `Let(_, lower(e), rest)` even when the
lowered sub-expression is itself terminal (e.g., a block ending with
`return`). The plan warns this produces malformed Core IR that the ANF
lowerer panics on. Fix: after lowering an expression statement, check
whether the result is a terminal form; if so, emit it directly without
`Let` wrapping.

**D2. QualifiedVariant pattern ignores qualifier (RISK: wrong TypeId)**
`lower_pattern` handles `QualifiedVariant(path, name, pats)` but ignores
the qualifier path — it uses the scrutinee type for TypeId resolution,
same as unqualified `Variant`. This breaks when the qualifier is the
disambiguator (e.g., `Foo.Ok` vs `Result.Ok` on a scrutinee whose type
doesn't uniquely determine the variant). Fix: resolve `TypeId` from the
qualified path via `resolve_type_id(env, path)`.

**D3. Monomorphization `match_type_against` ignores TypeId (FIXED)**
The `Named` case in `match_type_against` was matching any two `Named`
types regardless of their TypeId, causing incorrect substitutions when
different generic types share argument-list lengths. Fixed 2026-03-21:
added `tid_p.id != tid_c.id` guard.

**D4. `for x,i` / `collect x,i` index variable not bound (FIXED)**
`lower_for` ignored `ForStmt.index` entirely; `lower_collect` allocated
the local but never assigned the counter value. Fixed 2026-03-21: both
now bind the user index local to the loop counter before the body.

**D5. Field punning type always `Void` (FIXED)**
Field punning created a synthetic `Expr` with `id: -1` for type lookup;
since no AST node has that id, the type was always `Void`. Fixed
2026-03-21: uses `find_field_type(env, tid, name)` instead.

**D6. `Byte` missing from string interpolation (FIXED)**
The `Byte` case in `lower_string_interp` fell through to
`string_to_string` (FuncId 7) instead of `byte_to_string` (FuncId 1024).
Fixed 2026-03-21.

**D7. `LitByte(Byte)` in CoreExprKind unreachable (FIXED)**
Twinkle has no byte literal syntax — int literals are typed as `Byte`
via annotation and produce `LitInt` with `ty: MonoType.Byte`. Removed
`LitByte` from `CoreExprKind` 2026-03-21. Plan updated to match.

### Fix priority

Ordered by risk (ANF panics, wrong codegen) then breadth of impact:

1. **D1** — terminal expr-stmt wrapping (ANF panic risk)
2. **D2** — QualifiedVariant qualifier (wrong TypeId)
3. **G1 + G2** — method_calls + method call lowering (blocks all dot-call programs)
4. **G3** — complex lvalue assignment (blocks field/index/dict mutation)
5. **G4** — for-loop type dispatch (blocks string/range/dict/iterator loops)
6. **G5** — collect type dispatch (same iterator types as G4)
7. **G6** — type alias resolution (edge case, low frequency)
