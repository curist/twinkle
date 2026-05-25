# Cell.update Method-Value Specialization Plan

## Repro

`repro_cell_update_builtin_first_class_function_arg_uses_wrapper_trampoline` in
`boot/tests/suites/codegen_integration_suite.tw`.

```tw
fn inc(x: Int) Int { x + 1 }
fn apply_update(f: fn(Cell<Int>, fn(Int) Int) Void, c: Cell<Int>, g: fn(Int) Int) Void {
  f(c, g)
}
c := Cell.new(1)
apply_update(Cell.update, c, inc)
println(Int.to_string(c.get()))
```

## Symptom

The backend verifier sees a generic `Cell.update` method value in a slot where
the callee expects the specialized type:

```tw
fn(Cell<Int>, fn(Int) Int) Void
```

The verifier is correct to reject this. A first-class builtin method value must
be instantiated before it is wrapped or passed as an argument.

## Desired semantics

Taking `Cell.update` as a value in a context with an expected function type should
instantiate the builtin method signature from that expected type. The wrapper or
trampoline should expose the specialized ABI for `Cell<Int>`, not the generic
signature from the builtin registry.

## Likely root cause

Direct builtin calls have enough receiver/argument context to specialize during
lowering and monomorphization. Method values are different: the receiver is not
present syntactically at the method-value expression, so specialization must be
driven by the expected function type.

The current method-value path appears to retain the builtin definition mono and
then attempts to use it in a specialized call slot.

## Proper fix

- Trace checker output for `Cell.update` when it appears as a value rather than a
  direct call.
- Ensure expected-type information is attached to method-value expressions or is
  recoverable before Core lowering.
- Instantiate the builtin function signature using the expected function type
  before creating the `GlobalFunc`, `MakeClosure`, wrapper, or trampoline.
- Make backend metadata use the instantiated mono for both the closure value and
  the wrapper target.
- Keep verifier checks strict; they are catching the correct invariant.

## Validation

- Add a focused lower/backend test that checks the method value has the
  specialized mono before Wasm emission.
- Re-enable the codegen integration repro and assert the closure path uses the
  universal closure ABI and updates the typed `Cell<Int>` storage.
- Run `target/twk run boot/tests/main.tw`.

## Non-goals

- Do not weaken direct-call argument verification.
- Do not add a special case only for `Cell.update`; the mechanism should apply to
  generic builtin method values with expected function types.
