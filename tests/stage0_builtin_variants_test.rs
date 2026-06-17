use std::path::PathBuf;

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/run")
        .join(name)
        .to_string_lossy()
        .to_string()
}

#[test]
fn qualified_builtin_option_result_variants_build() {
    let path = fixture("qualified_builtin_variants.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    assert!(wat.contains("(module"));
}
