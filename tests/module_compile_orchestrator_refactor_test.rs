use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use twinkle::module::{CompileStage, compile_entry_from_source_map_with_trace};
use twinkle::query::cache::reset_global_cache;

fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<unknown>")
        .to_string()
}

#[test]
fn stage_trace_is_deterministic_for_multi_module_project() {
    let _guard = test_lock().lock().expect("test lock poisoned");
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/orchestrator_stage_order");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let dep_a = project_root.join("dep_a.tw");
    let dep_b = project_root.join("dep_b.tw");

    let mut sources = HashMap::new();
    sources.insert(
        entry.clone(),
        r#"
use dep_b
use dep_a

println("${dep_a.value() + dep_b.value()}")
"#
        .to_string(),
    );
    sources.insert(
        dep_a,
        r#"
pub fn value() Int {
  1
}
"#
        .to_string(),
    );
    sources.insert(
        dep_b,
        r#"
pub fn value() Int {
  2
}
"#
        .to_string(),
    );

    let (_core_module, _registry, trace) =
        compile_entry_from_source_map_with_trace(&entry, &sources, &project_root, &stdlib_root)
            .expect("compile should succeed");

    let summary: Vec<(String, CompileStage, bool)> = trace
        .into_iter()
        .map(|event| (file_name(&event.module_path), event.stage, event.cache_hit))
        .collect();

    assert_eq!(
        summary,
        vec![
            ("main.tw".to_string(), CompileStage::Parse, false),
            ("dep_b.tw".to_string(), CompileStage::Parse, false),
            ("dep_b.tw".to_string(), CompileStage::Resolve, false),
            ("dep_b.tw".to_string(), CompileStage::Typecheck, false),
            ("dep_b.tw".to_string(), CompileStage::Lower, false),
            ("dep_a.tw".to_string(), CompileStage::Parse, false),
            ("dep_a.tw".to_string(), CompileStage::Resolve, false),
            ("dep_a.tw".to_string(), CompileStage::Typecheck, false),
            ("dep_a.tw".to_string(), CompileStage::Lower, false),
            ("main.tw".to_string(), CompileStage::Resolve, false),
            ("main.tw".to_string(), CompileStage::Typecheck, false),
            ("main.tw".to_string(), CompileStage::Lower, false),
        ]
    );
}

#[test]
fn prelude_auto_imports_in_sorted_order() {
    let _guard = test_lock().lock().expect("test lock poisoned");
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/orchestrator_prelude_order");
    let stdlib_root = project_root.join("stdlib");
    let prelude_root = project_root.join("prelude");
    let entry = project_root.join("main.tw");
    let prelude_alpha = prelude_root.join("alpha.tw");
    let prelude_beta = prelude_root.join("beta.tw");

    let mut sources = HashMap::new();
    sources.insert(entry.clone(), "println(\"ok\")\n".to_string());
    sources.insert(prelude_beta, "pub fn beta() Int {\n  1\n}\n".to_string());
    sources.insert(prelude_alpha, "pub fn alpha() Int {\n  2\n}\n".to_string());

    let (_core_module, _registry, trace) =
        compile_entry_from_source_map_with_trace(&entry, &sources, &project_root, &stdlib_root)
            .expect("compile should succeed");

    let prelude_parse_order: Vec<String> = trace
        .into_iter()
        .filter(|event| {
            event.stage == CompileStage::Parse && event.module_path.starts_with(&prelude_root)
        })
        .map(|event| file_name(&event.module_path))
        .collect();

    assert_eq!(prelude_parse_order, vec!["alpha.tw", "beta.tw"]);
}

#[test]
fn prelude_auto_import_dedupes_explicit_import() {
    let _guard = test_lock().lock().expect("test lock poisoned");
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/orchestrator_prelude_dedupe");
    let stdlib_root = project_root.join("stdlib");
    let prelude_root = project_root.join("prelude");
    let entry = project_root.join("main.tw");
    let prelude_alpha = prelude_root.join("alpha.tw");

    let mut sources = HashMap::new();
    sources.insert(
        entry.clone(),
        r#"
use prelude.alpha

println("${alpha.alpha()}")
"#
        .to_string(),
    );
    sources.insert(
        prelude_alpha.clone(),
        "pub fn alpha() Int {\n  42\n}\n".to_string(),
    );
    sources.insert(
        prelude_root.join("beta.tw"),
        "pub fn beta() Int {\n  7\n}\n".to_string(),
    );

    let (_core_module, _registry, trace) =
        compile_entry_from_source_map_with_trace(&entry, &sources, &project_root, &stdlib_root)
            .expect("compile should succeed");

    let alpha_parse_count = trace
        .iter()
        .filter(|event| event.stage == CompileStage::Parse && event.module_path == prelude_alpha)
        .count();

    assert_eq!(
        alpha_parse_count, 1,
        "explicit import should dedupe prelude auto-import"
    );
}

#[test]
fn second_compile_hits_stage_cache_for_all_traced_events() {
    let _guard = test_lock().lock().expect("test lock poisoned");
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/orchestrator_cache_behavior");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let dep = project_root.join("dep.tw");

    let mut sources = HashMap::new();
    sources.insert(
        entry.clone(),
        r#"
use dep

println("${dep.value()}")
"#
        .to_string(),
    );
    sources.insert(
        dep,
        r#"
pub fn value() Int {
  5
}
"#
        .to_string(),
    );

    let (_core_module, _registry, first_trace) =
        compile_entry_from_source_map_with_trace(&entry, &sources, &project_root, &stdlib_root)
            .expect("first compile should succeed");

    assert!(
        first_trace.iter().any(|event| !event.cache_hit),
        "first compile should include cache misses"
    );

    let (_core_module, _registry, second_trace) =
        compile_entry_from_source_map_with_trace(&entry, &sources, &project_root, &stdlib_root)
            .expect("second compile should succeed");

    assert!(!second_trace.is_empty(), "trace should not be empty");
    assert!(
        second_trace.iter().all(|event| event.cache_hit),
        "all stages should hit cache on second compile"
    );
}
