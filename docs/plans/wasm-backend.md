# Wasm GC Backend & Runtime — Stage 8

## Stage 8 — Wasm GC Backend & Runtime

**Goal:** Build the full Wasm output pipeline: a Runtime IR + Linker for authoring the Twinkle
runtime at a structured level above raw WAT; a Wasm GC runtime implementing persistent arrays,
dicts, and strings; and a WAT emitter that compiles ANF IR to Wasm GC code calling into the
runtime. Produce `twk build file.tw -o output.wasm`.

**Wasm 3.0 features adopted in Stage 8:**

| Feature | Where used | Why |
|---|---|---|
| **Typed References** (`ref.func` + `call_ref`) | Stage 8b `$Closure`, Stage 8c emitter | Eliminates function table; typed, devirtualization-friendly closure calls |
| **Tail Calls** (`return_call` + `return_call_ref`) | Stage 8c emitter (tail positions) | Required for deep recursion in self-hosted compiler (Stage 10); prevents stack overflow |
| **GC** (structs, arrays, typed refs) | Entire runtime and emitter | Central to Twinkle's value model; now standardised in Wasm 3.0 |
| **JS String Builtins** | Stage 8e `rt.str` (opt-in) | Drop-in JS-native strings when `twc.wasm` runs in browser/npm host |

Features reviewed but not adopted: Multiple Memories and Memory64 (Twinkle uses GC, no linear memory); Relaxed SIMD (no SIMD use case); Exception Handling (Result + trap covers all cases without native exceptions).

**Key architectural shape:**

```text
Runtime modules (Rust-authored ModuleIR)──────────────────────┐
                                                               ▼
ANF IR → WAT emitter → user ModuleIR → Linker → LinkedModuleIR → emit → linked.wat → output.wasm
```

Both the runtime modules and the compiler-emitted user code reference types from `rt.types`
symbolically. The linker resolves all symbolic refs to numeric indices and emits a single
self-contained WAT file. The Rust `wat` crate assembles that WAT into the final `.wasm`
in-process (no external `wat2wasm`/`wasm-tools` command required).

**Distribution shape:** `twc.wasm` is the canonical compiler artifact. The Rust host (Wasmtime)
is a replaceable shell. Browser and npm hosting are natural future extensions — they implement
the same host import interface. No architecture decisions should assume the Wasmtime host is
permanent.

---

## 8a — Runtime IR + Linker (`src/wasm/`) ✅

New module `src/wasm/` with:

* `ir.rs` — symbolic IR types:
  * `TypeSym`, `FuncSym`, `GlobalSym` — stable string-based symbols (e.g. `rt.types.Array`).
  * `TypeDef`: `Struct { name, fields: Vec<FieldDef> }`, `Array { name, elem: ValType, mutable }`,
    `FuncTy { name?, params, results }`.
  * `ValType`: `I32 | I64 | F32 | F64 | Ref(Nullability, HeapType)` where
    `HeapType = Type(TypeSym) | Anyref | I31ref | Funcref | ...`.
  * `FuncDef`: `{ name: FuncSym, sig: FuncSig, locals: Vec<ValType>, body: Vec<Instr> }`.
  * `Instr` — covers the GC + numeric + control subset:
    `StructNew(TypeSym)`, `StructGet(TypeSym, field_idx)`, `StructSet(TypeSym, field_idx)`,
    `ArrayNew(TypeSym)`, `ArrayNewFixed(TypeSym, n)`, `ArrayGet(TypeSym)`, `ArraySet(TypeSym)`,
    `ArrayLen`, `RefIsNull`, `RefAsNonNull`, `RefEq`, `Call(FuncSym)`, `CallIndirect(TypeSym)`,
    `LocalGet(u32)`, `LocalSet(u32)`, `LocalTee(u32)`,
    `I32Const(i32)`, `I64Const(i64)`, `F64Const(f64)`,
    `I32Add`, `I32Sub`, `I32Mul`, `I32DivS`, `I32RemS`, `I32And`, `I32Or`, `I32Eq`, `I32LtS`,
    `I64Add`, `I64Sub`, `I64Mul`, `I64DivS`, `I64RemS`, `I64Eq`, `I64LtS`,
    `F64Add`, `F64Sub`, `F64Mul`, `F64Div`, `F64Eq`, `F64Lt`,
    `If { result, then_body, else_body }`, `Block { label, result, body }`,
    `Loop { label, result, body }`, `Br(label)`, `BrIf(label)`, `Return`, `Drop`, `Unreachable`.
  * No `RawWAT` escape hatch — extend `Instr` instead of adding escapes.
  * `ModuleIR`: collects `TypeDef`, `FuncDef`, `ImportDef`, `ExportDef`, `GlobalDef`.
  * `ImportDef`: `ImportFunc { module_ns, name, as_sym, sig }` (and memory/table if needed).
  * `ExportDef`: `ExportFunc { name, sym }`.

* `linker.rs` — `pub fn link(modules: Vec<ModuleIR>, manifest: &LinkManifest) -> LinkedModuleIR`:
  * Resolves all `FuncSym`/`TypeSym`/`GlobalSym` imports to matching exports.
  * Errors: `MissingExport`, `AmbiguousExport`, `TypeMismatch`, `NamespaceCollision`.
  * Assigns numeric indices deterministically: types first (with structurally identical
    `FuncTy` deduplication), then imports, then functions, then globals.
  * Synthesizes `__linked_init` calling each module's optional `__init` in declaration order,
    then the entry function.

* `emit.rs` — `pub fn emit_wat(module: &LinkedModuleIR) -> String`:
  * Emits standard WAT (s-expression format).
  * Also `pub fn emit_debug_json(module: &LinkedModuleIR) -> String` for inspection.

**Deliverable:** `cargo test --test wasm_ir_test` — unit tests for linking and WAT emission for
small hand-authored `ModuleIR` inputs.

---

## 8b — Runtime modules (`src/runtime/`) ✅

New top-level directory `runtime/` — Rust source files that programmatically construct
`ModuleIR` values using the `src/wasm/ir.rs` builder API. Each file is one runtime module.

**Type ownership rule:** `runtime/types.rs` (namespace `rt.types`) defines all shared Wasm GC
types. All other modules and the compiler emitter reference these by symbol; they never define
competing layouts.

Shared types in `rt.types`:

```wat
(type $Array    (array (mut anyref)))
(type $String   (array i8))                             ; UTF-8, immutable by construction
(type $DictEntry (struct (field key anyref) (field val anyref)))
(type $Dict     (array (mut (ref null $DictEntry))))    ; sorted by key, COW semantics
(type $ClosureEnv (array anyref))                       ; captured free variables
(type $ClosureFunc (func (param anyref anyref) (result anyref))) ; (env anyref, args anyref) → anyref
(type $Closure  (struct (field func_ref (ref null $ClosureFunc)) (field env (ref null $ClosureEnv))))
(type $Variant  (struct (field type_id i32) (field variant_id i32) (field payload (ref null $Array))))
(type $BoxedInt   (struct (field v i64)))
(type $BoxedFloat (struct (field v f64)))
```

> **Wasm 3.0 note:** `$Closure` stores a `(ref null $ClosureFunc)` typed function reference
> (Wasm 3.0 Typed References) instead of an `i32` function table index. Closure calls use
> `call_ref $ClosureFunc` instead of `call_indirect`, eliminating the Wasm table and element
> sections entirely. See [Stage 8c](#8c--anf--wat-emitter-srccodegen) for the call-site pattern.

**v0 data structure strategy — simplest-correct first; migrate later:**

* **Array (persistent):** copy-on-write — `rt.arr.set` copies the entire backing `$Array` and
  writes the new element. O(n) time and space. Correct semantics; replace with an RRB-tree or
  persistent trie when performance matters.
* **Dict (persistent):** sorted association list — `rt.dict.set` copies and inserts/replaces in
  order. O(n) lookup and mutation. Replace with HAMT when performance matters.
* **String:** `$String` (`array<i8>`, UTF-8). Immutable by construction; `str.concat` allocates
  a fresh array.

Runtime modules and their exported functions:

* `runtime/arr.rs` (`rt.arr`):
  `make(len: i32, fill: anyref) -> Array`,
  `get(arr, i: i32) -> anyref`,
  `set(arr, i: i32, val: anyref) -> Array` (COW — returns new array),
  `len(arr) -> i32`,
  `concat(a, b) -> Array`,
  `slice(arr, start: i32, end: i32) -> Array`.

* `runtime/dict.rs` (`rt.dict`):
  `make() -> Dict`,
  `get(dict, key: anyref) -> anyref` (returns null if absent),
  `has(dict, key: anyref) -> i32`,
  `set(dict, key: anyref, val: anyref) -> Dict` (COW),
  `remove(dict, key: anyref) -> Dict`,
  `len(dict) -> i32`,
  `keys(dict) -> Array`.

* `runtime/str.rs` (`rt.str`):
  `len(s) -> i32`,
  `concat(a, b) -> String`,
  `substring(s, start: i32, end: i32) -> String`,
  `eq(a, b) -> i32`,
  `from_i64(n: i64) -> String`,
  `from_f64(n: f64) -> String`,
  `from_bool(b: i32) -> String`.

* `runtime/core.rs` (`rt.core`):
  `eq(a: anyref, b: anyref) -> i32` (structural equality for variants/records),
  `trap(msg: String)` (calls host error),
  host imports: `host.print(s: String)`, `host.println(s: String)`, `host.error(s: String)`.

* `runtime/mod.rs`: convenience function producing a `Vec<ModuleIR>` of all runtime modules,
  ready to pass to the linker.

**Deliverable:** `twk runtime-dump` emits the linked runtime WAT. Unit tests for each runtime
function (invoke via Wasmtime in test harness, deferred to Stage 9).

---

## 8c — ANF → WAT Emitter (`src/codegen/`) ✅

**Prerequisite — ANF type annotations:** Several ANF nodes lack the type information needed
for code generation. Before starting the emitter, augment these nodes in `src/ir/anf.rs` and
update the ANF lowerer (`src/ir/lower_anf.rs`) to propagate types from the Core IR `TypeMap`:

* `ARecordGet { target, field }` → add `type_id: TypeId` (needed to cast to the correct
  `$UserRecord_N` before `struct.get`).
* `ARecordUpdate { base, field, value, can_reuse_in_place }` → add `type_id: TypeId` (needed
  for `struct.set` or copy-and-update).
* `ABinOp { op, left, right }` → add `operand_ty: NumKind` where
  `enum NumKind { Int, Float }` (needed to choose `i64` vs `f64` instructions and
  `$BoxedInt` vs `$BoxedFloat` unboxing).
* `AUnOp { op, expr }` → add `operand_ty: NumKind`.
* `AIndex { base, index }` → add `base_ty: IndexKind` where
  `enum IndexKind { Array, Dict }` (needed to choose `rt.arr.get` vs `rt.dict.get`).

Also add `param_tys: Vec<MonoType>` to `AnfFunctionDef` (propagated from the type checker's
`FunctionSignature`); the emitter uses this to emit typed locals and to generate correct
box/unbox code at function boundaries.

**Files:**

```
src/codegen/
  mod.rs          — pub mod emit; pub mod prelude; pub mod ctx;
  prelude.rs      — FuncId → runtime FuncSym mapping + Wasm import signatures
  ctx.rs          — EmitCtx: local map, label stack, type env, import set
  emit.rs         — emit_user_module(), emit_func(), emit_expr(), emit_atom()
```

* Entry: `pub fn emit_user_module(anf: &AnfModule, type_env: &TypeEnv, func_table: &HashMap<String, FuncId>) -> ModuleIR`.
* Imports all needed runtime functions by `FuncSym`; imports host functions.
* Defines Wasm GC struct types for each user record type (one `(type $UserRecord_N ...)` per
  `TypeId`), all fields `anyref` (v0).
* Emits one `FuncDef` per `AnfFunctionDef`; also emits a `__init` function for the init sequence.

**Value representation — typed locals, boxed at boundaries:**

Each Wasm local/param gets its concrete `ValType` based on its `MonoType`:

| Twinkle type       | Wasm `ValType`             | Box (→ anyref)             | Unbox (anyref →)                        |
|--------------------|----------------------------|----------------------------|-----------------------------------------|
| `Int (i64)`        | `i64`                      | `struct.new $BoxedInt`     | `ref.cast $BoxedInt` + `struct.get 0`   |
| `Float (f64)`      | `f64`                      | `struct.new $BoxedFloat`   | `ref.cast $BoxedFloat` + `struct.get 0` |
| `Bool`             | `i32`                      | `ref.i31`                  | `ref.cast i31` + `i31.get_s`            |
| `Void`             | (none / `i32`)             | `ref.i31 0`                | `drop`                                  |
| `String`           | `(ref null $String)`       | identity (already ref)     | `ref.cast $String`                      |
| `Array<T>`         | `(ref null $Array)`        | identity                   | `ref.cast $Array`                       |
| `Dict<K,V>`        | `(ref null $Dict)`         | identity                   | `ref.cast $Dict`                        |
| `Record(TypeId)`   | `(ref null $UserRecord_N)` | identity (subtype of any)  | `ref.cast $UserRecord_N`                |
| `Variant`          | `(ref null $Variant)`      | identity                   | `ref.cast $Variant`                     |
| `Closure / fn(…)`  | `(ref null $Closure)`      | identity                   | `ref.cast $Closure`                     |
| `Var("T")`         | `anyref`                   | already boxed              | `ref.cast` to concrete at use site      |

Boxing occurs at **polymorphism boundaries**: storing a typed value into something that
expects `anyref` (closure env, variant payload, `$Array` elements), and at **type-variable
positions** in generic function bodies. `MonoType::Var(_)` maps to `anyref` — callers box
arguments at generic call sites, and unbox the result afterward.

> **Monomorphization note:** The type-erasure strategy (`Var → anyref`) is the initial
> implementation. Stage 9.5 introduces a monomorphization pass that eliminates `Var` entirely
> by specializing generic functions per call-site type args. After monomorphization, no
> `Var("T")` survives into codegen and the `anyref` row above becomes dead code. See
> [Stage 9.5](future.md#stage-95--monomorphization) for details.

**Prep for monomorphization (do in Step 0):** During type checking, record the solved type
arguments at each generic call site. Add `generic_instantiations: HashMap<ExprId, Vec<MonoType>>`
to `TypeMap` (or a sibling struct). The type checker already solves these via `instantiate_vars`
+ MetaVar unification — just persist the zonked results before discarding them. This map costs
nothing at runtime and is the primary input to the Stage 9.5 monomorphization pass.

**Calling convention — hybrid direct/closure:**

* **Direct calls** (`ACall { callee: AGlobalFunc(id), args }`): Use the function's natural
  Wasm signature with typed params. No packing, no env param. Emits `call $func_N` directly.
  This is the common case and avoids all boxing/packing overhead.

* **Closure calls** (`ACall { callee: ALocal(c), args }`): Use the uniform `$ClosureFunc`
  signature `(func (param anyref anyref) (result anyref))` — first param is `$ClosureEnv`,
  second is a `$Array` of boxed args. Emits: unpack `$Closure`, box each arg into `$Array`,
  `call_ref $ClosureFunc`.

* **Closure body wrapper**: Every user function that can be stored as a closure value gets a
  generated **trampoline** `$func_N__closure` with the `$ClosureFunc` signature. The trampoline
  unpacks the `$Array` arg, unboxes each element to the expected type, calls the real
  `$func_N`, and boxes the result. `AMakeClosure { func_id, free_vars }` stores
  `ref.func $func_N__closure` in the `$Closure`.

* **`AGlobalFunc` in atom position** (e.g. `f := Array.len`): Emits `ref.func` for the
  trampoline + `struct.new $Closure` with empty env. Prelude functions similarly get trampolines.

* **0-arg functions**: Direct call passes no args. Closure call passes `ref.null none` as the
  args array.

> **Wasm 3.0 note (Typed References):** `ref.func` + `call_ref` replace `call_indirect` + a
> function table. The engine verifies type safety at validation time and can inline/devirtualize
> more aggressively. The `Instr::RefFunc` and `Instr::CallRef` variants in `src/wasm/ir.rs`
> implement this.

**Runtime/prelude calls** use native Wasm signatures, not the closure convention. The emitter
maintains a `prelude.rs` table mapping each prelude `FuncId` to its runtime `FuncSym` and Wasm
signature. At call sites the emitter converts Twinkle-typed args to the runtime's expected types
(e.g. box an `i64` to `anyref` before calling `rt.arr.set`). The runtime functions themselves
are not modified.

**Prelude FuncId → runtime symbol mapping** (in `prelude.rs`):

| FuncId | Twinkle name       | Runtime FuncSym          | Wasm signature                                    |
|--------|--------------------|--------------------------|---------------------------------------------------|
| 1      | `print`            | `rt_core__print`         | `(ref $String) → ()`                              |
| 2      | `println`          | `rt_core__println`       | `(ref $String) → ()`                              |
| 3      | `error`            | `rt_core__error`         | `(ref $String) → ()`                              |
| 4      | `int_to_string`    | `rt_str__from_i64`       | `(i64) → (ref $String)`                           |
| 5      | `float_to_string`  | `rt_str__from_f64`       | `(f64) → (ref $String)`                           |
| 6      | `bool_to_string`   | `rt_str__from_bool`      | `(i32) → (ref $String)`                           |
| 8      | `string_len`       | `rt_str__len`            | `(ref $String) → i32`                             |
| 9      | `string_concat`    | `rt_str__concat`         | `(ref $String, ref $String) → (ref $String)`      |
| 10     | `array_len`        | `rt_arr__len`            | `(ref $Array) → i32`                              |
| 11     | `array_append`     | `rt_arr__set` (COW)      | `(ref $Array, i32, anyref) → (ref $Array)`        |
| …      | (see full list in `src/ir/core.rs::prelude`) | …                     | …                                                |

**ANF → Wasm GC instruction translation (key cases):**

* `ALocal(id)` → `local.get N` (typed local).
* `AInit { value }` / `AAssign { local, value }` → `local.set N`.
* `ACall { callee: AGlobalFunc(id), args }` → push typed args, `call $func_N` (direct,
  no packing). If callee is a prelude func, box/unbox args to match runtime signature.
* `ACall { callee: ALocal(c), args }` → cast local to `$Closure`,
  `struct.get $Closure 1` (env), box args into `$Array`,
  `struct.get $Closure 0` (func_ref), `call_ref $ClosureFunc`, unbox result.
* `ABinOp { op, left, right, operand_ty }` → `local.get` both (already typed),
  apply `i64.add` / `f64.add` / etc. based on `operand_ty`. No box/unbox needed.
* `AUnOp { op, expr, operand_ty }` → same pattern.
* `AIf` → `if (result T) / else / end` where `T` is the concrete `ValType`.
* `AMatch` → nested `if`/`br_if` on `$Variant.type_id` and `$Variant.variant_id`;
  unbox payload fields from `$Array` into typed locals.
* `ALoop` / `Break` / `Continue` → `block $break_N` + `loop $cont_N` + `br`.
* `ARecord { type_id, fields }` → box each field to `anyref`, `struct.new $UserRecord_N`.
* `ARecordGet { target, type_id, field }` → `ref.cast $UserRecord_N`,
  `struct.get $UserRecord_N field_idx`, unbox result to expected type.
* `ARecordUpdate { base, type_id, field, value, can_reuse_in_place }`:
  * `can_reuse_in_place = true` → `ref.cast`, box value, `struct.set $UserRecord_N field_idx`.
  * `can_reuse_in_place = false` → `ref.cast`, copy all fields with the one updated,
    `struct.new $UserRecord_N`.
* `AVariant { type_id, variant, args }` → box args into `$Array` via `array.new_fixed`,
  `struct.new $Variant` with `i32` type_id, `i32` variant_id, payload.
* `AArrayLit(elems)` → box each element to `anyref`, `array.new_fixed $Array N`.
* `AIndex { base, index, base_ty }` → `call rt.arr.get` or `call rt.dict.get` depending
  on `base_ty`, then unbox result.
* `AMakeClosure { func_id, free_vars }` → box each free var to `anyref`,
  `array.new_fixed $ClosureEnv N`, `ref.func $func_N__closure`,
  `struct.new $Closure`.
* String literals → `array.new_fixed $String N` with `i32` byte constants (UTF-8).

**Guard:** Assert no `ADefer` nodes remain before codegen — the `defer_elim` pass must have
run. Panic with a clear message if an `ADefer` is encountered.

**Implementation steps:**

**Step 0 — ANF type annotations + monomorphization prep** ✅

*ANF annotations* (`src/ir/anf.rs`, `src/ir/lower_anf.rs`):
Add `NumKind`, `IndexKind` enums to `anf.rs`. Add `type_id` to `ARecordGet`/`ARecordUpdate`,
`operand_ty` to `ABinOp`/`AUnOp`, `base_ty` to `AIndex`, `param_tys` to `AnfFunctionDef`.
Update `lower_anf.rs` to propagate: thread the `TypeMap` through the ANF lowerer and extract
types during lowering. Update Display impls and the optimization passes that inspect these
nodes. Existing tests must still pass.

*Monomorphization prep* (`src/types/type_map.rs` or `src/types/check.rs`):
Add `generic_instantiations: HashMap<ExprId, Vec<MonoType>>` to `TypeMap`. In the type checker,
after each generic call site where `instantiate_vars` creates MetaVars and unification solves
them, persist the zonked concrete type args into this map. This is the primary input to the
Stage 9.5 monomorphization pass — recording it now is trivial and avoids a retroactive change
later.

**Step 1 — Scaffold** (`prelude.rs`, `ctx.rs`, `mod.rs`) ✅

* `prelude.rs`: `PreludeMap` — `HashMap<FuncId, PreludeEntry>` where each entry has the
  runtime `FuncSym`, param types, result type. Covers all 35 prelude FuncIds.
* `ctx.rs`: `EmitCtx` struct — `local_map: HashMap<LocalId, (u32, ValType)>` (Wasm local index
  + type), `label_stack: Vec<(Label, Label)>` (break/continue label pairs),
  `imports: BTreeSet<ImportDef>`, `type_env: &TypeEnv`, `prelude: &PreludeMap`.
* `EmitCtx::setup_locals(func: &AnfFunctionDef)` — scans body for all `Let`-bound LocalIds,
  assigns contiguous Wasm local indices after params, infers `ValType` from usage context.
* Helper: `fn mono_to_valtype(ty: &MonoType) -> ValType` — central mapping function.

**Step 2 — Atoms + literals** (`emit.rs`) ✅

* `emit_atom(atom, expected_ty, ctx)` → `Vec<Instr>`:
  * `ALocal(id)` → `LocalGet(idx)`, with box/unbox if local type ≠ expected type.
  * `AGlobalFunc(id)` → `RefFunc` + `StructNew $Closure` with null env (wraps in trampoline).
  * `ALitInt(n)` → `I64Const(n)`.
  * `ALitFloat(v)` → `F64Const(v)`.
  * `ALitBool(b)` → `I32Const(b as i32)`.
  * `ALitStr(s)` → `ArrayNewFixed $String` with UTF-8 bytes.
  * `ALitVoid` → (nothing, or `I32Const(0)` if a value is needed).

**Step 3 — BinOp, UnOp, If** ✅

* `ABinOp` — emit left + right (both typed), apply `i64.add`/`f64.mul`/`i32.eq`/etc.
  Comparison ops that cross types (e.g. `==` on strings) → `call rt_str__eq`.
* `AUnOp` — `Negate` → `i64.const 0; i64.sub` or `f64.neg`; `Not` → `i32.eqz`.
* `AIf` → `If { result: Some(valtype), then_body, else_body }`.

**Step 4 — Direct calls + prelude calls** ✅

* User-to-user direct call: push typed args, `call $func_N`.
* Prelude call: look up `PreludeEntry`, convert each arg from Twinkle type to runtime
  expected type (e.g. `i64` → `struct.new $BoxedInt` if runtime expects `anyref`), emit
  `call $rt_sym`, convert result back.
* Register each used runtime func as an import in `EmitCtx`.

**Step 5 — Closure calls + AMakeClosure** ✅

* `AMakeClosure` → generate trampoline `$func_N__closure` if not yet emitted; box free vars
  into `$ClosureEnv`, `ref.func $func_N__closure`, `struct.new $Closure`.
* Closure call → cast to `$Closure`, box args into `$Array`, extract env + func_ref,
  `call_ref $ClosureFunc`, unbox result.

**Step 6 — Records, variants, arrays** ✅

* `ARecord` → box fields, `struct.new $UserRecord_N`.
* `ARecordGet` → cast, `struct.get`, unbox.
* `ARecordUpdate` → in-place `struct.set` or copy-and-update.
* `AVariant` → box args into `$Array`, `struct.new $Variant`.
* `AArrayLit` → box elements, `array.new_fixed $Array`.
* `AIndex` → `call rt.arr.get` / `call rt.dict.get`, unbox result.

**Step 7 — Loops, break, continue** ✅

* `ALoop` → `Block { label: $break_N } + Loop { label: $cont_N, body }`.
* `Break` → `Br($break_N)`; `Continue` → `Br($cont_N)`.
* Push/pop label pairs on `EmitCtx.label_stack`.

**Step 8 — Pattern matching** ✅

* `AMatch` → `Block` per arm. Cast scrutinee to `$Variant`. For each arm:
  extract `struct.get $Variant 0` (type_id) and `struct.get $Variant 1` (variant_id),
  compare with `i32.eq` + `br_if` on mismatch.
  Bind payload fields: `struct.get $Variant 2` (payload array), `array.get` each slot,
  unbox to typed locals.
  Literal patterns: compare constants.
  Wildcard `_`: fallthrough.

**Step 9 — Build pipeline + CLI** (overlaps with 8d) ✅

* Wire `emit_user_module` into the compilation pipeline.
* Snapshot tests: compile `hello.tw`, `arithmetic.tw`, `records.tw` to WAT, assert valid
  output and no link errors.

**Post-8d Follow-up — Eliminate Non-Essential `anyref` Fallbacks** ✅

`anyref` is intentional at specific boundaries (generic type variables, closure trampolines,
runtime container element storage). That part is by design.

What is *not* intentional: local/result typing falling back to `anyref` where the compiler has
enough type information to keep concrete Wasm value types (`i64`, `f64`, concrete refs). These
fallbacks are correctness-safe but cause avoidable boxing/unboxing and allocation churn.

Findings (observed before the follow-up fixes):

1. `AIf` with one diverging branch (`continue`/`break`/`return`) can infer `anyref` instead of
   the value branch type.
2. `ALoop` result locals default to `anyref` because loop result type is not inferred from
   `break` values.
3. `ARecordGet` result local defaults to `anyref` even though `type_id + field` determines the
   exact type.
4. `AIndex` result local defaults to `anyref` because ANF does not currently carry element/result
   type metadata.
5. `AMatch` pattern-bound locals are inserted as `anyref` regardless of variant field type.

Proposed fixes (now implemented in stage0 `twk`):

1. `AIf` inference: treat diverging branch (`Never`/terminal) as compatible with the non-diverging
   branch type.
2. `ALoop` inference: infer loop result from all reachable `break` values (join type); use
   `Void/I32` only when no value break exists.
3. `ARecordGet` inference: look up record field type from `TypeEnv` via `type_id + field`, map via
   `mono_to_valtype`.
4. `AIndex` inference: extend ANF (`AIndex`) to carry result type (or element type) from Core
   expression typing and use it in local assignment.
5. Pattern binding typing: carry expected field types while traversing `CorePattern::Variant` and
   assign pattern locals with concrete `ValType` instead of unconditional `anyref`.
6. `AMatch` result inference: ignore diverging arms when inferring value type; use the
   non-diverging arm join type when available.
7. `CorePattern::Var` binding typing in `match`: use scrutinee local `ValType` instead of default
   `anyref` when pattern is a direct variable bind.

Regression coverage (added and enabled):

* `codegen::ctx::tests::local_type_if_with_continue_branch_prefers_value_type`
* `codegen::ctx::tests::local_type_loop_with_break_value_prefers_break_type`
* `codegen::ctx::tests::local_type_record_get_prefers_field_type`
* `codegen::ctx::tests::local_type_index_prefers_element_type`
* `codegen::ctx::tests::local_type_match_variant_binding_prefers_variant_field_type`
* `codegen::ctx::tests::local_type_match_var_binding_prefers_scrutinee_type`
* `codegen::ctx::tests::local_type_match_with_diverging_arm_prefers_non_diverging_type`
* `codegen::ctx::tests::local_type_match_result_payload_prefers_anyref_placeholder`

Acceptance criteria for this follow-up:

* Keep all listed regression tests enabled and green.
* Re-audit emitted WAT for fixture corpus: no `if/block (result anyref)` in user functions unless
  required by intentional type-erasure boundaries.
* Numeric hot paths (e.g. recursive `fib`) emit typed control-flow results (`i64`) without
  temporary `BoxedInt` round-trips.

**Post-8d Hardening (optimizer/codegen correctness)**

Additional findings from code review and status:

1. Dead-let elimination purity model treated all `ABinOp` as pure, which could erase trapping
   integer `Div/Mod` and suppress runtime traps.
2. Closure capture arity was inferred from callee body scans, which can drift from
   `AMakeClosure.free_vars` after optimization.
3. Wasmtime host string decode used lossy UTF-8 conversion, masking invalid runtime bytes.
4. Match arm chain always emitted `if (result <bind_ty>)`, even when both arms diverge.
5. `lower_anf` let-terminal accumulator behavior was flagged as structurally risky in malformed
   Core IR shapes (not currently observed in typed input programs).

Implemented fixes:

1. `is_pure` now marks integer `Div/Mod` as impure.
2. Closure capture layout/signatures/trampoline arity now use module-level
   `AMakeClosure.free_vars` as source of truth.
3. `run-wasm` UTF-8 decoding is now strict (`String::from_utf8` + context-rich error).
4. `AIf` and match-arm `if` now omit result type when both branches are
   known-diverging; diverging `AIf` bindings also skip dead `local.set`.
5. `lower_anf` `Let` lowering now uses an isolated value accumulator and only
   commits it on non-terminal values; terminal values return a self-contained
   `build_lets(value_accum, terminal)` subtree (no partial binding leakage).

Regression coverage:

* `opt::passes::tests::dead_let_elim_keeps_integer_div_even_when_unused`
* `opt::passes::tests::dead_let_elim_drops_unused_non_trapping_add`
* `opt::use_count::tests::is_pure_marks_integer_div_mod_impure`
* `cli::run_wasm::tests::decode_runtime_utf8_bytes_rejects_invalid_utf8`
* `codegen::emit::tests::emit_match_all_diverging_arms_emits_if_without_result_type`
* `codegen::emit::tests::emit_if_all_diverging_branches_emits_if_without_result_type`
* `ir::lower_anf::tests::let_value_terminal_keeps_value_bindings_inside_returned_subtree`
* `ir::lower_anf::tests::let_value_non_terminal_still_commits_value_bindings_to_outer_accum`

---

## 8d — Full build pipeline

Wire the complete pipeline in `src/cli/build.rs`:

1. Parse → resolve → typecheck → lower (Core IR) → [monomorphize (Stage 9.5)] → lower (ANF) → optimize → defer-eliminate.
2. `emit_user_module(anf, types)` → user `ModuleIR`.
3. Load runtime modules from `runtime/`.
4. `link([runtime_modules..., user_module], manifest)` → `LinkedModuleIR`.
5. `emit_wat(linked)` → write `output.wat`.
6. Assemble `output.wasm` in-process via the Rust `wat` crate.

**Host import interface** (what the linked module imports from `"host"`):

* `host.print(s: ref $String)` — write to stdout, no newline.
* `host.println(s: ref $String)` — write to stdout with newline.
* `host.eprint(s: ref $String)` — write to stderr, no newline.
* `host.eprintln(s: ref $String)` — write to stderr with newline.
* `host.error(s: ref $String)` — write to stderr and trap (does not return).

File I/O host imports (used by `@std.fs`; absent in programs that don't use it):

* `host.read_file(path: ref $String) -> ref $String`
* `host.write_file(path: ref $String, content: ref $String)`
* `host.write_bytes(path: ref $String, bytes: ref $Array)`
* `host.mkdirp(path: ref $String)`
* `host.list_dir(path: ref $String) -> ref $Array`
* `host.exists(path: ref $String) -> i32`

Process host imports (used by `@std.proc`; absent in programs that don't use it):

* `host.args() -> ref $Array`
* `host.env(name: ref $String) -> ref $Array` (0/1 encoded values)
* `host.cwd() -> ref $String`
* `host.exit(code: i64)`

CLI:

```bash
twk build file.tw [-o output.wasm] [--emit-wat]
```

Status update (2026-03-03):

* `twk build ... --emit-wat` now emits a sibling `.wat` when output is `.wasm` while still
  assembling `.wasm` in-process via the Rust `wat` crate.
* `twk runtime-dump` emits linked runtime WAT (always-on, no flag needed).
* Wasmtime host bindings in `run-wasm` now accept `@std.fs` host imports:
  `host.read_file`, `host.write_file`, `host.write_bytes`, `host.mkdirp`, `host.list_dir`,
  `host.exists`.
* Wasmtime host bindings in `run-wasm` now accept stderr + process imports:
  `host.eprint`, `host.eprintln`, `host.args`, `host.env`, `host.cwd`, `host.exit`.
* Build WAT golden snapshots were added for `hello.tw`, `arithmetic.tw`, `records.tw`
  (`tests/snapshots/build/*.wat`).

Deliverables:

* `twk build hello.tw` produces a runnable `hello.wasm`.
* `twk runtime-dump --wat` emits the linked runtime for inspection.
* Golden snapshot tests: a handful of programs (e.g. `hello.tw`, `arithmetic.tw`, `records.tw`)
  have their WAT output snapshotted and fail on regression.
* All runtime functions unit-tested via Wasmtime test harness.

---

## 8e — Standard library (`stdlib/`)

New directory `stdlib/` containing Twinkle source files for the MVP standard library modules.
These are compiled via the same Wasm GC backend pipeline as user programs and linked into
`twc.wasm` alongside the runtime. See [docs/stdlib.md](../stdlib.md) for the full API spec.

**`stdlib/path.tw` (`@std.path`)** — pure Twinkle, no host imports:

* `join`, `join_all`, `dirname`, `basename`, `stem`, `extension`, `normalize`, `is_absolute`.
* Testable via the Core IR interpreter immediately (no Wasm backend needed).

**`stdlib/fs.tw` (`@std.fs`)** — thin wrapper over host file I/O imports:

* `FsError` sum type: `{ NotFound, PermissionDenied, Other(String) }`.
* `DirEntry` record and `EntryKind` sum type.
* `read_text`, `write_text`, `write_bytes`, `mkdirp`, `list_dir`, `exists`.
* Calls `host.read_file`, `host.write_file`, `host.write_bytes`, `host.mkdirp`,
  `host.list_dir` — the same host imports declared in 8d.

**`stdlib/proc.tw` (`@std.proc`)** — thin wrapper over host process imports:

* `args`, `env`, `cwd`, `exit`.
* Calls `host.args`, `host.env`, `host.cwd`, `host.exit` through typed host bridge intrinsics.

**Module loader fix:** Update the module loader (`src/module/loader.rs`) to resolve `@name`
imports to `stdlib/*.tw` sources (with `TWINKLE_STDLIB_ROOT` override support) instead of
returning "not yet implemented". In stage0, stdlib modules are compiled from source as part of
normal module compilation; embedding precompiled stdlib `ModuleIR` into `twc.wasm` remains the
self-hosting stage behavior.

**Link step update:** The build pipeline from 8d gains stdlib modules in the link:

```
link([runtime_modules..., stdlib_modules..., user_module], manifest)
```

When building `twc.wasm` itself, all stdlib modules are linked in unconditionally (the
compiler must carry the full stdlib to embed it for user program builds). When `twc.wasm`
compiles a user program and produces `output.wasm`, only stdlib modules actually imported
by that user program are included — dead-module elimination at the linker level keeps
user output small.

Status update (2026-03-03):

* Added `stdlib/path.tw`, `stdlib/fs.tw`, and `stdlib/proc.tw` with MVP public APIs.
* Module compilation now resolves `use @std.*` imports via `stdlib/*.tw` instead of rejecting
  stdlib imports.
* Added host bridge intrinsics for `@std.fs` wrappers:
  `__host_read_file`, `__host_write_file`, `__host_write_bytes`, `__host_mkdirp`,
  `__host_list_dir`, `__host_exists` (typed, lowered, and codegen-mapped to `host.*` imports).
* Added host bridge intrinsics for `@std.proc` wrappers:
  `__host_args`, `__host_env`, `__host_cwd`, `__host_exit`.
* Added stderr prelude support:
  `eprint`, `eprintln` mapped through `rt.core` and host imports.
* Added end-to-end regression coverage:
  * interpreter: `tests/run/stdlib_path.tw` (`tests/run_test.rs::stdlib_path`)
  * Wasm host integration: `tests/stdlib_fs_wasm_test.rs`
  * Wasm run fixtures:
    `tests/run_wasm_test.rs::run_wasm_stdlib_path`,
    `tests/run_wasm_test.rs::run_wasm_stdlib_proc`,
    `tests/run_wasm_test.rs::run_wasm_stderr_prelude`

**Wasm 3.0 — JS String Builtins:** The `runtime/str.rs` module (`rt.str`) uses `$String (array
i8)` backed by runtime functions today. When running `twc.wasm` in a browser or npm (JS) host,
Wasm 3.0 JS String Builtins can replace the `rt.str` implementation with native JS string
operations — giving free concatenation, slicing, and comparison without UTF-8 encode/decode
at the boundary. Design `runtime/str.rs` with a clean interface seam: the exported function
symbols stay identical; a `--host=js` link-time flag swaps in a `runtime/str_js.rs` module
that emits extern-ref JS string calls instead of `array<i8>` operations. The compiler emitter
is unaffected — it calls `rt.str.*` symbolically regardless of which implementation is linked.

Deliverables:

* `use @std.path`, `use @std.fs`, and `use @std.proc` resolve and compile end-to-end.
* `@std.path` functions tested via existing interpreter test harness (`tests/run/`).
* `@std.fs` functions tested via Wasmtime test harness with a temporary directory fixture.
* `@std.proc` and stderr prelude (`eprint`, `eprintln`) tested via Wasm run fixtures.
