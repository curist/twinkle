# Twinkle Iterator Design

## 1. Overview

Twinkle supports iteration in `for` loops and `collect` comprehensions over a closed set of built-ins (`Array`, `Range`, `Dict`) with dedicated type-directed lowering, plus a general-purpose **`Iterator<T>`** type for user-defined and streaming iteration.

`Iterator<T>` is:

* **pure** — advancement returns a new iterator value rather than mutating in place,
* **persistent** — calling `next` does not consume or invalidate the original iterator,
* **opaque** — representation is hidden; constructed only through `Iterator` module functions,
* **extensible** via a single primitive constructor `Iterator.unfold`.

---

## 2. Types

`Iterator<T>` is a compiler built-in nominal type, like `Cell<T>` or `Range`. There is no `opaque type` keyword — the compiler simply does not expose any constructor or field for `Iterator<T>` to user code.

Two companion prelude types support the API:

```tw
// Result of stepping an iterator: a value and the remainder
type IterItem<T> = .{
  value: T,
  rest: Iterator<T>,
}

// Control signal returned by an unfold step function
type UnfoldStep<T, S> = {
  Done,
  Yield(T, S),
}
```

`IterItem<T>` is a record (fields are named and meaningful — value and continuation).

`UnfoldStep<T, S>` is an enum (encodes a control decision — stop or yield-and-continue). Using an enum rather than `Option<{value, state}>` avoids structural record types and makes the termination signal explicit.

---

## 3. Core API

### 3.1 Stepping

```tw
pub fn Iterator.next<T>(it: Iterator<T>) Option<IterItem<T>>
```

Semantics:

* `.None` — iterator is exhausted.
* `.Some(item)` — `item.value` is the current element; `item.rest` is the remaining iterator.

### 3.2 Construction via `unfold`

```tw
pub fn Iterator.unfold<T, S>(
  seed: S,
  step: fn(S) UnfoldStep<T, S>,
) Iterator<T>
```

Semantics:

* Internal state starts at `seed`.
* Each call to `Iterator.next` invokes `step(state)`:
  * `.Done` — stop; iterator is exhausted.
  * `.Yield(value, next_state)` — yield `value`, continue with `next_state`.

`unfold` is the universal user-extensibility point: any state machine of the form `fn(S) UnfoldStep<T, S>` can become an `Iterator<T>`.

---

## 4. Persistence and Effect Semantics

### 4.1 Persistence (non-consuming)

`Iterator.next` does not consume `it`. Calling `Iterator.next(it)` multiple times on the same iterator must return observationally equivalent results (same `value` and structurally equivalent `rest`), assuming deterministic evaluation.

### 4.2 Exhaustion stability

Once exhausted:

```tw
Iterator.next(it) == .None
```

all subsequent calls to `Iterator.next(it)` must also return `.None`.

### 4.3 Effects

The `step` function passed to `unfold` may perform effects (e.g. `println`, `Cell` updates, host I/O calls). If `Iterator.next(it)` is evaluated multiple times on the same iterator value, those effects occur multiple times.

The persistence guarantee covers the **values** returned (`value` / `rest`), not the suppression of side effects.

Recommended practice: keep iterator step functions effect-free unless explicitly modelling an effectful stream.

---

## 5. `for` Loop Lowering

When `coll` has type `Iterator<T>`, the loop:

```tw
for x in coll { body }
```

is conceptually lowered as:

```tw
loop_it := coll
for true {
  step := Iterator.next(loop_it)
  case step {
    .None => break,
    .Some(r) => {
      x := r.value
      loop_it = r.rest
      body
    },
  }
}
```

`coll` is evaluated once. Advancing the iterator is expressed as local rebinding (`loop_it = r.rest`), which preserves the pure value semantics of the language.

---

## 6. `collect` Lowering (Builder-backed)

To guarantee linear-time collection, `collect` uses an internal mutable builder (not user-visible) and freezes to an immutable array at the end.

For:

```tw
xs := collect x in coll { expr }
```

when `coll : Iterator<T>`, the conceptual lowering is:

```tw
__builder := array_builder_new()
loop_it := coll
for true {
  step := Iterator.next(loop_it)
  case step {
    .None => break,
    .Some(r) => {
      x := r.value
      loop_it = r.rest
      array_builder_push(__builder, expr)
    },
  }
}
array_builder_freeze(__builder)
```

`array_builder_new`, `array_builder_push`, and `array_builder_freeze` are compiler-internal intrinsics — same category as `dict_get_unsafe`. They are assigned FuncIds, implemented in the interpreter's `call_builtin`, and emitted directly by the lowerer. They are never registered in the user-visible type environment and cannot be called by user code.

If `expr` uses `continue`, it skips `array_builder_push` for that iteration but `loop_it = r.rest` has already executed, so the iterator still advances. This matches the `collect` semantics from spec §13: `continue` skips emission but does not stall the source.

Normative requirement: `collect` must be **O(n)** in the number of emitted elements (amortized).

---

## 7. Resource Ownership

`Iterator<T>` does not provide automatic finalization or resource cleanup.

If an iterator is abandoned early (e.g. via `break`), there is no guarantee of cleanup actions. Iterators must not be the sole ownership mechanism for external resources. Resource handles (e.g. `File`) must be managed by the scope that acquired them; explicit close or host-managed lifetime policies belong there, not inside the iterator.

This keeps iterator semantics simple and pure.

---

## 8. Interaction with Dict Two-Variable Loops

The `for k, v in dict` form keeps its dedicated type-directed lowering. It does not route through `Iterator<Pair<K,V>>`. `Iterator<T>` is for user-defined and streaming single-binder iteration.

---

## 9. FuncId and TypeId Allocation

### TypeIds (built-in types, in order)

| TypeId | Type |
|--------|------|
| 0 | `Option<T>` |
| 1 | `Result<T, E>` |
| 2 | `Cell<T>` |
| 3 | `Range` |
| 4 | `Iterator<T>` |
| 5 | `IterItem<T>` |
| 6 | `UnfoldStep<T, S>` |

User-defined types start at TypeId(7).

### FuncIds (Iterator-related)

| FuncId | Name | User-visible? |
|--------|------|---------------|
| 31 | `Iterator.next` | ✅ module call |
| 32 | `Iterator.unfold` | ✅ module call |
| 33 | `array_builder_new` | ❌ internal (like `dict_get_unsafe`) |
| 34 | `array_builder_push` | ❌ internal |
| 35 | `array_builder_freeze` | ❌ internal |

`USER_FUNC_START` shifts to FuncId(36).

---

## 10. Examples

### Custom range iterator

```tw
fn my_range(n: Int) Iterator<Int> {
  Iterator.unfold(0, fn(i: Int) {
    if i < n { .Yield(i, i + 1) }
    else     { .Done }
  })
}

sum := 0
for i in my_range(10) {
  sum = sum + i
}
println("sum 0..9 = ${sum}")  // 45
```

### String character iterator

```tw
fn chars(s: String) Iterator<Int> {
  n := s.len()
  Iterator.unfold(0, fn(i: Int) {
    if i < n {
      .Yield(String.char_at(s, i), i + 1)
    } else {
      .Done
    }
  })
}

for cp in chars("twinkle") {
  println("codepoint=${cp}")
}
```

### File line iterator (pure, offset-based)

Assumes host primitives (named types, since Twinkle has nominal records):

```tw
type ReadLineResult = .{ line: String, next_offset: Int }

pub fn File.read_line_at(file: File, offset: Int) Option<ReadLineResult>
```

```tw
fn lines(f: File) Iterator<String> {
  Iterator.unfold(0, fn(off: Int) {
    case File.read_line_at(f, off) {
      .None => .Done,
      .Some(r) => .Yield(r.line, r.next_offset),
    }
  })
}

case File.open("notes.txt") {
  .Err(e) => error(e),
  .Ok(f) => {
    for line in lines(f) {
      println("line=${line}")
    }
    // File.close(f) here if the runtime exposes it
  },
}
```
