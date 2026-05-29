# Persistent end-access: `drop_last` op, `Deque`, and a FIFO `Queue`

Status: proposal. Companion to [rrb-vector-concat.md](rrb-vector-concat.md).
Originally scoped as a pure-stdlib FIFO `Queue`, but a scan of the boot compiler
([Audit](#audit-how-the-boot-compiler-actually-uses-slice)) shows its end-drop
usage is mostly **LIFO stack pop**, so the recommended first step is now a small
**O(log n) `drop_last` vector runtime op** (`arr.tw`), with a structural `Deque`
type as the follow-on and a FIFO `Queue` only for external pure-FIFO needs.

## Why this exists

`Vector<T>` has no cheap end-removal: dropping the head/tail means `slice`, which
re-materializes the range and shares nothing with the source, so dequeue / pop
loops are **O(n²)** (see the RRB plan). Using a `Vector` as a queue is the common
way to hit that trap, and **FIFO queue/dequeue usage is essential**.

A small persistent queue type fixes the essential case cheaply — as a library
over the existing `Vector`, with no trie surgery. It **coexists** with the RRB
work rather than competing: RRB makes *arbitrary* `concat`/`slice` O(log n) on the
general-purpose `Vector`; this gives an O(1)-amortized-enqueue / sub-linear-dequeue
FIFO structure. Shipping it also strips the dequeue case out of RRB's
justification (RRB Gate A), letting RRB stand purely on arbitrary concat/slice.

## Audit: how the boot compiler actually uses slice

A scan of `boot/` (Gate A for slice) **corrects the premise** and reshapes the
recommendation. The overwhelming majority of `.slice(` is **`String` substring**
(lexer, paths, JSON, LSP framing) — irrelevant here. The real `Vector` end-drops
are dominated by **LIFO stack pop**, not FIFO dequeue:

| Pattern | Sites | Notes |
|---|---|---|
| **LIFO pop** `xs.slice(0, len-1)` | `checker.tw:85` & `lower_core/context.tw:101` (`pop_scope`), `codegen/type_order.tw:209` (Tarjan SCC worklist), `fmt/layout.tw:224` (`fit_stack`), `fmt/printer.tw:118` (trivia), `lexer.tw:369/379/394` (`interp_depths`) | scope stacks are hot but bounded-depth; **Tarjan worklist can be large → genuine O(n²)** |
| **FIFO head-drop** `xs.slice(1, len)` | `emit/match.tw` ×4 (**recursive** head/tail over match arms → O(k²)), `fmt/printer.tw:1242/1273` (recursive doc parts) | k = arms/parts, usually modest |
| one-shot drop-first | `loader.tw:74`, `checker.tw:1935/2006`, `run.tw`, `argv.tw` | harmless (not loops) |

**A FIFO `Queue` would touch almost none of these.** Consequences for what to build:

1. **An O(log n) `drop_last` / `pop` vector op** is the highest-value, most
   transparent fix. It is the inverse of `push` (shrink the tail / pull a leaf
   back in), genuinely O(log n) persistent, needs **no new type**, and does **not**
   need RRB (only left-drop does). Today `slice(0, len-1)` rebuilds via
   `from_array` at O(m); routing these consume-reassign sites to `drop_last` fixes
   scope stacks, the Tarjan worklist, and the fmt stacks at once. **Recommended
   first step** — it is a small `arr.tw` runtime addition, not a stdlib type.
2. If a structural type is wanted, prefer a **`Deque`** (LIFO + FIFO) over a
   FIFO-only `Queue`: the compiler's need is mostly the stack side.
3. The **match-arm O(k²) recursion** is a trivial local rewrite — pass a start
   index instead of `slice(1, …)` — needing no new type at all.

## One type, not two (cross-language norm)

It is uncommon to ship separate `Queue` and `Deque` types. The mainstream choice
is a single double-ended structure that also serves FIFO:

- **Rust** — `VecDeque<T>` (one growable ring buffer; mutable, not persistent).
- **Gleam** — `gleam/queue` is itself two-ended (push/pop both ends), built from
  two lists.
- **Haskell** — `Data.Sequence` (finger tree): both ends O(1) amortized, plus
  O(log n) concat/split/index — one type for everything.

So this plan ships **one type**. The open decision is which:

- **A. FIFO `Queue<T>`** — cheapest; two `Vector`s + a head cursor; no new
  recursive type. O(1)-amortized enqueue, **O(log n)** dequeue. Fully covers the
  essential workload.
- **B. `Deque<T>`** — both ends; the conventional shape, but needs a small cons
  `List<T>` (Okasaki two-list, amortized O(1) both ends) or a finger tree
  (≈ RRB complexity).

Recommendation (revised by the audit above): the compiler's own need is
LIFO-stack-heavy, which a FIFO-only `Queue` (A) does **not** serve. So:

- For the **compiler's internal sites**, ship the **O(log n) `drop_last` vector
  op** first (no new type) and rewrite the match-arm recursion to index-based.
- For a **structural type**, prefer **B (`Deque`)** since it covers both the
  stack (LIFO) and queue (FIFO) needs in one — matching the cross-language norm.
- The standalone FIFO `Queue` (A) is only worth it if a real *external* FIFO
  workload (not the compiler) wants the absolute-cheapest, zero-new-type option.

## Design A — FIFO `Queue<T>` over two Vectors + a cursor

The trick: never remove from a `Vector` (that would slice). Read the front by an
**index cursor** and append to the back; when the front cursor exhausts, the back
vector *becomes* the new front by a cheap reference move (no copy, no reversal —
`Vector` preserves order and `back[0]` is exactly the next element after the
front's tail).

```tw
pub type Queue<T> = .{ front: Vector<T>, head: Int, back: Vector<T> }
// logical sequence = front[head .. front.len()]  ++  back[0 .. back.len()]
// invariant: 0 <= head <= front.len()

pub fn new<T>() Queue<T> { .{ front: [], head: 0, back: [] } }

pub fn len<T>(q: Queue<T>) Int { (q.front.len() - q.head) + q.back.len() }

pub fn is_empty<T>(q: Queue<T>) Bool { q.len() == 0 }

pub fn enqueue<T>(q: Queue<T>, x: T) Queue<T> {
  q.back = q.back.append(x)        // O(1) amortized; rebinds q
}

pub fn peek<T>(q: Queue<T>) T? {
  if q.head < q.front.len() { .Some(q.front[q.head]) }
  else if q.back.len() > 0 { .Some(q.back[0]) }
  else { .None }
}

// Returns the head plus the rest of the queue (no tuples in the language).
pub type Dequeued<T> = .{ value: T, queue: Queue<T> }

pub fn dequeue<T>(q: Queue<T>) Dequeued<T>? {
  if q.head < q.front.len() {
    v := q.front[q.head]
    q.head = q.head + 1
    .Some(.{ value: v, queue: q })
  } else if q.back.len() > 0 {
    // rotate: back becomes the new front (reference move, O(1))
    q.front = q.back
    q.head = 1
    q.back = []
    .Some(.{ value: q.front[0], queue: q })
  } else {
    .None
  }
}
```

Inherent-method style (first param `Queue`, builder-returning) gives
`q.enqueue(x)`, `q.dequeue()`, `q.peek()`, `q.len()`. Plus `from_vector` /
`to_vector` bridges.

### Costs

| Op | Cost |
|---|---|
| `enqueue` | O(1) amortized (`Vector.append`) |
| `dequeue` | **O(log n)** (one `Vector` get + cursor/rotate; rotate is O(1)) |
| `peek` / `len` / `is_empty` | O(log n) / O(1) |

`n` enqueue+dequeue → **O(n log n)** (the goal: kills the O(n²) slice loop).

### Space

The dequeued prefix `front[0..head)` stays referenced until the next rotate, when
`front` is replaced wholesale. Live footprint is O(current size); the dead prefix
is bounded by the front length and reclaimed at rotate. (Optional: when
`head == front.len()` and `back` is empty, reset to `new()` to release eagerly.)

### Why not O(1) dequeue?

The O(log n) is the `Vector` index of `front[head]`. True O(1)-amortized dequeue
needs cons-list stacks (next section). For most workloads O(log n) is plenty and
the simplicity (no new recursive type, no reversal) is worth it.

## Deque upgrade (Design B, if both ends are needed)

A persistent **deque** with amortized O(1) at both ends is the Okasaki two-stack
queue generalization, which wants a minimal cons list:

```tw
pub type List<T> = { Nil, Cons(T, List<T>) }   // small, recursive
pub type Deque<T> = .{ front: List<T>, back: List<T> }
```

- `push_front` = `Cons` onto `front`; `push_back` = `Cons` onto `back`. O(1).
- `pop_front` from `front`; when `front` empties, split/reverse `back` into it
  (amortized O(1)). `pop_back` symmetric.
- A real-time variant removes the amortization if worst-case bounds are needed.

This is the conventional single double-ended type (matches Rust/Gleam/Haskell),
at the cost of introducing `List<T>` and the rebalance logic. Defer unless a
real both-ends workload shows up.

Note the layering once `drop_last` exists:

- A **Stack** needs no type at all — a plain `Vector` with `push` + `drop_last`
  is O(log n) both ways. This already covers every LIFO site in the audit.
- A **Deque**'s *back* ops (`push_back`/`pop_back`) are likewise O(log n) on a
  `Vector` via `append`/`drop_last`; only the *front* ops (`push_front`/
  `pop_front`) are the hard part — left-drop needs cons lists (Okasaki) or RRB,
  since a left `slice` is O(n) without relaxed nodes.

## Cost contract (to document in API.md)

| Type | enqueue / push | dequeue / pop | peek | notes |
|---|---|---|---|---|
| `Queue<T>` (A) | O(1) amortized | O(log n) | O(log n) | FIFO; two vectors + cursor |
| `Deque<T>` (B) | O(1) amortized | O(1) amortized | O(1) | both ends; needs `List<T>` |

## Testing

- Behavioral: FIFO ordering across interleaved enqueue/dequeue, empty/peek edge
  cases, rotate boundary (front exhausts exactly), `from_vector`/`to_vector`
  round-trip.
- **Scaling guard**: enqueue N then dequeue N (and interleaved) asserts
  near-linear total time — the regression the type exists to prevent, mirroring
  the RRB plan's dequeue guard.
- Differential stage0/boot (it's pure Twinkle, so this is just normal boot-test
  coverage; no runtime divergence to reconcile).

## Relationship to the RRB plan

Complementary and at different layers:

- `drop_last` op → cheap `arr.tw` addition; fixes the compiler's LIFO stack sites
  and makes `Vector` a proper O(log n) stack. Independent of RRB (right-drop
  doesn't need relaxed nodes).
- `Deque` / `Queue` type → library-level structural sequence for FIFO / both-ends.
- RRB → general-purpose O(log n) `concat`/`slice` (incl. left-drop) on `Vector`,
  bigger and gated.

Ship `drop_last` first; it removes the LIFO stack motivation from both the type
work and RRB, and is the smallest change with the broadest internal payoff.

## Open questions

- **Priority** — confirm the audit-driven order: `drop_last` op → (maybe) `Deque`
  → FIFO `Queue` only for external needs. Originally this doc led with the Queue.
- **`drop_last` surface** — expose as `Vector.drop_last` / `pop_last` returning
  `Vector<T>` (and a `last`-returning variant), and/or auto-route existing
  `slice(0, len-1)` consume-reassign sites to it during lowering?
- **`dequeue` shape** (if a type is built) — `Dequeued<T>?` record (above), a
  split API (`peek` + `drop`), or `(Queue, T?)`-style? No tuples in the language,
  so a small record is natural; confirm naming (`Dequeued` / `Popped`).
- **Naming** — `Queue` vs Rust-style `VecDeque` vs Gleam-style `Queue`-as-deque.
  A single `Deque` name avoids ever shipping two types.
- **Module path** — `@std.queue` / `@std.deque`? Prelude-visible or explicit `use`?
