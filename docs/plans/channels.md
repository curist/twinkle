# Channels (CSP-style concurrency)

Status: Design (2026-06-20)

## Goal

Give Twinkle a high-level, value-passing concurrency primitive that sits on top
of the existing cooperative `Task` scheduler. Channels let tasks communicate by
sending immutable values instead of sharing `Cell`-backed state, and let a
consumer *park* until a value is available instead of polling.

The motivating evidence is in `boot/commands/lsp.tw`: the LSP server hand-rolls
two `Cell`-backed queues (`ChunkQueue`, `DiagnosticsQueue`) plus `time.sleep(1)`
poll loops (`wait_for_dispatcher`, `diagnostics_loop`) to move data between the
reader, dispatcher, and diagnostics tasks. Channels replace that with
`ch.send(...)` / `for x in ch { ... }`, deleting the shared `Cell` state and the
busy-poll loops (lower idle CPU, instant wake instead of up-to-1ms latency).

This is the higher-leverage choice over exposing raw fibers: channels fit the
existing scheduler (no second, user-driven scheduler contending for the JSPI
"current" stack), stay in Twinkle's low-ceremony / immutable-value idiom, and
directly retire the polling pattern. Fibers remain an internal mechanism (the
stackful JSPI suspension already underlying `Task`), not a public surface.

## Non-goals (v1)

- `select` / waiting on multiple channels at once, and `recv`-with-timeout.
  Deferred — the LSP terminates via channel close, not select. Add later when a
  concrete need appears.
- Split send/receive endpoint types (`SendChannel` / `RecvChannel`). Single
  `Channel<T>` handle.
- Unbounded channels (send never blocks). Omitted deliberately (unbounded memory
  if a consumer lags), matching Go.
- Stage0 (Rust) parity. Boot + runtime only, consistent with `Task`.

## Why not a pull/iterator abstraction?

A natural question: why a new primitive instead of "just another iterator"? Because
**iterators pull; channels push and park.** An iterator is lazy and caller-driven —
the consumer asks for the next element, and two iterators over a collection iterate
independently. A channel is the inverse: producers push values into a shared stream,
and a consumer that asks for a value with none available *parks* (suspends via the
scheduler) until a producer supplies one. That parking — cross-task, scheduler-
managed, with backpressure — is exactly the thing an iterator does not model, and
exactly what removes the LSP poll loops. Channels reuse the `IntoIterator` contract
for the `for v in ch { }` *syntax*, but the semantics are push/park, not pull (see
"Iterating a channel consumes a shared stream").

## API surface

`Channel<T>` is a compiler-registered builtin reference type (same family as
`Task`, `Dict`, `Cell`); no import needed.

```tw
// Construction
Channel.new<T>() Channel<T>                  // unbuffered (rendezvous)
Channel.bounded<T>(capacity: Int) Channel<T> // buffered, capacity >= 1

// Send — blocks per the backpressure rules below
ch.send(v)        Bool                       // false if the channel is closed (not delivered)

// Receive
ch.recv()         T?                          // .None once closed AND drained
for v in ch { }                              // drains until closed (IntoIterator)

// Close — idempotent, no-op if already closed
ch.close()
```

Rationale:

- **Unbuffered + bounded** (Go's model): rendezvous handoff and a fixed-capacity
  buffer with natural backpressure (send blocks when full). Two constructors
  rather than a single `new(capacity)` with a magic `0`. `capacity < 1` traps.
- **`recv() T?`, `send(v) Bool`** — non-trapping, and minimal because a channel
  has exactly one terminal condition (closed). There is no
  timeout/cancelled/disconnected to distinguish, so `Option`/`Bool` carry all the
  information a full `Result` would. `recv`'s `.None` = closed+drained; `send`'s
  `false` = closed (value not delivered). The common path stays clean:
  `for v in ch { }` (no unwrapping — closed is loop end), `v := try ch.recv()` to
  propagate in an `Option`-returning fn, and the prelude `Option` family
  (`unwrap_or`, `unwrap_or_else`, …) for one-offs. If a later op needs richer
  outcomes (a `recv_timeout`, or `select`), it gets its own return type — `recv`
  stays `T?`. The runtime boundary uses an extensible tagged result (see
  Implementation) so adding those later does not re-plumb anything.
- **Single `Channel<T>` handle**; multiple producers/consumers allowed
  (fan-in/out falls out of the wait-queue design).
- **Constructor naming:** `bounded(n)` over `with_capacity(n)`. `with_capacity`
  reads (esp. to Rust users) as a soft preallocation hint that can still grow;
  `bounded` names the actual semantic — a hard cap with backpressure.

## Semantics

**Unbuffered (`Channel.new`)** — rendezvous:

- `send(v)`: receiver parked → hand `v` over, wake it, return `true`. Else park
  the sender (holding `v`) until a receiver arrives (→ `true`) or close (→ `false`).
- `recv()`: sender parked → take its `v`, wake it (its `send` returns `true`),
  return `.Some(v)`. Else closed → `.None`. Else park the receiver.

**Bounded (`Channel.bounded(n)`, n ≥ 1)** — buffer with backpressure:

- `send(v)`: closed → `false`; receiver parked (buffer empty) → hand off
  directly, `true`; buffer has space → enqueue, return `true` (no block); buffer
  full → park the sender (holding `v`) until space frees (→ `true`) or close
  (→ `false`).
- `recv()`: buffer non-empty → dequeue front; if a sender was parked on a full
  buffer, move its value into the tail and wake it (→ `true`); return `.Some(v)`.
  Empty + closed → `.None`. Empty + open → park the receiver.
- Invariant: never parked receivers with a non-empty buffer, nor parked senders
  with a non-full buffer.

**Close (`close()`):**

- Idempotent — a second close is a no-op (non-trapping).
- Wakes **all** parked receivers: they drain remaining buffered values first
  (`.Some` each), then further `recv()` returns `.None`. Buffered values survive
  close (drain-then-closed, like Go).
- Wakes **all** parked senders: their `send` returns `false`; the pending value
  is dropped.
- `send` after close → `false`.

Conceptually: **close is normal stream termination for receivers** — `recv()` →
`.None`, exactly like an iterator ending, not an error. For senders the stream is
gone, so `send` after close fails as `false`. This is why `recv` returns
`Option<T>` and `send` returns `Bool` rather than `Result`: there is no
exceptional failure here to describe, only "no next value" / "not delivered."

**Ordering & fairness:** FIFO throughout — values received in send order; parked
senders and receivers each woken FIFO; one value to exactly one receiver (no
duplication).

**Blocking = cooperative suspension, not spinning:** "block" means the `Task`
parks via the scheduler (same Suspending machinery as `await`/`sleep`); the host
event loop stays free. This is what removes the LSP poll loops.

**Deadlock detection:** a task parked on a channel counts as blocked
(`blockedOnChannel`, alongside `blockedOnTask`). A deadlock is declared **only**
when the scheduler is fully quiescent: no runnable tasks, **`pendingHost == 0`**
(no stdin/socket/timer in flight), and the remaining tasks are parked on
channels/awaits. This deliberately does *not* flag a long-lived daemon/service
that sits idle: a server parked on stdin (or any host I/O, or a timer) keeps
`pendingHost > 0`, so it stays alive. Only a task graph where nothing could ever
make progress is reported.

**Edge cases:** `recv()` on a forever-empty open channel parks indefinitely (as
in Go), caught by deadlock detection only if the whole scheduler is otherwise
idle. `Channel.bounded(n)` with `n < 1` traps.

### Close ownership (convention)

`Channel<T>` is a single handle and any holder *can* call `close()`. v1 does not
enforce who closes (no split `SendChannel`/`RecvChannel` types — YAGNI). The
**convention is that the producer side owns close**: the task(s) that send are
responsible for closing once done, and receivers never close. This keeps "no more
values will arrive" meaning exactly "the producer said so," and avoids the
send-after-close races that unstructured closing causes. Tooling could enforce it
later via endpoint types if it proves necessary.

### Iterating a channel consumes a shared stream

`for v in ch { }` does **not** behave like iterating a collection. Multiple
iterators over the same channel *compete* for values — each value goes to exactly
one of them (a work queue), not to all of them (no broadcast):

```tw
Task.spawn(fn() { for v in ch { ... } })   // these two
Task.spawn(fn() { for v in ch { ... } })   // split the stream, round-robin-ish
```

This is intended (it's how you fan work out to a pool), but it differs from the
usual "independent iteration" intuition, so it is called out explicitly.

### Cancellation

v1 has no separate cancellation primitive: **closing the channel is the shutdown
mechanism.** A worker blocked in `recv()` (or `for v in ch`) unblocks when the
channel closes (`.None` / loop end) and returns. This is sufficient when a worker
waits on a single channel. Cancelling a worker that must wait on *several* things
at once (work *or* a shutdown signal) is what `select` is for — and `select` is
deferred (see Non-goals), so multi-way cancellation waits for it.

## Implementation

Mirrors `Task`: a builtin type + methods on the compiler side, lowered to
suspending intrinsics handled inside the scheduler. Channels live next to the
scheduler because `send`/`recv` manipulate its run queue — they cannot be written
in pure Twinkle without reintroducing polling.

### Runtime — channel object + scheduler integration (`tools/js_runtime/runtime.mjs`)

A channel is an object owned by the scheduler:

```js
{ capacity, buffer: [], sendQ: [], recvQ: [], closed: false }
```

- `sendQ`/`recvQ` are FIFO queues of parked tasks (`{ id, value?, resolve }`).
- `send`/`recv` are **Suspending** ops (like `suspend_await`): when they must
  block they push the task to the relevant queue with its promise unresolved; the
  counterpart op (or `close`) resolves it and re-enqueues the task via the
  existing `runnable`/`schedule()` path, preserving the one-task-per-microtask
  invariant. Immediate cases (receiver waiting / buffer space) resolve through the
  same path.
- `close` is synchronous: flips `closed`, wakes all parked tasks per the close
  rules above.
- Add a `blockedOnChannel` counter alongside `blockedOnTask` so an
  all-parked-on-channels-with-nothing-pending state surfaces as a deadlock via the
  existing `checkQuiescence`.

### Boundary encoding

New scheduler imports: `channel_new(cap)`, `channel_send(ch, v)`,
`channel_recv(ch)`, `channel_close(ch)`. Values cross as `anyref` (the compiler
boxes `T`, as it does for generic containers / `Task` results). `recv` returns an
extensible **tagged result** object — `{ kind: "value", value }` or
`{ kind: "closed" }` — rather than a carrier/`null` sentinel. A `null` sentinel
would be a tagged union in disguise (and would collide with values whose own
representation is null-ish, e.g. an `Option`'s `.None`); the tagged object avoids
both and grows cleanly if future ops need more kinds (`timeout`, `cancelled`). The
thin Twinkle wrapper maps `value` → `.Some(v)` and `closed` → `.None` for `recv`;
`send` returns the scheduler's delivered/closed boolean directly.

### Compiler (boot only)

- Register `Channel` as a builtin generic type (new builtin `TypeId`), plus
  builtin constructors `Channel.new` / `Channel.bounded` and methods
  `send` / `recv` / `close`, lowering to the intrinsics above (mirrors
  `Task.spawn` → `task_create`).
- Make `Channel` satisfy the existing **`IntoIterator`** contract so
  `for v in ch { }` lowers to a `recv`-until-`.None` loop, reusing the
  access-contracts machinery.

### Twinkle surface (thin wrapper)

A small module provides the `recv` → `Option` and `send` → `Bool` wrappers and the
`IntoIterator` satisfier. The concurrency stays in the runtime; the Twinkle side is
shape/marshaling only.

## Testing

Most behavior is observable from Twinkle, so the bulk lives in a new
`boot/tests/suites/channel_suite.tw` (driven through the real scheduler via
`Task`), plus a couple of JS-level checks for scheduler accounting:

- Unbuffered rendezvous: producer + consumer hand off a value; assert value +
  ordering.
- Bounded buffering & backpressure: sends up to capacity don't block; the
  (capacity+1)-th send parks and resumes only after a `recv` frees space.
- `for v in ch`: drains FIFO and ends exactly at close.
- Close: buffered values still drain after close, then `recv()` → `.None`; parked
  receivers get `.None`; parked senders get `false`; `send` after close → `false`;
  double `close` is a no-op.
- Fan-in/out: N producers, M consumers — every value received exactly once
  (work-queue distribution, not broadcast).
- `try` integration: an `Option`-returning fn using `v := try ch.recv()`.
- Tagged-result encoding: send/recv a `.None` (`Option`) and a `Void` value to
  prove the closed result is distinct from any real value.
- Deadlock detection: a `recv` with no possible sender and nothing else runnable
  surfaces the scheduler's deadlock error — an integration test (`target/twk run`
  a deadlocking `.tw`, expect nonzero exit + message), since asserting inline
  would hang.
- Regressions: existing `task_suite` (incl. the `Task.yield` starvation test)
  stays green; `make bundle-cli` reaches a self-host fixed point.

## Rollout (independently shippable phases)

1. **Core primitive** — runtime ops + intrinsics + compiler builtin + thin
   wrapper + `IntoIterator` + `channel_suite`. Self-host green. Ships channels.
   Boot + runtime only, no stage0.
2. **LSP migration (first real adopter)** — replace `ChunkQueue` /
   `DiagnosticsQueue` and the `time.sleep(1)` poll loops in
   `boot/commands/lsp.tw` with channels; drop `wait_for_dispatcher` and the
   `pending` counter; fold exit handling into channel close. Verify with an LSP
   stdio driver that sends a formatting request mid cold-analysis (the harness
   used to validate the diagnostics responsiveness work — latency stays low, poll
   loops gone) and the full boot suite incl. LSP suites. Validates the API on real
   code and delivers the idle-CPU / latency payoff.
3. **Docs / later** — `docs/API.md` channel section; revisit `select` /
   `recv_timeout` only if a concrete need appears.

## Primary risks & mitigations

- Parking/waking races → reuse the one-task-per-microtask discipline +
  `blockedOnChannel` accounting; heavy fan-in/out coverage.
- Boundary tagged-result encoding → the `.None`/`Void` distinctness tests.
- Self-host breakage → the `make bundle-cli` fixed-point gate.

## Related

- `tools/js_runtime/runtime.mjs` cooperative `Task` scheduler (the layer this
  builds on); see the `Task.yield` macrotask-hop fix (commit 6aca841) for the
  microtask-vs-macrotask invariant.
- `boot/commands/lsp.tw` (first adopter).
- Access contracts / `IntoIterator` (`for v in ch`).
