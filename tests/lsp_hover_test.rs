use std::collections::HashMap;
use std::path::PathBuf;

use twinkle::lsp::hover_at_module;
use twinkle::lsp::position::{PositionUtf16, byte_offset_to_position_utf16};
use twinkle::query::cache::reset_global_cache;

#[test]
fn hover_returns_inferred_type_at_expression_position() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_hover");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");

    let mut sources = HashMap::new();
    sources.insert(
        entry.clone(),
        r#"value := 42
println("${value}")
"#
        .to_string(),
    );

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed");
    let main = analysis
        .modules
        .get(&analysis.entry_path)
        .expect("entry module should exist");

    let hover = hover_at_module(main, PositionUtf16::new(0, 9));
    assert_eq!(hover.as_deref(), Some("Int"));
}

#[test]
fn hover_returns_none_outside_expression() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_hover_none");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");

    let mut sources = HashMap::new();
    sources.insert(entry.clone(), "value := 1\n".to_string());

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed");
    let main = analysis
        .modules
        .get(&analysis.entry_path)
        .expect("entry module should exist");

    let byte_offset = sources
        .get(&entry)
        .expect("source exists")
        .find(":=")
        .expect("let binding has :=");
    let pos =
        byte_offset_to_position_utf16(sources.get(&entry).expect("source exists"), byte_offset)
            .expect("position");
    let hover = hover_at_module(main, pos);
    assert_eq!(hover, None);
}

#[test]
fn hover_returns_type_for_type_annotation_name() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_hover_type_annotation");
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
    let main = analysis
        .modules
        .get(&analysis.entry_path)
        .expect("entry module should exist");

    let byte_offset = source
        .find("Program!String")
        .expect("return type should contain Program");
    let pos = byte_offset_to_position_utf16(source, byte_offset).expect("position");
    let hover = hover_at_module(main, pos);
    assert_eq!(hover.as_deref(), Some("Program"));
}

#[test]
fn hover_on_inherent_method_name_shows_function_type() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_hover_method_name");
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
    sources.insert(math, math_source.to_string());

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed");
    let main = analysis
        .modules
        .get(&analysis.entry_path)
        .expect("entry module should exist");

    let byte_offset = main_source.find("inc()").expect("method call") + 1;
    let pos = byte_offset_to_position_utf16(main_source, byte_offset).expect("position");
    let hover = hover_at_module(main, pos);
    assert_eq!(hover.as_deref(), Some("fn(Int) Int"));
}

#[test]
fn hover_on_function_definition_name_shows_function_type() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_hover_fn_definition");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"fn make(x: Int) Int {
  x
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
    let main = analysis
        .modules
        .get(&analysis.entry_path)
        .expect("entry module should exist");

    let byte_offset = source.find("make(").expect("function name") + 1;
    let pos = byte_offset_to_position_utf16(source, byte_offset).expect("position");
    let hover = hover_at_module(main, pos);
    assert_eq!(hover.as_deref(), Some("fn(Int) Int"));
}

#[test]
fn hover_on_type_definition_name_shows_type_name() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_hover_type_definition");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"type Program = .{ source: String }
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
    let main = analysis
        .modules
        .get(&analysis.entry_path)
        .expect("entry module should exist");

    let byte_offset = source.find("Program").expect("type name") + 1;
    let pos = byte_offset_to_position_utf16(source, byte_offset).expect("position");
    let hover = hover_at_module(main, pos);
    assert_eq!(hover.as_deref(), Some("Program"));
}

#[test]
fn hover_on_top_level_binding_name_shows_inferred_type() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_hover_binding_definition");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"value := 42
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
    let main = analysis
        .modules
        .get(&analysis.entry_path)
        .expect("entry module should exist");

    let byte_offset = source.find("value").expect("binding name") + 1;
    let pos = byte_offset_to_position_utf16(source, byte_offset).expect("position");
    let hover = hover_at_module(main, pos);
    assert_eq!(hover.as_deref(), Some("Int"));
}

#[test]
fn hover_on_case_arm_variant_name_shows_source_variant_constructor() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_hover_case_pattern_variant");
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
    sources.insert(app, app_source.to_string());

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed");
    let main = analysis
        .modules
        .get(&analysis.entry_path)
        .expect("entry module should exist");

    let byte_offset = main_source
        .find(".HelpRequested(msg)")
        .expect("case arm variant")
        + 1;
    let pos = byte_offset_to_position_utf16(main_source, byte_offset).expect("position");
    let hover = hover_at_module(main, pos);
    assert_eq!(hover.as_deref(), Some("ParseError.HelpRequested(String)"));
}

#[test]
fn hover_on_result_case_arm_variants_shows_result_constructors() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_hover_result_case_variant");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"fn main() Int {
  r: Int!String = .Ok(1)
  case r {
    .Ok(v) => v,
    .Err(_) => 0,
  }
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
    let main = analysis
        .modules
        .get(&analysis.entry_path)
        .expect("entry module should exist");

    let ok_offset = source.find(".Ok(v)").expect("ok arm") + 1;
    let ok_pos = byte_offset_to_position_utf16(source, ok_offset).expect("position");
    let ok_hover = hover_at_module(main, ok_pos);
    assert_eq!(ok_hover.as_deref(), Some("Result.Ok(Int)"));

    let err_offset = source.find(".Err(_)").expect("err arm") + 1;
    let err_pos = byte_offset_to_position_utf16(source, err_offset).expect("position");
    let err_hover = hover_at_module(main, err_pos);
    assert_eq!(err_hover.as_deref(), Some("Result.Err(String)"));
}

#[test]
fn hover_on_builtin_function_shows_doc_string() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_hover_builtin_doc");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = "x := range(10)\n";

    let mut sources = HashMap::new();
    sources.insert(entry.clone(), source.to_string());

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed");
    let main = analysis
        .modules
        .get(&analysis.entry_path)
        .expect("entry module should exist");

    // Hover on "range" — should show signature + doc
    let byte_offset = source.find("range").expect("range call");
    let pos = byte_offset_to_position_utf16(source, byte_offset).expect("position");
    let hover = hover_at_module(main, pos).expect("should have hover");
    // Should contain both the type and the doc string
    assert!(
        hover.contains("fn("),
        "should contain function signature, got: {hover}"
    );
    assert!(
        hover.contains('\n'),
        "should have doc on a separate line, got: {hover}"
    );
}

#[test]
fn hover_on_method_call_shows_doc_string() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_hover_method_doc");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"xs := [1, 2, 3]
n := xs.len()
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
    let main = analysis
        .modules
        .get(&analysis.entry_path)
        .expect("entry module should exist");

    // Hover on ".len" — should show signature + doc
    let byte_offset = source.find("len").expect("len method");
    let pos = byte_offset_to_position_utf16(source, byte_offset).expect("position");
    let hover = hover_at_module(main, pos).expect("should have hover");
    assert!(
        hover.contains("fn("),
        "should contain function signature, got: {hover}"
    );
    assert!(
        hover.contains('\n'),
        "should have doc on a separate line, got: {hover}"
    );
}

#[test]
fn hover_on_user_function_call_shows_doc_comment() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_hover_user_doc");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"/// Add one to x.
fn add_one(x: Int) Int {
  x + 1
}

value := add_one(1)
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
    let main = analysis
        .modules
        .get(&analysis.entry_path)
        .expect("entry module should exist");

    let byte_offset = source.find("add_one(1)").expect("function call");
    let pos = byte_offset_to_position_utf16(source, byte_offset).expect("position");
    let hover = hover_at_module(main, pos).expect("should have hover");
    assert!(
        hover.contains("Add one to x."),
        "expected hover to include parsed /// docs, got: {hover}"
    );
}

#[test]
fn hover_on_builtin_qualified_function_shows_doc_comment() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_hover_builtin_qualified_doc");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"parsed := Int.from_string("42")
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
    let main = analysis
        .modules
        .get(&analysis.entry_path)
        .expect("entry module should exist");

    let byte_offset = source.find("from_string").expect("qualified function call");
    let pos = byte_offset_to_position_utf16(source, byte_offset).expect("position");
    let hover = hover_at_module(main, pos).expect("should have hover");
    assert!(
        hover.contains("Parse a string as an integer. Returns `Int?`."),
        "expected hover to include builtin qualified docs, got: {hover}"
    );
}
