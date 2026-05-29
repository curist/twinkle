# `Stack<T>` — and an O(log n) `drop_last` vector op

Status: **partially implemented**. The ergonomics layer has landed; the
performance foundation and migrations have not.

**Done:**
- `Stack<T>` in `@std.stack` (`new`, `from_vector`, `to_vector`, `push`, `top`,
  `pop`, `len`, `is_empty`) — first stdlib-owned generic type. Explicit `use`
  (not prelude-visible).
- `Vector.drop_first` / `drop_last` in `prelude/vector.tw` — **total** (empty →
  empty, no trap), with the settled names. **But these are O(m)**, built on
  `slice`, *not* the O(log n) runtime op below.
- `Stack.pop` on empty is a no-op (total); `top()` returns `T?`.
- Boot only (stage0 never compiles `@std.stack`).

**Not done (remaining work):**
- The **O(log n) runtime `drop_last` vector op** — the central performance thesis
  of this doc (see "Foundation" below). Today's `drop_last` is the O(m) slice
  shim; the API is forward-compatible with the runtime op replacing it.
- The **migration targets** (`pop_scope`, Tarjan worklist, fmt/lexer stacks) — none
  rerouted to `Stack`/`drop_last` yet, so the O(n²) Tarjan risk stands.
- Access-contract integration (`IndexRead`/`IndexWrite`) — see
  [access-contracts.md](access-contracts.md).
- `pop_value(s) .{ value, rest }?` combined shape (open question below).
- Stage0 mirror of the runtime op (only needed once the op exists).

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
pub fn len<T>(s: Stack<T>) Int        { s.items.len() }
pub fn is_empty<T>(s: Stack<T>) Bool  { s.items.len() == 0 }
// from_vector / to_vector bridges
```

`pop_scope` becomes `ctx.scopes = ctx.scopes.pop()`; the Tarjan loop does
`x := stack.top(); stack = stack.pop()`. The wrapper is **pure ergonomics** —
the perf comes entirely from `drop_last`.

`Stack<T>` also satisfies the access contracts
([access-contracts.md](access-contracts.md)) — `IndexRead<T>` (`top` is
`get(len-1)`) and `IndexWrite<T>` — so it plugs into the same write-once generic
algorithms (`fold`, `position`, …) as `Vector`, `String`, and `View`.

### Costs

| Op | Cost |
|---|---|
| `push` | O(1) amortized |
| `pop` / `top` | O(log n) |
| `len` / `is_empty` | O(1) |

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
  `IndexRead`/`IndexWrite` bounds these types satisfy.
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
- **Foundation**: confirm `drop_last` (recommended) over the cursor. **Still open**
  for the *runtime op*; the stdlib `Stack` currently rides the O(m) slice shim.
- ~~**Naming / module**~~ — resolved: `Stack<T>` in `@std.stack`, **explicit `use`**
  (not prelude-visible).
