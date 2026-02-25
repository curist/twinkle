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

    * `twk parse`, `twk check`, `twk run`, `twk build`.
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

  * Prelude functions retain FuncId 1–14.
  * User functions across all imported modules are assigned FuncIds in
    deterministic order (import order, then source order within each file).

Deliverables:

* Multi-file programs compile and run through `twk lower`.
* `p.translate(1,2)` correctly desugars in Core IR output.
* Tests for: import resolution, pub/private visibility, aliasing, collision errors,
  circular import error, and inherent method calls across modules.

---

### Stage 5 — Core IR Interpreter ⬅ Next

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
  * `Arr(Vec<Value>)`, `Dict(...)`,
  * `Record(TypeId, Vec<Value>)`,
  * `Variant(TypeId, VariantId, Vec<Value>)`,
  * `Closure(FuncId, Env)`,
  * `Void`.

* Environment & stack:

  * Flat `HashMap<LocalId, Value>` per call frame (LocalIds are unique per function).
  * `Assign` mutates in place; `Let` inserts a new entry.
  * Control flow signals: `Break(Option<Value>)`, `Continue`, `Return(Option<Value>)`
    bubble up through the expression tree and are caught at `Loop` / call boundaries.

* Built-ins:

  * Prelude FuncIds 1–14 dispatched natively in `call_builtin`.
  * Full stdlib implemented as native builtins:

    * `Array`: `set(arr, i, val) Array<T>`, `concat(arr1, arr2) Array<T>`, `slice(arr, start, end) Array<T>`.
    * `Dict`: `new() Dict<K,V>`, `set(m, k, v) Dict<K,V>`, `remove(m, k) Dict<K,V>`, `get(m, k) Option<V>`, `has(m, k) Bool`, `keys(m) Array<K>`, `len(m) Int`.
    * `String`: `substring(s, start, end) String`, `of_int(n) String`, `of_float(f) String`, `of_bool(b) String`.
      (Surface names are `String.of_*`; `int_to_string`/`float_to_string`/`bool_to_string` are intrinsic aliases already registered in `ValueEnv`.)
    * `Range`: `range(n) Array<Int>` (0..n−1), `range_from(a, b) Array<Int>`, `range_step(a, b, step) Array<Int>`.
  * User functions looked up in `CoreModule.functions` by `FuncId`.

* `Dict` runtime representation decision (open):

  * `HashMap<Value, Value>` — requires `Value: Hash + Eq`; efficient but constrains `Value`.
  * `Vec<(Value, Value)>` — simpler, no constraints; fine for small dicts in stage0.
  * Recommended: start with `Vec<(Value, Value)>` and revisit if performance matters.

CLI:

```bash
twk run file.tw   # parse + typecheck + lower + interpret
```

Deliverables:

* Real multi-file Twinkle programs run end-to-end.
* Closure capture-by-value test (`tests/closure/capture_by_value.tw`) passes.
* Interpreter tests: arithmetic, if/case, loops, collect, records, variants,
  inherent method calls across modules, dict operations, lvalue assignment forms.

---

### Stage 6 — Generics & Bidirectional Type Checking

**Goal:** Upgrade typechecker to support generics and inference.

Features:

* Types:

  * type variables (`TypeVar`),
  * schemes: universally quantified types for polymorphic functions and types.

* Type checking (spec §20):

  * bidirectional Damas–Milner with:

    * unification,
    * generalization at `fn` declarations (not local bindings) (spec §20 Generalization Rules),
    * instantiation at use sites.

* Generic functions and sum/record types (spec §3):

  * `type Option<T> = { None, Some(T) }`
  * `fn map<A, B>(xs: Arr<A>, f: fn(A) B) -> Arr<B> { ... }`

Core IR does not necessarily change; it just gets richer type annotations.

Deliverables:

* `twk check` supports generic code.
* Type inference tests:

  * polymorphic functions,
  * higher-order functions,
  * errors where inference fails or constraints are violated.

---

### Stage 7 — ANF IR (Backend-Oriented, Optional)

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

Once the core compiler is stable and (preferably) self-hosted, build:

* **Formatter** (pretty-printer) in Twinkle.
* **LSP server** in Twinkle (with a thin host wrapper).
* **Standard library** in Twinkle (collections, JSON, I/O via WASI).
* **Package manager**, **test runner**, **doc generator**, **build system**, etc.

These tools are separate concerns and plug into the pipeline via the existing compiler API (parse, typecheck, IR, codegen).
