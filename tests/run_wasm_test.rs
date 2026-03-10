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
fn run_wasm_strings() {
    check("tests/run/strings.tw");
}

#[test]
fn run_wasm_string_methods() {
    check("tests/run/string_methods.tw");
}

#[test]
fn run_wasm_closures() {
    check("tests/run/closures.tw");
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
