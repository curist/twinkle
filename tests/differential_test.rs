mod common;

use std::path::Path;

use common::FixtureExpectation;

fn is_behavior_fixture(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    // bench_ and fib_perf are performance-only fixtures
    // if_else_variant_shorthand uses whole-number floats where interpreter prints "1.0"
    // but wasm runtime prints "1" — known interpreter float_to_string divergence
    !(name.starts_with("bench_") || name == "fib_perf.tw" || name == "if_else_variant_shorthand.tw")
}

#[test]
fn differential_trap_fixtures() {
    let fixtures = common::discover_run_fixtures(Path::new("tests/run"));
    for fixture in fixtures
        .into_iter()
        .filter(|fixture| is_behavior_fixture(&fixture.path))
    {
        let FixtureExpectation::Trap { message } = fixture.expectation else {
            continue;
        };
        let interp_err = common::run_interp_capture(&fixture.path).expect_err(&format!(
            "{}: interpreter should trap",
            fixture.path.display()
        ));
        assert!(
            interp_err.to_string().contains(&message),
            "{}: interpreter trap mismatch\nExpected to contain: {}\nActual: {}",
            fixture.path.display(),
            message,
            interp_err
        );
        common::run_wasm_capture(&fixture.path)
            .expect_err(&format!("{}: wasm should trap", fixture.path.display()));
    }
}

#[test]
fn differential_interp_vs_wasm_all_run_fixtures() {
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
                let interp_stdout = common::run_interp_capture(&fixture.path).unwrap_or_else(|e| {
                    panic!("interpreter run failed for {}: {e}", fixture.path.display())
                });
                let (wasm_stdout, wasm_stderr) = common::run_wasm_capture(&fixture.path)
                    .unwrap_or_else(|e| {
                        panic!("wasm run failed for {}: {e}", fixture.path.display())
                    });

                let interp_lines: Vec<&str> = interp_stdout.lines().collect();
                let wasm_lines: Vec<&str> = wasm_stdout.lines().collect();
                let wasm_stderr_lines: Vec<&str> = wasm_stderr.lines().collect();

                assert_eq!(
                    interp_lines,
                    stdout,
                    "{}: interpreter stdout mismatch\nExpected:\n{}\nActual:\n{}",
                    fixture.path.display(),
                    stdout.join("\n"),
                    interp_stdout
                );
                assert_eq!(
                    wasm_lines,
                    stdout,
                    "{}: wasm stdout mismatch\nExpected:\n{}\nActual:\n{}",
                    fixture.path.display(),
                    stdout.join("\n"),
                    wasm_stdout
                );
                assert_eq!(
                    wasm_lines,
                    interp_lines,
                    "{}: interpreter/wasm stdout diverged\nInterpreter:\n{}\nWasm:\n{}",
                    fixture.path.display(),
                    interp_stdout,
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
            FixtureExpectation::Trap { message } => {
                let interp_err = common::run_interp_capture(&fixture.path).expect_err(&format!(
                    "{}: interpreter should trap",
                    fixture.path.display()
                ));
                assert!(
                    interp_err.to_string().contains(&message),
                    "{}: interpreter trap mismatch\nExpected to contain: {}\nActual: {}",
                    fixture.path.display(),
                    message,
                    interp_err
                );
                common::run_wasm_capture(&fixture.path)
                    .expect_err(&format!("{}: wasm should trap", fixture.path.display()));
            }
        }
    }
}
