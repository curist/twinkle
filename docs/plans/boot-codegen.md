# Boot Compiler — Codegen & Linker (Phase D)

Last updated: 2026-03-23

## Background

Phases A–C are complete: the self-hosted compiler has a full frontend (lexer,
parser, resolver, type checker), Core IR lowering with monomorphization, ANF
lowering, and a full optimization pipeline. Phase C produces an optimized
`AnfModule` with concrete types on every node — no type variables, no defers,
in-place flags set.

**Precondition:** The `AnfModule` entering Phase D must have all `ADefer`
nodes eliminated by the defer elimination pass (Phase C M11). If the emitter
encounters an `ADefer` node, it must trap with an error — this indicates the
optimization pipeline was not run.

Phase D transforms this ANF into a WAT (WebAssembly Text) module. This is the
most complex phase. The design is informed by **17 bug-fix commits** in stage0's
codegen, which cluster into 7 root-cause categories (see Appendix A). The
self-hosted codegen is structured to eliminate these categories by construction.

## Design Principles

From [self-hosting.md](self-hosting.md) — these are non-negotiable:

1. **Representation decided once, early** — `layout_of(ty)` computed before
   emission. No ad-hoc discovery.
2. **No shadow type system** — layout derived from `MonoType` via pure
   function. No `SumRepr` metadata, no `LocalBackendInfo`.
3. **Explicit boundary IR nodes** — `WrapAnyref` / `UnwrapAnyref` inserted
   as a dedicated pass, not implicit during emission.
4. **Layout registry, not on-demand generation** — `plan_wasm_types` builds
   the complete type set before emission starts.
5. **No flow-sensitive backend metadata** — no push/restore of per-local
   repr across branches.
6. **One metadata channel, not six** — a local's backend info is fully
   determined by its type + layout registry.

## Stage0 Bug Categories & How Phase D Avoids Them

| # | Category (stage0 commits) | Root cause | Phase D mitigation |
|---|---|---|---|
| 1 | Typed/erased sum mismatch (6) | Shadow metadata drifts from physical type | Rule 2: no shadow metadata; layout from MonoType |
| 2 | Never/divergence handling (4) | Never-typed values treated as producing stack values | M2: explicit `Never → unreachable` in every MonoType→ValType path |
| 3 | i64→i32 wraparound (3) | Bounds check after lossy truncation | M6: `emit_checked_i32_narrow` helper; all index paths use it |
| 4 | Stale flow metadata (3) | Per-local repr not propagated into nested scopes | Rules 5–6: no flow metadata; layout from type |
| 5 | Incorrect struct field index (2) | Hard-coded offsets diverge from layout | Rule 4: offsets from `WasmTypeRegistry`, never hard-coded |
| 6 | Wasm structural requirements (2) | Block result types, float literal casing | M9: post-emit WAT validation in tests |
| 7 | Pattern match short-circuit (1) | Flat AND of conditions; inner cast before outer check | M5: short-circuit emission for patterns |

## Pipeline

```
AnfModule (from Phase C)
    │
    ▼
plan_wasm_types(anf, type_env, builtins) → WasmTypeRegistry
    │  - Scans all functions for concrete types used
    │  - Computes WasmLayout for each MonoType
    │  - Registers struct defs for records, sums, closures, iterators
    │  - Collects string literal pool
    │
    ▼
insert_boundaries(anf, registry) → AnfModule'
    │  - Inserts WrapAnyref/UnwrapAnyref at runtime call boundaries
    │  - Makes all type coercions explicit and auditable
    │  - Pure ANF→ANF rewrite (no Wasm IR yet)
    │
    ▼
emit_module(anf', registry, builtins) → WasmModule
    │  - Emits type definitions from registry
    │  - Emits function bodies (ANF→Instr)
    │  - Emits trampolines, globals, init function
    │  - Emitter is stateless w.r.t. representations
    │
    ▼
link(user_module, runtime_modules) → LinkedModule
    │  - Namespace-qualifies symbols
    │  - Resolves imports to runtime exports
    │
    ▼
emit_wat(linked) → String
    - Serializes to WAT S-expressions
```

## Input Types

Phase D consumes these existing types:

| Type | Source | Purpose |
|------|--------|---------|
| `AnfModule` | `anf.tw` | Optimized flat IR |
| `AnfFunctionDef` | `anf.tw` | Per-function body + params + return type |
| `AnfExpr`, `AnfOp`, `Atom` | `anf.tw` | IR nodes to translate |
| `MonoType` | `resolver.tw` | Concrete types for layout computation |
| `ResolvedEnv` | `resolver.tw` | Type definitions (records, sums, aliases) |
| `BuiltinRegistry` | `builtins.tw` | FuncId → runtime dispatch info |
| `LocalId`, `FuncId`, `TypeId`, etc. | `core_ir.tw` | Identity types |
| `CorePattern` | `core_ir.tw` | Match arm patterns |

---

## Milestone Plan

### M1: Wasm IR Types (`boot/compiler/wasm_ir.tw`)

Define the Wasm IR data structures — the output of codegen, input to WAT
emission. Mirrors stage0's `src/wasm/ir.rs`.

**Types:**

```tw
pub type ValType = {
  I8,                      // packed storage type (array elements only)
  I32, I64, F64,
  Anyref, I31ref, Funcref,
  Ref(Bool, HeapType),     // (nullable, heap_type)
}

pub type HeapType = {
  Named(String),   // user struct/array type symbol
  Any, Eq, I31, Func, None_, Extern,
}

pub type FieldDef = .{
  name: String?,
  mutable: Bool,
  ty: ValType,
}

pub type TypeDef = {
  Struct(String, Vector<FieldDef>, String?, Bool),  // name, fields, supertype?, non_final
  Array(String, FieldDef),                           // name, element
  FuncType(String, Vector<ValType>, Vector<ValType>), // name, params, results
}

pub type Instr = {
  // Locals
  LocalGet(Int),
  LocalSet(Int),
  LocalTee(Int),
  GlobalGet(String),
  GlobalSet(String),

  // Constants
  I32Const(Int),
  I64Const(Int),
  F64Const(Float),

  // Arithmetic (i32, i64, f64 variants)
  I32Add, I32Sub, I32Mul, I32DivS, I32RemS,
  I64Add, I64Sub, I64Mul, I64DivS, I64RemS,
  F64Add, F64Sub, F64Mul, F64Div,
  I32Eqz, I64Eqz,
  I32Eq, I32Ne, I32LtS, I32GtS, I32LeS, I32GeS,
  I32LtU, I32GtU, I32LeU, I32GeU,
  I64Eq, I64Ne, I64LtS, I64GtS, I64LeS, I64GeS,
  F64Eq, F64Ne, F64Lt, F64Gt, F64Le, F64Ge,
  I32And, I32Or,
  I64And, I64Or, I64Xor, I64Shl, I64ShrS, I64ShrU, I32ShrU,
  F64Neg, F64Abs, F64Ceil, F64Floor, F64Sqrt,
  Select,

  // Conversions
  I32WrapI64, I64ExtendI32S, I64ExtendI32U, F64ConvertI64S, I64TruncF64S,
  F64ReinterpretI32,

  // References
  RefNull(HeapType),
  RefIsNull,
  RefAsNonNull,
  RefEq,
  RefI31,
  I31GetS,
  I31GetU,
  RefCast(Bool, HeapType),    // (nullable, heap)
  RefTest(Bool, HeapType),
  RefFunc(String),

  // Struct/Array
  StructNew(String),
  StructGet(String, Int),     // (type_sym, field_index)
  StructGetS(String, Int),    // signed packed field read
  StructSet(String, Int),
  ArrayNew(String),
  ArrayNewDefault(String),
  ArrayNewFixed(String, Int),
  ArrayNewData(String, String), // (type_sym, data_segment)
  ArrayGet(String),
  ArrayGetU(String),
  ArraySet(String),
  ArrayLen,
  ArrayCopy(String, String),

  // Calls
  Call(String),
  CallRef(String),
  CallIndirect(String, Int),  // (type_sym, table_index)
  ReturnCall(String),
  ReturnCallRef(String),

  // Control flow
  Drop,
  Return,
  Unreachable,
  Nop,
  If(ValType?, Vector<Instr>, Vector<Instr>),  // result?, then, else
  Block(String, ValType?, Vector<Instr>),       // label, result?, body
  Loop(String, ValType?, Vector<Instr>),
  Br(String),
  BrIf(String),
  BrTable(Vector<String>, String),              // targets, default
}

pub type FuncDef = .{
  name: String,
  params: Vector<ValType>,
  results: Vector<ValType>,
  locals: Vector<ValType>,
  body: Vector<Instr>,
}

pub type ImportDef = .{
  module: String,
  name: String,
  as_sym: String,
  params: Vector<ValType>,
  results: Vector<ValType>,
}

pub type GlobalDef = .{
  name: String,
  mutable: Bool,
  ty: ValType,
  init: Vector<Instr>,
}

pub type ExportDef = .{
  wasm_name: String,
  func_sym: String,
}

pub type TableDef = .{
  name: String,
  elem_type: ValType,
  min: Int,
  max: Int?,
}

pub type ElemDef = .{
  table: String,
  offset: Vector<Instr>,       // constant expression (e.g., [I32Const(0)])
  func_syms: Vector<String>,
}

pub type DataSegment = .{
  name: String,
  offset: Vector<Instr>,       // constant expression
  bytes: Vector<Byte>,
}

pub type WasmModule = .{
  namespace: String,
  types: Vector<TypeDef>,
  imports: Vector<ImportDef>,
  funcs: Vector<FuncDef>,
  globals: Vector<GlobalDef>,
  tables: Vector<TableDef>,
  elems: Vector<ElemDef>,
  exports: Vector<ExportDef>,
  data: Vector<DataSegment>,
  start: String?,
}
```

**Validation:** types compile, can construct and pattern-match each variant.

---

### M2: Wasm Layout Types & `layout_of` (`boot/compiler/wasm_layout.tw`)

The central representation decision. Every concrete MonoType maps to exactly
one `WasmLayout`. This is the single source of truth — no shadow metadata.

**Types:**

```tw
pub type WasmValType = { WI32, WI64, WF64, WAnyref }

pub type WasmLayout = {
  Scalar(WasmValType),
  Record(WasmRecordLayout),
  Sum(WasmSumLayout),
  Closure(WasmClosureLayout),
  Vector_(WasmLayout),
  Dict_(WasmLayout, WasmLayout),
  Iterator_(WasmIteratorLayout),
  Cell_(WasmLayout),
}

pub type WasmRecordLayout = .{
  type_id: TypeId,
  sym: String,                    // Wasm struct type symbol
  fields: Vector<WasmFieldLayout>,
}

pub type WasmFieldLayout = .{
  field_id: FieldId,
  name: String,
  val_type: ValType,              // Wasm-level field type
}

pub type WasmSumLayout = .{
  type_id: TypeId,
  sym: String,
  tag_type: ValType,              // i32 for variant tag
  variants: Vector<WasmVariantLayout>,
}

pub type WasmVariantLayout = .{
  variant_id: VariantId,
  name: String,
  payload_types: Vector<ValType>,
  struct_field_offset: Int,       // offset in the sum struct after tag
}

pub type WasmClosureLayout = .{
  closure_sym: String,            // closure struct type symbol (subtype of $Closure)
  universal_func_type_sym: String, // universal funcref type (field 0, inherited from $Closure)
  env_sym: String,                // $ClosureEnv array type (field 1, inherited from $Closure)
  typed_func_type_sym: String,    // typed funcref type (field 2, new in subtype)
  param_types: Vector<ValType>,   // concrete parameter types
  result_type: ValType?,          // concrete result type
}
// Layout: closure struct is a subtype of the universal $Closure:
//   field 0 = universal funcref  (ref null $ClosureFunc) — inherited
//   field 1 = env array          (ref null $ClosureEnv)  — inherited, captures packed here
//   field 2 = typed funcref      (ref null $typed_func)  — typed dispatch
// This 3-field subtype layout ensures universal dispatch compatibility:
// any typed closure can be upcast to $Closure and dispatched through
// the universal trampoline path.

pub type WasmIteratorLayout = .{
  state_sym: String,
  step_sym: String,
  yield_layout: WasmLayout,
}
```

**Core function:**

```tw
pub fn layout_of(ty: MonoType, env: ResolvedEnv) WasmLayout
```

Rules:
- `Int` → `Scalar(WI64)`
- `Float` → `Scalar(WF64)`
- `Bool`, `Byte` → `Scalar(WI32)`
- `Void` → `Scalar(WAnyref)` (canonically `i31ref` — always boxed via `RefI31`.
  This avoids context-dependent layout that would violate Rule 2. In statement
  context where Void is discarded, the emitter emits `Drop` after the i31ref.)
- `String` → `Scalar(WAnyref)` (runtime $String struct)
- `Named(type_id, args)` → look up `ResolvedTypeDef`:
  - `Record` → `Record(...)` with field layouts derived from substituted field types
  - `Sum` → `Sum(...)` with variant payload layouts
- `Vector(elem)` → `Vector_(layout_of(elem))`
- `Dict(k, v)` → `Dict_(layout_of(k), layout_of(v))`
- `Function(params, ret)` → `Closure(...)`
- `Optional(t)` → `Sum(...)` with None/Some variants
- `Result(t, e)` → `Sum(...)` with Ok/Err variants
- `Never` → no layout (must be handled as `unreachable` at every use site)

**`mono_to_key(ty: MonoType) String`** — canonical serialization of a MonoType
for use as a cache/registry key. Must produce identical keys for structurally
identical types. Example: `Named(TypeId(3), [Int, Bool])` → `"t3_i64_i32"`.
This function lives in `wasm_layout.tw` and is used by both the layout cache
and the type registry. Aliases must be resolved before keying.

**`val_type_of(layout: WasmLayout) ValType`** — converts layout to Wasm local
type. Full mapping:

| WasmLayout | ValType |
|---|---|
| `Scalar(WI32)` | `I32` |
| `Scalar(WI64)` | `I64` |
| `Scalar(WF64)` | `F64` |
| `Scalar(WAnyref)` | `Anyref` (String, Void) |
| `Record(r)` | `Ref(false, Named(r.sym))` |
| `Sum(s)` | `Ref(false, Named(s.sym))` |
| `Closure(c)` | `Ref(false, Named(c.closure_sym))` |
| `Vector_(elem)` | `Ref(false, Named("rt_types__Array"))` |
| `Dict_(k, v)` | `Ref(false, Named("rt_types__Dict"))` |
| `Iterator_(it)` | `Ref(false, Named(it.state_sym))` |
| `Cell_(inner)` | `Ref(false, Named(cell_sym))` — cell_sym derived from inner layout |

Containers map to the runtime's concrete struct/array types, not `anyref`.
This ensures container-self positions in ABI contracts match without wrapping.

**`val_type_of_mono(ty: MonoType, env: ResolvedEnv) ValType`** — shorthand
composition, with explicit `Never → unreachable` handling: this function
must trap/error if called on Never (callers must check first).

**Test:** `layout_of(Int)` → `Scalar(WI64)`. `layout_of(Optional(Int))` →
`Sum(...)` with two variants. `layout_of(Named(Point, []))` → `Record(...)`.

---

### M3: Wasm Type Registry & `plan_wasm_types` (`boot/compiler/wasm_plan.tw`)

Pre-compute the complete set of Wasm type definitions needed by the module.
The emitter receives this as input and never creates types during emission.

**Types:**

```tw
pub type WasmTypeRegistry = .{
  // Type definitions to emit (order matters for Wasm type section)
  type_defs: Vector<TypeDef>,

  // Lookup: MonoType → layout (cached from layout_of)
  layout_cache: Dict<String, WasmLayout>,

  // Lookup: layout symbol → TypeDef index
  type_index: Dict<String, Int>,

  // Closure signatures: FuncId → concrete param/return types
  concrete_func_sigs: Dict<Int, FuncSigEntry>,

  // Closure capture layouts: FuncId → ordered captured locals with types
  capture_layouts: Dict<Int, Vector<CaptureEntry>>,

  // Module globals: LocalId → symbol name
  module_globals: Dict<Int, String>,

  // String literal pool (see StringPoolEntry for emission scheme)
  string_pool: Dict<String, StringPoolEntry>,

  // Runtime import signatures (derived from BuiltinRegistry ABI contracts)
  runtime_imports: Vector<ImportDef>,
}

pub type FuncSigEntry = .{
  param_types: Vector<MonoType>,
  return_type: MonoType,
}

pub type CaptureEntry = .{
  local_id: LocalId,
  ty: MonoType,
}

pub type StringPoolEntry = .{
  global_sym: String,       // mutable global holding the cached ref $String
  getter_sym: String,       // getter function symbol
  data_segment_name: String, // data segment containing UTF-8 bytes
  byte_offset: Int,         // offset into the data segment
  byte_len: Int,            // length in bytes
}
```

**String literal emission scheme:** Each unique string literal in the module
produces three artifacts:
1. **Data segment**: UTF-8 bytes stored in a Wasm data segment (all literals
   can share one segment at different offsets).
2. **Mutable global**: `(global $str_N (mut (ref null $String)) (ref.null $String))`
   — initially null, lazily initialized on first access.
3. **Getter function**: `$str_N_get` — checks if global is null; if so,
   creates the string from the data segment via `array.new_data`, stores it
   in the global; returns the global. This lazy-init pattern avoids creating
   strings that are never used.

`ALitStr(s)` emits `[Call(getter_sym)]` — a single call to the getter.

**Core function:**

```tw
pub fn plan_wasm_types(
  anf: AnfModule,
  env: ResolvedEnv,
  builtins: BuiltinRegistry,
) WasmTypeRegistry
```

**Algorithm:**
1. Walk all `AnfFunctionDef` bodies, collecting every `MonoType` referenced
   (from `op_result_mono`, params, return types, and inline type annotations
   on `ARecord`, `AVariant`, `ARecordGet`, `ARecordUpdate`, `AIndex`).
2. For each unique MonoType, compute `layout_of` and cache it.
3. For each layout that requires a Wasm struct/functype definition, register
   the `TypeDef` in the output.
4. Scan for `AMakeClosure` → build `capture_layouts` and `concrete_func_sigs`.
5. Scan for string literals → build `string_pool`.
6. Compute module globals (locals in `__init__` referenced by other functions).
7. Build runtime imports from `BuiltinRegistry` entries with `RuntimeInfo`,
   using each entry's `AbiContract` for the exact Wasm param/result types.

**Dependency order:** Type definitions must be emitted in dependency order
(a struct referencing another struct must appear after it). Topological sort
on type symbol references.

**Test:** Plan a module with a record type, a closure, and an Option variant.
Verify all three type definitions appear in `type_defs`. Verify no duplicate
entries.

---

### M4: Boundary Insertion Pass (`boot/compiler/insert_boundaries.tw`)

Insert explicit `WrapAnyref` / `UnwrapAnyref` nodes at every point where a
typed value crosses a representation boundary (calls to runtime helpers that
use `anyref`).

**Extended ANF ops (add to `anf.tw`):**

```tw
// Added for Phase D boundary insertion
AWrapAnyref(Atom, MonoType),       // typed → anyref boxing
AUnwrapAnyref(Atom, MonoType),     // anyref → typed unboxing
```

**Core function:**

```tw
pub fn insert_boundaries(
  anf: AnfModule,
  registry: WasmTypeRegistry,
  builtins: BuiltinRegistry,
) AnfModule
```

**Runtime ABI reality:** The runtime modules do NOT use a uniform `anyref`
ABI. Each builtin has a specific Wasm-level signature with mixed types:

| Module | Example | Wasm params | Wasm result |
|--------|---------|-------------|-------------|
| `rt.arr` | `get` | `(ref null $Array, i32)` | `anyref` |
| `rt.arr` | `set` | `(ref null $Array, i32, anyref)` | `(ref $Array)` |
| `rt.arr` | `make` | `(i32, anyref)` | `(ref $Array)` |
| `rt.str` | `len` | `(ref null $String)` | `i32` |
| `rt.str` | `concat` | `(ref null $String, ref null $String)` | `(ref $String)` |
| `rt.dict` | `set` | `(ref null $Dict, anyref, anyref)` | `(ref $Dict)` |
| `rt.dict` | `has` | `(ref null $Dict, anyref)` | `i32` |
| `rt.dict` | `get` | `(ref null $Dict, anyref)` | `anyref` |

Key patterns:
- Container self-references use typed refs: `ref null $Array`, `ref null $String`
- **Element positions** use `anyref` (array elements, dict keys/values)
- Scalar args (indices, lengths) use `i32`/`i64` directly
- Scalars stored AS elements (e.g., `Int` in `Vector<Int>`) must be boxed
  to `anyref` via `RefI31` (for i31-range values) or `$BoxedInt`/`$BoxedFloat`

This means the boundary pass must know the **exact Wasm signature** of each
builtin, not assume a uniform erased ABI.

**Builtin ABI contract table:**

The boundary pass requires a per-builtin ABI contract that specifies the
Wasm-level type of each parameter and the result. This is stored in an
extended `BuiltinRegistry` (see "BuiltinRegistry extension" below).

```tw
pub type AbiContract = .{
  param_types: Vector<ValType>,    // exact Wasm param types
  result_types: Vector<ValType>,   // exact Wasm result types
}
```

Each `BuiltinEntry` gains an `abi: AbiContract` field. This is the single
source of truth for import signatures (M3), boundary insertion (M4), and
call emission (M6). The `make_builtin_registry()` function populates these
from the known runtime module signatures.

**BuiltinRegistry extension** (update `builtins.tw`):

```tw
pub type BuiltinEntry = .{
  name: String,
  func_id: FuncId,
  kind: BuiltinKind,
  abi: AbiContract,              // NEW: exact Wasm-level signature
}
```

**Algorithm:**

For each `ACall(callee, args)`:
1. Look up callee in `BuiltinRegistry`. Two cases:
   - **`Runtime(info)`**: the call becomes a Wasm import call. Compare each
     arg's semantic MonoType against the ABI contract's corresponding param
     type. Insert `AWrapAnyref` only where the semantic type is concrete but
     the ABI param is `anyref` (element positions). Container self-refs
     (e.g., `ref null $Array` for a `Vector<T>` arg) need no wrapping — just
     a ref cast. For the result, insert `AUnwrapAnyref` only where the ABI
     result is `anyref` but the semantic type is concrete.
   - **`Intrinsic`**: the emitter expands these inline (M6), but some
     intrinsics also cross the anyref boundary (e.g., `vector_get` returns
     `anyref`, `cell_get` returns `anyref`). Use the same `AbiContract` to
     determine which args/results need wrapping.
2. If callee is not a builtin (user function): no boundaries needed — all
   concrete types after monomorphization.

For each `AIndex`:
- Array indexing: element comes back as `anyref` → insert `AUnwrapAnyref`.
- Dict indexing: key/value are `anyref` → wrap key, unwrap result.
- String indexing: result is `Byte` (i32), no boundary needed.

**Boundary rules by value kind:**

| Value kind | At element/key/value position | At container-self position | At scalar position |
|------------|-------------------------------|----------------------------|--------------------|
| `Int` | Wrap to `anyref` (i31ref or BoxedInt) | N/A | No wrap (already i64) |
| `Float` | Wrap to `anyref` (BoxedFloat) | N/A | No wrap (already f64) |
| `Bool`, `Byte` | Wrap to `anyref` (i31ref) | N/A | No wrap (already i32) |
| `String` | No wrap (already ref $String) | No wrap (ref $String) | N/A |
| `Vector<T>` | Wrap to `anyref` (upcast) | No wrap (ref $Array) | N/A |
| Record/Sum | Wrap to `anyref` (upcast) | N/A | N/A |
| Closure | Wrap to `anyref` (upcast) | N/A | N/A |

The key insight: whether a value needs wrapping depends on its **position in
the ABI signature**, not just its type. An `Int` passed as an array index
(i32 position) needs no wrapping, but the same `Int` stored as an array
element (`anyref` position) must be boxed.

**Key design point:** After this pass, the emitter never needs to decide
whether to box/unbox — it's all explicit in the IR. This eliminates stage0
bug categories 1 and 4 entirely.

**Test:** A call to `vector_push(vec, item)` where `item: Int` — the `vec`
arg maps to ABI param `ref null $Array` (no wrap needed, just cast), but
`item` maps to ABI param `anyref` (insert `AWrapAnyref(item, Int)` to box
the i64 as i31ref/BoxedInt). The result type `ref $Array` is concrete and
matches the semantic `Vector<Int>` — no unwrap needed.

---

### M5: Pattern Match Emission (`boot/compiler/emit_pattern.tw`)

Dedicated module for pattern match compilation. Extracted from the main
emitter to isolate the complexity and ensure short-circuit correctness.

**Short-circuit rule (stage0 bug category 7):** Outer variant discriminant
checks MUST execute before any inner payload access. Never emit a flat AND.

**Functions:**

```tw
// Emit instructions for a complete AMatch operation
pub fn emit_match(
  scrutinee: Atom,
  arms: Vector<AnfMatchArm>,
  result_local: Int,
  ctx: EmitCtx,
) (Vector<Instr>, EmitCtx)

// Emit condition + body for one arm
fn emit_arm(
  scrutinee_local: Int,
  scrutinee_mono: MonoType,
  pattern: CorePattern,
  body: AnfExpr,
  ctx: EmitCtx,
) (Vector<Instr>, EmitCtx)

// Emit pattern bindings (extract payload fields into locals)
fn emit_pattern_bindings(
  scrutinee_local: Int,
  pattern: CorePattern,
  layout: WasmSumLayout,
  ctx: EmitCtx,
) (Vector<Instr>, EmitCtx)
```

**Emission strategy:**
- Nested `block`/`br_if` for variant dispatch (same as stage0).
- Variant tag check: `struct.get $Sum 0` → `i32.const tag` → `i32.eq` → `br_if`.
- Payload extraction: inside the matched block, AFTER the tag check succeeds.
  This ensures `struct.get` for payload fields only executes when the variant
  matches — **no eager evaluation of sub-patterns**.
- Wildcard/catch-all: final block, no tag check.

**Never in match arms:** If all arms diverge (Return/Break/Continue), the
match result type is `None` (no Wasm block result). Skip `local.set` for
the result.

**Test:** Pattern match on `Option<Int>` with None/Some arms. Verify tag
check precedes payload extraction. Verify all-diverging arms produce no
result type.

---

### M6: Core Emission — Atoms, Ops, Expressions (`boot/compiler/emit.tw`)

The main emitter. Translates ANF nodes to Wasm instructions using the
type registry (no flow metadata).

**Context:**

```tw
pub type EmitCtx = .{
  registry: WasmTypeRegistry,
  builtins: BuiltinRegistry,
  env: ResolvedEnv,

  // Per-function state (reset per function)
  local_map: Dict<Int, LocalEntry>,    // LocalId → wasm index + ValType
  next_local: Int,                     // next available Wasm local index
  label_stack: Vector<LabelPair>,      // break/continue targets
  loop_result_stack: Vector<ValType?>, // loop result types
  current_func_id: FuncId?,

  // Module-level (shared across functions)
  func_sym_map: Dict<Int, String>,     // FuncId → wasm symbol
}

pub type LocalEntry = .{
  index: Int,
  val_type: ValType,
}

pub type LabelPair = .{
  break_label: String,
  continue_label: String,
}
```

**Key design:** `EmitCtx` has NO representation metadata per local. A local's
Wasm type is decided at allocation time from its `MonoType` via
`val_type_of(layout_of(mono))`. It never changes.

**Core functions:**

```tw
pub fn emit_module(
  anf: AnfModule,
  registry: WasmTypeRegistry,
  builtins: BuiltinRegistry,
  env: ResolvedEnv,
) WasmModule

fn emit_func(func: AnfFunctionDef, ctx: EmitCtx) (FuncDef, EmitCtx)

fn emit_expr(expr: AnfExpr, ctx: EmitCtx) (Vector<Instr>, EmitCtx)

fn emit_let(local: LocalId, op: AnfOp, body: AnfExpr, ctx: EmitCtx) (Vector<Instr>, EmitCtx)

fn emit_op(op: AnfOp, result_local: Int, result_type: ValType?, ctx: EmitCtx) (Vector<Instr>, EmitCtx)

fn emit_atom(atom: Atom, ctx: EmitCtx) Vector<Instr>
```

**Atom emission:**
- `ALitInt(n)` → `[I64Const(n)]`
- `ALitFloat(f)` → `[F64Const(f)]`
- `ALitBool(b)` → `[I32Const(if b { 1 } else { 0 })]`
- `ALitStr(s)` → `[Call(string_pool_getter)]`
- `ALitVoid` → `[I32Const(0), RefI31]` (`ref.i31` consumes i32, produces i31ref)
- `ALocal(id)` → `[LocalGet(local_map[id].index)]`
- `AGlobalFunc(id)` → `[RefFunc(func_sym)]`

**Op emission (subset — full list in implementation):**
- `AInit(atom)` → emit atom, `LocalSet(result)`
- `AAssign(local, atom)` → emit atom, `LocalSet(local_map[local].index)`
- `ABinOp(op, left, right, kind)` → emit left, emit right, emit Wasm op
- `AUnOp(op, expr, kind)` → emit expr, emit Wasm op
- `ACall(callee, args)` → dispatch based on callee (see M7)
- `AIf(cond, then, else)` → emit cond, `If { then_body, else_body }`
- `ALoop(body)` → `Block { Loop { body } }`
- `ARecord(type_id, fields)` → emit fields in order, `StructNew(sym)`
- `ARecordGet(target, field_id, type_id)` → emit target, `StructGet(sym, offset)`
  where `offset` comes from `registry.layout_cache`, NOT hard-coded
- `ARecordUpdate(base, field, value, can_reuse_in_place, type_id)`:
  - If `can_reuse_in_place`: `struct.set` mutates in place. Stack discipline:
    1. Emit base (struct ref for `StructSet` target)
    2. Emit value (new field value)
    3. `StructSet(sym, offset)` — consumes struct ref + value, produces void
    4. Re-emit base (the mutated struct IS the result)
  - Else: allocate new struct, copy all fields, set updated field
- `AVariant(type_id, variant_id, args)` → emit tag, emit args, `StructNew(sym)`
- `AArrayLit(items)` → emit items, build array
- `AIndex(base, index, kind, result_ty)` → dispatch by kind
- `AMakeClosure(func_id, free_vars)` → pack captures into `$ClosureEnv` array,
  emit universal funcref (trampoline), emit typed funcref, `StructNew(closure_sym)`
- `AWrapAnyref(atom, mono)` → emit atom, box to anyref (e.g., `RefI31` for i32,
  upcast for struct refs)
- `AUnwrapAnyref(atom, mono)` → emit atom, cast from anyref (e.g., `I31GetS`
  for i32, `RefCast` for struct refs)
- `ADefer(...)` → `error("ADefer must be eliminated before codegen")` — this is
  a precondition violation (see Background section)

**Never handling (stage0 bug category 2):**

Every `emit_let` must check if the op's result type is `Never`:
- If Never: emit the op instructions (which will contain `Unreachable`),
  then emit `Unreachable` — do NOT emit `LocalSet` or continue to body.

Every `emit_expr` at `Atom` position must check if return type is Never:
- If the function return type is Never: emit atom then `Unreachable`.

**Integer narrowing (stage0 bug category 3):**

```tw
// Safe i64→i32 narrowing with bounds check in i64 domain.
// Traps if value < 0 or value > i32::MAX.
fn emit_checked_i32_narrow(ctx: EmitCtx) Vector<Instr>
```

ALL array index, string index, and length operations MUST use this helper.
No direct `I32WrapI64` except for known-safe contexts (e.g., bool → i32).

**Test:** Emit a function with `let x = 1 + 2; x`. Verify correct
instruction sequence. Emit a function returning Never — verify `Unreachable`
and no result type.

---

### M7: Call Emission & Trampolines (`boot/compiler/emit.tw` continued)

Call dispatch is the second-most complex part after pattern matching.

**Call dispatch logic:**

```tw
fn emit_call(callee: Atom, args: Vector<Atom>, ctx: EmitCtx) (Vector<Instr>, EmitCtx)
```

Three cases:
1. **Direct call to known function** (`AGlobalFunc(func_id)`):
   - Emit args, `Call(func_sym)`.
   - If calling a builtin with `RuntimeInfo`: `Call(qualified_runtime_sym)`.

2. **Indirect call via closure local** (`ALocal(id)` where type is `Function`):
   - Look up closure layout from type registry.
   - Typed path: get env (`StructGet(closure_sym, 1)`), emit args, get typed
     funcref (`StructGet(closure_sym, 2)`), `CallRef(typed_func_type_sym)`.
   - Field indices (0=universal funcref, 1=env, 2=typed funcref) come from the
     layout definition, consistent with the 3-field subtype described in M2.

3. **Direct call to function used as closure** (known FuncId but called
   through closure machinery): optimize to direct `Call` when possible.

**Trampoline emission:**

Every user function that can be used as a first-class value needs a
trampoline — a wrapper with the generic closure ABI that unpacks args and
calls the real function.

```tw
fn emit_trampoline(func_id: FuncId, sig: FuncSigEntry, ctx: EmitCtx) FuncDef
```

**Typed closure layout (from M2):**
Every closure struct is a **subtype of the universal `$Closure`**, with 3 fields:
- Field 0: universal funcref `(ref null $ClosureFunc)` — inherited from `$Closure`
- Field 1: env array `(ref null $ClosureEnv)` — captures packed here, inherited
- Field 2: typed funcref `(ref null $typed_func_type)` — concrete-signature dispatch

Captures are NOT inline struct fields. They are packed into the `$ClosureEnv`
array (field 1), same as the universal path. This ensures any typed closure can
be upcast to `$Closure` and dispatched through the universal trampoline.

**Indirect call dispatch:**
- Typed path: `StructGet(closure_sym, 2)` → `CallRef(typed_func_type)`
  (env from field 1, then concrete args, then typed funcref)
- Universal fallback: `StructGet($Closure, 0)` → `CallRef($ClosureFunc)`
  (env from field 1, then anyref-boxed args, then universal funcref)

Field offsets always from layout definition, never hard-coded.

**Test:** Direct call, indirect closure call, trampoline generation.

---

### M8: WAT Emission (`boot/compiler/wat.tw`)

Serialize `WasmModule` (or `LinkedModule`) to WAT text format.

**Core function:**

```tw
pub fn emit_wat(module: LinkedModule) String
```

This is mostly string building — formatting S-expressions from the
structured IR. Key concerns:

- **Float literal formatting (stage0 bug category 6):** `nan`, `inf`, `-inf`
  must be lowercase. Check explicitly for these values.
- **Type definition ordering:** must respect Wasm's forward-reference rules.
- **Deduplication of func types:** identical `(param ...) (result ...)` signatures
  share a single type definition.

**Test:** Round-trip: build a simple WasmModule in code, emit WAT, verify
the output compiles with wasmtime/wasm-tools.

---

### M9: Linker (`boot/compiler/linker.tw`)

Merge user module with runtime modules.

**Types:**

```tw
pub type LinkedModule = .{
  types: Vector<TypeDef>,
  imports: Vector<ImportDef>,
  funcs: Vector<FuncDef>,
  globals: Vector<GlobalDef>,
  tables: Vector<TableDef>,
  elems: Vector<ElemDef>,
  exports: Vector<ExportDef>,
  data: Vector<DataSegment>,
  start: String?,
}
```

**Core function:**

```tw
pub fn link(modules: Vector<WasmModule>, start_override: String?) LinkedModule
```

**Algorithm:**
1. Build export map: `(namespace, name) → qualified symbol`.
2. For each module, build rename map: `original_sym → ns__sym`.
3. Rewrite all symbol references in types, funcs, globals using rename maps.
4. Resolve imports: match `(import_module, import_name)` to exports.
5. Merge all definitions into flat `LinkedModule`.

**Runtime modules:** The boot compiler must embed or generate runtime module
IRs. Two approaches:
- **(a)** Port the Rust runtime module builders (`src/runtime/*.rs`) to Twinkle
  functions that return `WasmModule`. This is mechanical but verbose.
- **(b)** Embed pre-built runtime WAT as string literals, parse at link time.
  Simpler but less flexible.

Recommended: **(a)** for correctness — the runtime modules are ~800 lines of
Rust producing structured IR, and porting them ensures type-level consistency.
Defer to implementation time.

**Test:** Link a minimal user module with a mock runtime module. Verify
symbol qualification and import resolution.

---

### M10: Runtime Module Ports (`boot/compiler/runtime/`)

Port the stage0 Rust runtime module builders to Twinkle. These produce
`WasmModule` values containing the runtime type definitions and helper
functions.

**Files:**

```
boot/compiler/runtime/
  types.tw    # $Closure, $Variant, $String, $Array, etc.
  core.tw     # print, println, error, int_to_string, etc.
  arr.tw      # vector_len, vector_push, vector_get, etc.
  str.tw      # string_len, string_concat, string_slice, etc.
  dict.tw     # dict_new, dict_set, dict_get, etc.
```

Each file exports a function `fn module() WasmModule` that builds the
runtime module IR. The linker calls all of these and merges.

**Scope:** Mechanical port. Each Rust builder creates `TypeDef` / `FuncDef` /
`ImportDef` structs — the Twinkle version does the same with Twinkle types.

**Test:** Each runtime module's `module()` compiles and links with an empty
user module. Verify the linked WAT is valid.

---

### M11: Integration & End-to-End Testing

Wire the full Phase D pipeline and verify against stage0 output.

**Integration function:**

```tw
pub fn codegen(anf: AnfModule, env: ResolvedEnv, builtins: BuiltinRegistry) String
  // 1. plan_wasm_types
  // 2. insert_boundaries
  // 3. emit_module
  // 4. link with runtime modules
  // 5. emit_wat
```

**Test strategy:**

1. **Unit tests per milestone:** As described in each milestone above.

2. **Behavioral equivalence tests:** Compile the same `.tw` programs through
   both stage0 and boot, run both WAT outputs, compare execution output.
   WAT text is NOT expected to be identical (symbol names, emission order
   differ) — only execution behavior must match.

3. **WAT validation tests:** Run every emitted WAT through `wasm-tools validate`
   (or wasmtime validation) to catch structural issues early (stage0 bug
   category 6).

4. **Regression tests for each bug category:**
   - Sum boundary: Option/Result values flowing through branches
   - Never: functions with early returns, all-diverging matches
   - i64→i32: string indexing with large integers
   - Struct field: record update, closure capture access
   - Pattern match: nested variant matching with payload extraction

---

## File Layout

```
boot/compiler/
  wasm_ir.tw            # M1: Wasm IR types (TypeDef, Instr, WasmModule, etc.)
  wasm_layout.tw        # M2: WasmLayout, layout_of, val_type_of
  wasm_plan.tw          # M3: WasmTypeRegistry, plan_wasm_types
  insert_boundaries.tw  # M4: WrapAnyref/UnwrapAnyref insertion
  emit_pattern.tw       # M5: Pattern match compilation
  emit.tw               # M6–M7: Main emitter (ANF → WasmModule)
  wat.tw                # M8: WasmModule → WAT text
  linker.tw             # M9: Module linking
  runtime/
    types.tw            # M10: Runtime type definitions
    core.tw             # M10: Core runtime functions
    arr.tw              # M10: Array/vector runtime
    str.tw              # M10: String runtime
    dict.tw             # M10: Dict runtime
```

## Rust ↔ Twinkle Mapping (Phase D specific)

| Rust concept | Twinkle equivalent |
|---|---|
| `EmitCtx { repr_flow, specialized_types, ... }` with `&mut self` | `EmitCtx` record threaded purely; no repr_flow |
| `ReprFlowCtx` (4+ push/restore channels) | Eliminated — layout from type |
| `SumRepr` enum | Eliminated — sum layout from `WasmTypeRegistry` |
| `SpecializedTypeRegistry` (on-demand) | `WasmTypeRegistry` (pre-computed in M3) |
| `request_typed_closure_struct(...)` | Pre-registered in `plan_wasm_types` |
| `emit_sum_local_to_erased` (ad hoc) | Explicit `AWrapAnyref` IR node |
| `emit_anyref_option_or_variant` (runtime dispatch) | Eliminated — no mixed representations |
| `TypeSym` / `FuncSym` (interned strings) | `String` (sufficient for boot compiler) |
| `HashMap<K, V>` | `Dict<K, V>` or `Dict<Int, V>` |

## Dependencies

- `boot/compiler/anf.tw` (Phase C — exists)
- `boot/compiler/core_ir.tw` (Phase B — exists, provides CorePattern, LocalId, etc.)
- `boot/compiler/resolver.tw` (Phase A — exists, provides MonoType, ResolvedEnv)
- `boot/compiler/builtins.tw` (exists, provides BuiltinRegistry)
- `boot/compiler/anf_analysis.tw` (Phase C — exists, for free-local analysis)

## Risks & Mitigations

**Complexity of runtime module ports (M10):** The Rust runtime builders are
~800 lines total and use Wasm-level instruction building. Porting is mechanical
but tedious. Mitigation: port one module at a time with per-module integration
tests.

**String building performance for WAT (M8):** Large programs produce large WAT
output. Twinkle's string concatenation is O(n) per concat. Mitigation: use
`Vector<String>` as a buffer, join at the end. The `collect` pattern is
efficient with builder optimization.

**Boundary insertion correctness (M4):** The boundary pass must wrap/unwrap
exactly the right values — over-wrapping wastes cycles, under-wrapping causes
runtime traps. Mitigation: compare execution output against stage0 on the
full test suite.

**Deep recursion on large ANF in emitter:** Same risk as Phase C. The
let-chain structure keeps recursion depth proportional to function size, not
expression depth. Monitor on self-compilation.

---

## Appendix A: Stage0 Bug Taxonomy

The following 17 bug-fix commits to `src/codegen/emit.rs` were analyzed to
derive the design rules above. Grouped by root cause:

**1. Typed/erased sum representation mismatch (6 commits):**
`e906640`, `d99be46`, `46090e1`, `226a0e8`, `62430f2`, `f650bd0`
— metadata says "typed" but physical local is anyref, or vice versa.

**2. Never/divergence handling (4 commits):**
`ffd5c28`, `5ed7233`, `fb13099`, `fc72a8e`
— Never-typed values treated as producing stack values.

**3. i64→i32 wraparound (3 commits):**
`019a6e7`, `4410cb5`, `d4e3253`
— bounds check done after lossy i32 truncation.

**4. Stale flow metadata (3 commits):**
`ea6963f`, `54c6f37`, `d99be46`
— per-local metadata not propagated into nested scopes.

**5. Incorrect struct field index (2 commits):**
`bb68082`, `4e14555`
— hard-coded field offsets don't match layout.

**6. Wasm structural requirements (2 commits):**
`7d60484`, `112469d`
— block result types, float literal casing.

**7. Pattern match short-circuit (1 commit):**
`fb13099`
— flat AND of conditions caused inner ref.cast before outer tag check.
