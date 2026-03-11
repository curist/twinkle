//! Characterization tests for codegen boundary separation (Phases 0-2).
//!
//! These tests compile a single comprehensive `.tw` fixture through the full
//! backend pipeline and assert that:
//!   1. Both interpreter and Wasm produce identical, correct output.
//!   2. The generated WAT contains expected typed boundary structures
//!      (typed closures, typed Option/Result structs, typed iterator state,
//!       typed cells, trampolines).
//!
//! These tests serve as safety rails during the codegen refactor — any
//! structural regression in boundary handling will be caught here.

mod common;

use std::path::Path;

const FIXTURE: &str = "tests/run/codegen_boundary_characterization.tw";

fn fixture_path() -> String {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE)
        .to_string_lossy()
        .to_string()
}

fn build_wat() -> String {
    twinkle::cli::build::build_wat(&fixture_path())
        .expect("build_wat should succeed for boundary characterization fixture")
}

// ---------------------------------------------------------------------------
// 1. Behavioral correctness (interp + wasm agree with expected output)
// ---------------------------------------------------------------------------

#[test]
fn boundary_characterization_interp() {
    common::assert_interp_fixture(Path::new(FIXTURE));
}

#[test]
fn boundary_characterization_wasm() {
    common::assert_wasm_fixture(Path::new(FIXTURE));
}

#[test]
fn boundary_characterization_differential() {
    let interp =
        common::run_interp_capture(Path::new(FIXTURE)).expect("interpreter should succeed");
    let (wasm_stdout, _) =
        common::run_wasm_capture(Path::new(FIXTURE)).expect("wasm should succeed");
    assert_eq!(
        interp.lines().collect::<Vec<_>>(),
        wasm_stdout.lines().collect::<Vec<_>>(),
        "interpreter and wasm output must match for boundary characterization"
    );
}

// ---------------------------------------------------------------------------
// 2. WAT structural assertions — typed closure boundaries
// ---------------------------------------------------------------------------

#[test]
fn wat_contains_typed_closure_struct_for_int_to_int() {
    let wat = build_wat();
    assert!(
        wat.contains("$user__closure_i64_i64"),
        "WAT should contain typed closure struct for fn(Int) Int"
    );
    assert!(
        wat.contains("$user__closurefunc_i64_i64"),
        "WAT should contain typed closure functype for fn(Int) Int"
    );
}

#[test]
fn wat_contains_typed_closure_trampoline() {
    let wat = build_wat();
    // Typed trampolines use call_ref on the typed funcref
    assert!(
        wat.contains("call_ref $user__closurefunc_i64_i64"),
        "WAT should contain typed call_ref for fn(Int)->Int closure dispatch"
    );
}

#[test]
fn wat_contains_universal_closure_trampoline() {
    let wat = build_wat();
    // Universal (erased) trampolines use call_ref on the universal ClosureFunc
    assert!(
        wat.contains("call_ref $rt_types__ClosureFunc"),
        "WAT should contain universal call_ref for erased closure dispatch"
    );
}

// ---------------------------------------------------------------------------
// 3. WAT structural assertions — typed Option boundary
// ---------------------------------------------------------------------------

#[test]
fn wat_contains_typed_option_struct() {
    let wat = build_wat();
    assert!(
        wat.contains("$user__option__Int"),
        "WAT should contain typed Option<Int> struct"
    );
    // Typed option struct has variant_id + payload fields
    assert!(
        wat.contains("(type $user__option__Int (struct"),
        "typed Option<Int> should be a struct type"
    );
}

#[test]
fn wat_option_uses_typed_struct_get() {
    let wat = build_wat();
    assert!(
        wat.contains("struct.get $user__option__Int"),
        "WAT should access typed Option<Int> fields via struct.get"
    );
}

// ---------------------------------------------------------------------------
// 4. WAT structural assertions — iterator boundary
// ---------------------------------------------------------------------------

#[test]
fn wat_contains_typed_iterator_state() {
    let wat = build_wat();
    assert!(
        wat.contains("$user__iter_state__Int__Int"),
        "WAT should contain typed iterator state struct for Iterator<Int>"
    );
}

#[test]
fn wat_contains_typed_unfold_step() {
    let wat = build_wat();
    assert!(
        wat.contains("$user__unfold_step__Int__Int"),
        "WAT should contain typed UnfoldStep<Int, Int> struct"
    );
}

// ---------------------------------------------------------------------------
// 5. WAT structural assertions — typed Cell boundary
// ---------------------------------------------------------------------------

#[test]
fn wat_contains_typed_cell_struct() {
    let wat = build_wat();
    assert!(
        wat.contains("$user__cell_Int"),
        "WAT should contain typed Cell<Int> struct"
    );
}

// ---------------------------------------------------------------------------
// 6. WAT structural assertions — erased Variant type present for fallback
// ---------------------------------------------------------------------------

#[test]
fn wat_contains_erased_variant_type() {
    let wat = build_wat();
    assert!(
        wat.contains("$rt_types__Variant"),
        "WAT should contain the erased Variant runtime type for fallback paths"
    );
}

// ---------------------------------------------------------------------------
// 7. Boundary conversion helpers — typed→erased conversions exist
// ---------------------------------------------------------------------------

#[test]
fn wat_contains_typed_to_erased_cast_for_closure() {
    let wat = build_wat();
    // When a typed closure is passed where an erased one is expected (e.g., stored
    // in a Vector), the WAT should contain a ref.cast to the typed closure type.
    assert!(
        wat.contains("ref.cast (ref null $user__closure_i64_i64)"),
        "WAT should contain ref.cast for typed closure downcast"
    );
}

// ---------------------------------------------------------------------------
// 8. Boundary conversion instrumentation counters (debug builds only)
// ---------------------------------------------------------------------------

#[cfg(debug_assertions)]
#[test]
fn boundary_counters_are_nonzero_for_characterization_fixture() {
    use twinkle::codegen::emit::{boundary_counters, reset_boundary_counters};

    reset_boundary_counters();
    let _ = build_wat();
    let counters = boundary_counters();

    // Typed closure dispatch should fire for apply_int(adder, 5).
    assert!(
        counters.typed_closure_calls > 0,
        "expected at least one typed closure call, got {}",
        counters.typed_closure_calls
    );
    // Typed Option→erased conversion fires when passing Option<Int> across
    // function boundaries where the callee expects an erased Variant.
    assert!(
        counters.typed_option_to_erased > 0,
        "expected at least one typed Option→erased conversion, got {}",
        counters.typed_option_to_erased
    );
    // Iterator.unfold with concrete types should use the typed path.
    assert!(
        counters.typed_iterator_ops > 0,
        "expected at least one typed iterator op, got {}",
        counters.typed_iterator_ops
    );
    // Cell.new with a concrete Int should use the typed cell path.
    assert!(
        counters.typed_cell_ops > 0,
        "expected at least one typed cell op, got {}",
        counters.typed_cell_ops
    );
}

// ---------------------------------------------------------------------------
// 9. Phase 1 — ModuleEmitPlan extraction
// ---------------------------------------------------------------------------

#[test]
fn plan_builder_collects_concrete_func_sigs() {
    use twinkle::codegen::planner::build_module_emit_plan;

    let pipeline = twinkle::backend_pipeline::compile_backend_opt(&fixture_path())
        .expect("pipeline should succeed");
    let plan = build_module_emit_plan(
        &pipeline.optimized_anf_module,
        &pipeline.core_module.type_env,
    );

    // The fixture has `make_adder` returning fn(Int) Int — should be in concrete sigs.
    assert!(
        !plan.concrete_func_sigs.is_empty(),
        "plan should collect at least one concrete func sig"
    );
}

#[test]
fn plan_builder_collects_closure_capture_layouts() {
    use twinkle::codegen::planner::build_module_emit_plan;

    let pipeline = twinkle::backend_pipeline::compile_backend_opt(&fixture_path())
        .expect("pipeline should succeed");
    let plan = build_module_emit_plan(
        &pipeline.optimized_anf_module,
        &pipeline.core_module.type_env,
    );

    // The fixture has closures (make_adder returns fn(Int) Int with captured `n`).
    assert!(
        !plan.closure_capture_layouts.is_empty(),
        "plan should collect closure capture layouts"
    );
}

#[test]
fn plan_produces_same_wat_as_direct_emit() {
    use twinkle::codegen::planner::build_module_emit_plan;

    // The plan + emit path should produce identical WAT to direct emit.
    let wat_direct = build_wat();

    let pipeline = twinkle::backend_pipeline::compile_backend_opt(&fixture_path())
        .expect("pipeline should succeed");
    let plan = build_module_emit_plan(
        &pipeline.optimized_anf_module,
        &pipeline.core_module.type_env,
    );
    let wat_planned = plan.emit_wat(
        &pipeline.optimized_anf_module,
        &pipeline.core_module.type_env,
    );

    assert_eq!(
        wat_direct, wat_planned,
        "plan-based emission must produce identical WAT to direct emission"
    );
}

// ---------------------------------------------------------------------------
// 10. Phase 2 — ReprFlowCtx decomposition
// ---------------------------------------------------------------------------

#[test]
fn repr_flow_ctx_tracks_sum_repr() {
    use twinkle::codegen::ctx::{ReprFlowCtx, SumRepr};
    use twinkle::ir::LocalId;
    use twinkle::types::ty::{MonoType, OPTION_TYPE_ID};

    let mut flow = ReprFlowCtx::new();
    let local = LocalId(42);
    let option_int = MonoType::Named {
        type_id: OPTION_TYPE_ID,
        args: vec![MonoType::Int],
    };

    // Initially no repr
    assert!(flow.local_sum_repr(local).is_none());

    // Set typed option repr
    flow.set_local_sum_repr(local, Some(SumRepr::TypedOption(option_int.clone())));
    assert!(flow.local_sum_repr(local).unwrap().is_typed());

    // Push/restore scoped binding
    let prev = flow.push_flow_sum_repr_binding(local, Some(SumRepr::ErasedVariant));
    assert!(!flow.local_sum_repr(local).unwrap().is_typed());
    flow.restore_flow_sum_repr_binding(local, prev);
    assert!(flow.local_sum_repr(local).unwrap().is_typed());
}

#[test]
fn repr_flow_ctx_tracks_closure_locals() {
    use twinkle::codegen::ctx::ReprFlowCtx;
    use twinkle::ir::{FuncId, LocalId};

    let mut flow = ReprFlowCtx::new();
    let local = LocalId(10);
    let func = FuncId(5);

    assert!(flow.closure_local(local).is_none());
    flow.register_closure_local(local, func, vec![LocalId(1), LocalId(2)]);
    let (fid, captures) = flow.closure_local(local).unwrap();
    assert_eq!(*fid, func);
    assert_eq!(captures.len(), 2);
}

// ---------------------------------------------------------------------------
// 11. Phase 3 — Intrinsic dispatch layering
// ---------------------------------------------------------------------------

#[test]
fn intrinsic_spec_lowering_kind_matches_legacy_lookup() {
    // Verify that the unified `lowering_kind` field on IntrinsicSpec
    // agrees with the legacy `lowering_kind()` function for all specs.
    use twinkle::intrinsics::registry;

    for spec in registry::all_specs() {
        let legacy = registry::lowering_kind(spec.func_id);
        assert_eq!(
            spec.lowering_kind, legacy,
            "lowering_kind mismatch for {} (FuncId({}))",
            spec.twinkle_name, spec.func_id.0
        );
    }
}

// ---------------------------------------------------------------------------
// 12. Phase 4 — Boundary verifier runs without panics
// ---------------------------------------------------------------------------

#[test]
fn boundary_verifier_passes_for_characterization_fixture() {
    // The debug verifier runs automatically inside emit_user_module when
    // debug_assertions is enabled.  If it panics, this test will fail.
    // We simply call build_wat and assert it succeeds.
    let _ = build_wat();
}

#[test]
fn boundary_verifier_passes_for_all_run_fixtures() {
    use std::path::Path;

    let fixtures = common::discover_run_fixtures(Path::new("tests/run"));
    for fixture in fixtures {
        let name = fixture.path.file_name().unwrap().to_str().unwrap();
        if name.starts_with("bench_") || name == "fib_perf.tw" {
            continue;
        }
        let path_str = fixture.path.to_string_lossy().to_string();
        // This exercises the boundary verifier (via emit_user_module) for
        // every test fixture.  Any missing type definition will trigger a
        // debug_assert panic.
        let result = twinkle::cli::build::build_wat(&path_str);
        assert!(
            result.is_ok(),
            "boundary verifier or build_wat failed for {}: {}",
            name,
            result.unwrap_err()
        );
    }
}
