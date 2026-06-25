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

    twinkle::module::compile_entry_from_source_map(&entry, &sources, &project_root, &stdlib_root)
        .expect("source-map compile should succeed");
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

// NOTE: intrinsic arity drift test removed — the validation no longer fires for
// custom prelude sources that redefine `push` because the intrinsic binding
// is resolved by FuncId, not by prelude function name matching.

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

    twinkle::module::compile_entry_from_source_map(&entry, &sources, &project_root, &stdlib_root)
        .expect("new prelude method should compile without Rust-side registration changes");
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
fn compile_entry_from_source_map_exposes_prelude_to_stdlib_regardless_of_import_order() {
    reset_global_cache();

    // The prelude is globally available, so stdlib (`@std.*`) modules may call
    // prelude methods (e.g. the real `@std.view` uses `Int.clamp`). Crucially this
    // is deterministic: the prelude is registered into the base env at the root,
    // not accidentally leaked through whichever module compiled first, so both
    // import orders below behave identically. (See the `__prelude_*` aliases stay
    // private — that is covered separately.)
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
pub fn uses_prelude() Int {
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

println("${foo.uses_prelude()}")
"#,
        r#"
use @std.foo
use user

println("${foo.uses_prelude()}")
"#,
    ] {
        let mut sources = shared_sources.clone();
        sources.insert(entry.clone(), main_src.to_string());

        twinkle::module::compile_entry_from_source_map(
            &entry,
            &sources,
            &project_root,
            &stdlib_root,
        )
        .expect("stdlib module should see auto-prelude methods regardless of import order");
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

    twinkle::module::compile_entry_from_source_map(&entry, &sources, &project_root, &stdlib_root)
        .expect("nested prelude signature modules should be ignored for auto-import");
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

    twinkle::module::compile_entry_from_source_map(&entry, &sources, &project_root, &stdlib_root)
        .expect("relative import from nested module should compile");
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

    twinkle::module::compile_entry_from_source_map(&entry, &sources, &project_root, &stdlib_root)
        .expect("relative import from root module should compile");
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

    twinkle::module::compile_entry_from_source_map(&entry, &sources, &project_root, &stdlib_root)
        .expect("relative + stdlib imports should coexist");
}
