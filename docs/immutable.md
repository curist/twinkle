## 🧾 1. Values, Bindings, and Assignment

### 1.1 Values are immutable

* All user-visible values are immutable:

  * primitives (`int`, `float`, `bool`),
  * arrays, maps, strings,
  * records and user-defined types.

> There is no observable in-place mutation of values in the language model.

### 1.2 Bindings can be rebound (local “mutation”)

* A `let` binding introduces a name bound to a value:

  ```tw
  let x = expr
  ```

* Within the same block, the **same name** may appear on the left-hand side of `=` again, which is treated as **rebinding** / shadowing:

  ```tw
  x = expr2     // new value, old x is no longer used in this scope
  ```

* Semantically, this is equivalent to introducing a new binding `x₁` that hides `x₀`:

  ```tw
  let x₀ = expr
  let x₁ = expr2   // written as `x = expr2`
  ```

No other name is implicitly updated when you rebind `x`.

---

## 🧾 2. Update Statements (Field and Index Assignment)

Update-like syntax is provided for ergonomics but **always desugars to “build new value + rebind name”**.

### 2.1 Record field update: `x.field = expr`

* **Syntax:**

  ```tw
  x.field = expr
  ```

* **Constraints:**

  * `x` must be a simple local name bound in the current scope via `let` or a prior assignment.
  * `field` must be a valid field of the record type of `x`.

* **Desugaring:**

  ```tw
  // surface:
  x.field = expr

  // core semantics:
  x = { x with field = expr }
  ```

So only the binding `x` changes; any other names that referred to the old value keep seeing the old value.

---

### 2.2 Indexed update: `arr[index] = expr`

* **Syntax:**

  ```tw
  arr[index] = expr
  ```

* **Constraints:**

  * `arr` must be a simple local name bound in the current scope.
  * `arr` must have an indexable type (e.g. `Array<T>` or `Map<K, V>`).
  * `index` is an expression of the appropriate index type.

* **Desugaring (array example):**

  ```tw
  // surface:
  arr[index] = expr

  // core semantics:
  arr = Array.set(arr, index, expr)
  ```

* `Array.set` is a pure function returning a new array value; the previous array value is unchanged.

(Same idea for maps: `Map.set` / `Map.insert`.)

---

### 2.3 Numeric compound assignment: `x += y` (optional but consistent)

* **Syntax:**

  ```tw
  x += y
  ```

* **Constraints:**

  * `x` must be a simple local name of a numeric type.

* **Desugaring:**

  ```tw
  x += y    // ->  x = x + y
  ```

Again: pure arithmetic; `+` returns a new number.

---

## 🧾 3. Function Arguments and Local Updates

### 3.1 Call-by-value, no argument mutation

* Function parameters are **ordinary local names** bound to the argument values.
* A function **cannot mutate a caller’s value**:

  * Rebinding or updating a parameter name only affects the parameter within the function body.

Example:

```tw
fn bump(n: Int) -> Int {
  n = n + 1      // allowed; n is rebound locally
  n
}

let x = 10
let y = bump(x)
// x is still 10, y is 11
```

Same for records:

```tw
fn darken(ui: Config) -> Config {
  ui.theme = "dark"   // ui = { ui with theme = "dark" }
  ui
}
```

* The caller’s `Config` is untouched; they must accept the new value explicitly:

  ```tw
  let cfg = { theme: "light" }
  let cfg2 = darken(cfg)
  ```

---

## 🧾 4. Aliasing and Value Semantics

Because values are immutable:

* `let z = x` creates a **second name for the same value**.
* Any update to `x` is really a rebinding:

  ```tw
  let x = { y: 0 }
  let z = x

  x.y = 1      // x = { x with y = 1 }

  // Now:
  //   x == { y: 1 }
  //   z == { y: 0 }
  ```

This is **legal and well-defined**:
records (and other non-primitive values) are **by value**, *not* by reference.

If you want multiple values updated, you must explicitly build them:

```tw
let config      = { theme: "light" }
let ui_config   = config
let ui_config   = { ui_config with theme = "dark" }

// config.theme    == "light"
// ui_config.theme == "dark"
```

---

## 🧾 5. Allowed vs Forbidden Patterns

### 5.1 Allowed patterns

#### (A) Local “mutation” of a record

```tw
let p = { x: 0, y: 0 }
p.x = p.x + 1
p.y = 42
// p == { x: 1, y: 42 }
```

#### (B) Deriving a variant config

```tw
let base = { theme: "light", font_size: 14 }
let ui   = foo.derive_ui_config(base)   // any pure function

ui.theme = "dark"
// base.theme == "light"
// ui.theme   == "dark"
```

#### (C) Updating array elements

```tw
let arr = [0, 0, 0]
arr[1] = 42
// arr == [0, 42, 0]
```

#### (D) Rebinding in loops

```tw
let total = 0
for n in numbers {
  total = total + n
}
```

#### (E) Updating a parameter locally and returning

```tw
fn bump_point(p: Point) -> Point {
  p.x = p.x + 1
  p
}

let p0 = { x: 0, y: 0 }
let p1 = bump_point(p0)
// p0.x == 0
// p1.x == 1
```

---

### 5.2 Patterns that are **not allowed** (by design or syntax)

#### (1) Updating arbitrary expressions (non-name LHS)

Only a **plain local name** may be updated.

```tw
foo().x = 1          // ❌ not allowed

get_config().theme = "dark"  // ❌ not allowed

(user.profile).name = "Bob"  // ❌ not allowed
```

You must instead bind to a name:

```tw
let cfg = get_config()
cfg.theme = "dark"
```

---

#### (2) Updating nested projections in one shot (if you choose to keep v1 simple)

Depending on how simple you want v1, you might **forbid complex LHS**:

```tw
user.profile.name = "Bob"   // ❌ v1: too much magic
```

and require explicit nested updates:

```tw
user =
  { user
  with profile =
      { user.profile with name = "Bob" }
  }
```

(or via a helper function).

This keeps desugaring simple and predictable.

---

#### (3) Expecting “shared object” behavior

Code that *assumes* “update through one name affects all aliases” is **not supported**:

```tw
let config    = { theme: "light" }
let ui_config = config

ui_config.theme = "dark"

// Someone *expecting* config.theme == "dark" is wrong.
// It remains "light".
```

This is not a compile error, but it is a **semantic anti-pattern**; records are values, not objects.

You can document this clearly and optionally lint for it if you find it confusing.

---

#### (4) Real in-place mutation of shared state

There is **no way** to write code where:

* two different names point to the same mutable storage, and
* changing via one name magically affects what the other sees.

Any pattern that relies on that is simply impossible in the language:

* No `Ref<T>` / pointer types in the core model.
* No “global config object that everyone mutates in place”.

If you ever want such a thing, it should be via an **explicit mutable cell** abstraction (e.g. `Cell<T>`, `Atom<T>`), not plain records/arrays.

---

## 🧾 6. Informal Summary for the Spec

You can wrap this whole thing up in a short paragraph:

> **Update semantics.**
> Twinkle uses immutable values with rebindable names.
> Assignment-like syntax (`x = e`, `x.field = e`, `arr[i] = e`, `x += e`) is sugar for “construct a new value and bind the name to it”.
> Values themselves are never mutated, and no update through one name can implicitly change what another name sees.
> Functions cannot mutate caller-visible state; they only return new values.

