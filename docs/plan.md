# Twinkle Implementation Plan

## Goal

Build a self-hosting Twinkle compiler that ultimately runs as a single WebAssembly module, with:

* A small Rust **stage0** implementation.
* A clear internal pipeline:

  * Source → AST → Typed AST → Core IR → (ANF) → backends.
* An **interpreter-first** path for fast iteration.
* A later **WAT/Wasm backend** for distribution and self-hosting.

---

## High-Level Architecture

Compiler pipeline:

```text
Twinkle source
  → Lexer
  → Parser (AST with spans)
  → Typechecker (bidirectional)
  → Core IR (expression+block, loops, match, variants)
  → ANF IR (optional, backend-oriented)
  → Backend(s):
       - Core IR Interpreter (stage0)
       - WAT / Wasm GC backend (later)
```

Runtime / distribution:

* In development:

  * `twk` (Rust binary) with:

    * `twk parse`, `twk check`, `twk run`, `twk build`, `twk fmt`, `twk lint`, `twk lsp`.
  * `run` uses the interpreter backend.
* Later:

  * `twk build` emits `.wat` (and/or `.wasm`).
  * A small host wrapper (Rust/Node/etc.) runs the Wasm compiler.
  * Self-hosted compiler written in Twinkle.

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

### Stage 7 — ANF IR (Backend-Oriented, Optional) ⬅ Next

**Goal:** Add an ANF representation for backend use.

Ordering note:

* Keep ANF at this stage (after interpreter + generics), not before Stage 5.
* This keeps execution semantics anchored by Core IR first, then introduces
  backend-oriented normalization.

ANF IR:

* Same semantics as Core IR, but:

  * every intermediate expression is bound to a temporary via `let`,
  * function calls take only variables as arguments,
  * evaluation order is explicit and linear.

Core → ANF lowering:

* Walk Core IR and:

  * introduce temporaries for non-trivial subexpressions,
  * linearize nested structure into let-chains.

Usage:

* Interpreter can continue using Core IR.
* ANF IR is used for codegen (WAT/Wasm), if it simplifies backend logic.

Deliverables:

* `twk lower-anf file.tw` prints ANF IR.
* Tests:

  * check that ANF preserves behavior (e.g. interpret Core IR vs ANF IR and compare results on small programs, if you choose to interpret ANF too).

---

### Stage 7.5 — Mid-End CFG & Optimization (Recommended)

**Goal:** Introduce a control-flow-aware optimization layer after ANF, while preserving Twinkle's immutable language semantics.

Pipeline:

* `Core IR → ANF IR → CFG (optionally SSA) → optimized ANF/CFG → backend`.

Initial passes (safe + high value):

* Constant folding / algebraic simplification.
* Copy propagation.
* Dead code elimination.
* Branch simplification.
* Loop-invariant code motion (conservative).

Functional-update optimization (key feature):

* Rewrite persistent update patterns to in-place backend ops where provably safe:
  * `RecordUpdate(...)`
  * `Array.set(...)`
  * `Dict.set/remove(...)`
* This is an implementation optimization only; user-visible semantics remain immutable/value-based.

Safety gates for destructive rewrite:

* No live aliases to the pre-update value at the rewrite point.
* No later observable use of the pre-update value.
* Evaluation order and trap behavior remain unchanged.
* If proof fails, keep the original persistent operation.

SSA strategy:

* Start with CFG + dataflow (liveness/use-count/escape summaries).
* Add SSA form if/when it simplifies global optimization and codegen.
* Treat SSA as an internal optimization representation, not a user-visible IR contract.

Deliverables:

* `twk opt file.tw` (or equivalent debug mode) can dump pre/post optimization IR.
* Differential tests: optimized vs unoptimized execution must match interpreter semantics.
* Targeted tests for update-rewrite safety (aliases, closures, branch joins, loop-carried values).

---

### Stage 8 — WAT Backend

**Goal:** Compile Twinkle programs to human-readable WAT (WebAssembly text format).

Backend:

* Consume optimized ANF/CFG output (with fallback path from plain ANF during migration).
* Emit `.wat` with:

  * type section,
  * function definitions,
  * imports/exports,
  * local variables,
  * control structures:

    * `if`, `block`, `loop`, `br`, `br_if`.

Representation of Twinkle types in Wasm:

* Start simple:

  * map primitives (`Int`, `Bool`, `Float`) to numeric Wasm types (spec §2, §21).
  * map `Str`, `Arr`, `Dict`, `Record`, `Variant` to a runtime representation (e.g. indices into a linear memory managed by a small runtime).
* Later, experiment with Wasm GC (`struct`, `array`, `variant`) as the design stabilizes (spec §21).
* Entry point: top-level init sequence lowers to a Wasm start function (spec §8.1).

CLI:

```bash
twk build file.tw -o file.wat
```

At first, this is just a compilation target; executing WAT/Wasm can be done via external tools.

Deliverables:

* WAT emitted for simple programs.
* Golden tests:

  * `.tw` input → expected `.wat` pattern (or pretty-printed).

---

### Stage 9 — Wasm Execution Integration

**Goal:** Integrate a Wasm runtime (e.g. Wasmtime) so Twinkle can run via Wasm as well as via interpreter.

CLI modes:

```bash
twk run file.tw                 # default: interpreter backend
twk run --backend=wat file.tw   # compile to WAT, run via Wasmtime
twk build file.tw -o file.wat   # compile only
```

Implementation:

* `compile_to_wat(src: &str) -> String`.
* `run_wat(wat: &str)` using Wasmtime APIs (or a simple shell-out to `wasmtime` binary during development).

Interpreter remains the reference semantics; Wasm/WAT is a second backend.

Deliverables:

* Programs can run both via interpreter and via Wasm backend.
* Tests:

  * For selected programs, compare interpreter output vs Wasm execution output.

---

### Stage 10 — Self-Hosted Compiler in Twinkle

**Goal:** Re-implement the compiler pipeline in Twinkle itself.

Reimplemented components in Twinkle:

* Lexer,
* Parser,
* Typechecker/inference,
* Core IR lowering,
* (optionally ANF lowering),
* WAT backend.

Bootstrapping:

1. Use the Rust stage0 compiler to:

   * compile the Twinkle compiler sources (`compiler/*.tw`) to WAT/Wasm.
2. Run that Twinkle-written compiler under Wasmtime to:

   * compile user programs,
   * eventually compile itself.

Compatibility suite:

* A set of `.tw` inputs are compiled by:

  * Rust stage0 compiler,
  * Twinkle self-hosted compiler.
* Ensure outputs (WAT or Wasm) are identical or behavior-equivalent.

Deliverables:

* Self-hosted Twinkle compiler that can compile real programs.
* Stage0 Rust implementation can be frozen or kept as a reference.

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
