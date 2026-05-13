# Task Concurrency

## Goal

Add a library-first, single-threaded cooperative concurrency model for Twinkle
without adding new syntax. The public abstraction is `Task<T>`: a computation
scheduled by the runtime that eventually produces one value. Recoverable failure
stays explicit in the value type, typically `Task<Result<T, E>>`.

The MVP should preserve Twinkle's current value model:

* ordinary values remain immutable and safe to share between tasks;
* rebinding remains local and does not imply mutation;
* `Cell<T>` remains the explicit escape hatch for shared mutable state;
* no preemption, parallelism, or data-race model is introduced.

## Non-goals

* No `async`, `await`, `spawn`, or `yield` syntax in the MVP.
* No Lua-style public `Fiber.resume` / `Fiber.yield` API.
* No parallel workers or shared-memory threading.
* No implicit exception-like task failure channel. Use `Result` for recoverable
  errors and traps for unrecoverable errors.
* No sendability/ownership system in the first version.

## User-facing API Shape

MVP API (Phase 1):

```tw
type Task<T>

fn Task.spawn<T>(f: fn() T) Task<T>
fn Task.await<T>(task: Task<T>) T
```

Phase 2 adds:

```tw
fn Task.yield() Void
```

The primary style is dot-call based:

```tw
task := Task.spawn(fn() Int {
  expensive_work()
})

value := task.await()
```

Recoverable errors are encoded explicitly:

```tw
fn fetch_user(id: Int) Task<Result<User, String>> {
  Task.spawn(fn() Result<User, String> {
    // ...
  })
}

user := try fetch_user(42).await()
```

If a task body traps, that is an unrecoverable trap, consistent with ordinary
Twinkle execution. APIs that need recoverable failure should return `Result`.

## Semantics

### Scheduling

Tasks are cooperative and single-threaded. Only one task runs at a time. The
scheduler never preempts a running task. A task gives control back to the
scheduler only at defined suspension points.

Suspension points are introduced across phases:

* Phase 1: task completion; `Task.await` synchronously drives the scheduler
  (no continuation saving required).
* Phase 2: `Task.yield()` re-enqueues the current task; suspending
  `Task.await` saves the current continuation and parks it as a waiter until
  the awaited task completes.
* Future: host async operations that complete via callbacks.

The exact fairness policy can start as FIFO runnable queue semantics.

### Await

`Task.await(task)` waits until `task` completes and returns its final value.
Because the MVP has no syntax-level async function marker, `await` should be
specified as a runtime/library operation with implementation-defined suspension
constraints. In practice, the compiler/runtime may initially restrict `await` to
places it can lower safely.

Phase 1 `Task.await` may synchronously drive the scheduler: run queued tasks
until the awaited task completes, then return the result. This does not require
saving the current continuation. Phase 2 `Task.await` may suspend the current
task and resume it later via the state-machine transform.

### Cells and shared state

`Cell<T>` remains valid in the single-threaded task model. Since there is no
preemption, cell updates are not interrupted by another task unless the update
function itself reaches a supported suspension point, which should be disallowed
for the MVP.

Rule for MVP:

> `Cell.update` callbacks must be non-suspending. They cannot call
> `Task.await`, `Task.yield`, or future suspending host APIs.

This avoids reentrancy surprises and keeps `Cell.update` atomic with respect to
the cooperative scheduler.

### Closures

Tasks use ordinary Twinkle closure capture semantics. A task body captures values
at the point where the closure is created. Capturing a `Cell<T>` captures the
cell reference, so aliases observe updates in the same way they do today.

Example:

```tw
x := 1
task := Task.spawn(fn() Int { x })
x = 2

task.await() // 1
```

## Why Task Instead of Fiber

`Task<T>` is the higher-level concurrency abstraction. It says: run this
computation and eventually produce a value. That fits Twinkle's current surface:
small APIs, explicit effects, immutable ordinary values, and no new syntax.

A Lua-style fiber API is more powerful but lower-level. It exposes manual
`resume`/`yield` control transfer, yielded intermediate values, fiber states,
and scheduler design. It is also a poorer direct fit for the Wasm GC backend,
because stackful fibers require saving and restoring execution state that the
Wasm engine normally owns.

`Task<T>` keeps the implementation flexible. It can later be backed by an
internal fiber system, state-machine lowering, host promises, or Wasm stack
switching without committing the public API to raw coroutine semantics.

## Implementation Strategy

### Phase 1: Runtime task queue, no general suspension

Add the builtin `Task<T>` type and basic runtime structures:

* task object with state: pending/running/done (trapped state reserved for
  future phases with Wasm exception handling support);
* result storage for completed tasks;
* FIFO runnable queue;
* `Task.spawn` to enqueue a closure;
* `Task.await` for already-completed tasks and scheduler-driven completion;
* entrypoint integration that drains the scheduler as needed.

In Phase 1, spawned tasks run to completion when scheduled. Suspension points
other than completion are introduced in later phases.

### Phase 2: Cooperative yield points

Add `Task.yield()` as an explicit scheduler boundary. Lower supported task
bodies into intraprocedural state machines and run them through the scheduler's
trampoline loop.

Keep the first supported shape narrow: support `yield` and suspending
`await` only directly inside task bodies before allowing suspension through
arbitrary nested calls.

### Phase 3: Host async integration

Model host async operations as tasks or task-producing functions. For example,
future filesystem/network APIs can return `Task<Result<T, E>>` and complete via
host callbacks that re-enqueue waiting tasks.

### Phase 4: Structured helpers

Once basic tasks are stable, add library helpers rather than syntax:

```tw
fn Task.all<A, B>(a: Task<A>, b: Task<B>) Task<.{ first: A, second: B }>
fn Task.race<T>(a: Task<T>, b: Task<T>) Task<T>
```

A scoped task API can be considered later if unstructured task lifetimes become
a problem.

## Compiler Work

### Type checker and builtins

Both compilers need a builtin nominal `Task<T>` type and builtin functions for
the public API. Method resolution should make these available as inherent
methods:

```tw
task.await()
```

No grammar change is required for the MVP.

### Lowering and codegen

The hard part is suspension. The implementation should avoid promising arbitrary
stack suspension until the backend can support it.

Initial lowering can be deliberately limited:

* `Task.spawn(fn() T { ... })` accepts a closure and constructs a task object;
* task bodies may initially run to completion;
* `Task.yield` and Phase 2-style suspending `await` require resumable lowering
  before being accepted generally.

Diagnostics should be explicit when code uses suspension in an unsupported
position.

### Suspension lowering strategy

Use a trampoline runtime from the start, but keep the first resumable compiler
transform as an intraprocedural state machine. This avoids depending on stackful
Wasm suspension and keeps ordinary Twinkle functions as ordinary Wasm calls.

A suspendable task body is lowered to a task frame plus a generated resume
function. The frame stores the program counter and locals live across suspension
points. The scheduler repeatedly invokes resume functions until each task
completes, traps, or returns `Suspend`.

Conceptually:

```text
TaskFrame {
  pc: Int
  // hoisted locals live across suspension points
}

resume(frame):
  switch frame.pc:
    0:
      ...
      frame.pc = 1
      return Suspend
    1:
      ...
      return Complete(value)
```

`Task.yield()` saves the next program counter, stores live locals in the
frame, returns `Suspend`, and lets the scheduler re-enqueue the task.
`Task.await(other)` checks whether `other` is complete; if so, it reads the
result and continues. If not, it stores the current task as a waiter on `other`,
saves the continuation state, and returns `Suspend`.

For Phase 2, suspension is task-body-only:

* only closures passed directly to `Task.spawn` are eligible for resumable
  lowering;
* `Task.yield()` is accepted only directly in those task bodies;
* `Task.await()` may suspend only in top-level code or directly in those task
  bodies;
* ordinary functions called from a task body are non-suspending.

A validation pass should classify calls to known suspending operations and reject
them when they appear in unsupported positions, including inside `Cell.update`
callbacks. Diagnostics should explain that the operation would require
suspension through an ordinary call frame.

General CPS lowering is deferred until Twinkle needs suspension through nested
calls. At that point, add a suspending-function classification/effect and either
CPS-transform those functions or extend the state-machine transform across call
boundaries. Wasm stack switching can still replace the implementation later, but
the public `Task<T>` API should not depend on it.

### Runtime representation

A task object needs at least:

* state;
* closure or continuation;
* result slot;
* waiters/dependents, once `await` can suspend.

The task queue must be visible to GC root marking.

## Stage0 and Boot Compiler Plan

The boot compiler is the primary implementation. Stage0 (Rust) is a compiler,
not an interpreter — it does not need to execute tasks itself. It needs to:

1. **Type-check** `Task<T>` and the builtin function signatures.
2. **Emit valid Wasm** for task operations (constructing task objects, calling
   runtime scheduling functions, etc.).

The actual task scheduling happens in the emitted Wasm, executed by the Node.js
runtime. This means stage0 needs frontend and codegen support for tasks, but not
its own in-process task scheduler — the same work it already does for every
other builtin type.

Implementation order:

1. Add the spec/API documentation first.
2. Add `Task<T>` and builtin signatures to both stage0 and boot so both
   frontends can parse, resolve, and type-check programs using the API.
3. Implement runtime/codegen behavior in boot first.
4. Add codegen support to stage0 so it emits valid Wasm for task operations.
   Stage0 does not need to replicate the scheduler — it just needs to emit code
   that calls the same runtime functions the boot compiler targets.
5. If the boot compiler's own source starts using tasks, stage0 must be able to
   compile that code to valid Wasm. Since the runtime is in the emitted Wasm
   (not in stage0 itself), this is a codegen task, not a runtime task.
6. If Phase 2 suspension lowering (state-machine transform) is needed for boot
   compiler source, stage0 must implement the same transform. Until then, stage0
   can reject `Task.yield` and suspending `await` while supporting Phase 1
   run-to-completion task operations.

## Design Decisions

* **`Task.await` at top level:** Yes. The main module is implicitly a task, so
  `await` works everywhere including top-level code. Restricting it to task
  bodies would make results impossible to consume without a separate "run the
  scheduler" entrypoint.

* **Eager vs deferred spawn:** Eager enqueue. `Task.spawn` adds the task to the
  runnable queue immediately, but it does not run until the current task reaches
  a yield or await point, preserving cooperative semantics.

* **Trap propagation:** In Phase 1 and 2, a task body trap aborts execution
  immediately like any ordinary Wasm trap. A future trap-catching implementation
  (via Wasm exception handling) may record a `trapped` task state and make
  `Task.await` re-trap the awaiter. A `Task.try_await` returning `Result` can
  be considered at that point.

* **Suspension depth in Phase 2:** Task-body-only first. `Task.yield` is
  supported directly inside task bodies. Suspension through arbitrary nested
  calls requires CPS transform or Wasm stack-switching support and is deferred
  to a later phase.

* **Cancellation:** Deferred to a future structured concurrency phase. The
  initial API has no cancellation mechanism.

* **`Task.yield` in MVP:** Deferred. Phase 1 (MVP) ships spawn/await only.
  `Task.yield` arrives in Phase 2 alongside resumable lowering.

* **Task identity:** Reference/pointer equality. Two task values are equal iff
  they refer to the same task object. Equality and hash support, if exposed to
  collections, use identity semantics (pointer-based). This is sufficient for
  `Task.race`-style APIs that need to identify which task completed.

* **Phase 1 value:** Phase 1 tasks run to completion when scheduled; `spawn`
  only enqueues them. `Task.spawn` followed by `task.await()` is operationally
  equivalent to calling the function directly. The value of Phase 1 is
  type-system and runtime scaffolding: it establishes `Task<T>` as a builtin
  type, wires up the scheduler data structures, and lets user code adopt the
  task API before suspension is available. Real concurrency begins in Phase 2.

* **Top-level await model:** Top-level module code is wrapped in an implicit
  root task (or root scheduler frame), so `Task.await` can drive the scheduler
  from top-level code without special syntax. In Phase 2, the root task uses
  the same resumable lowering rules as spawned task bodies if top-level `await`
  needs to suspend.

* **Unawaited tasks:** When top-level code completes, the runtime drains
  runnable tasks until the queue is empty or a deadlock is detected. This makes
  eagerly spawned tasks run even if their result is not awaited. A spawned task
  is a commitment to execute, not a lazy thunk.

* **Await cycles / self-await:** Awaiting a task that cannot make progress (self-
  await, mutual cycles) is a runtime deadlock. The scheduler should trap when it
  has no runnable tasks but outstanding awaits remain.

