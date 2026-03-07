//! Differential trap test: verifies both the interpreter and Wasm backend
//! report errors for known trap fixtures.

use std::fs;

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
