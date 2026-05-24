# Builtin Identity Resolution

## Status

In progress.

Initial compatibility work is underway in the boot compiler: builtin method/type-qualified calls can prefer compiler-owned callable targets even when user functions reuse old internal names, and focused collision guardrails cover those names.

## Problem

Builtin, runtime, intrinsic, and host functions currently share the same string-keyed function namespace as user code in several compiler stages. Many compiler-owned implementation targets are named with ordinary identifiers such as `byte_to_int`, `int_to_string`, `vector_len`, and `dict_new`.

That makes implementation names observable enough to collide with user functions. A user-defined function named like an internal target can be resolved or emitted as the compiler-owned target, or the compiler-owned target can be resolved as the user function. The Byte migration exposed this with a user helper named `byte_to_int`, which compiled into a recursive self-call instead of calling the internal `Byte.to_int` intrinsic.

Using unrepresentable string names such as `intrinsic$byte_to_int` would reduce collision risk, but it would still leave builtin identity encoded in strings. The stronger fix is to make builtin identity a distinct concept throughout resolution, lowering, optimization, backend verification, and codegen.

## Goal

Represent compiler-owned callable targets by stable typed identity, not by user-visible function names.

User functions and compiler-owned builtins should be resolved in distinct namespaces. A source-level declaration named `byte_to_int`, `int_to_string`, `vector_len`, or similar must always be an ordinary user function and must never collide with the implementation target for `Byte.to_int`, `Int.to_string`, `Vector.len`, etc.

## Non-goals

- Do not change public Twinkle API names.
- Do not add syntax for calling internal builtins directly.
- Do not expose builtin implementation names in source-level name lookup.
- Do not rely on string prefixes as the final architecture, though reserved display names may remain useful for diagnostics and WAT symbol generation.

## Design

### Separate callable identity from source names

Introduce a typed callable identity for compiler-owned functions. Conceptually:

```tw
type BuiltinId = .{ id: Int }

type CallableRef = {
  UserFunction(FuncId),
  Builtin(BuiltinId),
}
```

The exact shape can be adapted to the existing boot compiler IR, but the key invariant is that a builtin target is not represented as a user function name.

`BuiltinId` should be assigned by the builtin registry and remain stable within a compilation. Builtin metadata should include:

- `id: BuiltinId`
- `kind: Runtime | Intrinsic | Host | Internal`
- canonical public name when one exists, e.g. `Byte.to_int`
- public free-function name when intentionally source-callable, e.g. `println`, `range`
- ABI parameter/result info
- runtime import module/name when applicable
- wrapper/materialization policy for first-class builtin values
- diagnostic/display name

### Distinct resolver namespaces

Resolver state should distinguish:

1. user value namespace — source-defined/imported functions and values;
2. public builtin namespace — free builtins intentionally callable by source identifier;
3. builtin method table — canonical method/member surface mapped to `BuiltinId`;
4. private builtin namespace — compiler-only targets, not source-resolvable.

Method resolution should return a builtin identity for builtin methods, not an internal implementation string. For example:

```tw
Byte.to_int  -> Builtin(ByteToInt)
Int.to_string -> Builtin(IntToString)
Vector.len -> Builtin(VectorLen)
```

A user declaration named `byte_to_int` remains in the user value namespace and does not affect the builtin method table.

### Carry identity through typed metadata

Checker metadata that currently stores method target names should store `CallableRef` or a builtin-specific target record. This applies to method calls, contract/stringify lowering, and first-class method values.

Where the compiler currently records a function name for a builtin, record a builtin id instead. Names should be used for diagnostics only.

### Core/ANF/backend callable references

Core IR and ANF currently model calls with global function references. Extend call representation so builtin calls are explicit, or ensure builtins have a separate `FuncId` space that cannot overlap with user functions.

Preferred direction:

```tw
type CoreCallee = {
  UserFunc(FuncId),
  Builtin(BuiltinId),
  LocalClosure(LocalId),
}
```

ANF/backend equivalents should preserve this distinction so codegen does not need to recover builtin-ness from a string name.

If reworking callee shapes is too large for one step, an intermediate compatibility layer can assign phantom function ids for builtins, but those ids must be tagged as builtin ids and must never be looked up through user-name maps.

### Codegen dispatch by builtin id

Codegen should dispatch builtin calls by `BuiltinId`/kind, not by string matching:

```tw
case builtin.kind {
  .Intrinsic => emit_intrinsic(builtin.id, args, ...),
  .Runtime(info) => emit_runtime_call(info, args, ...),
  .Host(info) => emit_host_call(info, args, ...),
}
```

Intrinsic emitters should match typed builtin ids or enum cases rather than names like `"byte_to_int"`.

### Diagnostics and symbols

Builtins still need human-readable names:

- diagnostics should prefer canonical names such as `Byte.to_int`;
- WAT/internal symbols may use reserved generated names such as `builtin$byte_to_int`;
- logs/debug IR should print both identity and display name when useful.

These names are display artifacts, not lookup keys in user namespaces.

## Migration plan

### Phase 1 — inventory and guardrails

- Inventory all string-keyed builtin lookups and exact-name comparisons.
- Add regression tests where user functions intentionally reuse old internal-looking names:
  - `byte_to_int`
  - `int_to_string`
  - `bool_to_string`
  - `vector_len`
  - `dict_new`
- Verify these user functions can call the corresponding public method/free API without recursion or collision.

### Phase 2 — builtin registry identity API

- Add typed `BuiltinId` helpers to the builtin registry.
- Keep existing names as metadata/display names only.
- Add lookup APIs by canonical public name and public free-function name.
- Avoid exposing private implementation-name lookup to resolver paths used for user code.

### Phase 3 — resolver/checker target metadata

- Change builtin method bindings from string target names to builtin ids.
- Change method-call metadata to carry builtin identity where applicable.
- Update stringify/contract lowering decisions to store typed builtin targets rather than names.

### Phase 4 — lowering and IR callable references

- Update lower_core calls and first-class method-value lowering to emit builtin callable references directly.
- Ensure closure materialization for builtin method values uses builtin wrapper metadata keyed by builtin id.
- Preserve user function `FuncId` lookup for actual user functions only.

### Phase 5 — backend and codegen

- Update backend verification to understand builtin callees directly.
- Update optimizer semantics to key builtin call semantics by builtin id.
- Replace codegen string dispatch with builtin-id dispatch.
- Keep generated symbol names reserved/unrepresentable, but do not use them for semantic lookup.

### Phase 6 — remove compatibility string paths

- Remove or restrict direct lookup of compiler-only builtin implementation names.
- Delete obsolete string-name guardrails that assume single flat namespace behavior.
- Update documentation/comments that describe single-underscore/double-underscore naming as collision prevention.

## Validation

Run after each major phase:

```bash
make stage2
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Add focused tests for:

- user-defined functions with old builtin-like names;
- builtin methods in direct-call position;
- builtin methods as first-class values;
- contract/stringify calls that choose builtin primitive conversions;
- imported prelude/std modules that reference builtin methods.

## Open questions

- Should public free builtins like `println` be modeled as `BuiltinId` in the public value namespace, or should they remain ordinary imported functions backed by builtin ids only after lowering?
- Should runtime, host, intrinsic, and internal helper builtins share one `BuiltinId` space with a `kind`, or use separate id types?
- Should builtin ids be stable across compiler versions for external tooling, or only stable within a compilation?
- How much IR churn is acceptable in one change versus using a compatibility adapter first?
