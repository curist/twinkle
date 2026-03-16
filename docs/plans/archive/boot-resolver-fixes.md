# Boot Resolver Fixes — COMPLETE

Follow-up fixes for `boot/compiler/resolver.tw` based on code review.
All items implemented and tested.

## F1 — Arity check for user-defined generic types ✓

**Problem:** `resolve_single_name` accepts any number of type args for user types
without checking against the declared type parameter count. `Box<Int, String>` is
silently accepted for `type Box<T> = .{ v: T }`.

**Fix:** `check_user_type_arity` looks up the entry's def via `def_type_param_count`,
compares against `args.len()`, and returns an error `TypeResolveResult` on mismatch.
Called from `resolve_single_name` before producing `Named(...)`.

**Tests:** Wrong arity, zero args for generic type, forward-ref arity check.

## F2 — Skip duplicate functions in Pass 2 ✓

**Problem:** Pass 1 detects duplicate function names and emits a diagnostic, but
Pass 2 adds both copies to `env.functions`.

**Fix:** In `resolve_references`, guard with `!has_function(cur, decl.name)` before
resolving a function decl. The first occurrence wins; the duplicate is skipped.

**Test:** After resolving `fn foo(...)\nfn foo(...)`, env has exactly one entry.

## F3 — Store span in TypeEntry for better diagnostics ✓

**Problem:** `detect_circular_aliases` emits errors with a zero-span because
`TypeEntry` has no span field.

**Fix:** Added `span: span.Span` to `TypeEntry`. Populated in Pass 1 from
`decl.span`, preserved in `set_type_def`. Used in Pass 3 for circular alias errors.

**Test:** Circular alias diagnostics have non-zero span.

## F4 — Collect all type-arg errors instead of early return ✓

**Problem:** `resolve_applied_type` and `resolve_fn_type` return on the first
failed type argument, so `Dict<Bad1, Bad2>` only reports one error.

**Fix:** Both functions now track `any_failed` and continue resolving all args.
`resolve_fn_type` also resolves the return type before checking `any_failed`,
so `fn(Bad1) Bad2` reports both errors.

**Tests:** Multiple type-arg errors, multiple fn param errors, fn return type errors.

## F5 — Additional test coverage ✓

- **Self-alias:** `type A = A` → circular error
- **3-node cycle:** `type A = B\ntype B = C\ntype C = A` → circular error
- **Type var shadows builtin:** `fn f<Int>(x: Int) Int { x }` → `Var(Int)` (confirmed)
- **Forward alias chain:** `type A = B\ntype B = Int` → resolves without error
- **Duplicate fn env count:** env has exactly one entry after duplicate

Also improved `mono_to_string` test helper: `Var` now renders as `Var(name)` for
disambiguation, and `Named` with multiple args renders all args (not just first).

## F6 — Topological sort for type resolution ✓

Originally deferred, implemented because the arity check (F1) requires type
definitions to be populated before dependents are resolved.

**Problem:** Pass 2 resolved types in source order. Forward references like
`type Foo = .{ x: Box<Int, String> }\ntype Box<T> = .{ v: T }` would skip the
arity check because `Box`'s def was still `.None` when `Foo` was processed.

**Fix:** `topo_sort_type_decls` performs a DFS-based topological sort on type
declarations before resolution. `collect_type_refs` walks each decl's `TypeExpr`
tree to find references to other declared types. `topo_visit` does post-order
DFS, producing dependency-first ordering. Cycles are handled gracefully (cyclic
nodes are emitted in source order after acyclic ones; Pass 3 detects the cycle).

**Tests:** Forward-ref arity check, forward-ref field resolution.
