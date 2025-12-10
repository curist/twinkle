## 1. `Cell<T>`: Overview

**Goal:**
Provide a single, explicit way to model **mutable state**, without changing:

* the immutable value model for arrays/records/etc.,
* the “update = rebinding” semantics,
* or the type system (still plain HM).

**Definition (informal):**

> `Cell<T>` is a heap-allocated box that contains a value of type `T`.
> The *contents* of a cell may be changed in place; the *binding* to the cell is still immutable.

So:

* `Cell<T>` is **mutable & shared**,
* `T` itself is still just a normal immutable value.

---

## 2. Core API

You can keep it very small to start:

```tw
type Cell<T>   // opaque, provided by the standard library
```

### 2.1 Construction

```tw
fn Cell.new<T>(initial: T) -> Cell<T>
```

* Allocates a new cell containing `initial`.

Example:

```tw
let counter = Cell.new(0)
```

---

### 2.2 Reading

```tw
fn Cell.get<T>(cell: Cell<T>) -> T
```

* Returns the *current* value stored in the cell.
* Does **not** change the cell.

Example:

```tw
let n = Cell.get(counter)
println(n)
```

---

### 2.3 Writing

```tw
fn Cell.set<T>(cell: Cell<T>, value: T) -> Unit
```

* Replaces the contents of `cell` with `value`.
* This is a **side effect**: future `Cell.get` sees `value`.

Example:

```tw
Cell.set(counter, 42)
```

---

### 2.4 Modify (optional but ergonomic)

```tw
fn Cell.update<T>(cell: Cell<T>, f: fn(T) -> T) -> Unit
```

* Reads the current `v = Cell.get(cell)`,
* computes `v2 = f(v)`,
* stores `v2` back into the cell.

Example:

```tw
Cell.update(counter, fn(n) { n + 1 })
```

This is your “atomic” read-modify-write primitive at the language level.

---

## 3. Semantics (how it fits your world)

### 3.1 Values vs cells

* **Normal values** (`int`, arrays, records, etc.) are **immutable**.
* **A `Cell<T>` is a mutable container** that *owns* a `T` internally.
* You can have many names pointing to the same `Cell<T>`:

  ```tw
  let counter = Cell.new(0)
  let counter2 = counter

  Cell.update(counter, fn(n) { n + 1 })
  println(Cell.get(counter2))   // prints 1
  ```

  This is **intended**: `Cell` is your *only* place where aliasable mutation lives.

### 3.2 No change to update sugar

All the “nice” update rules stay as they are:

* `x.y = v` → `x = { x with y = v }`
* `arr[i] = v` → `arr = Array.set(arr, i, v)`
* `x += 1` → `x = x + 1`

They **never** mutate a `Cell` for you.
If a record has a cell field:

```tw
type Model = { count: Cell<Int> }

let model = { count: Cell.new(0) }
```

You **do not** write:

```tw
model.count = ...       // this replaces the ENTIRE cell
```

To mutate the count, you explicitly go through the API:

```tw
Cell.update(model.count, fn(n) { n + 1 })
```

This keeps the semantic line crystal clear:

* `x.y =` → persistent record update, rebinding `x`.
* `Cell.set` / `Cell.update` → **actual side effects**.

---

## 4. Examples

### 4.1 Simple counter

```tw
fn make_counter() -> { inc: fn() -> Int } {
  let cell = Cell.new(0)

  let inc = fn() -> Int {
    Cell.update(cell, fn(n) { n + 1 })
    Cell.get(cell)
  }

  { inc = inc }
}

fn main() {
  let c = make_counter()
  println(c.inc())  // 1
  println(c.inc())  // 2
}
```

Notes:

* `cell` is shared between all calls to `inc`.
* State is *explicitly* in a `Cell`, not a random record field.

---

### 4.2 Config with locally mutable knob

```tw
type Config = {
  current: ConfigValue,
  history: Array<ConfigValue>,
}

type ConfigCell = Cell<Config>

fn set_theme(cfg: ConfigCell, theme: String) -> Unit {
  Cell.update(cfg, fn(c) {
    let new = { c.current with theme = theme }
    { current = new, history = c.history.append(new) }
  })
}

fn main() {
  let cfg = Cell.new({ current = default_config(), history = [] })

  set_theme(cfg, "dark")
  set_theme(cfg, "light")

  let final = Cell.get(cfg)
  println(final.current.theme)
}
```

Again:

* you don’t mutate `Config` in place,
* you mutate the **cell holding it**.

---

## 5. What patterns are now possible (and clearly marked as “stateful”)

With `Cell<T>`, you can now express:

* Shared counters
* Caches/memoization tables
* Module-level state
* “Objects” with internal state (via records of functions that close over Cells)
* Simple global toggles/flags

But with these constraints:

* All of them must go through `Cell.*`,
* Meaning your code clearly **visually marks** stateful stuff.

This is good both for humans and for tools (linters, docs).

---

## 6. What stays forbidden/unchanged

* You still **cannot** have:

  * “implicit” shared mutation via plain records/arrays.
  * `let a = x; let b = x; a.y = ...` magically updating something `b` sees.

* You **still** don’t have:

  * Rust-style mutable borrows,
  * aliasing-sensitive record updates,
  * trait-driven mutability magic.

`Cell` is a *deliberate escape hatch*, not the default way to model everything.

---

## 7. How hard is this to compile?

Implementation-wise, `Cell<T>` is just:

* a Wasm GC `struct` with one mutable field, or
* a pointer into linear memory with a payload slot.

The compiler doesn’t need special typing rules:

* `Cell<T>` is just another parametric type.
* `Cell.get/set/update` are ordinary functions (or intrinsics with known effects).
* Your type inference remains plain HM.

The only extra thing the compiler/runtime needs is:

* a way to represent `Cell<T>` references,
* and handle them in the garbage collector like any other heap node.

