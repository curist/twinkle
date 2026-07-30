# Generic Stringify I/O Builtins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `print`, `println`, `eprint`, `eprintln`, and `error` accept any value whose type satisfies the `Stringify` contract, while preserving the existing string-only runtime/host ABI.

**Architecture:** Split the public language API from the runtime sink ABI. Public prelude wrappers become ordinary generic Twinkle functions (`fn<T: Stringify>(value: T) ...`) that call hidden string-only runtime builtins after `value.to_string()`. The runtime imports, Wasm ABI, host functions, and stream behavior remain unchanged; only the public surface signatures and lowering path to those runtime sinks change.

**Tech Stack:** Twinkle boot compiler and prelude (`boot/`), contract checking through `Stringify`, existing runtime module `rt.core`, boot test suite via `target/twk run boot/tests/main.tw`, formatter/linter via `target/twk fmt ...` and `target/twk lint boot/main.tw`.

## Global Constraints

- Runtime and host ABI remain string-only: the host still receives `String` for print/error sinks.
- Public APIs become generic over `T: Stringify`: `print`, `println`, `eprint`, `eprintln`, and `error`.
- `error` keeps divergence semantics; the public wrapper must infer/behave as `Never`.
- `String` remains the no-surprise case; `println("hi")` and `error("boom")` keep working.
- Non-`Stringify` arguments are compile-time errors mentioning `Stringify`.
- Existing stdout/stderr split is unchanged.
- Do not add new user-declarable contracts or implicit conversions outside this family.
- Do not change tree-sitter grammar or run tree-sitter tests.
- After editing `.tw` files, run `target/twk fmt <changed .tw files>` and `target/twk lint boot/main.tw`.

---

## File Structure

- **Modify** `boot/compiler/signatures.tw` — carry each loaded signature file's stem so multiple free prelude modules (`range`, new `io`) can have correct function origins.
- **Modify** `boot/compiler/base_env.tw` — stop hardcoding public string-only I/O signatures; add hidden string-only sink signatures; bind public free names to prelude wrappers; set prelude origins from the signature-file stem.
- **Modify** `boot/compiler/builtins.tw` — rename runtime registry entries for the string-only sinks to hidden internal names while preserving wasm module/name targets and noreturn behavior.
- **Create** `boot/prelude/io.tw` — public generic wrappers for `print`, `println`, `eprint`, `eprintln`, and `error`.
- **Modify** `boot/tests/suites/checker_suite.tw` — checker coverage for generic accepted/rejected calls and `error` divergence.
- **Modify** `boot/tests/suites/codegen_integration_suite.tw` or an existing runtime/codegen suite — compile/run coverage proving generic calls stringify before reaching runtime sinks.
- **Modify** `boot/tests/suites/lsp_hover_suite.tw` — public hover signature/docs now show `T: Stringify`.
- **Modify** `boot/tests/suites/builtins_suite.tw`, `runtime_suite.tw`, `wasm_plan_suite.tw`, and any direct builtin-name tests — update expectations from public names to hidden runtime sink names where they inspect the registry/ABI rather than source calls.
- **Modify** `docs/contracts.md`, `docs/API.md`, and `docs/spec.md` — document that print/error sinks are selected generic APIs backed by `Stringify`.

---

## Task 1: Add failing public behavior tests

**Files:**
- Modify: `boot/tests/suites/checker_suite.tw`
- Modify: `boot/tests/suites/codegen_integration_suite.tw`
- Modify: `boot/tests/suites/lsp_hover_suite.tw`

**Interfaces:**
- Consumes: existing `check_ok_builtins`, `check_errs_builtins`, LSP hover helpers, and codegen harness helpers.
- Produces: failing tests that define the desired source-level API.

- [ ] **Step 1: Add checker acceptance tests for the print family.**

In `boot/tests/suites/checker_suite.tw`, near the existing `"call println"` test, add:

```twinkle
    .test(
      "print family accepts Stringify values",
      fn() {
        src := "fn demo() Void {\n  print(1)\n  println(true)\n  eprint(2.5)\n  eprintln(0x2a)\n}"
        _ := try check_ok_builtins(src)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Add checker acceptance tests for user-defined and generic `Stringify`.**

Add:

```twinkle
    .test(
      "println accepts user Stringify witness",
      fn() {
        src := "type Point = .{ x: Int, y: Int }\nfn to_string(p: Point) String { \"${p.x},${p.y}\" }\nfn demo() Void { println(Point.{ x: 1, y: 2 }) }"
        _ := try check_ok_builtins(src)
        .Ok({})
      },
    )
    .test(
      "println accepts generic Stringify bound",
      fn() {
        src := "fn log<T: Stringify>(x: T) Void { println(x) }\nfn demo() Void { log(42) }"
        _ := try check_ok_builtins(src)
        .Ok({})
      },
    )
```

- [ ] **Step 3: Add rejection test for non-`Stringify` values.**

Add:

```twinkle
    .test(
      "println rejects non Stringify value",
      fn() {
        msgs := try check_errs_builtins("type Hidden = .{}\nfn demo() Void { println(Hidden.{}) }")
        try assert.ok(msgs.len() > 0, "expected diagnostics")
        try assert.ok(msgs[0].contains("Stringify"), "diagnostic should mention Stringify")
        .Ok({})
      },
    )
```

- [ ] **Step 4: Add `error` generic/divergence tests.**

Add:

```twinkle
    .test(
      "error accepts Stringify and still diverges",
      fn() {
        src := "fn fail() Int { error(42) }"
        _ := try check_ok_builtins(src)
        .Ok({})
      },
    )
```

- [ ] **Step 5: Add runtime/codegen coverage for stdout.**

In `boot/tests/suites/codegen_integration_suite.tw`, add a source-run test near the other `println` integration tests. Use the suite's existing helper for compiling/running source strings; the source should be:

```twinkle
println(42)
println(true)
println(.Some(7))
```

Expected stdout:

```text
42
true
Some(7)
```

- [ ] **Step 6: Update builtin hover expectation.**

In `boot/tests/suites/lsp_hover_suite.tw`, change the `"hover on builtin println shows docs"` assertion from:

```twinkle
try assert.str_contains(content, "fn(s: String) Void")
```

to:

```twinkle
try assert.str_contains(content, "fn<T: Stringify>(value: T) Void")
```

Keep the doc assertion, but later tasks will update the text from “Print a string...” to “Print a Stringify value...”.

- [ ] **Step 7: Run focused tests and confirm they fail.**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: the new generic I/O tests fail because the public signatures are still string-only or because runtime calls still receive non-string arguments.

---

## Task 2: Track prelude signature file stems

**Files:**
- Modify: `boot/compiler/signatures.tw`
- Modify: `boot/compiler/base_env.tw`

**Interfaces:**
- Produces: `SignatureGroup` includes the source file stem, e.g. `"range"`, `"io"`, `"vector"`.
- Produces: `set_prelude_function_origins` can map free functions from `boot/prelude/io.tw` to the `io` module, not accidentally to `range`.
- Consumes: existing `SignatureGroup.receiver` and `SignatureGroup.sigs` behavior remains unchanged for method registration.

- [ ] **Step 1: Extend `SignatureGroup`.**

In `boot/compiler/signatures.tw`, change:

```twinkle
pub type SignatureGroup = .{ receiver: String?, sigs: Vector<FunctionSig> }
```

to:

```twinkle
pub type SignatureGroup = .{ stem: String, receiver: String?, sigs: Vector<FunctionSig> }
```

- [ ] **Step 2: Thread the stem through loading.**

Change `load_group_from_source` to accept `stem: String` and return it:

```twinkle
fn load_group_from_source(src: String, stem: String, receiver: String?, env: ResolvedEnv) Result<
  SignatureGroup,
  String,
> {
  // existing body
  .Ok(SignatureGroup.{ stem, receiver, sigs })
}
```

Update its caller in `load_signatures`:

```twinkle
group := try load_group_from_source(src, stem, receiver, type_env)
```

- [ ] **Step 3: Replace free-function origin guessing.**

In `boot/compiler/base_env.tw`, remove the `prelude_module_stem(receiver: String?)` helper and update `set_prelude_function_origins` to use `group.stem`:

```twinkle
fn set_prelude_function_origins(
  env: ResolvedEnv,
  groups: Vector<signatures.SignatureGroup>,
  prelude_dir: String,
  canonical_roots: imports.CanonicalRoots,
) ResolvedEnv {
  for group in groups {
    module_path := imports.canonical_module_path(
      path.join(prelude_dir, "${group.stem}.tw"),
      canonical_roots,
    )

    for sig in group.sigs {
      internal_name := signatures.to_internal_name(group.receiver, sig.name)
      env = .set_function_origin(internal_name, .{ module_path, func_name: sig.name })
    }
  }

  env
}
```

- [ ] **Step 4: Run formatter and a focused compiler test.**

Run:

```bash
target/twk fmt boot/compiler/signatures.tw boot/compiler/base_env.tw
target/twk run boot/tests/main.tw
```

Expected: existing behavior is unchanged except for tests added in Task 1, which still fail.

---

## Task 3: Introduce hidden string-only runtime sinks

**Files:**
- Modify: `boot/compiler/builtins.tw`
- Modify: `boot/compiler/base_env.tw`
- Modify: registry/ABI tests that inspect print/error builtin names directly

**Interfaces:**
- Produces hidden runtime builtin names:
  - `__print_string: fn(String) Void`
  - `__println_string: fn(String) Void`
  - `__error_string: fn(String) Never`
  - `__eprint_string: fn(String) Void`
  - `__eprintln_string: fn(String) Void`
- Preserves wasm imports:
  - `rt.core.print`
  - `rt.core.println`
  - `rt.core.trap`
  - `rt.core.eprint`
  - `rt.core.eprintln`

- [ ] **Step 1: Rename builtin registry specs while preserving wasm targets.**

In `boot/compiler/builtins.tw`, change the first runtime specs from public names to hidden names:

```twinkle
rt("__print_string", "rt.core", "print", .None),
rt("__println_string", "rt.core", "println", .None),
rt("__error_string", "rt.core", "trap", .None),
rt("__eprint_string", "rt.core", "eprint", .None),
rt("__eprintln_string", "rt.core", "eprintln", .None),
```

- [ ] **Step 2: Update string sink ABI cases.**

In `builtin_abi`, replace the public-name cases with hidden-name cases:

```twinkle
"__print_string" => abi([str_n()], []),
"__println_string" => abi([str_n()], []),
"__error_string" => abi([str_n()], []),
"__eprint_string" => abi([str_n()], []),
"__eprintln_string" => abi([str_n()], []),
```

- [ ] **Step 3: Preserve noreturn metadata for the hidden error sink.**

Update `is_noreturn`:

```twinkle
.Some(e) => e.name == "__error_string",
```

- [ ] **Step 4: Replace hardcoded public signatures with hidden signatures.**

In `boot/compiler/base_env.tw`, remove the hardcoded public `print`/`println`/`error`/`eprint`/`eprintln` signatures. Add hidden signatures instead:

```twinkle
builtin_sig("__print_string", [], ["s"], [.String], .Some(.Void)),
builtin_sig("__println_string", [], ["s"], [.String], .Some(.Void)),
builtin_sig("__error_string", [], ["msg"], [.String], .Some(.Never)),
builtin_sig("__eprint_string", [], ["s"], [.String], .Some(.Void)),
builtin_sig("__eprintln_string", [], ["s"], [.String], .Some(.Void)),
```

Keep `range`, `range_from`, and `range_step` in `bind_public_free_builtins`; keep `print`, `println`, `error`, `eprint`, and `eprintln` in that binding list so the names bind to the prelude wrappers added in Task 4.

- [ ] **Step 5: Update direct builtin registry tests.**

Where tests inspect builtin registry names or IDs, update them to use hidden names. Example replacements:

```twinkle
reg.id("println")
```

becomes:

```twinkle
reg.id("__println_string")
```

Assertions about imported wasm names should still expect `"println"` because the host import name did not change.

- [ ] **Step 6: Run focused tests.**

Run:

```bash
target/twk fmt boot/compiler/builtins.tw boot/compiler/base_env.tw
target/twk run boot/tests/main.tw
```

Expected: registry/ABI tests updated in this task pass; public generic behavior still fails until the wrappers exist.

---

## Task 4: Add public generic prelude wrappers

**Files:**
- Create: `boot/prelude/io.tw`
- Modify: `boot/compiler/base_env.tw` if public free binding list needs adjustment after the new signatures load

**Interfaces:**
- Consumes: hidden string-only sink signatures from Task 3.
- Produces public functions:
  - `print<T: Stringify>(value: T) Void`
  - `println<T: Stringify>(value: T) Void`
  - `eprint<T: Stringify>(value: T) Void`
  - `eprintln<T: Stringify>(value: T) Void`
  - `error<T: Stringify>(value: T)` with inferred `Never`

- [ ] **Step 1: Create `boot/prelude/io.tw`.**

Write:

```twinkle
/// Print a Stringify value to stdout.
pub fn print<T: Stringify>(value: T) Void {
  __print_string(value.to_string())
}

/// Print a Stringify value to stdout followed by a newline.
pub fn println<T: Stringify>(value: T) Void {
  __println_string(value.to_string())
}

/// Trap with an unrecoverable error message rendered from a Stringify value.
pub fn error<T: Stringify>(value: T) {
  __error_string(value.to_string())
}

/// Print a Stringify value to stderr.
pub fn eprint<T: Stringify>(value: T) Void {
  __eprint_string(value.to_string())
}

/// Print a Stringify value to stderr followed by a newline.
pub fn eprintln<T: Stringify>(value: T) Void {
  __eprintln_string(value.to_string())
}
```

- [ ] **Step 2: Confirm public free names bind to wrapper signatures.**

In `bind_public_free_builtins`, keep this list:

```twinkle
for name in [
  "print",
  "println",
  "error",
  "eprint",
  "eprintln",
  "range",
  "range_from",
  "range_step",
] {
  env = .bind_function(name, name)
}
```

Because Task 3 removed the public hardcoded signatures and Task 4 adds prelude signatures with those names, these bindings now target prelude wrappers.

- [ ] **Step 3: Verify the hidden sink calls resolve inside prelude code.**

Run:

```bash
target/twk fmt boot/prelude/io.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

Expected: the Task 1 checker and stdout runtime tests now pass. Any remaining failures should be direct tests that still expect public names in the runtime builtin registry.

- [ ] **Step 4: Inspect generated WAT for ABI preservation.**

Create a temporary source:

```bash
cat > /tmp/generic_io.tw <<'TW'
println(42)
TW
target/twk build /tmp/generic_io.tw -o /tmp/generic_io.wat
rg 'import "twinkle_runtime" "println"|__println_string|from_i64' /tmp/generic_io.wat
```

Expected: WAT imports host `println`, calls the Int stringification path, and passes a string to the runtime sink.

---

## Task 5: Update docs and hover expectations

**Files:**
- Modify: `docs/contracts.md`
- Modify: `docs/API.md`
- Modify: `docs/spec.md`
- Modify: `boot/tests/suites/lsp_hover_suite.tw`

**Interfaces:**
- Consumes: public signatures and docs from `boot/prelude/io.tw`.
- Produces: user-facing documentation matching the new generic API.

- [ ] **Step 1: Update `docs/contracts.md`.**

In the `Stringify` “Used by” list, add print/error sinks:

```markdown
- string interpolation: `"value=${expr}"`
- generic `.to_string()` calls on `T: Stringify`
- generic print/error sinks: `print`, `println`, `eprint`, `eprintln`, and `error`
- APIs that require canonical string rendering
```

- [ ] **Step 2: Update `docs/API.md` I/O signatures.**

Replace the I/O table with:

```markdown
| Function | Signature | Description |
|----------|-----------|-------------|
| `print` | `fn<T: Stringify>(value: T) Void` | Print a Stringify value to stdout (no newline) |
| `println` | `fn<T: Stringify>(value: T) Void` | Print a Stringify value to stdout with newline |
| `eprint` | `fn<T: Stringify>(value: T) Void` | Print a Stringify value to stderr (no newline) |
| `eprintln` | `fn<T: Stringify>(value: T) Void` | Print a Stringify value to stderr with newline |
| `error` | `fn<T: Stringify>(value: T) Never` | Trap with an unrecoverable error message rendered from a Stringify value |
```

- [ ] **Step 3: Update `docs/spec.md`.**

In §10.1, change the `Stringify` backs text to include print/error sinks:

```markdown
| `Stringify` | `to_string(self) String` | string interpolation, generic stringification, print/error sinks |
```

In the interpolation/stringification section, add:

```markdown
The print/error sink family (`print`, `println`, `eprint`, `eprintln`, and
`error`) uses the same `Stringify` proof as interpolation. At runtime these
functions still pass strings to the host; the generic wrapper renders the value
first.
```

- [ ] **Step 4: Update hover test doc text.**

In `boot/tests/suites/lsp_hover_suite.tw`, assert the new signature and doc:

```twinkle
try assert.str_contains(content, "fn<T: Stringify>(value: T) Void")
try assert.str_contains(content, "Print a Stringify value to stdout followed by a newline.")
```

- [ ] **Step 5: Run docs-adjacent tests.**

Run:

```bash
target/twk fmt boot/tests/suites/lsp_hover_suite.tw
target/twk run boot/tests/main.tw
```

Expected: hover tests pass with the generic signature.

---

## Task 6: Final verification and cleanup

**Files:**
- Modify any remaining tests that still assume public string-only I/O signatures or public runtime builtin names.

**Interfaces:**
- Consumes: completed implementation from Tasks 2–5.
- Produces: verified tree with docs, checker, lowering/linking, and runtime behavior aligned.

- [ ] **Step 1: Run formatter/linter.**

Run:

```bash
target/twk fmt boot/compiler/signatures.tw boot/compiler/base_env.tw boot/compiler/builtins.tw boot/prelude/io.tw boot/tests/suites/checker_suite.tw boot/tests/suites/codegen_integration_suite.tw boot/tests/suites/lsp_hover_suite.tw
target/twk lint boot/main.tw
```

Expected: formatter is idempotent and linter reports no new relevant findings.

- [ ] **Step 2: Run boot tests.**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: PASS.

- [ ] **Step 3: Run Rust tests if stage0-facing assumptions changed.**

Run only if implementation touched Rust stage0 or shared generated assets:

```bash
cargo test --release
```

Expected: PASS.

- [ ] **Step 4: Build the standalone CLI if boot payload or bundled prelude changed.**

Run:

```bash
make quick-bundle-cli
```

Expected: `target/twk` rebuilds successfully from the current `target/boot.wasm` payload.

- [ ] **Step 5: Manual smoke tests.**

Run:

```bash
cat > /tmp/generic_io_smoke.tw <<'TW'
type Point = .{ x: Int, y: Int }
fn to_string(p: Point) String { "(${p.x}, ${p.y})" }
println(42)
println(true)
println(Point.{ x: 1, y: 2 })
TW
target/twk run /tmp/generic_io_smoke.tw
```

Expected stdout:

```text
42
true
(1, 2)
```

- [ ] **Step 6: Commit.**

```bash
git add boot/compiler/signatures.tw boot/compiler/base_env.tw boot/compiler/builtins.tw boot/prelude/io.tw boot/tests/suites/checker_suite.tw boot/tests/suites/codegen_integration_suite.tw boot/tests/suites/lsp_hover_suite.tw docs/contracts.md docs/API.md docs/spec.md
git add boot/tests/suites/builtins_suite.tw boot/tests/suites/runtime_suite.tw boot/tests/suites/wasm_plan_suite.tw
git commit -m "Make print sinks Stringify-generic"
```

---

## Self-Review Notes

- The public API requirement is covered by Task 4 and documented in Task 5.
- Runtime ABI preservation is covered by hidden string sink names in Task 3 and WAT inspection in Task 4.
- `error` divergence is covered by `__error_string` noreturn metadata in Task 3 and checker tests in Task 1.
- Multiple free prelude modules are covered by the signature stem work in Task 2, avoiding incorrect origins for `boot/prelude/io.tw`.
- First-class use of the public functions is preserved by implementing them as ordinary prelude functions rather than special-casing direct calls in the lowerer.
