# Ord Contract

This document specifies the `Ord` contract and how it enables `sort()` on
vectors and comparison operators (`<`, `>`, `<=`, `>=`) on user-defined types.

---

## Motivation

Today, sorting requires passing an explicit comparator:

```tw
names.sort_by(String.compare)
points.sort_by(fn(a: Point, b: Point) Order {
  case Int.compare(a.x, b.x) {
    .Eq => Int.compare(a.y, b.y),
    other => other,
  }
})
```

And comparison operators (`<`, `>`, `<=`, `>=`) only work on primitives. You
cannot write `point1 < point2` even when an obvious ordering exists.

An `Ord` contract makes ordering explicit, statically checked, and wired into
both `sort()` and comparison operators:

```tw
names.sort()
point1 < point2
```

`sort_by` remains available for custom orderings.

---

## Definition

```tw
contract Ord {
  fn compare(self, other: Self) Order
}
```

Required inherent method:

```tw
fn compare(a: T, b: T) Order
```

A type satisfies `Ord` when its defining module provides an inherent `compare`
method with this shape (two parameters of the same type, returns `Order`).

**No auto-derive.** Unlike `Eq` (which auto-derives structural equality for
records), `Ord` always requires an explicit `compare` method. Field declaration
order as sort key is arbitrary and surprising — ordering is a domain decision.

**Note:** `Ord` and `Eq` are independent contracts. A type can satisfy one
without the other (though in practice most types satisfy both).

---

## Builtin Satisfaction

### Primitives (auto-satisfied)

These types satisfy `Ord` through their existing prelude `compare` functions:

| Type   | Existing function    |
|--------|----------------------|
| Int    | `Int.compare`        |
| Float  | `Float.compare`      |
| Byte   | `Byte.compare`       |
| String | `String.compare`     |

`Bool` does **not** satisfy `Ord` — ordering booleans is not meaningful.

**Float caveat:** Primitive `<`/`>`/`<=`/`>=` on `Float` continue to use direct
Wasm float comparison instructions (IEEE 754 semantics: NaN comparisons return
false). These are NOT lowered through `Ord`. `Float.compare` has NaN-last
semantics for deterministic sorting, but the primitive operators retain their
existing IEEE behavior. This distinction only matters for NaN values.

Note that generic code like `fn le<T: Ord>(a: T, b: T) Bool { a <= b }`,
when instantiated with `Float`, will use `Float.compare` (NaN-last), not the
IEEE Wasm instruction. So concrete `Float <= Float` and generic `le<Float>`
may differ for NaN inputs. This is acceptable — generic code uses the
contract's semantics, concrete code uses the primitive's.

### Builtin containers

| Type          | Condition                          |
|---------------|------------------------------------|
| Vector\<T\>   | T satisfies Ord                    |

Vectors compare lexicographically: element-by-element, shorter vector is
less when all shared elements are equal.

Unlike primitives (which satisfy `Ord` through existing prelude functions),
`Vector<T>: Ord` requires a concrete `compare` function that monomorphization
can resolve to. Add to `prelude/vector.tw`:

```tw
pub fn compare<T: Ord>(a: Vector<T>, b: Vector<T>) Order {
  min_len := if a.len() < b.len() { a.len() } else { b.len() }
  i := 0
  for i < min_len {
    case a[i].compare(b[i]) {
      .Eq => {},
      other => { return other },
    }
    i = i + 1
  }
  Int.compare(a.len(), b.len())
}
```

And register it in the resolver alongside other vector methods:

```tw
me("compare", "compare"),
```

Dict does **not** satisfy `Ord` — key-value maps have no natural ordering.

### Enums/Variants

An enum type satisfies `Ord` only when it defines an explicit `compare`
method. No auto-derive.

---

## Syntax Hooks

### Comparison operators: `<`, `>`, `<=`, `>=`

These operators are syntax hooks for `Ord`, analogous to `==`/`!=` for `Eq`.

**For primitives** (Int, Float, Byte, String), the operators continue to emit
direct Wasm instructions — no change from today. Primitives are never lowered
through the `Ord` contract.

**For non-primitive types** satisfying `Ord`, the operators lower through the
contract system:

| Operator | Lowering                              |
|----------|---------------------------------------|
| `a < b`  | `a.compare(b) == .Lt`                 |
| `a > b`  | `a.compare(b) == .Gt`                 |
| `a <= b` | `a.compare(b) != .Gt`                 |
| `a >= b` | `a.compare(b) != .Lt`                 |

For **concrete types** (e.g. `Point < Point`), this emits a direct call to the
resolved `compare` method.

For **generic types** (e.g. `a < b` where `a: T` and `T: Ord`), this emits a
`ContractCall(.Ord, "compare", a, [b])` which is resolved during
monomorphization — same mechanism as `Stringify` and `Eq` contract calls.

**Semantic tightening:** Currently `synth_cmp_op` accepts any same-type
operands after unification. Wiring to `Ord` intentionally narrows this — only
types satisfying `Ord` can use `<`/`>`/`<=`/`>=`. This should not break
existing code since today only primitives use these operators. Note: `Bool`
is primitive but intentionally does NOT satisfy `Ord`, so `Bool < Bool` becomes
a checker error. Currently `Bool < Bool` type-checks but would fail at codegen;
with `Ord` it fails earlier at the checker, which is better.

### `sort()` method

```tw
pub fn sort<T: Ord>(xs: Vector<T>) Vector<T>
```

Implemented directly in prelude using a contract call:

```tw
pub fn sort<T: Ord>(xs: Vector<T>) Vector<T> {
  xs.sort_by(fn(a: T, b: T) Order {
    a.compare(b)
  })
}
```

Since `T: Ord` means `a.compare(b)` is a bounded type-variable method call,
this lowers to `ContractCall(.Ord, "compare", a, [b])` — reusing the existing
contract-call infrastructure. Monomorphization resolves it to `Int.compare`,
`Point.compare`, etc. No special `sort()` lowering or `core_compare` runtime
builtin is needed.

### Interaction with `Eq`

`==` and `!=` remain wired to the `Eq` contract. `Ord` only governs `<`, `>`,
`<=`, `>=`, and `sort()`. The two contracts are fully independent — `Ord` does
NOT imply `Eq`. In practice this is fine: records auto-derive `Eq`, so adding
an explicit `compare` for `Ord` gives a type both `==` and `<` without
redundancy. Users who want equality from ordering can write
`compare(a, b) == .Eq` explicitly.

---

## Checker Changes

### `contracts.tw`

Add `Ord` variant to `BuiltinContract` and its spec:

```tw
pub type BuiltinContract = { Stringify, Eq, Ord }

// In spec():
.Ord => .{
  name: "Ord",
  methods: [.{
    method_name: "compare",
    receiver_param_count: 1,
    arg_count_without_receiver: 1,
    ret: .Order,   // new ContractReturnShape variant
  }],
},

// In resolve_builtin_contract_name():
"Ord" => .Some(.Ord),
```

### `ContractReturnShape`

Add `.Order` variant. Unlike `.Bool` and `.String` which map to primitive
`MonoType`s, `.Order` maps to `MonoType.Named(order_type_id, [])`. This
requires resolving `Order`'s TypeId at checker time — either by looking it up
from the env by name or by assigning it a stable builtin TypeId (like
Option=0, Result=1, etc.).

### `checker.tw`

Extend existing contract infrastructure:

1. **`same_builtin_contract`** — add `.Ord` branch
2. **`contract_name`** — add `"Ord"`
3. **`contract_return_type`** — add `.Order` case; needs env access or a stable
   TypeId for the `Order` enum
4. **`builtin_contract_satisfied_by_primitive`** — add `.Ord` branch for Int,
   Float, Byte, String (NOT Bool)
5. **`contract_supports_auto_derive`** — return `false` for `.Ord`
6. **`try_builtin_container_contract`** — add `.Ord` branch for Vector
   (conditionally satisfied when `T: Ord`)
7. **`synth_cmp_op`** — for non-primitive types, call
   `satisfies_contract(T, .Ord, ctx)` and emit diagnostic on failure

   Edge cases to handle (mirroring `synth_eq_op`):
   - `MetaVar`: skip contract check (same as Eq behavior)
   - Type variables with `Ord` bound: allowed
   - Byte/Int cross-comparison: keep the existing fast path (both are primitives
     satisfying Ord, no contract check needed)
   - `Bool`: not `Ord` — `Bool < Bool` is now a checker error (improvement over
     current behavior where it type-checks but fails at codegen)

### `prove_contract_method` fix (prerequisite)

The existing `prove_contract_method` only validates `params[0]` (the receiver)
but does not check that non-receiver parameters match the expected shape. For
contracts where `arg_count_without_receiver >= 1`, each additional parameter
must also unify with the target type. This means
`fn compare(a: Point, b: String) Order` would currently (incorrectly) satisfy
`Ord` for `Point`.

Fix by validating all non-receiver parameters:

```tw
for i in range(req.arg_count_without_receiver) {
  param_idx := req.receiver_param_count + i
  case silent_unify(recv_ctx, ty, inst.params[param_idx]) {
    .Some(next_ctx) => { recv_ctx = next_ctx },
    .None => { return .Err("...") },
  }
}
```

This fix benefits both `Eq` and `Ord`.

---

## Lowering Changes

### Comparison operators

In `lower_core/operators.tw`, extend comparison operator lowering for
non-primitive types satisfying `Ord`.

**For concrete types** (type is known, not a type variable):
1. Look up the `compare` method for the type (same as custom `eq` lookup)
2. Emit a direct call to the resolved function
3. Match the `Order` result to produce a `Bool`

**For generic types** (`T: Ord` where `T` is a type variable):
1. Emit `ContractCall(.Ord, "compare", left, [right])` — this is resolved
   during monomorphization to the concrete `compare` function

In both cases, the `Order` result is matched at the Core IR level using a
`Match` expression on the `Order` variants (`.Lt`, `.Eq`, `.Gt`), not via
low-level struct/tag extraction (that's a codegen detail):

```tw
// a < b  lowers to:
case compare(a, b) { .Lt => true, _ => false }

// a <= b  lowers to:
case compare(a, b) { .Gt => false, _ => true }
```

The lowering needs access to `Order`'s TypeId and variant IDs (Lt, Eq, Gt) to
construct the `Match` patterns. This is the same `Order` TypeId needed by
`contract_return_type` in the checker — resolve it once and thread it through.

### Prelude registration

Add `sort` alongside `sort_by` in the vector method registry:

```tw
// In resolver.tw, vector method registration:
me("sort", "sort"),
```

### Prelude `vector.tw`

```tw
pub fn sort<T: Ord>(xs: Vector<T>) Vector<T> {
  xs.sort_by(fn(a: T, b: T) Order {
    a.compare(b)
  })
}
```

The `a.compare(b)` call inside the closure is a contract call on bounded `T`,
lowered to `ContractCall(.Ord, "compare", a, [b])`. Monomorphization resolves
it to the concrete function. No special machinery needed.

---

## Files Requiring Changes

The `BuiltinContract` enum is pattern-matched across multiple files. All
exhaustive matches need an `.Ord` branch:

- `boot/compiler/contracts.tw` — type definition, spec, methods, resolve
- `boot/compiler/checker.tw` — satisfaction, proof, synth_cmp_op
- `boot/compiler/ir_print.tw` — contract name printing
- `boot/compiler/lower_core/operators.tw` — comparison operator lowering
- `boot/compiler/monomorphize.tw` — contract resolution during monomorphization
  (currently biased toward Stringify; generalize rather than add ad-hoc cases)
- `boot/compiler/core_linker/contract_resolve.tw` — pattern-matches
  `BuiltinContract`, needs `.Ord` branch
- `boot/compiler/core_linker/dce.tw` — if contract-resolution assumptions
  affect dead code elimination, may need adjustment
- `boot/compiler/resolver.tw` — vector sort and compare method registration
- `prelude/vector.tw` — sort and compare functions
- `prelude/signatures/vector.tw` — sort/compare signatures (if applicable;
  check whether builtin vector APIs need signature entries here)
- `boot/lib/module/core_lib.tw` — embedded prelude sources must be updated
  after prelude changes (sort and compare added to vector.tw)

---

## No Runtime Changes Needed

Unlike `Eq` which generates `eq_rec_*` / `eq_sum_*` runtime helpers for
structural dispatch, `Ord` does NOT need runtime helpers. Since there is no
auto-derive, every `Ord`-satisfying type has an explicit `compare` method that
is called directly. The runtime `core.tw` module is unchanged.

This is the key architectural difference from `Eq`: comparison is always
resolved statically to a concrete function, never dispatched dynamically at
runtime. `Ord` follows the `Stringify` model (explicit method, resolved at
monomorphization) rather than the `Eq` model (auto-derive + runtime dispatch).

---

## Implementation Chain

1. **Fix `prove_contract_method` non-receiver param validation** — prerequisite
   that benefits both Eq and Ord.
2. **Checker: add `Ord` contract** — extend `BuiltinContract`, `ContractSpec`,
   `ContractReturnShape`, contract satisfaction checks. No auto-derive. Resolve
   `Order` TypeId for return type.
3. **Checker: wire `<`/`>`/`<=`/`>=` to `Ord`** — extend `synth_cmp_op` to
   call `satisfies_contract(T, .Ord, ctx)` for non-primitive types.
4. **Lowering: comparison operators** — emit `ContractCall` for generic types,
   direct `compare` call for concrete types, with Core-level `Match` on `Order`.
5. **Monomorphization: generalize contract resolution** — ensure
   `resolve_contract_target_id` handles `Ord` cleanly alongside `Stringify`
   and `Eq`, not as ad-hoc special cases.
6. **Prelude: add `sort()` method** — register as Vector inherent method,
   implement in `vector.tw` using contract call in closure, update embedded
   prelude sources.
7. **Update all pattern-match sites** — `ir_print.tw`, `monomorphize.tw`, etc.
8. **Tests and examples** — Ord contract checker tests, comparison operator
   tests for user-defined types, sort tests, generic `T: Ord` tests.

---

## Examples

```tw
// Primitives — unchanged behavior
xs: Vector<Int> = [3, 1, 2]
xs.sort()  // [1, 2, 3]

names: Vector<String> = ["charlie", "alice", "bob"]
names.sort()  // ["alice", "bob", "charlie"]

// Records with explicit compare
type Point = .{ x: Int, y: Int }

fn compare(a: Point, b: Point) Order {
  case Int.compare(a.x, b.x) {
    .Eq => Int.compare(a.y, b.y),
    other => other,
  }
}

points: Vector<Point> = [.{ x: 3, y: 1 }, .{ x: 1, y: 2 }, .{ x: 1, y: 1 }]
points.sort()  // [.{ x: 1, y: 1 }, .{ x: 1, y: 2 }, .{ x: 3, y: 1 }]

// Comparison operators work on types satisfying Ord
p1 := Point.{ x: 1, y: 2 }
p2 := Point.{ x: 3, y: 1 }
p1 < p2   // true
p2 >= p1  // true

// Generic functions with Ord bounds
fn min<T: Ord>(a: T, b: T) T {
  if a <= b { a } else { b }
}

// Custom ordering still uses sort_by
points.sort_by(fn(a: Point, b: Point) Order {
  Int.compare(a.y, b.y)  // sort by y only
})

// Compile error: type does not satisfy Ord
type Wrapper = .{ name: String, callback: fn() Void }
ws: Vector<Wrapper> = [...]
ws.sort()       // ERROR: type Wrapper does not satisfy Ord
w1 < w2         // ERROR: type Wrapper does not satisfy Ord
```

---

## Non-Goals

- **Auto-derive for records or enums**: Ordering is a domain decision, not
  derivable from field/variant declaration order.
- **Runtime structural dispatch**: No `core.compare` runtime function. All
  comparison resolves statically to explicit `compare` methods. Follows the
  `Stringify` model, not the `Eq` model.
- **`Ord` implies `Eq`**: The contracts are independent. Auto-derived `Eq`
  covers most cases already.
- **`min`/`max` functions**: Could be added later using `T: Ord` bounds, but
  are out of scope for the initial implementation.
