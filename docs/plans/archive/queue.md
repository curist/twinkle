# `@std.queue` — persistent double-ended queue

Status: complete.

`@std.queue` provides a standard-library `Queue<T>` for immutable FIFO and deque
workloads. `Vector<T>` remains the default ordered collection, but it is not the
best fit for repeated front removal: `Vector.drop_first()` is implemented as a
structural slice, while `Vector.drop_last()` can often shrink the tail cheaply.
`Queue<T>` uses two vectors internally so common queue operations work against
vector tails and avoid repeated front slices in user code.

## Goals

- Provide a low-ceremony immutable deque under `@std.queue`.
- Support front and back operations with the shape users expect from a
  double-ended queue.
- Preserve value immutability: operations return a new queue and share structure
  where possible.
- Keep the implementation entirely in Twinkle stdlib code, with no compiler or
  runtime primitive.
- Document performance conservatively: the queue is designed for efficient
  operations at both ends, but rebalancing and `Vector.reverse()` still matter.

## Non-goals

- Do not replace `Vector<T>` as the general ordered collection.
- Do not put `Queue<T>` in the prelude initially.
- Do not add mutation or builder-only APIs in the first version.
- Do not promise strict O(1) persistence semantics for all sharing patterns.

## Public API

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

Types:

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

Functions:

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

Qualified and method-call forms both work:

```tw
q = queue.push_back(q, x)
q = q.push_back(x)
```

## Representation

`Queue<T>` uses the classic two-vector representation:

```tw
pub type Queue<T> = .{ front: Vector<T>, back: Vector<T> }
```

The logical sequence is:

```tw
front.reverse() ++ back
```

`front` stores front-side elements in reverse order. `back` stores back-side
elements in normal order. Pushing to either end appends to the corresponding
internal vector:

- `push_front(q, x)` appends `x` to `front`.
- `push_back(q, x)` appends `x` to `back`.

This orientation lets `pop_front` and `pop_back` consume with
`Vector.drop_last()` while the active side has elements. `to_vector()` presents
the public logical order by reversing `front` and concatenating `back`.

When one side is empty and the other side must serve the opposite end, the
implementation materializes the remaining logical sequence and splits it back
across both sides with `from_vector`. This avoids repeatedly reversing the whole
queue in alternating operation patterns.

The representation fields are public because Twinkle records are public today.
Users should treat them as representation details and prefer the module API so
future implementations can preserve invariants consistently.

## Performance notes

The intended benefits over plain `Vector` are:

- FIFO workloads avoid repeated `Vector.drop_first()` structural slices.
- Both-end workloads usually push and pop against vector tails.
- Rebalancing is occasional for linear use, giving amortized efficient behavior.

The first implementation should not be documented as unconditional O(1). A full
rebalance uses `Vector.reverse()` and vector materialization in Twinkle code, so
sharing patterns and constants matter. Public docs use conservative wording such
as "designed for efficient operations at both ends" rather than strict bounds.

A quick local FIFO benchmark showed `Queue.pop_front()` substantially outperforming
repeated `Vector.drop_first()` after building the same logical sequence. Small
queues still have enough fixed overhead that callers should choose based on API
clarity unless a path is known to be queue-heavy.

## Completed implementation work

- Added `boot/stdlib/queue.tw` with the public API above.
- Added `boot/tests/suites/stdlib_queue_suite.tw` covering construction,
  peeking, front/back push and pop, vector conversion, rebalancing, and
  persistence of old queue values.
- Registered the suite in `boot/tests/main.tw`.
- Updated `docs/API.md` with the public API and conservative performance notes.
- Regenerated the embedded core library as part of the bootstrap flow.

Validation used:

```bash
make stage2
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

## Deferred follow-ups

- `iter()` can be added when there is a clear need. For now, `to_vector()` plus
  vector iteration keeps the initial API small.
- Queue equality can be considered later. Delegating to `to_vector()` would be
  convenient, but may hide allocation and reversal work.
- Concatenation or `append_front` / `append_back` can be added when use cases
  appear.
- More formal benchmarks should cover FIFO consumption, both-end workloads,
  alternating operations that force rebalancing, and very small queues.
