# Task / Channel API Maturation Plan

> **For agentic workers:** Only WS-E and WS-F remain active. Both are
> design-first workstreams: write and review the design before creating an
> implementation plan. They can be developed independently except where WS-F
> explicitly depends on decisions from WS-E.

## Goal

Complete Twinkle's cooperative-concurrency API by designing the two capabilities
that the current surface cannot express:

- wait for whichever of several channel operations becomes ready (`select`);
- manage related task lifetimes, joins, cancellation, and timeouts without
  hand-written coordination.

The earlier API and inference work that enabled this design is complete and
recorded below.

## Current Baseline

`Task<T>` and `Channel<T>` are compiler-recognized intrinsics backed by the
cooperative scheduler in `tools/js_runtime/runtime.mjs`. Tasks run on one program
thread and switch at explicit task points or task-aware host operations. Values
are immutable, so cooperative tasks do not introduce data races through shared
mutation.

The current public surface is:

```tw
Task.spawn<T>(fn() T) Task<T>
task.await<T>() T
task.try_await<T>() Result<T, String>
Task.yield() Void

Channel.new<T>() Channel<T>
Channel.bounded<T>(capacity: Int) Channel<T>
ch.send<T>(value: T) Result<Void, SendError>
ch.recv<T>() T?
ch.close<T>() Void
```

`SendError` is `{ Closed }`. A channel can also be drained with `for v in ch`;
iteration ends once it is closed and drained. The scheduler detects deadlock and
surfaces unobserved task failures instead of silently hanging or dropping them.

### Implementation layers

- Twinkle-visible signatures live in `boot/prelude/signatures/task.tw` and
  `boot/prelude/signatures/channel.tw`; channel iteration lives in
  `boot/prelude/channel.tw`. Generated copies are embedded in
  `boot/lib/module/core_lib.tw`.
- The boot compiler recognizes the builtin types and lowers their operations to
  runtime imports. Stage0 has the corresponding compile-time support needed to
  bootstrap boot source that uses channels.
- `tools/js_runtime/runtime.mjs` implements the JSPI scheduler, task operations,
  channels, and non-JSPI fail-on-use imports.

### Constraints for future primitives

- Any prelude or compiler change must survive the self-host fixed point with
  `make stage2` and keep `make boot-test` green.
- A new primitive needs stage0 compiler support only when source reachable from
  `boot/main.tw` adopts it. Tests, stdlib code outside that graph, and the
  playground do not by themselves create a stage0 dependency.
- Runtime intrinsics need both the JSPI implementation and a non-JSPI
  fail-on-use import. Preserve the Safari extern-metadata behavior documented in
  `runtime.mjs` when adding imports.
- Run heavyweight verification sequentially: `make boot-test`, `make stage2`,
  then `make bundle-cli`.
- Format and lint every edited `.tw` file with `target/twk fmt` and
  `target/twk lint`.

## Completed Foundation

The following workstreams have shipped. Their implementation and canonical API
semantics now live in the compiler, tests, `docs/API.md`, and `docs/spec.md`.

### WS-A: Document `Channel`

Completed in `66e88f6d` (`docs(api): document Channel in the API reference`).
The API reference now covers construction, backpressure, close and drain
behavior, iteration, and typed send failure.

### WS-B: Preserve inferred-return constraints

Completed in `f9be5362` (`fix(check): preserve inferred return constraints`).
The checker now preserves substitution links when an inferred function return
flows through a closure into a generic call. This fixes the original
`Task<Void>` ambiguity and prevents an earlier concrete constraint from being
silently overwritten.

### WS-C: Recover task failure with `try_await`

Completed in `bc3558f4` (`feat(task): add recoverable task awaiting`).
`task.try_await()` returns `Result<T, String>` and converts any failure raised by
the spawned task—including explicit errors and runtime traps—into an error
message. Plain `await` retains its trap-propagating behavior.

### WS-D: Return typed send failures

Completed in `d2dcdc18` (`feat(channel): return typed send failures`).
`ch.send(value)` now returns `Result<Void, SendError>`. `.Err(.Closed)` means the
value was not delivered; `.Ok({})` means the send completed, without promising
that a receiver remains alive afterward. The caller retains the immutable value
in either case.

## Remaining Work

| Order | Workstream | Deliverable | Dependency |
|-------|------------|-------------|------------|
| 1 | WS-E: `select` over channels | Reviewed design, then a separate implementation plan | None |
| 2 | WS-F: Structured concurrency and cancellation | Reviewed design, then a separate implementation plan | Incorporate relevant WS-E decisions |

## WS-E: `select` over channels

### Why

There is no way to wait for whichever of several channels becomes ready or to
attempt a non-blocking receive. This prevents direct multiplexing and makes
timeouts and cancellation awkward to compose.

This is a design workstream. Do not write implementation tasks until the API and
scheduler design has been reviewed.

### Questions the design must settle

- **Surface:** choose between syntax such as `select { ... }` and a library or
  intrinsic form. Account for parser, checker, lowering, type inference, and the
  difference between homogeneous and heterogeneous channel arms.
- **Operations:** decide whether v1 supports receive arms only or both send and
  receive arms. If send arms are deferred, state that explicitly.
- **Non-blocking behavior:** define a default arm or a separate try-receive
  operation, including its return shape.
- **Closed channels:** specify whether a closed-and-drained receive arm is
  immediately ready with `.None`. For send arms, specify whether a closed
  channel is ready and produces `.Err(.Closed)` or is excluded.
- **Duplicate channels:** define behavior when more than one arm refers to the
  same channel, including two receives or a send and receive on one channel.
- **Fairness and ordering:** define which arm wins when several are ready and
  how selection interacts with the existing FIFO order of parked channel
  waiters. Avoid starvation without making deterministic testing impractical.
- **Timeouts:** choose composition with a timer channel or a first-class timeout
  clause. If composition is preferred, include the required timer primitive in
  the design scope rather than assuming it exists.
- **Scheduler protocol:** describe multi-channel waiter registration, atomic
  winner selection, deregistration of losing arms, close wakeups, and accurate
  blocked-task accounting. Cover races caused by immediate readiness during
  registration even though execution is single-threaded.
- **Bootstrap boundary:** keep the primitive out of boot compiler and command
  source initially if practical; otherwise include the necessary stage0
  typechecking and lowering work.

### Deliverables

- [ ] Write `docs/plans/select-design.md` (or a design document under
  `docs/design/`) with chosen semantics, rationale, worked examples, type rules,
  and the runtime waiter-registration protocol.
- [ ] Review the design with the API owner.
- [ ] After approval, create a separate implementation plan covering the parser
  or intrinsic surface, checker, lowering, runtime, tests, documentation, and
  any demonstrated stage0 requirement.

## WS-F: Structured concurrency and cancellation

### Why

There is no task group, cancellation mechanism, timeout, or structured scope
that accounts for its children on exit. Fan-in code therefore needs a manual
closer task, and missing that coordination produces a detected deadlock.

This is a design workstream. It should follow WS-E far enough to reuse settled
selection and timeout semantics, but joins and task-group failure policy can be
designed independently.

### Questions the design must settle

- **Task groups and join policy:** decide whether the basic abstraction is
  `join_all`, a task group, a lexical scope, or a small combination. Define
  result ordering, homogeneous versus heterogeneous children, and whether
  failure is fail-fast or accumulated. Provide both trapping and recoverable
  behavior only if each has a clear use case.
- **Scope exit:** specify behavior on normal completion, `return`, `try` early
  return, and failure. Define whether remaining children are joined, cancelled,
  or allowed to outlive the scope.
- **Cancellation:** use cooperative cancellation and define exactly where it is
  observed. Decide whether blocked `await`, `recv`, `send`, sleeps, and host I/O
  are interruptible and what result each operation produces when cancelled.
- **Channel lifecycle and ownership:** explain how a task group knows when a
  result channel should close. A plain `join_all` cannot replace the closer task
  when bounded producers must run concurrently with a draining consumer. Decide
  whether closure remains explicit, task groups own resources, or the API gains
  sender/receiver ownership or completion-aware receive semantics.
- **Failure propagation:** define how child failure interacts with sibling
  cancellation, `try_await`, unobserved-failure reporting, and failures raised
  while cleaning up a scope.
- **Timeouts:** define timeout as cancellation triggered by WS-E/timer
  composition or as a task-group operation, including cleanup of the losing
  task or timer.
- **Deadlock diagnostics:** decide whether group-aware diagnostics should add
  task/group relationships to the existing scheduler trap.
- **Resource cleanup:** specify whether cancelled tasks run `defer` blocks and
  how cleanup is bounded if a child never reaches another cooperative task
  point.

### Deliverables

- [ ] Write `docs/plans/structured-concurrency-design.md` with chosen semantics,
  rationale, and worked examples that safely replace the playground's manual
  closer-task pattern.
- [ ] Review the design with the API owner.
- [ ] After approval, create a separate implementation plan covering compiler,
  runtime, bootstrap, tests, diagnostics, and documentation as required by the
  chosen model.

## Lifecycle

Once both design workstreams have moved into reviewed implementation plans,
archive this umbrella document and remove its row from `docs/plans/README.md`.
Each implementation plan should be tracked independently from that point.
