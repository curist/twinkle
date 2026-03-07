# Iterator Design

Twinkle supports iteration over a closed set of built-in types (`Array`, `Range`,
`Dict`) with type-directed lowering, plus a general-purpose `Iterator<T>` type
for user-defined and streaming iteration.

`Iterator<T>` is:

* **pure** — advancement returns a new iterator value rather than mutating in place,
* **persistent** — calling `next` does not consume or invalidate the original,
* **opaque** — constructed only through `Iterator` module functions,
* **extensible** via a single primitive constructor `Iterator.unfold`.

---

## Types

`Iterator<T>` is a compiler built-in nominal type, like `Cell<T>` or `Range`.
The compiler does not expose any constructor or fields to user code.

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

`IterItem<T>` is a record (value and continuation). `UnfoldStep<T, S>` is an
enum (stop or yield-and-continue) — using an enum rather than `Option` avoids
structural record types and makes the termination signal explicit.

---

## Core API

### Stepping

```tw
pub fn Iterator.next<T>(it: Iterator<T>) Option<IterItem<T>>
```

* `.None` — iterator is exhausted.
* `.Some(item)` — `item.value` is the current element; `item.rest` is the remaining iterator.

### Construction via `unfold`

```tw
pub fn Iterator.unfold<T, S>(
  seed: S,
  step: fn(S) UnfoldStep<T, S>,
) Iterator<T>
```

Internal state starts at `seed`. Each call to `next` invokes `step(state)`:
* `.Done` — stop; iterator is exhausted.
* `.Yield(value, next_state)` — yield `value`, continue with `next_state`.

`unfold` is the universal extensibility point: any state machine of the form
`fn(S) UnfoldStep<T, S>` can become an `Iterator<T>`.

---

## Persistence and Effects

### Persistence (non-consuming)

`Iterator.next` does not consume `it`. Calling it multiple times on the same
iterator returns observationally equivalent results, assuming deterministic
evaluation.

### Exhaustion stability

Once `Iterator.next(it)` returns `.None`, all subsequent calls on `it` must
also return `.None`.

### Effects

The `step` function passed to `unfold` may perform effects. If `next` is
evaluated multiple times on the same iterator, those effects occur multiple
times. The persistence guarantee covers returned values, not suppression of
side effects.

---

## `for` Loop Lowering

When `coll` has type `Iterator<T>`:

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

`coll` is evaluated once. Advancing the iterator is local rebinding, preserving
value semantics.

---

## `collect` Lowering

`collect` uses an internal mutable builder (not user-visible) and freezes to an
immutable array at the end, guaranteeing O(n) time in the number of emitted
elements.

If `continue` is used in the body, it skips emission but the iterator still
advances — matching the spec's semantics.

---

## Resource Ownership

`Iterator<T>` does not provide automatic finalization or cleanup. If an iterator
is abandoned early (via `break`), there is no cleanup guarantee. Resource handles
must be managed by the scope that acquired them, not inside the iterator.

---

## Dict Two-Variable Loops

`for k, v in dict` keeps its dedicated type-directed lowering. It does not route
through `Iterator<Pair<K,V>>`. `Iterator<T>` is for user-defined and streaming
single-binder iteration.

---

## Examples

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
