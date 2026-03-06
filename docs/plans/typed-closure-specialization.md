# Stage 9.6 — Typed Closure Specialization

**Goal:** Eliminate anyref boxing at higher-order function call boundaries by giving
concrete-typed closures a specialized Wasm function reference type, allowing `call_ref`
to use concrete value types (`i64`, `f64`, `i32`) instead of boxing through
`$rt_types__ClosureFunc`.

---

## The Gap Left by Stage 9.5

After monomorphization, a function like `fold__Int_Int` has fully concrete type
annotations in Core IR:

```
fold__Int_Int(xs: Vector<Int>, init: Int, f: fn(Int, Int) Int) Int
```

However the WAT emitter ignores the concrete `fn(Int, Int) Int` type and still emits
`$rt_types__Closure` for all function-typed parameters, with the universal dispatch:

```wat
;; inside fold__Int_Int — even though acc and x are i64:
struct.new $rt_types__BoxedInt  ;; box acc
struct.new $rt_types__BoxedInt  ;; box x
array.new_fixed $rt_types__Array 2
call_ref $rt_types__ClosureFunc ;; (anyref, anyref) → anyref
ref.cast $rt_types__BoxedInt    ;; unbox result
struct.get $rt_types__BoxedInt 0
```

This is the same anyref round-trip that monomorphization was supposed to eliminate.
The boxing/unboxing happens **per loop iteration**, making higher-order functions over
large collections (fold, map, filter) much slower than hand-written loops.

---

## Root Cause

The universal closure type:

```wat
(type $rt_types__ClosureFunc (func (param anyref) (param anyref) (result anyref)))
```

uses a single fixed signature so any closure can be stored in `$rt_types__Closure` and
dispatched uniformly. The `__closure` wrapper (generated during lambda hoisting) adapts
the concrete hoisted function to this anyref interface:

```wat
(func $user__func_42__closure (param anyref) (param anyref) (result anyref)
    ;; unbox args, call concrete func_42, box result
    ...)
```

Wasm GC `call_ref` requires a statically-known function reference type. As long as every
closure goes through `$rt_types__ClosureFunc`, all arguments must be anyref.

---

## Approach

For each unique concrete function type `fn(T1, ..., Tn) R` (where no `Var` remains after
monomorphization), emit a **specialized closure function type** and a matching
**specialized `__closure` wrapper**.

### 1. Typed closure function types

For each distinct concrete function signature appearing as a parameter type in any
post-monomorphization `FunctionDef`, emit a dedicated Wasm `func` type:

```wat
(type $closurefunc_i64_i64_i64 (func
    (param (ref null $rt_types__Array))  ;; closure env
    (param i64)                          ;; arg 0: Int
    (param i64)                          ;; arg 1: Int
    (result i64)))                       ;; result: Int
```

### 2. Specialized `__closure` wrapper per concrete signature

Instead of one `__closure` wrapper per hoisted function (adapting to the universal
anyref interface), generate a wrapper per concrete call site type:

```wat
(func $user__func_42__closure_i64_i64_i64
    (param (ref null $rt_types__Array))
    (param i64)
    (param i64)
    (result i64)
    local.get $p1   ;; acc — already i64, no unboxing needed
    local.get $p2   ;; x   — already i64, no unboxing needed
    call $user__func_42)
```

### 3. Typed closure struct variant

Add a typed variant of `$rt_types__Closure` per concrete signature, or use the existing
struct with a typed `funcref` field (Wasm GC supports `(ref $functype)` as a field type):

```wat
(type $closure_i64_i64_i64 (struct
    (field (ref $closurefunc_i64_i64_i64))
    (field (ref null $rt_types__ClosureEnv))))
```

### 4. Typed `call_ref` at specialized call sites

In `fold__Int_Int`, instead of boxing and using `call_ref $rt_types__ClosureFunc`:

```wat
;; typed call — no boxing, no unboxing:
local.get $p3           ;; acc: i64
local.get $p9           ;; x: i64
local.get $p2           ;; f: (ref $closure_i64_i64_i64)
struct.get $closure_i64_i64_i64 0
call_ref $closurefunc_i64_i64_i64
local.set $p3           ;; result: i64 directly
```

---

## Scope and Edge Cases

* **Closures stored in data structures** (e.g. `Vector<fn(Int) Int>`): elements are
  still accessed as `anyref` from the array, so an unbox/cast is needed at read time.
  This stage targets function-typed *parameters* in monomorphized functions, not
  collection elements.

* **Generic closures still in use** (e.g. a closure passed to a non-monomorphized
  generic function): those call sites continue using `$rt_types__ClosureFunc`. Both
  wrapper variants (universal and typed) coexist; the hoisted function is shared.

* **Multiple concrete signatures for the same closure:** if `id` is used as both
  `fn(Int) Int` and `fn(String) String`, two `__closure` wrappers are generated
  (`__closure_i64` and `__closure_anyref`). The hoisted body `$user__func_id` is not
  duplicated.

* **Env-capturing closures:** the env field is still passed as `(ref null
  $rt_types__ClosureEnv)`; only the argument/result types become concrete.

---

## Expected Impact

The benchmark in `benches/wasm_exec.rs` (`bench_generic/exec`) measures this directly:
`fold` over a 100k-element vector calling a concrete `fn(Int, Int) Int` closure. After
this stage the per-iteration `call_ref` cost drops from anyref boxing (multiple heap
allocations per element) to a plain typed function call.

---

## Pipeline Position

No change to pipeline ordering. This is a codegen improvement, not a Core IR transform:

```text
... → monomorphize (9.5) → lower (ANF) → optimize → emit (9.6 changes here)
```

The ANF emitter (`src/codegen/emit.rs`) is the primary change site. The runtime
(`src/wasm/ir.rs`, `rt.types`) needs the new typed closure struct and functype
definitions.

---

## Deliverables

* New typed `ClosureFunc` variants in `src/wasm/ir.rs` / runtime type definitions.
* `emit.rs`: detect concrete `MonoType::Function` params in post-mono `FunctionDef`s;
  emit typed `call_ref` and typed `__closure` wrappers.
* `benches/wasm_exec.rs` `bench_generic` shows meaningful speedup for higher-order
  functions over large collections.
* All existing `tests/run_wasm_test.rs` fixtures continue to pass.
