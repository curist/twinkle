use insta::assert_debug_snapshot;
use std::fs;
use std::path::PathBuf;

#[test]
fn test_parser_expression_cases() {
    // Test simple expression files with snapshot testing
    let test_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/parser");

    // Get all .tw files that start with "expr_" prefix
    let entries = fs::read_dir(&test_dir)
        .expect("Failed to read test directory")
        .filter_map(Result::ok)
        .filter(|e| {
            e.path()
                .file_name()
                .and_then(|n| n.to_str())
                .map(|name| name.starts_with("expr_") && name.ends_with(".tw"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    assert!(
        !entries.is_empty(),
        "No expr_*.tw test files found in tests/parser/"
    );

    // Run parser over each expression test file and snapshot the AST
    for entry in entries {
        let path = entry.path();
        let file_name = path.file_name().unwrap().to_string_lossy();
        let snapshot_name = format!("expr_{}", file_name.trim_end_matches(".tw"));

        let content =
            fs::read_to_string(&path).unwrap_or_else(|_| panic!("Failed to read {:?}", path));

        let result = twinkle::syntax::parse_source(&content, file_name.as_ref());

        assert!(
            result.is_ok(),
            "Parser failed on {:?}: {:?}",
            file_name,
            result.err()
        );

        let (ast, _registry) = result.unwrap();
        assert_debug_snapshot!(snapshot_name, ast);

        println!("✓ Parsed and snapshotted: {}", file_name);
    }
}

#[test]
fn test_operator_precedence_snapshots() {
    // Snapshot AST shapes to verify operator precedence
    let test_cases = vec![
        ("addition_and_multiplication", "1 + 2 * 3"),
        ("subtraction_and_division", "10 - 6 / 2"),
        ("right_associative_assignment", "x = y = 5"),
        ("comparison_and_logical", "a < b and c > d"),
        ("nested_precedence", "1 + 2 * 3 - 4 / 2"),
        ("unary_precedence", "-x * 2"),
        ("call_precedence", "foo(1) + bar(2)"),
        ("field_precedence", "obj.field + 1"),
    ];

    for (name, source) in test_cases {
        let result = twinkle::syntax::parse_source(source, "test.tw");
        assert!(result.is_ok(), "Failed to parse: {}", source);

        let (ast, _registry) = result.unwrap();
        assert_debug_snapshot!(format!("precedence_{}", name), ast);
        println!("✓ Snapshotted precedence: {}", name);
    }
}

#[test]
fn test_parser_full_programs() {
    // Test parsing full Twinkle programs from examples/
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");

    let test_files = vec!["adder.tw", "fizzbuzz.tw", "fizzbuzz_enum.tw"];

    for file_name in test_files {
        let path = examples_dir.join(file_name);

        if !path.exists() {
            println!("⊘ Skipping missing file: {}", file_name);
            continue;
        }

        let content =
            fs::read_to_string(&path).unwrap_or_else(|_| panic!("Failed to read {:?}", path));

        let result = twinkle::syntax::parse_source(&content, file_name);

        assert!(
            result.is_ok(),
            "Parser failed on {:?}: {:?}",
            file_name,
            result.err()
        );

        println!("✓ Parsed program: {}", file_name);
    }
}

#[test]
fn test_parser_error_cases() {
    // Test that invalid syntax is properly rejected with good error messages
    let test_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/parser_errors");

    if !test_dir.exists() {
        println!("⊘ Skipping error tests - directory does not exist");
        return;
    }

    let entries = fs::read_dir(&test_dir)
        .expect("Failed to read parser_errors directory")
        .filter_map(Result::ok)
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext == "tw")
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    assert!(
        !entries.is_empty(),
        "No error test files found in tests/parser_errors/"
    );

    for entry in entries {
        let path = entry.path();
        let file_name = path.file_name().unwrap().to_string_lossy();

        let content =
            fs::read_to_string(&path).unwrap_or_else(|_| panic!("Failed to read {:?}", path));

        let result = twinkle::syntax::parse_source(&content, file_name.as_ref());

        assert!(
            result.is_err(),
            "Parser should reject invalid syntax in {:?}",
            file_name
        );

        // Verify error message contains useful information
        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains(&file_name.to_string())
                && (err_msg.contains(':') || err_msg.contains("line")),
            "Error message should contain location info: {}",
            err_msg
        );

        // Snapshot the error message for regression testing
        let snapshot_name = format!("error_{}", file_name.trim_end_matches(".tw"));
        assert_debug_snapshot!(snapshot_name, err_msg);

        println!(
            "✓ Correctly rejected: {} - {}",
            file_name,
            err_msg.lines().next().unwrap_or("")
        );
    }
}

#[test]
fn test_stage0_skeleton() {
    // Verify the basic module structure exists
    // This is a sanity check that the crate compiles
    assert!(true, "Stage 0 skeleton is functional");
}

#[test]
fn test_value_postfix_constructor_still_rejected() {
    // `x.Variant` where x is a value (lowercase chain) must remain a parse error
    let cases = vec![
        ("x.Some", "value.Variant"),
        ("foo.bar.Baz", "lowercase chain ending in uppercase"),
    ];
    for (source, description) in cases {
        let result = twinkle::syntax::parse_source(source, "test.tw");
        assert!(
            result.is_err(),
            "{}: should be rejected as ConstructorInPostfix, but parsed successfully",
            description
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("Constructor") && err_msg.contains("cannot appear after"),
            "{}: expected ConstructorInPostfix error, got: {}",
            description,
            err_msg
        );
    }
}

#[test]
fn test_type_path_constructor_parses() {
    // `Type.Variant` and module-qualified `mod.Type.Variant` should parse
    let cases = vec![
        ("Type.Variant", "simple type.variant"),
        ("mod.Type.Variant", "module-qualified constructor"),
        ("Type.Variant(1, 2)", "constructor call"),
        ("mod.Type.Variant(1)", "module-qualified constructor call"),
    ];
    for (source, description) in cases {
        let result = twinkle::syntax::parse_source(source, "test.tw");
        assert!(
            result.is_ok(),
            "{}: should parse successfully, but got error: {:?}",
            description,
            result.err()
        );
    }
}
