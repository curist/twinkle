## Records

Twinkle has **nominal record types** and **record literals**.
Record types map directly to wasm GC `struct` types.

### 1. Named record types (nominal)

A record type is declared with:

```tw
type Point = .{ x: Int, y: Int }
```

This introduces a **new nominal type** `Point`:

* The shape is fixed and closed: fields are exactly `x: Int` and `y: Int`.

* Two record types with the same fields are still **distinct**:

  ```tw
  type Point = .{ x: Int, y: Int }
  type Vec2  = .{ x: Int, y: Int }
  // Point and Vec2 are different types, even though shapes match
  ```

* There is no record subtyping or row polymorphism in the MVP.

Field names live in the type’s namespace; they are accessed via dot on values, not globally.

---

### 2. Record value construction

Given:

```tw
type Point = .{ x: Int, y: Int }
```

You can construct a `Point` in **two equivalent ways**.

#### 2.1 Contextual anonymous literal

Use `.{ ... }` where an **expected record type** is known:

```tw
p: Point = .{ x: 1, y: 2 }

fn origin() Point {
  .{ x: 0, y: 0 }
}

fn move(p: Point) Point {
  .{ x: p.x + 1, y: p.y }
}

fn main() {
  move(.{ x: 1, y: 2 })  // expected param type is Point
}
```

Rules:

* The literal `.{ x: expr1, y: expr2 }` is checked **against the expected record type** `Point`.
* It is valid iff:

  * `Point` has fields `x` and `y` (no missing/extra fields), and
  * `expr1` has type `Int`, `expr2` has type `Int` (compatible with field types).
* If the expected type is not a record, or fields/types don’t match, it is a type error.

Anonymous record literals **do not introduce new structural types**. They are just a convenient literal form when you already know which record type is expected.

#### 2.2 Named constructor form

You can also use a **named constructor syntax** that always produces a specific record type:

```tw
p := Point.{ x: 1, y: 2 }
q := Point.{ x: 1 + 10, y: 2 + 20 }

fn origin() Point {
  Point.{ x: 0, y: 0 }
}

fn main() {
  move(Point.{ x: 1, y: 2 })
}
```

Rules:

* `Point.{ field: expr, ... }` always has type `Point`.
* The compiler checks that:

  * the fields exist on `Point`,
  * the field types match the declared field types.
* This form does **not** depend on contextual type information.

#### 2.3 Equivalence

These two are equivalent:

```tw
p: Point = .{ x: 1, y: 2 }
p2 := Point.{ x: 1, y: 2 }
```

Both produce a `Point` value; only the syntax differs.

---

### 3. Field access

Field access works the same regardless of how the value was constructed:

```tw
p: Point = .{ x: 1, y: 2 }

x_coord := p.x
y_coord := p.y
```

Rules:

* `p.x` is valid iff the static type of `p` is a record type with field `x`.
* Access to unknown fields is a compile-time error.

---

### 4. Where anonymous `.{ ... }` is allowed

Anonymous record literals are only allowed in **contexts with an expected record type**, such as:

1. **Annotated bindings**:

```tw
   p: Point = .{ x: 1, y: 2 }
   ```

2. **Function arguments**, when parameter type is a record:

   ```tw
   fn move(p: Point) Point { ... }

   move(.{ x: 1, y: 2 })
   ```

3. **Return expressions**, when function returns a record:

   ```tw
   fn origin() Point {
     .{ x: 0, y: 0 }
   }
   ```

4. **Record fields whose type is a record**:

   ```tw
   type Box = .{
     p: Point,
     name: String,
   }

   b: Box = .{
     p: .{ x: 1, y: 2 },   // expected type for p is Point
     name: "hello",
   }
   ```

In all these cases, the literal `.{ ... }` is checked against the known record type from context.

---

### 5. Where `.{ ... }` is *not* allowed

Anonymous literals are **not** allowed when there is no expected record type:

```tw
p := .{ x: 1, y: 2 }    // ❌ no expected type → error in MVP
```

The compiler will not invent structural record types. If you want a record value there, either:

```tw
p: Point = .{ x: 1, y: 2 }
```

or:

```tw
p := Point.{ x: 1, y: 2 }
```

Similarly, anonymous record **types** are not allowed in type positions in the MVP:

```tw
fn f(p: .{ x: Int, y: Int }) Int { ... }  // ❌ not allowed
```

You must declare a named type:

```tw
type Point = .{ x: Int, y: Int }

fn f(p: Point) Int { ... }                // ✅
```

---

### 6. Ambiguity and multiple candidate types

If the expected type is ambiguous (more than one candidate record type fits), the compiler reports an error.

Example:

```tw
type Point = .{ x: Int, y: Int }
type Vec2  = .{ x: Int, y: Int }

fn use_point(p: Point) Void { ... }
fn use_vec(v: Vec2)  Void { ... }

fn main() {
  v: Vec2 = .{ x: 1, y: 2 }        // ✅ explicit annotation: Vec2

  use_point(.{ x: 1, y: 2 })          // ✅ expected type = Point
  use_vec(.{ x: 1, y: 2 })            // ✅ expected type = Vec2

  // But this is ambiguous:
  fn id_record(r) { r }               // ❌ not allowed in MVP (no type)
}
```

In practice, you only get ambiguity if you write code with missing type information. The MVP keeps things simple by requiring expected record types for `.{ ... }`.

---

### 7. Interaction with dot methods

* Dot method resolution **does not** depend on how the record was constructed; it uses the static record type (`Point`, `Vec2`, etc.).
* Whether you constructed a `Point` via `.{ ... }` or `Point.{ ... }` makes no difference for method resolution.
* Twinkle has no traits; capabilities are explicit records of functions (see **Capabilities via Records of Functions** in the spec).

---

## Good vs Bad Examples

### ✅ Good: annotated binding

```tw
type Point = .{ x: Int, y: Int }

fn demo() {
  p: Point = .{ x: 1, y: 2 }
  println("p = (${p.x}, ${p.y})")
}
```

### ✅ Good: constructor form

```tw
type Point = .{ x: Int, y: Int }

fn demo() {
  p := Point.{ x: 1, y: 2 }
  println("p = (${p.x}, ${p.y})")
}
```

### ✅ Good: call-site literal (contextual)

```tw
type Point = .{ x: Int, y: Int }

fn move(p: Point) Point {
  .{ x: p.x + 1, y: p.y }
}

fn main() {
  move(.{ x: 1, y: 2 })
  move(Point.{ x: 10, y: 20 })
}
```

### ✅ Good: literal in record field

```tw
type Point = .{ x: Int, y: Int }
type Box = .{ p: Point, label: String }

fn make_box() Box {
  .{
    p: .{ x: 1, y: 2 },     // expected type for p is Point
    label: "hello",
  }
}
```

---

### ❌ Bad: no expected type

```tw
type Point = .{ x: Int, y: Int }

fn demo() {
  p := .{ x: 1, y: 2 }   // ERROR: no expected record type
}
```

Fix:

```tw
fn demo() {
  p: Point = .{ x: 1, y: 2 }
  // or:
  p := Point.{ x: 1, y: 2 }
}
```

---

### ❌ Bad: wrong shape

```tw
type Point = .{ x: Int, y: Int }

fn demo() {
  p: Point = .{ x: 1 }       // ERROR: missing field y
  q: Point = .{ x: 1, y: 2, z: 3 }  // ERROR: extra field z
}
```

---

### ❌ Bad: wrong field type

```tw
type Point = .{ x: Int, y: Int }

fn demo() {
  p: Point = .{ x: "1", y: 2 }  // ERROR: x expects Int, got String
}
```

---

### ❌ Bad: anonymous record type in signature (MVP)

```tw
fn translate(p: .{ x: Int, y: Int }) .{ x: Int, y: Int } { ... }  // ERROR in MVP
```

Fix:

```tw
type Point = .{ x: Int, y: Int }

fn translate(p: Point) Point { ... }                             // ✅
```

