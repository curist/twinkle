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
parameters. `json.succeed` is unaffected because `A` appears in its parameter:

```twinkle
pub fn succeed<A>(value: A) Decoder<A> { ... }
// A appears in the parameter → subst is solved → no bug
```

## Affected Files

- `src/ir/monomorphize.rs` — stage0 pass (two locations)
- `boot/compiler/monomorphize.tw` — boot mirror (two locations, same bug)

## Root Cause

### Stage0: `collect_instantiations` (line 187)

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
and hits the `debug_assert!` panic at line 478.

The `CoreExpr` for the Call node already carries the typechecker-resolved type
(e.g. `Decoder<Int>` when context demands it). That information is available
but never consulted.

### Stage0: `rewrite_calls_in_kind` (line 461)

This function independently reconstructs `subst` from param/arg types at
rewrite time (lines 470-473). It has the same gap: if `subst` is missing type
params, `type_args` is filled with `MonoType::Void` via `unwrap_or` (line
476), and the `debug_assert!` at line 478 fires. Even after `collect_instantiations`
is fixed and the spec is seeded, this function still panics independently if it
cannot solve all type params from arguments alone.

Both locations must be fixed — the fix to `collect_instantiations` alone is not
sufficient.

### Boot mirror: same bug in two places

`boot/compiler/monomorphize.tw` has the identical structure in both functions:

- `collect_instantiations` at line 485: `if subst_covers(type_params, subst)` —
  when `subst` is empty (no param carries the type variable), `subst_covers`
  returns false and nothing is queued.
- `rewrite_calls_kind` at line 715: `if !subst_covers(type_params, subst)` —
  same guard causes the rewrite to silently skip the specialization lookup and
  fall through to an un-rewritten call.

Both need the return-type fallback.

## Fix

### Precondition

The fix assumes `expr.ty` on the Call node is fully resolved (no lingering
`MetaVar` or `Var`) by the time monomorphization runs. This holds for all
well-typed programs because monomorphization runs after type checking and
lowering. If `expr.ty` were still a type variable, `match_type_against` would
"solve" `A` to that variable, producing a broken specialization caught later by
the existing `debug_assert!` in the queue-processing loop (line 714).
The assumption should be documented or asserted where the fallback is added.

### Stage0: `collect_instantiations` — Call arm

After matching param types, if any type parameters are still unsolved, match
the generic function's declared return type against the call expression's
resolved `.ty`:

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
            if type_params.iter().any(|p| !subst.contains_key(p)) {
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

`expr` is the parent `CoreExpr` for the Call node. `match_type_against`
already handles recursive structural matching — no new machinery needed.

### Stage0: `rewrite_calls_in_kind` — Call arm (line 461)

Apply the same fallback at rewrite time. After the param/arg matching loop
(lines 470-473), before constructing `type_args`:

```rust
// After existing param-type matching loop:
let unsolved: Vec<_> = type_params.iter()
    .filter(|p| !subst.contains_key(*p))
    .collect();
if !unsolved.is_empty() {
    match_type_against(&gf.return_ty, &parent.ty, &mut subst);
}
// Then proceed to build type_args as before.
```

`parent.ty` is the `.ty` of the `CoreExpr` passed into `rewrite_calls_in_kind`.

### Boot mirror: `boot/compiler/monomorphize.tw`

Apply the analogous fix to both:

- `collect_instantiations` around line 485: after the param-matching loop,
  if `!subst_covers(type_params, subst)`, call `match_type_against` on
  `gf.return_ty` vs `expr.ty` before testing coverage again.
- `rewrite_calls_kind` around line 715: same pattern before the
  `subst_covers` guard that currently causes silent fallthrough.

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

2. **Mixed params regression** — a function with `A` in both a parameter and
   the return (`fn wrap<A>(val: A) Option<A>`) to confirm existing behavior is
   not regressed.

3. **Chained / nested** — `json.decode(.Null, json.fail("msg"))` pattern,
   where the outer `decode` call constrains the inner `fail` call's return
   type indirectly. This is the hardest case: the type of the `fail` call node
   must already be resolved on the `CoreExpr` from the typechecker for the
   fallback to work.

## Affected Tests

Once the fix lands:

- `boot/tests/suites/lib_json_suite.tw` should compile as part of
  `boot/tests/test_api.tw` without the workaround of excluding it.
- The "decode fail always errors" and "decode succeed always gives constant"
  tests in `lib_json_suite` exercise the affected call pattern directly.
- The `one_of` tests in `lib_json_suite` (which use `Vector<Decoder<String>>`)
  may also be affected and should be validated.

## Impact Scope

Changes are local to two functions in `src/ir/monomorphize.rs` and their
mirrors in `boot/compiler/monomorphize.tw`. No changes to the type system,
parser, or lowering are required. The typechecker already resolves the correct
type and stores it on the `CoreExpr` node; both monomorphization passes just
need to read it.
