# `Stack<T>` — and an O(log n) `drop_last` vector op

Status: **implemented** (foundation + ergonomics + migrations). Remaining items
are the optional follow-ons noted at the end.

**Done:**
- The **O(1)-amortized runtime `drop_last` vector op** (the foundation below).
  It shrinks the tail in O(1) and pulls the last trie leaf back into a fresh tail
  only at a 32-boundary (then O(log n)) — the exact inverse of `push`. Lives in
  `rt.arr` (boot `codegen/runtime/arr.tw`) and is mirrored in stage0
  `src/runtime/arr.rs`. Wired as the `Vector.drop_last` builtin in both compilers.
  One safety refinement over textbook Clojure `pop`: the boundary case **copies**
  the pulled leaf into a private tail rather than sharing it, so the `set_in_place`
  uniqueness optimization can never corrupt the original vector's trie.
- `Vector.drop_last` is now the runtime op (not the slice shim); `Vector.drop_first`
  stays an O(m) prelude function built on `slice` (left-drop needs RRB).
- The **migration targets** are rerouted to `Vector.drop_last`: `pop_scope`
  (checker + lower_core), the Tarjan SCC worklist (`type_order.tw`), the lexer
  `interp_depths` stack, and the `fmt` `fit_stack` / trivia stacks. This bounds
  the Tarjan worklist's repeated pops to O(log n) each.
- `Stack<T>` in `@std.stack` (`new`, `push`, `pop`, `top`, `is_empty`,
  `to_vector`) — first stdlib-owned generic type. `Stack.pop` on empty is a
  no-op (total); `top()` returns `T?`. The surface is deliberately minimal: the
  LIFO core plus `to_vector` as the escape hatch for size/traversal. `from_vector`
  and `len` were dropped (2026-05-30) — `from_vector` was a rarely-used seeder and
  `len` is recoverable via `to_vector().len()`; neither pulled its weight.

**Why the compiler migrated to `Vector.drop_last`, not `Stack<T>`:** the perf comes
entirely from `drop_last`; the wrapper is pure ergonomics. `@std.stack` is also
*not* in the bootstrap closure (and uses prelude helpers stage0 doesn't inject into
stdlib), so using it inside the compiler would break stage0. The `Vector` op is the
correct receiver for these internal sites.

**Decided against — access-contract integration (2026-05-30).** `Stack<T>` is
**not** an `IndexRead`/`IndexWrite` satisfier, reversing the earlier plan. A `Stack`
is an *access-restricting* abstraction: its whole value over a bare `Vector` is that
it constrains callers to LIFO (`push`/`pop`/`top`) and signals that intent — exactly
as a queue constrains to FIFO. Exposing `at(i)` / `s[i]` / for-in would let callers
reach past the abstraction into arbitrary positions, defeating the reason to choose a
`Stack`. Backing it on a `Vector` makes positional access free to *implement*, but
that is not a reason to *expose* it. To traverse, materialize explicitly with
`to_vector()`. (`View` is different — it *is* a random-access window, so `IndexRead`
is its essence; see [view.md](view.md).)

**Not done (optional follow-ons):**
- `pop_value(s) .{ value, rest }?` combined shape (open question below) — the
  take-and-continue sites (Tarjan) still read `top` then `drop_last` separately.
- An O(log n) `drop_first` (left-drop) — needs RRB relaxed nodes.

Companion to [slice-performance.md](slice-performance.md) (the
audit), [view.md](view.md) (the read-only side),
[access-contracts.md](access-contracts.md) (the general access bounds), and
[rrb-vector-concat.md](rrb-vector-concat.md).

Supersedes the earlier queue/deque proposal: the boot-compiler audit showed the
real need is **LIFO stack**, not FIFO — see "Why a stack, not a queue" below.

## Why a stack, not a queue

The slice audit ([slice-performance.md](slice-performance.md)) found the
compiler's `Vector` end-drops are overwhelmingly **LIFO stack pops**
(`xs.slice(0, xs.len() - 1)`):

- `checker.tw` / `lower_core/context.tw` — `pop_scope` (scope stacks)
- `codegen/type_order.tw` — Tarjan SCC worklist (the one with O(n²) risk)
- `fmt/layout.tw` (`fit_stack`), `fmt/printer.tw` (trivia), `lexer.tw`
  (`interp_depths`)

A FIFO queue/deque doesn't fit any of these, so that idea is dropped. What's
wanted is a stack — both for **performance** (today `slice(0, len-1)` rebuilds
the whole prefix via `from_array`, O(m)) and for **ergonomics** (`push`/`pop`/
`peek` instead of `append` + `slice(0, len-1)` + `xs[len-1]`).

## Foundation: an O(log n) `drop_last` vector op

The core fix is a runtime op on `Vector` itself, the **inverse of `push`**: drop
the last element by shrinking the tail (or pulling the last trie leaf back into
the tail). It is genuinely **O(log n)** persistent, needs **no RRB** (only
*left*-drop needs relaxed nodes), and is independently useful:

- `Vector.drop_last(v) Vector<T>` — O(log n), shares structure (vs `slice(0,
  len-1)` which is O(m)). **Total**: on an empty vector it returns the empty
  vector (no trap).
- `Vector.last(v) T?` already-style peek.

**Settled name**: `drop_last` (paired with `Vector.last` / `Vector.first` peeks and
the `View` window ops `drop_first`/`drop_last`, [view.md](view.md)). It follows
Swift `dropLast()` / Kotlin `dropLast()` / Clojure `drop-last`, same semantics.

This alone fixes every LIFO site if those `slice(0, len-1)` consume-reassigns are
rerouted to `drop_last` (and bounds the Tarjan O(n²) to O(n log n)). Lands in
boot `arr.tw` first, mirrored to stage0 `arr.rs` (same discipline as the RRB
plan).

## The `Stack<T>` type (ergonomics over `drop_last`)

Once `drop_last` exists, a `Vector` *is* an O(log n) stack — so `Stack<T>` is a
thin wrapper that signals intent and gives stack-shaped names:

```tw
pub type Stack<T> = .{ items: Vector<T> }     // top == items[items.len() - 1]

pub fn new<T>() Stack<T>              { .{ items: [] } }
pub fn push<T>(s: Stack<T>, x: T) Stack<T> { s.items = s.items.append(x) }   // O(1) amortized
pub fn top<T>(s: Stack<T>) T?         { s.items.last() }                      // O(log n)
pub fn pop<T>(s: Stack<T>) Stack<T>   { s.items = s.items.drop_last() }       // O(log n), drops top
pub fn is_empty<T>(s: Stack<T>) Bool  { s.items.len() == 0 }
pub fn to_vector<T>(s: Stack<T>) Vector<T> { s.items }   // escape hatch: size/traversal
```

`pop_scope` becomes `ctx.scopes = ctx.scopes.pop()`; the Tarjan loop does
`x := stack.top(); stack = stack.pop()`. The wrapper is **pure ergonomics** —
the perf comes entirely from `drop_last`.

`Stack<T>` deliberately does **not** satisfy the access contracts
([access-contracts.md](access-contracts.md)) — no `IndexRead`/`IndexWrite`. Random
positional access would defeat the point of a LIFO abstraction (see "Decided
against" above); traverse by materializing with `to_vector()`.

### Costs

| Op | Cost |
|---|---|
| `push` | O(1) amortized |
| `pop` / `top` | O(log n) |
| `is_empty` / `to_vector` | O(1) |

## Alternative without a runtime change: a cursor

If we'd rather not add `drop_last`, a `Stack<T> = .{ items: Vector<T>, top: Int }`
cursor works in pure stdlib: `push` appends or `set`s at `top` and bumps it;
`pop` just decrements `top` (no allocation); `peek` reads `items[top-1]`. Costs
are similar (O(log n)), but the semantics are subtler (popped slots linger until
overwritten; push-after-pop is an O(log n) `set`). Prefer `drop_last` — it's a
clean general op and keeps `Stack` trivial.

## Migration targets

`pop_scope` (checker + lower_core), the Tarjan worklist (`type_order.tw`), and
the `fmt`/`lexer` stacks. The Tarjan one is the only genuine O(n²) risk; the
scope stacks are bounded-depth (the win there is removing per-pop `from_array`
allocation churn, not asymptotics).

## How it fits the family

- **`Stack` / `drop_last`** (this doc) — LIFO build/shrink.
- **`View<C>`** ([view.md](view.md)) — read-only traversal / drop-first.
- **Access contracts** ([access-contracts.md](access-contracts.md)) — the general
  `IndexRead`/`IndexWrite` bounds that `Vector`/`String`/`View` satisfy (`Stack`
  deliberately does not — see "Decided against" above).
- **RRB** ([rrb-vector-concat.md](rrb-vector-concat.md)) — arbitrary O(log n)
  `concat`/`slice`.
- Queue/Deque — considered and **dropped** (audit showed FIFO isn't a real need).

## Open questions

- ~~**Type vs just `drop_last` + `last` on `Vector`**~~ — resolved: **wrapper shipped.**
  `Stack<T>` lives in `@std.stack` for intent-signaling; the `Vector` ops remain
  available for sites that prefer them.
- **`pop` shape**: `pop(s) Stack<T>` (discard top, above) is convenient for the
  discard-heavy sites (`pop_scope`); add a combined `pop_value(s) .{ value, rest }?`
  (no tuples in the language) for the take-and-continue sites (Tarjan)? **Open** —
  not yet added.
- ~~**Empty `pop`**~~ — resolved: **no-op, returns empty** (total). `Vector.drop_last`
  on an empty vector returns the empty vector, so `Stack.pop` on an empty stack
  yields the empty stack rather than trapping. `top()` already returns `T?`
  (`.None` when empty), so callers that need to detect underflow check `top`.
- ~~**Foundation**: confirm `drop_last` (recommended) over the cursor.~~ — resolved:
  the O(1)-amortized runtime `drop_last` landed in both compilers and backs both
  `Vector.drop_last` and `Stack.pop`. The cursor alternative was not needed.
- ~~**Naming / module**~~ — resolved: `Stack<T>` in `@std.stack`, **explicit `use`**
  (not prelude-visible).
