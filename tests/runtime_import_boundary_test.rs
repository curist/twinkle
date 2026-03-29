use std::path::PathBuf;

use twinkle::backend_pipeline::compile_backend_anf;
use twinkle::cli::build::build_wat;
use twinkle::ir::core::{CoreExpr, CoreExprKind, FuncId, FunctionDef, MatchArm};
use twinkle::ir::lower::prelude;

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("boot/tests/fixtures")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn run_fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/run")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn count_substring_in_user_funcs(wat: &str, needle: &str) -> usize {
    let mut in_user = false;
    let mut depth: i32 = 0;
    let mut count = 0;

    for line in wat.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("(func") && trimmed.contains("$user__func_") {
            in_user = true;
            depth = 0;
        }
        if in_user {
            for ch in trimmed.chars() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            in_user = false;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if in_user && trimmed.contains(needle) {
                count += 1;
            }
        }
    }

    count
}

fn expr_mentions_func_id(expr: &CoreExpr, target: FuncId) -> bool {
    match &expr.kind {
        CoreExprKind::GlobalFunc(id) => *id == target,
        CoreExprKind::MakeClosure { func_id, .. } => *func_id == target,
        CoreExprKind::Let { value, body, .. } => {
            expr_mentions_func_id(value, target) || expr_mentions_func_id(body, target)
        }
        CoreExprKind::Assign { value, .. } => expr_mentions_func_id(value, target),
        CoreExprKind::BinOp { left, right, .. } => {
            expr_mentions_func_id(left, target) || expr_mentions_func_id(right, target)
        }
        CoreExprKind::UnOp { expr, .. } => expr_mentions_func_id(expr, target),
        CoreExprKind::Call { callee, args } => {
            expr_mentions_func_id(callee, target)
                || args.iter().any(|arg| expr_mentions_func_id(arg, target))
        }
        CoreExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_mentions_func_id(cond, target)
                || expr_mentions_func_id(then_branch, target)
                || expr_mentions_func_id(else_branch, target)
        }
        CoreExprKind::Match { scrutinee, arms } => {
            expr_mentions_func_id(scrutinee, target)
                || arms
                    .iter()
                    .any(|MatchArm { body, .. }| expr_mentions_func_id(body, target))
        }
        CoreExprKind::Loop { body } => expr_mentions_func_id(body, target),
        CoreExprKind::Break { value } | CoreExprKind::Return { value } => value
            .as_ref()
            .is_some_and(|value| expr_mentions_func_id(value, target)),
        CoreExprKind::Record { fields, .. } => fields
            .iter()
            .any(|(_, value)| expr_mentions_func_id(value, target)),
        CoreExprKind::RecordGet { target: expr, .. } => expr_mentions_func_id(expr, target),
        CoreExprKind::Variant { args, .. } => {
            args.iter().any(|arg| expr_mentions_func_id(arg, target))
        }
        CoreExprKind::ArrayLit { elements } => elements
            .iter()
            .any(|arg| expr_mentions_func_id(arg, target)),
        CoreExprKind::Index { base, index } => {
            expr_mentions_func_id(base, target) || expr_mentions_func_id(index, target)
        }
        CoreExprKind::RecordUpdate { base, value, .. } => {
            expr_mentions_func_id(base, target) || expr_mentions_func_id(value, target)
        }
        CoreExprKind::Defer(inner) => expr_mentions_func_id(inner, target),
        CoreExprKind::LitInt(_)
        | CoreExprKind::LitFloat(_)
        | CoreExprKind::LitBool(_)
        | CoreExprKind::LitStr(_)
        | CoreExprKind::LitVoid
        | CoreExprKind::Local(_)
        | CoreExprKind::GlobalLocal(_)
        | CoreExprKind::Continue => false,
    }
}

fn find_function<'a>(functions: &'a [FunctionDef], name: &str) -> &'a FunctionDef {
    functions
        .iter()
        .find(|func| func.name == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

#[test]
fn boot_lib_vector_stub_lowers_to_library_abi_func_ids() {
    let pipeline = compile_backend_anf(&fixture("vector_i64_boundary.tw"))
        .expect("fixture should compile through backend ANF");
    let functions = &pipeline.core_module.functions;

    let expected = [
        ("vector_i64_make", prelude::LIB_VECTOR_I64_MAKE),
        ("vector_i64_get", prelude::LIB_VECTOR_I64_GET),
        ("vector_i64_set", prelude::LIB_VECTOR_I64_SET),
        ("vector_i64_len", prelude::LIB_VECTOR_I64_LEN),
        ("vector_i64_push", prelude::LIB_VECTOR_I64_PUSH),
        ("vector_i64_concat", prelude::LIB_VECTOR_I64_CONCAT),
        ("vector_i64_slice", prelude::LIB_VECTOR_I64_SLICE),
        (
            "vector_i64_builder_new",
            prelude::LIB_VECTOR_I64_BUILDER_NEW,
        ),
        (
            "vector_i64_builder_from",
            prelude::LIB_VECTOR_I64_BUILDER_FROM,
        ),
        (
            "vector_i64_builder_push",
            prelude::LIB_VECTOR_I64_BUILDER_PUSH,
        ),
        (
            "vector_i64_builder_freeze",
            prelude::LIB_VECTOR_I64_BUILDER_FREEZE,
        ),
    ];

    for (name, func_id) in expected {
        let func = find_function(functions, name);
        assert!(
            expr_mentions_func_id(&func.body, func_id),
            "{name} should reference library ABI FuncId({})",
            func_id.0
        );
    }
}

#[test]
fn boot_lib_vector_stub_emits_rt_arr_imports_for_library_abi_calls() {
    let wat = build_wat(&fixture("vector_i64_boundary.tw"))
        .expect("fixture should build through full backend");

    for sym in [
        "$rt_arr__make_i64",
        "$rt_arr__get_i64",
        "$rt_arr__set_i64",
        "$rt_arr__len_i64",
        "$rt_arr__push_i64",
        "$rt_arr__concat_i64",
        "$rt_arr__slice_i64",
        "$rt_arr__builder_new",
        "$rt_arr__builder_from_i64",
        "$rt_arr__builder_push_i64",
        "$rt_arr__builder_freeze_i64",
    ] {
        assert!(
            wat.contains(sym),
            "expected linked WAT to reference {sym} via library ABI stubs"
        );
    }
}

#[test]
fn stage0_vector_methods_route_through_bootlib_vector_module() {
    let wat =
        build_wat(&run_fixture("vector_methods.tw")).expect("vector methods fixture should build");

    assert!(
        wat.contains("(export \"bootlib_vector_i64__vector_i64_make\""),
        "expected linked WAT to export compiler-owned bootlib.vector_i64 functions"
    );
    assert!(
        count_substring_in_user_funcs(&wat, "call $bootlib_vector_i64__func_") > 0,
        "expected user funcs to call through bootlib.vector_i64"
    );

    for sym in [
        "call $rt_arr__len_i64",
        "call $rt_arr__concat_i64",
        "call $rt_arr__slice_i64",
        "call $rt_arr__make_i64",
        "call $rt_arr__set_i64",
    ] {
        assert_eq!(
            count_substring_in_user_funcs(&wat, sym),
            0,
            "expected {sym} to stay behind the bootlib.vector_i64 boundary in user funcs"
        );
    }
}
