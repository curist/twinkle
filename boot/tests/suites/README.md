# Twinkle Test Suites

Test suites run via the Twinkle-native test runner (`boot/tests/runner.tw`).

## Suite Contract

1. Each suite file exports exactly one `pub fn suite() runner.Suite`.
2. Each test callback returns `Result<Void, String>`.
3. Use `try assert.*(...)` for checks and end with `.Ok({})`.
4. Keep test data local and deterministic (no random/time/process spawning).
5. Name tests by behavior, not implementation detail, so `TWK_TEST_FILTER` stays useful.
6. Prefer one semantic claim per test; split multi-claim flows into multiple tests unless setup is expensive.
7. Use helper functions inside the suite file for repeated setup/transforms.

## Adding a New Suite

1. Create `boot/tests/suites/<name>_suite.tw`
2. Add `use tests.suites.<name>_suite` to `boot/tests/main.tw`
3. Add `<name>_suite.suite()` to the `run_all([...])` call

## Running

```bash
# Both backends
cargo run -- run -i boot/tests/main.tw
cargo run -- run boot/tests/main.tw

# Filtered
TWK_TEST_FILTER='closure' cargo run -- run -i boot/tests/main.tw
```

## Template

```tw
use tests.runner
use tests.assert

pub fn suite() runner.Suite {
  runner.suite("my domain")
    .test("behavior under test", fn() {
      try assert.equal(1 + 1, 2)
      .Ok({})
    })
}
```
