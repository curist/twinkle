# Boot Compiler Tests

## Running Tests

Default suite (`main.tw`, excluding the slow codegen integration suite):

```bash
cargo run --release -- run boot/tests/main.tw        # Wasm backend
cargo run --release -- run -i boot/tests/main.tw      # interpreter
```

Grouped entry points for faster iteration:

| Entry point        | Scope                                    |
|--------------------|------------------------------------------|
| `test_api.tw`      | prelude, stdlib, source, module loader   |
| `test_frontend.tw` | parser, resolver, checker, semantics     |
| `test_ir.tw`       | core IR, ANF, builtins                   |
| `test_opt.tw`      | optimization passes                      |
| `test_codegen.tw`  | wasm IR, layout, emit, linker, runtime   |

```bash
cargo run --release -- run -i boot/tests/test_opt.tw
```

`test_codegen.tw` still includes the slower `codegen integration (M11)` suite, but
`main.tw` no longer does.

The default reporter is compact: passing tests print `.`, failing tests print `x`,
and failures are listed at the end. Set `TWK_TEST_REPORT=verbose` to restore the
per-suite, per-test output.

## Filtering

Set `TWK_TEST_FILTER` to match test or suite names:

```bash
TWK_TEST_FILTER="module loader" cargo run --release -- run -i boot/tests/test_api.tw
TWK_TEST_FILTER="resolve" cargo run --release -- run -i boot/tests/test_api.tw
TWK_TEST_REPORT=verbose cargo run --release -- run -i boot/tests/test_api.tw
```

## Structure

```
boot/tests/
  main.tw              # runs all suites (CI entry point)
  test_*.tw            # grouped entry points
  runner.tw            # test harness (Suite, Test, run_all)
  assert.tw            # assertion helpers (str_eq, int_eq, is_true, ...)
  helpers/             # shared test utilities
  suites/              # one file per test suite
    parser_suite.tw
    checker_suite.tw
    module_loader_suite.tw
    ...
```

## Adding a New Suite

1. Create `suites/my_feature_suite.tw`:

```tw
use tests.runner
use tests.assert

pub fn suite() runner.Suite {
  runner.suite("my feature")
    .test("does the thing", fn() {
      try assert.equal(actual, "expected")
      .Ok({})
    })
}
```

2. Add it to the appropriate `test_*.tw` group and to `main.tw`:

```tw
use tests.suites.my_feature_suite

// in the run_all([...]) list:
  my_feature_suite.suite(),
```
