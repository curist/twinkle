# Inherent Protocols

Twinkle has no traits or interfaces, but it already has a practical extension
mechanism: inherent methods resolved from the static receiver type.

This document names and formalizes that mechanism as **Inherent Protocols**.

---

## Why This Exists

Today, some language features rely on compiler-recognized method contracts:

* String interpolation uses `to_string() -> String`
* Collection behavior is partly syntax-driven (`[]`, `for`, `collect`) and partly
  method-based (`len`, `slice`, `next`, etc.)

Without a named model, these can feel ad-hoc. Inherent Protocols provide a
single vocabulary:

* a feature advertises a protocol name,
* the protocol defines required inherent method signatures,
* a type conforms if those signatures resolve on its static type.

No trait solver is introduced.

---

## Definition

An **Inherent Protocol** is a compiler-defined contract of one or more inherent
method signatures.

A type **conforms** to a protocol when method resolution on its static type can
find those signatures.

Resolution model is unchanged:

1. Determine static receiver type.
2. Resolve inherent method via existing type/module method registry.
3. Instantiate generics, type-check args, verify return type contract.

This is the same machinery used by normal method calls and method values.

---

## Design Constraints

Inherent Protocols are intentionally weaker than traits:

* No generic bounds like `T: Slice`.
* No global instance search/coherence.
* No impl blocks or orphan rules.
* No dynamic dispatch surface added.

Generic polymorphism across "anything that can X" remains explicit via
capability records (records of functions).

---

## Protocol Catalog

This is the recommended initial catalog.

### `Stringify` (current behavior, formalized)

Purpose: interpolation `${expr}`.

Required method:

```tw
to_string(self) -> String
```

Conformance:

* Builtins conform where registered (`Int`, `Float`, `Bool`, `Byte`, `String`).
* User named types conform by defining an inherent `to_string` with matching
  signature.

---

### `Slice` (proposed)

Purpose: range-based indexing/slicing syntax.

Required methods:

```tw
slice(self, start: Int, end: Int) -> Sliced
```

Optional helper for open bounds:

```tw
len(self) -> Int
```

Surface mapping:

* `x[m..n]` uses `slice(m, n)`
* `x[m..]`, `x[..n]`, `x[..]` require `len` and desugar to closed `slice` calls

Notes:

* Closed range values (`m..n`) can remain first-class `Range`.
* Open forms (`m..`, `..n`, `..`) are slice-selector syntax inside `[]`.

---

### `IndexRead` / `IndexWrite` (optional, later)

Purpose: generalize `x[i]` and `x[i] = v` beyond builtins.

Potential contracts:

```tw
get_unsafe(self, index: Index) -> Elem
set_unsafe(self, index: Index, value: Elem) -> Self
```

This is optional for initial rollout. Twinkle can adopt `Slice` first and keep
current builtin-only index behavior for non-slice forms.

---

### `IntoIterator` (optional, later)

Purpose: allow `for x in value` for user-defined types via explicit conversion.

Potential contract:

```tw
to_iter(self) -> Iterator<T>
```

Then:

```tw
for x in value { ... }
```

desugars to:

```tw
for x in value.to_iter() { ... }
```

`Iterator.unfold` remains the constructor primitive; `to_iter` is the entry
point protocol.

---

## Syntax and Desugaring

### Closed range expression

```tw
m..n
```

desugars to `range_from(m, n)` and produces `Range`.

### Slice selectors in indexing

```tw
x[m..n]
x[m..]
x[..n]
x[..]
```

desugar to `slice` (and `len` for open bounds) on the receiver.

### Reusable ranges

Closed ranges remain reusable values:

```tw
r := 2..5
part := xs[r]
```

This keeps first-class range value semantics while limiting open-bound syntax to
indexing context.

---

## Type-Checking Rules

For each protocol-backed surface form:

1. Resolve required method(s) through normal inherent method lookup.
2. Check arity and argument types.
3. Validate protocol return type contract.
4. Emit protocol-specific diagnostics when missing/mismatched.

Example diagnostics:

* `type Buffer does not conform to protocol Slice: missing slice(self, Int, Int)`
* `type User has to_string(self) -> Byte, expected String for Stringify`

---

## Lowering Rules

Lowering should use ordinary method-call lowering after protocol checks. No new
runtime representation is required for protocols.

Examples:

* `${x}` -> `x.to_string()` + string concat
* `x[m..n]` -> `x.slice(m, n)`
* `x[m..]` -> `x.slice(m, x.len())`

---

## Interaction With Capability Records

Inherent Protocols are syntax hooks. Capability records remain the mechanism for
generic APIs that abstract over behavior.

Use protocol conformance when language syntax needs it.
Use capability records when API polymorphism needs it.

These two models are complementary.

---

## Non-Goals

* Adding trait/interface syntax
* Implicit generic constraints
* Global instance selection
* Changing `Range` into an open-ended iterator family in MVP

---

## Rollout Plan

1. Document `Stringify` as an explicit protocol (no semantic change).
2. Add `Range` literal `m..n` as first-class expression (closed range).
3. Add `Slice` protocol-backed indexing forms:
   * `x[m..n]` first
   * `x[m..]`, `x[..n]`, `x[..]` next
4. Consider `IntoIterator` and `IndexRead/IndexWrite` only after syntax and
   diagnostics stabilize.

