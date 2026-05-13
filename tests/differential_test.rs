mod common;

use std::path::Path;

use common::FixtureExpectation;

fn is_behavior_fixture(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    // bench_ and fib_perf are performance-only fixtures
    !(name.starts_with("bench_") || name == "fib_perf.tw")
}

#[test]
fn wasm_all_run_fixtures() {
    let fixtures = common::discover_run_fixtures(Path::new("tests/run"));
    let fixtures: Vec<_> = fixtures
        .into_iter()
        .filter(|fixture| is_behavior_fixture(&fixture.path))
        .collect();
    assert!(
        !fixtures.is_empty(),
        "no runnable fixtures discovered under tests/run"
    );

    for fixture in fixtures {
        match fixture.expectation {
            FixtureExpectation::Output { stdout, stderr } => {
                let (wasm_stdout, wasm_stderr) = common::run_wasm_capture(&fixture.path)
                    .unwrap_or_else(|e| {
                        panic!("wasm run failed for {}: {e}", fixture.path.display())
                    });

                let wasm_lines: Vec<&str> = wasm_stdout.lines().collect();
                let wasm_stderr_lines: Vec<&str> = wasm_stderr.lines().collect();

                assert_eq!(
                    wasm_lines,
                    stdout,
                    "{}: wasm stdout mismatch\nExpected:\n{}\nActual:\n{}",
                    fixture.path.display(),
                    stdout.join("\n"),
                    wasm_stdout
                );
                assert_eq!(
                    wasm_stderr_lines,
                    stderr,
                    "{}: wasm stderr mismatch\nExpected:\n{}\nActual:\n{}",
                    fixture.path.display(),
                    stderr.join("\n"),
                    wasm_stderr
                );
            }
            FixtureExpectation::Trap { .. } => {
                common::run_wasm_capture(&fixture.path)
                    .expect_err(&format!("{}: wasm should trap", fixture.path.display()));
            }
        }
    }
}
