# BuiltinEntry Callable Metadata Refactor Plan

## Goal

Make the builtin registry the single source of truth for builtin function-value
codegen metadata. Today `callable_targets.tw` decides whether a builtin can be
materialized as a closure, and which dedicated trampoline wrapper it needs, by
comparing `FuncId`s against hardcoded builtin names. That works, but it makes
adding or changing a first-class builtin fragile because the registration site
and the callable-target policy can drift.

## Current state

`BuiltinEntry` currently carries identity, canonical name, runtime/intrinsic
kind, and ABI contract. First-class callable metadata is derived later in
`boot/compiler/backend/callable_targets.tw`:

- `builtin_wrapper_kind` maps specific `FuncId`s to dedicated closure trampoline
  wrapper kinds.
- `builtin_generic_closure_materializable` allow-lists builtins that can use the
  generic universal closure trampoline.
- `builtin_target` combines those helpers with semantic signature lookup to
  produce `CallableTargetInfo`.

This means a new first-class builtin may need changes in the builtin registry,
callable target policy, trampoline emission, direct-call intrinsic tables, and
tests. Some of those are unavoidable, but closure materialization policy should
live with the builtin entry.

## Desired design

Extend builtin registration metadata with a callable policy record, for example:

```tw
type BuiltinClosurePolicy = {
  NotMaterializable,
  GenericClosure,
  WrapperClosure(WrapperKind),
}
```

or, if avoiding a dependency from `builtins.tw` to backend wrapper enums is
preferable:

```tw
type BuiltinClosureWrapper = { ByteToString, CellUpdate, IteratorUnfold, HostWriteBytes }
type BuiltinClosurePolicy = {
  NotMaterializable,
  GenericClosure,
  WrapperClosure(BuiltinClosureWrapper),
}
```

Then add it to `BuiltinEntry`:

```tw
pub type BuiltinEntry = .{
  name: String,
  canonical_name: String?,
  func_id: FuncId,
  kind: BuiltinKind,
  abi: AbiContract,
  closure_policy: BuiltinClosurePolicy,
}
```

Registration helpers should default to `NotMaterializable`, with explicit helper
variants for materializable builtins:

```tw
runtime_closure(..., .GenericClosure)
intrinsic_closure(..., .WrapperClosure(.CellUpdate))
```

The exact helper names can be chosen to fit the existing `runtime`,
`runtime_with_abi`, `intrinsic`, and `intrinsic_with_abi` style.

## Implementation steps

### Add registry-level metadata

- Add closure policy types to `boot/compiler/builtins.tw`.
- Add `closure_policy` to `BuiltinEntry`.
- Update registration helpers and `with_canonical` to preserve the policy.
- Keep the default policy non-materializable so most builtins require no special
  annotation.

### Mark existing first-class builtins

Set explicit policies for the builtins currently recognized by
`callable_targets.tw`:

- Generic closure trampoline: numeric/string `to_string` builtins that already
  use the generic path.
- Dedicated wrapper trampoline: byte `to_string`, `Cell.update`,
  `Iterator.unfold`, and the host bytes writer if it remains closure-capable.

This should not change direct-call lowering. Direct calls should continue to use
runtime imports or intrinsic emitters exactly as they do today; the new policy is
only about materializing builtin values as closures.

### Simplify callable target resolution

- Replace `builtin_wrapper_kind` and `builtin_generic_closure_materializable` with
  conversion from `BuiltinEntry.closure_policy` to `CallableTargetInfo` fields.
- If `builtins.tw` defines a frontend-neutral wrapper enum, map it to
  `callable_targets.WrapperKind` in one small conversion function.
- Derive `closure_materializable` and builtin `typed_closure_support` from the
  same policy check: anything other than `NotMaterializable` supports the
  current builtin closure path.
- Keep semantic signature lookup in `callable_targets.tw`; the registry should
  not duplicate type signatures already stored in the resolver environment.

### Preserve emission behavior

- `emit/closures.tw` should continue consuming `CallableTargetInfo.wrapper_kind`.
- Dedicated trampoline bodies remain in the emitter because they are codegen
  details, not registry details.
- Direct-call intrinsic tables remain separate unless a later refactor also moves
  intrinsic dispatch metadata into `BuiltinEntry`.

## Validation

- Boot tests should continue to cover builtin function values stored in locals,
  record fields, call arguments, and return positions.
- Add a focused regression around callable-target resolution proving that the
  materialization decision comes from `BuiltinEntry` policy, not a hardcoded
  `FuncId` allow-list.
- Run the formatter on edited Twinkle files.
- Run the boot test suite. If the refactor touches Rust stage0 bootstrap data or
  shared builtins, run the Rust suite as well.

## Known follow-up

`callable_targets.tw` still resolves builtin semantic signatures through a
fallback chain (`entry.name`, then `canonical_name`, then `__${entry.name}`).
That lookup is a separate source of builtin registration fragility. This refactor
should leave it in place to keep the metadata move behavior-preserving, but a
follow-up should make semantic signature identity explicit enough that adding a
builtin does not rely on fallback naming conventions.

## Non-goals

- Do not change the universal closure ABI.
- Do not move trampoline emission into the builtin registry.
- Do not duplicate semantic type signatures in `BuiltinEntry`.
- Do not expand the set of closure-materializable builtins as part of the
  metadata move; preserve current behavior first.
