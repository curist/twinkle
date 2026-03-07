# Core IR & Interpreter — Stages 3–5

## Stage 3 — Core IR Design & Lowering ✅

**Goal:** Introduce a Core IR that captures Twinkle semantics with a small set of constructs, and lower typed AST into it.

Core IR (sketch):

* Expressions:

  * literals (`LitInt`, `LitFloat`, `LitStr`, `LitBool`, `Void`),
  * `Local(LocalId)`,
  * `Let { bind: LocalId, value: ExprId, body: ExprId }` (or `Block` + `Stmt`),
  * `If { cond, then_branch, else_branch }`,
  * `Call { callee, args }`,
  * `Lambda { params, body }`,
  * `Record { type_id, fields }`,
  * `RecordGet { target, field_id }`,
  * `Variant { type_id, variant_id, args }`,
  * `Match { scrutinee, arms }`,
  * `ArrayLit { elems }`,
  * `Index { base, index }`,
  * `Loop { body }`,
  * `Break { value }`,
  * `Continue`.

* Patterns for `Match`:

  * wildcard `_`,
  * binding names,
  * literal patterns,
  * variant patterns (with resolved sum type + variant id).

Lowering steps:

* Desugar:

  * `collect` into loops and explicit array building (spec §13).
  * `for` forms (`x in xs`, `key, value in dict`, `for expr`) into `Loop` (spec §12).
  * `try expr` into a `Match` over `Result` plus early-return/propagation (spec §18).
  * `.Variant(...)` shorthand into `Variant { type_id, variant_id, ... }` using type info (spec §5).
  * Lvalue assignment forms into rebinding + functional update calls (spec §7.5):
    * `r.field = expr` → `r = RecordUpdate(r, field, expr)`
    * `arr[i] = expr` → `arr = Array.set(arr, i, expr)`
    * `m[k] = expr` → `m = Dict.set(m, k, expr)`
* Convert blocks with statements into nested `Let` chains or keep a `Block` node in Core and only eliminate it later.

Deliverables:

* `twk lower file.tw` prints Core IR.
* IR tests:

  * For small programs, assert Core IR matches expectations.
  * Spot-check desugaring of `collect`, `for`, `try`, and `.Variant`.

---

## Stage 4 — Module System & Inherent Method Desugaring ✅

**Goal:** Enable multi-file programs and complete dot-syntax method resolution.

These two features are implemented together because user-defined inherent methods
(`p.translate(1,2)` → `point.translate(p,1,2)`) require knowing which module
defines the receiver type — they are fundamentally coupled.

> **Full design rationale:** See [docs/design/module.md](../design/module.md).

Features:

* **Module system** (spec §8):

  * `use foo.bar` — loads `<root>/foo/bar.tw`, binds module as `bar`.
  * `use foo.bar as baz` — loads same file, binds module as `baz`.
  * `use @array` — stdlib module (via `@` sigil); resolved from stdlib path.
  * `pub fn` / `pub type` / `pub name :=` — visibility: public vs private.
  * Module identifier = last path segment (without extension), or the `as` alias.
  * Qualified access: `math.add`, `math.Point`.
  * Per-path caching (compile each file once).
  * No destructuring in MVP (`use foo.{a,b}` is a future feature).

  * **Project root resolution:**
    1. Walk up from entry file's directory to find `twinkle.toml`.
    2. `TWINKLE_ROOT` env var overrides with an absolute path.
    3. No manifest found → entry file's directory is root (single-file scripts).

  * **Collision & error rules:**
    * Same module identifier bound twice without `as` → compile error with hint.
    * Circular imports → compile error listing the cycle.

* **Inherent method resolution (full)** (spec §9):

  * Type checker resolves `receiver.method(args)` by looking up the module
    that defines the receiver's type, finding a matching first-argument function,
    and recording its `FuncId` in `TypeMap`.
  * Lowerer reads that `FuncId` and emits `Call(GlobalFunc(id), [receiver, ...args])`.
  * Field-vs-method collision detection finalised.

* **FuncId assignment across modules:**

  * Prelude functions retain fixed FuncIds starting from 1 (see `USER_FUNC_START`
    in `src/ir/lower.rs` for the current boundary; this grows as builtins are added).
  * User functions across all imported modules are assigned FuncIds in
    deterministic order (import order, then source order within each file).

Deliverables:

* Multi-file programs compile and run through `twk lower`.
* `p.translate(1,2)` correctly desugars in Core IR output.
* Tests for: import resolution, pub/private visibility, aliasing, collision errors,
  circular import error, and inherent method calls across modules.

---

## Stage 5 — Core IR Interpreter ✅

**Goal:** Run real Twinkle programs (including multi-file) by interpreting Core IR.

The interpreter operates entirely on Core IR — it has no knowledge of source files
or modules. By Stage 5, the pipeline (resolver → type checker → lowerer) already
produces a complete, self-contained `CoreModule`, so the interpreter just walks it.

Ordering note:

* Keep interpreter-first semantics as the primary oracle.
* ANF/CFG and optimization work stays later in the pipeline so semantic regressions
  can be validated against the Core IR interpreter.

Runtime representation:

* `Value` enum with variants:

  * `Int(i64)`, `Float(f64)`, `Bool(bool)`, `Str(String)`,
  * `Arr(Vec<Value>)`, `Dict(Vec<(Value, Value)>)`,
  * `Record(TypeId, Vec<Value>)`,
  * `Variant(TypeId, VariantId, Vec<Value>)`,
  * `Closure(FuncId, HashMap<LocalId, Value>)` — FuncId of the hoisted lambda body,
    plus a snapshot of captured free-variable values (capture-by-value, spec §7.7).
    Named functions used as first-class values produce `Closure(func_id, empty_map)`.
  * `Void`.

* Environment & stack:

  * Flat `HashMap<LocalId, Value>` per call frame (LocalIds are unique per function).
  * `Assign` mutates in place; `Let` inserts a new entry.
  * Control flow signals: `Break(Option<Value>)`, `Continue`, `Return(Option<Value>)`
    bubble up through the expression tree and are caught at `Loop` / call boundaries.
  * `GlobalFunc(FuncId)` evaluates to `Closure(func_id, HashMap::new())` — a named
    function reference with no captured env.

* Lambda hoisting (lowerer work):

  * When lowering `fn(params) RetTy { body }` in expression position, the lowerer:
    1. Walks `body` to collect all `Local(id)` references not in `params` → `free_vars`.
    2. Hoists the lambda as a new `FunctionDef` with a fresh `FuncId`.
    3. Emits `MakeClosure { func_id, free_vars }` at the use site.
  * At runtime, `MakeClosure` snapshots `free_vars` from the current frame into
    `Closure(func_id, captured)`.

* Built-ins (add only as needed by tests):

  * Prelude builtins dispatched natively in `call_builtin`; see `src/ir/lower.rs`
    (`prelude` constants) for the current list and `USER_FUNC_START` for the boundary.
    Add builtins only as tests require them; bump `USER_FUNC_START` each time.
  * User functions looked up in `CoreModule.functions` by `FuncId`.

* `Dict` runtime representation: `Vec<(Value, Value)>` — no `Hash` bound needed;
  linear scan is fine for stage0. Long-term: restrict `K` to `Int` or `String`
  at the type-checker level (compiler-known closed set; `Bool` excluded).

* Module-level value bindings referenced across functions (e.g. `PI: Float = 3.14`
  used inside `fn area(r: Float) Float { PI * r * r }`) require a global store
  separate from call frames. **Deferred for Stage 5**: the init sequence supports
  top-level let bindings and expression statements, but functions referencing
  module-level globals need a cross-frame lookup mechanism not yet designed.
  For now, module globals can only be used within `__init__` itself; functions
  that need shared constants should take them as parameters.

Entry point:

* Per spec §8.1, there is **no special `main` function**. The program entry is
  the **top-level initialization sequence**: all top-level `Item::Stmt` items
  (value bindings and expression statements) lowered into a synthetic `__init__`
  function, called automatically by the interpreter. The lowerer currently
  ignores `Item::Stmt` — fixing this is a required part of Stage 5.
* `__init__` has `Void` return type. Existing test files that define `fn run()` or
  `fn main()` need a top-level call (e.g. `run()`) to produce output.
* When compiling to Wasm, `__init__` becomes the Wasm start function.

CLI:

```bash
twk run file.tw   # parse + typecheck + lower + interpret
```

Example programs are in `tests/run/` — each has expected output in a leading comment
and uses top-level expression statements as the entry point (no `fn main()` magic).

Deliverables:

* All `tests/run/*.tw` example programs produce the expected output.
* Top-level expression statements and let bindings execute in source order.
* Lambda hoisting and closure capture-by-value pass (`tests/closure/capture_by_value.tw`
  and `tests/run/closures.tw`).
* Cross-module programs run end-to-end (existing `tests/modules/` tests pass after
  adding top-level calls).
