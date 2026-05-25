# Cell.update Method-Value Specialization

## Status

Resolved. The former repro is now active as
`test_cell_update_builtin_first_class_function_arg_uses_wrapper_trampoline` in
`boot/tests/suites/codegen_integration_suite.tw`.

## Fix

Boundary insertion now uses the expected function type when a materialized global
function value still has generic `Var` placeholders. That gives `Cell.update` the
concrete `fn(Cell<Int>, fn(Int) Int) Void` mono when it is passed as a
first-class argument.

Wasm planning also records the concrete mono from the prepared closure slot and
uses it for closure-materialized builtins, so wrapper trampoline emission sees
the specialized signature instead of the generic builtin definition.

## Validation

`make bundle-cli` and `make boot-test` pass with the repro enabled.
