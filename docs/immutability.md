> Note: This is a design note. Canonical language syntax/rules are `docs/spec.md` and `docs/grammar.ebnf`.

# Twinkle Immutability and Explicit State

This document consolidates:

* `docs/immutable.md`
* `docs/immtable-with-cell.md`
* `docs/compare-to-other-pl-immutable.md`

## 1. Core Model

Twinkle uses immutable values with rebindable names.

* Primitives, strings, arrays, records, dicts, and functions are immutable values.
* `x = expr` means rebinding a name to a new value, not mutating in place.
* Assignment-like updates are sugar over "build new value + rebind".

## 2. Update Syntax and Desugaring

Twinkle provides imperative-looking update syntax, but semantics are purely value-based.

### 2.1 Record field update

```tw
x.field = expr
```

Desugars to:

```tw
x = { x with field = expr }
```

### 2.2 Array update

```tw
arr[i] = value
```

Desugars to:

```tw
arr = Array.set(arr, i, value)
```

### 2.3 Dict update

```tw
m[k] = v
```

Desugars to:

```tw
m = Dict.set(m, k, v)
```

### 2.4 LHS constraints

* The root assignment target must be a local identifier.
* Nested field chains are supported:

```tw
a.b.c = x
// a = { a with b = { a.b with c = x } }
```

* Chains starting from expressions are not allowed:

```tw
foo().x = 1    // error
```

## 3. Functions, Aliasing, and Closures

### 3.1 Function parameters

Parameters are local bindings. Rebinding a parameter is local to the function and cannot mutate caller-visible ordinary values.

```tw
fn bump(n: Int) Int {
  n = n + 1
  n
}
```

### 3.2 Aliasing with immutable values

```tw
type Pt = .{ y: Int }

p := Pt.{ y: 0 }
q := p
p.y = 1
```

Result:

* `p == Pt.{ y: 1 }`
* `q == Pt.{ y: 0 }`

Twinkle has value semantics for ordinary values.

### 3.3 Closure capture

Closures capture values at definition time. Rebinding later does not affect existing closures.

```tw
x := 1
f := fn() Int { x }
x = 2
f()    // 1
```

If a closure captures a `Cell<T>` value, it captures the cell reference value, so all aliases observe the same cell updates.

## 4. `Cell<T>`: Explicit Mutable State

`Cell<T>` is the explicit escape hatch for shared mutable state.

```tw
type Cell<T>

fn Cell.new<T>(initial: T) Cell<T>
fn Cell.get<T>(cell: Cell<T>) T
fn Cell.set<T>(cell: Cell<T>, value: T) Void
fn Cell.update<T>(cell: Cell<T>, f: fn(T) T) Void
```

Semantics:

* `Cell<T>` is mutable and aliasable.
* The payload `T` is still an ordinary immutable value.
* `Cell.set` and `Cell.update` are side effects.
* Update sugar (`x.y = ...`, `arr[i] = ...`) never mutates a cell implicitly.

Example:

```tw
counter := Cell.new(0)
other := counter

counter.update(fn(n: Int) Int { n + 1 })
println(other.get())    // 1
```

## 5. Allowed and Disallowed Patterns

Allowed:

* Local rebinding in loops and branches.
* Record/array/dict updates via rebinding sugar.
* Shared mutable state only through explicit `Cell.*` APIs.

Disallowed (or semantically invalid expectations):

* Treating record/array updates as shared-object mutation.
* LHS roots that are expressions (`foo().x = 1`).
* Rebinding captured variables from within closures.

## 6. Positioning vs Other Languages

Twinkle is closest to the Elm/Gleam/Roc/ML family in semantics:

* immutable values,
* updates as new values,
* value semantics by default.

The main stylistic difference is surface syntax:

* Twinkle allows assignment-like rebinding (`x = ...`, `x.y = ...`, `arr[i] = ...`) while keeping persistent semantics.
* Shared mutation is explicit via `Cell<T>`, not implicit via ordinary records/arrays.
