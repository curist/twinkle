/// Optimization pass tests.
///
/// These tests verify:
/// 1. ANF structural invariants still hold after optimization (reuses the
///    invariant checker from anf_test.rs via a shared helper).
/// 2. Node-count reduction for programs with compile-time constants.
/// 3. Golden snapshot tests for the optimized ANF of two fixtures.
/// 4. Liveness annotation: `can_reuse_in_place` set/unset correctly.
use std::fs;
use std::path::Path;

use twinkle::ir::anf::{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfModule, AnfOp};
use twinkle::ir::lower_anf;
use twinkle::opt::optimize_module;

// ── Shared helpers ────────────────────────────────────────────────────────────

fn compile_anf(path: &str) -> AnfModule {
    let (core_module, _) = twinkle::module::compile_entry(path)
        .unwrap_or_else(|e| panic!("compile_entry failed for {}: {}", path, e));
    lower_anf::lower_module(&core_module)
}

fn compile_opt(path: &str) -> AnfModule {
    optimize_module(compile_anf(path))
}

// ── ANF invariant checker (mirrors anf_test.rs) ───────────────────────────────

fn check_anf_invariants(module: &AnfModule, name: &str) {
    assert!(!module.functions.is_empty(), "ANF module '{}' has no functions", name);
    if let Some(init_id) = module.init_func_id {
        assert!(
            module.functions.iter().any(|f| f.func_id == init_id),
            "ANF module '{}': init_func_id not found",
            name
        );
    }
    for func in &module.functions {
        check_anf_func(func, name);
    }
}

fn check_anf_func(func: &AnfFunctionDef, prog: &str) {
    check_anf_expr(&func.body, prog, &func.name);
}

fn check_anf_expr(expr: &AnfExpr, prog: &str, func: &str) {
    match expr {
        AnfExpr::Let { local, op, body } => {
            assert_ne!(
                local.0, u32::MAX,
                "Sentinel LocalId(MAX) in '{}' function '{}'",
                prog, func
            );
            check_anf_op(op, prog, func);
            check_anf_expr(body, prog, func);
        }
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue | AnfExpr::Atom(_) => {}
    }
}

fn check_anf_op(op: &AnfOp, prog: &str, func: &str) {
    // Structural invariant: all operand fields are atoms (guaranteed by types).
    // Recurse into sub-expressions of control-flow ops.
    match op {
        AnfOp::AIf { then_branch, else_branch, .. } => {
            check_anf_expr(then_branch, prog, func);
            check_anf_expr(else_branch, prog, func);
        }
        AnfOp::AMatch { arms, .. } => {
            for AnfMatchArm { body, .. } in arms {
                check_anf_expr(body, prog, func);
            }
        }
        AnfOp::ALoop { body } => {
            check_anf_expr(body, prog, func);
        }
        _ => {}
    }
}

// ── Invariant tests: all tests/run/*.tw programs ──────────────────────────────

fn invariant_check(path: &str) {
    assert!(Path::new(path).exists(), "Test file not found: {}", path);
    let module = compile_opt(path);
    let name = Path::new(path).file_name().unwrap().to_string_lossy().to_string();
    check_anf_invariants(&module, &name);
}

#[test] fn opt_hello()              { invariant_check("tests/run/hello.tw"); }
#[test] fn opt_arithmetic()         { invariant_check("tests/run/arithmetic.tw"); }
#[test] fn opt_strings()            { invariant_check("tests/run/strings.tw"); }
#[test] fn opt_strings_escape()     { invariant_check("tests/run/strings_escape.tw"); }
#[test] fn opt_control_flow()       { invariant_check("tests/run/control_flow.tw"); }
#[test] fn opt_loops()              { invariant_check("tests/run/loops.tw"); }
#[test] fn opt_for_break()          { invariant_check("tests/run/for_break.tw"); }
#[test] fn opt_collect()            { invariant_check("tests/run/collect.tw"); }
#[test] fn opt_records()            { invariant_check("tests/run/records.tw"); }
#[test] fn opt_arrays()             { invariant_check("tests/run/arrays.tw"); }
#[test] fn opt_array_methods()      { invariant_check("tests/run/array_methods.tw"); }
#[test] fn opt_closures()           { invariant_check("tests/run/closures.tw"); }
#[test] fn opt_capability_records() { invariant_check("tests/run/capability_records.tw"); }
#[test] fn opt_nested_field_update(){ invariant_check("tests/run/nested_field_update.tw"); }
#[test] fn opt_type_alias()         { invariant_check("tests/run/type_alias.tw"); }
#[test] fn opt_mutual_recursion()   { invariant_check("tests/run/mutual_recursion.tw"); }
#[test] fn opt_result_void()        { invariant_check("tests/run/result_void.tw"); }
#[test] fn opt_dicts()              { invariant_check("tests/run/dicts.tw"); }
#[test] fn opt_dict_methods()       { invariant_check("tests/run/dict_methods.tw"); }
#[test] fn opt_string_methods()     { invariant_check("tests/run/string_methods.tw"); }
#[test] fn opt_variant_collision()  { invariant_check("tests/run/variant_collision.tw"); }
#[test] fn opt_range()              { invariant_check("tests/run/range.tw"); }
#[test] fn opt_iterator()           { invariant_check("tests/run/iterator.tw"); }
#[test] fn opt_iterator_advanced()  { invariant_check("tests/run/iterator_advanced.tw"); }
#[test] fn opt_generic_types()      { invariant_check("tests/run/generic_types.tw"); }
#[test] fn opt_empty_array()        { invariant_check("tests/run/empty_array.tw"); }
#[test] fn opt_module_globals()     { invariant_check("tests/run/module_globals.tw"); }
#[test] fn opt_error_types()        { invariant_check("tests/run/error_types.tw"); }
#[test] fn opt_option_shorthand()   { invariant_check("tests/run/option_shorthand.tw"); }
#[test] fn opt_result_shorthand()   { invariant_check("tests/run/result_shorthand.tw"); }
#[test] fn opt_result_try()         { invariant_check("tests/run/result_try.tw"); }
#[test] fn opt_multi_module()       { invariant_check("tests/run/multi_module/main.tw"); }
#[test] fn opt_multi_module_alias() { invariant_check("tests/run/multi_module_alias/main.tw"); }
#[test] fn opt_pub_values()         { invariant_check("tests/run/pub_values/main.tw"); }
#[test] fn opt_trap_array_oob()     { invariant_check("tests/run/traps/array_oob.tw"); }
#[test] fn opt_trap_div_zero()      { invariant_check("tests/run/traps/div_zero.tw"); }
#[test] fn opt_trap_error_call()    { invariant_check("tests/run/traps/error_call.tw"); }
#[test] fn opt_method_chaining()    { invariant_check("tests/run/method_chaining.tw"); }

// ── Node-count reduction test ─────────────────────────────────────────────────

fn count_let_nodes(expr: &AnfExpr) -> usize {
    match expr {
        AnfExpr::Let { op, body, .. } => 1 + count_let_nodes_in_op(op) + count_let_nodes(body),
        _ => 0,
    }
}

fn count_let_nodes_in_op(op: &AnfOp) -> usize {
    match op {
        AnfOp::AIf { then_branch, else_branch, .. } => {
            count_let_nodes(then_branch) + count_let_nodes(else_branch)
        }
        AnfOp::AMatch { arms, .. } => {
            arms.iter().map(|a| count_let_nodes(&a.body)).sum()
        }
        AnfOp::ALoop { body } => count_let_nodes(body),
        _ => 0,
    }
}

fn total_lets(module: &AnfModule) -> usize {
    module.functions.iter().map(|f| count_let_nodes(&f.body)).sum()
}

#[test]
fn opt_constant_folding_reduces_nodes() {
    let path = "tests/opt/constant_folding.tw";
    let original = compile_anf(path);
    let optimized = optimize_module(original.clone());
    let before = total_lets(&original);
    let after = total_lets(&optimized);
    assert!(
        after < before,
        "Expected fewer Let nodes after optimization: before={}, after={}",
        before,
        after
    );
}

#[test]
fn opt_dead_let_reduces_nodes() {
    let path = "tests/opt/dead_let.tw";
    let original = compile_anf(path);
    let optimized = optimize_module(original.clone());
    let before = total_lets(&original);
    let after = total_lets(&optimized);
    assert!(
        after < before,
        "Expected fewer Let nodes after dead-let elimination: before={}, after={}",
        before,
        after
    );
}

// ── Golden snapshot tests ─────────────────────────────────────────────────────

fn snapshot_dir() -> &'static str {
    "tests/snapshots/opt"
}

fn check_opt_snapshot(tw_path: &str, name: &str) {
    let module = compile_opt(tw_path);
    let actual = format!("{}", module);
    let snap_path = format!("{}/{}.txt", snapshot_dir(), name);

    if std::env::var("UPDATE_SNAPSHOTS").is_ok() || !Path::new(&snap_path).exists() {
        fs::create_dir_all(snapshot_dir()).expect("create snapshot dir");
        fs::write(&snap_path, &actual).expect("write snapshot");
        return;
    }

    let expected = fs::read_to_string(&snap_path)
        .unwrap_or_else(|_| panic!("Could not read snapshot: {}", snap_path));
    assert_eq!(
        actual, expected,
        "Opt snapshot mismatch for '{}'\n\
         To update: UPDATE_SNAPSHOTS=1 cargo test {}",
        tw_path, name
    );
}

#[test]
fn opt_snapshot_constant_folding() {
    check_opt_snapshot("tests/opt/constant_folding.tw", "constant_folding");
}

#[test]
fn opt_snapshot_dead_let() {
    check_opt_snapshot("tests/opt/dead_let.tw", "dead_let");
}

// ── Liveness annotation tests ─────────────────────────────────────────────────

fn has_in_place_update(module: &AnfModule) -> bool {
    module.functions.iter().any(|f| expr_has_in_place(&f.body))
}

fn expr_has_in_place(expr: &AnfExpr) -> bool {
    match expr {
        AnfExpr::Let { op, body, .. } => op_has_in_place(op) || expr_has_in_place(body),
        _ => false,
    }
}

fn op_has_in_place(op: &AnfOp) -> bool {
    match op {
        AnfOp::ARecordUpdate { can_reuse_in_place: true, .. } => true,
        AnfOp::AIf { then_branch, else_branch, .. } => {
            expr_has_in_place(then_branch) || expr_has_in_place(else_branch)
        }
        AnfOp::AMatch { arms, .. } => arms.iter().any(|a| expr_has_in_place(&a.body)),
        AnfOp::ALoop { body } => expr_has_in_place(body),
        _ => false,
    }
}

#[test]
fn opt_record_in_place_annotated() {
    let module = compile_opt("tests/opt/record_in_place.tw");
    assert!(
        has_in_place_update(&module),
        "Expected at least one ARecordUpdate with can_reuse_in_place=true"
    );
}

#[test]
fn opt_record_aliased_not_annotated() {
    let module = compile_opt("tests/opt/record_aliased.tw");
    assert!(
        !has_in_place_update(&module),
        "Expected no ARecordUpdate with can_reuse_in_place=true when base is reused"
    );
}
