# `View<C>` — zero-copy windows over any indexable backing

Status: proposal. Pure **stdlib** type — no runtime change. Companion to
[access-contracts.md](access-contracts.md) (the general bound it satisfies),
[stack.md](stack.md) (the LIFO/build side), and
[slice-performance.md](slice-performance.md) (the audit that motivates both).

## Idea

Many hot operations don't need a new `String`/`Vector` — they need to *traverse*
or *compare* a window of an existing one. Today that's done two clumsy ways:

- `xs.slice(1, xs.len())` — allocates/copies (O(n), and O(n²) in a drop-first
  loop or head/tail recursion like `emit/match.tw`), or
- threading a raw `start` index through every function and base-case check.

A view fixes both. A `View` is just a backing plus `start`/`len`, and it reaches
elements through the **`IndexRead` access contract**
([access-contracts.md](access-contracts.md)) rather than a captured closure:

```tw
pub type View<C> = .{ source: C, start: Int, len: Int }   // window into source

pub fn from<C: IndexRead<E>, E>(c: C) View<C> { .{ source: c, start: 0, len: c.len() } }

// element reads delegate through the IndexRead contract — direct after monomorphization:
pub fn get<C: IndexRead<E>, E>(v: View<C>, i: Int) E { v.source.get(v.start + i) }
pub fn first<C: IndexRead<E>, E>(v: View<C>) E?       { if v.len == 0 { .None } else { .Some(v.get(0)) } }
pub fn len<C>(v: View<C>) Int        { v.len }
pub fn is_empty<C>(v: View<C>) Bool  { v.len == 0 }

// window ops are O(1) — they only adjust start/len and SHARE the same source.
// Total: on an empty view (len == 0) they return the empty view (len clamps at 0):
pub fn drop_first<C>(v: View<C>) View<C> { if v.len == 0 { return v }  v.start = v.start + 1  v.len = v.len - 1  v }
pub fn drop_last<C>(v: View<C>) View<C>  { if v.len == 0 { return v }  v.len = v.len - 1  v }
pub fn sub<C>(v: View<C>, a: Int, b: Int) View<C> { v.start = v.start + a  v.len = b - a  v }
// plus: fold / for-iteration / to_vector / (for View<String>) to_string
```

**Settled names**: `drop_first` / `drop_last` (paired with `first` / `last` peeks),
matching the `Vector` ops in [stack.md](stack.md). Both are **total** — an empty
view drops to the empty view, never a trap (consistent with `Vector.drop_last`).

The element type `E` is **never stored in `View`** — it's recovered at every
method from `source`'s `IndexRead<E>` via the functional dependency
([access-contracts.md](access-contracts.md)). So `View` needs only the backing
parameter `C`; `E` follows. That's exactly what the parameterized-contract
extension buys.

`drop_first` / `drop_last` / `sub` are O(1) — they adjust `start`/`len` and share
the one `source`. Head/tail recursion over a `View` is O(1) per level (O(k) total,
not O(k²)), with no copy and no index bookkeeping leaking into signatures.

## No indirection — that's the point of using a contract

Unlike a closure-capability view (`at: fn(Int) T`), `View.get` calls
`source.get`, which is an **inherent method resolved statically and monomorphized
to a direct read**:

- **`View` over `String`** — `source.get(i)` is the O(1) direct byte read; the
  window adds only an integer add. No indirect call, no closure allocation.
- **`View` over `Vector<T>`** — `source.get(i)` is the same O(log n) trie read you'd
  pay indexing the `Vector` directly. The view adds nothing.

So a `View` costs essentially the same as direct indexing — the generality is
free at the element level, paid only by the O(1) window arithmetic.

## Composes over any backing — through the contract

`source: C` where `C: IndexRead<E>`. `Vector<T>` gives a `View` with `E = T`;
`String` gives one with `E = Byte`; a `View`'s `source` can itself be another
satisfier (sub-view of a view). The **`IndexRead` contract absorbs the per-backing
difference** (trie read vs. flat byte read) — no closure, no higher-kinded
plumbing. And because a `View` provides its own `get`/`len`/`iter`, it *itself*
satisfies `IndexRead<E>` / `IntoIterator<E>` / `Sliceable`, so views plug into the
same write-once generic algorithms as their backings.

## The one real caveat: backing retention

A small `View` keeps its whole backing alive (`start`/`len` don't free the rest).
This is **opt-in and localized** — you only create a `View` where you want one —
and you drop the backing by materializing (`to_vector` / `to_string`) at the
boundary where an owned value is actually needed.

The lexer's innermost byte loop keeps direct `s[i]` (Tier 1 in
[slice-performance.md](slice-performance.md)) — not because of indirection (there
is none), but because wrapping a single tight scan in a `View` buys nothing there.
`View` is for *composable/structural* use.

## First consumer

`emit/match.tw` recurses with `tail := arms.slice(1, arms.len())` (O(k²) over k
arms). With a view: `head := arms.first(); rest := arms.drop_first()` — O(k)
total, no copy, no `(arms, start)` parameter. Same shape fixes the
`fmt/printer.tw` doc-parts recursions and one-shot drop-firsts like
`segments.slice(1, …)`.

## Scope / non-goals

- **Read-only.** A `View` is a window for traversal/compare; you don't mutate
  through it. Building/shrinking a collection is the Stack/`drop_last` story
  ([stack.md](stack.md)).
- Not a replacement for `Vector`/`String` — an adjunct. Materialize with
  `to_vector` / `to_string` at the boundary where an owned value is needed.
- Doesn't change `concat`/arbitrary slice asymptotics — that's RRB
  ([rrb-vector-concat.md](rrb-vector-concat.md)).

## Cost contract

| Op | Cost |
|---|---|
| `from` | O(1) |
| `drop_first` / `drop_last` / `sub` / `len` / `is_empty` | O(1) |
| `get` / `first` | backing cost, **direct** (String O(1); Vector O(log n)) |
| `to_vector` / `to_string` | O(len) (materialize) |

## How it fits the family

- **`View<C>`** (this doc) — read-only zero-copy windows over any indexable
  backing; drop-first/traversal.
- **Access contracts** ([access-contracts.md](access-contracts.md)) — the bounds
  `View` satisfies and is written against.
- **Stack / `drop_last`** ([stack.md](stack.md)) — LIFO build/shrink.
- **RRB** ([rrb-vector-concat.md](rrb-vector-concat.md)) — arbitrary O(log n)
  `concat`/`slice` on `Vector` itself.
- **Tier 1 string compares** ([slice-performance.md](slice-performance.md)) —
  direct-indexing in the hot byte loop.

## Open questions

- **Type parameter:** `View<C>` (element via the FD, above) vs. `View<C, E>`
  (explicit). Lean `View<C>` — it showcases the functional dependency and keeps
  signatures clean.
- **String element type:** a `String` view yields `Byte`; provide `to_string` and
  compare-against-`String` helpers so it's ergonomic for text scanning.
- **Retention control:** a copy-on-small-view materialization helper for the cases
  where pinning a large backing is a concern?
- **Naming:** `View<C>` vs `Window<C>`; method surface (`get`/`first`/`rest`?).
- **Module path:** `@std.view`? Prelude-visible or explicit `use`?
