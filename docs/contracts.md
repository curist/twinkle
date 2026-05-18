# Twinkle Contracts Reference

This is the reference for compiler-recognized contracts available in Twinkle today.
For design rationale and non-goals, see [design/contracts.md](design/contracts.md).

A contract is a named method requirement used by generic bounds and selected
syntax hooks. Types satisfy contracts through inherent methods, builtin rules, or
compiler-supported derivation where noted.

## Bounds

Generic parameters can require contracts:

```tw
fn show<T: Stringify>(x: T) String {
  x.to_string()
}

fn equal<T: Eq>(a: T, b: T) Bool {
  a == b
}

fn min<T: Ord>(a: T, b: T) T {
  if a < b { a } else { b }
}
```

Multiple bounds use `+`:

```tw
fn assert_equal<T: Eq + Stringify>(actual: T, expected: T) Result<Void, String> {
  if actual == expected { .Ok(()) } else { .Err(actual.to_string()) }
}
```

## `Stringify`

Required method:

```tw
to_string(self) -> String
```

Used by:

- string interpolation: `"value=${expr}"`
- generic `.to_string()` calls on `T: Stringify`
- APIs that require canonical string rendering

Builtin satisfaction:

- `Int`
- `Float`
- `Bool`
- `Byte`
- `String`

Other satisfaction:

- User-defined types satisfy `Stringify` by defining an inherent `to_string`.
- Generic user-defined types may satisfy it when their `to_string` method's own
  bounds are satisfied.
- `Vector<T>` satisfies `Stringify` through its prelude `to_string<T: Stringify>`
  witness.
- `Stringify` is not auto-derived.

Example:

```tw
type Point = .{ x: Int, y: Int }

fn to_string(p: Point) String {
  "(${p.x}, ${p.y})"
}
```

## `Eq`

Required method:

```tw
eq(self, other: Self) -> Bool
```

Used by:

- `==`
- `!=`
- generic APIs that require equality

Builtin satisfaction:

- `Int`
- `Float`
- `Bool`
- `Byte`
- `String`

Conditional satisfaction:

- `Option<T>` satisfies `Eq` when `T: Eq`.
- `Result<T, E>` satisfies `Eq` when `T: Eq` and `E: Eq`.
- `Vector<T>` satisfies `Eq` when `T: Eq`.
- `Dict<K, V>` satisfies `Eq` when `K: Eq` and `V: Eq`.

Auto-derivation:

- Records auto-derive `Eq` when all fields satisfy `Eq`.
- Enums auto-derive `Eq` when all payloads satisfy `Eq`.
- Recursive shapes are supported by coinductive proof.

Explicit satisfaction:

- A user-defined inherent `eq(self, other: Self) -> Bool` can witness `Eq`.

## `Ord`

Required method:

```tw
compare(self, other: Self) -> Order
```

Used by:

- `<`
- `<=`
- `>`
- `>=`
- `Vector.sort()`
- generic APIs that require canonical ordering

Builtin satisfaction:

- `Int`
- `Float`
- `Byte`
- `String`

Conditional satisfaction:

- `Vector<T>` satisfies `Ord` through its prelude `compare<T: Ord>` witness.

Explicit satisfaction:

- User-defined types satisfy `Ord` by defining an inherent
  `compare(self, other: Self) -> Order`.
- `Ord` is not auto-derived.
- `Ord` does not imply `Eq`.

## Syntax hooks

| Syntax | Contract |
|--------|----------|
| string interpolation | `Stringify` |
| `==`, `!=` | `Eq` |
| `<`, `<=`, `>`, `>=` | `Ord` |

For generic operands, the relevant contract must be present as a bound. For
concrete operands, the type must satisfy the contract through the rules above.
