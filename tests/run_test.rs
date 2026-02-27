use std::fs;

/// Parse the expected output from the leading comment block in a `.tw` file.
/// Recognises lines of the form `// Expected output:` followed by `//   <line>`.
fn parse_expected(source: &str) -> Vec<String> {
    let mut lines = source.lines();
    // Find the "// Expected output:" header
    let found = lines.by_ref().any(|l| l.trim() == "// Expected output:");
    if !found {
        return vec![];
    }
    let mut result = Vec::new();
    for line in lines {
        if let Some(rest) = line.strip_prefix("//   ") {
            result.push(rest.to_string());
        } else if line.starts_with("//") {
            // blank comment line
            result.push("".to_string());
        } else {
            break;
        }
    }
    result
}

/// Run a `.tw` file through the interpreter and return captured stdout.
fn run_and_capture(path: &str) -> anyhow::Result<String> {
    let (core_module, _registry) = twinkle::module::compile_entry(path)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let mut interp = twinkle::interp::Interpreter::new(core_module, Vec::<u8>::new());
    interp.run()?;
    let bytes = interp.into_output();
    Ok(String::from_utf8(bytes).expect("interpreter output is valid UTF-8"))
}

fn check(path: &str) {
    let source = fs::read_to_string(path).expect("test file exists");
    let expected = parse_expected(&source);
    assert!(
        !expected.is_empty(),
        "No '// Expected output:' block in {path}"
    );
    let actual_raw = run_and_capture(path).expect("interpreter should not error");
    let actual: Vec<&str> = actual_raw.lines().collect();
    assert_eq!(
        actual, expected,
        "Output mismatch for {path}\nExpected:\n{}\nActual:\n{}",
        expected.join("\n"), actual_raw
    );
}

#[test]
fn hello() {
    check("tests/run/hello.tw");
}

#[test]
fn arithmetic() {
    check("tests/run/arithmetic.tw");
}

#[test]
fn strings() {
    check("tests/run/strings.tw");
}

#[test]
fn control_flow() {
    check("tests/run/control_flow.tw");
}

#[test]
fn loops() {
    check("tests/run/loops.tw");
}

#[test]
fn collect() {
    check("tests/run/collect.tw");
}

#[test]
fn records() {
    check("tests/run/records.tw");
}

#[test]
fn arrays() {
    check("tests/run/arrays.tw");
}

#[test]
fn closures() {
    check("tests/run/closures.tw");
}

#[test]
fn multi_module() {
    check("tests/run/multi_module/main.tw");
}

#[test]
fn variant_collision() {
    check("tests/run/variant_collision.tw");
}

#[test]
fn range() {
    check("tests/run/range.tw");
}

#[test]
fn dicts() {
    check("tests/run/dicts.tw");
}

#[test]
fn strings_escape() {
    check("tests/run/strings_escape.tw");
}

#[test]
fn for_break() {
    check("tests/run/for_break.tw");
}

#[test]
fn type_alias() {
    check("tests/run/type_alias.tw");
}

#[test]
fn mutual_recursion() {
    check("tests/run/mutual_recursion.tw");
}

#[test]
fn result_void() {
    check("tests/run/result_void.tw");
}

#[test]
fn capability_records() {
    check("tests/run/capability_records.tw");
}

#[test]
fn nested_field_update() {
    check("tests/run/nested_field_update.tw");
}

#[test]
fn array_methods() {
    check("tests/run/array_methods.tw");
}

#[test]
fn dict_methods() {
    check("tests/run/dict_methods.tw");
}

#[test]
fn string_methods() {
    check("tests/run/string_methods.tw");
}

#[test]
fn multi_module_alias() {
    check("tests/run/multi_module_alias/main.tw");
}

#[test]
fn generic_types() {
    check("tests/run/generic_types.tw");
}

#[test]
fn method_chaining() {
    check("tests/run/method_chaining.tw");
}

#[test]
fn iterator() {
    check("tests/run/iterator.tw");
}

#[test]
fn iterator_advanced() {
    check("tests/run/iterator_advanced.tw");
}

#[test]
fn empty_array() {
    check("tests/run/empty_array.tw");
}
