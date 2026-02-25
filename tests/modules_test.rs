use std::path::PathBuf;

fn modules_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/modules")
}

fn check(relative: &str) -> Result<(), String> {
    let path = modules_dir().join(relative);
    twinkle::module::check_entry(path.to_str().unwrap()).map(|_| ()).map_err(|e| e.to_string())
}

fn lower(relative: &str) -> Result<(), String> {
    let path = modules_dir().join(relative);
    twinkle::module::compile_entry(path.to_str().unwrap())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// basic cross-module type and function usage (use math)
#[test]
fn test_module_simple_check() {
    let result = check("simple/main.tw");
    assert!(result.is_ok(), "Expected check to pass: {:?}", result.err());
}

#[test]
fn test_module_simple_lower() {
    let result = lower("simple/main.tw");
    assert!(result.is_ok(), "Expected lower to pass: {:?}", result.err());
}

/// use-as alias: `use geometry as pt`
#[test]
fn test_module_alias_check() {
    let result = check("alias/main.tw");
    assert!(result.is_ok(), "Expected check to pass: {:?}", result.err());
}

#[test]
fn test_module_alias_lower() {
    let result = lower("alias/main.tw");
    assert!(result.is_ok(), "Expected lower to pass: {:?}", result.err());
}

/// accessing a private function should be rejected
#[test]
fn test_module_private_access_rejected() {
    let result = check("private/main.tw");
    assert!(
        result.is_err(),
        "Expected private symbol access to fail, but check succeeded"
    );
    let err = result.unwrap_err();
    // Error should mention the private function
    assert!(
        err.contains("private_fn") || err.contains("private") || err.contains("not found"),
        "Unexpected error message: {}",
        err
    );
}

/// circular imports should be detected and reported
#[test]
fn test_module_circular_import_detected() {
    let result = check("circular/a.tw");
    assert!(
        result.is_err(),
        "Expected circular import to fail, but check succeeded"
    );
    let err = result.unwrap_err();
    assert!(
        err.to_lowercase().contains("circular"),
        "Expected 'circular' in error message, got: {}",
        err
    );
}

/// private type from an imported module must be inaccessible
#[test]
fn test_module_private_type_rejected() {
    let result = check("private_type/main.tw");
    assert!(
        result.is_err(),
        "Expected private type access to fail, but check succeeded"
    );
}

/// inherent method call on a type from a module imported via 'as' alias
#[test]
fn test_module_alias_inherent_method_check() {
    let result = check("alias_method/main.tw");
    assert!(
        result.is_ok(),
        "Expected inherent method via alias to pass: {:?}",
        result.err()
    );
}

#[test]
fn test_module_alias_inherent_method_lower() {
    let result = lower("alias_method/main.tw");
    assert!(
        result.is_ok(),
        "Expected inherent method via alias lowering to pass: {:?}",
        result.err()
    );
}

/// two imported modules with identically named functions — qualified calls must resolve correctly
#[test]
fn test_module_name_collision_check() {
    let result = check("name_collision/main.tw");
    assert!(
        result.is_ok(),
        "Expected name collision (qualified calls) to pass: {:?}",
        result.err()
    );
}

#[test]
fn test_module_name_collision_lower() {
    let result = lower("name_collision/main.tw");
    assert!(
        result.is_ok(),
        "Expected name collision lower to pass: {:?}",
        result.err()
    );
}

/// cross-module inherent method desugaring: p.translate(1, 2) → point.translate(p, 1, 2)
#[test]
fn test_module_inherent_method_check() {
    let result = check("inherent_method/main.tw");
    assert!(
        result.is_ok(),
        "Expected inherent method resolution to pass: {:?}",
        result.err()
    );
}

#[test]
fn test_module_inherent_method_lower() {
    let result = lower("inherent_method/main.tw");
    assert!(
        result.is_ok(),
        "Expected inherent method lowering to pass: {:?}",
        result.err()
    );
}

/// two modules with same-named types: qualified access resolves each to the correct TypeId
#[test]
fn test_module_type_collision_qualified_check() {
    let result = check("type_collision/main.tw");
    assert!(
        result.is_ok(),
        "Expected qualified access to same-named types to pass: {:?}",
        result.err()
    );
}

#[test]
fn test_module_type_collision_qualified_lower() {
    let result = lower("type_collision/main.tw");
    assert!(
        result.is_ok(),
        "Expected qualified access to same-named types to lower: {:?}",
        result.err()
    );
}

/// bare (unqualified) use of a type name that exists in two imports must be rejected
#[test]
fn test_module_type_collision_bare_rejected() {
    let result = check("type_collision/main_bare.tw");
    assert!(
        result.is_err(),
        "Expected bare ambiguous type name to fail, but check succeeded"
    );
}

/// multi-segment module path: `use math.vec` loads math/vec.tw;
/// the module alias is the last segment so types are accessed as `vec.Vec2`
#[test]
fn test_module_multiseg_check() {
    let result = check("multiseg/main.tw");
    assert!(
        result.is_ok(),
        "Expected multi-segment module path to pass: {:?}",
        result.err()
    );
}

#[test]
fn test_module_multiseg_lower() {
    let result = lower("multiseg/main.tw");
    assert!(
        result.is_ok(),
        "Expected multi-segment module lower to pass: {:?}",
        result.err()
    );
}
