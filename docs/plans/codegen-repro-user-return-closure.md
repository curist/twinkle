# User Function Return Closure Materialization Plan

## Repro

`repro_user_function_returned_from_function_then_called` in
`boot/tests/suites/codegen_integration_suite.tw`.

```tw
fn double(n: Int) Int { n * 2 }
fn get_double() fn(Int) Int { double }
f := get_double()
println(Int.to_string(f(21)))
```

## Symptom

The returned user function currently appears as a raw function reference in WAT
instead of a heap-allocated closure value. The call site then treats the result as
a closure, so the emitted representation is inconsistent with the universal
closure ABI.

## Desired semantics

Returning a user function value should produce the same closure representation as
storing that function value in a local or record field. The callee should return a
closure object, and the caller should invoke it through
`call_ref $rt_types__ClosureFunc`.

## Relationship to builtin returns

This is the user-function counterpart of
[builtin function return closure materialization](codegen-repro-builtin-return-closure.md).
Both should be fixed through the same return-boundary model, but user functions
also need to preserve closure metadata and capture handling for future captured
closures.

## Proper fix

- Extend return-boundary insertion so function-typed return values are converted
  to closure values before returning.
- Reuse the same `AMakeClosure` path already used for local and record-field
  function values.
- Ensure closure conversion records capture metadata for returned user closures.
- Keep zero-capture and captured user functions on the same path so later closure
  returns do not need a second representation rule.

## Validation

- Add focused coverage for returning a named user function and, if supported by
  the current lowering path, a captured closure.
- Re-enable the integration repro and assert the emitted WAT allocates
  `rt_types__Closure` and calls through the universal closure function type.
- Run `target/twk run boot/tests/main.tw`.

## Non-goals

- Do not rely on raw `ref.func` values as the source-level function value
  representation.
- Do not weaken call-site casts to accept both raw functions and closures.
