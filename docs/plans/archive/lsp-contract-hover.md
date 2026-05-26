# LSP Contract Hover

## Goal

Add hover information for Twinkle's current compiler-recognized contracts in the
LSP, without adding hover support for operator tokens.

This should make contract bounds and contract-backed method calls explainable at
the point where users encounter them.

## Scope

In scope:

* hover on builtin contract names in generic bounds,
* hover on contract-backed method calls on bounded type parameters,
* tests through the existing LSP hover suite.

Out of scope:

* hover on `==`, `!=`, `<`, `<=`, `>`, or `>=` operator tokens,
* user-defined contracts,
* changes to contract checking or lowering semantics.

## Current State

Contract metadata already exists in `boot/compiler/contracts.tw`:

* `BuiltinContract`
* `spec(contract)`
* `methods(contract)`
* `resolve_builtin_contract_name(name)`

The checker records contract-backed method calls in `MethodCallInfo`:

```tw
type MethodCallInfo = .{
  func_name: String,
  has_receiver: Bool,
  contract: BuiltinContract?,
  method_name: String,
}
```

For calls such as `x.to_string()` where `x: T` and `T: Stringify`, `func_name`
is intentionally empty and `contract` carries the contract witness information.
`hover_recorded_method` currently returns no hover for that case.

Type-parameter bounds are parsed into `TypeParam.bounds`, but hover does not yet
inspect type parameters on function or type declarations. `hover.tw` currently
imports several AST types explicitly, so this work will also need to import
`TypeParam` from `compiler.ast`.

## Desired Behavior

### Bound hover

```tw
fn show<T: Stringify>(x: T) String { x.to_string() }
           ^ hover
```

Should show a concise contract reference, for example:

```text
contract Stringify

Required method: `fn to_string(value: Self) String`

Used by string interpolation and generic stringification.
```

Multiple bounds should work independently:

```tw
fn assert_equal<T: Eq + Stringify>(a: T, b: T) Void { ... }
                   ^    ^ hover each bound
```

Type declaration bounds should also work:

```tw
type Box<T: Stringify> = .{ value: T }
            ^ hover
```

### Contract-backed method hover

```tw
fn show<T: Stringify>(x: T) String {
  x.to_string()
    ^ hover
}
```

Should show the contract method shape and identify the source contract, for
example:

```text
fn to_string(value: Self) String

Contract method from `Stringify`.
```

Equivalent behavior should exist for `Eq.eq` and `Ord.compare` when called on a
bounded type parameter.

## Implementation Plan

### 1. Add contract hover formatting helpers

In `boot/compiler/query/hover.tw`, import the contract metadata module and add
small helpers that return hover text for builtin contracts and their methods.

Suggested helpers:

```tw
fn contract_hover(contract: BuiltinContract) String
fn contract_method_hover(contract: BuiltinContract, method_name: String) String?
```

Keep the strings reference-like and stable. They should match the current
reference in `docs/contracts.md`.

The first paragraph of each returned string is rendered as a Twinkle code block
by the LSP adapter, so use a code-like first line such as `contract Stringify` or
`fn to_string(value: Self) String`. Put explanatory prose after a blank line.

### 2. Hover over type-parameter bounds

Add a helper that inspects `Vector<TypeParam>`:

```tw
fn hover_type_params(params: Vector<TypeParam>, offset: Int) HoverResult?
```

For each bound path:

1. Check whether `bound.span.contains(offset)`.
2. Resolve the bound spelling with `resolve_builtin_contract_name`. Current
   parsed bounds are single-segment paths, but using the last segment keeps the
   helper tolerant of future qualified syntax.
3. Return `contract_hover(contract)` with the bound span.
4. Return no hover for unknown contracts; diagnostics already report them.

Call this helper from:

* `hover_function_decl` before parameter/return/body hover,
* `hover_type_decl` before type definition hover.

This should not interfere with existing hovers because type-parameter bound spans
are disjoint from function names, type names, parameter names, and type
annotations.

### 3. Hover over contract-backed method calls

Update `hover_recorded_method`:

* If `meta.func_name != ""`, keep the existing registered-function behavior.
* If `meta.func_name == ""` and `meta.contract` is present, return
  `contract_method_hover(contract, meta.method_name)`.
* Keep returning no hover when neither a function nor a contract is available.

This uses checker-provided evidence instead of re-resolving contracts in the
hover query.

### 4. Add tests

Extend `boot/tests/suites/lsp_hover_suite.tw` with cases covering:

* hover on `Stringify` in a function bound,
* hover on both bounds in `T: Eq + Stringify`,
* hover on `Stringify` in a type declaration bound,
* hover on `to_string` in a contract-backed method call on `T: Stringify`,
* hover on `eq` or `compare` in contract-backed method calls for `Eq`/`Ord`.

Use `assert.str_contains` for long markdown hovers rather than exact equality.
Suggested checks:

* bound hover contains `contract Stringify`, `contract Eq`, or `contract Ord`,
* bound hover contains the required method name,
* method hover contains the method signature,
* method hover contains `Contract method from` and the contract name.

## Notes

The LSP adapter already formats hover content by splitting at the first blank
line: the first paragraph becomes a Twinkle code block and the rest becomes
markdown. Contract hover strings should use that convention intentionally.

Operator-token hover is deliberately excluded because current hover resolution is
AST-node based and operator token spans are not the useful first target for this
feature.
