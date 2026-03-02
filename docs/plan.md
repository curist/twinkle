# Twinkle Implementation Plan

## Goal

Build a self-hosting Twinkle compiler whose canonical artifact is `twc.wasm` — a single
WebAssembly module that can run in any compliant host. The Rust `twk` binary is the first
host shell; browser and npm hosts follow naturally from the same interface.

* A small Rust **stage0** implementation (`twk`) for fast iteration.
* A clear internal pipeline: Source → AST → Typed AST → Core IR → ANF → Wasm GC backend.
* An **interpreter-first** path (Core IR interpreter) as the semantic oracle.
* A **Wasm GC backend** that emits code calling into a persistent-data-structure runtime.
* `twc.wasm` as the stable, host-agnostic compiler artifact.

---

## High-Level Architecture

Compiler pipeline:

```text
Twinkle source
  → Lexer
  → Parser (AST with spans)
  → Typechecker (bidirectional, Damas–Milner)
  → Core IR (expression+block, loops, match, variants)
  → ANF IR (backend-oriented, with optimization passes)
  → Backend(s):
       - Core IR Interpreter (stage0, semantic oracle)
       - Wasm GC backend → Runtime IR + Linker → linked.wat → output.wasm
```

Runtime / distribution:

```text
Runtime modules (rt.types, rt.arr, rt.dict, rt.str, rt.core)   ┐
Stdlib modules  (compiled from stdlib/*.tw via Wasm GC backend) ├─→ Linker → twc.wasm
Compiler modules (compiled from compiler/*.tw via stage0)       ┘

                                                                  ┌── (stdlib + runtime ModuleIR
                                                                  │    embedded in twc.wasm)
user source files → twc.wasm (running in host) → user ModuleIR → Linker → output.wasm
```

* **`twc.wasm`** bundles three things: the compiler, the stdlib, and the runtime — all linked
  together by the same Runtime IR + Linker pipeline. It is `output.wasm` when the sources are
  `compiler/main.tw` + `stdlib/*.tw` + the runtime modules.
* **Stdlib is embedded**, not loaded from disk. Stdlib `.tw` sources are compiled via the
  Wasm GC backend to `ModuleIR` and linked into `twc.wasm` at build time. The host only needs
  to provide FS access for user source files and build outputs — not for the stdlib.
* When compiling a user program, `twc.wasm` carries the pre-compiled stdlib and runtime
  `ModuleIR` internally. It emits the user's `ModuleIR`, then links it together with those
  embedded artifacts to produce `output.wasm`.
* Once self-hosted, the **host shell drives `twc.wasm`**: it provides file I/O (reading user
  source files, writing output) and instantiates `twc.wasm`, which executes the full compiler
  pipeline internally. The compiler pipeline diagram above describes what runs *inside* `twc.wasm`.
* The Rust host (Wasmtime) is a replaceable shell; browser and npm hosting implement the
  same host import interface.
* **Host interface** (what any host must provide):
  * Console: `host.print`, `host.println`, `host.error`.
  * File I/O (for reading user source files and writing build outputs; stdlib is embedded):
    `host.read_file`, `host.write_file`, `host.write_bytes`, `host.mkdirp`, `host.list_dir`,
    `host.exists`.
  * Paths are logical (`/`-separated); the host maps them to OS paths or virtual FS.
  * No clock, no randomness, no process spawning — compiler output is deterministic.
* In development: `twk` (Rust binary) with subcommands
  `parse`, `check`, `run`, `lower`, `lower-anf`, `opt`, `build`, `runtime-dump`.
* `run` uses the Core IR interpreter; `build` uses the Wasm GC backend.
* Self-hosted: `twc.wasm` compiled by stage0, then compiles itself.

---

## Design Principles

1. **Pure compiler core**
   Compiler modules operate only on in-memory data:

   * `String → Vec<Token> → AST → Typed AST → IR`.
   * File I/O, CLI, and host integration live in thin wrappers.

2. **Core IR as semantic backbone**
   Twinkle features (`collect`, `try`, `.Variant`, `for x in`, etc.) are desugared into a **Core IR** that directly expresses the semantics in a small set of constructs (spec §7.5, §12, §13, §18):

   * `Let`, `If`, `Match`, `Loop`, `Call`, `Record`, `Variant`, etc.

3. **ANF for backend friendliness**
   A simple **ANF IR** can be derived from Core IR:

   * Every intermediate result bound to a `let`.
   * Evaluation order explicit, easy to map to WAT/Wasm.
   * Optional at first; used primarily by the backend.

4. **Interpreter-first**
   Stage0 backend is an interpreter for Core IR:

   * Fast path to a usable language.
   * Provides a reference semantics engine.
   * Makes it easier to validate language design before committing to Wasm details.

5. **WAT/Wasm is a later backend**
   A WAT/Wasm backend is added after Core IR and interpreter are solid:

   * Emit `.wat` text from IR/ANF.
   * Use Wasmtime (or another runtime) as an external tool at first.
   * Eventually integrate Wasmtime into CLI and move toward self-hosting.

6. **Self-hosting as a deliberate phase**
   Once the language and compiler pipeline stabilize:

   * Re-implement the compiler in Twinkle.
   * Use stage0 Rust compiler to compile the Twinkle compiler to WAT/Wasm.
   * Distribute the Twinkle-implemented compiler as a `.wasm` artifact.

---

## Repository Layout (Stage0-Oriented)

Suggested Rust layout:

```text
twinkle/
  src/
    main.rs               # CLI entry (twk)
    cli/                  # CLI commands (parse/check/run/build)
    syntax/
      lexer.rs
      tokens.rs
      parser.rs
      ast.rs
      span.rs
    types/
      ty.rs               # type representation
      unify.rs            # unification
      infer.rs            # type inference / checking
      env.rs              # type and value environments
    ir/
      core.rs             # Core IR definitions
      lower_core.rs       # AST → Core IR lowering
      anf.rs              # ANF IR definitions
      lower_anf.rs        # Core IR → ANF lowering (later)
    interp/
      value.rs
      eval_core.rs        # Core IR interpreter
    codegen/
      wat.rs              # IR/ANF → WAT backend (later)
  tests/
    parser/
    typecheck/
    ir/
    run/
  docs/
    plan.md               # (this document)
    ir.md                 # Core IR & ANF spec (later)
    lang-spec.md          # language spec
```

This keeps the front end, IR, interpreter, and backend clearly separated.

---

## Stages

### Stage 0 — Skeleton & Testing Infrastructure ✅

**Goal:** Basic structure and a test harness.

* Set up crate with the module layout above.
* Add a `twk` binary with stub subcommands:

  * `twk parse file.tw`
  * `twk check file.tw`
  * `twk run file.tw`
  * `twk build file.tw`
* Implement a minimal golden-test harness:

  * Read `.tw` files from `tests/parser/`,
  * For now, just assert “parses” or “returns an error”.
* Wire CI (e.g. `cargo test`).

Deliverable:

* Project compiles.
* Tests run.
* No real language yet, but the skeleton is stable.

---

### Stage 1 — Lexer, Parser, Spans ✅

**Goal:** Parse full Twinkle surface syntax into an AST with precise spans.

Features:

* Tokens:

  * identifiers, keywords, literals (`Int`, `Float`, `String`, `Bool`),
  * operators (`+ - * / % == != < <= > >= and or`),
  * punctuation (`(` `)` `{` `}` `[` `]` `,` `:` `.` `:=` `=` etc.).
* Comments:

  * `//` line comments,
  * possibly doc comments (`/// ...`).
* String interpolation (spec §11):

  * Lexed as alternating `STRING_SEGMENT` + `${` *Expr* `}` tokens.
* Parser:

  * Expressions with precedence (`or` < `and` < `==` < `<` < `+ -` < `* / %`).
  * Blocks `{ ... }` as expression-with-statements.
  * `if` expressions (spec §12).
  * `case` expressions (spec §5, §12).
  * `for` / `collect` (spec §12, §13).
  * Function declarations (`fn name(...) [ReturnType] Block`) (spec §7.1).
  * Type declarations (records + sum types + type aliases) (spec §3, §5, §6).
  * Top-level statements and expressions (spec §8.1).

Every AST node carries a `Span`:

```rust
pub struct Span {
    pub file_id: FileId,
    pub start: u32,
    pub end: u32,
}

pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}
```

Deliverables:

* `twk parse file.tw` prints/unparses AST or a debug representation.
* Parser test cases:

  * Operator precedence and associativity.
  * Block nesting.
  * Basic error reporting with spans.

---

### Stage 2 — Name Resolution & Monomorphic Typechecking ✅

**Goal:** Typecheck non-generic programs with basic types and declarations.

Features:

* Type representation (monomorphic for now):

  * Primitive: `Int`, `Float`, `Bool`, `Str`, `Void` (spec §2).
  * Records: nominal record types with fields (spec §6).
  * Sum types: nominal variants (`type Result = { Ok(Int), Err(Str) }`) (spec §5).
  * Arrays & dicts: `Arr<T>`, `Dict<K,V>` (spec §14, §17).
  * Functions: `fn(T1, T2, ...) Tret` (spec §7.1).
  * Type aliases: `type ID = Int` — expands transparently, not a new nominal type (spec §3).

* Name resolution:

  * Module-level symbol table for:

    * `type` declarations,
    * `fn` declarations,
    * top-level values (spec §8.1, §8.2).
  * Basic support for qualified names in types and expressions (e.g. `Module.Point`).

* Typechecker:

  * Expression typechecking.
  * Let bindings (spec §7.2, §7.3):

    * `x := expr` (inferred).
    * `x: T = expr` (checked).
  * Function declarations and calls.
  * `if` expressions (branch type agreement) (spec §12).
  * `case` expressions (spec §5, §12):

    * scrutinee type must be a sum type.
    * arms must all produce a compatible result type.
    * basic exhaustiveness checking (can start minimal).

Deliverables:

* `twk check file.tw` reports:

  * success, or
  * clear type errors with locations.
* Typechecker tests:

  * Correct typing for simple examples.
  * Expected failures for incompatible types.

---

### Stage 3 — Core IR Design & Lowering ✅

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

### Stage 4 — Module System & Inherent Method Desugaring ✅

**Goal:** Enable multi-file programs and complete dot-syntax method resolution.

These two features are implemented together because user-defined inherent methods
(`p.translate(1,2)` → `point.translate(p,1,2)`) require knowing which module
defines the receiver type — they are fundamentally coupled.

> **Full design rationale:** See [docs/module.md](module.md) D-001 through D-009.

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

### Stage 5 — Core IR Interpreter ✅

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

---

### Stage 6a — User-Defined Generics & Bidirectional Type Checking ✅

**Goal:** Support generic type declarations and bidirectional checking for common expression forms.

What was done:

* User-defined generic record and sum types (`type Pair<A,B> = .{ ... }`, `type Tree<T> = { ... }`).
* One `TypeId` per generic definition; field/variant types stored with `Var("T")` placeholders.
* Substitution applied at field reads, record literals, variant literals, patterns, and capability records.
* Bidirectional `check_expr` for `case` arms, anonymous record literals, lambda params, and `if` branches.
* `MonoType::Never` (bottom type) for diverging expressions (`break`/`continue`/`return`).
* `try expr` sugar desugared into a match over `Result`.

Remaining limitation: type variables are only introduced via explicit `<T>` parameters on `fn`/`type`
declarations. Call-site inference (e.g. `let f := id` where `id` is polymorphic) is not supported —
see Stage 6b.

---

### Stage 6b — Query-Friendly Pipeline Refactor ✅

**Goal:** Reshape the compiler pipeline so each stage is a pure function with explicit
inputs and outputs, enabling independent stage invocation, per-file incremental
re-compilation, and better testability — without adding any framework dependency.

> **Full design:** See [docs/query-pipeline.md](query-pipeline.md).

Key changes:

* Replace `CompilationContext` mutation with per-module artifact structs:
  `ResolvedModule`, `TypedModule`, `LoweredModule`, `LinkedProgram`.
* Each stage becomes a pure function: `resolve(ast, deps)`, `typecheck(ast, resolved)`,
  `lower(ast, typed)`, `link(modules)`.
* FuncIds assigned module-locally (starting at USER_FUNC_START, after prelude slots)
  and remapped by the linker with per-module base offsets — stable across re-compilations.
* `compile_module` becomes a thin coordinator; no shared mutable state.
* `CompilationContext` shrinks to just the module loader cache and import stack.

**Current status (2026-02-28):**

* Artifact structs exist (`ResolvedModule`, `TypedModule`, `LoweredModule`) and stage
  boundaries are cleaner than before.
* `resolve`, `typecheck`, and `lower` are callable with explicit inputs and artifacts.
* The `compile_module` coordinator now uses explicit stage data flow (no env-swap
  `mem::replace` pattern).
* `CompileState` is reduced to module-graph/coordinator accumulation state.
* User FuncIds are now module-local during lowering and remapped in `link` with a
  deterministic topo order of modules.
* FuncId stability tests exist for import-order changes and unrelated entry-module edits.
* An in-process content-hash stage cache exists for parse / resolve / typecheck / lower,
  including reverse-dependent invalidation and cache hit/miss tests.
* Tool-facing query API includes structured diagnostics, direct stage entry points, and
  symbol queries without requiring `CompileState` construction.

Stage 6b scope is complete in this repo. The compiler can be called query-style for
parse/resolve/typecheck/lower and supports in-process incremental reuse.

* **6b.1 Stateless stage contracts**
  * done: stage functions consume explicit inputs and return artifacts;
  * done: env swap pattern removed from coordinator stage flow.

* **6b.2 Stable module-local IDs + linker remap**
  * done: module-local FuncIds + deterministic linker remap;
  * done: stability tests for import-order and unrelated edits.

* **6b.3 Incremental cache database**
  * done: content-hash keys + independent stage caches;
  * done: reverse-dependent invalidation;
  * non-goal for Stage 6b: on-disk persistence across process invocations.

* **6b.4 Tool-facing query API**
  * done: parse/resolve/typecheck/lower APIs + structured diagnostics;
  * done: symbol query API + default query context helpers.

Deliverables (done when all below are true):

* All existing tests (`tests/run/`, `tests/modules/`) pass unchanged.
* Each stage independently testable without constructing a full context.
* No Salsa or other framework dependency introduced.
* Incremental tests prove unchanged modules skip resolve/typecheck/lower.
* FuncId stability tests prove deterministic IDs after unrelated edits.

**Execution checklist (file/module map):**

* **Step A — Refactor stage boundaries (`6b.1`)**
  * `src/module/mod.rs`:
    * split orchestration from stage logic; coordinator should pass immutable inputs and collect outputs;
    * remove env swapping pattern in favor of explicit stage data flow.
  * `src/module/context.rs`:
    * shrink or remove `CompileState` as mutable cross-stage carrier;
    * move only loader/cycle detection concerns into a thin context.
  * `src/module/artifacts.rs`:
    * extend artifact structs so all stage outputs are explicit (including method registrations / per-module metadata).
  * `src/types/resolve.rs`, `src/types/check.rs`, `src/ir/lower.rs`:
    * keep stage functions pure over explicit inputs; no hidden mutation dependencies.

* **Step B — Stable IDs via linker remap (`6b.2`)**
  * `src/ir/lower.rs`:
    * assign module-local user FuncIds (per-module numbering), not global monotonic IDs.
  * `src/module/artifacts.rs`:
    * store module-local function sets and metadata required for remapping.
  * `src/module/mod.rs` (`link`):
    * topologically order modules, assign module base offsets, and remap all FuncId references.
  * Tests to add:
    * `tests/modules_test.rs` / new dedicated test file asserting FuncId stability under import-order changes.

* **Step C — Incremental cache + invalidation (`6b.3`)**
  * New module recommended: `src/query/` (`mod.rs`, `cache.rs`, `keys.rs`, `graph.rs`).
    * stage cache keying: source hash + transitive dep hashes + stage context hash.
    * dep graph and reverse-dependency invalidation.
  * `src/module/mod.rs`, `src/module/loader.rs`:
    * integrate cache lookup/store and dependency graph updates.
  * `src/cli/check.rs`, `src/cli/build.rs`, `src/cli/run.rs`:
    * add cache-aware execution paths (warm/cold behavior).
  * Tests to add:
    * cache hit/miss behavior;
    * reverse-dependent invalidation when an imported module changes.

* **Step D — Tool-facing query API (`6b.4`)**
  * `src/lib.rs`:
    * export stable query entry points for parse / resolve / typecheck / diagnostics / symbol lookup.
  * New module recommended: `src/query/api.rs`:
    * single ergonomic facade used by CLI, future `twk lsp`, and future `twk lint`.
  * `src/types/error.rs`, `src/syntax/span.rs`:
    * ensure diagnostics include stable machine-readable IDs + spans + severity.
  * Tests to add:
    * API-level snapshots for diagnostics and symbol queries.

**Recommended order:** A -> B -> C -> D (do not start LSP incremental work before C).

---

### Stage 6c — Full Damas–Milner Inference ✅

**Goal:** Complete the type inference engine with unification, generalization, and instantiation at use sites.

Features:

* True type variables and unification:

  * `MonoType::MetaVar(u32)` — fresh unification variables created at each generic instantiation site.
  * Full structural unification engine with occurs check.
  * `zonk` / `zonk_ty` — apply substitution maps to resolve MetaVars after checking.

* Generalization at `fn` declarations (not local `:=` bindings):

  * `fn id<A>(x: A) A { x }` — polymorphic; `A` is generic.
  * `f := id` — error (`AmbiguousType`): cannot bind a polymorphic function without a type annotation.
  * `annotated: fn(Int) Int = id` — annotation provides a concrete type; accepted.

* Instantiation at use sites (all dispatch paths):

  * Direct calls: `id(42)` → fresh MetaVar solved to `Int` by argument.
  * Module-qualified calls: `lib.id("s")` instantiated from full `FunctionSignature`.
  * Inherent method calls: `box.get()` where `box: Box<String>` — receiver type unifies MetaVars.
  * Zero-arg generic variants: `UnfoldStep.Done` via field-access path now uses MetaVars, not raw `Var`.
  * `TypeName.Variant(args)` calls: already used MetaVars; verified consistent.

* Soundness invariants enforced:

  * `Var(_)` wildcard removed from `unify` — `fn bad<A>(x: A) Int { x }` is now a type error.
  * `AmbiguousType` reported for: unannotated bindings holding unsolved MetaVars, inferred function return types containing MetaVars, generic references used without calling.
  * `OccursCheckFailed` guard in `solve_meta` for infinite-type prevention (unreachable at current language level due to required parameter annotations; documented in-code for when unannotated lambdas are introduced).
  * Per-function MetaVar scope: `meta_subst` cleared and TypeMap zonked after each function; final zonk after module-level stmts.

Core IR does not change; it just gets richer type annotations.

Deliverables:

* `twk check` supports call-site type inference for generic functions across all dispatch paths.
* Type inference tests:

  * `tests/typecheck/pass/inference.tw` — direct calls, higher-order (`apply`), annotated binding.
  * `tests/typecheck/fail/polymorphic_binding.tw` — `f := id` rejected as ambiguous.
  * `tests/typecheck/fail/generic_body_return_mismatch.tw` — `fn bad<A>(x: A) Int { x }` rejected.
  * `tests/typecheck/fail/generic_method_mismatch.tw` — `Box<String>.get()` assigned to `Int` rejected.
  * `tests/typecheck/fail/generic_ref_escape.tw` — unapplied generic reference in function body rejected.

---

### Stage 7 — ANF IR (Backend-Oriented) ✅

**Goal:** Add an ANF (Administrative Normal Form) IR layer between Core IR and the WAT
backend. ANF makes evaluation order explicit and ensures every non-trivial intermediate
value is bound to a named local — a requirement for straightforward WAT/Wasm code generation.

Ordering note:

* Keep ANF at this stage (after interpreter + generics), not before Stage 5.
* This keeps execution semantics anchored by Core IR first, then introduces
  backend-oriented normalization.

**Scope decision:** The interpreter continues to use Core IR. No ANF interpreter is added
at this stage. Behavioral preservation is validated by structural invariant checks and
golden output snapshots; full equivalence testing against the interpreter is deferred to
Stage 8 where the WAT backend becomes the second execution path.

ANF IR structure (full spec in `docs/ir.md §3`):

* **Atom** — trivially available values (locals or literals):
  * `ALocal(LocalId)`, `ALitInt(i64)`, `ALitFloat(f64)`, `ALitBool(bool)`,
    `ALitStr(String)`, `ALitVoid`.

* **ANFExpr** — a flat let-chain terminating in an atom:
  * `Let { local: LocalId, op: AnfOp, body: Box<ANFExpr> }`
  * `Return(Atom)` — function return (terminal).
  * `Break(Option<Atom>)` — loop break (terminal).
  * `Continue` — loop continue (terminal).

  > Note: `Break`/`Continue` are terminal `ANFExpr` variants, not `AnfOp` entries,
  > because they carry no value to bind and the body after them is unreachable.

* **AnfOp** — a single non-atomic computation whose result is bound by the enclosing `Let`:
  * `ACall { callee: Atom, args: Vec<Atom> }`
  * `AIf { cond: Atom, then_branch: ANFExpr, else_branch: ANFExpr }`
  * `AMatch { scrutinee: Atom, arms: Vec<AnfMatchArm> }`
  * `ALoop { body: ANFExpr }`
  * `ABinOp { op: BinOp, left: Atom, right: Atom }`
  * `AUnOp { op: UnOp, expr: Atom }`
  * `AMakeClosure { func_id: FuncId, free_vars: Vec<Atom> }`
  * `ARecord { type_id: TypeId, fields: Vec<(FieldId, Atom)> }`
  * `ARecordGet { target: Atom, field: FieldId }`
  * `ARecordUpdate { base: Atom, field: FieldId, value: Atom }`
  * `AVariant { type_id: TypeId, variant: VariantId, args: Vec<Atom> }`
  * `AArrayLit(Vec<Atom>)`
  * `AIndex { base: Atom, index: Atom }`
  * `AAssign { local: LocalId, value: Atom }` — maps to Wasm `local.set`.

Core → ANF lowering rules (from `docs/ir.md §4`):

* **A1** — Non-atom subexpressions are let-bound to fresh temporaries before use.
  The lowering is continuation-passing: `lower_expr(expr, cont)` where `cont` is
  the rest of the computation that expects an `Atom`.
* **A2** — `If` cond is atomized; branches are lowered recursively into `ANFExpr`.
* **A3** — `Match` scrutinee is atomized; arm bodies lowered recursively.
* **A4** — `Loop` body lowered independently into `ANFExpr`.
* **A5** — `MakeClosure` free vars are already locals (atoms); lambda body lowered
  as an independent function.

Fresh temporaries: a simple counter per function, starting above the function's
existing max `LocalId`. No need for the full `LocalAllocator`.

Deliverables:

* `twk lower-anf file.tw` prints ANF IR in a readable form.
* All programs in `tests/run/` pass ANF invariant checks (see Step D).
* Golden ANF output snapshots for a representative subset of test programs.

**Execution checklist (file/module map):**

* **Step A — ANF IR type definitions (`src/ir/anf.rs`)**
  * Define `Atom`, `AnfExpr`, `AnfOp`, `AnfMatchArm` per the structure above.
  * Define `AnfFunctionDef { func_id: FuncId, params: Vec<LocalId>, body: AnfExpr, return_ty: MonoType }`.
  * Define `AnfModule { functions: Vec<AnfFunctionDef>, init_func_id: FuncId }` mirroring `CoreModule`.
  * Implement `Display` (or a `pretty_print`) for `AnfExpr` — used by `twk lower-anf`.
  * Register `pub mod anf` in `src/ir/mod.rs`; re-export `AnfModule`.

* **Step B — Core → ANF lowering pass (`src/ir/lower_anf.rs`)**
  * Entry point: `pub fn lower_module(module: &CoreModule) -> AnfModule`.
  * Per-function: `lower_func(func: &FunctionDef) -> AnfFunctionDef`.
  * Core expression lowering via CPS: `lower_expr(expr: &CoreExpr, cont: impl FnOnce(Atom) -> AnfExpr) -> AnfExpr`.
    * Atomic cases (`LitInt`, `LitBool`, `Local`, etc.) call `cont` directly with the atom.
    * Non-atomic cases (e.g. `BinOp`, `Call`, `Record`) recursively atomize their subexpressions,
      allocate a fresh `LocalId`, emit `Let(tmp, AnfOp, cont(ALocal(tmp)))`.
    * Terminal cases (`Break`, `Continue`, `Return`) emit the terminal `ANFExpr` variant directly
      (ignore `cont` — unreachable after a terminal).
    * Structural cases (`If`, `Match`, `Loop`) atomize their guard/scrutinee and recurse into branches.
  * Fresh temp counter: track `next_temp: u32` per function, initialized to `max(params) + 1` or
    the function's local count.

* **Step C — CLI command (`src/cli/lower_anf.rs`)**
  * Implement `pub fn cmd_lower_anf(path: &Path) -> anyhow::Result<()>` using the same pipeline
    as `twk lower`: parse → resolve → typecheck → lower (Core IR) → `lower_anf::lower_module`.
  * Wire as `twk lower-anf <file>` in `src/cli/mod.rs` and `src/main.rs`.
  * Fix stale comment in `src/codegen/mod.rs`: change `// WAT/Wasm backend - Stage 7` to
    `// WAT/Wasm backend - Stage 8`.

* **Step D — Tests (`tests/anf_test.rs`)**
  * **Invariant checker** (`fn check_anf_invariants(module: &AnfModule)`): walk `AnfExpr` and assert:
    * All `ACall` args are `Atom` (no nested expressions).
    * All `ARecord` field values are `Atom`.
    * All `AVariant` args are `Atom`.
    * All `ABinOp`/`AUnOp` operands are `Atom`.
    * `Let` body is never immediately another `Let` wrapping the same op (no redundant nesting).
  * Run the invariant checker on every `tests/run/*.tw` program as part of `cargo test`.
  * **Golden snapshot tests**: pick a handful of simple programs (e.g. `hello.tw`, `arithmetic.tw`,
    `closures.tw`, `records.tw`) and snapshot their `twk lower-anf` output; fail on diff.

---

### Stage 7.5 — Dataflow Analysis & ANF Optimization ✅

**Goal:** Introduce a dataflow-aware optimization pass over ANF IR — computing use-def
information and applying peephole rewrites — to reduce redundant computation before WAT
emission. Also provides liveness-based last-use proof for safe functional-update annotation,
consumed by the Stage 8 WAT backend.

**Scope decision:**

* Optimizations operate directly on ANF IR. No separate CFG IR is introduced.
* **Why no flat basic-block CFG:** WAT uses structured control flow (`block`/`loop`/`if`),
  not arbitrary jumps. A flat CFG would require a re-structuring pass before WAT emission,
  making it wasted work for this target. ANF's `AIf`/`ALoop`/`AMatch` structure already maps
  directly to WAT constructs. Dataflow analysis (use counting, liveness) is equally expressible
  as a tree-walk over structured ANF — no flattening needed.
* The same reasoning applies to `defer` (Stage 7.6): defer elimination can be implemented as
  an ANF tree-walk pass that threads scope-aware defer lists, rather than CFG edge insertion.
  See Stage 7.6 for details.
* CFG construction is deferred indefinitely; if advanced whole-function analysis is ever needed
  (e.g. alias analysis for array/dict in-place rewriting), it can be added on top of ANF at
  that point.
* The Core IR interpreter is unchanged. Semantic correctness is validated by structural invariant
  checks (ANF invariants still hold post-optimization) and by formal argument per rewrite rule;
  runtime differential testing awaits the Stage 8 WAT backend.
* Functional-update annotation (Step C) only sets flags on ANF nodes; no evaluation semantics
  change in Stage 7.5. The WAT backend reads the flags.

**Pipeline addition:**

```text
Core IR → ANF IR → [Stage 7.5] optimized ANF IR → WAT/Wasm backend
```

**Optimization passes (concrete rules):**

**A — Use counting**
Walk `AnfExpr` recursively and count `ALocal(id)` appearances in *operand* position.
Exclude `Let.local` (the binding site, not a use) and `AAssign.local` (a write target, not a read).
Include everything else: atoms in `ABinOp`, `ACall.args`, `ACall.callee`, `ARecordGet.target`, etc.
Result: `HashMap<LocalId, usize>` (absent key = 0 uses).

**B — Pure-op predicate**
`is_pure(op) = true` for: `AInit`, `ABinOp`, `AUnOp`, `ARecord`, `ARecordGet`, `ARecordUpdate`,
`AVariant`, `AArrayLit`, `AMakeClosure`.
`is_pure(op) = false` for: `ACall` (may I/O or trap), `AAssign` (mutates state),
`AIf`/`AMatch`/`ALoop` (contain arbitrary sub-expressions; treated conservatively).

**C — Dead let elimination (DLE)**
```
Let(t, pure_op, body)   where   uses[t] == 0   →   body
```
Pure lets with no uses are dropped. Repeated until stable.

**D — Literal copy propagation**
```
Let(t, AInit(atom), body)
  where  atom ∈ { ALitInt, ALitFloat, ALitBool, ALitStr, ALitVoid, AGlobalFunc }
  and    uses[t] <= 1
  →  body with every ALocal(t) replaced by atom
```
Only non-local atoms are propagated; `ALocal(u)` atoms are not, because `u` could be
reassigned between the init and the single use. Literals are always safe regardless of
intermediate mutations. After substitution the let becomes dead; DLE eliminates it next round.

**E — Constant folding**
```
Let(t, ABinOp(op, ALitInt(a),   ALitInt(b)),   body)  →  Let(t, AInit(ALitInt(eval(op,a,b))),   body)
Let(t, ABinOp(op, ALitFloat(a), ALitFloat(b)), body)  →  Let(t, AInit(ALitFloat(eval(op,a,b))), body)
Let(t, AUnOp(Not, ALitBool(b)),                body)  →  Let(t, AInit(ALitBool(!b)),             body)
Let(t, AUnOp(Neg, ALitInt(a)),                 body)  →  Let(t, AInit(ALitInt(-a)),              body)
```
Integer division / modulo by zero literals: leave as-is (runtime trap is intentional).
After folding to `AInit`, copy propagation eliminates `t` in the next round.

**F — Branch simplification**
```
Let(t, AIf { cond: ALitBool(true),  then_branch, _ }, body)
  →  splice then_branch: if it ends Atom(a), rewrite to Let(t, AInit(a), body)
Let(t, AIf { cond: ALitBool(false), _, else_branch }, body)
  →  same for else_branch
```
After splicing, copy propagation eliminates the `Let(t, AInit(a), ...)` wrapper.

**Fixed-point iteration:** Repeat: count-uses → DLE → copy-prop → constant-fold →
branch-simplify → until no change (or max 10 rounds).

**Liveness analysis:**
Backward walk computing `live(body)` = set of locals that may be read at or after each point.

```
live(Atom(ALocal(t)))          = {t}
live(Atom(_non-local_))        = {}
live(Return(Some(ALocal(t))))  = {t}
live(Return(_) | Break(_) | Continue)  = {}
live(Let(t, op, body))         = (live(body) \ {t}) ∪ locals_in_atoms(op)
```

For ops containing sub-expressions (`AIf`, `AMatch`, `ALoop`), `locals_in_atoms` includes
live sets of those sub-expressions unioned together (conservative).

**Functional-update annotation:**
For `Let(t, ARecordUpdate { base: ALocal(r), field, value }, body)`:
- If `r ∉ live(body)` → set `can_reuse_in_place = true` on the `ARecordUpdate` node.
- Meaning: the record referenced by `r` has no further observable readers; the WAT backend
  may emit `struct.set` (in-place mutation) instead of allocating a new struct.

Safety invariants preserved:
- No observable alias: `r` is dead in `body`, so no later code can observe the pre-update value.
- Evaluation order unchanged: the update expression is still evaluated; only the allocation strategy changes.
- Trap behavior unchanged: out-of-bounds struct field access traps identically either way.

**IR change:** Add `can_reuse_in_place: bool` to `AnfOp::ARecordUpdate`. Default `false`.
Set `true` only by the liveness pass.

Deliverables:

* `twk opt file.tw` prints optimized ANF IR; `--show-original` flag also prints the unoptimized form.
* `tests/opt_test.rs`:
  * ANF invariants hold on the optimized module for every `tests/run/*.tw` program.
  * Node-count reduction: a dedicated `tests/opt/constant_folding.tw` fixture with compile-time
    constants produces fewer `Let` nodes after optimization.
  * Golden snapshot tests for `tests/opt/constant_folding.tw` and `tests/opt/dead_let.tw`.
  * Liveness annotation tests: `tests/opt/record_in_place.tw` (base local dies at update → annotated
    `can_reuse_in_place = true`); `tests/opt/record_aliased.tw` (base reused after → `false`).

**Execution checklist (file/module map):**

* **Step A — Module skeleton + use counting (`src/opt/`)**
  * `src/opt/mod.rs`:
    * `pub mod use_count; pub mod passes; pub mod liveness; pub mod pipeline;`
    * Re-export `pipeline::optimize_module` for use by CLI and future WAT backend.
  * `src/opt/use_count.rs`:
    * `pub fn count_uses(body: &AnfExpr) -> HashMap<LocalId, usize>` — recursive walk;
      count `ALocal(id)` in all atom-position fields; skip `Let.local` binder and
      `AAssign.local` target.
    * `pub fn is_pure(op: &AnfOp) -> bool` — pure set as specified above.
    * `fn locals_in_op(op: &AnfOp) -> Vec<LocalId>` — private helper returning all `ALocal`
      references in operand positions of `op` (used by liveness).

* **Step B — Peephole passes (`src/opt/passes.rs`)**
  * `pub fn dead_let_elim(body: AnfExpr, uses: &HashMap<LocalId, usize>) -> (AnfExpr, bool)` —
    walk `AnfExpr`; on `Let(t, pure_op, inner)` where `uses.get(&t) == None or 0`, return
    `(inner, changed=true)`; recurse into sub-expressions of other nodes.
  * `pub fn copy_propagate(body: AnfExpr, uses: &HashMap<LocalId, usize>) -> (AnfExpr, bool)` —
    on `Let(t, AInit(lit), inner)` where `lit` is non-local and `uses[t] <= 1`, call
    `subst_atom(inner, t, lit)` and return `(result, true)`.
  * `pub fn constant_fold(body: AnfExpr) -> (AnfExpr, bool)` — on `Let(t, ABinOp/AUnOp
    with literal atoms, inner)`, compute the result literal, rewrite to `Let(t, AInit(result), inner)`.
  * `pub fn branch_simplify(body: AnfExpr) -> (AnfExpr, bool)` — on `Let(t, AIf(ALitBool(b),
    then_e, else_e), inner)`, select the known branch and splice it into `inner`.
  * `fn subst_atom(body: AnfExpr, target: LocalId, replacement: Atom) -> AnfExpr` — recursive
    substitution of `ALocal(target)` → `replacement` everywhere in `body`. Only called with
    non-local `replacement` atoms, so mutation-safety is not a concern.

* **Step C — Liveness + in-place annotation (`src/opt/liveness.rs`, `src/ir/anf.rs`)**
  * `src/ir/anf.rs`:
    * Add `can_reuse_in_place: bool` field to `AnfOp::ARecordUpdate` (default `false`).
    * Update `Display` impl for `ARecordUpdate` to show `[in-place]` when set.
  * `src/opt/liveness.rs`:
    * `pub fn live_after(body: &AnfExpr) -> HashSet<LocalId>` — backward liveness walk;
      returns the set of locals live at the *entry* of `body`.
    * `pub fn annotate_in_place(func: &mut AnfFunctionDef)` — walk the function body;
      at each `Let(t, ARecordUpdate { base: ALocal(r), .. }, inner)`, call `live_after(inner)`;
      if `r` is absent from the live set, set `can_reuse_in_place = true`.

* **Step D — Pipeline driver + CLI + tests**
  * `src/opt/pipeline.rs`:
    * `pub fn optimize_func(func: AnfFunctionDef) -> AnfFunctionDef` — fixed-point loop
      (max 10 rounds): count-uses → DLE → copy-prop → constant-fold → branch-simplify
      → repeat if changed; then call `annotate_in_place`.
    * `pub fn optimize_module(module: AnfModule) -> AnfModule` — map `optimize_func` over
      all functions.
  * `src/cli/opt.rs`:
    * `pub fn cmd_opt(path: &Path, show_original: bool) -> anyhow::Result<()>` — full pipeline
      through `lower_anf::lower_module`; optionally print original ANF; then `optimize_module`;
      print optimized ANF.
  * Wire `twk opt <file> [--show-original]` in `src/cli/mod.rs` and `src/main.rs`.
  * `tests/opt_test.rs`:
    * Invariant tests: for each `tests/run/*.tw`, lower to ANF, optimize, run
      `check_anf_invariants` — must pass (reuse the checker from `anf_test.rs`).
    * Node-count tests: `tests/opt/constant_folding.tw` fixture; count `Let` nodes before
      and after — optimized count must be strictly smaller.
    * Snapshot tests: golden ANF output for `tests/opt/constant_folding.tw` and
      `tests/opt/dead_let.tw`.
    * Liveness annotation tests: `tests/opt/record_in_place.tw` asserts at least one
      `can_reuse_in_place = true`; `tests/opt/record_aliased.tw` asserts none.

---

### ✅ Stage 7.6 — Defer

**Goal:** Implement `defer` end-to-end: interpreter execution and ANF-level elimination,
leaving no `Defer` nodes for the WAT backend.

> **Full design:** See [docs/defer.md](defer.md).

`defer expr` is a block-scoped statement that schedules an expression to run when the
enclosing block exits. Semantics: LIFO ordering, capture-by-value, triggers on normal
exit / `return` / `break` / `continue` / `try`-propagated `Err`, does **not** trigger on traps.

**Why no CFG for defer:** defer elimination is naturally a structured-scope problem. Since
ANF already encodes scope structure via nested `Let`-chains, and WAT requires structured
control flow anyway, an ANF tree-walk pass with scope-aware defer lists is sufficient and
simpler than CFG edge insertion. The tree structure *is* the scope structure.

**ANF defer elimination — scope threading:**

The elimination pass walks `AnfExpr` recursively, threading two lists:

* `fn_defers` — defers active between the current point and the enclosing function boundary;
  these run on `Return`.
* `loop_defers` — defers active within the current loop iteration; these run on `Break` and
  `Continue` (which exit only the current loop, not the function).

Rewrite rules:

```
Let(_, ADefer(d), body)        →  eliminate_defers(body, fn_defers=[..d], loop_defers=[..d])
Let(t, ALoop { body }, rest)   →  ALoop body' where body' = eliminate_defers(body,
                                       fn_defers=fn_defers++loop_defers, loop_defers=[])
                                   then eliminate_defers(rest, fn_defers, loop_defers)
Return(v)                      →  prepend (fn_defers ++ loop_defers) LIFO, then Return(v)
Break(v)                       →  prepend loop_defers LIFO, then Break(v)
Continue                       →  prepend loop_defers LIFO, then Continue
Atom(a) at end of deferred scope →  prepend own-scope defers LIFO, then Atom(a)
```

The nested-loop case works correctly: entering `ALoop` folds the current `loop_defers` into
`fn_defers` (so inner `Return` still unwinds outer defers) and resets `loop_defers` to empty
(so inner `Break`/`Continue` do not run outer loop's defers).

**Work items:**

* **Grammar & parser** — `defer` keyword and `defer expr` statement form (already in grammar
  from tree-sitter work; verify parser handles it).
* **AST** — `StmtKind::Defer(ExprId)`.
* **Type checker** — type-check the deferred expression in the current scope; result type
  is discarded. Any expression type is accepted — function calls, block expressions, etc.
  Expressions with type `Never` (i.e. those that diverge: `return`, `break`, `continue`,
  `error(...)`) are rejected at the type-check level, because a defer body that itself
  performs a non-local exit would silently swallow the surrounding control flow and is
  almost certainly a bug.
* **Core IR** — `CoreExprKind::Defer(ExprId)` as an opaque pass-through node; lowerer emits it
  directly without desugaring.
* **Interpreter** — maintain a defer stack (a `Vec<Vec<CoreExpr>>`) alongside the eval frame;
  push a new scope on block entry, drain LIFO on any `Signal` except `Trap`.
* **ANF IR** — add `AnfOp::ADefer(Box<AnfExpr>)` to preserve deferred expressions through
  linearization; `lower_anf` emits it as-is.
* **ANF elimination pass** (`src/opt/defer_elim.rs`) — `pub fn eliminate_defers(func: AnfFunctionDef)
  -> AnfFunctionDef`: tree-walk as described above; after this pass no `ADefer` ops remain;
  run as the final step in `optimize_module` (after all peephole passes).

**Deliverables:**

* `defer` works correctly in `twk run` (interpreter path).
* ANF elimination pass removes all `ADefer` nodes; WAT backend sees no `Defer` nodes.
* Tests covering:
  * Basic LIFO ordering within a block.
  * `return` unwinds all active defer scopes (function-level).
  * `break` / `continue` unwind only the current loop's defer scope.
  * Nested loops: inner `break` does not run outer loop's defers.
  * `try`-propagated `Err` triggers defers (same as return).
  * Trap does not trigger defers.
  * Capture-by-value at declaration time.

---

### Stage 8 — Wasm GC Backend & Runtime

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
self-contained WAT file. `wat2wasm` (or the `wasm-tools` crate) produces the final `.wasm`.

**Distribution shape:** `twc.wasm` is the canonical compiler artifact. The Rust host (Wasmtime)
is a replaceable shell. Browser and npm hosting are natural future extensions — they implement
the same host import interface. No architecture decisions should assume the Wasmtime host is
permanent.

---

#### 8a — Runtime IR + Linker (`src/wasm/`) ✅

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

#### 8b — Runtime modules (`src/runtime/`) ✅

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

#### 8c — ANF → WAT Emitter (`src/codegen/`)

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
> [Stage 9.5](#stage-95--monomorphization) for details.

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

**Step 1 — Scaffold** (`prelude.rs`, `ctx.rs`, `mod.rs`) ← next

* `prelude.rs`: `PreludeMap` — `HashMap<FuncId, PreludeEntry>` where each entry has the
  runtime `FuncSym`, param types, result type. Covers all 35 prelude FuncIds.
* `ctx.rs`: `EmitCtx` struct — `local_map: HashMap<LocalId, (u32, ValType)>` (Wasm local index
  + type), `label_stack: Vec<(Label, Label)>` (break/continue label pairs),
  `imports: BTreeSet<ImportDef>`, `type_env: &TypeEnv`, `prelude: &PreludeMap`.
* `EmitCtx::setup_locals(func: &AnfFunctionDef)` — scans body for all `Let`-bound LocalIds,
  assigns contiguous Wasm local indices after params, infers `ValType` from usage context.
* Helper: `fn mono_to_valtype(ty: &MonoType) -> ValType` — central mapping function.

**Step 2 — Atoms + literals** (`emit.rs`)

* `emit_atom(atom, expected_ty, ctx)` → `Vec<Instr>`:
  * `ALocal(id)` → `LocalGet(idx)`, with box/unbox if local type ≠ expected type.
  * `AGlobalFunc(id)` → `RefFunc` + `StructNew $Closure` with null env (wraps in trampoline).
  * `ALitInt(n)` → `I64Const(n)`.
  * `ALitFloat(v)` → `F64Const(v)`.
  * `ALitBool(b)` → `I32Const(b as i32)`.
  * `ALitStr(s)` → `ArrayNewFixed $String` with UTF-8 bytes.
  * `ALitVoid` → (nothing, or `I32Const(0)` if a value is needed).

**Step 3 — BinOp, UnOp, If**

* `ABinOp` — emit left + right (both typed), apply `i64.add`/`f64.mul`/`i32.eq`/etc.
  Comparison ops that cross types (e.g. `==` on strings) → `call rt_str__eq`.
* `AUnOp` — `Negate` → `i64.const 0; i64.sub` or `f64.neg`; `Not` → `i32.eqz`.
* `AIf` → `If { result: Some(valtype), then_body, else_body }`.

**Step 4 — Direct calls + prelude calls**

* User-to-user direct call: push typed args, `call $func_N`.
* Prelude call: look up `PreludeEntry`, convert each arg from Twinkle type to runtime
  expected type (e.g. `i64` → `struct.new $BoxedInt` if runtime expects `anyref`), emit
  `call $rt_sym`, convert result back.
* Register each used runtime func as an import in `EmitCtx`.

**Step 5 — Closure calls + AMakeClosure**

* `AMakeClosure` → generate trampoline `$func_N__closure` if not yet emitted; box free vars
  into `$ClosureEnv`, `ref.func $func_N__closure`, `struct.new $Closure`.
* Closure call → cast to `$Closure`, box args into `$Array`, extract env + func_ref,
  `call_ref $ClosureFunc`, unbox result.

**Step 6 — Records, variants, arrays**

* `ARecord` → box fields, `struct.new $UserRecord_N`.
* `ARecordGet` → cast, `struct.get`, unbox.
* `ARecordUpdate` → in-place `struct.set` or copy-and-update.
* `AVariant` → box args into `$Array`, `struct.new $Variant`.
* `AArrayLit` → box elements, `array.new_fixed $Array`.
* `AIndex` → `call rt.arr.get` / `call rt.dict.get`, unbox result.

**Step 7 — Loops, break, continue**

* `ALoop` → `Block { label: $break_N } + Loop { label: $cont_N, body }`.
* `Break` → `Br($break_N)`; `Continue` → `Br($cont_N)`.
* Push/pop label pairs on `EmitCtx.label_stack`.

**Step 8 — Pattern matching**

* `AMatch` → `Block` per arm. Cast scrutinee to `$Variant`. For each arm:
  extract `struct.get $Variant 0` (type_id) and `struct.get $Variant 1` (variant_id),
  compare with `i32.eq` + `br_if` on mismatch.
  Bind payload fields: `struct.get $Variant 2` (payload array), `array.get` each slot,
  unbox to typed locals.
  Literal patterns: compare constants.
  Wildcard `_`: fallthrough.

**Step 9 — Build pipeline + CLI** (overlaps with 8d)

* Wire `emit_user_module` into the compilation pipeline.
* Snapshot tests: compile `hello.tw`, `arithmetic.tw`, `records.tw` to WAT, assert valid
  output and no link errors.

---

#### 8d — Full build pipeline

Wire the complete pipeline in `src/cli/build.rs`:

1. Parse → resolve → typecheck → lower (Core IR) → [monomorphize (Stage 9.5)] → lower (ANF) → optimize → defer-eliminate.
2. `emit_user_module(anf, types)` → user `ModuleIR`.
3. Load runtime modules from `runtime/`.
4. `link([runtime_modules..., user_module], manifest)` → `LinkedModuleIR`.
5. `emit_wat(linked)` → write `output.wat`.
6. Shell out to `wasm-tools` (or the `wat` crate) to assemble `output.wasm`.

**Host import interface** (what the linked module imports from `"host"`):

* `host.print(s: ref $String)` — write to stdout, no newline.
* `host.println(s: ref $String)` — write to stdout with newline.
* `host.error(s: ref $String)` — write to stderr and trap (does not return).

File I/O host imports (used by `@fs`; absent in programs that don't use it):

* `host.read_file(path: ref $String) -> ref $String`
* `host.write_file(path: ref $String, content: ref $String)`
* `host.write_bytes(path: ref $String, bytes: ref $Array)`
* `host.mkdirp(path: ref $String)`
* `host.list_dir(path: ref $String) -> ref $Array`
* `host.exists(path: ref $String) -> i32`

CLI:

```bash
twk build file.tw [-o output.wasm] [--emit-wat]
```

Deliverables:

* `twk build hello.tw` produces a runnable `hello.wasm`.
* `twk runtime-dump --wat` emits the linked runtime for inspection.
* Golden snapshot tests: a handful of programs (e.g. `hello.tw`, `arithmetic.tw`, `records.tw`)
  have their WAT output snapshotted and fail on regression.
* All runtime functions unit-tested via Wasmtime test harness.

---

#### 8e — Standard library (`stdlib/`)

New directory `stdlib/` containing Twinkle source files for the MVP standard library modules.
These are compiled via the same Wasm GC backend pipeline as user programs and linked into
`twc.wasm` alongside the runtime. See [docs/stdlib.md](stdlib.md) for the full API spec.

**`stdlib/path.tw` (`@path`)** — pure Twinkle, no host imports:

* `join`, `join_all`, `dirname`, `basename`, `stem`, `extension`, `normalize`, `is_absolute`.
* Testable via the Core IR interpreter immediately (no Wasm backend needed).

**`stdlib/fs.tw` (`@fs`)** — thin wrapper over host file I/O imports:

* `FsError` sum type: `{ NotFound, PermissionDenied, Other(String) }`.
* `DirEntry` record and `EntryKind` sum type.
* `read_text`, `write_text`, `write_bytes`, `mkdirp`, `list_dir`, `exists`.
* Calls `host.read_file`, `host.write_file`, `host.write_bytes`, `host.mkdirp`,
  `host.list_dir` — the same host imports declared in 8d.

**Module loader fix:** Update the module loader (`src/module/loader.rs`) to resolve `@name`
imports to the corresponding embedded stdlib `ModuleIR` rather than returning "not yet
implemented". Stdlib modules are registered at startup alongside the runtime modules.

**Link step update:** The build pipeline from 8d gains stdlib modules in the link:

```
link([runtime_modules..., stdlib_modules..., user_module], manifest)
```

When building `twc.wasm` itself, all stdlib modules are linked in unconditionally (the
compiler must carry the full stdlib to embed it for user program builds). When `twc.wasm`
compiles a user program and produces `output.wasm`, only stdlib modules actually imported
by that user program are included — dead-module elimination at the linker level keeps
user output small.

**Wasm 3.0 — JS String Builtins:** The `runtime/str.rs` module (`rt.str`) uses `$String (array
i8)` backed by runtime functions today. When running `twc.wasm` in a browser or npm (JS) host,
Wasm 3.0 JS String Builtins can replace the `rt.str` implementation with native JS string
operations — giving free concatenation, slicing, and comparison without UTF-8 encode/decode
at the boundary. Design `runtime/str.rs` with a clean interface seam: the exported function
symbols stay identical; a `--host=js` link-time flag swaps in a `runtime/str_js.rs` module
that emits extern-ref JS string calls instead of `array<i8>` operations. The compiler emitter
is unaffected — it calls `rt.str.*` symbolically regardless of which implementation is linked.

Deliverables:

* `use @path` and `use @fs` resolve and compile end-to-end.
* `@path` functions tested via existing interpreter test harness (`tests/run/`).
* `@fs` functions tested via Wasmtime test harness with a temporary directory fixture.

---

### Stage 9 — Host Integration & Validation

**Goal:** Implement the Wasmtime host shell that satisfies the runtime's host import interface,
run compiled programs end-to-end via Wasm, and validate correctness against the interpreter
via differential testing.

**Host shell design:**

The host is a thin Rust + Wasmtime layer. It is *not* the compiler — it merely provides
the host import functions (`host.print`, `host.println`, `host.error`) and instantiates the
linked Wasm module. The compiler pipeline remains in Rust at this stage, but the host interface
is deliberately minimal so any other host (Node, browser shim) can implement it identically.

WASI is a host concern: the Wasmtime host implements `host.read_file`, `host.write_file`,
`host.write_bytes`, `host.mkdirp`, `host.list_dir`, `host.exists` using WASI or native
calls. `twc.wasm` imports file I/O abstractly — it is not aware of WASI. This keeps
`twc.wasm` host-agnostic.

At Stage 9, the host shell only needs console imports (`host.print`, `host.println`,
`host.error`) since the compiler pipeline is still in Rust. File I/O imports become live
in Stage 10 when `twc.wasm` itself reads source files.

CLI:

```bash
twk run file.tw                  # interpreter backend (unchanged)
twk run --backend=wasm file.tw   # compile → link → run via Wasmtime
twk build file.tw -o output.wasm # compile + link only
```

**Differential testing (`tests/wasm_test.rs`):**

For every program in `tests/run/`:

1. Run via interpreter → capture stdout.
2. Compile + link + run via Wasmtime → capture stdout.
3. Assert outputs are identical.

Any divergence is a regression in the WAT emitter or runtime. The interpreter remains the
reference semantic oracle.

**Wasm 3.0 — Tail Calls:** The WAT emitter should emit `return_call $f` / `return_call_ref
$ClosureFunc` for calls in tail position. Tail calls matter for:

* The recursive-descent parser in the self-hosted compiler (Stage 10).
* Mutually-recursive functions that otherwise hit Wasm's call stack limit on large inputs.

The `Instr::ReturnCall` and `Instr::ReturnCallRef` variants in `src/wasm/ir.rs` are available
for the emitter to use. Identify tail-position calls in ANF IR (a `Return(ACall(...))` pattern)
and emit the tail-call form. This is a safety gate for Stage 10 correctness.

Deliverables:

* All `tests/run/*.tw` programs produce correct output via `--backend=wasm`.
* Differential test suite passing.
* Host interface documented: the exact set of imports `twc.wasm` requires from the host,
  their types, and their observable behavior. This is the stability boundary for future hosts.
* Tail-position calls emitted as `return_call` / `return_call_ref`; verified on a
  deeply-recursive test program (e.g. Fibonacci with large N).

---

### Stage 9.5 — Monomorphization

**Goal:** Eliminate all type-variable boxing by specializing generic functions at each unique
instantiation. After this pass, no `MonoType::Var` survives into ANF or codegen — every
function has fully concrete typed params and locals.

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
trampoline generator uses concrete types. The `anyref` row in the value representation table
is dead code.

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

---

### Stage 10 — Self-Hosted Compiler

**Goal:** Re-implement the compiler pipeline in Twinkle, use the stage0 Rust compiler to
compile it to `twc.wasm`, then run and verify the Twinkle-hosted compiler.

**Bootstrapping sequence:**

1. Write the compiler in Twinkle under `compiler/` (lexer, parser, type checker, Core IR
   lowering, ANF lowering, optimizer, WAT emitter, Runtime IR + linker).
2. Stage0 Rust: `twk build compiler/main.tw -o twc.wasm`.
3. Verify: run `twc.wasm` under Wasmtime on `hello.tw`; output must match stage0 output.
4. Self-hosting round: compile `compiler/main.tw` with `twc.wasm` → new `twc.wasm`; verify
   the two are behaviorally equivalent on the compatibility suite.

**Prerequisites:** The Twinkle language must be expressive enough to write a compiler.
File I/O (reading source files) is provided by the host via WASI or a custom import — the
compiler sources import it as an abstract interface. String manipulation, arrays, and dicts
(already in the runtime) are sufficient for symbol tables and AST representations.

**Porting note:** The Runtime IR + Linker (`src/wasm/`) is implemented in Rust for stage0 but
must be ported to Twinkle for self-hosting. It is the largest self-hosting prerequisite beyond
the compiler pipeline itself.

**Compatibility suite:**

A set of `.tw` programs compiled by both stage0 (Rust) and stage1 (Twinkle self-hosted);
outputs (Wasm execution results) must be identical.

Deliverables:

* `twc.wasm` produced by stage0 can compile real Twinkle programs.
* `twc.wasm` produced by itself compiles the same programs to equivalent results.
* Stage0 Rust implementation frozen as a reference and bootstrap tool.

---

### Later Stages — Tooling & Ecosystem

> **Full design:** See [docs/tooling.md](tooling.md).

**Prerequisites before tooling:**

* **Lossless lexer**: comments preserved as trivia tokens (required by formatter and LSP).
* **Parser error recovery**: partial AST on syntax errors (required by LSP; nice-to-have for formatter).

**Practical goals for "easy tooling":**

* **Whole-program formatting is easy to run**
  * `twk fmt --all` discovers project files from root and formats all `.tw` files deterministically;
  * formatting is idempotent (`fmt` twice yields no diff);
  * on syntax error, command reports file + span and continues other files (non-zero exit at end).

* **Incremental LSP is easy to keep fast**
  * on file change, re-run parse/resolve/typecheck only for the changed file and affected reverse-dependents;
  * unchanged modules are served from stage caches;
  * diagnostics/hover/go-to-definition read query artifacts directly (no full lower/link requirement).

**Milestones to reach those goals:**

* **T1 — Formatter core**
  * finish lossless lexer trivia model and formatter AST printer;
  * add `twk fmt <file>` with golden tests + idempotence tests.

* **T2 — Whole-project formatter UX**
  * add `twk fmt --all` file discovery, include/exclude rules, and stable output ordering;
  * add CI-friendly exit codes and summary reporting.

* **T3 — Incremental diagnostics for LSP**
  * complete Stage 6b query-cache work;
  * add dependency graph invalidation + reverse-dependency tracking;
  * expose diagnostics query endpoint reused by CLI and LSP host.

* **T4 — Interactive LSP features**
  * add hover / go-to-definition / completion on top of cached query artifacts;
  * add latency benchmarks with warm cache and edit-loop workloads.

**Tooling implementation map (files):**

* **Formatter core (`T1`)**
  * `src/syntax/lexer.rs`, `src/syntax/tokens.rs`:
    * add trivia/comment preservation model required by formatter.
  * `src/syntax/parser.rs`:
    * ensure parser exposes token/trivia links needed by formatting decisions.
  * `src/syntax/pretty.rs`:
    * implement canonical formatter printer.
  * `src/cli/mod.rs`, new `src/cli/fmt.rs`, `src/main.rs`:
    * wire `twk fmt <file>` and `--check`.
  * Tests: add formatter golden/idempotence suite under `tests/` (new formatter tests file + fixtures).

* **Whole-project formatting UX (`T2`)**
  * New module recommended: `src/cli/project_files.rs` (shared project file discovery).
  * `src/cli/fmt.rs`:
    * implement `--all`, deterministic file ordering, partial-failure reporting, CI exit codes.

* **Incremental diagnostics (`T3`)**
  * Build on query cache modules from Stage 6b Step C.
  * New module recommended: `src/diagnostics/mod.rs` for normalized diagnostic structures.
  * `src/cli/check.rs`:
    * consume diagnostics query API (no full lower/link unless requested).

* **LSP (`T4`)**
  * New module recommended: `src/lsp/mod.rs` (or standalone crate later).
  * Integrate with query API from Stage 6b Step D; keep lower/link out of hot path.
  * Add edit-loop latency benchmark harness (new `benches/lsp_latency.rs`).

**Planned tools** (all as `twk` subcommands initially, rewritten in Twinkle post-self-hosting):

* **`twk fmt`**: canonical formatter. Only needs parse stage + lossless lexer. No config; one official style.
* **`twk lint`**: linter with syntactic rules (parse only) and semantic rules (parse + typecheck). Key rule: warn on rebinding-without-use (the "looks like mutation but isn't" trap).
* **`twk lsp`**: language server (LSP protocol). Needs query-friendly pipeline + lossless lexer + error recovery. Initial features: diagnostics, hover types, go-to-definition, completion.
* **Standard library** in Twinkle (collections, JSON, I/O via WASI).
* **Package manager**, **test runner**, **doc generator**, **build system**.

These tools are separate concerns and plug into the pipeline via the existing compiler API (parse, typecheck, IR, codegen).
