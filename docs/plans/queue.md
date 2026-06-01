# `@std.queue` — persistent double-ended queue

Status: proposal.

Add a standard-library `Queue<T>` that supports efficient operations at both
ends. `Vector<T>` remains the default sequence type, but it is not the best API
or performance fit for FIFO/deque-heavy code: `Vector.drop_first()` is currently
implemented as `slice(1, len)` and documented as O(log n), while `drop_last()` is
O(log n) worst-case and O(1) when shrinking the tail. A dedicated queue can use
`Vector`'s strong append/drop-last behavior internally while avoiding repeated
front slices in user code.

## Goals

- Provide a low-ceremony immutable deque under `@std.queue`.
- Support both front and back operations, matching the shape users expect from a
  double-ended queue.
- Preserve value immutability: every operation returns a new queue and shares
  structure where possible.
- Keep the first version entirely in Twinkle stdlib code; no compiler/runtime
  primitive is required.
- Document performance conservatively until benchmarks confirm constants and
  rebalancing behavior.

## Non-goals

- Do not replace `Vector<T>` as the general ordered collection.
- Do not put `Queue<T>` in the prelude initially.
- Do not add mutation or builder-only APIs in the first version.
- Do not promise strict O(1) persistence semantics for all sharing patterns; use
  amortized wording and benchmark before strengthening the guarantee.

## API

```tw
use @std.queue
use @std.queue.{Queue}

q: Queue<Int> = queue.new()
q = q.push_back(1).push_front(0)

case q.pop_front() {
  .Some(pop) => {
    value := pop.value
    q = pop.rest
  },
  .None => {},
}
```

Proposed public types:

```tw
pub type Queue<T> = .{
  front: Vector<T>,
  back: Vector<T>,
}

pub type Pop<T> = .{
  value: T,
  rest: Queue<T>,
}
```

Proposed functions:

```tw
pub fn new<T>() Queue<T>
pub fn singleton<T>(value: T) Queue<T>
pub fn from_vector<T>(xs: Vector<T>) Queue<T>
pub fn to_vector<T>(q: Queue<T>) Vector<T>

pub fn len<T>(q: Queue<T>) Int
pub fn is_empty<T>(q: Queue<T>) Bool

pub fn push_front<T>(q: Queue<T>, value: T) Queue<T>
pub fn push_back<T>(q: Queue<T>, value: T) Queue<T>

pub fn peek_front<T>(q: Queue<T>) T?
pub fn peek_back<T>(q: Queue<T>) T?
pub fn pop_front<T>(q: Queue<T>) Pop<T>?
pub fn pop_back<T>(q: Queue<T>) Pop<T>?
```

Qualified and method-call forms should both work:

```tw
q = queue.push_back(q, x)
q = q.push_back(x)
```

## Representation

Use the classic two-vector representation:

```tw
pub type Queue<T> = .{ front: Vector<T>, back: Vector<T> }
```

The logical sequence is:

```tw
front ++ back.reverse()
```

`front` stores elements available from the front in normal order. `back` stores
elements available from the back in reverse order. Pushing to either end appends
to the corresponding internal vector:

- `push_front(q, x)` appends `x` to `front`'s rear representation if `front` is
  kept reversed for front-side popping, or uses the symmetric representation
  chosen during implementation.
- `push_back(q, x)` appends `x` to `back`.

The implementation should choose the exact orientation that lets both `pop_front`
and `pop_back` consume with `Vector.drop_last()` on the active side, because
`drop_last()` has a better fast path than `drop_first()` today. `to_vector()` is
responsible for presenting the public logical order.

When one side is empty and the other side must serve the opposite end, rebalance
by splitting/reversing the non-empty side into both sides. This avoids repeatedly
reversing the whole queue after alternating operations.

## Performance expectations

The intended benefits over plain `Vector` are:

- FIFO workloads avoid repeated `Vector.drop_first()` structural slices.
- Both-end workloads can usually push and pop against vector tails.
- Rebalancing is occasional for linear use, giving amortized efficient behavior.

Do not overclaim the initial implementation. Current `Vector.reverse()` is a
Twinkle-level loop using indexed reads and appends, so a full rebalance may carry
more than a simple linear constant until runtime/vector helpers improve. The docs
should say "amortized efficient" or "designed for efficient operations at both
ends" rather than unconditional O(1), unless benchmarks justify a tighter claim.

Benchmarks should compare:

- repeated `Vector.drop_first()` FIFO consumption
- repeated `Queue.pop_front()` FIFO consumption
- mixed `push_front`/`push_back` and `pop_front`/`pop_back`
- small queues, where `Vector` may still win due to lower overhead

## Implementation steps

1. Add `boot/stdlib/queue.tw` with `Queue<T>`, `Pop<T>`, and the API above.
2. Add tests under `boot/tests/suites/stdlib_queue_suite.tw` covering:
   - construction and emptiness
   - front/back pushes and pops
   - peeks without removing
   - `from_vector`/`to_vector` preserving order
   - rebalancing after one side empties
   - persistence: old queue values remain usable after operations
3. Register the suite in `boot/tests/main.tw`.
4. Update `docs/API.md` with the public API and conservative performance notes.
5. Run formatter on edited `.tw` files.
6. Validate with `target/twk run boot/tests/main.tw` (or `BOOT_WASM=...` during
   bootstrap-sensitive work).

## Open questions

- Should the pop result type be named `Pop<T>`, `Entry<T>`, or `Item<T>`?
  `Pop<T>` is explicit and avoids colliding with iterator terminology.
- Should `Queue<T>` expose `iter()` immediately? It is useful, but `to_vector()`
  plus vector iteration may be enough for the first version.
- Should `Queue<T>` support `==` by delegating to `to_vector()` equality when
  `T: Eq`? This is convenient but may hide an allocation; consider after the
  core API lands.
- Do we want `append_front`/`append_back` or concatenation later? Leave out until
  use cases appear.
