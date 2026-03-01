/// ANF IR tests — invariant checker over all tests/run/*.tw programs.
///
/// These tests verify that the Core IR → ANF lowering pass produces
/// structurally correct ANF IR for every program in the test suite.
/// The key invariant: every operation's inputs are atoms (locals or literals),
/// not nested expressions.
use std::fs;
use std::path::Path;
use twinkle::ir::anf::{Atom, AnfExpr, AnfFunctionDef, AnfMatchArm, AnfModule, AnfOp};
use twinkle::ir::lower_anf;

// ── Invariant checker ─────────────────────────────────────────────────────────

/// Verify ANF invariants for a module.
///
/// Checks:
/// - The module was successfully lowered and has at least one function.
/// - The init_func_id (if set) is present in the functions list.
/// - All `Let` bodies are valid ANF expressions.
/// - All `ACall` callee and args are atoms (trivially true by type, but verified).
/// - All `ARecord` field values are atoms.
/// - All `AVariant` args are atoms.
/// - All `ABinOp`/`AUnOp` operands are atoms.
fn check_anf_invariants(module: &AnfModule, name: &str) {
    // Must have at least one function.
    assert!(
        !module.functions.is_empty(),
        "ANF module for '{}' has no functions",
        name
    );

    // If init_func_id is set, it must be in the functions list.
    if let Some(init_id) = module.init_func_id {
        assert!(
            module.functions.iter().any(|f| f.func_id == init_id),
            "ANF module for '{}': init_func_id {:?} not found in functions",
            name,
            init_id
        );
    }

    // Check each function.
    for func in &module.functions {
        check_anf_func(func, name);
    }
}

fn check_anf_func(func: &AnfFunctionDef, prog: &str) {
    check_anf_expr(&func.body, prog, &func.name);
}

fn check_anf_expr(expr: &AnfExpr, prog: &str, func: &str) {
    match expr {
        AnfExpr::Let { local: _, op, body } => {
            check_anf_op(op, prog, func);
            check_anf_expr(body, prog, func);
        }
        AnfExpr::Return(_) | AnfExpr::Break(_) | AnfExpr::Continue | AnfExpr::Atom(_) => {
            // Terminals and atoms are valid leaf nodes.
        }
    }
}

fn check_anf_op(op: &AnfOp, prog: &str, func: &str) {
    match op {
        AnfOp::ACall { callee, args } => {
            assert_is_atom(callee, "ACall.callee", prog, func);
            for arg in args {
                assert_is_atom(arg, "ACall.arg", prog, func);
            }
        }
        AnfOp::AIf { cond, then_branch, else_branch } => {
            assert_is_atom(cond, "AIf.cond", prog, func);
            check_anf_expr(then_branch, prog, func);
            check_anf_expr(else_branch, prog, func);
        }
        AnfOp::AMatch { scrutinee, arms } => {
            assert_is_atom(scrutinee, "AMatch.scrutinee", prog, func);
            for AnfMatchArm { body, .. } in arms {
                check_anf_expr(body, prog, func);
            }
        }
        AnfOp::ALoop { body } => {
            check_anf_expr(body, prog, func);
        }
        AnfOp::ABinOp { left, right, .. } => {
            assert_is_atom(left, "ABinOp.left", prog, func);
            assert_is_atom(right, "ABinOp.right", prog, func);
        }
        AnfOp::AUnOp { expr, .. } => {
            assert_is_atom(expr, "AUnOp.expr", prog, func);
        }
        AnfOp::AMakeClosure { .. } => {
            // free_vars are Vec<LocalId>, always atoms by construction.
        }
        AnfOp::ARecord { fields, .. } => {
            for (_, value) in fields {
                assert_is_atom(value, "ARecord.field", prog, func);
            }
        }
        AnfOp::ARecordGet { target, .. } => {
            assert_is_atom(target, "ARecordGet.target", prog, func);
        }
        AnfOp::ARecordUpdate { base, value, .. } => {
            assert_is_atom(base, "ARecordUpdate.base", prog, func);
            assert_is_atom(value, "ARecordUpdate.value", prog, func);
        }
        AnfOp::AVariant { args, .. } => {
            for arg in args {
                assert_is_atom(arg, "AVariant.arg", prog, func);
            }
        }
        AnfOp::AArrayLit(elems) => {
            for elem in elems {
                assert_is_atom(elem, "AArrayLit.elem", prog, func);
            }
        }
        AnfOp::AIndex { base, index } => {
            assert_is_atom(base, "AIndex.base", prog, func);
            assert_is_atom(index, "AIndex.index", prog, func);
        }
        AnfOp::AAssign { value, .. } => {
            assert_is_atom(value, "AAssign.value", prog, func);
        }
    }
}

/// Assert that an `Atom` is a valid atom (always true by type — this confirms
/// the lowering produced proper atoms rather than trying to embed expressions).
fn assert_is_atom(atom: &Atom, context: &str, prog: &str, func: &str) {
    // By construction, all AnfOp fields of type Atom ARE atoms.
    // This assertion documents and checks the structural invariant.
    match atom {
        Atom::ALocal(_)
        | Atom::AGlobalFunc(_)
        | Atom::ALitInt(_)
        | Atom::ALitFloat(_)
        | Atom::ALitBool(_)
        | Atom::ALitStr(_)
        | Atom::ALitVoid => {
            // Valid atom.
        }
    }
    // Also check that AAssign sentinel (LocalId::MAX) does not appear in real ops.
    if let Atom::ALocal(id) = atom {
        assert_ne!(
            id.0,
            u32::MAX,
            "Sentinel LocalId(MAX) found in {} for program '{}' function '{}'",
            context,
            prog,
            func
        );
    }
}

// ── Test helpers ──────────────────────────────────────────────────────────────

fn lower_anf_for(path: &str) -> AnfModule {
    let (core_module, _) = twinkle::module::compile_entry(path)
        .unwrap_or_else(|e| panic!("compile_entry failed for {}: {}", path, e));
    lower_anf::lower_module(&core_module)
}

fn check(path: &str) {
    // Verify the file exists.
    assert!(
        Path::new(path).exists(),
        "Test file not found: {}",
        path
    );
    let module = lower_anf_for(path);
    let prog_name = Path::new(path).file_name().unwrap().to_string_lossy().to_string();
    check_anf_invariants(&module, &prog_name);
}

// ── Individual test functions for each test/run/*.tw program ──────────────────

#[test]
fn anf_hello() { check("tests/run/hello.tw"); }

#[test]
fn anf_arithmetic() { check("tests/run/arithmetic.tw"); }

#[test]
fn anf_strings() { check("tests/run/strings.tw"); }

#[test]
fn anf_strings_escape() { check("tests/run/strings_escape.tw"); }

#[test]
fn anf_control_flow() { check("tests/run/control_flow.tw"); }

#[test]
fn anf_loops() { check("tests/run/loops.tw"); }

#[test]
fn anf_for_break() { check("tests/run/for_break.tw"); }

#[test]
fn anf_collect() { check("tests/run/collect.tw"); }

#[test]
fn anf_records() { check("tests/run/records.tw"); }

#[test]
fn anf_arrays() { check("tests/run/arrays.tw"); }

#[test]
fn anf_array_methods() { check("tests/run/array_methods.tw"); }

#[test]
fn anf_closures() { check("tests/run/closures.tw"); }

#[test]
fn anf_capability_records() { check("tests/run/capability_records.tw"); }

#[test]
fn anf_nested_field_update() { check("tests/run/nested_field_update.tw"); }

#[test]
fn anf_type_alias() { check("tests/run/type_alias.tw"); }

#[test]
fn anf_mutual_recursion() { check("tests/run/mutual_recursion.tw"); }

#[test]
fn anf_result_void() { check("tests/run/result_void.tw"); }

#[test]
fn anf_dicts() { check("tests/run/dicts.tw"); }

#[test]
fn anf_dict_methods() { check("tests/run/dict_methods.tw"); }

#[test]
fn anf_string_methods() { check("tests/run/string_methods.tw"); }

#[test]
fn anf_variant_collision() { check("tests/run/variant_collision.tw"); }

#[test]
fn anf_range() { check("tests/run/range.tw"); }

#[test]
fn anf_iterator() { check("tests/run/iterator.tw"); }

#[test]
fn anf_iterator_advanced() { check("tests/run/iterator_advanced.tw"); }

#[test]
fn anf_generic_types() { check("tests/run/generic_types.tw"); }

#[test]
fn anf_method_chaining() { check("tests/run/method_chaining.tw"); }

#[test]
fn anf_empty_array() { check("tests/run/empty_array.tw"); }

#[test]
fn anf_module_globals() { check("tests/run/module_globals.tw"); }

#[test]
fn anf_error_types() { check("tests/run/error_types.tw"); }

#[test]
fn anf_option_shorthand() { check("tests/run/option_shorthand.tw"); }

#[test]
fn anf_result_shorthand() { check("tests/run/result_shorthand.tw"); }

#[test]
fn anf_result_try() { check("tests/run/result_try.tw"); }

#[test]
fn anf_multi_module() { check("tests/run/multi_module/main.tw"); }

#[test]
fn anf_multi_module_alias() { check("tests/run/multi_module_alias/main.tw"); }

#[test]
fn anf_pub_values() { check("tests/run/pub_values/main.tw"); }

// Trap tests — these panic at the interpreter level, but the lowering to ANF
// should still succeed (trapping happens at runtime, not at compile time).
#[test]
fn anf_trap_array_oob() { check("tests/run/traps/array_oob.tw"); }

#[test]
fn anf_trap_div_zero() { check("tests/run/traps/div_zero.tw"); }

#[test]
fn anf_trap_error_call() { check("tests/run/traps/error_call.tw"); }

// ── Golden snapshot tests ─────────────────────────────────────────────────────
// These tests verify the exact ANF output for a few representative programs.
// Run with UPDATE_SNAPSHOTS=1 to regenerate the golden files.

fn snapshot_path(name: &str) -> String {
    format!("tests/snapshots/anf/{}.txt", name)
}

fn check_snapshot(tw_path: &str, snapshot_name: &str) {
    let module = lower_anf_for(tw_path);
    let actual = format!("{}", module);

    let snap_path = snapshot_path(snapshot_name);
    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        // Regenerate snapshot.
        let snap_dir = format!("tests/snapshots/anf");
        fs::create_dir_all(&snap_dir).expect("create snapshot dir");
        fs::write(&snap_path, &actual).expect("write snapshot");
        return;
    }

    if !Path::new(&snap_path).exists() {
        // No snapshot yet — create it on first run.
        let snap_dir = format!("tests/snapshots/anf");
        fs::create_dir_all(&snap_dir).expect("create snapshot dir");
        fs::write(&snap_path, &actual).expect("write snapshot");
        return;
    }

    let expected = fs::read_to_string(&snap_path)
        .unwrap_or_else(|_| panic!("Could not read snapshot: {}", snap_path));

    assert_eq!(
        actual, expected,
        "ANF snapshot mismatch for '{}'\n\
         To update: UPDATE_SNAPSHOTS=1 cargo test {}",
        tw_path, snapshot_name
    );
}

#[test]
fn anf_snapshot_hello() {
    check_snapshot("tests/run/hello.tw", "hello");
}

#[test]
fn anf_snapshot_arithmetic() {
    check_snapshot("tests/run/arithmetic.tw", "arithmetic");
}

#[test]
fn anf_snapshot_closures() {
    check_snapshot("tests/run/closures.tw", "closures");
}

#[test]
fn anf_snapshot_records() {
    check_snapshot("tests/run/records.tw", "records");
}
