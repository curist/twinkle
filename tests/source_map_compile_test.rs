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
fn compile_entry_from_source_map_rejects_intrinsic_arity_drift() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/intrinsic_signature_validation");
    let stdlib_root = project_root.join("stdlib");
    let prelude_root = project_root.join("prelude");
    let entry = project_root.join("main.tw");
    let prelude_vector = prelude_root.join("vector.tw");

    let mut sources = HashMap::new();
    sources.insert(entry.clone(), "println(\"ok\")\n".to_string());
    sources.insert(
        prelude_vector,
        r#"
pub fn push(xs: Vector<Int>) Vector<Int> {
  xs
}
"#
        .to_string(),
    );

    let err = twinkle::module::compile_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect_err("mismatched intrinsic signature should fail validation");

    let msg = err.to_string();
    assert!(
        msg.contains("intrinsic signature validation failed"),
        "unexpected error:\n{}",
        msg
    );
    assert!(msg.contains("Vector.push"), "unexpected error:\n{}", msg);
    assert!(msg.contains("arity mismatch"), "unexpected error:\n{}", msg);
}

#[test]
fn compile_entry_from_source_map_accepts_new_prelude_method_without_rust_changes() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/prelude_method_drift");
    let stdlib_root = project_root.join("stdlib");
    let prelude_root = project_root.join("prelude");
    let entry = project_root.join("main.tw");
    let prelude_vector = prelude_root.join("vector.tw");

    let mut sources = HashMap::new();
    sources.insert(
        entry.clone(),
        r#"
xs := [1, 2, 3]
println("${xs.first_or(0)}")
println("${Vector.first_or([], 99)}")
"#
        .to_string(),
    );
    sources.insert(
        prelude_vector,
        r#"
pub fn first_or<A>(xs: Vector<A>, fallback: A) A {
  case xs.get(0) {
    .Some(v) => v,
    .None => fallback,
  }
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
    .expect("new prelude method should compile without Rust-side registration changes");

    let mut interp = twinkle::interp::Interpreter::new(core_module, Vec::<u8>::new());
    interp.run().expect("compiled module should run");
    let output = String::from_utf8(interp.into_output()).expect("utf8 output");
    assert_eq!(output, "1\n99\n");
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

#[test]
fn compile_entry_from_source_map_hides_internal_prelude_aliases() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/prelude_alias_visibility");
    let stdlib_root = project_root.join("stdlib");
    let prelude_root = project_root.join("prelude");
    let entry = project_root.join("main.tw");
    let prelude_vector = prelude_root.join("vector.tw");

    let mut sources = HashMap::new();
    sources.insert(
        entry.clone(),
        r#"
xs := [1, 2]
ys := __prelude_vector.map(xs, fn(x: Int) Int { x * 2 })
println("${ys.len()}")
"#
        .to_string(),
    );
    sources.insert(
        prelude_vector,
        r#"
pub fn map<A, B>(xs: Vector<A>, f: fn(A) B) Vector<B> {
  collect x in xs { f(x) }
}
"#
        .to_string(),
    );

    let err = twinkle::module::compile_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect_err("internal prelude aliases must not be user-visible");

    let msg = err.to_string();
    assert!(
        msg.contains("Undefined variable: __prelude_vector"),
        "unexpected error:\n{}",
        msg
    );
}

#[test]
fn compile_entry_from_source_map_does_not_leak_prelude_into_stdlib_by_import_order() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/prelude_isolation");
    let stdlib_root = project_root.join("stdlib");
    let prelude_root = project_root.join("prelude");
    let user = project_root.join("user.tw");
    let stdlib_foo = stdlib_root.join("foo.tw");
    let prelude_vector = prelude_root.join("vector.tw");
    let entry = project_root.join("main.tw");

    let mut shared_sources = HashMap::new();
    shared_sources.insert(
        user,
        r#"
pub fn ok() Int {
  xs := [1, 2]
  ys := xs.map(fn(x: Int) Int { x + 1 })
  ys.len()
}
"#
        .to_string(),
    );
    shared_sources.insert(
        stdlib_foo,
        r#"
pub fn broken() Int {
  xs := [1, 2]
  ys := xs.map(fn(x: Int) Int { x + 1 })
  ys.len()
}
"#
        .to_string(),
    );
    shared_sources.insert(
        prelude_vector,
        r#"
pub fn map<A, B>(xs: Vector<A>, f: fn(A) B) Vector<B> {
  collect x in xs { f(x) }
}
"#
        .to_string(),
    );

    for main_src in [
        r#"
use user
use @std.foo

println("${foo.broken()}")
"#,
        r#"
use @std.foo
use user

println("${foo.broken()}")
"#,
    ] {
        let mut sources = shared_sources.clone();
        sources.insert(entry.clone(), main_src.to_string());

        let err = twinkle::module::compile_entry_from_source_map(
            &entry,
            &sources,
            &project_root,
            &stdlib_root,
        )
        .expect_err("stdlib module should not see auto-prelude methods");
        let msg = err.to_string();
        assert!(
            msg.contains("Vector has no method 'map'"),
            "unexpected error:\n{}",
            msg
        );
    }
}

#[test]
fn compile_entry_from_source_map_ignores_nested_prelude_signature_modules() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/prelude_nested_signatures");
    let stdlib_root = project_root.join("stdlib");
    let prelude_root = project_root.join("prelude");
    let entry = project_root.join("main.tw");
    let prelude_vector = prelude_root.join("vector.tw");
    let prelude_signature_cell = prelude_root.join("signatures").join("cell.tw");

    let mut sources = HashMap::new();
    sources.insert(entry.clone(), "println(\"ok\")\n".to_string());
    sources.insert(
        prelude_vector,
        r#"
pub fn map<A, B>(xs: Vector<A>, f: fn(A) B) Vector<B> {
  collect x in xs { f(x) }
}
"#
        .to_string(),
    );
    // This file intentionally contains type errors and must not be auto-imported
    // as a prelude module by source-map compilation.
    sources.insert(
        prelude_signature_cell,
        r#"
pub fn bad<T>(value: T) Cell<T> {
  value
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
    .expect("nested prelude signature modules should be ignored for auto-import");

    let mut interp = twinkle::interp::Interpreter::new(core_module, Vec::<u8>::new());
    interp.run().expect("compiled module should run");
    let output = String::from_utf8(interp.into_output()).expect("utf8 output");
    assert_eq!(output, "ok\n");
}

#[test]
fn compile_entry_from_source_map_relative_import_nested_module() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/relative_nested");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let app = project_root.join("lib").join("app.tw");
    let helper = project_root.join("lib").join("helper.tw");

    let mut sources = HashMap::new();
    sources.insert(
        entry.clone(),
        r#"
use lib.app

println(app.run())
"#
        .to_string(),
    );
    sources.insert(
        app,
        r#"
use .helper

pub fn run() String {
    helper.greet("world")
}
"#
        .to_string(),
    );
    sources.insert(
        helper,
        r#"
pub fn greet(name: String) String {
    "hello ${name}"
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
    .expect("relative import from nested module should compile");

    let mut interp = twinkle::interp::Interpreter::new(core_module, Vec::<u8>::new());
    interp.run().expect("compiled module should run");
    let output = String::from_utf8(interp.into_output()).expect("utf8 output");
    assert_eq!(output, "hello world\n");
}

#[test]
fn compile_entry_from_source_map_relative_import_from_root_module() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/relative_root");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let util = project_root.join("util.tw");

    let mut sources = HashMap::new();
    sources.insert(
        entry.clone(),
        r#"
use .util

println("${util.double(21)}")
"#
        .to_string(),
    );
    sources.insert(
        util,
        r#"
pub fn double(x: Int) Int {
    x * 2
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
    .expect("relative import from root module should compile");

    let mut interp = twinkle::interp::Interpreter::new(core_module, Vec::<u8>::new());
    interp.run().expect("compiled module should run");
    let output = String::from_utf8(interp.into_output()).expect("utf8 output");
    assert_eq!(output, "42\n");
}

#[test]
fn compile_entry_from_source_map_relative_and_stdlib_imports_coexist() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/relative_stdlib");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("lib").join("app.tw");
    let helper = project_root.join("lib").join("helper.tw");
    let stdlib_proc = stdlib_root.join("proc.tw");

    let mut sources = HashMap::new();
    sources.insert(
        entry.clone(),
        r#"
use .helper
use @std.proc

println(helper.greet("relative"))
"#
        .to_string(),
    );
    sources.insert(
        helper,
        r#"
pub fn greet(name: String) String {
    "hi ${name}"
}
"#
        .to_string(),
    );
    sources.insert(
        stdlib_proc,
        r#"
pub fn args() Vector<String> {
    []
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
    .expect("relative + stdlib imports should coexist");

    let mut interp = twinkle::interp::Interpreter::new(core_module, Vec::<u8>::new());
    interp.run().expect("compiled module should run");
    let output = String::from_utf8(interp.into_output()).expect("utf8 output");
    assert_eq!(output, "hi relative\n");
}
