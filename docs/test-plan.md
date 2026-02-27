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

Current test directories (stage 0–5 bootstrap compiler):

```
tests/
  parser/           # parser snapshot tests (.tw + insta snapshots)
  parser_errors/    # parser error cases
  typecheck/
    pass/           # programs that must type-check cleanly
    fail/           # programs that must produce specific type errors
  closure/          # closure capture-by-value semantics
  modules/          # multi-module compilation tests
  ir/               # Core IR lowering snapshots
  run/              # end-to-end interpreter run tests (expected output in leading comment)
  snapshots/        # insta snapshot storage
```

Future directories (added as later stages are implemented):

```
tests/
  lower_anf/        # ANF IR lowering (Stage 7)
  codegen/          # WAT/Wasm golden output (Stage 8)
  wasi/             # WASI contract tests (Stage 9+)
  selfhost/         # self-hosting compatibility (Stage 10)
benchmarks/
```

We describe the current categories in detail below.

---

# 3. Lexer Tests

> **Future.** No dedicated `tests/lexer/` directory yet. Lexer correctness is
> validated indirectly through parser tests and integration tests.

When added, format will be:

```
tests/lexer/xxx.tw
tests/lexer/xxx.tokens    # golden snapshot: token kinds, lexemes, spans
```

---

# 4. Parser Tests

### Structure

```
tests/parser/        # programs that must parse successfully (insta snapshots)
tests/parser_errors/ # programs that must produce specific parse errors
```

Insta snapshots capture the pretty-printed AST. Tests are driven by `tests/integration_test.rs`.

### Coverage

* operator precedence and associativity
* block expressions
* if/else, case, for, collect
* lambda and function syntax
* record and enum syntax
* string interpolation
* error cases: unexpected tokens, unterminated strings, etc.

---

# 5. Pretty-Printer Tests (Formatter Baseline)

> **Future.** No dedicated `tests/pretty/` directory yet.

When added:

```
tests/pretty/original.tw
tests/pretty/original.formatted.tw
```

Goal: `format(format(src)) == format(src)`.

---

# 6. Typechecker Tests

### Structure

```
tests/typecheck/pass/   # programs that must type-check cleanly
tests/typecheck/fail/   # programs that must produce specific type errors
```

Tests are driven by `tests/typecheck_test.rs`. Each `fail/` file has the expected
error message(s) embedded as comments.

### Coverage

* basic type inference and checking
* record field access and update
* variant construction and pattern matching
* generic type declarations and substitution
* capability records (function-typed fields)
* `try`/`Result` sugar
* cross-module type checking

---

# 7. Lowering Tests (AST → Core IR)

### Structure

```
tests/ir/   # Core IR lowering snapshots (insta)
```

`tests/integration_test.rs` drives `twk lower` and captures the output.

### Coverage

* no surface sugar remains after lowering
* `try` desugared into match
* `collect` lowered to loop + array append
* `for x in` lowered to indexed iteration
* all identifiers replaced by LocalIds
* variant and record constructors resolved to TypeId/VariantId

> **Future:** ANF IR tests will live in `tests/lower_anf/` (Stage 7).

---

# 8. End-to-End Run Tests

These ensure the full pipeline (parse → typecheck → lower → interpret) produces
correct output.

### Structure

```
tests/run/   # interpreter run tests
```

Each `.tw` file has the expected stdout embedded as leading comments:

```tw
// Expected output:
//   hello
//   42
println("hello")
println("${42}")
```

Tests are driven by `tests/run_test.rs`, which compares actual interpreter output
against the embedded expected output.

### Coverage

* arithmetic, strings, booleans
* control flow (if, case, for, collect)
* closures and higher-order functions
* records, arrays, dicts
* generic types and capability records
* `try`/`Result`
* multi-module programs (`tests/run/multi_module/`)

---

# 9. Module Tests

### Structure

```
tests/modules/   # multi-module compilation tests
```

Tests cover: import resolution, `pub`/private visibility, module aliasing,
collision errors, circular import errors, and cross-module inherent method calls.

---

# 10. Closure Tests

### Structure

```
tests/closure/   # closure capture semantics
```

Tests specifically for closure capture-by-value (spec §7.7).

---

# 11. Codegen Tests (WAT + Wasm)

> **Future** — Stage 8.

When added:

```
tests/codegen/xxx.tw
tests/codegen/xxx.wat         (golden)
tests/codegen/xxx.stdout      (execution result)
```

Requirements:

1. Lowered ANF IR → WAT must match golden `.wat`.
2. Compiling `.wat` → Wasm → executing → stdout must match `.stdout`.

---

# 12. Execution Tests (Cross-Runtime)

> **Future** — Stage 9+.

When added, execution must match across all runtimes:

* wasmtime
* self-hosted wasm
* Node (WASI)
* Deno (WASI)

---

# 13. WASI Contract Tests

> **Future** — Stage 9+.

When added:

```
tests/wasi/*.tw
tests/wasi/*.stdout
tests/wasi/fs/   (fixture FS hierarchy)
```

---

# 14. Self-Hosting Compatibility Tests

> **Future** — Stage 10.

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

# 15. Performance Tests (Optional)

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

# 16. CI Testing Matrix

### Build matrix includes:

Current (stage 0–5):

| Stage                   | Rust | Notes |
| ----------------------- | ---- | ----- |
| Parser tests            | ✓    | insta snapshots |
| Typechecker             | ✓    | pass/fail dirs  |
| IR lowering             | ✓    | insta snapshots |
| Run (interpreter)       | ✓    | expected output in comments |
| Module tests            | ✓    | multi-file programs |
| Closure tests           | ✓    | capture-by-value |

Future (stages 7–10):

| Stage                   | Rust | Self-hosted | wasmtime | Node | Deno | Browser |
| ----------------------- | ---- | ----------- | -------- | ---- | ---- | ------- |
| ANF IR tests            | ✓    | ✓           | —        | —    | —    | —       |
| Codegen                 | ✓    | ✓           | ✓        | —    | —    | —       |
| Execute (Wasm)          | ✓    | ✓           | ✓        | ✓    | ✓    | ✓       |
| WASI                    | ✓    | ✓           | ✓        | ✓    | ✓    | ✓       |
| Self-host compatibility | ✓    | ✓           | ✓        | —    | —    | —       |

---

# 17. Directory Layout

Current:

```
tests/
  parser/
  parser_errors/
  typecheck/
    pass/
    fail/
  closure/
  modules/
  ir/
  run/
  snapshots/
```

Future:

```
tests/
  lower_anf/
  codegen/
  wasi/
    fs/
  selfhost/
benchmarks/
```

---

# 18. Test Tooling

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

# 19. Stability Guarantees

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

# 20. How to Add a New Feature

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

# 21. Summary

This test plan ensures:

* correctness at every layer,
* deterministic compilation,
* stable IR semantics,
* reliable self-hosting,
* cross-platform WASI compatibility,
* safe evolution of the language.

Together with `docs/ir.md`, this establishes a robust foundation for Twinkle’s long-term health.

