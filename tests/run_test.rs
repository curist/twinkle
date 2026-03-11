mod common;

use std::path::Path;

fn check(path: &str) {
    common::assert_interp_fixture(Path::new(path));
}

fn check_trap(path: &str) {
    common::assert_interp_fixture(Path::new(path));
}

#[test]
fn hello() {
    check("tests/run/hello.tw");
}

#[test]
fn arithmetic() {
    check("tests/run/arithmetic.tw");
}

#[test]
fn strings() {
    check("tests/run/strings.tw");
}

#[test]
fn control_flow() {
    check("tests/run/control_flow.tw");
}

#[test]
fn loops() {
    check("tests/run/loops.tw");
}

#[test]
fn collect() {
    check("tests/run/collect.tw");
}

#[test]
fn collect_parity() {
    check("tests/run/collect_parity.tw");
}

#[test]
fn collect_while() {
    check("tests/run/collect_while.tw");
}

#[test]
fn records() {
    check("tests/run/records.tw");
}

#[test]
fn vectors() {
    check("tests/run/vectors.tw");
}

#[test]
fn closures() {
    check("tests/run/closures.tw");
}

#[test]
fn multi_module() {
    check("tests/run/multi_module/main.tw");
}

#[test]
fn variant_collision() {
    check("tests/run/variant_collision.tw");
}

#[test]
fn range() {
    check("tests/run/range.tw");
}

#[test]
fn dicts() {
    check("tests/run/dicts.tw");
}

#[test]
fn strings_escape() {
    check("tests/run/strings_escape.tw");
}

#[test]
fn for_break() {
    check("tests/run/for_break.tw");
}

#[test]
fn type_alias() {
    check("tests/run/type_alias.tw");
}

#[test]
fn mutual_recursion() {
    check("tests/run/mutual_recursion.tw");
}

#[test]
fn result_void() {
    check("tests/run/result_void.tw");
}

#[test]
fn capability_records() {
    check("tests/run/capability_records.tw");
}

#[test]
fn nested_field_update() {
    check("tests/run/nested_field_update.tw");
}

#[test]
fn vector_methods() {
    check("tests/run/vector_methods.tw");
}

#[test]
fn dict_methods() {
    check("tests/run/dict_methods.tw");
}

#[test]
fn string_methods() {
    check("tests/run/string_methods.tw");
}

#[test]
fn multi_module_alias() {
    check("tests/run/multi_module_alias/main.tw");
}

#[test]
fn pub_values() {
    check("tests/run/pub_values/main.tw");
}

#[test]
fn generic_types() {
    check("tests/run/generic_types.tw");
}

#[test]
fn method_chaining() {
    check("tests/run/method_chaining.tw");
}

#[test]
fn iterator() {
    check("tests/run/iterator.tw");
}

#[test]
fn iterator_advanced() {
    check("tests/run/iterator_advanced.tw");
}

#[test]
fn iterator_direct_next() {
    check("tests/run/iterator_direct_next.tw");
}

#[test]
fn iterator_first_class_return() {
    check("tests/run/iterator_first_class_return.tw");
}

#[test]
fn iterator_for_loop() {
    check("tests/run/iterator_for_loop.tw");
}

#[test]
fn iterator_rebind_shape_change() {
    check("tests/run/iterator_rebind_shape_change.tw");
}

#[test]
fn iterator_unfold_callback_typing() {
    check("tests/run/iterator_unfold_callback_typing.tw");
}

#[test]
fn iterator_unfold_rebind_callback_typing() {
    check("tests/run/iterator_unfold_rebind_callback_typing.tw");
}

#[test]
fn iterator_unfold_nested_match_typing() {
    check("tests/run/iterator_unfold_nested_match_typing.tw");
}

#[test]
fn case_closure_pattern_binding() {
    check("tests/run/case_closure_pattern_binding.tw");
}

#[test]
fn unfold_step_match() {
    check("tests/run/unfold_step_match.tw");
}

#[test]
fn stdlib_path() {
    check("tests/run/stdlib_path.tw");
}

#[test]
fn stdlib_vector_string_ext() {
    check("tests/run/stdlib_vector_string_ext.tw");
}

#[test]
fn stdlib_numeric_dict_ext() {
    check("tests/run/stdlib_numeric_dict_ext.tw");
}

#[test]
fn empty_vector() {
    check("tests/run/empty_vector.tw");
}

#[test]
fn module_globals() {
    check("tests/run/module_globals.tw");
}

#[test]
fn error_types() {
    check("tests/run/error_types.tw");
}

#[test]
fn option_shorthand() {
    check("tests/run/option_shorthand.tw");
}

#[test]
fn option_assign_boundary() {
    check("tests/run/option_assign_boundary.tw");
}

#[test]
fn option_assign_match_boundary() {
    check("tests/run/option_assign_match_boundary.tw");
}

#[test]
fn result_assign_boundary() {
    check("tests/run/result_assign_boundary.tw");
}

#[test]
fn result_typed_specialization() {
    check("tests/run/result_typed_specialization.tw");
}

#[test]
fn option_generic_boundary() {
    check("tests/run/option_generic_boundary.tw");
}

#[test]
fn option_record_field_boundary() {
    check("tests/run/option_record_field_boundary.tw");
}

#[test]
fn result_match_reassign_boundary() {
    check("tests/run/result_match_reassign_boundary.tw");
}

#[test]
fn option_branch_merge_boundary() {
    check("tests/run/option_branch_merge_boundary.tw");
}

#[test]
fn sum_cross_module_record() {
    check("tests/run/sum_cross_module_record/main.tw");
}

#[test]
fn sum_closure_capture_option_record() {
    check("tests/run/sum_closure_capture_option_record/main.tw");
}

#[test]
fn option_match_branch_reassign() {
    check("tests/run/option_match_branch_reassign.tw");
}

#[test]
fn sum_function_roundtrip() {
    check("tests/run/sum_function_roundtrip.tw");
}

#[test]
fn sum_record_field_roundtrip() {
    check("tests/run/sum_record_field_roundtrip.tw");
}

#[test]
fn sum_closure_return_boundary() {
    check("tests/run/sum_closure_return_boundary.tw");
}

#[test]
fn result_shorthand() {
    check("tests/run/result_shorthand.tw");
}

#[test]
fn result_try() {
    check("tests/run/result_try.tw");
}

#[test]
fn generic_user_funcs() {
    check("tests/run/generic_user_funcs.tw");
}

#[test]
fn closure_capture_cross_module() {
    check("tests/run/closure_capture_cross_module/main.tw");
}

#[test]
fn cell_update() {
    check("tests/run/cell_update.tw");
}

#[test]
fn fold_inferred_callback() {
    check("tests/run/fold_inferred_callback.tw");
}

// --- Trap tests ---

#[test]
fn trap_array_oob() {
    check_trap("tests/run/traps/array_oob.tw");
}

#[test]
fn trap_div_zero() {
    check_trap("tests/run/traps/div_zero.tw");
}

#[test]
fn trap_error_call() {
    check_trap("tests/run/traps/error_call.tw");
}

#[test]
fn defer_basic() {
    check("tests/run/defer_basic.tw");
}

#[test]
fn defer_return() {
    check("tests/run/defer_return.tw");
}

#[test]
fn defer_loop() {
    check("tests/run/defer_loop.tw");
}

#[test]
fn defer_capture() {
    check("tests/run/defer_capture.tw");
}

#[test]
fn defer_if() {
    check("tests/run/defer_if.tw");
}

#[test]
fn string_ordering() {
    check("tests/run/string_ordering.tw");
}

#[test]
fn char_code_at() {
    check("tests/run/char_code_at.tw");
}

#[test]
fn numeric_parsing() {
    check("tests/run/numeric_parsing.tw");
}

#[test]
fn type_to_string_ref() {
    check("tests/run/type_to_string_ref.tw");
}

#[test]
fn and_short_circuit() {
    check("tests/run/and_short_circuit.tw");
}

#[test]
fn string_iteration_index() {
    check("tests/run/string_iteration_index.tw");
}

#[test]
fn string_get() {
    check("tests/run/string_get.tw");
}

#[test]
fn string_large_index_semantics() {
    check("tests/run/string_large_index_semantics.tw");
}

#[test]
fn trap_string_index_oob() {
    check_trap("tests/run/traps/string_index_oob.tw");
}

#[test]
fn trap_string_large_index_traps() {
    check_trap("tests/run/traps/string_large_index_traps.tw");
}

#[test]
fn trap_string_slice_large_index() {
    check_trap("tests/run/traps/string_slice_large_index.tw");
}

#[test]
fn byte_type() {
    check("tests/run/byte_type.tw");
}

#[test]
fn byte_arithmetic_promotion() {
    check("tests/run/byte_arithmetic_promotion.tw");
}

#[test]
fn string_chars() {
    check("tests/run/string_chars.tw");
}

#[test]
fn string_code_point() {
    check("tests/run/string_code_point.tw");
}

#[test]
fn string_from_code_point_large_int() {
    check("tests/run/string_from_code_point_large_int.tw");
}

#[test]
fn string_utf8() {
    check("tests/run/string_utf8.tw");
}

#[test]
fn hex_literals() {
    check("tests/run/hex_literals.tw");
}

#[test]
fn bitwise_ops() {
    check("tests/run/bitwise_ops.tw");
}
