# `Slice<T>` — one generic view to rule them all

Status: proposal. Pure **stdlib** type — no compiler or runtime changes.
Companion to [stack.md](stack.md) (the LIFO/mutation side) and
[slice-performance.md](slice-performance.md) (the audit that motivates both).

## Idea

Many hot operations don't need a new `String`/`Vector` — they need to *traverse*
or *compare* a window of an existing one. Today that's done two clumsy ways:

- `xs.slice(1, xs.len())` — allocates/copies (O(n), and O(n²) in a drop-first
  loop or head/tail recursion like `emit/match.tw`), or
- threading a raw `start` index through every function and base-case check.

A view fixes both. And in Twinkle a view need not be tied to one backing type: a
slice is just **"a length plus a way to get element *i*"** — which is exactly a
**capability record** (the language's no-traits idiom: *capabilities are passed
explicitly as records of functions*). So one generic type covers `Vector<T>`,
`String` (as `Slice<Byte>`), sub-slices of slices, even computed sequences:

```tw
pub type Slice<T> = .{ at: fn(Int) T, start: Int, len: Int }

// Constructors build the accessor once:
pub fn from_vector<T>(v: Vector<T>) Slice<T> { .{ at: fn(i: Int) { v[i] }, start: 0, len: v.len() } }
pub fn from_string(s: String) Slice<Byte>   { .{ at: fn(i: Int) { s[i] }, start: 0, len: s.len() } }

// All views are O(1) and REUSE the same `at` closure:
pub fn get<T>(sl: Slice<T>, i: Int) T       { sl.at(sl.start + i) }
pub fn first<T>(sl: Slice<T>) T?            { if sl.len == 0 { .None } else { .Some(sl.at(sl.start)) } }
pub fn is_empty<T>(sl: Slice<T>) Bool       { sl.len == 0 }
pub fn drop_first<T>(sl: Slice<T>) Slice<T> { .{ at: sl.at, start: sl.start + 1, len: sl.len - 1 } }
pub fn drop_last<T>(sl: Slice<T>) Slice<T>  { .{ at: sl.at, start: sl.start,     len: sl.len - 1 } }
pub fn sub<T>(sl: Slice<T>, a: Int, b: Int) Slice<T> { .{ at: sl.at, start: sl.start + a, len: b - a } }
// plus: fold / for-iteration / to_vector / (for Slice<Byte>) to_string
```

`drop_first`/`drop_last`/`sub` are O(1) — they only adjust `start`/`len` and
**share the one `at` closure** built at construction. So head/tail recursion over
a `Slice` is O(1) per level (O(k) total, not O(k²)), with no copy and no index
bookkeeping leaking into signatures.

## Why one type works here

The accessor-capability encoding is what makes it general: `at` absorbs the
difference between backings (trie `get`, flat-array byte read, a nested slice's
`get`, …). This is idiomatic Twinkle — no traits, no higher-kinded plumbing,
just a closure in a record. A `Vector`-only or `String`-only view would each be a
special case of this.

## The one real caveat: indirection

Every element read goes through an **indirect call** (`call_ref` on the captured
funcref) instead of a direct `array.get`:

- **`Vector`** — `get` is already O(log n); the extra indirection is noise.
- **Structural traversal** (match arms, doc parts, drop-first consumption) —
  fine; these aren't tight inner loops.
- **The lexer's innermost byte loop** — scanning megabytes byte-by-byte, a
  per-byte indirect call is a real constant-factor hit. So **keep direct `s[i]`
  there** (Tier 1 in [slice-performance.md](slice-performance.md) already does);
  `Slice` is for composable/structural use, not the tightest scan.

So `Slice` is the right tool everywhere *except* the hot byte loop, which already
uses direct indexing — meaning the indirection lands only where it doesn't
dominate.

## First consumer

`emit/match.tw` recurses with `tail := arms.slice(1, arms.len())` (O(k²) over k
arms). With a slice: `head := arms.first(); rest := arms.drop_first()` — O(k)
total, no copy, no `(arms, start)` parameter. Same shape fixes the
`fmt/printer.tw` doc-parts recursions and one-shot drop-firsts like
`segments.slice(1, …)`.

## Scope / non-goals

- **Read-only.** A `Slice` is a window for traversal/compare; you don't mutate
  through it. Building/shrinking a collection is the Stack/`drop_last` story
  ([stack.md](stack.md)).
- Not a replacement for `Vector`/`String` — an adjunct. Materialize with
  `to_vector` / `to_string` at the boundary where an owned value is needed.
- Doesn't change `concat`/arbitrary slice asymptotics — that's RRB
  ([rrb-vector-concat.md](rrb-vector-concat.md)).

## Cost contract

| Op | Cost |
|---|---|
| `from_vector` / `from_string` | O(1) (+ one closure alloc) |
| `drop_first` / `drop_last` / `sub` / `len` / `is_empty` | O(1) |
| `get` / `first` | backing cost + one indirect call (Vector O(log n); String O(1)+call) |
| `to_vector` / `to_string` | O(len) (materialize) |

## How it fits the family

- **`Slice<T>`** (this doc) — read-only views; drop-first/traversal; one generic
  type over any backing.
- **Stack / `drop_last`** ([stack.md](stack.md)) — LIFO build/shrink.
- **RRB** ([rrb-vector-concat.md](rrb-vector-concat.md)) — arbitrary O(log n)
  `concat`/`slice` on `Vector` itself.
- **Tier 1 string compares** ([slice-performance.md](slice-performance.md)) —
  direct-indexing `region_eq` in the hot byte loop (no view, no indirection).

## Open questions

- **Indirection cost**: benchmark `Slice`-based traversal vs direct indexing on a
  representative workload to confirm it's negligible outside the byte loop.
- **Accessor encoding**: a captured `at: fn(Int) T` closure (open, composes over
  any backing) vs a sum-typed backing (`{ VecSource, StrSource }`, avoids the
  indirect call but closed and awkward for `String`'s `Byte` element type). The
  closure form is more idiomatic and general; the caveat above is its price.
- **String element type**: a `String` view is `Slice<Byte>`; provide
  `Slice<Byte>` helpers (`to_string`, compare-against-`String`) so it's ergonomic
  for text scanning.
- **Naming**: `Slice<T>` vs `View<T>`; method surface (`get`/`first`/`rest`?).
- **Module path**: `@std.slice`? Prelude-visible or explicit `use`?
