# Builtin Function Return Closure Materialization Plan

## Repro

`repro_builtin_returned_from_function_then_called` in
`boot/tests/suites/codegen_integration_suite.tw`.

```tw
fn get_converter() fn(Int) String { Int.to_string }
f := get_converter()
println(f(42))
```

## Symptom

Returning a builtin function value from a function does not produce a valid
closure value for the universal closure ABI. The codegen path reaches a lookup
failure for the returned builtin function id instead of materializing the builtin
trampoline closure at the return boundary.

## Desired semantics

Returning a builtin function value should behave like storing that builtin in a
local or record field. The return expression should construct a closure using the
specialized builtin trampoline. The caller should receive a normal closure value
and invoke it through `call_ref $rt_types__ClosureFunc`.

## Likely root cause

Boundary insertion covers local initialization and record construction, but the
return boundary does not apply the same function-value-to-closure conversion for
builtin functions. As a result, a raw builtin `FuncId` crosses a boundary where a
closure representation is required.

## Proper fix

- Compare boundary insertion for local initialization, record construction, and
  return expressions.
- Add explicit return-position closure materialization when the function return
  type is a function type and the returned expression is a builtin function
  value.
- Ensure the materialized closure uses the same builtin trampoline mechanism as
  local and record-field cases.
- Preserve direct-call lowering for builtin calls; only value-return boundaries
  should allocate closures.

## Validation

- Add a focused boundary insertion or backend test for returning a builtin
  function value.
- Re-enable the integration repro and assert the emitted WAT contains a builtin
  trampoline closure allocation.
- Run `target/twk run boot/tests/main.tw`.

## Non-goals

- Do not export raw builtin function ids to make lookup succeed.
- Do not special-case `Int.to_string`; any builtin function value returned across
  a function boundary should use the same mechanism.
