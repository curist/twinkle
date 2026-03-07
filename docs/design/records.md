# Record Types

Twinkle has **nominal record types** that map directly to Wasm GC `struct` types.
This document covers declaration, construction, field access, and the rules
around anonymous record literals.

---

## Declaration

A record type is declared with:

```tw
type Point = .{ x: Int, y: Int }
```

This introduces a new nominal type `Point`:

* The shape is fixed and closed: fields are exactly `x: Int` and `y: Int`.
* Two record types with the same fields are still distinct:

  ```tw
  type Point = .{ x: Int, y: Int }
  type Vec2  = .{ x: Int, y: Int }
  // Point and Vec2 are different types, even though shapes match
  ```

* There is no record subtyping or row polymorphism in the MVP.

Field names live in the type's namespace; they are accessed via dot on values, not globally.

---

## Construction

Given `type Point = .{ x: Int, y: Int }`, a `Point` can be constructed in two
equivalent ways.

### Anonymous literal (contextual)

Use `.{ ... }` where an expected record type is known:

```tw
p: Point = .{ x: 1, y: 2 }

fn origin() Point {
  .{ x: 0, y: 0 }
}

move(.{ x: 1, y: 2 })  // expected param type is Point
```

The literal is checked against the expected record type. All fields must be
present, with no extras, and field types must match. Anonymous literals do not
introduce structural types — they are a convenience when the target type is
already known.

### Named constructor

Use `Type.{ ... }` when context doesn't provide an expected type:

```tw
p := Point.{ x: 1, y: 2 }
```

This form always produces the named type and does not depend on contextual type
information.

### Equivalence

These two are equivalent:

```tw
p: Point = .{ x: 1, y: 2 }
p2 := Point.{ x: 1, y: 2 }
```

Both produce a `Point` value; only the syntax differs.

---

## Field Access

```tw
p: Point = .{ x: 1, y: 2 }
x_coord := p.x
y_coord := p.y
```

`p.x` is valid iff the static type of `p` is a record type with field `x`.
Access to unknown fields is a compile-time error.

---

## Where Anonymous Literals Are Allowed

Anonymous `.{ ... }` literals require an expected record type from context:

1. **Annotated bindings:** `p: Point = .{ x: 1, y: 2 }`
2. **Function arguments:** `move(.{ x: 1, y: 2 })` when the parameter type is a record
3. **Return expressions:** when the function returns a record type
4. **Record fields:** when a field's declared type is a record

They are **not** allowed when there is no expected type:

```tw
p := .{ x: 1, y: 2 }    // error: no expected record type
```

The compiler does not invent structural record types. Use an annotation or the
named constructor form instead.

Anonymous record types in signatures are also not allowed in MVP:

```tw
fn f(p: .{ x: Int, y: Int }) Int { ... }  // error
```

---

## Ambiguity

If multiple record types could match an anonymous literal, the compiler reports
an error. In practice this only happens with missing type information — the MVP
requires expected record types for `.{ ... }`.

---

## Interaction with Dot Methods

Dot method resolution uses the static record type, regardless of how the value
was constructed. Whether you used `.{ ... }` or `Point.{ ... }` makes no
difference.
