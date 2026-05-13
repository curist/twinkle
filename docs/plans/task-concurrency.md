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

Initial API:

```tw
type Task<T>

fn Task.spawn<T>(f: fn() T) Task<T>
fn Task.await<T>(task: Task<T>) T
fn Task.yield_now() Void
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

For the initial implementation, valid suspension points are runtime-controlled:

* task completion;
* `Task.yield_now()`;
* future host async operations, once added.

The exact fairness policy can start as FIFO runnable queue semantics.

### Await

`Task.await(task)` waits until `task` completes and returns its final value.
Because the MVP has no syntax-level async function marker, `await` should be
specified as a runtime/library operation with implementation-defined suspension
constraints. In practice, the compiler/runtime may initially restrict `await` to
places it can lower safely.

A conservative first implementation may support `await` from task bodies and the
main entry fiber/task only, then broaden support as lowering improves.

### Cells and shared state

`Cell<T>` remains valid in the single-threaded task model. Since there is no
preemption, cell updates are not interrupted by another task unless the update
function itself reaches a supported suspension point, which should be disallowed
for the MVP.

Rule for MVP:

> `Cell.update` callbacks must be non-suspending. They cannot call
> `Task.await`, `Task.yield_now`, or future suspending host APIs.

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

* task object with state: pending/running/done/trapped;
* result storage for completed tasks;
* FIFO runnable queue;
* `Task.spawn` to enqueue a closure;
* `Task.await` for already-completed tasks and scheduler-driven completion;
* entrypoint integration that drains the scheduler as needed.

This phase may run spawned tasks to completion unless they call a supported
runtime yield point.

### Phase 2: Cooperative yield points

Add `Task.yield_now()` as an explicit scheduler boundary. Lower supported task
bodies into resumable state machines or an equivalent trampoline representation.

Keep the first supported shape narrow if necessary. For example, support
`yield_now` only directly inside task bodies before allowing suspension through
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
* `Task.yield_now` and non-completed `await` require resumable lowering before
  being accepted generally.

Diagnostics should be explicit when code uses suspension in an unsupported
position.

### Runtime representation

A task object needs at least:

* state;
* closure or continuation;
* result slot;
* waiters/dependents, once `await` can suspend;
* trapped/error payload if the runtime records trap details.

The task queue must be visible to GC root marking.

## Stage0 and Boot Compiler Plan

The boot compiler is the primary implementation, but this feature touches the
language/runtime surface. Implement it in both compilers in staged form:

1. Add the spec/API documentation first.
2. Add `Task<T>` and builtin signatures to stage0 and boot so both frontends can
   parse, resolve, and type-check programs using the API.
3. Implement runtime/codegen behavior in boot first where possible.
4. Keep stage0 either behaviorally equivalent or explicitly limited to the
   subset needed to bootstrap and run parity tests.
5. Once boot uses any task API internally or tests depend on task execution,
   stage0 must support enough of the same behavior to rebuild the boot compiler.

In other words: yes, this should exist in both stage0 and boot for language
surface parity. The boot compiler can lead, but stage0 cannot be ignored if the
new builtins become part of bootstrapping, stdlib, or conformance tests.

## Open Questions

* Should `Task.await` be allowed at top level, or only inside task bodies?
* Should `Task.spawn` start eagerly immediately, or only when the scheduler is
  run/awaited? The proposed default is eager scheduling, cooperative execution.
* What result should `await` produce if a task traps? Trap again, or return a
  runtime error object? The proposed default is to trap.
* How much suspension should the first backend support: task-body-only or nested
  function calls as well?
* Should cancellation exist in the initial API, or wait for structured
  concurrency?
* Should `Task.yield_now()` be part of MVP, or should MVP start with spawn/await
  only and no explicit yield?
