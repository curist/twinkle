use std::fs;

/// Parse expected output from the leading comment block in a `.tw` fixture.
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
            result.push(String::new());
        } else {
            break;
        }
    }
    result
}

fn check(path: &str) {
    let source = fs::read_to_string(path).expect("test file exists");
    let expected = parse_expected(&source);
    assert!(
        !expected.is_empty(),
        "No '// Expected output:' block in {path}"
    );

    let (stdout, stderr) = twinkle::cli::run_wasm::run_wasm_capture(path)
        .unwrap_or_else(|e| panic!("run_wasm_capture failed for {path}: {e}"));
    let actual: Vec<&str> = stdout.lines().collect();

    assert_eq!(
        actual,
        expected,
        "Output mismatch for {path}\nExpected:\n{}\nActual:\n{}",
        expected.join("\n"),
        stdout
    );
    assert!(
        stderr.is_empty(),
        "Expected empty stderr for {path}, got:\n{stderr}"
    );
}

#[test]
fn run_wasm_hello() {
    check("tests/run/hello.tw");
}

#[test]
fn run_wasm_arithmetic() {
    check("tests/run/arithmetic.tw");
}

#[test]
fn run_wasm_collect_parity() {
    check("tests/run/collect_parity.tw");
}

#[test]
fn run_wasm_collect_while() {
    check("tests/run/collect_while.tw");
}

#[test]
fn run_wasm_strings() {
    check("tests/run/strings.tw");
}

#[test]
fn run_wasm_string_methods() {
    check("tests/run/string_methods.tw");
}

#[test]
fn run_wasm_closures() {
    check("tests/run/closures.tw");
}

#[test]
fn run_wasm_cell_update() {
    check("tests/run/cell_update.tw");
}

#[test]
fn run_wasm_defer_capture() {
    check("tests/run/defer_capture.tw");
}

#[test]
fn run_wasm_defer_return_loop_order() {
    check("tests/run/defer_return_loop_order.tw");
}

#[test]
fn run_wasm_for_break() {
    check("tests/run/for_break.tw");
}

#[test]
fn run_wasm_capability_records() {
    check("tests/run/capability_records.tw");
}

#[test]
fn run_wasm_iterator_direct_next() {
    check("tests/run/iterator_direct_next.tw");
}

#[test]
fn run_wasm_iterator_first_class_return() {
    check("tests/run/iterator_first_class_return.tw");
}

#[test]
fn run_wasm_iterator_advanced() {
    check("tests/run/iterator_advanced.tw");
}

#[test]
fn run_wasm_iterator_for_loop() {
    check("tests/run/iterator_for_loop.tw");
}

#[test]
fn run_wasm_iterator_rebind_shape_change() {
    check("tests/run/iterator_rebind_shape_change.tw");
}

#[test]
fn run_wasm_unfold_step_match() {
    check("tests/run/unfold_step_match.tw");
}

#[test]
fn run_wasm_stdlib_path() {
    check("tests/run/stdlib_path.tw");
}

#[test]
fn run_wasm_stdlib_vector_string_ext() {
    check("tests/run/stdlib_vector_string_ext.tw");
}

#[test]
fn run_wasm_stdlib_numeric_dict_ext() {
    check("tests/run/stdlib_numeric_dict_ext.tw");
}

#[test]
fn run_wasm_numeric_parsing() {
    check("tests/run/numeric_parsing.tw");
}

#[test]
fn run_wasm_twinkle_typechecker() {
    check("tests/run/twinkle_typechecker.tw");
}

#[test]
fn run_wasm_stdlib_proc() {
    check("tests/run/stdlib_proc.tw");
}

#[test]
fn run_wasm_stderr_prelude() {
    let (stdout, stderr) = twinkle::cli::run_wasm::run_wasm_capture("tests/run/stderr_prelude.tw")
        .expect("stderr prelude fixture should run");
    assert_eq!(stdout, "done\n");
    assert_eq!(stderr, "warn:bad\n");
}
