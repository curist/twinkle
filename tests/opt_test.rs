/// Optimization pass tests.
///
/// These tests verify:
/// 1. ANF structural invariants still hold after optimization (reuses the
///    invariant checker from anf_test.rs via a shared helper).
/// 2. Node-count reduction for programs with compile-time constants.
/// 3. Golden snapshot tests for the optimized ANF of two fixtures.
/// 4. Record-update in-place annotation: `can_reuse_in_place` set/unset correctly.
use std::fs;
use std::path::Path;

use twinkle::ir::anf::{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfModule, AnfOp, Atom};
use twinkle::ir::core::FuncId;
use twinkle::opt::optimize_module;

// ── Shared helpers ────────────────────────────────────────────────────────────

fn compile_anf(path: &str) -> AnfModule {
    twinkle::backend_pipeline::compile_backend_anf(path)
        .unwrap_or_else(|e| panic!("compile_backend_anf failed for {}: {}", path, e))
        .anf_module
}

fn compile_opt(path: &str) -> AnfModule {
    twinkle::backend_pipeline::compile_backend_opt(path)
        .unwrap_or_else(|e| panic!("compile_backend_opt failed for {}: {}", path, e))
        .optimized_anf_module
}

// ── ANF invariant checker (mirrors anf_test.rs) ───────────────────────────────

fn check_anf_invariants(module: &AnfModule, name: &str) {
    assert!(
        !module.functions.is_empty(),
        "ANF module '{}' has no functions",
        name
    );
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
                local.0,
                u32::MAX,
                "Sentinel LocalId(MAX) in '{}' function '{}'",
                prog,
                func
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
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => {
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
        AnfOp::ADefer(inner) => {
            check_anf_expr(inner, prog, func);
        }
        _ => {}
    }
}

// ── Invariant tests: all tests/run/*.tw programs ──────────────────────────────

fn invariant_check(path: &str) {
    assert!(Path::new(path).exists(), "Test file not found: {}", path);
    let module = compile_opt(path);
    let name = Path::new(path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    check_anf_invariants(&module, &name);
}

#[test]
fn opt_hello() {
    invariant_check("tests/run/hello.tw");
}
#[test]
fn opt_arithmetic() {
    invariant_check("tests/run/arithmetic.tw");
}
#[test]
fn opt_strings() {
    invariant_check("tests/run/strings.tw");
}
#[test]
fn opt_strings_escape() {
    invariant_check("tests/run/strings_escape.tw");
}
#[test]
fn opt_control_flow() {
    invariant_check("tests/run/control_flow.tw");
}
#[test]
fn opt_loops() {
    invariant_check("tests/run/loops.tw");
}
#[test]
fn opt_for_break() {
    invariant_check("tests/run/for_break.tw");
}
#[test]
fn opt_collect() {
    invariant_check("tests/run/collect.tw");
}
#[test]
fn opt_records() {
    invariant_check("tests/run/records.tw");
}
#[test]
fn opt_vectors() {
    invariant_check("tests/run/vectors.tw");
}
#[test]
fn opt_vector_methods() {
    invariant_check("tests/run/vector_methods.tw");
}
#[test]
fn opt_closures() {
    invariant_check("tests/run/closures.tw");
}
#[test]
fn opt_capability_records() {
    invariant_check("tests/run/capability_records.tw");
}
#[test]
fn opt_nested_field_update() {
    invariant_check("tests/run/nested_field_update.tw");
}
#[test]
fn opt_type_alias() {
    invariant_check("tests/run/type_alias.tw");
}
#[test]
fn opt_mutual_recursion() {
    invariant_check("tests/run/mutual_recursion.tw");
}
#[test]
fn opt_result_void() {
    invariant_check("tests/run/result_void.tw");
}
#[test]
fn opt_dicts() {
    invariant_check("tests/run/dicts.tw");
}
#[test]
fn opt_dict_methods() {
    invariant_check("tests/run/dict_methods.tw");
}
#[test]
fn opt_string_methods() {
    invariant_check("tests/run/string_methods.tw");
}
#[test]
fn opt_variant_collision() {
    invariant_check("tests/run/variant_collision.tw");
}
#[test]
fn opt_range() {
    invariant_check("tests/run/range.tw");
}
#[test]
fn opt_iterator() {
    invariant_check("tests/run/iterator.tw");
}
#[test]
fn opt_iterator_advanced() {
    invariant_check("tests/run/iterator_advanced.tw");
}
#[test]
fn opt_generic_types() {
    invariant_check("tests/run/generic_types.tw");
}
#[test]
fn opt_empty_vector() {
    invariant_check("tests/run/empty_vector.tw");
}
#[test]
fn opt_module_globals() {
    invariant_check("tests/run/module_globals.tw");
}
#[test]
fn opt_error_types() {
    invariant_check("tests/run/error_types.tw");
}
#[test]
fn opt_option_shorthand() {
    invariant_check("tests/run/option_shorthand.tw");
}
#[test]
fn opt_result_shorthand() {
    invariant_check("tests/run/result_shorthand.tw");
}
#[test]
fn opt_result_try() {
    invariant_check("tests/run/result_try.tw");
}
#[test]
fn opt_multi_module() {
    invariant_check("tests/run/multi_module/main.tw");
}
#[test]
fn opt_multi_module_alias() {
    invariant_check("tests/run/multi_module_alias/main.tw");
}
#[test]
fn opt_pub_values() {
    invariant_check("tests/run/pub_values/main.tw");
}
#[test]
fn opt_trap_array_oob() {
    invariant_check("tests/run/traps/array_oob.tw");
}
#[test]
fn opt_trap_div_zero() {
    invariant_check("tests/run/traps/div_zero.tw");
}
#[test]
fn opt_trap_error_call() {
    invariant_check("tests/run/traps/error_call.tw");
}
#[test]
fn opt_method_chaining() {
    invariant_check("tests/run/method_chaining.tw");
}
#[test]
fn opt_defer_basic() {
    invariant_check("tests/run/defer_basic.tw");
}
#[test]
fn opt_defer_return() {
    invariant_check("tests/run/defer_return.tw");
}
#[test]
fn opt_defer_loop() {
    invariant_check("tests/run/defer_loop.tw");
}
#[test]
fn opt_defer_capture() {
    invariant_check("tests/run/defer_capture.tw");
}
#[test]
fn opt_defer_if() {
    invariant_check("tests/run/defer_if.tw");
}

// ── Node-count reduction test ─────────────────────────────────────────────────

fn count_let_nodes(expr: &AnfExpr) -> usize {
    match expr {
        AnfExpr::Let { op, body, .. } => 1 + count_let_nodes_in_op(op) + count_let_nodes(body),
        _ => 0,
    }
}

fn count_let_nodes_in_op(op: &AnfOp) -> usize {
    match op {
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => count_let_nodes(then_branch) + count_let_nodes(else_branch),
        AnfOp::AMatch { arms, .. } => arms.iter().map(|a| count_let_nodes(&a.body)).sum(),
        AnfOp::ALoop { body } => count_let_nodes(body),
        _ => 0,
    }
}

fn total_lets(module: &AnfModule) -> usize {
    module
        .functions
        .iter()
        .map(|f| count_let_nodes(&f.body))
        .sum()
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

// ── Record update in-place annotation tests ───────────────────────────────────

fn has_in_place_update(module: &AnfModule) -> bool {
    module.functions.iter().any(|f| expr_has_in_place(&f.body))
}

fn count_in_place_updates(module: &AnfModule) -> usize {
    module
        .functions
        .iter()
        .map(|f| expr_count_in_place(&f.body))
        .sum()
}

fn expr_has_in_place(expr: &AnfExpr) -> bool {
    match expr {
        AnfExpr::Let { op, body, .. } => op_has_in_place(op) || expr_has_in_place(body),
        _ => false,
    }
}

fn expr_count_in_place(expr: &AnfExpr) -> usize {
    match expr {
        AnfExpr::Let { op, body, .. } => op_count_in_place(op) + expr_count_in_place(body),
        _ => 0,
    }
}

fn op_has_in_place(op: &AnfOp) -> bool {
    match op {
        AnfOp::ARecordUpdate {
            can_reuse_in_place: true,
            ..
        } => true,
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => expr_has_in_place(then_branch) || expr_has_in_place(else_branch),
        AnfOp::AMatch { arms, .. } => arms.iter().any(|a| expr_has_in_place(&a.body)),
        AnfOp::ALoop { body } => expr_has_in_place(body),
        _ => false,
    }
}

fn op_count_in_place(op: &AnfOp) -> usize {
    match op {
        AnfOp::ARecordUpdate {
            can_reuse_in_place: true,
            ..
        } => 1,
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => expr_count_in_place(then_branch) + expr_count_in_place(else_branch),
        AnfOp::AMatch { arms, .. } => arms.iter().map(|a| expr_count_in_place(&a.body)).sum(),
        AnfOp::ALoop { body } => expr_count_in_place(body),
        _ => 0,
    }
}

#[test]
fn opt_record_in_place_annotated() {
    let module = compile_opt("tests/opt/record_unique_in_place.tw");
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

#[test]
fn opt_record_alias_escape_not_annotated() {
    let module = compile_opt("tests/opt/record_alias_escape_not_in_place.tw");
    assert!(
        !has_in_place_update(&module),
        "Expected no in-place record update when aliased value remains observable"
    );
}

#[test]
fn opt_record_capture_escape_not_annotated() {
    let module = compile_opt("tests/opt/record_capture_escape_not_in_place.tw");
    assert!(
        !has_in_place_update(&module),
        "Expected no in-place record update when value is closure-captured"
    );
}

#[test]
fn opt_record_update_chain_escape_at_end_annotated() {
    let module = compile_opt("tests/opt/record_update_chain_escape_at_end.tw");
    assert_eq!(
        count_in_place_updates(&module),
        3,
        "Expected all sequential record updates to be marked reusable before terminal escape"
    );
}

#[test]
fn opt_record_update_chain_alias_mid_not_annotated() {
    let module = compile_opt("tests/opt/record_update_chain_alias_mid_not_in_place.tw");
    assert!(
        !has_in_place_update(&module),
        "Expected no in-place record update when record is aliased mid-chain"
    );
}

// ── Uniqueness-based vector set rewrite tests ─────────────────────────────────

const VECTOR_SET_UNSAFE: FuncId = FuncId(12);
const VECTOR_SET: FuncId = FuncId(39);
const VECTOR_SET_IN_PLACE: FuncId = FuncId(1013);
const VECTOR_APPEND: FuncId = FuncId(11);
const VECTOR_BUILDER_NEW: FuncId = FuncId(33);
const VECTOR_BUILDER_PUSH: FuncId = FuncId(34);
const VECTOR_BUILDER_FREEZE: FuncId = FuncId(35);
const VECTOR_BUILDER_FROM: FuncId = FuncId(1014);
const VECTOR_BUILDER_EXTEND: FuncId = FuncId(1100);
const VECTOR_CONCAT: FuncId = FuncId(25);
const DICT_SET: FuncId = FuncId(13);
const DICT_REMOVE: FuncId = FuncId(29);
const DICT_SET_IN_PLACE: FuncId = FuncId(1015);
const DICT_REMOVE_IN_PLACE: FuncId = FuncId(1016);

/// Check whether any ACall in user functions uses the given FuncId as callee.
fn has_call_to(module: &AnfModule, func_id: FuncId) -> bool {
    module
        .functions
        .iter()
        .filter(|f| !f.name.starts_with("__prelude_"))
        .any(|f| expr_has_call_to(&f.body, func_id))
}

fn count_calls_to(module: &AnfModule, func_id: FuncId) -> usize {
    module
        .functions
        .iter()
        .filter(|f| !f.name.starts_with("__prelude_"))
        .map(|f| expr_count_calls_to(&f.body, func_id))
        .sum()
}

fn expr_has_call_to(expr: &AnfExpr, func_id: FuncId) -> bool {
    match expr {
        AnfExpr::Let { op, body, .. } => {
            op_has_call_to(op, func_id) || expr_has_call_to(body, func_id)
        }
        _ => false,
    }
}

fn expr_count_calls_to(expr: &AnfExpr, func_id: FuncId) -> usize {
    match expr {
        AnfExpr::Let { op, body, .. } => {
            op_count_calls_to(op, func_id) + expr_count_calls_to(body, func_id)
        }
        _ => 0,
    }
}

fn op_has_call_to(op: &AnfOp, func_id: FuncId) -> bool {
    match op {
        AnfOp::ACall { callee, .. } => *callee == Atom::AGlobalFunc(func_id),
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => expr_has_call_to(then_branch, func_id) || expr_has_call_to(else_branch, func_id),
        AnfOp::AMatch { arms, .. } => arms.iter().any(|a| expr_has_call_to(&a.body, func_id)),
        AnfOp::ALoop { body } => expr_has_call_to(body, func_id),
        _ => false,
    }
}

fn op_count_calls_to(op: &AnfOp, func_id: FuncId) -> usize {
    match op {
        AnfOp::ACall { callee, .. } => usize::from(*callee == Atom::AGlobalFunc(func_id)),
        AnfOp::AIf {
            then_branch,
            else_branch,
            ..
        } => expr_count_calls_to(then_branch, func_id) + expr_count_calls_to(else_branch, func_id),
        AnfOp::AMatch { arms, .. } => arms
            .iter()
            .map(|a| expr_count_calls_to(&a.body, func_id))
            .sum(),
        AnfOp::ALoop { body } => expr_count_calls_to(body, func_id),
        _ => 0,
    }
}

fn run_and_capture(path: &str) -> String {
    let (core_module, _registry) = twinkle::module::compile_entry(path)
        .unwrap_or_else(|e| panic!("compile_entry failed for {path}: {e}"));
    let mut interp = twinkle::interp::Interpreter::new(core_module, Vec::<u8>::new());
    interp
        .run()
        .unwrap_or_else(|e| panic!("interpreter run failed for {path}: {e}"));
    String::from_utf8(interp.into_output()).expect("interpreter output is valid UTF-8")
}

fn assert_runtime_output(path: &str, expected: &[&str]) {
    let actual_raw = run_and_capture(path);
    let actual: Vec<&str> = actual_raw.lines().collect();
    assert_eq!(
        actual,
        expected,
        "Runtime output mismatch for {path}\nExpected:\n{}\nActual:\n{}",
        expected.join("\n"),
        actual_raw
    );
}

fn assert_runtime_matrix(matrix: &[(&str, &[&str])]) {
    for (path, expected) in matrix {
        assert_runtime_output(path, expected);
    }
}

fn assert_runtime_output_wasm(path: &str, expected: &[&str]) {
    let (stdout, stderr) = twinkle::cli::run_wasm::run_wasm_capture(path)
        .unwrap_or_else(|e| panic!("run_wasm_capture failed for {path}: {e}"));
    let actual: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        actual,
        expected,
        "Wasm runtime output mismatch for {path}\nExpected:\n{}\nActual:\n{}",
        expected.join("\n"),
        stdout
    );
    assert!(
        stderr.is_empty(),
        "Expected empty stderr for {path}, got:\n{stderr}"
    );
}

#[test]
fn opt_vector_set_unique_rewritten_to_in_place() {
    // Fresh array, index update, base dead after → should rewrite
    let module = compile_opt("tests/opt/vector_set_unique.tw");
    assert!(
        has_call_to(&module, VECTOR_SET_IN_PLACE),
        "Expected VECTOR_SET_UNSAFE to be rewritten to VECTOR_SET_IN_PLACE for unique array"
    );
    assert!(
        !has_call_to(&module, VECTOR_SET_UNSAFE),
        "Expected no remaining VECTOR_SET_UNSAFE calls after uniqueness rewrite"
    );
}

#[test]
fn opt_vector_set_aliased_not_rewritten() {
    // Array aliased (ys := xs) before index update → must NOT rewrite
    let module = compile_opt("tests/opt/vector_set_aliased.tw");
    assert!(
        has_call_to(&module, VECTOR_SET_UNSAFE),
        "Expected VECTOR_SET_UNSAFE to remain when array is aliased"
    );
    assert!(
        !has_call_to(&module, VECTOR_SET_IN_PLACE),
        "Expected no VECTOR_SET_IN_PLACE when array is aliased"
    );
}

#[test]
fn opt_vector_set_captured_not_rewritten() {
    // Array captured by closure before index update → must NOT rewrite
    let module = compile_opt("tests/opt/vector_set_captured.tw");
    assert!(
        has_call_to(&module, VECTOR_SET_UNSAFE),
        "Expected VECTOR_SET_UNSAFE to remain when array is closure-captured"
    );
    assert!(
        !has_call_to(&module, VECTOR_SET_IN_PLACE),
        "Expected no VECTOR_SET_IN_PLACE when array is closure-captured"
    );
}

#[test]
fn opt_vector_set_param_not_rewritten() {
    // Function parameter — not a fresh producer → must NOT rewrite
    let module = compile_opt("tests/opt/vector_set_param.tw");
    assert!(
        has_call_to(&module, VECTOR_SET_UNSAFE),
        "Expected VECTOR_SET_UNSAFE to remain for function parameter array"
    );
    assert!(
        !has_call_to(&module, VECTOR_SET_IN_PLACE),
        "Expected no VECTOR_SET_IN_PLACE for function parameter array"
    );
}

#[test]
fn opt_vector_set_alias_via_init_not_rewritten() {
    // ys := xs keeps alias alive; mutating ys must stay COW.
    let module = compile_opt("tests/opt/vector_set_alias_via_init.tw");
    assert!(
        has_call_to(&module, VECTOR_SET_UNSAFE),
        "Expected VECTOR_SET_UNSAFE to remain for alias via init copy"
    );
    assert!(
        !has_call_to(&module, VECTOR_SET_IN_PLACE),
        "Expected no VECTOR_SET_IN_PLACE for alias via init copy"
    );
}

#[test]
fn opt_vector_set_alias_via_assign_not_rewritten() {
    // ys = xs keeps alias alive; mutating ys must stay COW.
    let module = compile_opt("tests/opt/vector_set_alias_via_assign.tw");
    assert!(
        has_call_to(&module, VECTOR_SET_UNSAFE),
        "Expected VECTOR_SET_UNSAFE to remain for alias via assignment"
    );
    assert!(
        !has_call_to(&module, VECTOR_SET_IN_PLACE),
        "Expected no VECTOR_SET_IN_PLACE for alias via assignment"
    );
}

#[test]
fn opt_vector_set_after_len_rewritten() {
    // Read-only len() should not taint uniqueness.
    let module = compile_opt("tests/opt/vector_set_after_len.tw");
    assert!(
        has_call_to(&module, VECTOR_SET_IN_PLACE),
        "Expected VECTOR_SET_UNSAFE to rewrite after read-only len()"
    );
    assert!(
        !has_call_to(&module, VECTOR_SET_UNSAFE),
        "Expected no remaining VECTOR_SET_UNSAFE after rewrite"
    );
}

#[test]
fn opt_vector_append_then_set_rewritten() {
    // Consuming push + reassign should preserve uniqueness for later set.
    let module = compile_opt("tests/opt/vector_append_then_set.tw");
    assert!(
        has_call_to(&module, VECTOR_SET_IN_PLACE),
        "Expected VECTOR_SET_UNSAFE to rewrite after VECTOR_APPEND reassign"
    );
    assert!(
        !has_call_to(&module, VECTOR_SET_UNSAFE),
        "Expected no remaining VECTOR_SET_UNSAFE after rewrite"
    );
}

#[test]
fn opt_vector_set_additional_positive_rewrites() {
    let fixtures = [
        "tests/opt/vector_set_move_via_init_rebind.tw",
        "tests/opt/vector_set_move_via_assign_rebind.tw",
        "tests/opt/vector_set_from_make.tw",
        "tests/opt/vector_set_twice_chain.tw",
        "tests/opt/vector_set_in_if_branches.tw",
        "tests/opt/vector_set_after_len_in_branch.tw",
        "tests/opt/vector_set_after_append_chain.tw",
        "tests/opt/vector_set_after_get.tw",
    ];

    for path in fixtures {
        let module = compile_opt(path);
        assert!(
            has_call_to(&module, VECTOR_SET_IN_PLACE),
            "Expected VECTOR_SET_IN_PLACE in {}",
            path
        );
        assert!(
            !has_call_to(&module, VECTOR_SET_UNSAFE),
            "Expected no VECTOR_SET_UNSAFE in {}",
            path
        );
    }
}

#[test]
fn opt_vector_set_additional_negative_no_rewrite() {
    let fixtures = [
        "tests/opt/vector_set_after_user_call.tw",
        "tests/opt/vector_set_after_indirect_call.tw",
        "tests/opt/vector_set_stored_in_array.tw",
        "tests/opt/vector_set_after_append_then_user_call.tw",
        "tests/opt/vector_set_branch_alias_escape.tw",
        "tests/opt/vector_set_capture_in_branch.tw",
        "tests/opt/vector_set_init_alias_capture_escape_in_branch.tw",
        "tests/opt/vector_set_stored_in_option_variant.tw",
        "tests/opt/vector_set_after_safe_set_call.tw",
        "tests/opt/vector_set_after_concat.tw",
        "tests/opt/vector_set_after_slice.tw",
    ];

    for path in fixtures {
        let module = compile_opt(path);
        assert!(
            has_call_to(&module, VECTOR_SET_UNSAFE),
            "Expected VECTOR_SET_UNSAFE to remain in {}",
            path
        );
        assert!(
            !has_call_to(&module, VECTOR_SET_IN_PLACE),
            "Expected no VECTOR_SET_IN_PLACE in {}",
            path
        );
    }
}

#[test]
fn opt_vector_set_safe_option_not_rewritten_to_in_place() {
    let module = compile_opt("tests/opt/vector_set_safe_option_not_rewritten.tw");
    assert!(
        has_call_to(&module, VECTOR_SET),
        "Expected VECTOR_SET (safe) call to remain"
    );
    assert!(
        !has_call_to(&module, VECTOR_SET_IN_PLACE),
        "Expected no VECTOR_SET_IN_PLACE for safe Vector.set"
    );
    assert!(
        !has_call_to(&module, VECTOR_SET_UNSAFE),
        "Expected no VECTOR_SET_UNSAFE for safe Vector.set fixture"
    );
}

#[test]
fn opt_vector_set_precise_call_counts() {
    let matrix = [
        (
            "tests/opt/vector_set_twice_chain.tw",
            2usize, // VECTOR_SET_IN_PLACE
            0usize, // VECTOR_SET_UNSAFE
            0usize, // VECTOR_SET (safe)
        ),
        (
            "tests/opt/vector_set_in_if_branches.tw",
            2usize,
            0usize,
            0usize,
        ),
        (
            "tests/opt/vector_set_after_append_chain.tw",
            1usize,
            0usize,
            0usize,
        ),
        (
            "tests/opt/vector_set_safe_option_not_rewritten.tw",
            0usize,
            0usize,
            1usize,
        ),
        (
            "tests/opt/vector_set_after_user_call.tw",
            0usize,
            1usize,
            0usize,
        ),
        (
            "tests/opt/vector_set_branch_alias_escape.tw",
            0usize,
            1usize,
            0usize,
        ),
        (
            "tests/opt/vector_set_init_alias_capture_escape_in_branch.tw",
            0usize,
            1usize,
            0usize,
        ),
    ];

    for (path, expected_in_place, expected_unsafe, expected_safe) in matrix {
        let module = compile_opt(path);
        assert_eq!(
            count_calls_to(&module, VECTOR_SET_IN_PLACE),
            expected_in_place,
            "VECTOR_SET_IN_PLACE call count mismatch in {}",
            path
        );
        assert_eq!(
            count_calls_to(&module, VECTOR_SET_UNSAFE),
            expected_unsafe,
            "VECTOR_SET_UNSAFE call count mismatch in {}",
            path
        );
        assert_eq!(
            count_calls_to(&module, VECTOR_SET),
            expected_safe,
            "VECTOR_SET call count mismatch in {}",
            path
        );
    }
}

#[test]
fn opt_vector_set_runtime_semantics_core_paths() {
    let matrix: [(&str, &[&str]); 9] = [
        ("tests/opt/vector_append_then_set.tw", &["99"]),
        ("tests/opt/vector_set_unique.tw", &["99"]),
        ("tests/opt/vector_set_param.tw", &["99"]),
        ("tests/opt/vector_set_aliased.tw", &["1", "99"]),
        ("tests/opt/vector_set_captured.tw", &["1", "99"]),
        ("tests/opt/vector_set_alias_via_init.tw", &["1", "99"]),
        ("tests/opt/vector_set_alias_via_assign.tw", &["1", "99"]),
        ("tests/opt/vector_set_after_len.tw", &["3", "99"]),
        ("tests/opt/vector_set_move_via_init_rebind.tw", &["99"]),
    ];
    assert_runtime_matrix(&matrix);
}

#[test]
fn opt_vector_set_runtime_semantics_call_and_branch_paths() {
    let matrix: [(&str, &[&str]); 10] = [
        ("tests/opt/vector_set_move_via_assign_rebind.tw", &["99"]),
        ("tests/opt/vector_set_from_make.tw", &["42"]),
        ("tests/opt/vector_set_twice_chain.tw", &["20"]),
        ("tests/opt/vector_set_in_if_branches.tw", &["1"]),
        ("tests/opt/vector_set_after_user_call.tw", &["3", "99"]),
        ("tests/opt/vector_set_after_indirect_call.tw", &["3", "99"]),
        ("tests/opt/vector_set_after_get.tw", &["1", "99"]),
        ("tests/opt/vector_set_stored_in_array.tw", &["1", "99"]),
        (
            "tests/opt/vector_set_after_append_then_user_call.tw",
            &["4", "99"],
        ),
        ("tests/opt/vector_set_safe_option_not_rewritten.tw", &["99"]),
    ];
    assert_runtime_matrix(&matrix);
}

#[test]
fn opt_vector_set_after_get_wasm_semantics() {
    assert_runtime_output_wasm("tests/opt/vector_set_after_get.tw", &["1", "99"]);
}

#[test]
fn opt_vector_set_runtime_semantics_escape_paths() {
    let matrix: [(&str, &[&str]); 9] = [
        ("tests/opt/vector_set_branch_alias_escape.tw", &["1", "99"]),
        (
            "tests/opt/vector_set_after_branch_local_alias.tw",
            &["1", "99"],
        ),
        ("tests/opt/vector_set_after_len_in_branch.tw", &["3", "99"]),
        ("tests/opt/vector_set_after_append_chain.tw", &["99"]),
        ("tests/opt/vector_set_capture_in_branch.tw", &["1", "99"]),
        (
            "tests/opt/vector_set_init_alias_capture_escape_in_branch.tw",
            &["1", "99"],
        ),
        (
            "tests/opt/vector_set_stored_in_option_variant.tw",
            &["1", "99"],
        ),
        ("tests/opt/vector_set_after_safe_set_call.tw", &["7", "99"]),
        ("tests/opt/vector_set_after_concat.tw", &["4", "99"]),
    ];
    assert_runtime_matrix(&matrix);
}

#[test]
fn opt_vector_set_runtime_semantics_slice_path() {
    assert_runtime_output("tests/opt/vector_set_after_slice.tw", &["2", "99"]);
}

#[test]
fn opt_vector_set_runtime_semantics_loop_branch_escape_path() {
    assert_runtime_output(
        "tests/opt/vector_set_cell_closure_loop_branch_escape_not_rewritten_interp.tw",
        &["1", "99"],
    );
}

#[test]
fn opt_vector_set_init_alias_capture_escape_in_branch_wasm_semantics() {
    // Regression guard: branch-local `ys := xs` captured into escaping closure
    // must taint `xs`, preventing in-place rewrite.
    assert_runtime_output_wasm(
        "tests/opt/vector_set_init_alias_capture_escape_in_branch.tw",
        &["1", "99"],
    );
}

#[test]
fn opt_vector_set_cell_closure_loop_branch_escape_not_rewritten() {
    // This fixture includes `collect range(...)` which can contribute legitimate
    // VECTOR_SET_IN_PLACE calls from collect lowering. Guard only the user update
    // path: VECTOR_SET_UNSAFE for xs[0] must remain.
    let module =
        compile_opt("tests/opt/vector_set_cell_closure_loop_branch_escape_not_rewritten.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_SET_UNSAFE),
        1,
        "Expected one VECTOR_SET_UNSAFE for user xs[0] update in stress fixture"
    );
}

#[test]
fn opt_vector_set_cell_closure_loop_branch_escape_wasm_semantics() {
    // Stress case: loop + branch + Cell + closure-captured init alias.
    // Must not rewrite vector set in place.
    assert_runtime_output_wasm(
        "tests/opt/vector_set_cell_closure_loop_branch_escape_not_rewritten.tw",
        &["1", "99"],
    );
}

#[test]
fn opt_vector_set_cell_closure_loop_branch_escape_wasm_stress() {
    // Stronger Wasm-only stress case. Keep the interpreter-side semantics test
    // lightweight, but push the Wasm runtime with a larger loop count.
    assert_runtime_output_wasm(
        "tests/opt/vector_set_cell_closure_loop_branch_escape_wasm_stress.tw",
        &["1", "99"],
    );
}

#[test]
fn opt_vector_append_loop_unique_rewritten_to_builder() {
    let module = compile_opt("tests/opt/vector_append_loop_unique.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_APPEND),
        0,
        "Expected no VECTOR_APPEND in rewritten loop accumulator fixture"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_NEW),
        1,
        "Expected one VECTOR_BUILDER_NEW call"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FROM),
        0,
        "Expected no VECTOR_BUILDER_FROM call for empty-seed accumulator"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_PUSH),
        1,
        "Expected one VECTOR_BUILDER_PUSH call"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected one VECTOR_BUILDER_FREEZE call"
    );
}

#[test]
fn opt_vector_append_loop_seeded_rewritten_to_builder_from() {
    let module = compile_opt("tests/opt/vector_append_loop_seeded_not_rewritten.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_APPEND),
        0,
        "Expected no VECTOR_APPEND in seeded rewritten fixture"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FROM),
        1,
        "Expected one VECTOR_BUILDER_FROM call for seeded accumulator"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_NEW),
        0,
        "Expected no VECTOR_BUILDER_NEW call for seeded accumulator"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_PUSH),
        1,
        "Expected one VECTOR_BUILDER_PUSH call"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected one VECTOR_BUILDER_FREEZE call"
    );
}

#[test]
fn opt_vector_append_loop_negative_cases_not_rewritten() {
    let fixtures = [
        "tests/opt/vector_append_loop_reads_acc_not_rewritten.tw",
        "tests/opt/vector_append_loop_captured_not_rewritten.tw",
    ];

    for path in fixtures {
        let module = compile_opt(path);
        assert!(
            has_call_to(&module, VECTOR_APPEND),
            "Expected VECTOR_APPEND to remain in {}",
            path
        );
        assert!(
            !has_call_to(&module, VECTOR_BUILDER_PUSH),
            "Expected no VECTOR_BUILDER_PUSH in {}",
            path
        );
        assert!(
            !has_call_to(&module, VECTOR_BUILDER_NEW),
            "Expected no VECTOR_BUILDER_NEW in {}",
            path
        );
        assert!(
            !has_call_to(&module, VECTOR_BUILDER_FREEZE),
            "Expected no VECTOR_BUILDER_FREEZE in {}",
            path
        );
    }
}

#[test]
fn opt_vector_append_straight_line_rewritten_to_builder() {
    let module = compile_opt("tests/opt/vector_append_straight_line.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_APPEND),
        0,
        "Expected no VECTOR_APPEND in straight-line builder rewrite"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_NEW),
        1,
        "Expected one VECTOR_BUILDER_NEW for empty-seed straight-line chain"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_PUSH),
        5,
        "Expected five VECTOR_BUILDER_PUSH calls (one per append)"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected one VECTOR_BUILDER_FREEZE call"
    );
}

#[test]
fn opt_vector_append_straight_line_runtime_semantics() {
    assert_runtime_output("tests/opt/vector_append_straight_line.tw", &["5", "50"]);
}

#[test]
fn opt_vector_concat_after_refresh_rewritten() {
    let module = compile_opt("tests/opt/vector_concat_after_refresh_rewritten.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_CONCAT),
        0,
        "Expected no VECTOR_CONCAT after refreshed-base concat rewrite"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FROM),
        1,
        "Expected one VECTOR_BUILDER_FROM for refreshed-base concat rewrite"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_EXTEND),
        1,
        "Expected one VECTOR_BUILDER_EXTEND for refreshed-base concat rewrite"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected one VECTOR_BUILDER_FREEZE call"
    );
}

#[test]
fn opt_vector_concat_after_refresh_runtime_semantics() {
    assert_runtime_output(
        "tests/opt/vector_concat_after_refresh_rewritten.tw",
        &["3", "6"],
    );
}

#[test]
fn opt_vector_concat_loop_after_refresh_rewritten() {
    let module = compile_opt("tests/opt/vector_concat_loop_after_refresh_rewritten.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_CONCAT),
        0,
        "Expected no VECTOR_CONCAT after refreshed-base loop concat rewrite"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FROM),
        1,
        "Expected one VECTOR_BUILDER_FROM for refreshed-base loop concat rewrite"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_EXTEND),
        1,
        "Expected one VECTOR_BUILDER_EXTEND call site in loop body"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected one VECTOR_BUILDER_FREEZE for refreshed-base loop concat rewrite"
    );
}

#[test]
fn opt_vector_concat_loop_after_refresh_runtime_semantics() {
    assert_runtime_output(
        "tests/opt/vector_concat_loop_after_refresh_rewritten.tw",
        &["4", "10"],
    );
}

#[test]
fn opt_vector_concat_single_straight_line_rewritten_to_builder_extend() {
    let module = compile_opt("tests/opt/vector_concat_single_straight_line.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_CONCAT),
        0,
        "Expected no VECTOR_CONCAT in single straight-line concat builder rewrite"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_NEW),
        1,
        "Expected one VECTOR_BUILDER_NEW for empty single concat chain"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_EXTEND),
        1,
        "Expected one VECTOR_BUILDER_EXTEND for single concat site"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected one VECTOR_BUILDER_FREEZE call"
    );
}

#[test]
fn opt_vector_concat_single_straight_line_runtime_semantics() {
    assert_runtime_output(
        "tests/opt/vector_concat_single_straight_line.tw",
        &["3", "6"],
    );
}

#[test]
fn opt_vector_concat_straight_line_rewritten_to_builder_extend() {
    let module = compile_opt("tests/opt/vector_concat_straight_line.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_CONCAT),
        0,
        "Expected no VECTOR_CONCAT in straight-line concat builder rewrite"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_NEW),
        1,
        "Expected one VECTOR_BUILDER_NEW for empty-seed concat chain"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_EXTEND),
        3,
        "Expected one VECTOR_BUILDER_EXTEND per concat site"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected one VECTOR_BUILDER_FREEZE call"
    );
}

#[test]
fn opt_vector_concat_straight_line_runtime_semantics() {
    assert_runtime_output("tests/opt/vector_concat_straight_line.tw", &["5", "75"]);
}

#[test]
fn opt_vector_concat_negative_self_not_rewritten() {
    let module = compile_opt("tests/opt/vector_concat_self_not_rewritten.tw");
    assert!(
        has_call_to(&module, VECTOR_CONCAT),
        "Expected VECTOR_CONCAT to remain for self-concat"
    );
    assert!(
        !has_call_to(&module, VECTOR_BUILDER_EXTEND),
        "Expected no VECTOR_BUILDER_EXTEND for self-concat"
    );
    assert_runtime_output(
        "tests/opt/vector_concat_self_not_rewritten.tw",
        &["4", "1", "2", "1", "2"],
    );
}

#[test]
fn opt_vector_concat_loop_rewritten_to_builder_extend() {
    let module = compile_opt("tests/opt/vector_concat_loop_unique.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_CONCAT),
        0,
        "Expected no VECTOR_CONCAT in loop concat builder rewrite"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_NEW),
        1,
        "Expected one VECTOR_BUILDER_NEW for empty loop concat accumulator"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_EXTEND),
        1,
        "Expected one VECTOR_BUILDER_EXTEND call site in loop body"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected one VECTOR_BUILDER_FREEZE for loop concat rewrite"
    );
}

#[test]
fn opt_vector_concat_loop_runtime_semantics() {
    assert_runtime_output("tests/opt/vector_concat_loop_unique.tw", &["6", "36"]);
}

#[test]
fn opt_vector_concat_loop_negative_self_not_rewritten() {
    let module = compile_opt("tests/opt/vector_concat_loop_self_not_rewritten.tw");
    assert!(
        has_call_to(&module, VECTOR_CONCAT),
        "Expected VECTOR_CONCAT to remain for loop self-concat"
    );
    assert!(
        !has_call_to(&module, VECTOR_BUILDER_EXTEND),
        "Expected no VECTOR_BUILDER_EXTEND for loop self-concat"
    );
    assert_runtime_output(
        "tests/opt/vector_concat_loop_self_not_rewritten.tw",
        &["4", "1", "2", "1", "2"],
    );
}

#[test]
fn opt_vector_concat_dead_base_rewritten_to_builder_extend() {
    let module = compile_opt("tests/opt/vector_concat_dead_base.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_CONCAT),
        0,
        "Expected no VECTOR_CONCAT after dead-base concat rewrite"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FROM),
        1,
        "Expected one VECTOR_BUILDER_FROM for dead-base concat rewrite"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_EXTEND),
        1,
        "Expected one VECTOR_BUILDER_EXTEND for dead-base concat rewrite"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected one VECTOR_BUILDER_FREEZE for dead-base concat rewrite"
    );
}

#[test]
fn opt_vector_concat_dead_base_runtime_semantics() {
    assert_runtime_output("tests/opt/vector_concat_dead_base.tw", &["4", "10"]);
}

#[test]
fn opt_vector_builder_region_mixed_straight_line_rewritten() {
    let module = compile_opt("tests/opt/vector_builder_region_mixed_straight_line.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_APPEND),
        0,
        "Expected no VECTOR_APPEND in mixed straight-line builder region"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_CONCAT),
        0,
        "Expected no VECTOR_CONCAT in mixed straight-line builder region"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_NEW),
        1,
        "Expected one VECTOR_BUILDER_NEW for empty mixed builder region"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_PUSH),
        2,
        "Expected one VECTOR_BUILDER_PUSH per append site"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_EXTEND),
        2,
        "Expected one VECTOR_BUILDER_EXTEND per concat site"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected one VECTOR_BUILDER_FREEZE for mixed straight-line builder region"
    );
}

#[test]
fn opt_vector_builder_region_mixed_straight_line_runtime_semantics() {
    assert_runtime_output(
        "tests/opt/vector_builder_region_mixed_straight_line.tw",
        &["6", "63"],
    );
}

#[test]
fn opt_vector_builder_region_mixed_loop_rewritten() {
    let module = compile_opt("tests/opt/vector_builder_region_mixed_loop.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_APPEND),
        0,
        "Expected no VECTOR_APPEND in mixed loop builder region"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_CONCAT),
        0,
        "Expected no VECTOR_CONCAT in mixed loop builder region"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_NEW),
        1,
        "Expected one VECTOR_BUILDER_NEW for empty mixed loop builder region"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_PUSH),
        1,
        "Expected one VECTOR_BUILDER_PUSH call site in mixed loop body"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_EXTEND),
        1,
        "Expected one VECTOR_BUILDER_EXTEND call site in mixed loop body"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected one VECTOR_BUILDER_FREEZE for mixed loop builder region"
    );
}

#[test]
fn opt_vector_builder_region_mixed_loop_runtime_semantics() {
    assert_runtime_output("tests/opt/vector_builder_region_mixed_loop.tw", &["8", "6"]);
}

#[test]
fn opt_vector_append_loop_runtime_semantics() {
    assert_runtime_output("tests/opt/vector_append_loop_unique.tw", &["3", "6"]);
    assert_runtime_output(
        "tests/opt/vector_append_loop_seeded_not_rewritten.tw",
        &["10", "4"],
    );
    assert_runtime_output(
        "tests/opt/vector_append_loop_reads_acc_not_rewritten.tw",
        &["0", "1", "2", "3"],
    );
    assert_runtime_output(
        "tests/opt/vector_append_loop_captured_not_rewritten.tw",
        &["0", "1", "2", "3"],
    );
}

#[test]
fn opt_vector_append_loop_seeded_runtime_wasm_semantics() {
    // Regression guard for builder_from capacity correctness in Wasm runtime path.
    assert_runtime_output_wasm(
        "tests/opt/vector_append_loop_seeded_not_rewritten.tw",
        &["10", "4"],
    );
}

#[test]
fn opt_dict_set_unique_rewritten_to_in_place() {
    let module = compile_opt("tests/opt/dict_set_unique.tw");
    assert!(
        has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected DICT_SET to rewrite to DICT_SET_IN_PLACE for unique dict"
    );
    assert!(
        !has_call_to(&module, DICT_SET),
        "Expected no remaining DICT_SET calls after rewrite"
    );
}

#[test]
fn opt_dict_set_aliased_not_rewritten() {
    let module = compile_opt("tests/opt/dict_set_aliased_not_rewritten.tw");
    assert!(
        has_call_to(&module, DICT_SET),
        "Expected DICT_SET to remain when dict is aliased"
    );
    assert!(
        !has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected no DICT_SET_IN_PLACE when dict is aliased"
    );
}

#[test]
fn opt_dict_remove_unique_rewritten_to_in_place() {
    let module = compile_opt("tests/opt/dict_remove_unique.tw");
    assert_eq!(
        count_calls_to(&module, DICT_REMOVE_IN_PLACE),
        2,
        "Expected two DICT_REMOVE_IN_PLACE calls for unique dict removes"
    );
    assert_eq!(
        count_calls_to(&module, DICT_REMOVE),
        0,
        "Expected no DICT_REMOVE calls after rewrite"
    );
}

#[test]
fn opt_dict_chain_unique_rewritten_to_in_place() {
    let module = compile_opt("tests/opt/dict_chain_unique_rewritten.tw");
    assert_eq!(
        count_calls_to(&module, DICT_SET_IN_PLACE),
        2,
        "Expected two DICT_SET_IN_PLACE calls in unique dict update chain"
    );
    assert_eq!(
        count_calls_to(&module, DICT_REMOVE_IN_PLACE),
        1,
        "Expected one DICT_REMOVE_IN_PLACE call in unique dict update chain"
    );
    assert_eq!(
        count_calls_to(&module, DICT_SET),
        0,
        "Expected no DICT_SET calls after rewrite"
    );
    assert_eq!(
        count_calls_to(&module, DICT_REMOVE),
        0,
        "Expected no DICT_REMOVE calls after rewrite"
    );
}

#[test]
fn opt_dict_additional_negative_no_rewrite() {
    // dict_after_user_call: dict passed to user function, can't prove uniqueness
    let module = compile_opt("tests/opt/dict_after_user_call_not_rewritten.tw");
    assert!(
        has_call_to(&module, DICT_SET),
        "Expected DICT_SET to remain in dict_after_user_call"
    );
    assert!(
        !has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected no DICT_SET_IN_PLACE in dict_after_user_call"
    );

    // dict_stored_in_array: probe() stores dict in array (tainted, stays COW),
    // but top-level n is fresh+unique and only read after set (dict.len is
    // now classified as read-only), so the point rewrite correctly applies.
    let module = compile_opt("tests/opt/dict_stored_in_array_not_rewritten.tw");
    assert!(
        has_call_to(&module, DICT_SET),
        "Expected DICT_SET to remain in probe() where dict is stored in array"
    );
    assert!(
        has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected DICT_SET_IN_PLACE for top-level unique dict n"
    );
}

#[test]
fn opt_dict_phase4_runtime_semantics() {
    assert_runtime_output("tests/opt/dict_set_unique.tw", &["7", "1"]);
    assert_runtime_output("tests/opt/dict_set_aliased_not_rewritten.tw", &["0", "1"]);
    assert_runtime_output("tests/opt/dict_remove_unique.tw", &["1", "false", "true"]);
    assert_runtime_output(
        "tests/opt/dict_remove_captured_not_rewritten.tw",
        &["1", "0"],
    );
}

#[test]
fn opt_dict_phase6_runtime_semantics() {
    assert_runtime_output(
        "tests/opt/dict_chain_unique_rewritten.tw",
        &["1", "false", "true"],
    );
    assert_runtime_output("tests/opt/dict_after_user_call_not_rewritten.tw", &["1"]);
    assert_runtime_output(
        "tests/opt/dict_stored_in_array_not_rewritten.tw",
        &["0", "1"],
    );
}

#[test]
fn opt_dict_phase4_wasm_semantics() {
    assert_runtime_output_wasm("tests/opt/dict_set_unique.tw", &["7", "1"]);
    assert_runtime_output_wasm("tests/opt/dict_remove_unique.tw", &["1", "false", "true"]);
}

#[test]
fn opt_dict_phase6_wasm_semantics() {
    assert_runtime_output_wasm(
        "tests/opt/dict_chain_unique_rewritten.tw",
        &["1", "false", "true"],
    );
}

// ── Phase A: reassign-aware taint refinement ─────────────────────────────────

#[test]
fn opt_phase_a_dict_set_chain_escape_at_end_rewritten() {
    // Sequential dict sets on a fresh local that only escapes at function end.
    // Phase A should untaint the local, enabling all sets to be in-place.
    let module = compile_opt("tests/opt/dict_set_chain_escape_at_end.tw");
    assert_eq!(
        count_calls_to(&module, DICT_SET_IN_PLACE),
        3,
        "Expected 3 DICT_SET_IN_PLACE calls for sequential dict build with late escape"
    );
    assert_eq!(
        count_calls_to(&module, DICT_SET),
        0,
        "Expected no remaining DICT_SET calls after Phase A untainting"
    );
}

#[test]
fn opt_phase_a_dict_set_chain_alias_mid_not_rewritten() {
    // Dict is aliased mid-chain — escape before COW ops, must NOT untaint.
    let module = compile_opt("tests/opt/dict_set_chain_alias_mid_not_rewritten.tw");
    assert!(
        has_call_to(&module, DICT_SET),
        "Expected DICT_SET to remain when dict is aliased mid-chain"
    );
    assert!(
        !has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected no DICT_SET_IN_PLACE when dict is aliased mid-chain"
    );
}

#[test]
fn opt_phase_a_runtime_semantics() {
    assert_runtime_output(
        "tests/opt/dict_set_chain_escape_at_end.tw",
        &["3", "10", "20", "30"],
    );
    assert_runtime_output(
        "tests/opt/dict_set_chain_alias_mid_not_rewritten.tw",
        &["0", "1"],
    );
}

#[test]
fn opt_phase_a_wasm_semantics() {
    assert_runtime_output_wasm(
        "tests/opt/dict_set_chain_escape_at_end.tw",
        &["3", "10", "20", "30"],
    );
    assert_runtime_output_wasm(
        "tests/opt/dict_set_chain_alias_mid_not_rewritten.tw",
        &["0", "1"],
    );
}

// ── Phase B: fresh-after-COW tracking ────────────────────────────────────────

#[test]
fn opt_phase_b_dict_set_param_then_set_rewritten() {
    // COW on tainted param produces fresh result; second set should be in-place.
    let module = compile_opt("tests/opt/dict_set_param_then_set.tw");
    assert!(
        has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected DICT_SET_IN_PLACE for second set on fresh COW result"
    );
    assert!(
        has_call_to(&module, DICT_SET),
        "Expected DICT_SET to remain for first set on tainted param"
    );
}

#[test]
fn opt_phase_b_dict_set_param_result_aliased_not_rewritten() {
    // COW result is aliased before second set — must stay COW.
    let module = compile_opt("tests/opt/dict_set_param_result_aliased_not_rewritten.tw");
    assert_eq!(
        count_calls_to(&module, DICT_SET),
        2,
        "Expected both DICT_SET calls to remain when COW result is aliased"
    );
    assert_eq!(
        count_calls_to(&module, DICT_SET_IN_PLACE),
        0,
        "Expected no DICT_SET_IN_PLACE when COW result is aliased"
    );
}

#[test]
fn opt_phase_b_runtime_semantics() {
    assert_runtime_output("tests/opt/dict_set_param_then_set.tw", &["2", "10", "20"]);
    assert_runtime_output(
        "tests/opt/dict_set_param_result_aliased_not_rewritten.tw",
        &["1", "2"],
    );
}

#[test]
fn opt_phase_b_wasm_semantics() {
    assert_runtime_output_wasm("tests/opt/dict_set_param_then_set.tw", &["2", "10", "20"]);
    assert_runtime_output_wasm(
        "tests/opt/dict_set_param_result_aliased_not_rewritten.tw",
        &["1", "2"],
    );
}

// ── Characterization fixtures for known optimizer limits ─────────────────────

#[test]
fn opt_dict_set_param_assign_back_chain_rewritten_after_refresh() {
    let module = compile_opt("tests/opt/dict_set_param_assign_back_chain.tw");
    assert_eq!(
        count_calls_to(&module, DICT_SET),
        1,
        "Expected the first param-backed dict set to remain COW"
    );
    assert_eq!(
        count_calls_to(&module, DICT_SET_IN_PLACE),
        1,
        "Expected the second dict set to rewrite in-place after refresh"
    );
}

#[test]
fn opt_dict_set_param_forward_bind_chain_rewritten_after_relaxed_consume_reassign() {
    let module = compile_opt("tests/opt/dict_set_param_forward_bind_chain.tw");
    assert_eq!(
        count_calls_to(&module, DICT_SET),
        1,
        "Expected the first dict set to remain COW through the forward-bind refresh chain"
    );
    assert_eq!(
        count_calls_to(&module, DICT_SET_IN_PLACE),
        1,
        "Expected the second dict set to rewrite after forward-bind consume-reassign recognition"
    );
}

#[test]
fn opt_vector_append_helper_wrapper_rewritten_in_caller() {
    let module = compile_opt("tests/opt/vector_append_helper_wrapper.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_APPEND),
        1,
        "Expected the helper body to keep its own VECTOR_APPEND implementation"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_NEW),
        1,
        "Expected the caller to seed one builder for the helper-wrapped append chain"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_PUSH),
        3,
        "Expected the caller's helper-wrapped append chain to rewrite to builder pushes"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected one builder freeze at the end of the rewritten helper-wrapped chain"
    );
}

#[test]
fn opt_vector_append_helper_forward_wrapper_rewritten_in_caller() {
    let module = compile_opt("tests/opt/vector_append_helper_forward_wrapper.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_APPEND),
        1,
        "Expected the forwarding helper body to keep its own VECTOR_APPEND implementation"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_NEW),
        1,
        "Expected the caller to seed one builder for the forwarding helper chain"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_PUSH),
        3,
        "Expected the caller's forwarding helper chain to rewrite to builder pushes"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected one builder freeze at the end of the rewritten forwarding helper chain"
    );
}

#[test]
fn opt_dict_set_helper_wrapper_rewritten_in_caller() {
    let module = compile_opt("tests/opt/dict_set_helper_wrapper.tw");
    assert_eq!(
        count_calls_to(&module, DICT_SET),
        1,
        "Expected the helper body to keep its own DICT_SET implementation"
    );
    assert_eq!(
        count_calls_to(&module, DICT_SET_IN_PLACE),
        2,
        "Expected both helper-wrapped dict sets in the caller to rewrite in place"
    );
}

#[test]
fn opt_dict_set_helper_forward_wrapper_rewritten_in_caller() {
    let module = compile_opt("tests/opt/dict_set_helper_forward_wrapper.tw");
    assert_eq!(
        count_calls_to(&module, DICT_SET),
        1,
        "Expected the forwarding helper body to keep its own DICT_SET implementation"
    );
    assert_eq!(
        count_calls_to(&module, DICT_SET_IN_PLACE),
        2,
        "Expected both forwarding helper-wrapped dict sets in the caller to rewrite in place"
    );
}

#[test]
fn opt_dict_remove_helper_wrapper_rewritten_in_caller() {
    let module = compile_opt("tests/opt/dict_remove_helper_wrapper.tw");
    assert_eq!(
        count_calls_to(&module, DICT_REMOVE),
        1,
        "Expected the helper body to keep its own DICT_REMOVE implementation"
    );
    assert_eq!(
        count_calls_to(&module, DICT_REMOVE_IN_PLACE),
        2,
        "Expected both helper-wrapped dict removes in the caller to rewrite in place"
    );
}

#[test]
fn opt_dict_remove_helper_forward_wrapper_rewritten_in_caller() {
    let module = compile_opt("tests/opt/dict_remove_helper_forward_wrapper.tw");
    assert_eq!(
        count_calls_to(&module, DICT_REMOVE),
        1,
        "Expected the forwarding helper body to keep its own DICT_REMOVE implementation"
    );
    assert_eq!(
        count_calls_to(&module, DICT_REMOVE_IN_PLACE),
        2,
        "Expected both forwarding helper-wrapped dict removes in the caller to rewrite in place"
    );
}

#[test]
fn opt_dict_set_after_if_join_propagates_post_join_uniqueness() {
    let module = compile_opt("tests/opt/dict_set_after_if_join.tw");
    assert_eq!(
        count_calls_to(&module, DICT_SET_IN_PLACE),
        1,
        "Expected the post-join dict set to rewrite in-place after branch merge propagation"
    );
    assert_eq!(
        count_calls_to(&module, DICT_SET),
        2,
        "Expected only the branch-local param updates to remain COW"
    );
}

#[test]
fn opt_characterization_runtime_semantics() {
    assert_runtime_output(
        "tests/opt/dict_set_param_assign_back_chain.tw",
        &["2", "10", "20"],
    );
    assert_runtime_output(
        "tests/opt/dict_set_param_forward_bind_chain.tw",
        &["2", "10", "20"],
    );
    assert_runtime_output("tests/opt/vector_append_helper_wrapper.tw", &["3", "3"]);
    assert_runtime_output(
        "tests/opt/vector_append_helper_forward_wrapper.tw",
        &["3", "3"],
    );
    assert_runtime_output("tests/opt/dict_set_helper_wrapper.tw", &["2", "10", "20"]);
    assert_runtime_output(
        "tests/opt/dict_set_helper_forward_wrapper.tw",
        &["2", "10", "20"],
    );
    assert_runtime_output(
        "tests/opt/dict_remove_helper_wrapper.tw",
        &["1", "false", "true"],
    );
    assert_runtime_output(
        "tests/opt/dict_remove_helper_forward_wrapper.tw",
        &["1", "false", "true"],
    );
    assert_runtime_output(
        "tests/opt/dict_set_after_if_join.tw",
        &["2", "10", "30", "2", "20", "30"],
    );
}

#[test]
fn opt_characterization_wasm_semantics() {
    assert_runtime_output_wasm(
        "tests/opt/dict_set_param_assign_back_chain.tw",
        &["2", "10", "20"],
    );
    assert_runtime_output_wasm(
        "tests/opt/dict_set_param_forward_bind_chain.tw",
        &["2", "10", "20"],
    );
    assert_runtime_output_wasm("tests/opt/vector_append_helper_wrapper.tw", &["3", "3"]);
    assert_runtime_output_wasm(
        "tests/opt/vector_append_helper_forward_wrapper.tw",
        &["3", "3"],
    );
    assert_runtime_output_wasm("tests/opt/dict_set_helper_wrapper.tw", &["2", "10", "20"]);
    assert_runtime_output_wasm(
        "tests/opt/dict_set_helper_forward_wrapper.tw",
        &["2", "10", "20"],
    );
    assert_runtime_output_wasm(
        "tests/opt/dict_remove_helper_wrapper.tw",
        &["1", "false", "true"],
    );
    assert_runtime_output_wasm(
        "tests/opt/dict_remove_helper_forward_wrapper.tw",
        &["1", "false", "true"],
    );
    assert_runtime_output_wasm(
        "tests/opt/dict_set_after_if_join.tw",
        &["2", "10", "30", "2", "20", "30"],
    );
}

// ── Dict loop in-place rewrite ───────────────────────────────────────────────

#[test]
fn opt_dict_set_loop_unique_rewritten_to_in_place() {
    let module = compile_opt("tests/opt/dict_set_loop_unique.tw");
    assert!(
        has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected DICT_SET in loop to rewrite to DICT_SET_IN_PLACE for unique dict"
    );
    assert!(
        !has_call_to(&module, DICT_SET),
        "Expected no remaining DICT_SET calls after loop rewrite"
    );
}

#[test]
fn opt_dict_remove_loop_unique_rewritten_to_in_place() {
    let module = compile_opt("tests/opt/dict_remove_loop_unique.tw");
    // The build loop has dict_set, the remove loop has dict_remove — both should be rewritten
    assert!(
        has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected DICT_SET in loop to rewrite to DICT_SET_IN_PLACE"
    );
    assert!(
        has_call_to(&module, DICT_REMOVE_IN_PLACE),
        "Expected DICT_REMOVE in loop to rewrite to DICT_REMOVE_IN_PLACE"
    );
    assert!(
        !has_call_to(&module, DICT_SET),
        "Expected no remaining DICT_SET calls"
    );
    assert!(
        !has_call_to(&module, DICT_REMOVE),
        "Expected no remaining DICT_REMOVE calls"
    );
}

#[test]
fn opt_dict_set_loop_aliased_not_rewritten() {
    let module = compile_opt("tests/opt/dict_set_loop_aliased_not_rewritten.tw");
    assert!(
        has_call_to(&module, DICT_SET),
        "Expected DICT_SET to remain when dict is aliased before loop"
    );
    assert!(
        !has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected no DICT_SET_IN_PLACE when dict is aliased"
    );
}

#[test]
fn opt_dict_set_loop_after_refresh_rewritten() {
    let module = compile_opt("tests/opt/dict_set_loop_after_refresh.tw");
    assert!(
        has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected DICT_SET_IN_PLACE for refreshed dict loop accumulator"
    );
    assert!(
        !has_call_to(&module, DICT_SET),
        "Expected no DICT_SET after refreshed dict loop rewrite"
    );
}

#[test]
fn opt_dict_set_loop_captured_not_rewritten() {
    let module = compile_opt("tests/opt/dict_set_loop_captured_not_rewritten.tw");
    assert!(
        has_call_to(&module, DICT_SET),
        "Expected DICT_SET to remain when dict is closure-captured in loop"
    );
    assert!(
        !has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected no DICT_SET_IN_PLACE when dict is closure-captured"
    );
}

#[test]
fn opt_dict_set_loop_multiple_ops_rewritten() {
    let module = compile_opt("tests/opt/dict_set_loop_multiple_ops.tw");
    assert_eq!(
        count_calls_to(&module, DICT_SET_IN_PLACE),
        2,
        "Expected two DICT_SET_IN_PLACE calls (two sets per iteration)"
    );
    assert_eq!(
        count_calls_to(&module, DICT_SET),
        0,
        "Expected no DICT_SET calls after rewrite"
    );
}

#[test]
fn opt_dict_set_loop_with_read_rewritten() {
    let module = compile_opt("tests/opt/dict_set_loop_with_read.tw");
    assert!(
        has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected DICT_SET to rewrite despite dict reads in same loop"
    );
    assert!(
        !has_call_to(&module, DICT_SET),
        "Expected no remaining DICT_SET calls"
    );
}

#[test]
fn opt_dict_set_loop_helper_wrapper_rewritten() {
    let module = compile_opt("tests/opt/dict_set_loop_helper_wrapper.tw");
    assert!(
        has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected helper-wrapped DICT_SET in loop to rewrite in place"
    );
    assert_eq!(
        count_calls_to(&module, DICT_SET),
        1,
        "Expected only the helper body to keep its own DICT_SET implementation"
    );
}

#[test]
fn opt_dict_remove_loop_helper_wrapper_rewritten() {
    let module = compile_opt("tests/opt/dict_remove_loop_helper_wrapper.tw");
    assert!(
        has_call_to(&module, DICT_REMOVE_IN_PLACE),
        "Expected helper-wrapped DICT_REMOVE in loop to rewrite in place"
    );
    assert_eq!(
        count_calls_to(&module, DICT_REMOVE),
        1,
        "Expected only the helper body to keep its own DICT_REMOVE implementation"
    );
}

#[test]
fn opt_dict_set_loop_helper_forward_wrapper_rewritten() {
    let module = compile_opt("tests/opt/dict_set_loop_helper_forward_wrapper.tw");
    assert!(
        has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected forwarding helper-wrapped DICT_SET in loop to rewrite in place"
    );
    assert_eq!(
        count_calls_to(&module, DICT_SET),
        1,
        "Expected only the forwarding helper body to keep its own DICT_SET implementation"
    );
}

#[test]
fn opt_dict_remove_loop_helper_forward_wrapper_rewritten() {
    let module = compile_opt("tests/opt/dict_remove_loop_helper_forward_wrapper.tw");
    assert!(
        has_call_to(&module, DICT_REMOVE_IN_PLACE),
        "Expected forwarding helper-wrapped DICT_REMOVE in loop to rewrite in place"
    );
    assert_eq!(
        count_calls_to(&module, DICT_REMOVE),
        1,
        "Expected only the forwarding helper body to keep its own DICT_REMOVE implementation"
    );
}

#[test]
fn opt_dict_loop_runtime_semantics() {
    assert_runtime_output("tests/opt/dict_set_loop_unique.tw", &["10"]);
    assert_runtime_output("tests/opt/dict_remove_loop_unique.tw", &["0"]);
    assert_runtime_output(
        "tests/opt/dict_set_loop_aliased_not_rewritten.tw",
        &["0", "3"],
    );
    assert_runtime_output("tests/opt/dict_set_loop_after_refresh.tw", &["3", "1", "2"]);
    assert_runtime_output("tests/opt/dict_set_loop_captured_not_rewritten.tw", &["3"]);
    assert_runtime_output("tests/opt/dict_set_loop_multiple_ops.tw", &["20"]);
    assert_runtime_output("tests/opt/dict_set_loop_with_read.tw", &["5"]);
    assert_runtime_output("tests/opt/dict_set_loop_helper_wrapper.tw", &["10"]);
    assert_runtime_output("tests/opt/dict_remove_loop_helper_wrapper.tw", &["0"]);
    assert_runtime_output("tests/opt/dict_set_loop_helper_forward_wrapper.tw", &["10"]);
    assert_runtime_output(
        "tests/opt/dict_remove_loop_helper_forward_wrapper.tw",
        &["0"],
    );
}

#[test]
fn opt_dict_loop_wasm_semantics() {
    assert_runtime_output_wasm("tests/opt/dict_set_loop_unique.tw", &["10"]);
    assert_runtime_output_wasm("tests/opt/dict_remove_loop_unique.tw", &["0"]);
    assert_runtime_output_wasm(
        "tests/opt/dict_set_loop_aliased_not_rewritten.tw",
        &["0", "3"],
    );
    assert_runtime_output_wasm("tests/opt/dict_set_loop_multiple_ops.tw", &["20"]);
    assert_runtime_output_wasm("tests/opt/dict_set_loop_with_read.tw", &["5"]);
    assert_runtime_output_wasm("tests/opt/dict_set_loop_helper_wrapper.tw", &["10"]);
    assert_runtime_output_wasm("tests/opt/dict_remove_loop_helper_wrapper.tw", &["0"]);
    assert_runtime_output_wasm("tests/opt/dict_set_loop_helper_forward_wrapper.tw", &["10"]);
    assert_runtime_output_wasm(
        "tests/opt/dict_remove_loop_helper_forward_wrapper.tw",
        &["0"],
    );
}

#[test]
fn opt_record_escape_runtime_semantics() {
    assert_runtime_output("tests/opt/record_alias_escape_not_in_place.tw", &["1"]);
    assert_runtime_output("tests/opt/record_capture_escape_not_in_place.tw", &["1"]);
}

#[test]
fn opt_record_escape_wasm_semantics() {
    assert_runtime_output_wasm("tests/opt/record_alias_escape_not_in_place.tw", &["1"]);
    assert_runtime_output_wasm("tests/opt/record_capture_escape_not_in_place.tw", &["1"]);
}

#[test]
fn opt_record_update_chain_runtime_semantics() {
    assert_runtime_output(
        "tests/opt/record_update_chain_escape_at_end.tw",
        &["10", "20", "30", "4"],
    );
    assert_runtime_output(
        "tests/opt/record_update_chain_alias_mid_not_in_place.tw",
        &["2", "9"],
    );
}

#[test]
fn opt_record_update_chain_wasm_semantics() {
    assert_runtime_output_wasm(
        "tests/opt/record_update_chain_escape_at_end.tw",
        &["10", "20", "30", "4"],
    );
    assert_runtime_output_wasm(
        "tests/opt/record_update_chain_alias_mid_not_in_place.tw",
        &["2", "9"],
    );
}

#[test]
fn opt_vector_concat_guard_fresh_rewritten_to_builder() {
    // An accumulator that is `[]` should survive an early-return guard branch
    // and still be eligible for concat builder rewrite on the continuation path.
    let module = compile_opt("tests/opt/vector_concat_guard_fresh_rewritten.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_CONCAT),
        0,
        "Expected no VECTOR_CONCAT: acc stays known-empty past early-return guard"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_NEW),
        1,
        "Expected VECTOR_BUILDER_NEW for fresh empty accumulator concat"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_EXTEND),
        1,
        "Expected VECTOR_BUILDER_EXTEND for concat lowering"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected VECTOR_BUILDER_FREEZE for builder result"
    );
}

#[test]
fn opt_vector_concat_guard_fresh_runtime_semantics() {
    assert_runtime_output(
        "tests/opt/vector_concat_guard_fresh_rewritten.tw",
        &["3", "6"],
    );
}

#[test]
fn opt_dict_set_fresh_first_in_place() {
    // Dict.new() should have its FIRST set rewritten to DICT_SET_IN_PLACE.
    // Before source_fresh, only the second+ set was in-place (via refreshed).
    let module = compile_opt("tests/opt/dict_set_fresh_first_in_place.tw");
    assert_eq!(
        count_calls_to(&module, DICT_SET),
        0,
        "Expected no COW DICT_SET: all sets on fresh Dict.new() should be in-place"
    );
    assert_eq!(
        count_calls_to(&module, DICT_SET_IN_PLACE),
        2,
        "Expected both dict sets to be DICT_SET_IN_PLACE via source_fresh"
    );
}

#[test]
fn opt_dict_set_fresh_first_in_place_runtime() {
    assert_runtime_output(
        "tests/opt/dict_set_fresh_first_in_place.tw",
        &["2", "10", "20"],
    );
}

#[test]
fn opt_vector_append_nonempty_init_loop_rewritten_to_builder() {
    // A non-empty array literal moved to a tainted local via init should be
    // eligible for loop builder rewrites (builder_from + loop push + freeze).
    let module = compile_opt("tests/opt/vector_append_nonempty_init_loop.tw");
    assert_eq!(
        count_calls_to(&module, VECTOR_APPEND),
        0,
        "Expected no VECTOR_APPEND: non-empty init source_fresh enables loop builder"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FROM),
        1,
        "Expected VECTOR_BUILDER_FROM for non-empty initial content"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_PUSH),
        1,
        "Expected VECTOR_BUILDER_PUSH in loop"
    );
    assert_eq!(
        count_calls_to(&module, VECTOR_BUILDER_FREEZE),
        1,
        "Expected VECTOR_BUILDER_FREEZE after loop"
    );
}

#[test]
fn opt_vector_append_nonempty_init_loop_runtime_semantics() {
    assert_runtime_output(
        "tests/opt/vector_append_nonempty_init_loop.tw",
        &["6", "15"],
    );
}

// ── Fresh-wrapper destructuring tests ───────────────────────────────────────

/// Fresh wrapper helper returning .{ ctx, func_id } should let the caller move
/// r.ctx into a unique/refreshed local and then rewrite the compound dict
/// update on ctx.table in place.
#[test]
fn opt_fresh_wrapper_destructure_dict_rewritten_in_place() {
    let module = compile_opt("tests/opt/fresh_wrapper_destructure_dict.tw");
    assert!(
        has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected DICT_SET_IN_PLACE after fresh-wrapper destructuring"
    );
    assert!(
        !has_call_to(&module, DICT_SET),
        "Expected no remaining DICT_SET after fresh-wrapper destructuring"
    );
}

/// Re-reading the same field from the fresh wrapper keeps an alias path alive,
/// so the first extraction must not gain deep uniqueness.
#[test]
fn opt_fresh_wrapper_destructure_reread_same_field_not_rewritten() {
    let module = compile_opt("tests/opt/fresh_wrapper_destructure_reread_not_rewritten.tw");
    assert!(
        has_call_to(&module, DICT_SET),
        "Expected DICT_SET to remain when the wrapper field is re-read"
    );
    assert!(
        !has_call_to(&module, DICT_SET_IN_PLACE),
        "Expected no DICT_SET_IN_PLACE when the wrapper field is re-read"
    );
}

#[test]
fn opt_fresh_wrapper_destructure_runtime_semantics() {
    assert_runtime_output("tests/opt/fresh_wrapper_destructure_dict.tw", &["42"]);
}

// ── Field-borrow optimization tests ──────────────────────────────────────────

/// Field-borrow: struct with dict fields, multiple compound updates.
/// After the first update (COW), ctx becomes unique via ARecordUpdate + AAssign.
/// The second and third updates should fire field-borrow, producing DICT_SET_IN_PLACE.
#[test]
fn opt_field_borrow_dict_second_and_third_updates_in_place() {
    let module = compile_opt("tests/opt/field_borrow_dict.tw");
    let in_place = count_calls_to(&module, DICT_SET_IN_PLACE);
    let cow = count_calls_to(&module, DICT_SET);
    assert!(
        in_place >= 2,
        "Expected at least 2 DICT_SET_IN_PLACE (field-borrow on cache + extra), got {}; DICT_SET={}",
        in_place,
        cow
    );
}

/// Field-borrow runtime: value inserted in each dict field must be observable.
#[test]
fn opt_field_borrow_dict_runtime_semantics() {
    assert_runtime_output("tests/opt/field_borrow_dict.tw", &["42"]);
}
