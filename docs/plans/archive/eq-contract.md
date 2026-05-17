# Eq Contract

This document specifies the `Eq` contract and how `==` / `!=` operators consume
it as a syntax hook, following the same pattern as string interpolation and
`Stringify`.

---

## Motivation

Today `==` compiles to a runtime `eq()` function that does dynamic type dispatch
over `anyref`. This has two problems:

1. **Records use reference equality.** Two records with identical field values
   compare as unequal unless they are the same object. This is almost never what
   users want.
2. **No static guarantee.** Any two values of the same type can be `==`'d,
   including types where equality is meaningless (e.g., closures).

An `Eq` contract makes equality opt-in, statically checked, and
monomorphizable.

---

## Definition

```tw
contract Eq {
  fn eq(self, other: Self) Bool
}
```

Required inherent method:

```tw
fn eq(a: T, b: T) Bool
```

A type satisfies `Eq` when its defining module provides an inherent `eq` method
with this shape (two parameters of the same type, returns `Bool`).

**Important:** The second parameter must unify with the receiver type. A function
`fn eq(a: Point, b: String) Bool` does NOT satisfy `Eq` for `Point` — both
parameters must be the same type.

---

## Syntax Hook: `==` and `!=`

`==` is a syntax hook for the `Eq` contract, analogous to `${}` for `Stringify`.

```tw
a == b
```

is legal when the type of both operands satisfies `Eq`.

Lowering: the checker resolves the `eq` method through contract satisfaction,
then emits a direct call to the resolved function. `!=` lowers to `not (a == b)`.

### Type checking flow

1. Checker encounters `BinOp.Eq` (or `BinOp.Ne`)
2. Synth/unify to determine the common type `T`
3. Call `satisfies_contract(T, .Eq, ctx)`
4. If satisfied: record the resolved method, return `Bool`
5. If not: emit diagnostic "type T does not satisfy Eq"

---

## Builtin Satisfaction

### Primitives (auto-satisfied)

These types satisfy `Eq` through compiler-recognized builtin equality:

| Type   | Wasm lowering       |
|--------|---------------------|
| Int    | `i64.eq`            |
| Float  | `f64.eq`            |
| Bool   | `i32.eq`            |
| Byte   | `i32.eq`            |
| String | byte-wise `rt_str__eq` |

No actual method call is emitted for primitives — the compiler emits the
appropriate Wasm instruction directly.

**Float semantics:** `Float` uses IEEE 754 equality (`f64.eq`), meaning
`NaN != NaN` and `+0.0 == -0.0`. This propagates into containers —
`[NaN] == [NaN]` is `false`. This matches most languages and is intentional.

### Builtin containers (conditionally satisfied)

| Type          | Condition                        |
|---------------|----------------------------------|
| Vector\<T\>  | T satisfies Eq                   |
| Dict\<K, V\> | K satisfies Eq AND V satisfies Eq |

Note: Dict key types already require hashability (Int or String). The `K: Eq`
requirement is additive — it does not replace the key-type restriction.

**Dict equality is order-independent.** Two dicts are equal when they contain the
same set of key-value entries, regardless of insertion order.

Lowering for containers calls the existing runtime helpers (`eq_vec`, `eq_dict`)
which recursively use the monomorphized element equality.

### Enums/Variants (auto-derived)

An enum type automatically satisfies `Eq` when all of its variant payloads
satisfy `Eq`. Unit variants (no payload) are compared by tag. Payload variants
compare tag + recursive payload equality.

This means `Option<T>` satisfies `Eq` when `T: Eq`, and `Result<T, E>` satisfies
`Eq` when both `T: Eq` and `E: Eq`.

Auto-derive for generic enums produces **conditional satisfaction**: the enum
satisfies `Eq` when all type parameters appearing in variant payloads satisfy
`Eq`. Type parameters that appear only in phantom position (unused in payloads)
do not contribute constraints.

---

## User-Defined Records

A user-defined record satisfies `Eq` in one of two ways:

### 1. Explicit inherent method

```tw
type Point = .{ x: Int, y: Int }

fn eq(a: Point, b: Point) Bool {
  a.x == b.x and a.y == b.y
}
```

### 2. Auto-derived structural equality

If a record type does NOT define an explicit `eq` method, the compiler
auto-derives field-wise structural equality **when all fields satisfy `Eq`**.

```tw
type Point = .{ x: Int, y: Int }
// No explicit eq needed — Int satisfies Eq, so Point auto-derives Eq

p1 := Point.{ x: 1, y: 2 }
p2 := Point.{ x: 1, y: 2 }
p1 == p2  // true (structural, not reference)
```

If any field does NOT satisfy `Eq` (e.g., a closure field), the type does not
auto-derive and the user must either:
- Define an explicit `eq` that handles those fields, or
- Accept that the type cannot be compared with `==`

### Why auto-derive for records?

Without auto-derive, every record that wants equality would need boilerplate.
Auto-derive is safe because:
- It only activates when ALL fields satisfy Eq
- Users can override with an explicit `eq` for custom semantics
- Types with non-Eq fields simply don't satisfy the contract (no silent fallback)

---

## Recursive Types (Coinductive Satisfaction)

Self-referential types are common (linked lists, trees, ASTs). A naive recursive
proof would loop forever:

```tw
type List<T> = { Nil, Cons(T, List<T>) }
// proving Eq for List<T> requires proving Eq for List<T> (in Cons payload)
```

The solution is **coinductive proof**: when proving `Eq` for a type `T`, if we
encounter `T` again during the recursive field/payload walk, we **assume it
holds** (return success) rather than reporting a cycle error.

This is sound because structural equality on recursive types is well-defined:
two values are equal iff their finite unfoldings match at every level. The
runtime already handles this correctly via the variant equality dispatch.

### Implementation

In `prove_contract`, when `active.has(key)` is true:
- For `Eq` (and future auto-derivable contracts): return `.Ok(ctx)` (coinductive assumption)
- For `Stringify` (not auto-derived): keep the existing `.Err(...)` behavior

This distinction is safe because auto-derived equality has a canonical recursive
semantics, while user-defined methods (like `to_string`) might not terminate on
cyclic structures — requiring an explicit definition forces the user to handle
that case.

---

## Generic Bounds

```tw
fn contains<T: Eq>(xs: Vector<T>, target: T) Bool {
  for x in xs {
    if x == target { return true }
  }
  false
}
```

Within the body, `==` on `T` is legal because `T: Eq` is declared.

---

## Conditional Satisfaction for Generic Types

```tw
type Pair<A, B> = .{ first: A, second: B }

fn eq<A: Eq, B: Eq>(a: Pair<A, B>, b: Pair<A, B>) Bool {
  a.first == b.first and a.second == b.second
}
```

`Pair<A, B>` satisfies `Eq` when `A: Eq` and `B: Eq`.

---

## Implementation Notes

### Changes required in `checker.tw`

These functions must be extended to handle the new variants:

1. **`same_builtin_contract`** — add `.Eq` branch
2. **`contract_return_type`** — add `.Bool => .Bool`
3. **`contract_requirement_summary`** — add `.Bool => "Bool"`
4. **`builtin_contract_satisfied_by_primitive`** — add `.Eq` branch (same types
   as Stringify: Int, Float, Bool, Byte, String)

### Second parameter validation in `prove_contract_method`

Currently `prove_contract_method` unifies the receiver (`inst.params[0]`) with
the target type but does not validate additional parameters. For `Eq`, `params[1]`
must also unify with the receiver type:

```tw
// After receiver unification succeeds:
if req.arg_count_without_receiver >= 1 {
  case silent_unify(recv_ctx, ty, inst.params[1]) {
    .Some(next_ctx) => { recv_ctx = next_ctx },
    .None => { return .Err("...") },
  }
}
```

### Fix hardcoded contract in `try_synth_method_call`

Line ~1214 hardcodes `.Stringify` in the `MethodCallInfo`. Fix:
`lookup_scoped_contract_method` should return the contract alongside the
requirement (e.g., as a tuple or a new record type):

```tw
type ScopedContractMethod = .{
  contract: BuiltinContract,
  req: ContractMethodRequirement,
}

fn lookup_scoped_contract_method(...) ScopedContractMethod? {
  // ... return .Some(.{ contract, req }) ...
}
```

Then use the returned contract in the `MethodCallInfo`.

### Auto-derive branch in `prove_contract`

Add a new branch in `prove_contract` between the primitive check and method
lookup. When the contract supports auto-derivation (currently only `Eq`):

1. Check if the type is a record → recursively prove `Eq` for all fields
2. Check if the type is an enum → recursively prove `Eq` for all variant payloads
3. If all recursive proofs succeed, return `.Ok(ctx)` without requiring an
   explicit `eq` method
4. If a field/payload fails, fall through to method lookup (allowing an explicit
   override)

This mirrors Option A from the review: a branch in `prove_contract` itself,
not synthetic function injection. No new functions are registered in the env.

### Auto-derive codegen strategy

`prove_contract` determines *satisfaction* at type-check time, but codegen still
needs to emit actual comparison instructions. The strategy:

- During monomorphization/lowering, when `==` is encountered on a type that was
  auto-derive-satisfied (no explicit `eq` method), the emitter generates an
  **inline field-wise comparison** at the call site (or a synthetic local helper
  function in the codegen output — not in the user-visible env).
- For records: emit `a.field1 == b.field1 and a.field2 == b.field2 and ...`
  (each sub-`==` recursively dispatches to the field type's equality).
- For enums: emit tag comparison, then per-variant payload comparison in a
  `case`-like branch.
- For recursive types: the synthetic helper calls itself, which is fine because
  Wasm functions can self-recurse.

This is analogous to how Rust's `#[derive(PartialEq)]` generates a synthetic
impl during macro expansion — except here it happens at codegen time rather than
in the source env.

### Coinductive cycle handling

Replace the blanket cycle error with contract-aware logic:

```tw
if active.has(key) {
  if contract_supports_auto_derive(contract) {
    return .Ok(ctx)  // coinductive: assume holds for recursive types
  }
  return .Err("cyclic proof for ...")
}
```

---

## Implementation Chain

1. **Enable multi-bound syntax** — remove the "multiple bounds are not supported
   yet" error in the parser/checker; accumulate multiple bounds in the
   `ResolvedTypeParam.bounds` vector (already a `Vector`). ✓
2. **Implement `Eq` contract** — extend `prove_contract` with auto-derive
   branch, coinductive cycle handling, builtin container conditional
   satisfaction, fix `lookup_scoped_contract_method` to return the contract. ✓
3. **Add proof cache** — memoize `satisfies_contract` results to avoid
   recomputing recursive proofs for the same type on every `==` occurrence.
   Without this, enforcing Eq at `==` sites is too expensive for large
   codebases (the boot compiler has thousands of `==` on complex types).
4. **Wire `==`/`!=` to `Eq`** — modify `synth_eq_op` to call
   `satisfies_contract(T, .Eq, ctx)` and emit diagnostics on failure.
   Requires step 3 for practical performance.
5. **Replace `assert.int_eq` / `bool_eq` / `string_eq`** with a single generic
   `assert.eq<T: Eq + Stringify>`.

---

## Migration Path

The current runtime `eq()` function uses dynamic anyref dispatch. After this
spec is implemented, `==` gains static checking (reject non-Eq types) while
continuing to use the runtime `eq()` for codegen. This is non-breaking for
nearly all existing code since records auto-derive and all commonly compared
types (primitives, strings, collections, enums) auto-satisfy.

**Future optimization (not required):** Since the compiler knows types at
compile time, it could emit monomorphized equality (inline field comparisons,
direct Wasm instructions) instead of boxing to anyref and dispatching at
runtime. This is purely a codegen optimization — the semantics are identical.

---

## Impact on Test Assertions

With `Eq` and `==` working structurally on records, the test library can add:

```tw
fn eq<T: Eq + Stringify>(actual: T, expected: T) Result<Void, String> {
  if actual == expected {
    .Ok(void)
  } else {
    .Err("expected ${expected}, got ${actual}")
  }
}
```

Note: Multiple bounds (`T: Eq + Stringify`) are parsed but not yet supported by
the checker. Enabling this is prerequisite work — the parser already recognizes
the `+` syntax and reports "multiple bounds are not supported yet." Once
enabled, this replaces the family of `int_eq`, `string_eq`, `bool_eq` etc. with
a single generic assertion.

---

## Types That Should NOT Satisfy Eq

- `fn(A) B` — closures have no meaningful equality
- Record types containing closure fields (unless explicit `eq` is defined that
  ignores or handles those fields)

---

## Relationship to Ordering

A future `Ord` contract (requiring `fn cmp(a: T, b: T) Order`) could follow the
same pattern for `<`, `<=`, `>`, `>=`. That is out of scope for this document.

---

## Summary

| Aspect | Stringify | Eq |
|--------|-----------|-----|
| Syntax hook | `${expr}` | `==` / `!=` |
| Required method | `to_string(self) String` | `eq(self, other: Self) Bool` |
| Primitives | auto-satisfied | auto-satisfied |
| Containers | conditional (T: Stringify) | conditional (T: Eq) |
| Records | must define `to_string` | auto-derived OR explicit `eq` |
| Enums | must define `to_string` | auto-derived when payloads: Eq |
| Recursive types | cycle error (must define explicitly) | coinductive (auto-derive succeeds) |
| Auto-derive | no | yes (records + enums) |
