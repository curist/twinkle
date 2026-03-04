//! Differential test: runs every tests/run/*.tw fixture through both the
//! interpreter and the Wasm backend, comparing stdout output.

use std::collections::BTreeSet;
use std::fs;

/// Fixtures that are known to fail in the Wasm backend. As bugs are fixed,
/// entries are removed. The goal is an empty skip list.
fn wasm_skip_list() -> BTreeSet<&'static str> {
    [
        // Defer capture-by-value semantics (wasm sees mutated value)
        "defer_capture",
        // Needs __debug_stdin_read_all stub
        "twinkle_typechecker",
        // Bug 5: String runtime bounds
        "string_methods",
        // Interpreter bug (proc.args() not supported in test harness)
        "stdlib_proc",
    ]
    .into_iter()
    .collect()
}

/// Parse the expected output from leading comment block.
fn parse_expected(source: &str) -> Vec<String> {
    let mut lines = source.lines();
    let found = lines.by_ref().any(|l| l.trim() == "// Expected output:");
    if !found {
        return vec![];
    }
    let mut result = Vec::new();
    for line in lines {
        if let Some(rest) = line.strip_prefix("//   ") {
            result.push(rest.to_string());
        } else if line.starts_with("//") {
            result.push("".to_string());
        } else {
            break;
        }
    }
    result
}

/// Run through interpreter.
fn run_interp(path: &str) -> anyhow::Result<String> {
    let (core_module, _registry) =
        twinkle::module::compile_entry(path).map_err(|e| anyhow::anyhow!("{}", e))?;
    let mut interp = twinkle::interp::Interpreter::new(core_module, Vec::<u8>::new());
    interp.run()?;
    let bytes = interp.into_output();
    Ok(String::from_utf8(bytes).expect("interpreter output is valid UTF-8"))
}

/// Run through Wasm backend.
fn run_wasm(path: &str) -> anyhow::Result<String> {
    let (stdout, _stderr) = twinkle::cli::run_wasm::run_wasm_capture(path)?;
    Ok(stdout)
}

/// Parse expected trap message.
fn parse_expected_trap(source: &str) -> Option<String> {
    source.lines().find_map(|l| {
        l.trim()
            .strip_prefix("// Expected trap: ")
            .map(|s| s.to_string())
    })
}

/// Discover all .tw test fixtures (non-trap) that have expected output.
fn discover_fixtures() -> Vec<(String, String)> {
    let mut fixtures = Vec::new();

    // Single-file fixtures
    for entry in fs::read_dir("tests/run").expect("tests/run dir exists") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map_or(true, |e| e != "tw") {
            continue;
        }
        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
        let source = fs::read_to_string(&path).unwrap();
        if !parse_expected(&source).is_empty() {
            fixtures.push((name, path.to_str().unwrap().to_string()));
        }
    }

    // Multi-file fixtures (directories with main.tw)
    for entry in fs::read_dir("tests/run").expect("tests/run dir exists") {
        let entry = entry.unwrap();
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let main_tw = path.join("main.tw");
        if main_tw.exists() {
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            let source = fs::read_to_string(&main_tw).unwrap();
            if !parse_expected(&source).is_empty() {
                fixtures.push((name, main_tw.to_str().unwrap().to_string()));
            }
        }
    }

    fixtures.sort();
    fixtures
}

#[test]
fn differential_interp_vs_wasm() {
    let skip = wasm_skip_list();
    let fixtures = discover_fixtures();
    assert!(!fixtures.is_empty(), "no fixtures discovered");

    let mut passed = 0;
    let mut skipped = 0;
    let mut failed = Vec::new();

    for (name, path) in &fixtures {
        if skip.contains(name.as_str()) {
            skipped += 1;
            continue;
        }

        let interp_out = match run_interp(path) {
            Ok(out) => out,
            Err(e) => {
                failed.push(format!("{name}: interpreter error: {e}"));
                continue;
            }
        };

        let wasm_out = match run_wasm(path) {
            Ok(out) => out,
            Err(e) => {
                failed.push(format!("{name}: wasm error: {e}"));
                continue;
            }
        };

        if interp_out != wasm_out {
            failed.push(format!(
                "{name}: output mismatch\n  interp: {:?}\n  wasm:   {:?}",
                interp_out.lines().collect::<Vec<_>>(),
                wasm_out.lines().collect::<Vec<_>>(),
            ));
        } else {
            passed += 1;
        }
    }

    if !failed.is_empty() {
        panic!(
            "Differential test failures ({} passed, {} skipped, {} failed):\n{}",
            passed,
            skipped,
            failed.len(),
            failed.join("\n\n")
        );
    }

    eprintln!(
        "differential test: {passed} passed, {skipped} skipped (in skip list), {} total fixtures",
        fixtures.len()
    );
}

/// Trap tests: verify both interpreter and Wasm produce errors (not success).
/// Note: Wasm trap messages are generic ("wasm trap: ...") and don't match
/// the interpreter's detailed messages, so we only check that both error.
#[test]
fn differential_traps() {
    let trap_files = [
        "tests/run/traps/array_oob.tw",
        "tests/run/traps/div_zero.tw",
        "tests/run/traps/error_call.tw",
    ];

    for path in &trap_files {
        let source = fs::read_to_string(path).unwrap();
        let expected_msg = parse_expected_trap(&source)
            .unwrap_or_else(|| panic!("No '// Expected trap:' in {path}"));

        // Interpreter must trap with expected message
        let interp_err = run_interp(path).expect_err(&format!("{path}: interpreter should trap"));
        assert!(
            interp_err.to_string().contains(&expected_msg),
            "{path}: interp trap mismatch: {}",
            interp_err
        );

        // Wasm must also trap (message may differ)
        run_wasm(path).expect_err(&format!("{path}: wasm should trap"));
    }
}
