# Collection ergonomics — shared `Vector` and `View` helpers

Status: proposal.

## Goal

Make common scripting/data-processing collection pipelines pleasant without adding
new syntax or changing the ownership model:

- `Vector<T>` remains the owned persistent collection API.
- `View<C>` becomes the main read-only sequence/window API over any `IndexRead`
  backing (`Vector`, `String`, or another `View`).
- `Vector` exposes convenience methods for operations users naturally expect on
  vectors, forwarding to `View` when the operation is fundamentally window-based.

This keeps zero-copy contiguous views available where they matter while avoiding
ceremony for everyday code.

## API shape

### Existing APIs to keep as-is

`Vector` already has:

```tw
xs.map(fn(x) { ... })
xs.filter(fn(x) { ... })
xs.fold(init, fn(acc, x) { ... })
xs.flat_map(fn(x) { ... })
xs.find(fn(x) { ... })
xs.any(fn(x) { ... })
xs.all(fn(x) { ... })
xs.position(fn(x) { ... })
```

`View` already has:

```tw
v.sub(a, b)
v.drop_first()
v.drop_last()
v.to_vector()
v.fold(init, fn(acc, x) { ... })
```

### Add to both `Vector` and `View`

These methods are common, low-surprise names across collection libraries and do
not require tuple/record return types.

```tw
.take(n)              // first n elements
.drop(n)              // skip first n elements
.take_while(pred)     // prefix while pred is true
.drop_while(pred)     // suffix after the true prefix
.find_map(f)          // first Some(...) produced by f
.count_where(pred)    // count elements satisfying pred
.zip_with(other, f)   // combine two sequences without materializing Pair values
```

Return types:

```tw
Vector.take<T>(xs: Vector<T>, n: Int) Vector<T>
Vector.drop<T>(xs: Vector<T>, n: Int) Vector<T>
Vector.take_while<T>(xs: Vector<T>, pred: fn(T) Bool) Vector<T>
Vector.drop_while<T>(xs: Vector<T>, pred: fn(T) Bool) Vector<T>
Vector.find_map<T, U>(xs: Vector<T>, f: fn(T) Option<U>) Option<U>
Vector.count_where<T>(xs: Vector<T>, pred: fn(T) Bool) Int
Vector.zip_with<A, B, C>(a: Vector<A>, b: Vector<B>, f: fn(A, B) C) Vector<C>

View.take<C>(v: View<C>, n: Int) View<C>
View.drop<C>(v: View<C>, n: Int) View<C>
View.take_while<C: IndexRead<E>, E>(v: View<C>, pred: fn(E) Bool) View<C>
View.drop_while<C: IndexRead<E>, E>(v: View<C>, pred: fn(E) Bool) View<C>
View.find_map<C: IndexRead<E>, E, U>(v: View<C>, f: fn(E) Option<U>) Option<U>
View.count_where<C: IndexRead<E>, E>(v: View<C>, pred: fn(E) Bool) Int
View.zip_with<A: IndexRead<EA>, EA, B: IndexRead<EB>, EB, C>(
  a: View<A>,
  b: View<B>,
  f: fn(EA, EB) C,
) Vector<C>
```

Notes:

- `View.take` / `View.drop` return `View<C>` because they preserve contiguity.
- `Vector.take` / `Vector.drop` can use `slice` directly or forward through
  `view.from(xs).take(n).to_vector()` depending on which is clearer/faster.
- `take` / `drop` should be total: clamp negative counts to `0` and counts past
  length to the collection length. This matches `View.sub`'s forgiving behavior.
- `zip_with` stops at the shorter input. It avoids adding an `enumerate`/`zip`
  API that would force `Pair` into the common path.

### Add primarily to `View`, with `Vector` conveniences

Window-oriented operations belong naturally on `View` because each result can be a
zero-copy sub-window.

```tw
v.chunks(size)         // non-overlapping contiguous windows
v.windows(size)        // sliding contiguous windows
```

Return types:

```tw
View.chunks<C>(v: View<C>, size: Int) Vector<View<C>>
View.windows<C>(v: View<C>, size: Int) Vector<View<C>>

Vector.chunks<T>(xs: Vector<T>, size: Int) Vector<View<Vector<T>>>
Vector.windows<T>(xs: Vector<T>, size: Int) Vector<View<Vector<T>>>
```

Semantics:

- `chunks(size)` traps when `size <= 0`; otherwise returns non-overlapping views.
  The final chunk may be shorter.
- `windows(size)` traps when `size <= 0`; returns every length-`size` contiguous
  window. If `size > len`, returns an empty vector.
- `Vector` implementations should forward through `view.from(xs)` so the chunks
  and windows share the original vector rather than copying elements.

### Add to `View` for parity with `Vector`

`View` should also grow the read-only materializing helpers that already exist on
`Vector`:

```tw
v.map(fn(x) { ... })       // Vector<U>
v.filter(fn(x) { ... })    // Vector<E>
v.find(fn(x) { ... })      // Option<E>
v.any(fn(x) { ... })       // Bool
v.all(fn(x) { ... })       // Bool
v.position(fn(x) { ... })  // Option<Int>
v.flat_map(fn(x) { ... })  // Vector<U>
```

`map`, `filter`, and `flat_map` materialize because they either change element
shape or may produce non-contiguous results. `find`, `any`, `all`, and `position`
short-circuit over the view without materializing.

### Add to `Vector` only

These are owned-collection helpers and are less natural for `View` because they
materialize by definition or depend on equality/set membership.

```tw
.compact()             // Vector<Option<T>> -> Vector<T>
.dedup()               // remove adjacent duplicates
.unique()              // preserve first occurrence, using Set-compatible keys
.intersperse(sep)      // insert sep between elements
```

Candidate signatures:

```tw
Vector.compact<T>(xs: Vector<Option<T>>) Vector<T>
Vector.dedup<T: Eq>(xs: Vector<T>) Vector<T>
Vector.unique<T>(xs: Vector<T>) Vector<T>        // T must be Set-compatible
Vector.intersperse<T>(xs: Vector<T>, sep: T) Vector<T>
```

`unique` needs the same key constraints as `Set`/`Dict` (`Int`, `String`, or
`Byte`) unless/until `Set` supports arbitrary `Eq` values. If that constraint is
awkward to express in the current type system, defer `unique`.

## Non-goals

- Do not add `enumerate`. The language already supports `for x, i in xs`, and a
  materialized `enumerate` return would pull `Pair` or an ad-hoc record into the
  common collection API.
- Do not add `zip` returning pairs in the first pass. Prefer `zip_with` to keep
  the API tuple-free.
- Do not make `filter` return a `View`; filtered elements are not guaranteed to
  be contiguous.
- Do not make `View` mutable or writable. It remains a read-only access window.
- Do not add lazy iterator adapters here. `Iterator` can grow its own adapters,
  but this plan focuses on eager `Vector` results and zero-copy `View` windows.

## Implementation plan

1. Extend `boot/stdlib/view.tw` with the `View` helpers. Prefer simple loops over
   abstraction-heavy implementations so monomorphized code stays predictable.
2. Extend `boot/prelude/vector.tw` with vector conveniences. Reuse existing
   vector primitives (`slice`, `append`, `len`, indexing) and forward to
   `@std.view` only where the result type is explicitly `View`-based
   (`chunks`, `windows`).
3. Add API documentation to `docs/API.md` under `Vector<T>` and `@std.view`.
4. Add boot tests covering edge behavior:
   - `take` / `drop` clamp behavior
   - `take_while` / `drop_while` prefix behavior
   - `find_map` short-circuit behavior
   - `zip_with` shorter-input behavior
   - `chunks` final short chunk and invalid size trap
   - `windows` normal, too-large, and invalid size behavior
   - `compact`, `dedup`, and `intersperse` owned-vector behavior
5. Run the formatter on changed `.tw` files.
6. Run `make boot-test`; if prelude/bootstrap changes require it, rebuild the
   bundled CLI through the normal self-host workflow.

## Open questions

- Should `count_where` instead be named `count` with a predicate? `count_where`
  is explicit and avoids confusion with `String.count(needle)`, but `count` is
  common in some collection libraries.
- Should invalid `chunks(0)` / `windows(0)` trap, or return an empty vector?
  Trapping catches bugs and avoids silently accepting nonsensical window sizes.
- Should `take_while` / `drop_while` on `Vector` return `Vector` directly or a
  `View<Vector<T>>`? The ergonomic answer is `Vector`; users who want zero-copy
  can call `view.from(xs).take_while(...)`.
- Can `unique` express the key constraint cleanly today? If not, defer it rather
  than weakening type checking or broadening `Set` semantics as part of this
  plan.
