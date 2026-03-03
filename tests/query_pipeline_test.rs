use std::path::Path;

use twinkle::ir::lower::prelude;
use twinkle::query::api::{
    QuerySymbolKind, default_query_context, lower_stage, parse_file, preassign_module_function_ids,
    resolve_stage, resolve_stage_with_diagnostics, symbols_stage, typecheck_stage,
    typecheck_stage_with_diagnostics,
};
use twinkle::types::env::{TypeEnv, ValueEnv};

#[test]
fn query_stages_can_typecheck_without_compile_context() {
    let parsed =
        parse_file(Path::new("tests/run/mutual_recursion.tw")).expect("parse should succeed");

    let resolved = resolve_stage(&parsed.ast, TypeEnv::new(), ValueEnv::new())
        .expect("resolve should succeed");

    let module_aliases = default_query_context().module_aliases;
    let typed =
        typecheck_stage(&parsed.ast, resolved, module_aliases).expect("typecheck should succeed");

    assert!(typed.value_env.get_function("is_even").is_some());
    assert!(typed.value_env.get_function("is_odd").is_some());
}

#[test]
fn query_lower_stage_handles_function_preassignment() {
    let parsed =
        parse_file(Path::new("tests/run/mutual_recursion.tw")).expect("parse should succeed");

    let resolved = resolve_stage(&parsed.ast, TypeEnv::new(), ValueEnv::new())
        .expect("resolve should succeed");

    let seed = default_query_context();
    let typed = typecheck_stage(&parsed.ast, resolved, seed.module_aliases.clone())
        .expect("typecheck should succeed");

    let start_next_func_id = prelude::USER_FUNC_START;
    let input = seed.lower_input(typed.type_env.clone(), prelude::USER_FUNC_START);

    let lowered = lower_stage(&parsed.ast, typed.type_map, input, &parsed.alias)
        .expect("lower should succeed");

    assert!(!lowered.functions.is_empty());
    assert!(lowered.next_func_id_after >= start_next_func_id);
}

#[test]
fn preassign_module_function_ids_is_idempotent() {
    let parsed =
        parse_file(Path::new("tests/run/mutual_recursion.tw")).expect("parse should succeed");

    let mut func_table = default_query_context().func_table;
    let start_next = prelude::USER_FUNC_START;
    let mut next_func_id = prelude::USER_FUNC_START;

    preassign_module_function_ids(
        &parsed.ast,
        &parsed.alias,
        &mut func_table,
        &mut next_func_id,
    );
    let after_first = next_func_id;

    let even_id = *func_table
        .get("is_even")
        .expect("is_even should be preassigned");
    let qualified_even_id = *func_table
        .get(&format!("{}.is_even", parsed.alias))
        .expect("qualified is_even should be preassigned");
    assert_eq!(even_id, qualified_even_id);
    assert!(after_first > start_next);

    preassign_module_function_ids(
        &parsed.ast,
        &parsed.alias,
        &mut func_table,
        &mut next_func_id,
    );
    assert_eq!(next_func_id, after_first);
    assert_eq!(func_table.get("is_even"), Some(&even_id));
}

#[test]
fn query_stages_report_type_errors() {
    let parsed = parse_file(Path::new("tests/typecheck/fail/type_mismatch.tw"))
        .expect("parse should succeed");

    let resolved = resolve_stage(&parsed.ast, TypeEnv::new(), ValueEnv::new())
        .expect("resolve should succeed");

    let module_aliases = default_query_context().module_aliases;
    match typecheck_stage(&parsed.ast, resolved, module_aliases) {
        Ok(_) => panic!("typecheck should fail"),
        Err(errors) => assert!(!errors.is_empty()),
    }
}

#[test]
fn query_typecheck_diagnostics_are_structured() {
    let parsed = parse_file(Path::new("tests/typecheck/fail/type_mismatch.tw"))
        .expect("parse should succeed");
    let resolved = resolve_stage(&parsed.ast, TypeEnv::new(), ValueEnv::new())
        .expect("resolve should succeed");
    let module_aliases = default_query_context().module_aliases;

    let diags = match typecheck_stage_with_diagnostics(
        &parsed.ast,
        resolved,
        module_aliases,
        &parsed.file_registry,
    ) {
        Ok(_) => panic!("typecheck should fail"),
        Err(diags) => diags,
    };

    assert!(!diags.is_empty());
    assert_eq!(diags[0].code, "E_TYPE_MISMATCH");
    assert!(diags[0].span.is_some());
}

#[test]
fn query_symbols_stage_returns_types_functions_and_values() {
    let parsed =
        parse_file(Path::new("tests/run/module_globals.tw")).expect("parse should succeed");
    let resolved = resolve_stage(&parsed.ast, TypeEnv::new(), ValueEnv::new())
        .expect("resolve should succeed");
    let module_aliases = default_query_context().module_aliases;
    let typed =
        typecheck_stage(&parsed.ast, resolved, module_aliases).expect("typecheck should succeed");

    let symbols = symbols_stage(&parsed.ast, &typed, &parsed.file_registry);
    assert!(!symbols.is_empty());

    let has_double_pi = symbols.iter().any(|s| {
        s.name == "double_pi" && s.kind == QuerySymbolKind::Function && s.detail.starts_with("fn(")
    });
    let has_pi_value = symbols
        .iter()
        .any(|s| s.name == "PI" && s.kind == QuerySymbolKind::Value && s.detail.contains("Int"));

    assert!(has_double_pi);
    assert!(has_pi_value);
}

#[test]
fn query_resolve_diagnostics_are_structured() {
    let parsed = parse_file(Path::new("tests/typecheck/fail/undefined_type.tw"))
        .expect("parse should succeed");

    let diags = match resolve_stage_with_diagnostics(
        &parsed.ast,
        TypeEnv::new(),
        ValueEnv::new(),
        &parsed.file_registry,
    ) {
        Ok(_) => panic!("resolve should fail"),
        Err(diags) => diags,
    };

    assert!(!diags.is_empty());
    assert_eq!(diags[0].code, "E_UNDEFINED_TYPE");
    assert!(diags[0].span.is_some());
}
