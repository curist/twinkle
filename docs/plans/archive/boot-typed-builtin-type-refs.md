# Boot Typed Builtin Type References

> **Status: Completed.** The five `Order`/`Iterator`/`Range` id literals were
> replaced with trapping `builtin_refs` accessors (`order_type` / `iterator_type`
> / `range_type`), env threaded into `contract_return_type` and the
> `lower_ord_cmp` / IntoIterator lowering sites; `synth_range_op` uses `ctx.env`.
> `IterItem` / `UnfoldStep` / `Task` were audited — already name-based, no
> accessors added. Two unit tests (`checker_suite`'s IntoIterator-bound test,
> `checker_coverage_suite`'s range tests) migrated from `empty_env` to the
> builtin env. Self-host reaches a fixed point with ids unchanged (`Order`=7,
> `Iterator`=4, `Range`=3); all boot tests pass.

## Goal

Extend the typed-builtin-reference pattern established in
[archive/boot-typed-builtin-refs.md](archive/boot-typed-builtin-refs.md) — which
removed hardcoded `Option`/`Result` variant-layout assumptions from `try` and
`for`-in lowering — to the remaining builtin *types* that compiler code still
references by raw `TypeId.{ id: N }`: `Order`, `Iterator`, and `Range`. Audit
`IterItem`, `UnfoldStep`, and `Task` for the same class of assumption.

The end state: no compiler code outside `boot/compiler/builtin_refs.tw` (and the
`base_env.tw` construction boundary) depends on the numeric id or construction
order of a builtin type.

## Motivation

Same as the predecessor plan. Hardcoded builtin ids are brittle: they silently
encode the construction order in `builtin_type_entries()`. Reordering the
builtin type list, or inserting a new builtin before `Range`/`Iterator`/`Order`,
would miscompile contract return types, range loops, iterator loops, and unit
`Order` comparisons with no error at the point of breakage.

The variant work already proved the approach is behavior-preserving (the
resolved ids equal the current base_env ids, so the self-host stays at a
byte-identical fixed point) and that names can be confined to `builtin_refs.tw`.

## Current State

After the variant-ref work, exactly five `TypeId.{ id: N }` literals for builtin
*types* remain in compiler (non-test) code:

| Site | Literal | Builtin |
|------|---------|---------|
| `checker.tw` `contract_return_type` (`.Order` arm) | `TypeId.{ id: 7 }` | `Order` |
| `checker.tw` `contract_return_type` (`.IteratorElem` arm) | `TypeId.{ id: 4 }` | `Iterator` |
| `checker.tw` `synth_range_op` | `TypeId.{ id: 3 }` | `Range` |
| `lower_core/operators.tw` `order_type_id()` | `TypeId.{ id: 7 }` | `Order` |
| `lower_core/iteration.tw` (IntoIterator `iter()` wrap) | `TypeId.{ id: 4 }` | `Iterator` |

Already name-based and **not** in scope as fixes (audit only):

- `IterItem` — resolved via `resolve_type_id(ctx.env, "IterItem")`
  (`lower_core/iteration.tw`); `resolver.tw` treats it as a known builtin name.
- `UnfoldStep` — referenced by name in `resolver.tw`; no id literal.
- `Task` — driven by `builtins.method_id("Task", …)` and signature group names;
  no type-id literal.

Unlike `Option`/`Result`, these arms span the **checker** as well as lowering,
so the env must be threaded to two checker helpers that are currently pure.

## Design

Extend `builtin_refs.tw` with **trapping** type-id accessors:

```tw
fn require_type_id(env: ResolvedEnv, type_name: String) TypeId {
  case type_id_of(env, type_name) {
    .Some(tid) => tid,
    .None => error("internal error: builtin_refs: missing builtin type ${type_name}"),
  }
}

pub fn order_type_id(env: ResolvedEnv) TypeId    { require_type_id(env, "Order") }
pub fn iterator_type_id(env: ResolvedEnv) TypeId { require_type_id(env, "Iterator") }
pub fn range_type_id(env: ResolvedEnv) TypeId    { require_type_id(env, "Range") }
```

These **trap** on a missing builtin, unlike the existing `option_type_id` /
`result_type_id` which return `TypeId?`. The distinction is intentional and
mirrors the variant accessors' two flavors (see the comment already in
`builtin_refs.tw`):

- `option_type_id` / `result_type_id` feed `types.type_id_from_mono`, whose
  `TypeId?` contract lets a caller bail; they must stay optional.
- The `Order`/`Iterator`/`Range` sites **unconditionally build a
  `MonoType.Named(tid, args)`** and cannot proceed without a definite id, so a
  loud trap is the correct failure.

Optionally add `MonoType`-building convenience wrappers where call sites repeat
the `Named` construction (`operators.tw` already has a local `order_type()`):

```tw
pub fn iterator_type(env: ResolvedEnv, elem_ty: MonoType) MonoType {
  MonoType.Named(iterator_type_id(env), [elem_ty])
}
```

Keep the named-accessor style; **do not** introduce the `BuiltinTypeRefs` /
`BuiltinFieldRefs` record split floated in the predecessor plan's Follow-Up
section. The shipped variant accessors are flat functions and the type set here
is small; a record adds threading without payoff. Revisit only if the accessor
count grows substantially.

## Implementation Plan

1. **Extend `builtin_refs.tw`.**
   - Add `require_type_id` and `order_type_id` / `iterator_type_id` /
     `range_type_id`.
   - Optionally add `iterator_type(env, elem)` / `order_type(env)` /
     `range_type(env)` wrappers if they de-duplicate call sites.

2. **Thread env into the checker helpers.**
   - `contract_return_type(shape, recv_ty, elem_ty)` → add an `env` parameter;
     replace the `.Order` and `.IteratorElem` arms with `builtin_refs` calls.
     Update its caller(s) (they run inside an `InferCtx` with `env`).
   - `synth_range_op` already has `ctx`; use `ctx.env`.

3. **Update lowering sites.**
   - `operators.tw`: fold `order_type_id()` / `order_type()` onto
     `builtin_refs.order_type_id(env)` / `order_type(env)`; thread `env` from the
     callers (the surrounding functions have `ctx.env` or an `env` param, as the
     variant work did).
   - `iteration.tw`: replace the IntoIterator `iter()`-wrap `Named(TypeId{4}, …)`
     with `builtin_refs.iterator_type(ctx.env, elem_ty)`.

4. **Audit `IterItem` / `UnfoldStep` / `Task`.**
   - Confirm no raw id literals remain (grep `TypeId.{ id:` in `boot/compiler/`
     should return zero builtin-type hits after step 3).
   - These are already name-based; only add accessors if it removes an
     ad-hoc `resolve_type_id(env, "…")` for symmetry. Not required.

5. **Migrate affected unit tests.**
   - The risk area, identical in shape to the predecessor's two `try` tests:
     checker/lowering tests built on `resolver.empty_env()` that construct or
     expect `Order` / `Iterator` / `Range` types will now need those builtins in
     the env. Candidates: contract-return-type tests, `synth_range_op` / range
     tests, iterator-loop lowering tests, unit-`Order`-comparison tests.
   - Migrate each to the production-representative helper (`builtin_env()` /
     `builtin_type_env()`), the same fix used for the `try` tests. Because the
     resolved ids equal the base_env ids (`Order`=7, `Iterator`=4, `Range`=3),
     expected lowered output is unchanged; only env construction shifts.

6. **Verify.**
   - `make boot-test`: self-host must reach a fixed point (byte-identical output
     expected, since ids are unchanged) and the full suite must pass.
   - `make fmt` idempotent on all touched files.

## Non-Goals

- Do not make `Order`, `Iterator`, `Range`, `IterItem`, `UnfoldStep`, or `Task`
  user-definable. They remain compiler-known builtins.
- Do not revisit the `Option`/`Result` variant refs — that work is complete and
  archived.
- Do not introduce a record-based `BuiltinTypeRefs` API (see Design).
- Do not change builtin method registration, prelude signature loading, or the
  `base_env.tw` construction order. This plan only removes downstream id
  literals.
