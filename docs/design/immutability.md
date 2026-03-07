# Immutability and Explicit State

Twinkle uses immutable values with rebindable names. This document covers the
core model, update syntax, aliasing behavior, closures, and `Cell<T>` as the
explicit escape hatch for shared mutable state.

---

## Core Model

* Primitives, strings, arrays, records, dicts, and functions are immutable values.
* `x = expr` means rebinding a name to a new value, not mutating in place.
* Assignment-like updates are sugar over "build new value + rebind".

---

## Update Syntax

Twinkle provides imperative-looking update syntax, but semantics are purely
value-based.

### Record field update

```tw
x.field = expr
// desugars to: x = RecordUpdate(x, field, expr)
```

### Array update

```tw
arr[i] = value
// desugars to: arr = Array.set(arr, i, value)
```

### Dict update

```tw
m[k] = v
// desugars to: m = Dict.set(m, k, v)
```

### LHS constraints

* The root assignment target must be a local identifier.
* Nested field chains are supported: `a.b.c = x` desugars to nested `RecordUpdate` calls.
* Chains starting from expressions are not allowed: `foo().x = 1` is an error.

---

## Aliasing

Since values are immutable, aliasing is safe:

```tw
type Pt = .{ y: Int }

p := Pt.{ y: 0 }
q := p
p.y = 1
// p == Pt.{ y: 1 }
// q == Pt.{ y: 0 }
```

Rebinding `p` creates a new value; `q` still refers to the original.

---

## Function Parameters

Parameters are local bindings. Rebinding a parameter is local to the function
and cannot affect the caller:

```tw
fn bump(n: Int) Int {
  n = n + 1
  n
}
```

---

## Closures

Closures capture values at definition time. Rebinding later does not affect
existing closures:

```tw
x := 1
f := fn() Int { x }
x = 2
f()    // 1
```

If a closure captures a `Cell<T>` value, it captures the cell reference, so all
aliases observe the same cell updates.

---

## `Cell<T>`: Explicit Mutable State

`Cell<T>` is the escape hatch for shared mutable state:

```tw
type Cell<T>

fn Cell.new<T>(initial: T) Cell<T>
fn Cell.get<T>(cell: Cell<T>) T
fn Cell.set<T>(cell: Cell<T>, value: T) Void
fn Cell.update<T>(cell: Cell<T>, f: fn(T) T) Void
```

* `Cell<T>` is mutable and aliasable.
* The payload `T` is still an ordinary immutable value.
* `Cell.set` and `Cell.update` are side effects.
* Update sugar (`x.y = ...`, `arr[i] = ...`) never mutates a cell implicitly.

```tw
counter := Cell.new(0)
other := counter

counter.update(fn(n: Int) Int { n + 1 })
println(other.get())    // 1
```

---

## Positioning

Twinkle is closest to the Elm/Gleam/Roc/ML family:

* Immutable values, updates as new values, value semantics by default.
* Assignment-like rebinding syntax (`x = ...`, `x.y = ...`, `arr[i] = ...`)
  while keeping persistent semantics.
* Shared mutation is explicit via `Cell<T>`, not implicit via ordinary data.
