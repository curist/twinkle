# Iterator.unfold Method-Value Specialization

## Status

Resolved. The former repro is now active as
`test_iterator_unfold_builtin_first_class_function_arg_uses_wrapper_trampoline`
in `boot/tests/suites/codegen_integration_suite.tw`.

## Fix

The generic method-value path now preserves the concrete expected function type
when a builtin signature contains `Var` placeholders. For `Iterator.unfold`, this
carries the nested specialized mono through closure materialization:

```tw
fn(Int, fn(Int) UnfoldStep<Int, Int>) Iterator<Int>
```

Wasm planning extracts the concrete mono from the prepared closure slot and uses
that signature for closure-materialized builtins, avoiding generic `Var` types in
wrapper trampoline emission.

## Validation

`make bundle-cli` and `make boot-test` pass with the repro enabled.
