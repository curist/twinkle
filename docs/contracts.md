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
- `Option<T>` satisfies `Stringify` through its prelude `to_string<T: Stringify>`
  witness: `Some(v)` renders as `Some(<v>)`, `None` as `None`.
- `Result<T, E>` satisfies `Stringify` through its prelude
  `to_string<T: Stringify, E: Stringify>` witness: `Ok(v)` → `Ok(<v>)`,
  `Err(e)` → `Err(<e>)`.
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
| `c[i]` (positional, `Int`-indexed) | `IndexRead<E>` |

For generic operands, the relevant contract must be present as a bound. For
concrete operands, the type must satisfy the contract through the rules above.
`c[i]` desugars to `IndexRead.at(c, i)` (unchecked, traps on out-of-bounds) when
`c` is a type variable bounded `IndexRead<E>`; concrete `Vector`/`String` keep
their direct positional read, and keyed `Dict<K, V>[K] -> V?` stays a separate
special case (a future `KeyedRead<K, V>`).

## Access contracts

A general positional-access pattern over collections is provided by
**parameterized contracts** with a `Self → E` functional dependency.

**`IndexRead<E>`** is implemented: `len(self) Int` and `at(self, Int) E`.
`Vector<T>` satisfies it (`E = T`) and `String` satisfies it (`E = Byte`); any
type with matching `len`/`at` inherent methods conforms. It backs the `c[i]`
syntax hook above and lets generic algorithms (`find`/`position`/`region_eq`/
`starts_with`) be written once over the bound and monomorphized to direct reads.

Still planned: `IntoIterator<E>` (`for x in`), `IndexWrite<E>`, and `Sliceable`
(range-slice `v[a..b]`, tracked separately). Design:
[plans/access-contracts.md](plans/access-contracts.md) (and the
[contract-model rationale](design/contracts.md)).
