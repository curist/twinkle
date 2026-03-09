use std::collections::HashMap;
use std::path::PathBuf;
use twinkle::query::cache::reset_global_cache;

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

#[test]
fn compile_entry_from_source_map_rejects_user_direct_host_intrinsic_calls() {
    let project_root = PathBuf::from("/virtual/project");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");

    let mut sources = HashMap::new();
    sources.insert(
        entry.clone(),
        r#"
println(__host_cwd())
"#
        .to_string(),
    );

    let err = twinkle::module::compile_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect_err("user module should not be allowed to call __host_* directly");

    let msg = err.to_string();
    assert!(
        msg.contains("Undefined variable: __host_cwd"),
        "unexpected error:\n{}",
        msg
    );
}

#[test]
fn compile_entry_from_source_map_allows_stdlib_host_intrinsic_calls() {
    let project_root = PathBuf::from("/virtual/project");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let stdlib_fs = stdlib_root.join("fs.tw");

    let mut sources = HashMap::new();
    sources.insert(
        entry.clone(),
        r#"
use @std.fs

println(fs.cwd())
"#
        .to_string(),
    );
    sources.insert(
        stdlib_fs,
        r#"
pub fn cwd() String {
  __host_cwd()
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
    .expect("stdlib module should be allowed to call __host_*");

    let mut interp = twinkle::interp::Interpreter::new(core_module, Vec::<u8>::new());
    interp.run().expect("compiled module should run");
    let output = String::from_utf8(interp.into_output()).expect("utf8 output");
    assert!(!output.trim().is_empty(), "cwd output should not be empty");
}

#[test]
fn source_map_typecheck_cache_does_not_cross_internal_mode_boundary() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/project");
    let entry = project_root.join("main.tw");

    let mut sources = HashMap::new();
    sources.insert(
        entry.clone(),
        r#"
println(__host_cwd())
"#
        .to_string(),
    );

    let permissive_stdlib_root = project_root.clone();
    twinkle::module::compile_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &permissive_stdlib_root,
    )
    .expect("first compile should treat module as internal and succeed");

    let strict_stdlib_root = project_root.join("stdlib");
    let err = twinkle::module::compile_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &strict_stdlib_root,
    )
    .expect_err("second compile should not reuse permissive typecheck cache entry");

    let msg = err.to_string();
    assert!(
        msg.contains("Undefined variable: __host_cwd"),
        "unexpected error:\n{}",
        msg
    );
}
