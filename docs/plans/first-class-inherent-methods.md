# First Class Inherent Methods Plan

## Goal

Support extracting inherent methods as first-class values, so patterns like:

```tw
f := foo.hello
f()
```

work the same way direct dot-calls already do (`foo.hello()`).

## Motivation

Current repro (`/tmp/first.tw`):

```tw
type Foo = .{ callme: fn() Void }

fn hello(_: Foo) {
  println("hello foo")
}

foo := Foo.{ callme: fn() { println("callme") } }
foo.hello()

callme := foo.callme
callme()

// f := foo.hello
// f()
```

Today:

- `foo.hello()` works (method call desugaring already exists).
- `foo.callme` works (record field of function type).
- `foo.hello` fails in typecheck (`UnsupportedFeature: method value references`).

## Behavior Contract

1. `foo.method` resolves to a function value with receiver already bound.
2. If `method` has signature `fn(Foo, A, B) R`, then `foo.method` has type `fn(A, B) R`.
3. Receiver expression is evaluated exactly once at extraction time.
4. Field/method collision behavior is unchanged: if both exist with the same name, dot access is still an error.
5. Existing module-qualified function references (`Vector.len`, `Int.to_string`) keep current behavior.

## Non-Goals

- Trait/typeclass dispatch.
- Changing dot resolution priority.
- Implicit partial application for general functions (this change is method-reference specific).

## Implementation Plan

### Phase 1: Typechecker Support

Files:

- `src/types/check.rs`

Changes:

1. Replace the current `method value references` rejection in `synth_field_access`.
2. Add a helper for method value synthesis:
   - resolve method function via `TypeEnv`/`ValueEnv` (builtin + user-defined paths),
   - instantiate generic vars,
   - unify receiver param with `base_ty`,
   - return function type made from remaining params and return type.
3. Keep field precedence and collision checks exactly as-is.
4. Match existing module-function-ref behavior for polymorphic ambiguity:
   - allow monomorphic method values in synth mode,
   - require expected function type annotation when inference cannot decide remaining type vars.

### Phase 2: Lowering to Closure Values

Files:

- `src/ir/lower.rs`

Changes:

1. Extend `ExprKind::FieldAccess` lowering:
   - if record field exists, keep existing `RecordGet`,
   - else if inherent method exists, lower to a closure that captures the receiver.
2. Generate a hoisted wrapper function for each method-value expression:
   - params = method params excluding receiver,
   - body = call resolved method function with `[captured_receiver, ...params]`.
3. Emit `MakeClosure` capturing the evaluated receiver local.
4. Reuse existing closure pipeline (Core IR, ANF, interpreter, wasm backend) without new IR forms.

### Phase 3: Tests

Add/extend tests:

- `tests/run/`:
  - new fixture for `f := foo.hello; f()`,
  - receiver-evaluated-once case (`f := make().hello`).
- `tests/typecheck/pass/`:
  - method value type inference for monomorphic methods,
  - annotated polymorphic method value.
- `tests/typecheck/fail/`:
  - ambiguous polymorphic method value without annotation,
  - field/method collision still errors.
- `tests/modules_test.rs` + module fixtures:
  - cross-module inherent method reference extraction.

### Phase 4: Spec + Docs

Files:

- `docs/spec.md`

Changes:

1. Add explicit rule that `receiver.method` may denote a first-class method value (bound receiver), not only a call callee.
2. Add examples for:
   - `f := foo.hello`,
   - function type after binding.
3. Keep existing collision and no-traits rules unchanged.

## Risks

- Polymorphic method values can introduce ambiguous bindings if not annotation-gated.
- Lowering must avoid re-evaluating receiver expressions when building closures.
- Wrapper-function generation increases hoisted function count; verify no regressions in optimizer/codegen assumptions.

## Success Criteria

1. `/tmp/first.tw` pattern (`f := foo.hello; f()`) typechecks and runs.
2. Existing direct method-call behavior and capability-record field calls remain unchanged.
3. Interpreter and wasm backend both pass new first-class inherent method tests.
