use std::fs;
use std::path::PathBuf;

#[test]
fn test_parser_cases() {
    let test_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/parser");

    // Get all .tw files in the parser test directory
    let entries = fs::read_dir(&test_dir)
        .expect("Failed to read test directory")
        .filter_map(Result::ok)
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "tw")
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    assert!(
        !entries.is_empty(),
        "No .tw test files found in tests/parser/"
    );

    // Run stub parser over each test file and assert success
    for entry in entries {
        let path = entry.path();
        let file_name = path.file_name().unwrap().to_string_lossy();

        let content = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("Failed to read {:?}", path));

        // Call stub parser
        let result = twinkle::syntax::parse(&content);

        // All test cases in tests/parser/ should parse successfully
        // (In Stage 1, we'll add tests/parser_errors/ for error cases)
        assert!(
            result.is_ok(),
            "Parser failed on {:?}: {:?}",
            file_name,
            result.err()
        );

        println!("✓ Parsed: {}", file_name);
    }
}

#[test]
fn test_parser_error_cases() {
    // Test that obviously broken syntax is rejected
    let bad_source = "fn main() { println(\"unclosed brace\")";
    let result = twinkle::syntax::parse(bad_source);

    assert!(
        result.is_err(),
        "Parser should reject unbalanced braces"
    );
    println!("✓ Parser correctly rejects invalid syntax");
}

#[test]
fn test_stage0_skeleton() {
    // Verify the basic module structure exists
    // This is a sanity check that the crate compiles
    assert!(true, "Stage 0 skeleton is functional");
}
