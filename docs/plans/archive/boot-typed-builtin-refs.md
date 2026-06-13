# Boot Typed Builtin References

## Status (implemented)

Done: `boot/compiler/builtin_refs.tw` resolves `Option`/`Result` variants by name
from the `ResolvedEnv`; `lower_try` and the `for`-in iterator loop
(`lower_core/iteration.tw`) now use those refs instead of `TypeId.{ id: 0/1 }`
and literal variant indices. Self-host reaches a fixed point and the boot suite
passes, including a new test that locks the resolved ordering by name.

Design deviation from the original sketch below: refs are resolved **at the
feature site** from `ctx.env` (e.g. `builtin_refs.option_some(ctx.env)`), not
eagerly into a `LowerCtx.builtin_variants` field. Eager resolution in `new_ctx`
forced *every* env — including the deliberately minimal synthetic envs used by
lowering unit tests (`resolver.empty_env()`), which production never uses — to
carry both `Option` and `Result` even when the lowered program uses neither, and
trapped otherwise. Site-local resolution scopes the requirement to the feature
that actually needs the builtin (a `for`-in needs only `Option`; `try` on
`Result` needs only `Result`), matching the existing test convention where a
lowering test registers the builtins its path exercises. Source-level names stay
confined to `builtin_refs.tw`; a missing builtin still traps loudly as a
compiler bug.

Step 4 is also done: `lower_core/types.tw`'s `variant_index` and
`type_id_from_mono` no longer hardcode Option/Result layout. `variant_index`
resolves builtin variants via `builtin_refs.option_variant_index` /
`result_variant_index` (returning `-1` for unknown, matching its sentinel
contract); `type_id_from_mono` now takes the env and resolves Optional/Result
type ids via `builtin_refs.option_type_id` / `result_type_id` (the new `env`
argument was threaded through its ten lowering call sites). No further test
migration was needed — only the two `try` lowering tests above exercise
Option/Result through these helpers under a bare `empty_env`, and they already
use the builtin-env helper.

All five remaining `TypeId.{ id: N }` literals in compiler code construct the
builtin *types* `Order` (7), `Iterator` (4), and `Range` (3) in `checker.tw`,
`operators.tw`, and `iteration.tw`. These are the "Follow-Up Opportunities" set
named below (`Order`, `Iterator`, …), not Option/Result variant-layout
assumptions, and are left for the separate typed-builtin-type-refs pass.

## Goal

Remove implicit builtin layout assumptions from the boot compiler by resolving
well-known builtin types and variants into typed compiler references once, then
threading those references through the compiler contexts that need them.

The immediate target is `try` lowering, which currently assumes that builtin
`TypeId(0)` is `Option` and builtin `TypeId(1)` is `Result`. The end state should
make this relationship explicit without spreading string lookups throughout the
lowering code.

## Motivation

Twinkle advertises a simple, explicit programming model: direct code over typed,
immutable values. The compiler should follow the same style where practical.

Hardcoded builtin ids are brittle because they depend on construction order in
`base_env.tw`. Reordering builtin types, adding a new builtin before `Option`, or
changing variant order can silently miscompile features that lower through those
builtins.

Repeated string lookups are also brittle if they leak into feature code. Names
such as `"Option"`, `"Some"`, and `"Err"` should exist at the bootstrap boundary,
not throughout lowering and codegen.

## Current State

`boot/compiler/lower_core/control_flow.tw` lowers `try` by constructing Core
matches over hardcoded type and variant ids:

```tw
option_tid := TypeId.{ id: 0 }
result_tid := TypeId.{ id: 1 }
```

The lowering then assumes the builtin variant ordering:

- `Option.None` is variant `0`
- `Option.Some` is variant `1`
- `Result.Ok` is variant `0`
- `Result.Err` is variant `1`

Those assumptions match the current `builtin_type_entries()` ordering, but the
connection is implicit.

## Design

Introduce typed builtin references as compiler data:

```tw
type VariantRef = .{ tid: TypeId, vid: Int }

type BuiltinVariantRefs = .{
  option_none: VariantRef,
  option_some: VariantRef,
  result_ok: VariantRef,
  result_err: VariantRef,
}
```

Resolve the record once from the already-built `ResolvedEnv`:

```tw
fn resolve_builtin_variants(env: ResolvedEnv) Result<BuiltinVariantRefs, String>
```

The resolver may use strings internally, but only as centralized bootstrap names.
After construction, consumers use fields:

```tw
refs := ctx.builtin_variants
ok_pattern := variant_pattern(refs.result_ok.tid, refs.result_ok.vid, [...])
err_value := variant_expr(refs.result_err.tid, refs.result_err.vid, [...], ret_ty, s)
```

This keeps the invariant explicit:

> `try` lowers through the builtin `Option.Some` / `Option.None` and
> `Result.Ok` / `Result.Err` variants.

It also keeps feature code non-stringly and independent of builtin construction
order.

## Placement

Prefer a focused module, for example:

```text
boot/compiler/builtin_refs.tw
```

This module should own:

- `VariantRef`
- `BuiltinVariantRefs`
- lookup helpers for builtin type definitions and variants
- `resolve_builtin_variant_refs(env)`

Then add a field to lowering context:

```tw
pub type LowerCtx = .{
  ...
  builtin_variants: BuiltinVariantRefs,
}
```

`lower_core.context.new_ctx` can build the refs from `check_result.env`. If
resolution fails, the compiler should report an internal/lowering error rather
than manufacturing fallback ids.

## Implementation Plan

1. Add `boot/compiler/builtin_refs.tw`.
   - Implement a small `VariantRef` record.
   - Implement `BuiltinVariantRefs`.
   - Implement helpers that find a named builtin type and named variant.
   - Return `Result<_, String>` with clear messages for missing or malformed
     builtin definitions.

2. Extend `LowerCtx`.
   - Add `builtin_variants`.
   - Initialize it in `new_ctx` from `check_result.env`.
   - Decide whether initialization failure should be represented as an emitted
     lowering diagnostic or an internal compile error path. Prefer failing early
     and loudly; missing builtin definitions are compiler bugs, not user errors.

3. Update `lower_try`.
   - Remove hardcoded `TypeId.{ id: 0 }` and `TypeId.{ id: 1 }`.
   - Use `ctx.builtin_variants.option_some`, `.option_none`, `.result_ok`, and
     `.result_err`.
   - Keep the existing `MonoType.Optional` / `MonoType.Result` type dispatch.

4. Search for other numeric builtin assumptions.
   - Replace direct `TypeId.{ id: ... }` assumptions when they refer to builtin
     language types.
   - Do not replace ordinary synthetic ids, counters, span file ids, or test
     fixture ids unless they encode builtin layout assumptions.

5. Add regression coverage.
   - Existing `try` tests should keep passing.
   - Add a focused compiler test if there is a suitable boot test location.
   - If practical, add a unit-style test for builtin reference resolution that
     checks the expected refs are found by name.

## Non-Goals

- Do not make `Option` and `Result` ordinary user-definable types. They remain
  compiler-known type constructors represented by `MonoType.Optional` and
  `MonoType.Result`.
- Do not remove the centralized bootstrap names. The compiler still needs a
  boundary where source-level builtin names connect to internal ids.
- Do not redesign builtin method registration or prelude signature loading.
  This plan only removes implicit builtin type/variant layout assumptions.

## Follow-Up Opportunities

The same pattern can later cover other well-known builtin definitions where code
currently relies on names or layout indirectly, such as `Order`, `Iterator`,
`IterItem`, `UnfoldStep`, or `Task`.

If those references grow, split the data by concern:

```tw
type BuiltinTypeRefs = .{ ... }
type BuiltinVariantRefs = .{ ... }
type BuiltinFieldRefs = .{ ... }
```

Keep the public API typed and field-based; keep string lookup confined to the
construction boundary.
