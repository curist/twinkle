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
