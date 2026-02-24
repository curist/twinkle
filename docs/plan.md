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
  → Typechecker (HM-style)
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
   Twinkle features (`collect`, `try`, `.Variant`, `for x in`, etc.) are desugared into a **Core IR** that directly expresses the semantics in a small set of constructs:

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
* String interpolation:

  * Lexed as alternating `STRING_SEGMENT` + `${` *Expr* `}` tokens.
* Parser:

  * Expressions with precedence (`or` < `and` < `==` < `<` < `+ -` < `* / %`).
  * Blocks `{ ... }` as expression-with-statements.
  * `if` expressions.
  * `case` expressions.
  * `for` / `collect`.
  * Function declarations (`fn name(...) [ReturnType] Block`).
  * Type declarations (records + sum types).
  * Top-level statements and expressions.

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

  * Primitive: `Int`, `Float`, `Bool`, `Str`, `Void`.
  * Records: nominal record types with fields.
  * Sum types: nominal variants (`type Result = { Ok(Int), Err(Str) }`).
  * Arrays & dicts: `Arr<T>`, `Dict<K,V>`.
  * Functions: `fn(T1, T2, ...) Tret`.

* Name resolution:

  * Module-level symbol table for:

    * `type` declarations,
    * `fn` declarations,
    * top-level values.
  * Basic support for qualified names in types and expressions (e.g. `Module.Point`).

* Typechecker:

  * Expression typechecking.
  * Let bindings:

    * `x := expr` (inferred).
    * `x: T = expr` (checked).
  * Function declarations and calls.
  * `if` expressions (branch type agreement).
  * `case` expressions:

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

  * `collect` into loops and explicit array building.
  * `for` forms (`x in xs`, `key, value in dict`, `for expr`) into `Loop`.
  * `try expr` into a `Match` over `Result` plus early-return/propagation.
  * `.Variant(...)` shorthand into `Variant { type_id, variant_id, ... }` using type info.
* Convert blocks with statements into nested `Let` chains or keep a `Block` node in Core and only eliminate it later.

Deliverables:

* `twk lower file.tw` prints Core IR.
* IR tests:

  * For small programs, assert Core IR matches expectations.
  * Spot-check desugaring of `collect`, `for`, `try`, and `.Variant`.

---

### Stage 4 — Core IR Interpreter ⬅ Next

**Goal:** Run Twinkle programs by interpreting Core IR.

Runtime representation:

* `Value` enum with variants:

  * `Int(i64)`, `Float(f64)`, `Bool(bool)`, `Str(String)`,
  * `Arr(Vec<Value>)`, `Dict(...)`,
  * `Record(TypeId, Vec<Value>)`,
  * `Variant(TypeId, VariantId, Vec<Value>)`,
  * `Closure(FuncId, Env)`,
  * `Void`.

* Environment & stack:

  * Map `LocalId → Value` within each frame,
  * Simple call stack for function calls.

Evaluator:

* Evaluate:

  * literals and locals,
  * `Let`/`Block`,
  * `If`, `Match`,
  * `Loop` with `Break`/`Continue`,
  * function calls (`Call`),
  * records/variants and field/variant access.

* Built-ins:

  * Handle a small set of built-ins in a native table:

    * `println`, `print`, `len`, maybe `error`.
  * Represent them as `Value::Builtin(BuiltinId)` and dispatch in `Call`.

CLI:

```bash
twk run file.tw   # parse + typecheck + Core IR + interpret
```

Deliverables:

* A non-trivial subset of Twinkle runs end-to-end.
* Interpreter tests:

  * Small arithmetic programs.
  * `if`/`case`.
  * simple loops, `collect`, and `try`.

---

### Stage 5 — Generics & Hindley–Milner Type Inference

**Goal:** Upgrade typechecker to support generics and inference.

Features:

* Types:

  * type variables (`TypeVar`),
  * schemes: universally quantified types for polymorphic functions and types.

* Type inference:

  * standard HM with:

    * unification,
    * generalization at let-bindings,
    * instantiation at use sites.

* Generic functions and sum/record types:

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

### Stage 6 — ANF IR (Backend-Oriented, Optional)

**Goal:** Add an ANF representation for backend use.

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

### Stage 7 — WAT Backend

**Goal:** Compile Twinkle programs to human-readable WAT (WebAssembly text format).

Backend:

* Consume Core IR or ANF IR (whichever feels cleaner).
* Emit `.wat` with:

  * type section,
  * function definitions,
  * imports/exports,
  * local variables,
  * control structures:

    * `if`, `block`, `loop`, `br`, `br_if`.

Representation of Twinkle types in Wasm:

* Start simple:

  * map primitives (`Int`, `Bool`, `Float`) to numeric Wasm types.
  * map `Str`, `Arr`, `Dict`, `Record`, `Variant` to a runtime representation (e.g. indices into a linear memory managed by a small runtime).
* Later, experiment with Wasm GC (`struct`, `array`, `variant`) as the design stabilizes.

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

### Stage 8 — Wasm Execution Integration

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

### Stage 9 — Self-Hosted Compiler in Twinkle

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

