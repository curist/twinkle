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

## API surface

`Channel<T>` is a compiler-registered builtin reference type (same family as
`Task`, `Dict`, `Cell`); no import needed.

```tw
// Construction
Channel.new<T>() Channel<T>                  // unbuffered (rendezvous)
Channel.bounded<T>(capacity: Int) Channel<T> // buffered, capacity >= 1

// Send — blocks per the backpressure rules below
ch.send(v)        Result<Void, SendError>    // .Err(.Closed) if channel is closed

// Receive
ch.recv()         Result<T, RecvError>       // .Err(.Closed) once closed AND drained
for v in ch { }                              // drains until closed (IntoIterator)

// Close — idempotent, no-op if already closed
ch.close()

type SendError = { Closed }
type RecvError = { Closed }
```

Rationale:

- **Unbuffered + bounded** (Go's model): rendezvous handoff and a fixed-capacity
  buffer with natural backpressure (send blocks when full). Two constructors
  rather than a single `new(capacity)` with a magic `0`. `capacity < 1` traps.
- **`Result`-based `recv`/`send`**: explicit and non-trapping. Verbosity is a
  non-issue because the common consumption path is `for v in ch { }` (no
  per-recv unwrapping — the closed `Err` is just loop termination), propagation
  is `v := try ch.recv()`, and the prelude already provides the full unwrap
  family (`unwrap_or`, `unwrap_or_else`, `ok()` → `Option`, `map`, `and_then`,
  …). (`send` → `Bool` is the fallback if a must-use-`Result` lint makes the
  `Result` noisy.)
- **Single `Channel<T>` handle**; multiple producers/consumers allowed
  (fan-in/out falls out of the wait-queue design).

## Semantics

**Unbuffered (`Channel.new`)** — rendezvous:

- `send(v)`: receiver parked → hand `v` over, wake it, return `.Ok`. Else park
  the sender (holding `v`) until a receiver arrives or close.
- `recv()`: sender parked → take its `v`, wake it (its `send` returns `.Ok`),
  return `.Ok(v)`. Else closed → `.Err(.Closed)`. Else park the receiver.

**Bounded (`Channel.bounded(n)`, n ≥ 1)** — buffer with backpressure:

- `send(v)`: closed → `.Err(.Closed)`; receiver parked (buffer empty) → hand off
  directly; buffer has space → enqueue, return `.Ok` (no block); buffer full →
  park the sender (holding `v`) until space frees or close.
- `recv()`: buffer non-empty → dequeue front; if a sender was parked on a full
  buffer, move its value into the tail and wake it; return `.Ok(v)`. Empty +
  closed → `.Err(.Closed)`. Empty + open → park the receiver.
- Invariant: never parked receivers with a non-empty buffer, nor parked senders
  with a non-full buffer.

**Close (`close()`):**

- Idempotent — a second close is a no-op (non-trapping).
- Wakes **all** parked receivers: they drain remaining buffered values first
  (`.Ok` each), then further `recv()` returns `.Err(.Closed)`. Buffered values
  survive close (drain-then-closed, like Go).
- Wakes **all** parked senders: their `send` returns `.Err(.Closed)`; the pending
  value is dropped.
- `send` after close → `.Err(.Closed)`.

**Ordering & fairness:** FIFO throughout — values received in send order; parked
senders and receivers each woken FIFO; one value to exactly one receiver (no
duplication).

**Blocking = cooperative suspension, not spinning:** "block" means the `Task`
parks via the scheduler (same Suspending machinery as `await`/`sleep`); the host
event loop stays free. This is what removes the LSP poll loops.

**Deadlock detection:** a task parked on a channel counts as blocked. If the
scheduler goes quiescent with all remaining tasks parked on channels and nothing
pending (no runnable, no host I/O/timers), that surfaces as a deadlock —
extending the existing `blockedOnTask`/`checkQuiescence` accounting with a
`blockedOnChannel` counter.

**Edge cases:** `recv()` on a forever-empty open channel parks indefinitely (as
in Go), caught by deadlock detection only if the whole scheduler is otherwise
idle. `Channel.bounded(n)` with `n < 1` traps.

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
boxes `T`, as it does for generic containers / `Task` results). `recv` signals
closed-and-drained with a distinct runtime **carrier**: a non-null carrier wraps
a real value; `null` means closed. The carrier (not bare-null) avoids colliding
with values whose own representation is null-ish (e.g. an `Option`'s `.None`). A
thin Twinkle wrapper turns the carrier/null into the `Result`.

### Compiler (boot only)

- Register `Channel` as a builtin generic type (new builtin `TypeId`), plus
  builtin constructors `Channel.new` / `Channel.bounded` and methods
  `send` / `recv` / `close`, lowering to the intrinsics above (mirrors
  `Task.spawn` → `task_create`).
- Make `Channel` satisfy the existing **`IntoIterator`** contract so
  `for v in ch { }` lowers to a `recv`-until-`.Closed` loop, reusing the
  access-contracts machinery.

### Twinkle surface (thin wrapper)

A small module provides the `Result`-building wrappers, the
`SendError`/`RecvError` enums, and the `IntoIterator` satisfier. The concurrency
stays in the runtime; the Twinkle side is shape/marshaling only.

## Testing

Most behavior is observable from Twinkle, so the bulk lives in a new
`boot/tests/suites/channel_suite.tw` (driven through the real scheduler via
`Task`), plus a couple of JS-level checks for scheduler accounting:

- Unbuffered rendezvous: producer + consumer hand off a value; assert value +
  ordering.
- Bounded buffering & backpressure: sends up to capacity don't block; the
  (capacity+1)-th send parks and resumes only after a `recv` frees space.
- `for v in ch`: drains FIFO and ends exactly at close.
- Close: buffered values still drain after close, then `.Err(.Closed)`; parked
  receivers get `.Closed`; parked senders get `.Err(.Closed)`; `send` after close
  → `.Err(.Closed)`; double `close` is a no-op.
- Fan-in/out: N producers, M consumers — every value received exactly once.
- `try` integration: a `Result`-returning fn using `v := try ch.recv()`.
- Carrier encoding: send/recv a `.None` and a `Void` value to prove the closed
  sentinel never collides with a real value.
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
- Boundary carrier encoding → the `.None`/`Void` carrier tests.
- Self-host breakage → the `make bundle-cli` fixed-point gate.

## Related

- `tools/js_runtime/runtime.mjs` cooperative `Task` scheduler (the layer this
  builds on); see the `Task.yield` macrotask-hop fix (commit 6aca841) for the
  microtask-vs-macrotask invariant.
- `boot/commands/lsp.tw` (first adopter).
- Access contracts / `IntoIterator` (`for v in ch`).
