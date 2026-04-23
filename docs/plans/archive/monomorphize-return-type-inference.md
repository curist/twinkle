# Monomorphizer: Infer Type Args from Return Type

> Status: Completed and archived.
>
> Implemented in:
>
> - `src/ir/monomorphize.rs`
> - `boot/compiler/monomorphize.tw`
> - `boot/tests/suites/core_ir_suite.tw`
>
> Landed behavior:
>
> - call-site monomorphization now falls back to the resolved call result type
>   when arguments alone do not solve all type parameters
> - Rust stage0 now also specializes and rewrites generic `MakeClosure` targets
> - the boot compiler now collects monomorphization type parameters from the
>   function signature only, not body expression annotations
> - stage0 unit tests, boot compiler regression tests, and
>   `boot/tests/test_api.tw` all cover the fixed behavior

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
`Decoder<A>`. The typechecker correctly infers `A = Int` because the
`json.fail("oops")` call expression is resolved to `Decoder<Int>` from its
surrounding context, but the monomorphizer never seeds `A` from the return
type and therefore panics.

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
            if type_params.iter().all(|p| subst.contains_key(p)) {
                queue.push_back((*fid, subst));
            }
        }
    }
    ...
}
```

`expr` is the parent `CoreExpr` for the Call node. `match_type_against`
already handles recursive structural matching — no new machinery needed.
The Rust pass should use a full-coverage check here rather than `!subst.is_empty()`;
queue processing already assumes every collected type parameter is solved.

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
This is also a good place for a `debug_assert!` documenting the pass invariant
that the call node's type is fully resolved before monomorphization.

### Boot mirror: `boot/compiler/monomorphize.tw`

Two locations need the same fix.

**`collect_instantiations`, line 485** — current code:

```twinkle
// boot/compiler/monomorphize.tw:478-487
type_params := collect_type_params(gf)
subst: Dict<String, MonoType> = Dict.new()
i := 0
for i < gf.params.len() and i < args.len() {
  subst = match_type_against(gf.params[i].ty, args[i].ty, subst)
  i = i + 1
}
if subst_covers(type_params, subst) {
  queue.items = queue.items.append(InstItem.{ func_id: fid, subst })
}
```

After the param-matching loop, if coverage is still incomplete, fall back to
matching `gf.return_ty` against `expr.ty` before testing coverage:

```twinkle
if !subst_covers(type_params, subst) {
  subst = match_type_against(gf.return_ty, expr.ty, subst)
}
if subst_covers(type_params, subst) {
  queue.items = queue.items.append(InstItem.{ func_id: fid, subst })
}
```

**`rewrite_calls_kind`, line 715** — current code:

```twinkle
// boot/compiler/monomorphize.tw:708-717
type_params := collect_type_params(gf)
subst: Dict<String, MonoType> = Dict.new()
i := 0
for i < gf.params.len() and i < new_args.len() {
  subst = match_type_against(gf.params[i].ty, new_args[i].ty, subst)
  i = i + 1
}
if !subst_covers(type_params, subst) {
  return .Call(rewrite_calls(callee, ...), new_args)  // ← silent miss
}
```

Apply the same fallback before the early return:

```twinkle
if !subst_covers(type_params, subst) {
  subst = match_type_against(gf.return_ty, parent.ty, subst)
}
if !subst_covers(type_params, subst) {
  return .Call(rewrite_calls(callee, ...), new_args)
}
```

`parent` is the `CoreExpr` argument passed to `rewrite_calls_kind`; `parent.ty`
is the typechecker-resolved return type of the call, same as `expr.ty` above.

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
a `#[cfg(test)]` block with module-building helpers). The tests should assert
not just that monomorphization completes, but that the rewritten call site
actually targets the specialized `FuncId`:

1. **Return-only type param, concrete context** — a generic function
   `fn const_decoder<A>(x: String) A` (type var only in return) called where
   the surrounding `Let` binding has a concrete type. Verify a specialization
   is created for the expected concrete type arguments and that the rewritten
   callee uses that specialization.

2. **Mixed params regression** — a function with `A` in both a parameter and
   the return (`fn wrap<A>(val: A) Option<A>`) to confirm existing behavior is
   not regressed.

3. **Mixed solved-by-arg and solved-by-return** — a function where one type
   parameter is solved from an argument and another appears only in the return.
   This verifies that the return-type fallback complements argument matching
   rather than replacing it.

4. **Chained / nested** — `json.decode(.Null, json.fail("msg"))` pattern,
   where the outer `decode` call constrains the inner `fail` call's return
   type indirectly. This is the most important case: the type of the `fail`
   call node must already be resolved on the `CoreExpr` from the typechecker
   for the fallback to work.

## Affected Tests

Once the fix lands:

- `boot/tests/suites/lib_json_suite.tw` should compile as part of
  `boot/tests/test_api.tw` without the workaround of excluding it.
- The "decode fail always errors" and "decode succeed always gives constant"
  tests in `lib_json_suite` exercise the affected call pattern directly.
- The `one_of` tests in `lib_json_suite` (which use `Vector<Decoder<String>>`)
  may also be affected and should be validated.

## Implementation Notes

The final implementation followed the planned call-site fallback, but also
closed two adjacent issues discovered during validation:

- Rust stage0 needed matching specialization/rewrite handling for
  `CoreExprKind::MakeClosure`, otherwise generic hoisted lambdas could survive
  monomorphization and later produce dangling closure trampoline references in
  codegen.
- The boot compiler's `collect_type_params` was narrowed to the function
  signature. Scanning body expression annotations could introduce non-signature
  type variables into `subst_covers`, causing solved call sites to be skipped.

Both implementations now keep their call-site inference logic structurally
aligned through shared helpers.

## Impact Scope

The landed fix touched `src/ir/monomorphize.rs` and `boot/compiler/monomorphize.tw`,
with accompanying regression coverage in Rust unit tests, boot compiler
`core_ir_suite`, and end-to-end API tests. No changes to the type system,
parser, or lowering were required.
