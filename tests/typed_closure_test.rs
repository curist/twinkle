//! Stage 9.6 — Typed Closure Specialization tests.
//!
//! Regression tests for typed closure specialization.
//! These assertions validate that typed closure emission is active, reduces
//! anyref arg-boxing at call sites, and preserves runtime behaviour.
//!
//! Run: `cargo test --test typed_closure_test -- --nocapture`

use std::path::PathBuf;

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/run")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn build_wat(file_path: &str) -> String {
    twinkle::cli::build::build_wat(file_path).expect("build_wat failed")
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

fn window_around_line(haystack: &str, needle: &str, radius: usize) -> Option<String> {
    let lines = haystack.lines().collect::<Vec<_>>();
    let center = lines.iter().position(|line| line.contains(needle))?;
    let start = center.saturating_sub(radius);
    let end = (center + radius + 1).min(lines.len());
    Some(lines[start..end].join("\n"))
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

// ─── Typed closure assertions ───────────────────────────────────────────────

/// The WAT must contain at least one typed closure func-type definition
/// (e.g. `closurefunc_i64_i64_i64`).
#[test]
fn typed_closure_emit_produces_typed_closurefunc_types() {
    let path = fixture("fold_small.tw");
    let wat = build_wat(&path);
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
    let wat = build_wat(&path);
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
    let wat = build_wat(&path);
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
        helper.contains("struct.get $user__closure_i64_")
            && helper.contains("call_ref $user__closurefunc_i64_")
            && helper.contains("struct.new $user__option__iter_item__Int__Int")
            && !helper.contains("call_ref $rt_types__ClosureFunc")
            && !helper.contains("struct.new $rt_types__Variant")
            && !helper.contains("array.new_fixed $rt_types__Array"),
        "Expected concrete iterator-next helper to use typed closure dispatch.\n{helper}"
    );
}

#[test]
fn build_wat_erases_unfold_step_at_function_boundary() {
    let path = fixture("unfold_step_match.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    let producer = find_func_block_containing(&wat, "(func $user__func_41")
        .expect("expected UnfoldStep producer in build_wat output");
    assert!(
        producer.contains("(result (ref null $rt_types__Variant))")
            && producer.contains("struct.new $user__unfold_step__Int__Int")
            && producer.contains("struct.new $rt_types__Variant"),
        "Expected producer to keep typed local construction but erase at the function boundary.\n{producer}"
    );

    let matcher = find_func_block_containing(&wat, "call $user__func_41")
        .expect("expected UnfoldStep consumer in build_wat output");
    assert!(
        matcher.contains("struct.get $rt_types__Variant 2")
            && !matcher.contains("ref.test (ref null $user__unfold_step__Int__Int)"),
        "Expected match lowering to consume erased Variant payloads at the boundary.\n{matcher}"
    );
}

#[test]
fn build_wat_specializes_iter_item_for_loop_consumption() {
    let path = fixture("iterator_for_loop.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    let loop_func = find_func_block_containing(&wat, "call $user__user____iterator_next__Int__Int")
        .expect("expected for-loop consumer in build_wat output");
    assert!(
        loop_func.contains("ref.test (ref null $user__option__iter_item__Int__Int)")
            && loop_func.contains("struct.get $user__option__iter_item__Int__Int 0")
            && loop_func.contains("struct.get $user__option__iter_item__Int__Int 1")
            && loop_func.contains("ref.cast (ref null $user__iter_item__Int__Int)")
            && loop_func.contains("struct.get $user__iter_item__Int__Int 0")
            && loop_func.contains("struct.get $user__iter_item__Int__Int 1")
            && loop_func.contains("struct.new $user__iter_state__Int__Int")
            && !loop_func.contains("struct.get $user__UserRecord_5")
            && !loop_func.contains("struct.get $rt_types__Variant"),
        "Expected for-loop lowering to consume typed Option/IterItem fields directly.\n{loop_func}"
    );
}

#[test]
fn build_wat_keeps_universal_iterator_next_for_erased_iterator_param() {
    let path = fixture("iterator_advanced.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    let fallback_consumer = find_func_block_containing(&wat, "call $user__user____iterator_next")
        .expect("expected erased iterator consumer to call the universal iterator-next helper");
    assert!(
        fallback_consumer.contains("struct.get $rt_types__Variant 2")
            && fallback_consumer.contains("array.get $rt_types__Array")
            && fallback_consumer.contains("struct.get $user__UserRecord_5 0"),
        "Expected erased iterator parameter path to keep the universal Variant/UserRecord fallback.\n{fallback_consumer}"
    );

    let erased_boundary = find_func_block_containing(&wat, "call $user__func_42")
        .expect("expected mixed typed-to-erased iterator boundary in build_wat output");
    assert!(
        erased_boundary.contains("struct.new $rt_types__IterState")
            && !erased_boundary
                .contains("ref.cast (ref null $rt_types__IterState)\n    call $user__func_42"),
        "Expected concrete iterator state to be wrapped into a universal IterState at the erased call boundary.\n{erased_boundary}"
    );
}

#[test]
fn build_wat_typed_closure_trampoline_uses_erased_iterator_return() {
    let path = fixture("iterator_first_class_return.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    let typed_trampoline = find_func_block_containing(&wat, "__typed_closure")
        .expect("expected typed closure trampoline in build_wat output");
    assert!(
        typed_trampoline.contains("(result (ref null $rt_types__IterState))")
            && !typed_trampoline.contains("(result (ref null $user__iter_state__Int__Int))"),
        "Expected typed closure trampoline to use erased iterator ABI at function boundaries.\n{typed_trampoline}"
    );
}

// ─── Phase 6 regressions: closure/cell local fast path vs erased ABI ─────────

/// The monomorphized fold function should use the typed closure struct as its param
/// (the specialization is explicit via monomorphization) and use typed dispatch internally.
/// The typed closure param and typed call_ref must agree — no mixed erased/typed boundary.
#[test]
fn typed_closure_param_and_dispatch_agree() {
    let path = fixture("fold_small.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    let fold_func = find_func_block_containing(&wat, "call_ref $user__closurefunc_i64_i64_i64")
        .expect("expected fold function using typed closure dispatch in build_wat output");

    // The specialized fold function takes a typed closure param
    assert!(
        fold_func.contains("(ref null $user__closure_i64_i64_i64))"),
        "Expected monomorphized fold to accept typed closure param.\n{fold_func}"
    );

    // And uses typed dispatch — both sides agree
    assert!(
        fold_func.contains("struct.get $user__closure_i64_i64_i64")
            && fold_func.contains("call_ref $user__closurefunc_i64_i64_i64")
            && !fold_func.contains("call_ref $rt_types__ClosureFunc"),
        "Expected fold function param type and internal dispatch to agree on typed closure.\n{fold_func}"
    );
}

/// Universal closure trampolines bridge typed function bodies back to the erased
/// calling convention. Their result type should be `anyref` (the universal return),
/// not a typed closure struct.
#[test]
fn closure_returning_function_abi_result_is_consistent() {
    let path = fixture("fold_small.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    // Find the universal closure trampoline — its name ends in `__closure` but
    // does NOT contain `__typed_closure`.
    let lines = wat.lines().collect::<Vec<_>>();
    let mut found_trampoline = false;
    for (start, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("(func")
            && trimmed.contains("__closure")
            && !trimmed.contains("__typed_closure")
            && trimmed.contains("$user__func_")
        {
            // Collect the full function block
            let mut depth: i32 = 0;
            let mut block = Vec::new();
            for l in &lines[start..] {
                block.push(*l);
                for ch in l.trim().chars() {
                    match ch {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                }
                if depth == 0 {
                    break;
                }
            }
            let trampoline = block.join("\n");
            // Universal trampolines return anyref, not a typed closure struct
            assert!(
                !trampoline.contains("(result (ref null $user__closure_i64"),
                "Universal trampoline should not return a typed closure struct.\n{trampoline}"
            );
            found_trampoline = true;
        }
    }
    assert!(
        found_trampoline,
        "Expected at least one universal closure trampoline in fold_small.tw WAT"
    );
}

/// After linking, all specialized type references emitted by the user module
/// should be properly qualified. No unqualified `$closure_`, `$cell_`, or
/// `$iter_state__` should survive linking.
#[test]
fn specialization_types_properly_qualified_after_linking() {
    let path = fixture("fold_small.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    for line in wat.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("(type ") {
            continue;
        }
        assert!(
            !trimmed.contains("$closure_i64") || trimmed.contains("$user__closure_i64"),
            "Found unqualified specialized closure type reference: {trimmed}"
        );
        assert!(
            !trimmed.contains("$closurefunc_i64") || trimmed.contains("$user__closurefunc_i64"),
            "Found unqualified specialized closurefunc type reference: {trimmed}"
        );
    }
}

/// Typed closure and cell type registration should go through the unified
/// `SpecializedTypeRegistry` — the same pipeline used by iterator types.
/// This test verifies that `emit_user_module` populates the registry
/// for closures and cells, ensuring a single source of truth.
#[test]
fn closure_and_cell_types_registered_through_unified_registry() {
    // fold_small.tw has a closure (fn(Int,Int) Int) — should register a typed closure
    let path = fixture("fold_small.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    // The WAT should contain both the closurefunc type and the closure struct
    assert!(
        wat.contains("$user__closurefunc_i64_i64_i64"),
        "Expected typed closurefunc type from registry.\n"
    );
    assert!(
        wat.contains("$user__closure_i64_i64_i64"),
        "Expected typed closure struct from registry.\n"
    );

    // cell_update.tw has Cell<Int> — should register a typed cell
    let path = fixture("cell_update.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    assert!(
        wat.contains("$user__cell_Int"),
        "Expected typed cell struct from registry.\n"
    );
}

/// Verify the ABI boundary policy: iterator-adjacent types use erased representation
/// at function boundaries, while closures and cells use typed representation.
/// This test documents the current policy explicitly so changes are surfaced.
#[test]
fn abi_boundary_policy_iterator_erased_closure_cell_typed() {
    // iterator_first_class_return.tw: function returns Iterator<Int>
    // The ABI should use erased IterState, not typed iter_state__Int__Int
    let path = fixture("iterator_first_class_return.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    // The mk function returns Iterator<Int> — should use erased IterState at boundary
    let mk_func = find_func_block_containing(&wat, "unfold")
        .expect("expected mk function in build_wat output");
    assert!(
        mk_func.contains("(result (ref null $rt_types__IterState))"),
        "Expected iterator-returning function to use erased IterState at ABI boundary.\n{mk_func}"
    );
    assert!(
        !mk_func.contains("(result (ref null $user__iter_state__"),
        "Iterator-returning function should NOT use typed iter_state at ABI boundary.\n{mk_func}"
    );

    // fold_small.tw: function takes fn(Int,Int) Int — should use typed closure at boundary
    let path = fixture("fold_small.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    let fold_func = find_func_block_containing(&wat, "call_ref $user__closurefunc_i64_i64_i64")
        .expect("expected fold function in build_wat output");
    assert!(
        fold_func.contains("(ref null $user__closure_i64_i64_i64)"),
        "Expected closure param to use typed closure struct at ABI boundary.\n{fold_func}"
    );

    // cell_update.tw: function takes Cell<Int> — should use typed cell at boundary
    let path = fixture("cell_update.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    // The apply_update function takes Cell<Int> — find it by searching for the
    // typed cell reference in a user function's parameter list
    let cell_func = find_func_block_containing(&wat, "(ref null $user__cell_Int)")
        .expect("expected a user function with typed cell param in build_wat output");
    assert!(
        cell_func.contains("(ref null $user__cell_Int)"),
        "Expected Cell param to use typed cell struct at ABI boundary.\n{cell_func}"
    );
}

// ─── Section C: General variant payload specialization ───────────────────────

/// When `Option<Int>` is created and matched locally (no function boundary),
/// the backend should use a typed option struct with an unboxed i64 field
/// instead of the universal Variant + payload array + BoxedInt path.
#[test]
fn typed_option_local_uses_specialized_struct() {
    let path = fixture("option_local_match.tw");
    let wat = twinkle::cli::build::build_wat(&path).expect("build_wat failed");

    // There should be a typed option struct for Option<Int>
    assert!(
        wat.contains("option__Int"),
        "Expected typed Option<Int> struct definition in WAT.\n"
    );

    // The main function should use typed struct creation and field access
    let main_func = find_func_block_containing(&wat, "struct.new $user__option__Int")
        .expect("expected typed Option<Int> literal in build_wat output");

    // Typed match should use struct.get on the typed option, not array.get on payload
    assert!(
        main_func.contains("struct.get $user__option__Int"),
        "Expected typed Option<Int> match to use direct struct field access.\n{main_func}"
    );
    let typed_window = window_around_line(&main_func, "struct.get $user__option__Int 1", 28)
        .expect("expected typed Option<Int> payload access in function body");
    assert!(
        !typed_window.contains("array.get $rt_types__Array"),
        "Expected typed Option<Int> match to avoid payload array indirection.\n{main_func}"
    );
}

/// Typed local option specialization must preserve behaviour when values cross
/// a function boundary that still expects universal Option layout.
#[test]
fn typed_option_boundary_call_preserves_behavior() {
    let path = fixture("option_boundary_call.tw");
    let (stdout, _stderr) =
        twinkle::cli::run_wasm::run_wasm_capture(&path).expect("wasm run should succeed");
    assert_eq!(
        stdout.trim(),
        "got 42\nnone",
        "option boundary call output mismatch"
    );
}
