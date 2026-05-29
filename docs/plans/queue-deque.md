# Persistent Queue (FIFO) — and the Deque upgrade path

Status: proposal. Pure **stdlib** type — no compiler or runtime changes. Likely
`@std.queue` (sibling of `@std.fs`, `@std.date`, …). Companion to
[rrb-vector-concat.md](rrb-vector-concat.md).

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

Recommendation: **start with A** (immediate cheap win, zero new types), and adopt
B only if push/pop at *both* ends turns out to be needed — see *Deque upgrade*.

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
real both-ends workload shows up; the FIFO `Queue` already covers dequeue.

(Note: a vector-only deque is possible with cursors on both stacks, but `pop`
from a `Vector` stack still needs an O(k) slice at rebalance — cons lists are
cleaner for both ends.)

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

Complementary and at a different layer:

- This type → cheap, library-level, solves FIFO/dequeue now.
- RRB → general-purpose O(log n) `concat`/`slice` on `Vector`, bigger and gated.

Ship this first; it removes the essential dequeue motivation from the RRB
decision.

## Open questions

- **Which type to ship (A vs B)** — FIFO `Queue` now, or go straight to a
  `Deque` (and `List<T>`)? Recommendation: A first.
- **`dequeue` shape** — `Dequeued<T>?` record (above), or a split API
  (`peek` + `drop`), or `(Queue, T?)`-style? The language has no tuples, so a
  small record is the natural choice; confirm naming (`Dequeued` / `Popped`).
- **Naming** — `Queue` vs Rust-style `VecDeque` vs Gleam-style `Queue`-as-deque.
  If B is ever adopted, a single `Deque` name avoids shipping two types.
- **Module path** — `@std.queue`? And whether it's prelude-visible or an explicit
  `use @std.queue`.
