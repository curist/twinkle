use std::collections::HashMap;
use std::path::PathBuf;

use twinkle::query::cache::reset_global_cache;
use twinkle::syntax::ast::{ExprId, Item, Stmt};
use twinkle::types::ty::MonoType;

#[test]
fn analyze_entry_from_source_map_collects_typed_modules_and_imports() {
    reset_global_cache();

    let project_root = PathBuf::from("/virtual/lsp_analysis");
    let stdlib_root = project_root.join("stdlib");
    let entry = project_root.join("main.tw");
    let math = project_root.join("math.tw");

    let mut sources = HashMap::new();
    sources.insert(
        entry.clone(),
        r#"
use math

value := math.answer()
println("${value}")
"#
        .to_string(),
    );
    sources.insert(
        math.clone(),
        r#"
pub fn answer() Int {
  42
}
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

    assert_eq!(analysis.entry_path, entry);
    assert!(analysis.diagnostics.is_empty());
    assert!(analysis.modules.contains_key(&math));

    let main = analysis
        .modules
        .get(&analysis.entry_path)
        .expect("entry module should be present in analysis");

    assert_eq!(main.imports.len(), 1);
    assert_eq!(main.imports[0].alias, "math");
    assert_eq!(main.imports[0].canonical_path, math);
    assert!(
        main.typed.value_env.get_function("math.answer").is_some(),
        "entry typed env should include imported qualified function names"
    );

    let value_expr_id = first_top_level_let_expr_id(&main.ast, "value")
        .expect("top-level `value := ...` binding should exist");
    assert_eq!(
        main.typed.type_map.get_expr_type(value_expr_id),
        Some(&MonoType::Int),
        "type map should include inferred type for the binding value expression"
    );
}

fn first_top_level_let_expr_id(
    ast: &twinkle::syntax::ast::SourceFile,
    name: &str,
) -> Option<ExprId> {
    ast.items.iter().find_map(|item| {
        if let Item::Stmt(Stmt::Let {
            pattern: twinkle::syntax::ast::Pattern::Ident(binding, _),
            value,
            ..
        }) = item
        {
            if binding == name {
                return Some(value.id);
            }
        }
        None
    })
}
