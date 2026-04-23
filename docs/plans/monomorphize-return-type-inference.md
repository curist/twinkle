# Monomorphizer: Infer Type Args from Return Type

## Goal

Fix the stage0 monomorphizer so that generic functions whose type parameters
appear only in the return type — not in any parameter — are correctly
specialised at call sites.

The canonical example is `json.fail` from `boot/lib/json.tw`:

```twinkle
pub fn fail<A>(message: String) Decoder<A> {
  make(fn(_: Json) { .Err(message) })
}
```

`A` does not appear in any parameter; it only appears in the return type
`Decoder<A>`. The typechecker correctly infers `A = Int` when the result is
used in a context that expects `Result<Int, String>`, but the monomorphizer
never seeds `A` from the return type and therefore panics.

The same class of bug affects any generic function with return-only type
parameters, e.g. `json.succeed`:

```twinkle
pub fn succeed<A>(value: A) Decoder<A> { ... }
```

Wait — `succeed` has `A` in the parameter, so it works. The problem is
exclusively with functions like `fail` where no argument constrains any type
variable.

## Affected File

`src/ir/monomorphize.rs`

## Root Cause

`collect_instantiations` seeds the specialization queue only by matching each
generic parameter type against the corresponding argument type:

```rust
// src/ir/monomorphize.rs:187-196
CoreExprKind::Call { callee, args } => {
    if let CoreExprKind::GlobalFunc(fid) = &callee.kind {
        if let Some(gf) = generic_funcs.get(fid) {
            let mut subst = HashMap::new();
            for (param_ty, arg) in gf.param_tys.iter().zip(args.iter()) {
                match_type_against(param_ty, &arg.ty, &mut subst);
            }
            if !subst.is_empty() {            // ← bug: skips when subst empty
                queue.push_back((*fid, subst));
            }
        }
    }
    ...
}
```

For `fail("oops")`:

- param type: `String`
- arg type: `String`
- `match_type_against(String, String, subst)` adds nothing — no type variable
- `subst.is_empty()` → true → nothing queued

The call site is skipped entirely. Later, `rewrite_calls_in_kind` reaches the
same call and tries to solve `A` from arguments alone — still finds nothing —
and hits the `debug_assert!` panic at line 480.

The `CoreExpr` for the Call node already carries the typechecker-resolved type
(e.g. `Decoder<Int>` when context demands it). That information is available
but never consulted.

## Fix

After matching parameter types, if any type parameters are still unsolved,
also match the generic function's declared return type against the `CoreExpr`'s
own `.ty` (which the typechecker has already resolved from context).

In `collect_instantiations` for the `Call` arm:

```rust
CoreExprKind::Call { callee, args } => {
    if let CoreExprKind::GlobalFunc(fid) = &callee.kind {
        if let Some(gf) = generic_funcs.get(fid) {
            let type_params = collect_type_params(&gf.param_tys, &gf.return_ty);
            let mut subst = HashMap::new();
            // Existing: match param types against argument types.
            for (param_ty, arg) in gf.param_tys.iter().zip(args.iter()) {
                match_type_against(param_ty, &arg.ty, &mut subst);
            }
            // New: if any type params remain unsolved, match the generic
            // return type against the call expression's resolved type.
            let unsolved: Vec<_> = type_params
                .iter()
                .filter(|p| !subst.contains_key(*p))
                .collect();
            if !unsolved.is_empty() {
                match_type_against(&gf.return_ty, &expr.ty, &mut subst);
            }
            if !subst.is_empty() {
                queue.push_back((*fid, subst));
            }
        }
    }
    ...
}
```

`expr` here is the parent `CoreExpr` for the Call node, which holds `.ty` =
the typechecker-resolved return type of the call. `match_type_against` already
handles recursive structural matching, so this requires no new machinery.

The same extension should be applied to `rewrite_calls_in_kind` at line
469-486, which rebuilds the substitution at rewrite time. Currently it also
only matches param types against arg types; if a type param is unsolved after
that pass, it should likewise fall back to matching `gf.return_ty` against
`parent.ty`.

## Symptom Reproduction

```twinkle
// boot/tests/suites/lib_json_suite.tw
r: Result<Int, String> = json.decode(.Null, json.fail("oops"))
```

Building `boot/tests/test_api.tw` with stage0 panics:

```
thread 'main' panicked at src/ir/monomorphize.rs:478:21:
unsolved type params ["A"] at call site for FuncId(110)
```

`FuncId(110)` is `json.fail`. `A` is left unsolved because no argument
matches `A`, and the return type `Decoder<A>` is never consulted.

## Test Cases to Add

Alongside the fix, add unit tests in `monomorphize.rs` (the file already has
a `#[cfg(test)]` block with module-building helpers):

1. **Return-only type param, concrete context** — a generic function
   `fn const_decoder<A>(x: String) A` (type var only in return) called where
   the surrounding `Let` binding has a concrete type. Verify specialization
   succeeds and the correct `FuncId` is emitted.

2. **Mixed params** — a function with `A` in both a parameter and the return
   (`fn wrap<A>(val: A) Option<A>`) to confirm existing behavior is not
   regressed.

3. **Chained / nested** — `json.decode(.Null, json.fail("msg"))` pattern,
   where the outer `decode` call constrains the inner `fail` call's return
   type indirectly.

## Relation to boot Compiler

The boot compiler (`boot/compiler/`) mirrors the monomorphization pass in
`boot/compiler/monomorphize.tw`. Once the stage0 fix is validated, check
whether the same gap exists there and apply the analogous fix to the
self-hosted pass.

## Affected Tests

Once the fix lands:

- `boot/tests/suites/lib_json_suite.tw` should compile as part of
  `boot/tests/test_api.tw` without the workaround of excluding it.
- The "decode fail always errors" and "decode succeed always gives constant"
  tests in `lib_json_suite` exercise the affected call pattern directly.
- The `one_of` tests in `lib_json_suite` (which use `Vector<Decoder<String>>`)
  may also be affected and should be validated.

## Impact Scope

The fix is local to `collect_instantiations` and `rewrite_calls_in_kind` in
`src/ir/monomorphize.rs`. No changes to the type system, parser, or lowering
are required. The typechecker already resolves the correct type and stores it
on the `CoreExpr` node; the monomorphizer just needs to read it.
