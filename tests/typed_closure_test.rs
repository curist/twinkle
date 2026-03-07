//! Stage 9.6 — Typed Closure Specialization tests.
//!
//! Regression tests for typed closure specialization.
//! These assertions validate that typed closure emission is active, reduces
//! anyref arg-boxing at call sites, and preserves runtime behaviour.
//!
//! Run: `cargo test --test typed_closure_test -- --nocapture`

use std::collections::HashMap;
use std::path::PathBuf;

use twinkle::codegen::emit::{emit_user_module, emit_user_module_typed};
use twinkle::ir::lower_anf::lower_module;
use twinkle::ir::monomorphize::monomorphize;
use twinkle::opt::optimize_module;
use twinkle::runtime;
use twinkle::wasm::{emit::emit_wat, linker::link};

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/run")
        .join(name)
        .to_string_lossy()
        .to_string()
}

/// Build WAT with monomorphization but WITHOUT typed closure specialization
/// (universal anyref boxing — Stage 9.5 baseline).
fn build_wat_with_mono(file_path: &str) -> String {
    let (core_module, _) = twinkle::module::compile_entry(file_path).expect("compile failed");
    let core_module = monomorphize(core_module);
    let anf = lower_module(&core_module);
    let optimized = optimize_module(anf);
    let user_module = emit_user_module(&optimized, &core_module.type_env, &HashMap::new());
    let mut modules = runtime::all_modules();
    modules.push(user_module);
    let linked = link(modules, None).expect("link failed");
    emit_wat(&linked)
}

/// Build WAT with monomorphization AND typed closure specialization (Stage 9.6).
fn build_wat_with_typed_closure(file_path: &str) -> String {
    let (core_module, _) = twinkle::module::compile_entry(file_path).expect("compile failed");
    let core_module = monomorphize(core_module);
    let anf = lower_module(&core_module);
    let optimized = optimize_module(anf);
    let user_module = emit_user_module_typed(&optimized, &core_module.type_env, &HashMap::new());
    let mut modules = runtime::all_modules();
    modules.push(user_module);
    let linked = link(modules, None).expect("link failed");
    emit_wat(&linked)
}

/// Count occurrences of `array.new_fixed` inside user function bodies.
/// These represent argument-boxing operations for universal closure calls.
fn count_array_new_fixed_in_user_funcs(wat: &str) -> usize {
    let mut in_user = false;
    let mut depth: i32 = 0;
    let mut count = 0;
    for line in wat.lines() {
        let trimmed = line.trim();
        if trimmed.contains("$user__func_") && trimmed.starts_with("(func") {
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
            if in_user && trimmed.contains("array.new_fixed") {
                count += 1;
            }
        }
    }
    count
}

fn find_func_block_containing<'a>(wat: &'a str, needle: &str) -> Option<String> {
    let lines = wat.lines().collect::<Vec<_>>();
    for (start, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !(trimmed.starts_with("(func") && trimmed.contains("$user__func_")) {
            continue;
        }

        let mut depth: i32 = 0;
        let mut block = Vec::new();
        for line in &lines[start..] {
            let trimmed = line.trim();
            block.push(*line);
            for ch in trimmed.chars() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            let joined = block.join("\n");
                            if joined.contains(needle) {
                                return Some(joined);
                            }
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    None
}

fn find_named_func_block(wat: &str, func_name: &str) -> Option<String> {
    let lines = wat.lines().collect::<Vec<_>>();
    for (start, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !(trimmed.starts_with("(func") && trimmed.contains(func_name)) {
            continue;
        }

        let mut depth: i32 = 0;
        let mut block = Vec::new();
        for line in &lines[start..] {
            let trimmed = line.trim();
            block.push(*line);
            for ch in trimmed.chars() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(block.join("\n"));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    None
}

// ─── Baseline sanity checks (should pass both before and after Stage 9.6) ────

/// The universal-closure (mono-only) baseline DOES box args into an anyref
/// array for each closure call — this confirms the benchmark is exercising
/// the right path.
#[test]
fn baseline_mono_closure_uses_arg_boxing() {
    let path = fixture("fold_small.tw");
    let wat = build_wat_with_mono(&path);
    let count = count_array_new_fixed_in_user_funcs(&wat);
    assert!(
        count > 0,
        "Expected universal closure to use array.new_fixed for arg boxing in user funcs, got 0"
    );
}

// ─── Stage 9.6 assertions ──────────────────────────────────────────────────────

/// After typed closure specialization the WAT must contain at least one typed
/// closure func-type definition (e.g. `closurefunc_i64_i64_i64`).
#[test]
fn typed_closure_emit_produces_typed_closurefunc_types() {
    let path = fixture("fold_small.tw");
    let wat = build_wat_with_typed_closure(&path);
    assert!(
        wat.contains("closurefunc_"),
        "Expected typed closure func type (e.g. 'closurefunc_i64_i64_i64') in WAT."
    );
}

/// After typed closure specialization the number of arg-boxing `array.new_fixed`
/// operations in user function bodies must be strictly lower than the universal
/// baseline — ideally zero for fully-concrete call sites.
#[test]
fn typed_closure_call_eliminates_arg_boxing() {
    let path = fixture("fold_small.tw");
    let wat = build_wat_with_typed_closure(&path);
    let fold_block = find_func_block_containing(&wat, "(ref null $user__closure_i64_i64_i64)")
        .expect("expected specialized fold function in WAT");

    assert!(
        fold_block.contains("call_ref $user__closurefunc_i64_i64_i64"),
        "Expected specialized fold function to use typed call_ref.\n{fold_block}"
    );
    assert!(
        !fold_block.contains("call_ref $rt_types__ClosureFunc"),
        "Expected specialized fold function to avoid universal closure dispatch.\n{fold_block}"
    );
    assert!(
        !fold_block.contains("array.new_fixed $rt_types__Array 2"),
        "Expected specialized fold function to avoid per-call arg array boxing.\n{fold_block}"
    );
}

/// Typed closure specialization must not change observable behaviour.
/// Uses a small 10-element fold to keep the test fast.
#[test]
fn typed_closure_execution_produces_correct_output() {
    use twinkle::cli::run_wasm::{build_engine, execute_module};
    use wasmtime::Module;

    let path = fixture("fold_small.tw");
    let wat = build_wat_with_typed_closure(&path);
    let wasm = wat::parse_str(&wat).expect("WAT parse failed");

    let engine = build_engine().expect("engine");
    let module = Module::new(&engine, &wasm).expect("module");
    let (stdout, _stderr) = execute_module(&engine, &module).expect("execution failed");

    assert_eq!(
        stdout.trim(),
        "45",
        "fold_small.tw produced wrong output with typed closures"
    );
}

/// The normal build pipeline should also use typed closure specialization,
/// not just the explicit test-only emitter path.
#[test]
fn build_wat_uses_typed_closure_specialization() {
    let path = fixture("fold_small.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");
    assert!(
        wat.contains("closurefunc_"),
        "Expected build_wat output to contain typed closure func types"
    );

    let fold_block = find_func_block_containing(&wat, "(ref null $user__closure_i64_i64_i64)")
        .expect("expected specialized fold function in build_wat output");
    assert!(
        fold_block.contains("call_ref $user__closurefunc_i64_i64_i64")
            && !fold_block.contains("call_ref $rt_types__ClosureFunc")
            && !fold_block.contains("array.new_fixed $rt_types__Array 2"),
        "Expected build_wat to specialize the fold call site.\n{fold_block}"
    );
}

/// Named function values passed as first-class arguments should also
/// specialize to typed closures, not just anonymous `fn(...) { ... }` values.
#[test]
fn build_wat_specializes_named_function_args() {
    let path = fixture("generic_user_funcs.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    let apply_block =
        find_func_block_containing(&wat, "(param $p0 (ref null $user__closure_i64_i64))")
            .expect("expected monomorphized apply(Int, Int) block in build_wat output");
    assert!(
        apply_block.contains("call_ref $user__closurefunc_i64_i64")
            && !apply_block.contains("call_ref $rt_types__ClosureFunc")
            && !apply_block.contains("array.new_fixed $rt_types__Array 1"),
        "Expected named-function higher-order call to use typed closure dispatch.\n{apply_block}"
    );

    assert!(
        wat.contains("ref.func $user__func_43__typed_closure"),
        "Expected build_wat to materialize a typed closure for the named function argument"
    );
}

#[test]
fn build_wat_specializes_iterator_next_helper_for_concrete_unfold() {
    let path = fixture("iterator_direct_next.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    let helper = find_named_func_block(&wat, "$user__user____iterator_next__Int__Int")
        .expect("expected specialized iterator-next helper in build_wat output");
    assert!(
        helper.contains("struct.get $user__closure_i64_ref 2")
            && helper.contains("call_ref $user__closurefunc_i64_ref")
            && !helper.contains("call_ref $rt_types__ClosureFunc"),
        "Expected concrete iterator-next helper to use typed closure dispatch.\n{helper}"
    );
}
