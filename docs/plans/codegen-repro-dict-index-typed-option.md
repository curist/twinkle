# Dict Index Typed Option Boundary Plan

## Repro

`repro_dict_index_materializes_typed_option` in
`boot/tests/suites/codegen_integration_suite.tw`.

```tw
d: Dict<String, Int> = Dict.new()
d["x"] = 10
println(Int.to_string(d["x"].unwrap_or(0)))
```

## Symptom

The source now links through the prelude `Option.unwrap_or` path, but the user
module still routes dict indexing through the erased `rt_dict__get_option` ABI
when the value flows into a prelude generic call. The repro expects the dict
boundary to call `rt_dict__get`, then materialize the concrete
`Option<Int>` struct in user code.

## Desired semantics

Dict indexing has source-level type `Option<V>`. At a typed boundary, codegen
should construct the concrete option representation for the known `V`, regardless
of whether the value is immediately consumed by user code or by a prelude generic
function.

The erased `rt_dict__get_option` helper is still useful at truly erased
boundaries, but it should not be selected merely because the consumer is a
prelude generic call.

## Likely root cause

Boundary insertion and/or prepared slot assignment loses the typed expected value
for the dict-index expression before Wasm emission. `emit_index_op` has both
paths available:

- typed path: `rt_dict__get` plus typed `Option<V>` construction
- erased path: `rt_dict__get_option`

The repro shows the erased path is selected in a context that still has a typed
source-level result.

## Proper fix

- Trace the expression from type checking through Core IR, monomorphization, ANF,
  boundary insertion, and prepared slots.
- Identify where the expected `Option<Int>` result is replaced by an erased
  `anyref` destination for the prelude `Option.unwrap_or` call.
- Preserve typed egress at the dict-index boundary, then insert any required
  typed-to-erased coercion at the actual prelude function call boundary.
- Keep the ownership of representation conversions explicit: container egress
  should materialize source-level typed values; call-boundary adaptation should
  handle ABI erasure.

## Validation

- Add focused coverage near boundary insertion or prepared-slot assignment that
  proves a dict-index expression retains its typed optional result before call
  adaptation.
- Re-enable the codegen integration repro and assert that the user function body
  calls `rt_dict__get`, does not call `rt_dict__get_option`, and constructs the
  typed option struct.
- Run `target/twk run boot/tests/main.tw`.

## Non-goals

- Do not remove the erased runtime helper.
- Do not special-case `Option.unwrap_or`; the fix should apply to generic
  prelude call arguments generally.
