use std::collections::HashMap;
use std::path::PathBuf;

#[test]
fn compile_entry_from_source_map_compiles_multimodule_program() {
    let project_root = PathBuf::from("/virtual/project");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let math = project_root.join("math.tw");

    let mut sources = HashMap::new();
    sources.insert(
        entry.clone(),
        r#"
use math

println("${math.answer()}")
"#
        .to_string(),
    );
    sources.insert(
        math,
        r#"
pub fn answer() Int {
  42
}
"#
        .to_string(),
    );

    let (core_module, _registry) = twinkle::module::compile_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("source-map compile should succeed");
    let mut interp = twinkle::interp::Interpreter::new(core_module, Vec::<u8>::new());
    interp.run().expect("compiled module should run");
    let output = String::from_utf8(interp.into_output()).expect("utf8 output");
    assert_eq!(output, "42\n");
}
