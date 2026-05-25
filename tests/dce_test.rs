use std::path::PathBuf;

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/run")
        .join(name)
        .to_string_lossy()
        .to_string()
}

#[test]
fn dce_removes_unused_imported_functions() {
    let path = fixture("dce_test/main.tw");
    let (core_module, _) =
        twinkle::module::compile_entry(&path).expect("compile_entry should succeed");

    let func_names: Vec<&str> = core_module
        .functions
        .iter()
        .map(|f| f.name.as_str())
        .collect();

    // used_fn should be kept (called from main)
    assert!(
        func_names
            .iter()
            .any(|n| n.ends_with(".used_fn") || *n == "used_fn"),
        "used_fn should be reachable. Functions: {:?}",
        func_names
    );

    // unused_fn and also_unused should be eliminated
    assert!(
        !func_names
            .iter()
            .any(|n| n.ends_with(".unused_fn") || *n == "unused_fn"),
        "unused_fn should be eliminated by DCE. Functions: {:?}",
        func_names
    );
    assert!(
        !func_names
            .iter()
            .any(|n| n.ends_with(".also_unused") || *n == "also_unused"),
        "also_unused should be eliminated by DCE. Functions: {:?}",
        func_names
    );
}

#[test]
fn dce_renumbers_funcids_compactly() {
    let path = fixture("dce_test/main.tw");
    let (core_module, _) =
        twinkle::module::compile_entry(&path).expect("compile_entry should succeed");

    let ids: Vec<u32> = core_module.functions.iter().map(|f| f.func_id.0).collect();

    // IDs should be compact starting from USER_FUNC_START (41)
    let mut sorted_ids = ids.clone();
    sorted_ids.sort();
    let expected: Vec<u32> = (41..41 + sorted_ids.len() as u32).collect();
    assert_eq!(
        sorted_ids, expected,
        "FuncIds should be compact sequential from 41. Got: {:?}",
        ids
    );
}
