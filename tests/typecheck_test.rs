use std::fs;
use std::path::PathBuf;

#[test]
fn test_typecheck_pass_cases() {
    let test_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/typecheck/pass");

    // Check if directory exists
    if !test_dir.exists() {
        panic!(
            "Test directory does not exist: {}",
            test_dir.display()
        );
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
        let (type_env, value_env) = match twinkle::types::Resolver::resolve(&ast) {
            Ok(envs) => envs,
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
        match twinkle::types::TypeChecker::check_module(&ast, type_env.clone(), value_env) {
            Ok(()) => {
                passed += 1;
            }
            Err(errors) => {
                let error_msg = errors
                    .iter()
                    .map(|e| e.format(&registry, Some(&type_env)))
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
        eprintln!("\n❌ Failed {} out of {} tests:\n", failed.len(), test_count);
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
        panic!(
            "Test directory does not exist: {}",
            test_dir.display()
        );
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
        let result = twinkle::types::Resolver::resolve(&ast).and_then(|(te, ve)| {
            twinkle::types::TypeChecker::check_module(&ast, te, ve)
        });

        match result {
            Err(_) => {
                // Expected to fail
                passed += 1;
            }
            Ok(()) => {
                failed.push(format!(
                    "{}: Expected type checking to fail, but it succeeded",
                    file_name
                ));
            }
        }
    }

    if !failed.is_empty() {
        eprintln!("\n❌ Failed {} out of {} tests:\n", failed.len(), test_count);
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
