use std::collections::HashMap;
use std::path::PathBuf;

use twinkle::lsp::completion::CompletionKind;
use twinkle::lsp::position::byte_offset_to_position_utf16;
use twinkle::lsp::session::AnalysisSession;
use twinkle::query::cache::reset_global_cache;

#[test]
fn completion_in_function_body_includes_local_binding() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_completion_local");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"fn main() Int {
  foo := 1
  value := foo
  value
}
"#;

    let mut base_sources = HashMap::new();
    base_sources.insert(entry.clone(), source.to_string());
    let session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

    let foo_usage = source
        .find("value := foo")
        .expect("foo usage should be present");
    let position = byte_offset_to_position_utf16(source, foo_usage + "value := f".len())
        .expect("position should convert");
    let items = session
        .completion(&entry, &entry, position)
        .expect("completion should succeed");

    assert!(
        items
            .iter()
            .any(|item| item.label == "foo" && item.kind == CompletionKind::Variable),
        "expected local variable completion for `foo`, got: {:?}",
        items
    );
}

#[test]
fn completion_after_import_alias_dot_lists_public_exports() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_completion_alias");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let math = project_root.join("math.tw");
    let main_source = r#"use math

value := math.answer()
"#;
    let math_source = r#"pub fn answer() Int {
  42
}

fn private_secret() Int {
  0
}
"#;

    let mut base_sources = HashMap::new();
    base_sources.insert(entry.clone(), main_source.to_string());
    base_sources.insert(math, math_source.to_string());
    let session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

    let position = position_after(main_source, "math.");
    let items = session
        .completion(&entry, &entry, position)
        .expect("completion should succeed");

    assert!(
        items
            .iter()
            .any(|item| item.label == "answer" && item.kind == CompletionKind::Function),
        "expected exported function completion for `answer`, got: {:?}",
        items
    );
    assert!(
        !items.iter().any(|item| item.label == "private_secret"),
        "non-public symbols should not be offered from module export completion"
    );
}

#[test]
fn completion_after_receiver_dot_includes_methods() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_completion_methods");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"text := "hello"
size := text.len()
"#;

    let mut base_sources = HashMap::new();
    base_sources.insert(entry.clone(), source.to_string());
    let session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

    let position = position_after(source, "text.");
    let items = session
        .completion(&entry, &entry, position)
        .expect("completion should succeed");

    assert!(
        items
            .iter()
            .any(|item| item.label == "len" && item.kind == CompletionKind::Method),
        "expected method completion for `len`, got: {:?}",
        items
    );
}

#[test]
fn completion_includes_doc_comment_for_function_items() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_completion_docs_function");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"/// Adds one to x.
fn add_one(x: Int) Int {
  x + 1
}

fn main() Int {
  value := add_one(1)
  value
}
"#;

    let mut base_sources = HashMap::new();
    base_sources.insert(entry.clone(), source.to_string());
    let session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

    let position = position_after(source, "value := add");
    let items = session
        .completion(&entry, &entry, position)
        .expect("completion should succeed");

    let add_item = items
        .iter()
        .find(|item| item.label == "add_one" && item.kind == CompletionKind::Function)
        .expect("expected function completion item for add_one");
    assert_eq!(
        add_item.documentation.as_deref(),
        Some("Adds one to x."),
        "expected completion item to include parsed /// docs"
    );
}

#[test]
fn method_completion_includes_doc_comment_for_user_method() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_completion_docs_method");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"/// Increment an integer by one.
fn inc(x: Int) Int {
  x + 1
}

n := 1
value := n.inc()
"#;

    let mut base_sources = HashMap::new();
    base_sources.insert(entry.clone(), source.to_string());
    let session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

    let position = position_after(source, "n.");
    let items = session
        .completion(&entry, &entry, position)
        .expect("completion should succeed");

    let inc_item = items
        .iter()
        .find(|item| item.label == "inc")
        .expect("expected method completion item for inc");
    assert_eq!(inc_item.kind, CompletionKind::Method);
    assert_eq!(
        inc_item.documentation.as_deref(),
        Some("Increment an integer by one."),
        "expected method completion docs from /// comment"
    );
}

#[test]
fn completion_includes_doc_for_builtin_function() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_completion_builtin_doc");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"value := range(1)
"#;

    let mut base_sources = HashMap::new();
    base_sources.insert(entry.clone(), source.to_string());
    let session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

    let position = position_after(source, "ran");
    let items = session
        .completion(&entry, &entry, position)
        .expect("completion should succeed");

    let item = items
        .iter()
        .find(|item| item.label == "range")
        .expect("expected completion item for range");
    assert_eq!(
        item.documentation.as_deref(),
        Some("Create a range from 0 to `n` (exclusive)."),
        "expected builtin function docs from signature registry"
    );
}

#[test]
fn completion_includes_doc_for_builtin_type_name() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_completion_builtin_type_doc");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let source = r#"fn id(x: Option<Int>) Option<Int> {
  x
}
"#;

    let mut base_sources = HashMap::new();
    base_sources.insert(entry.clone(), source.to_string());
    let session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

    let position = position_after(source, "Opt");
    let items = session
        .completion(&entry, &entry, position)
        .expect("completion should succeed");

    let option_item = items
        .iter()
        .find(|item| item.label == "Option")
        .expect("expected completion item for Option");
    assert_eq!(option_item.kind, CompletionKind::Struct);
    assert_eq!(
        option_item.documentation.as_deref(),
        Some("Optional value: None or Some(T)."),
        "expected builtin type docs on completion item"
    );
}

fn position_after(source: &str, needle: &str) -> twinkle::lsp::position::PositionUtf16 {
    let byte_offset = source.find(needle).expect("needle should be present") + needle.len();
    byte_offset_to_position_utf16(source, byte_offset).expect("position should convert")
}
