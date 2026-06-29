# View (generic stdlib type) method-not-found crash — RESOLVED

## Symptom

Binding the result of an **absent** method on a `View` value crashed the backend
verifier with no source location:

```tw
use @std.view
v := view.from([1])
x := v.reverse()   // AMakeClosure result must have ClosureRef repr, got OpaqueAnyref
```

The same shape also crashed for a *present* method referenced as a value
(`f := v.len`). It looked View-specific because `View<C>` carries a bare
type-variable field (`source: C`), so its unresolved method-value lowers to an
opaque-anyref closure the verifier rejects.

## Actual root cause (the original plan was wrong)

The original plan blamed `checker.tw`'s `method_owner_name` returning `.None`
for `View` (leaking absent-method *calls* to lowering). That is **not** what
happened — `method_owner_name` returns `.Some("View")` and the checker *does*
emit a clean `.NoMethod` diagnostic for the call. The bug was that the
diagnostic was being **discarded** before it could gate compilation:

`check()` runs in passes. Pass 1 checks top-level lets (diagnostics suppressed),
Pass 2 checks function bodies, Pass 3 re-checks unannotated top-level lets with
full type info (this is where the real `.NoMethod` is emitted), and Pass 4
re-checks function bodies *if any top-level let's type changed between Pass 1 and
Pass 3* (`any_type_updated`). Pass 4 reset `cur_diags` back to its pre-Pass-2
length to drop stale function diagnostics — but that slice also dropped the
**Pass-3 top-level-let** diagnostics, and Pass 4 only re-checks functions, so
those let errors were lost forever. Lowering then ran on a program that should
have been rejected, and the unresolved `View` method crashed the backend.

`any_type_updated` flips whenever a top-level let calls an unannotated function
(its return type is a fresh metavar in Pass 1, resolved by Pass 3) or, as here,
binds a generic stdlib value whose element type is pinned only after Pass 1. The
record/`Vector` cases reported cleanly only because they happened *not* to flip
that flag.

## Fix

`boot/compiler/checker.tw` — `check()` now keeps diagnostics in three
independent streams (top-level statements, function bodies, re-checked
unannotated lets) and concatenates them once at the end. Re-running a pass
(Pass 4 recomputing stale function diagnostics) replaces only that pass's stream,
so the offset-arithmetic that truncated a shared list — and silently dropped the
Pass-3 let errors — is gone entirely. The duplicated function-checking loop
(Pass 2 / Pass 4) was also extracted into `check_module_functions`.

This is the correct-layer fix: the checker already produced the right
`type View<...> has no method 'reverse'` error, so simply not discarding it makes
every leaked-to-lowering crash impossible for this class. No lowering-recovery
hardening was needed.

## Regression test

`boot/tests/suites/query_diagnostics_suite.tw` — "absent-method error survives a
top-level let type correction" feeds a stdlib-free repro (`a := make()` calling
an unannotated `make`, then `b := a.bogus()`) through `analyze_document` and
asserts the `no method` diagnostic is present. Fails before the fix (surfaces a
stray `ambiguous type` instead), passes after. Self-host fixed point reached;
full boot suite green.
