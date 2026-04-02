use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use twinkle::types::TypeError;
use twinkle::types::env::{TypeEnv, ValueEnv};

/// Built-in module aliases recognised by the type checker.
fn builtin_aliases() -> HashSet<String> {
    [
        "Cell", "Dict", "Iterator", "Vector", "String", "Int", "Float", "Bool",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[test]
fn test_typecheck_pass_cases() {
    let test_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/typecheck/pass");

    // Check if directory exists
    if !test_dir.exists() {
        panic!("Test directory does not exist: {}", test_dir.display());
    }

    let mut test_count = 0;
    let mut passed = 0;
    let mut failed = Vec::new();

    for entry in fs::read_dir(&test_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("tw") {
            continue;
        }

        let file_name = path.file_name().unwrap().to_str().unwrap();
        let content = fs::read_to_string(&path).unwrap();

        test_count += 1;

        // Parse
        let (ast, registry) = match twinkle::syntax::parse_source(&content, file_name) {
            Ok(result) => result,
            Err(e) => {
                failed.push(format!("{}: Parse failed: {}", file_name, e));
                continue;
            }
        };

        // Resolve names
        let resolved =
            match twinkle::types::Resolver::resolve(&ast, TypeEnv::new(), ValueEnv::new()) {
                Ok(r) => r,
                Err(errors) => {
                    let error_msg = errors
                        .iter()
                        .map(|e| e.format(&registry, None))
                        .collect::<Vec<_>>()
                        .join("\n");
                    failed.push(format!(
                        "{}: Name resolution failed:\n{}",
                        file_name, error_msg
                    ));
                    continue;
                }
            };

        // Type check
        match twinkle::types::TypeChecker::check_module(
            &ast,
            resolved.type_env.clone(),
            resolved.value_env,
            builtin_aliases(),
        ) {
            Ok(_typed) => {
                passed += 1;
            }
            Err(errors) => {
                let error_msg = errors
                    .iter()
                    .map(|e| e.format(&registry, Some(&resolved.type_env)))
                    .collect::<Vec<_>>()
                    .join("\n");
                failed.push(format!(
                    "{}: Type checking failed:\n{}",
                    file_name, error_msg
                ));
            }
        }
    }

    if !failed.is_empty() {
        eprintln!(
            "\n❌ Failed {} out of {} tests:\n",
            failed.len(),
            test_count
        );
        for failure in &failed {
            eprintln!("{}\n", failure);
        }
        panic!(
            "Type checker failed on {} passing test case(s)",
            failed.len()
        );
    }

    println!("✓ All {} passing test cases succeeded", passed);
}

#[test]
fn test_typecheck_fail_cases() {
    let test_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/typecheck/fail");

    // Check if directory exists
    if !test_dir.exists() {
        panic!("Test directory does not exist: {}", test_dir.display());
    }

    let mut test_count = 0;
    let mut passed = 0;
    let mut failed = Vec::new();

    for entry in fs::read_dir(&test_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("tw") {
            continue;
        }

        let file_name = path.file_name().unwrap().to_str().unwrap();
        let content = fs::read_to_string(&path).unwrap();

        test_count += 1;

        // Parse
        let (ast, _registry) = match twinkle::syntax::parse_source(&content, file_name) {
            Ok(result) => result,
            Err(_) => {
                // Parse errors are also valid failures
                passed += 1;
                continue;
            }
        };

        // Resolve names and type check
        let result = twinkle::types::Resolver::resolve(&ast, TypeEnv::new(), ValueEnv::new())
            .and_then(|r| {
                twinkle::types::TypeChecker::check_module(
                    &ast,
                    r.type_env,
                    r.value_env,
                    builtin_aliases(),
                )
            });

        match result {
            Err(_) => {
                // Expected to fail
                passed += 1;
            }
            Ok(_typed) => {
                failed.push(format!(
                    "{}: Expected type checking to fail, but it succeeded",
                    file_name
                ));
            }
        }
    }

    if !failed.is_empty() {
        eprintln!(
            "\n❌ Failed {} out of {} tests:\n",
            failed.len(),
            test_count
        );
        for failure in &failed {
            eprintln!("{}\n", failure);
        }
        panic!(
            "Type checker incorrectly accepted {} failing test case(s)",
            failed.len()
        );
    }

    println!("✓ All {} failing test cases failed as expected", passed);
}

/// Run a program through parse → resolve → typecheck and return all formatted error messages.
fn check_errors(src: &str) -> Vec<String> {
    let (ast, registry) = twinkle::syntax::parse_source(src, "test.tw").expect("parse failed");
    let resolved = match twinkle::types::Resolver::resolve(&ast, TypeEnv::new(), ValueEnv::new()) {
        Ok(r) => r,
        Err(errors) => {
            return errors.iter().map(|e| e.format(&registry, None)).collect();
        }
    };
    match twinkle::types::TypeChecker::check_module(
        &ast,
        resolved.type_env.clone(),
        resolved.value_env,
        builtin_aliases(),
    ) {
        Ok(_) => vec![],
        Err(errors) => errors
            .iter()
            .map(|e| e.format(&registry, Some(&resolved.type_env)))
            .collect(),
    }
}

#[test]
fn test_error_note_regular_call() {
    // Non-generic call: wrong arg type should include argument position and function name
    let src = "fn add(x: Int, y: Int) Int { x + y }\nadd(1, \"hello\")";
    let errors = check_errors(src);
    assert!(!errors.is_empty(), "expected a type error");
    let joined = errors.join("\n");
    assert!(
        joined.contains("argument 2"),
        "expected 'argument 2' in error:\n{}",
        joined
    );
    assert!(
        joined.contains("add"),
        "expected function name 'add' in error:\n{}",
        joined
    );
}

#[test]
fn test_error_note_generic_call() {
    // Generic call: type variable conflicts across arguments
    let src = "fn first<A>(a: A, b: A) A { a }\nfirst(1, \"hello\")";
    let errors = check_errors(src);
    assert!(!errors.is_empty(), "expected a type error");
    let joined = errors.join("\n");
    assert!(
        joined.contains("argument 2"),
        "expected 'argument 2' in error:\n{}",
        joined
    );
    assert!(
        joined.contains("first"),
        "expected function name 'first' in error:\n{}",
        joined
    );
}

#[test]
fn test_failed_let_initializer_does_not_emit_undefined_variable_cascade() {
    let src = r#"
fn main() {
    s: Int = "oops"
    println("${s}")
}
main()
"#;
    let errors = check_errors(src);
    assert!(!errors.is_empty(), "expected a type error");
    let joined = errors.join("\n");
    assert!(
        joined.contains("Type mismatch"),
        "expected primary type mismatch error:\n{}",
        joined
    );
    assert!(
        !joined.contains("Undefined variable: s"),
        "unexpected cascade error for 's':\n{}",
        joined
    );
}

#[test]
fn test_failed_top_level_let_initializer_does_not_emit_undefined_variable_cascade() {
    let src = r#"
t: Int = "oops"
println("${t}")
"#;
    let errors = check_errors(src);
    assert!(!errors.is_empty(), "expected a type error");
    let joined = errors.join("\n");
    assert!(
        joined.contains("Type mismatch"),
        "expected primary type mismatch error:\n{}",
        joined
    );
    assert!(
        !joined.contains("Undefined variable: t"),
        "unexpected cascade error for 't':\n{}",
        joined
    );
}

#[test]
fn test_byte_literal_out_of_range_reports_note() {
    let src = "b: Byte = 256";
    let errors = check_errors(src);
    assert!(!errors.is_empty(), "expected a type error");
    let joined = errors.join("\n");
    assert!(
        joined.contains("out of range for Byte (0..255)"),
        "expected byte range note in error:\n{}",
        joined
    );
}

// Closure capture-by-value semantics (spec §7.7).
// A closure must capture the value at definition time; later rebinding of
// the source variable must not affect the captured value.
//
#[test]
fn test_closure_capture_by_value() {
    // Use top-level statements (not `fn main`) so interpreter execution path
    // in tests matches other run fixtures.
    let src = r#"
fn main() {
    acc := 0
    f := fn() Int { acc }
    acc = acc + 1
    println("${f()}")
    println("${acc}")
}
main()
"#;
    let path = std::env::temp_dir().join(format!(
        "twinkle_capture_by_value_{}.tw",
        std::process::id()
    ));
    fs::write(&path, src).expect("write temp tw source");
    let (core_module, _registry) =
        twinkle::module::compile_entry(path.to_str().expect("temp path utf8"))
            .expect("compile capture_by_value snippet");
    let mut interp = twinkle::interp::Interpreter::new(core_module, Vec::<u8>::new());
    interp.run().expect("run capture_by_value snippet");
    let bytes = interp.into_output();
    let output = String::from_utf8(bytes).expect("interpreter output is valid UTF-8");
    let _ = fs::remove_file(path);
    assert_eq!(output, "0\n1\n");
}

#[test]
fn test_missing_method_signature_reports_type_error_not_panic() {
    let src = r#"
fn main() Int {
    xs := [1]
    f := xs.push
    f(2).len()
}
main()
"#;

    let (ast, _registry) = twinkle::syntax::parse_source(src, "test.tw").expect("parse failed");
    let resolved = twinkle::types::Resolver::resolve(&ast, TypeEnv::new(), ValueEnv::new())
        .expect("resolve failed");

    let mut broken_value_env = resolved.value_env;
    broken_value_env.remove_function("Vector.append");

    let result = twinkle::types::TypeChecker::check_module(
        &ast,
        resolved.type_env,
        broken_value_env,
        builtin_aliases(),
    );

    let errors = result.expect_err("expected a type error, not success");
    assert!(
        errors.iter().any(|err| {
            matches!(
                err,
                TypeError::UndefinedVariable { name, .. } if name == "Vector.append"
            )
        }),
        "expected UndefinedVariable for Vector.append, got: {:?}",
        errors
    );
}

#[test]
fn test_eq_ne_variant_propagation_no_duplicate_errors() {
    // `kind == .Use` should type-check cleanly when kind has a known sum type.
    let src = r#"
type TokenKind = { Use, Fn, Ident }
fn check(kind: TokenKind) Bool { kind == .Use }
"#;
    let errors = check_errors(src);
    assert!(
        errors.is_empty(),
        "expected no errors for `kind == .Use`, got:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_eq_both_shorthand_variants_still_fails() {
    // Both sides context-free should still produce an error, not silently pass.
    let src = r#"
type TokenKind = { Use, Fn, Ident }
fn check() Bool { .Use == .Fn }
"#;
    let errors = check_errors(src);
    assert!(
        !errors.is_empty(),
        "expected errors for `.Use == .Fn` without context"
    );
    // Should get exactly the right errors, not duplicated cascades
    assert!(
        errors.len() <= 2,
        "expected at most 2 errors (one per side), got {}:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

#[test]
fn test_eq_wrong_variant_payload_fails() {
    let src = r#"
fn check(x: Int?) Bool { x == .Some("hello") }
"#;
    let errors = check_errors(src);
    assert!(
        !errors.is_empty(),
        "expected type error for wrong payload type in .Some(\"hello\")"
    );
}
