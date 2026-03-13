use std::collections::HashMap;
use std::path::PathBuf;

use twinkle::lsp::diagnostics::LspSeverity;
use twinkle::lsp::session::AnalysisSession;
use twinkle::query::cache::reset_global_cache;

#[test]
fn analysis_collects_type_error_diagnostics_instead_of_failing() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_diag");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");

    let mut sources = HashMap::new();
    // Undefined variable should produce a diagnostic, not crash analysis
    sources.insert(entry.clone(), "value := undefined_var\n".to_string());

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed even with errors");

    let diags = analysis
        .diagnostics
        .get(&entry)
        .expect("should have diagnostics for entry");
    assert!(!diags.is_empty(), "should have at least one diagnostic");
    assert!(
        diags.iter().any(|d| d.code == "E_UNDEFINED_VARIABLE"),
        "should contain undefined variable error, got: {:?}",
        diags
    );
}

#[test]
fn analysis_collects_parse_error_diagnostics() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_diag_parse");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");

    let mut sources = HashMap::new();
    // Broken syntax
    sources.insert(entry.clone(), "fn foo( {\n".to_string());

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed even with parse errors");

    let diags = analysis
        .diagnostics
        .get(&entry)
        .expect("should have diagnostics for entry");
    assert!(
        !diags.is_empty(),
        "should have at least one parse diagnostic"
    );
}

#[test]
fn valid_code_produces_empty_diagnostics() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_diag_clean");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");

    let mut sources = HashMap::new();
    sources.insert(entry.clone(), "value := 42\n".to_string());

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed");

    // Either no entry or empty vec
    let diags = analysis.diagnostics.get(&entry);
    assert!(
        diags.is_none() || diags.unwrap().is_empty(),
        "valid code should have no diagnostics"
    );
}

#[test]
fn error_in_dependency_does_not_block_entry_analysis() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_diag_dep");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let helper = project_root.join("helper.tw");

    let mut sources = HashMap::new();
    sources.insert(entry.clone(), "use helper\nvalue := 42\n".to_string());
    // helper has a type error
    sources.insert(
        helper.clone(),
        "pub fn bad() Int { undefined_var }\n".to_string(),
    );

    let analysis = twinkle::module::analyze_entry_from_source_map(
        &entry,
        &sources,
        &project_root,
        &stdlib_root,
    )
    .expect("analysis should succeed even with dependency errors");

    // helper should have diagnostics
    let helper_diags = analysis.diagnostics.get(&helper);
    assert!(
        helper_diags.is_some() && !helper_diags.unwrap().is_empty(),
        "helper module should have diagnostics"
    );
}

#[test]
fn session_diagnostics_returns_lsp_diagnostics_for_errors() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_session_diag");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");

    let mut base_sources = HashMap::new();
    base_sources.insert(entry.clone(), "value := 42\n".to_string());

    let mut session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

    // Valid code: no diagnostics
    let diags = session
        .diagnostics(&entry, &entry)
        .expect("diagnostics should succeed");
    assert!(diags.is_empty(), "valid code should have no diagnostics");

    // Introduce an error
    session.did_change(&entry, "value := undefined_var\n".to_string());
    let diags = session
        .diagnostics(&entry, &entry)
        .expect("diagnostics should succeed");
    assert!(
        !diags.is_empty(),
        "should have diagnostics after error introduced"
    );
    assert_eq!(diags[0].severity, LspSeverity::Error);
    assert_eq!(diags[0].code, "E_UNDEFINED_VARIABLE");

    // Fix the error
    session.did_change(&entry, "value := 42\n".to_string());
    let diags = session
        .diagnostics(&entry, &entry)
        .expect("diagnostics should succeed");
    assert!(diags.is_empty(), "should have no diagnostics after fix");
}

#[test]
fn session_all_diagnostics_returns_per_module_diagnostics() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_session_all_diag");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let helper = project_root.join("helper.tw");

    let mut base_sources = HashMap::new();
    base_sources.insert(entry.clone(), "use helper\nvalue := 42\n".to_string());
    base_sources.insert(
        helper.clone(),
        "pub fn bad() Int { undefined_var }\n".to_string(),
    );

    let session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

    let all_diags = session
        .all_diagnostics(&entry)
        .expect("all_diagnostics should succeed");

    // Should have diagnostics for the helper module
    let helper_diags = all_diags.iter().find(|(p, _)| p == &helper);
    assert!(
        helper_diags.is_some(),
        "should have diagnostics for helper module"
    );
    assert!(
        !helper_diags.unwrap().1.is_empty(),
        "helper diagnostics should not be empty"
    );
}

#[test]
fn session_diagnostics_returns_parse_errors_as_lsp_diagnostics() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_session_parse");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");

    let mut base_sources = HashMap::new();
    base_sources.insert(entry.clone(), "fn foo( {\n".to_string());

    let session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

    let diags = session
        .diagnostics(&entry, &entry)
        .expect("diagnostics should succeed");
    assert!(!diags.is_empty(), "should have parse error diagnostics");
    assert_eq!(diags[0].severity, LspSeverity::Error);
}
