# Test Plan

Testing methodology for the Twinkle compiler, designed to guarantee correctness
across all compiler stages, ensure IR stability, support self-hosting, and
validate multi-runtime consistency.

---

## Philosophy

1. **Test across the full stack.** Syntax + semantics + IR + execution — no
   single-layer tests are sufficient.
2. **Golden tests are authoritative.** AST, Core IR, ANF, WAT output, and
   diagnostics are all snapshot-validated.
3. **All tests run on at least two compilers** — Rust bootstrap and self-hosted.
4. **Produced Wasm must be identical** between bootstrap and self-hosted builds.
5. **Execution output must match across runtimes** (wasmtime, Node, Deno, browser).
6. **Tests must be hermetic** — no randomness, no time, no nondeterministic
   ordering, no external I/O beyond WASI-mocked FS.

---

## Current Test Categories

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
  run/              # end-to-end interpreter run tests
  snapshots/        # insta snapshot storage
```

### Parser Tests

Insta snapshots capture the pretty-printed AST. Cover operator precedence,
block expressions, control flow, lambda/function syntax, record/enum syntax,
string interpolation, and error cases.

### Typechecker Tests

`pass/` programs must type-check cleanly. `fail/` programs must produce specific
type errors (embedded as comments). Cover inference, record access/update,
variants, generics, capability records, `try`/`Result`, cross-module types.

### Core IR Lowering Tests

Verify no surface sugar remains: `try` → match, `collect` → loop+append,
`for x in` → indexed iteration, all identifiers → LocalIds, variants/records →
TypeId/VariantId.

### End-to-End Run Tests

Each `.tw` file has expected stdout embedded as leading comments. Tests are
driven by `tests/run_test.rs`. Cover arithmetic, strings, control flow,
closures, records, arrays, dicts, generics, `try`/`Result`, and multi-module
programs.

### Module Tests

Import resolution, `pub`/private visibility, module aliasing, collision errors,
circular import errors, cross-module inherent method calls.

### Closure Tests

Specifically for capture-by-value semantics (spec §7.7).

---

## Future Test Categories

| Category | Stage | Purpose |
|----------|-------|---------|
| `tests/lower_anf/` | 7 | ANF IR lowering snapshots |
| `tests/codegen/` | 8 | WAT/Wasm golden output + execution |
| `tests/wasi/` | 9+ | WASI contract tests |
| `tests/selfhost/` | 10 | Self-hosting compatibility |

### Codegen Tests

Each test produces a golden `.wat` and a `.stdout`. Compiling `.wat` → Wasm →
executing must match the expected output.

### Self-Hosting Compatibility

The most important long-term stability test: compile with bootstrap, compile
with self-hosted, diff outputs (must be byte-for-byte identical), execute both
and compare.

### Cross-Runtime Execution

Execution must match across wasmtime, Node (WASI), Deno (WASI), and browser.

---

## CI Matrix

Current (stages 0–5):

| Stage | Rust |
|-------|------|
| Parser tests | insta snapshots |
| Typechecker | pass/fail dirs |
| IR lowering | insta snapshots |
| Run (interpreter) | expected output |
| Module tests | multi-file programs |
| Closure tests | capture-by-value |

Future (stages 7–10):

| Stage | Rust | Self-hosted | wasmtime | Node | Deno | Browser |
|-------|------|-------------|----------|------|------|---------|
| ANF IR | yes | yes | — | — | — | — |
| Codegen | yes | yes | yes | — | — | — |
| Execute | yes | yes | yes | yes | yes | yes |
| WASI | yes | yes | yes | yes | yes | yes |
| Self-host compat | yes | yes | yes | — | — | — |

---

## Adding New Features

A PR must include tests for: parsing, typechecking, IR lowering, ANF (if
applicable), codegen for new ops, execution, and self-host tests if it affects
output.

---

## Tooling

* **Golden snapshots**: insta for Rust bootstrap, custom harness for Twinkle
* `.json` for AST/Core IR/ANF IR, `.wat` for WAT, `.stdout` for execution
* Deterministic sorting in pretty-printer and dict iteration

```bash
cargo test            # bootstrap tests
twinkle test tests/   # self-hosted test runner
```

Together with `docs/internals/ir.md`, this establishes the foundation for
Twinkle's long-term correctness guarantees.
