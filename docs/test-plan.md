# Twinkle Compiler Test Plan

Comprehensive Testing Strategy for All Compiler Stages

This document defines the complete testing methodology for the Twinkle compiler ecosystem.
It is designed to:

* guarantee correctness of all compiler stages,
* ensure long-term stability of IR formats and semantics,
* support self-hosting,
* support multi-runtime (wasmtime, Node, Deno, browser),
* operate efficiently in CI.

This plan must be followed for all new features and changes.

---

# 1. Test Philosophy

Twinkle’s compiler is designed to be:

* small,
* predictable,
* deterministic,
* self-hosted.

Our testing philosophy follows from these constraints:

1. **Always test syntax + semantics + IR + execution.**
   No single-layer tests are sufficient.

2. **Golden tests are authoritative.**
   AST, Core IR, ANF, WAT output, diagnostics — all are snapshot-validated.

3. **All tests run on at least two compilers:**

   * Rust bootstrap compiler (`twinkle-bootstrap`)
   * Self-hosted compiler (`twinkle-compiler.wasm`)

4. **All produced Wasm must be identical.**
   This prevents divergent semantics between bootstrap and self-hosted builds.

5. **All executed output must match across runtimes:**

   * wasmtime
   * Node+Deno WASI backends
   * browser polyfill (future CI)

6. **Tests must be hermetic:**
   No random numbers, no time, no nondeterministic ordering, no external I/O beyond WASI-mocked FS.

---

# 2. Test Categories

Twinkle has 8 major test categories.

```
tests/
  lexer/
  parser/
  pretty/
  typecheck/
  lower/
  ir/
  codegen/
  execute/
  wasi/
  selfhost/
```

We describe them in detail below.

---

# 3. Lexer Tests

### Purpose

Validate that the lexer produces correct tokens and source spans.

### Format

```
tests/lexer/xxx.tw
tests/lexer/xxx.tokens
```

`.tokens` is a golden snapshot containing:

* token kinds
* lexemes
* spans (start/end indices)

### Examples

* integer literals
* float literals
* string escapes
* identifiers
* operators & punctuators
* comments
* error cases (unterminated string, unexpected char)

---

# 4. Parser Tests

### Structure

```
tests/parser/xxx.tw
tests/parser/xxx.ast.json
```

`.ast.json` contains:

* full AST
* spans
* no type information

### Test Subtypes

* precedence parsing
* block desugaring
* operator associativity
* if/else
* lambda syntax
* record syntax
* enum and pattern syntax
* error recovery tests (parser must produce partial AST + diagnostics)

---

# 5. Pretty-Printer Tests (Formatter Baseline)

### Purpose

Ensure parse → format → parse is idempotent.

### Rules

For each file:

```
formatted1 = format(src)
formatted2 = format(formatted1)
assert(formatted2 == formatted1)
```

### Fixtures

```
tests/pretty/original.tw
tests/pretty/original.formatted.tw
```

The `.formatted.tw` is a golden file.

---

# 6. Typechecker Tests

### Files

```
tests/type_ok/*.tw
tests/type_ok/*.typed.json

tests/type_err/*.tw
tests/type_err/*.error
```

### Type OK Tests (`type_ok`)

* Produce typed AST (`.typed.json`) including inferred types.
* Must match golden snapshot exactly.

### Type Error Tests (`type_err`)

* Exact diagnostic messages must match `.error`.
* Error location spans must match exactly.
* Tests include:

  * unification failures
  * unknown identifiers
  * invalid field access
  * non-exhaustive pattern matches
  * trait-constraint failures (`Show`, generics)
  * wrong arity

---

# 7. Lowering Tests (AST → Core IR → ANF IR)

### Files

```
tests/lower_core/*.tw
tests/lower_core/*.core.json

tests/lower_anf/*.tw
tests/lower_anf/*.anf.json
```

### Core IR Tests

Verify:

* no surface sugar remains
* implicit return is gone
* `try` desugared into match
* `collect` lowered to loop
* `for x in` lowered to explicit iterator
* all identifiers replaced by locals
* variant + record constructors resolved

### ANF Tests

Verify:

* every non-atomic subexpression is let-bound
* evaluation order is explicit
* no nested calls or nested ops remaining

---

# 8. IR Interpretation Tests

These ensure semantics of Core IR and ANF IR are correct even before Wasm backend exists.

### Files

```
tests/ir/xxx.tw
tests/ir/xxx.result
```

The test pipeline is:

```
twinkle-bootstrap parse+typecheck+lower_core xxx.tw → core.ir
run-internal-core-interpreter core.ir → result
compare result to xxx.result
```

This guarantees:

* interpreter defines semantics,
* backend must match the interpreter exactly.

---

# 9. Codegen Tests (WAT + Wasm)

### Files

```
tests/codegen/xxx.tw
tests/codegen/xxx.wat         (golden)
tests/codegen/xxx.stdout      (execution result)
```

### Requirements

1. Lowered ANF IR → WAT must match golden `.wat`.
2. Compiling `.wat` → Wasm → executing → stdout must match `.stdout`.

### Codegen Cases

* numeric ops
* boolean logic
* closures and lambda capture
* arrays, dicts, records
* variant construction & match
* loops
* try/Result
* large integer values
* branching and nested blocks

---

# 10. Execution Tests (End-to-End)

These focus on real-world semantic behaviors.

### Files

```
tests/execute/xxx.tw
tests/execute/xxx.stdout
```

Execution must match across all runtimes:

* wasmtime
* self-hosted wasm
* Node (WASI)
* Deno (WASI)

Each test runs:

```
twinkle-bootstrap compile xxx.tw → xxx.wasm
wasmtime xxx.wasm > out1

twinkle-compiler.wasm compile xxx.tw → xxx2.wasm
wasmtime xxx2.wasm > out2

assert(out1 == out2)
assert(out1 == golden)
```

---

# 11. WASI Contract Tests

These validate the Host ABI contract (file I/O, args, env, fs operations).

### Files

```
tests/wasi/*.tw
tests/wasi/*.stdout
tests/wasi/fs/   (fixture FS hierarchy)
```

### Requirements

All runtimes must produce identical results:

* wasmtime
* Node WASI
* Deno WASI
* browser polyfill

All WASI tests must be self-contained with a virtual filesystem.

---

# 12. Self-Hosting Compatibility Tests

The most important long-term stability test.

### Files

```
tests/selfhost/*.tw
tests/selfhost/*.expected.wasm
tests/selfhost/*.expected.stdout
```

### Procedure

For every test:

#### Step A — Using Bootstrap Compiler

```
bootstrap_compile test.tw → out1.wasm
```

#### Step B — Using Self-Hosted Compiler

```
selfhost_compile test.tw → out2.wasm
```

#### Step C — Output Comparison

```
diff out1.wasm out2.wasm   (must be byte-for-byte identical)
```

#### Step D — Execution Comparison

```
wasmtime out1.wasm → r1
wasmtime out2.wasm → r2
assert(r1 == r2)
assert(r1 == golden_stdout)
```

---

# 13. Performance Tests (Optional)

Benchmarks are not part of CI correctness, but tracked:

* lexer speed
* parser throughput
* type checker throughput with large files
* ANF + codegen timing
* end-to-end compile speed
* wasm execution microbenchmarks

Stored under:

```
benchmarks/
```

---

# 14. CI Testing Matrix

### Build matrix includes:

| Stage                   | Rust | Self-hosted | wasmtime | Node | Deno | Browser |
| ----------------------- | ---- | ----------- | -------- | ---- | ---- | ------- |
| Lexer tests             | ✓    | —           | —        | —    | —    | —       |
| Parser tests            | ✓    | —           | —        | —    | —    | —       |
| Typechecker             | ✓    | —           | —        | —    | —    | —       |
| IR tests                | ✓    | ✓           | —        | —    | —    | —       |
| Codegen                 | ✓    | ✓           | ✓        | —    | —    | —       |
| Execute                 | ✓    | ✓           | ✓        | ✓    | ✓    | ✓       |
| WASI                    | ✓    | ✓           | ✓        | ✓    | ✓    | ✓       |
| Self-host compatibility | ✓    | ✓           | ✓        | —    | —    | —       |

The self-host compiler must compile itself once per CI run.

---

# 15. Directory Layout

```
tests/
  lexer/
  parser/
  pretty/
  type_ok/
  type_err/
  lower_core/
  lower_anf/
  ir/
  codegen/
  execute/
  wasi/
    fs/
  selfhost/
benchmarks/
```

---

# 16. Test Tooling

We use:

* **golden snapshots** (insta for Rust bootstrap, custom harness for Twinkle)
* `.json` for AST, Core IR, ANF IR
* `.wat` for WAT golden outputs
* `.stdout` for execution outputs
* deterministic sorting rules inside pretty-printer and dict iteration

A test harness is provided:

```
cargo test            # bootstrap tests
twinkle test tests/   # self-hosted test runner
```

---

# 17. Stability Guarantees

This test suite guarantees:

1. **Surface semantics never regress**
   thanks to golden AST + Core IR + execution tests.

2. **IR formats remain backward compatible**
   any change requires updating golden tests explicitly.

3. **Self-hosted compiler is trustworthy**
   byte-for-byte identical outputs must hold at every version.

4. **Multi-runtime consistency**
   (Node/Deno/wasmtime/browser) environments behave identically.

5. **Deterministic codegen**
   no nondeterminism in wasm output.

---

# 18. How to Add a New Feature

A PR must include:

* tests for parsing,
* tests for typechecking,
* tests for IR lowering,
* tests for ANF (if applicable),
* codegen tests for new ops,
* execution tests,
* selfhost tests if it affects output.

No change is accepted without complete coverage across the stack.

---

# 19. Summary

This test plan ensures:

* correctness at every layer,
* deterministic compilation,
* stable IR semantics,
* reliable self-hosting,
* cross-platform WASI compatibility,
* safe evolution of the language.

Together with `docs/ir.md`, this establishes a robust foundation for Twinkle’s long-term health.

