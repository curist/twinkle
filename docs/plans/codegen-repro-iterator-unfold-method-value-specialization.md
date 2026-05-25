# Iterator.unfold Method-Value Specialization Plan

## Repro

`repro_iterator_unfold_builtin_first_class_function_arg_uses_wrapper_trampoline`
in `boot/tests/suites/codegen_integration_suite.tw`.

```tw
fn step(i: Int) UnfoldStep<Int, Int> {
  if i >= 3 { UnfoldStep.Done } else { UnfoldStep.Yield(i, i + 1) }
}
fn apply_unfold(
  f: fn(Int, fn(Int) UnfoldStep<Int, Int>) Iterator<Int>,
  seed: Int,
  g: fn(Int) UnfoldStep<Int, Int>,
) Iterator<Int> {
  f(seed, g)
}
it := apply_unfold(Iterator.unfold, 0, step)
println(Int.to_string(it.take(2).to_vector().len()))
```

## Symptom

The backend verifier sees the generic `Iterator.unfold` method value where the
callee expects a fully instantiated function type using `Int` for both yielded
value and seed state.

## Desired semantics

`Iterator.unfold` should be usable as a first-class function when an expected
function type provides concrete type arguments. The produced closure/trampoline
must have the specialized function mono:

```tw
fn(Int, fn(Int) UnfoldStep<Int, Int>) Iterator<Int>
```

## Relationship to Cell.update

This is the same structural issue as
[Cell.update method-value specialization](codegen-repro-cell-update-method-value-specialization.md),
but with multiple type parameters and nested generic types in both arguments and
return type. It should be kept as an integration repro because it exercises the
more complex substitution path.

## Proper fix

- Reuse the generic builtin method-value specialization mechanism needed for
  `Cell.update`.
- Verify substitution handles nested function types and named generic sums such
  as `UnfoldStep<T, S>`.
- Ensure wrapper/trampoline metadata records the specialized signature, not the
  builtin registry's generic definition.
- Preserve the normal direct-call lowering for `Iterator.unfold`; the fix is for
  method values passed through first-class function slots.

## Validation

- Add focused coverage for expected-type-driven specialization with nested
  generics.
- Re-enable the codegen integration repro and assert it still emits the closure
  path and constructs iterator state.
- Run `target/twk run boot/tests/main.tw`.

## Non-goals

- Do not inline `Iterator.unfold` just to avoid the method-value path.
- Do not relax verifier mono equality for function arguments.
