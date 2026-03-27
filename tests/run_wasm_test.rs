mod common;

use std::path::Path;

fn check(path: &str) {
    common::assert_wasm_fixture(Path::new(path));
}

#[test]
fn run_wasm_hello() {
    check("tests/run/hello.tw");
}

#[test]
fn run_wasm_arithmetic() {
    check("tests/run/arithmetic.tw");
}

#[test]
fn run_wasm_collect_parity() {
    check("tests/run/collect_parity.tw");
}

#[test]
fn run_wasm_collect_while() {
    check("tests/run/collect_while.tw");
}

#[test]
fn run_wasm_collect_break_unfold() {
    check("tests/run/collect_break_unfold.tw");
}

#[test]
fn run_wasm_strings() {
    check("tests/run/strings.tw");
}

#[test]
fn run_wasm_strings_escape_hex() {
    check("tests/run/strings_escape_hex.tw");
}

#[test]
fn run_wasm_strings_escape_unicode() {
    check("tests/run/strings_escape_unicode.tw");
}

#[test]
fn run_wasm_string_methods() {
    check("tests/run/string_methods.tw");
}

#[test]
fn run_wasm_module_alias_shadow_local() {
    check("tests/run/module_alias_shadow_local.tw");
}

#[test]
fn run_wasm_module_alias_shadow_sibling() {
    check("tests/run/module_alias_shadow_sibling/main.tw");
}

#[test]
fn run_wasm_multi_module() {
    check("tests/run/multi_module/main.tw");
}

#[test]
fn run_wasm_multi_module_alias() {
    check("tests/run/multi_module_alias/main.tw");
}

#[test]
fn run_wasm_method_via_type_import() {
    check("tests/run/method_via_type_import/main.tw");
}

#[test]
fn run_wasm_method_via_type_import_selective() {
    check("tests/run/method_via_type_import_selective/main.tw");
}

#[test]
fn run_wasm_method_transitive_module() {
    check("tests/run/method_transitive_module/main.tw");
}

#[test]
fn run_wasm_method_via_type_import_aliased() {
    check("tests/run/method_via_type_import_aliased/main.tw");
}

#[test]
fn run_wasm_method_via_type_import_multi() {
    check("tests/run/method_via_type_import_multi/main.tw");
}

#[test]
fn run_wasm_method_via_type_import_first_class() {
    check("tests/run/method_via_type_import_first_class/main.tw");
}

#[test]
fn run_wasm_qualified_variant_constructor() {
    check("tests/run/qualified_variant_constructor/main.tw");
}

#[test]
fn run_wasm_qualified_variant_constructor_edge() {
    check("tests/run/qualified_variant_constructor_edge/main.tw");
}

#[test]
fn run_wasm_closures() {
    check("tests/run/closures.tw");
}

#[test]
fn run_wasm_closure_capture_cross_module() {
    check("tests/run/closure_capture_cross_module/main.tw");
}

#[test]
fn run_wasm_cell_update() {
    check("tests/run/cell_update.tw");
}

#[test]
fn run_wasm_defer_capture() {
    check("tests/run/defer_capture.tw");
}

#[test]
fn run_wasm_defer_return_loop_order() {
    check("tests/run/defer_return_loop_order.tw");
}

#[test]
fn run_wasm_for_break() {
    check("tests/run/for_break.tw");
}

#[test]
fn run_wasm_capability_records() {
    check("tests/run/capability_records.tw");
}

#[test]
fn run_wasm_record_field_punning() {
    check("tests/run/record_field_punning.tw");
}

#[test]
fn run_wasm_iterator_direct_next() {
    check("tests/run/iterator_direct_next.tw");
}

#[test]
fn run_wasm_iterator_first_class_return() {
    check("tests/run/iterator_first_class_return.tw");
}

#[test]
fn run_wasm_iterator_advanced() {
    check("tests/run/iterator_advanced.tw");
}

#[test]
fn run_wasm_iterator_for_loop() {
    check("tests/run/iterator_for_loop.tw");
}

#[test]
fn run_wasm_iterator_rebind_shape_change() {
    check("tests/run/iterator_rebind_shape_change.tw");
}

#[test]
fn run_wasm_iterator_unfold_collect_len() {
    check("tests/run/iterator_unfold_collect_len.tw");
}

#[test]
fn run_wasm_iterator_to_vector() {
    check("tests/run/iterator_to_vector.tw");
}

#[test]
fn run_wasm_iterator_unfold_callback_typing() {
    check("tests/run/iterator_unfold_callback_typing.tw");
}

#[test]
fn run_wasm_iterator_unfold_rebind_callback_typing() {
    check("tests/run/iterator_unfold_rebind_callback_typing.tw");
}

#[test]
fn run_wasm_iterator_unfold_nested_match_typing() {
    check("tests/run/iterator_unfold_nested_match_typing.tw");
}

#[test]
fn run_wasm_case_closure_pattern_binding() {
    check("tests/run/case_closure_pattern_binding.tw");
}

#[test]
fn run_wasm_unfold_step_match() {
    check("tests/run/unfold_step_match.tw");
}

#[test]
fn run_wasm_stdlib_path() {
    check("tests/run/stdlib_path.tw");
}

#[test]
fn run_wasm_stdlib_vector_string_ext() {
    check("tests/run/stdlib_vector_string_ext.tw");
}

#[test]
fn run_wasm_stdlib_numeric_dict_ext() {
    check("tests/run/stdlib_numeric_dict_ext.tw");
}

#[test]
fn run_wasm_numeric_parsing() {
    check("tests/run/numeric_parsing.tw");
}

#[test]
fn run_wasm_twinkle_typechecker() {
    check("tests/run/twinkle_typechecker.tw");
}

#[test]
fn run_wasm_stdlib_proc() {
    check("tests/run/stdlib_proc.tw");
}

#[test]
fn run_wasm_stderr_prelude() {
    check("tests/run/stderr_prelude.tw");
}

#[test]
fn run_wasm_and_short_circuit() {
    check("tests/run/and_short_circuit.tw");
}

#[test]
fn run_wasm_string_iteration_index() {
    check("tests/run/string_iteration_index.tw");
}

#[test]
fn run_wasm_string_get() {
    check("tests/run/string_get.tw");
}

#[test]
fn run_wasm_char_code_at() {
    check("tests/run/char_code_at.tw");
}

#[test]
fn run_wasm_string_large_index_semantics() {
    check("tests/run/string_large_index_semantics.tw");
}

#[test]
fn run_wasm_trap_string_index_oob() {
    check("tests/run/traps/string_index_oob.tw");
}

#[test]
fn run_wasm_trap_string_large_index_traps() {
    check("tests/run/traps/string_large_index_traps.tw");
}

#[test]
fn run_wasm_trap_string_slice_large_index() {
    check("tests/run/traps/string_slice_large_index.tw");
}

#[test]
fn run_wasm_string_from_code_point_large_int() {
    check("tests/run/string_from_code_point_large_int.tw");
}

#[test]
fn run_wasm_string_graphemes() {
    check("tests/run/string_graphemes.tw");
}

#[test]
fn run_wasm_byte_arithmetic_promotion() {
    check("tests/run/byte_arithmetic_promotion.tw");
}

#[test]
fn run_wasm_byte_int_comparison() {
    check("tests/run/byte_int_comparison.tw");
}

#[test]
fn run_wasm_byte_contextual_literals() {
    check("tests/run/byte_contextual_literals.tw");
}

#[test]
fn run_wasm_bitwise_ops() {
    check("tests/run/bitwise_ops.tw");
}

#[test]
fn run_wasm_option_assign_boundary() {
    check("tests/run/option_assign_boundary.tw");
}

#[test]
fn run_wasm_option_assign_match_boundary() {
    check("tests/run/option_assign_match_boundary.tw");
}

#[test]
fn run_wasm_option_amatch_typed_metadata() {
    check("tests/run/option_amatch_typed_metadata.tw");
}

#[test]
fn run_wasm_result_amatch_typed_metadata() {
    check("tests/run/result_amatch_typed_metadata.tw");
}

#[test]
fn run_wasm_closure_capture_cross_module2() {
    check("tests/run/closure_capture_cross_module2/main.tw");
}

#[test]
fn run_wasm_result_assign_boundary() {
    check("tests/run/result_assign_boundary.tw");
}

#[test]
fn run_wasm_result_typed_specialization() {
    check("tests/run/result_typed_specialization.tw");
}

#[test]
fn run_wasm_option_generic_boundary() {
    check("tests/run/option_generic_boundary.tw");
}

#[test]
fn run_wasm_option_record_field_boundary() {
    check("tests/run/option_record_field_boundary.tw");
}

#[test]
fn run_wasm_result_match_reassign_boundary() {
    check("tests/run/result_match_reassign_boundary.tw");
}

#[test]
fn run_wasm_option_branch_merge_boundary() {
    check("tests/run/option_branch_merge_boundary.tw");
}

#[test]
fn run_wasm_sum_cross_module_record() {
    check("tests/run/sum_cross_module_record/main.tw");
}

#[test]
fn run_wasm_sum_closure_capture_option_record() {
    check("tests/run/sum_closure_capture_option_record/main.tw");
}

#[test]
fn run_wasm_option_match_branch_reassign() {
    check("tests/run/option_match_branch_reassign.tw");
}

#[test]
fn run_wasm_sum_function_roundtrip() {
    check("tests/run/sum_function_roundtrip.tw");
}

#[test]
fn run_wasm_sum_record_field_roundtrip() {
    check("tests/run/sum_record_field_roundtrip.tw");
}

#[test]
fn run_wasm_sum_closure_return_boundary() {
    check("tests/run/sum_closure_return_boundary.tw");
}

/// Regression guard: same-scope/nested shadow (:=) + closure capture.
/// See tests/run/shadow_rebind_closure_capture.tw for details.
#[test]
fn run_wasm_shadow_rebind_closure_capture() {
    check("tests/run/shadow_rebind_closure_capture.tw");
}

#[test]
fn run_wasm_first_class_method() {
    check("tests/run/first_class_method.tw");
}

#[test]
fn run_wasm_first_class_method_once() {
    check("tests/run/first_class_method_once.tw");
}

#[test]
fn run_wasm_first_class_method_args() {
    check("tests/run/first_class_method_args.tw");
}

#[test]
fn run_wasm_first_class_method_hof() {
    check("tests/run/first_class_method_hof.tw");
}

#[test]
fn run_wasm_first_class_method_builtin() {
    check("tests/run/first_class_method_builtin.tw");
}

#[test]
fn run_wasm_first_class_method_registered() {
    check("tests/run/first_class_method_registered.tw");
}

#[test]
fn run_wasm_stdlib_callable_shape_characterization() {
    check("tests/run/stdlib_callable_shape_characterization.tw");
}

#[test]
fn run_wasm_first_class_method_cross_module() {
    check("tests/modules/method_value_ref/main.tw");
}
