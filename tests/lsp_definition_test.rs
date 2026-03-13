use std::collections::HashMap;
use std::path::PathBuf;

use twinkle::lsp::definition::definition_at_workspace;
use twinkle::lsp::position::{PositionUtf16, byte_offset_to_position_utf16};
use twinkle::query::cache::reset_global_cache;

#[test]
fn definition_resolves_local_parameter_reference() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_definition_local");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"fn add_one(x: Int) Int {
  x + 1
}
"#;

    let mut sources = HashMap::new();
    sources.insert(entry.clone(), source.to_string());

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed");

    let pos = position_of(source, "x + 1", 0);
    let target =
        definition_at_workspace(&analysis, &entry, pos).expect("local definition should resolve");
    assert_eq!(target.path, entry);

    let module = analysis.modules.get(&target.path).expect("module");
    let snippet = module
        .file_registry
        .snippet(target.span)
        .expect("definition snippet");
    assert!(snippet.contains("x"));
}

#[test]
fn definition_resolves_local_function_reference_to_identifier_span() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_definition_local_function");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"fn caller() Int {
  callee()
}

fn callee() Int {
  42
}
"#;

    let mut sources = HashMap::new();
    sources.insert(entry.clone(), source.to_string());

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed");

    let pos = position_of(source, "callee()", 0);
    let target =
        definition_at_workspace(&analysis, &entry, pos).expect("local function should resolve");
    assert_eq!(target.path, entry);

    let module = analysis.modules.get(&target.path).expect("module");
    let snippet = module
        .file_registry
        .snippet(target.span)
        .expect("definition snippet");
    assert_eq!(snippet, "callee");
}

#[test]
fn definition_resolves_module_qualified_symbol() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_definition_import");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let math = project_root.join("math.tw");
    let main_source = r#"use math

value := math.answer()
"#;
    let math_source = r#"pub fn answer() Int {
  42
}
"#;

    let mut sources = HashMap::new();
    sources.insert(entry.clone(), main_source.to_string());
    sources.insert(math.clone(), math_source.to_string());

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed");

    let pos = position_of(main_source, "answer()", 0);
    let target =
        definition_at_workspace(&analysis, &entry, pos).expect("import definition should resolve");
    assert_eq!(target.path, math);

    let module = analysis.modules.get(&target.path).expect("module");
    let snippet = module
        .file_registry
        .snippet(target.span)
        .expect("definition snippet");
    assert_eq!(snippet, "answer");
}

#[test]
fn definition_resolves_method_call_target() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_definition_method");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let math = project_root.join("math.tw");
    let main_source = r#"use math

n := 1
value := n.inc()
"#;
    let math_source = r#"pub fn inc(x: Int) Int {
  x + 1
}
"#;

    let mut sources = HashMap::new();
    sources.insert(entry.clone(), main_source.to_string());
    sources.insert(math.clone(), math_source.to_string());

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed");

    let pos = position_of(main_source, "inc()", 0);
    let target =
        definition_at_workspace(&analysis, &entry, pos).expect("method definition should resolve");
    assert_eq!(target.path, math);

    let module = analysis.modules.get(&target.path).expect("module");
    let snippet = module
        .file_registry
        .snippet(target.span)
        .expect("definition snippet");
    assert_eq!(snippet, "inc");
}

#[test]
fn definition_resolves_type_annotation_name() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_definition_type_annotation");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"type Program = .{ source: String }

fn parse_recursive_types_source(source: String) Program!String {
  .Err("todo")
}
"#;

    let mut sources = HashMap::new();
    sources.insert(entry.clone(), source.to_string());

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed");

    let pos = position_of(source, "Program!String", 0);
    let target =
        definition_at_workspace(&analysis, &entry, pos).expect("type definition should resolve");
    assert_eq!(target.path, entry);

    let module = analysis.modules.get(&target.path).expect("module");
    let snippet = module
        .file_registry
        .snippet(target.span)
        .expect("definition snippet");
    assert_eq!(snippet, "Program");
}

#[test]
fn definition_resolves_case_arm_variant_pattern_to_sum_variant_decl() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_definition_case_variant_pattern");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let app = project_root.join("app.tw");
    let main_source = r#"use app

fn main() String {
  err := app.parse()
  case err {
    .HelpRequested(msg) => msg,
    .Other => "",
  }
}
"#;
    let app_source = r#"pub type ParseError = {
  HelpRequested(String),
  Other,
}

pub fn parse() ParseError {
  .HelpRequested("help")
}
"#;

    let mut sources = HashMap::new();
    sources.insert(entry.clone(), main_source.to_string());
    sources.insert(app.clone(), app_source.to_string());

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed");

    let pos = position_of(main_source, ".HelpRequested(msg)", 1);
    let target =
        definition_at_workspace(&analysis, &entry, pos).expect("variant definition should resolve");
    assert_eq!(target.path, app);

    let module = analysis.modules.get(&target.path).expect("module");
    let snippet = module
        .file_registry
        .snippet(target.span)
        .expect("definition snippet");
    assert_eq!(snippet, "HelpRequested");
}

fn position_of(source: &str, needle: &str, relative_offset: usize) -> PositionUtf16 {
    let start = source.find(needle).expect("needle should be present");
    byte_offset_to_position_utf16(source, start + relative_offset).expect("position should convert")
}
