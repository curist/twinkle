# Collection Literal Type Inference

## Context

Writing a JSON decoder library in Twinkle (`poc/json/decoder.tw`) surfaced two
related stage0 type inference gaps. Both prevent natural idiomatic code from
compiling when the collection type cannot be resolved at the point of creation:

**1. `Dict.new()` requires an explicit annotation**

```tw
// stage0: error: Unsupported feature: Dict.new() without type annotation
d := Dict.new()
d["key"] = value
```

Must be written as:

```tw
d: Dict<String, Json> = Dict.new()
d["key"] = value
```

**2. Empty vector literals `[]` without a locally resolvable type**

```tw
// stage0: error: Empty vectors require type annotations (not yet supported)
pub fn list<A>(da: Decoder<A>) Decoder<Vector<A>> {
  make(fn(json: Json) {
    // ...
    out := []          // A is in scope from outer fn, but not visible to checker here
    out = out.append(a)
    .Ok(out)
  })
}
```

The boot compiler accepts both forms; the stage0 Rust compiler does not.

Both gaps share the same root cause: the stage0 type inferencer does not
propagate expected types forward from downstream usage ("push" inference), and
does not thread outer-function type parameters into closure bodies when resolving
literal types. Both are standard capabilities of constraint-based HM inference.

---

## Goal

Make `Dict.new()` and `[]` type-check without explicit annotations in all cases
where the type is determinable from context — downstream assignments, function
return types, or outer type parameters visible to the current scope.

---

## Scope

In scope:

- `Dict.new()` type inference from usage context
- Empty vector literal `[]` type inference from usage context
- Type parameters from enclosing generic functions visible inside closures

Out of scope:

- Inference for non-empty literals (those work already)
- Any change to user-visible syntax
- Non-collection inference gaps (tracked separately if found)

---

## Root Causes

### 1. Zero-argument generic functions (`Dict.new()`)

`Dict.new<K, V>()` has no arguments from which to infer `K` and `V`. The
stage0 checker currently errors immediately if it cannot resolve type variables
from the call-site arguments alone. It does not attempt to unify against the
declared type of the binding or against downstream usage of the result.

The correct behavior: emit a fresh metavariable pair for `K` and `V`, then
unify against subsequent uses of the bound variable (assignments, field access,
function arguments). If still unresolved after the binding's scope is exhausted,
report an ambiguity error.

### 2. Empty vector literal `[]`

The stage0 checker has an explicit unsupported-feature guard on empty vector
literals when the element type cannot be determined immediately. This is
documented by the error message "Empty vectors require type annotations (not
yet supported)".

A secondary variant of this problem occurs inside closures of generic functions:
even if the enclosing function has `<A>` in scope, the stage0 checker does not
propagate `A` into the closure's local inference context. So even a type
annotation `out: Vector<A> = []` fails, because `A` is treated as an undefined
type inside the closure.

Both require the same fix: unify the empty literal's element type metavariable
against later usage within the same scope, and extend type-parameter visibility
into closure bodies.

---

## Milestones

### M1 — `Dict.new()` without annotation

Allow `d := Dict.new()` when type can be resolved from downstream usage.

**Acceptance:**

```tw
d := Dict.new()
d["x"] = 1          // checker infers Dict<String, Int>
```

```tw
fn make_obj() Dict<String, Int> {
  d := Dict.new()   // inferred from return type
  d["x"] = 1
  d
}
```

Both should typecheck without annotation.

Likely files:

- `src/types/infer.rs` or equivalent checker call-site unification
- The `Dict::new` call resolution in `src/module/typecheck.rs`

### M2 — Empty vector literal `[]` from usage

Allow `xs := []` when the element type can be resolved from subsequent usage.

**Acceptance:**

```tw
xs := []
xs = xs.append(42)  // checker infers Vector<Int>
```

```tw
fn empty_strings() Vector<String> {
  xs := []          // inferred from return type
  xs
}
```

Likely files:

- The empty-literal special case in `src/types/infer.rs` (the current
  "not yet supported" guard)

### M3 — Type parameters visible inside closures

Outer generic function type parameters should be in scope when typechecking
closure bodies, including for literal annotations.

**Acceptance:**

```tw
pub fn list<A>(da: Decoder<A>) Decoder<Vector<A>> {
  make(fn(json: Json) {
    out := []             // A visible from outer fn; infers Vector<A>
    out = out.append(a)   // a: A, consistent
    .Ok(out)
  })
}
```

Alternatively, this becomes unnecessary if M2 succeeds: if empty literal type
is inferred purely from usage, the annotation `out: Vector<A>` is not needed at
all and the closure need not see outer type parameters.

Likely files:

- `src/types/infer.rs`: closure type environment construction
- `src/module/typecheck.rs`: type parameter scoping across closure boundaries

---

## Implementation Notes

The preferred fix path for M1 and M2 is the same: instead of immediately
failing when a type cannot be resolved at the point of creation, introduce a
fresh metavariable and defer resolution until the binding's scope is exhausted.
This is the standard HM approach and matches how the boot compiler already
behaves.

M3 may become a no-op if M2 lands cleanly: a user writing idiomatic code would
use `xs := []` (no annotation), which resolves purely from downstream usage
without needing the outer type parameter in scope. The annotation form
`xs: Vector<A> = []` is a separate question about whether type annotations in
let-bindings inside closures can reference outer type params — that is a
legitimate but lower-priority fix.

Preferred resolution order: **M1 → M2 → M3 only if still needed after M2**.

---

## Test Cases

### Dict inference

```tw
// no annotation — type from assignment
d := Dict.new()
d["a"] = 1
d["b"] = 2
assert d.len() == 2

// no annotation — type from function argument
fn takes_dict(d: Dict<String, Bool>) Bool { d.has("x") }
d := Dict.new()
d["x"] = true
takes_dict(d)

// no annotation — type from return position
fn make_dict() Dict<String, String> {
  d := Dict.new()
  d["hello"] = "world"
  d
}
```

### Empty vector inference

```tw
// type from append
xs := []
xs = xs.append("hello")
xs = xs.append("world")
assert xs.len() == 2

// type from return position
fn empty_ints() Vector<Int> {
  xs := []
  xs
}

// type from function argument
fn takes_strings(v: Vector<String>) Int { v.len() }
xs := []
xs = xs.append("a")
takes_strings(xs)
```

### Closure + generic function (M3 / fallback after M2)

```tw
// If M2 lands, this should work via usage inference alone,
// without needing A visible in the closure:
pub fn list<A>(da: Decoder<A>) Decoder<Vector<A>> {
  make(fn(json: Json) {
    case json {
      .Array(arr) => {
        out := []
        for item in arr {
          case da.run(item) {
            .Ok(a)  => out = out.append(a),
            .Err(e) => { return .Err(e) },
          }
        }
        .Ok(out)
      },
      _ => .Err("expected array"),
    }
  })
}
```

---

## Exit Criteria

- `Dict.new()` without annotation compiles when the type is determinable from
  downstream usage
- `[]` without annotation compiles when the element type is determinable from
  downstream usage or return position
- The workarounds in `poc/json/decoder.tw` and `poc/json/main.tw` are removed
  and replaced with the natural idiomatic forms (see Current Workarounds below)
- All existing type-inference tests continue to pass
- A targeted test file covers the cases above

---

## Current Workarounds

Until this plan is implemented, the pragmatic workarounds are:

- `Dict.new()` → `d: Dict<K, V> = Dict.new()` (explicit annotation on the binding)
- `[]` inside generic closure → use `collect item in xs { f(item) }` instead
  of a mutable accumulator, so the element type is always established by the
  first element in the collect body rather than inferred from a later `append`

These workarounds compile on both stage0 and the boot compiler.

**Once the fix lands, remove these workarounds:**

In `poc/json/decoder.tw`, the `list` decoder currently uses a two-pass
`collect` to avoid the mutable accumulator. Replace it with the simpler form:

```tw
pub fn list<A>(da: Decoder<A>) Decoder<Vector<A>> {
  make(fn(json: Json) {
    case json {
      .Array(arr) => {
        out := []
        i := 0
        for i < arr.len() {
          case da.run(arr[i]) {
            .Ok(a)  => out = out.append(a),
            .Err(e) => { return .Err("index ${i}: ${e}") },
          }
          i = i + 1
        }
        .Ok(out)
      },
      _ => .Err("expected array"),
    }
  })
}
```

In `poc/json/main.tw`, all `d: Dict<String, Json> = Dict.new()` bindings
should be simplified back to `d := Dict.new()`.
