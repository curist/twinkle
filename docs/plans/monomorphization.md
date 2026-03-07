# Stage 9.5 — Monomorphization

**Goal:** Eliminate all type-variable boxing by specializing generic functions at each unique
instantiation. After this pass, no `MonoType::Var` survives into ANF or codegen — every
function has fully concrete typed params and locals.

This plan is about IR and function specialization, not final Wasm data layout choices.
After monomorphization, codegen should see concrete types such as `Cell<Int>` instead of
`Cell<T>`, but deciding whether `Cell<Int>` lowers to a typed Wasm struct or an erased
runtime container is a separate backend concern tracked elsewhere.

**Why not type erasure permanently:** Type erasure (`Var → anyref`) requires boxing/unboxing
at every generic call boundary. For `fn id<T>(x: T) T` called as `id(42)`, the caller boxes
`i64` → `struct.new $BoxedInt` → `anyref`, passes it, the generic body treats `x` as `anyref`,
and the caller unboxes the result. This is 2 heap allocations and 2 casts per call. With
monomorphization, `id` is specialized to `id__Int(x: i64) -> i64` — zero overhead.

**Approach — Core IR → Core IR transform:**

The monomorphization pass runs after type checking and before Core IR → ANF lowering.
It is a whole-program transform:

1. **Collect instantiations.** Walk all `CoreExprKind::Call` nodes. For each call to a generic
   function, look up the solved type args from `TypeMap.generic_instantiations` (recorded during
   type checking per the 8c prep step). Build a map:
   `HashMap<FuncId, BTreeSet<Vec<MonoType>>>` — each generic FuncId to its set of unique
   concrete type-arg tuples.

2. **Specialize.** For each `(FuncId, type_args)` pair, clone the generic `FunctionDef`,
   substitute every `Var("T")` → concrete `MonoType` in params, return type, and body.
   Assign a fresh `FuncId` to each specialization. Name it `original_name__TypeA_TypeB`
   (e.g. `id__Int`, `map__Int_String`).

3. **Rewrite call sites.** Replace each generic `Call(func_id, args)` with
   `Call(specialized_func_id, args)` based on the call's type args.

4. **Remove generic originals.** The original generic `FunctionDef` (with `Var` types) is
   dropped — no function with `Var` types reaches ANF.

**Scope and edge cases:**

* **Rank-1 guarantee:** Damas-Milner ensures every instantiation is fully concrete and known
  at compile time. There are no higher-rank or existential types that would require runtime
  dispatch. The set of specializations is always finite.

* **Recursive generics:** `fn f<T>(x: T) { f(x) }` — the recursive call uses the same type
  args as the outer call, so it produces no new instantiations. The pass terminates because
  rank-1 prevents type args from growing (no `f(wrap(x))` where `wrap` adds a layer).

* **Transitive specialization:** If `f<T>` calls `g<T>` internally, specializing `f` to
  `f__Int` reveals a call to `g<Int>`. The pass must iterate (or process in dependency order)
  until no new instantiations are discovered. In practice this converges in 2-3 rounds for
  typical code.

* **Generic functions used as first-class values:** `let f = id` where the binding has a
  concrete type annotation (e.g. `f: fn(Int) Int = id`) — the monomorphizer generates
  `id__Int` and the closure wraps that specialization. If a generic function is stored without
  a concrete type context (e.g. `let f = id` with no annotation), the type checker already
  rejects this as `AmbiguousType`.

* **Cross-module generics:** A generic function exported from module A and called from module B
  with concrete types — the monomorphization pass runs on the linked Core IR (after all modules
  are lowered but before ANF), so cross-module instantiations are visible.

**Integration with the emitter:**

After monomorphization, the emitter never sees `MonoType::Var`. The `mono_to_valtype` mapping
for `Var` becomes `unreachable!()`. All functions have concrete Wasm signatures. The closure
trampoline generator uses concrete types.

This does **not** by itself eliminate every erased backend representation. Some runtime
layouts may still choose `Anyref` or boxed payloads even when the source type is concrete.
Those backend follow-ups, such as monomorphized `Cell<T>` layouts, belong in the Wasm
type-erasure reduction plan rather than this pass.

**Pipeline position:**

```text
parse → resolve → typecheck → lower (Core IR) → **monomorphize** → lower (ANF) → optimize → emit
```

**Deliverables:**

* `src/ir/monomorphize.rs` — the pass.
* All `tests/run/*.tw` programs produce identical output before and after monomorphization
  (differential test against interpreter).
* Wasm output for generic-heavy test programs (e.g. `generic_types.tw`, `iterator.tw`)
  shows specialized function names and no `anyref` locals in specialized bodies.
* Code-size report: compare total WAT line count with and without monomorphization on the
  test suite. Document the bloat ratio.
