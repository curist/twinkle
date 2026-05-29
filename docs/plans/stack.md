# `Stack<T>` — and an O(log n) `drop_last` vector op

Status: proposal. Companion to [slice-performance.md](slice-performance.md) (the
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
  len-1)` which is O(m)).
- `Vector.last(v) T?` already-style peek.

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

- **Type vs just `drop_last` + `last` on `Vector`**: is the `Stack<T>` wrapper
  worth it, or do the renamed `Vector` ops read clearly enough on their own? The
  wrapper's only value is intent-signaling.
- **`pop` shape**: `pop(s) Stack<T>` (discard top, above) is convenient for the
  discard-heavy sites (`pop_scope`); add a combined `pop_value(s) .{ value, rest }?`
  (no tuples in the language) for the take-and-continue sites (Tarjan)?
- **Empty `pop`**: trap (treat as OOB) or no-op? Lean trap, matching array OOB.
- **Foundation**: confirm `drop_last` (recommended) over the cursor.
- **Naming / module**: `Stack<T>`, `@std.stack`; prelude-visible or explicit `use`?
