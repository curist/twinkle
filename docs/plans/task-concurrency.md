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

* Phase 1: no scheduler; `Task.spawn` calls the closure synchronously and
  wraps the result. `Task.await` unwraps it (always already complete).
* Phase 2: introduces the scheduler, FIFO runnable queue, `Task.yield()`,
  and suspending `Task.await` via a deliberately narrow straight-line
  state-machine transform.
* Phase 3: broadens transformed task bodies to support ordinary local control
  flow such as branches, pattern matches, and loops.
* Future: host async operations that complete via callbacks.

The exact fairness policy can start as FIFO runnable queue semantics.

### Await

`Task.await(task)` waits until `task` completes and returns its final value.
Because the MVP has no syntax-level async function marker, `await` should be
specified as a runtime/library operation with implementation-defined suspension
constraints. In practice, the compiler/runtime may initially restrict `await` to
places it can lower safely.

Phase 1 `Task.await` simply unwraps the result (the task is always already
complete). Phase 2 introduces two lowering modes: top-level `Task.await`
synchronously drives the scheduler via `scheduler_run_until`; `Task.await`
inside a transformed task body suspends the current task and parks it as a
waiter via the state-machine transform.

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

### Phase 1: Type scaffolding, synchronous execution ✓ complete

Add the builtin `Task<T>` type and basic intrinsics with the simplest
possible runtime behavior:

* `Task.spawn(f)` calls `f()` synchronously and wraps the result.
* `Task.await(task)` unwraps the result (always already complete).
* No scheduler, no queue, no task state machine.

Phase 1 is operationally equivalent to calling the function directly.
Its value is establishing `Task<T>` as a builtin type, wiring up method
resolution and codegen paths, and letting user code adopt the task API
before real scheduling exists. Suspension points and the runtime scheduler
are introduced in Phase 2.

### Phase 2: Cooperative yield points ✓ complete

Add `Task.yield()` as an explicit scheduler boundary. Lower supported task
bodies into intraprocedural state machines and run them through the scheduler's
trampoline loop.

Keep the first supported shape narrow: support `yield` and suspending
`await` only in straight-line task bodies. Phase 3 extends this to local control
flow inside task bodies; suspension through arbitrary nested calls remains a
later feature.

### Phase 3: Broader task-body control flow

Make transformed task bodies useful for ordinary cooperative programs by
supporting suspension inside local control flow:

* `Task.yield` and suspending `Task.await` inside `if` branches;
* `Task.yield` and suspending `Task.await` inside `case` arms;
* suspension inside `for` loops, including condition-style `for cond { ... }`
  loops, with loop state hoisted into the task frame;
* nested local control flow within a task body.

This phase is about making `Task<T>` broadly useful without adding suspension
through arbitrary function calls. Ordinary functions called from a task body
remain non-suspending until a later CPS/state-machine-across-calls phase or a
Wasm stack-switching implementation exists.

Phase 3 also adds a top-level `Task.yield()` scheduler-pump convenience. In
that context, top-level code is still not transformed into a task: `Task.yield()`
runs at most one scheduler turn and then returns. If no runnable tasks exist, it
returns `Void`; if blocked tasks remain and no progress is possible, it traps
with a deadlock error.

### Phase 4: Host async integration

Model host async operations as tasks or task-producing functions. For example,
future filesystem/network APIs can return `Task<Result<T, E>>` and complete via
host callbacks that re-enqueue waiting tasks.

### Phase 5: Structured helpers

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

Phase 3 extends this transform to local control flow inside task bodies
(branches, pattern matches, and loops). General CPS lowering is deferred until
Twinkle needs suspension through nested calls. At that point, add a
suspending-function classification/effect and either CPS-transform those
functions or extend the state-machine transform across call boundaries. Wasm
stack switching can still replace the implementation later, but the public
`Task<T>` API should not depend on it.

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

In Phase 1, stage0 lowers `Task.spawn`/`Task.await` synchronously (call
closure, wrap/unwrap result). In Phase 2, stage0 must either emit calls to
the scheduler runtime helpers and implement the state-machine transform, or
reject Phase 2 task features (`Task.yield`, suspending `Task.await`) until
implemented. Stage0 only needs Phase 2 support if the boot compiler's own
source starts using suspension features.

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

* **`Task.await` at top level:** Yes. Top-level code acts as the scheduler
  root — it is not itself a task, but it can drive the scheduler via
  `scheduler_run_until` when awaiting a task. Restricting `await` to task
  bodies would make results impossible to consume without a separate "run the
  scheduler" entrypoint.

* **Eager vs deferred spawn:** Phase 1 calls synchronously (no queue). From
  Phase 2 onward, eager enqueue: `Task.spawn` adds the task to the runnable
  queue immediately, but it does not run until the current task reaches a
  yield or await point (or top-level code drains the scheduler), preserving
  cooperative semantics.

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

* **Phase 1 value:** Phase 1 `Task.spawn` calls the closure synchronously and
  wraps the result; `Task.await` unwraps it. This is operationally equivalent
  to calling the function directly. The value of Phase 1 is type-system and
  API scaffolding: it establishes `Task<T>` as a builtin type, wires up
  method resolution and codegen, and lets user code adopt the task API before
  scheduling exists. Real concurrency begins in Phase 2.

* **Top-level await model:** Top-level module code acts as the scheduler root.
  It is ordinary Wasm code (the module init function), not a transformed task.
  `Task.await` in top-level code calls `scheduler_run_until(target)`, which
  blocks the init function until the target completes. This is a synchronous,
  scheduler-driving operation — not a suspending await. In a future phase,
  top-level code could be transformed into a resumable task if needed, but
  Phase 2 does not require this.

* **Top-level yield model:** Phase 2 rejects `Task.yield()` in top-level module
  code. Phase 3 adds it as a scheduler-pump convenience rather than a true
  suspension point: run at most one scheduler turn, return immediately if no
  runnable tasks exist, and trap only when blocked tasks remain with no possible
  progress.

* **Unawaited tasks:** When top-level code completes, the runtime drains
  runnable tasks until the queue is empty or a deadlock is detected. This makes
  eagerly spawned tasks run even if their result is not awaited. A spawned task
  is a commitment to execute, not a lazy thunk.

* **Await cycles / self-await:** Awaiting a task that cannot make progress (self-
  await, mutual cycles) is a runtime deadlock. The scheduler should trap when it
  has no runnable tasks but outstanding awaits remain.

## Phase 2 Detailed Design: Straight-Line State-Machine Transform

This section specifies the compiler transform, runtime representation, and
scheduler for cooperative yield points. Phase 2 adds `Task.yield()` and makes
`Task.await` suspending, but only for straight-line task-body code — suspension
inside branches, pattern matches, loops, and arbitrary nested calls is deferred.

### Runtime Representation

#### Task states

Tasks have four distinct lifecycle states:

```
RUNNABLE = 0   task is in the scheduler queue, ready to be picked up
RUNNING  = 1   task is currently executing its resume function
BLOCKED  = 2   task is waiting on another task (not in the queue)
DONE     = 3   task has completed; result is available
```

State transitions:

```
spawn        → RUNNABLE (enqueue)
dequeue      → RUNNING
yield        → RUNNABLE (re-enqueue)
await (fast) → RUNNING  (no state change; target already DONE)
await (slow) → BLOCKED  (register as waiter; do NOT enqueue)
target done  → RUNNABLE (wake waiter; enqueue)
complete     → DONE
```

Separating RUNNABLE and BLOCKED makes scheduler invariants unambiguous:

- RUNNABLE tasks are always in the queue; BLOCKED tasks are never in the queue.
- Setting `state = RUNNABLE` and enqueueing are a single atomic scheduler
  operation. Never enqueue a task that is already RUNNABLE. This prevents
  duplicate queue entries from runtime bugs.

#### Task object

Replace the Phase 1 1-element-array representation with a proper GC struct:

```wasm
(type $rt_task (struct
  (field $state   (mut i32))          ;; RUNNABLE=0, RUNNING=1, BLOCKED=2, DONE=3
  (field $frame   (mut anyref))       ;; task-specific frame struct (null when done)
  (field $resume  (ref $rt_resume_fn)) ;; resume function
  (field $result  (mut anyref))       ;; boxed result value (set on completion)
  (field $waiters (mut (ref null $rt_types__Array)))  ;; tasks waiting on this one
))
```

Fields:
- **state**: lifecycle tag (see state transitions above).
- **frame**: per-task-body frame struct (see below). The resume function casts
  this to the concrete frame type. Set to null after completion to allow GC
  to collect the frame.
- **resume**: funcref to the generated resume function for this task body.
- **result**: boxed result value, written by the resume function before
  returning `COMPLETE`. See "Result boxing" below for how primitives are
  boxed.
- **waiters**: array of `$rt_task` refs. Initialized to null (no waiters).
  The `append_waiter` helper allocates a fresh array on first use, then
  appends to it on subsequent waiter registrations. Set to null after
  completion (waiters have been woken and enqueued).

Task identity (reference equality) is preserved: a task is a single GC
object that persists across all state transitions. User code holding a
`Task<T>` reference always points to the same object regardless of state.

#### Resume function signature

```wasm
(type $rt_resume_fn (func (param (ref $rt_task)) (result i32)))
```

Returns an action code:
- `0` = **COMPLETE** — task finished; result stored in `task.result`.
- `1` = **YIELD** — re-enqueue as RUNNABLE.
- `2` = **AWAIT** — task is BLOCKED waiting on another task; the resume
  function has already set `task.state = BLOCKED` and registered it as a
  waiter on the target.

Using an i32 return avoids allocating a sum-typed action object on every
resume.

#### Result boxing

`task.result` is `anyref`, but Twinkle primitives are unboxed. The resume
function boxes the result before storing it; `Task.await` unboxes after
reading it. Phase 2 uses the existing boxing infrastructure:

| Type    | Boxing                                    | Unboxing                         |
|---------|-------------------------------------------|----------------------------------|
| `Int`   | `StructNew("rt_types__BoxedInt")`         | `StructGet` field 0 → i64       |
| `Float` | `StructNew("rt_types__BoxedFloat")`       | `StructGet` field 0 → f64       |
| `Bool`  | `RefI31`                                  | `I31GetU` → i32                 |
| `Void`  | `I32Const(0)` + `RefI31`                 | `Drop` (result unused)          |
| GC refs | identity (already anyref-compatible)      | `RefCast` to concrete type       |

These are the same `emit_box_to_anyref` / `emit_unbox_from_anyref` helpers
already used for closure argument passing. No new boxing mechanism is needed.

#### Task frame (generated per task body)

Each distinct task body produces a unique frame struct type:

```wasm
(type $rt_frame_<id> (struct
  (field $pc     (mut i32))     ;; current segment index
  (field $cap0   (mut <type>))  ;; captured variables from enclosing scope
  (field $cap1   (mut <type>))
  ...
  (field $loc0   (mut <type>))  ;; hoisted locals live across suspension points
  (field $loc1   (mut <type>))
  ...
  (field $awaitN (mut anyref))  ;; saved awaited-task reference per AWAIT point
  ...
))
```

Frame fields include:
- **pc**: program counter — indexes the segment to resume at.
- **capture values**: the original closure's captured variables. Always
  hoisted because they are live from segment 0 across the implicit suspension
  between spawn and first resume.
- **hoisted locals**: every local whose value is defined before a suspension
  point and used after it. Stored with their concrete Wasm type (i32/i64/f64
  or anyref for GC references).
- **await targets**: for each AWAIT suspension point, a field to store the
  reference to the awaited task. On resume, the result is read from
  `await_target.result`. Typed as nullable `anyref` — initialized to null,
  set before suspension, cast to `(ref $rt_task)` on resume, and nulled
  after use to release the reference.

The initial capture values are written into the frame at spawn time. Body
locals are written to the frame before each suspension and read back after
resume.

#### Scheduler state

```wasm
(global $sched_queue (mut (ref null $rt_types__Array)) ...)  ;; FIFO queue of $rt_task
(global $sched_size  (mut i32) (i32.const 0))                ;; enqueued (RUNNABLE) count
(global $sched_blocked (mut i32) (i32.const 0))              ;; BLOCKED task count
```

The queue is a growable `rt_types__Array` used as a ring buffer or shifted
array. Exact growth strategy is an implementation detail — correctness only
requires FIFO ordering.

`sched_blocked` tracks the number of BLOCKED tasks. This enables deadlock
detection without a full task registry: when the queue is empty and
`sched_blocked > 0`, all remaining tasks are in a deadlock cycle.

Accounting rules:
- `Task.spawn` → `sched_size += 1`
- Dequeue → `sched_size -= 1`
- Enqueue (yield/wake) → `sched_size += 1`
- Await slow path → `sched_blocked += 1`
- Wake waiter → `sched_blocked -= 1`, `sched_size += 1`
- Complete → no counter change (task leaves the system)

Future phases with host async operations can suppress deadlock detection
while external completions are pending (e.g., a separate
`sched_pending_host` counter).

### Scheduler Trampoline

Two scheduler entry points serve different purposes:

#### `scheduler_run_until(target: ref $rt_task)`

Drives the scheduler until a specific task completes. Used by top-level
(blocking) `Task.await` only. Transformed task-body await is lowered inline
as a fast-path/slow-path check and does NOT call `scheduler_run_until` — the
slow path parks the task as BLOCKED and returns AWAIT to the scheduler.

```
fn scheduler_run_until(target):
  while target.state != DONE:
    if sched_size == 0:
      // No runnable tasks remain.
      // NOTE: Phase 4 host-async adds sched_pending_host; deadlock check
      // must become: sched_blocked > 0 AND sched_pending_host == 0.
      if sched_blocked > 0:
        trap("task deadlock: awaited task cannot complete")
      else:
        trap("task deadlock: no tasks remaining")
    task = dequeue()
    sched_size -= 1
    task.state = RUNNING
    action = call_ref task.resume(task)
    match action:
      COMPLETE (0):
        task.state = DONE
        task.frame = null
        // wake all waiters
        if task.waiters != null:
          for each waiter in task.waiters:
            waiter.state = RUNNABLE
            sched_blocked -= 1
            enqueue(waiter)
            sched_size += 1
          task.waiters = null
      YIELD (1):
        task.state = RUNNABLE
        enqueue(task)
        sched_size += 1
      AWAIT (2):
        // task.state already set to BLOCKED by resume function
        // sched_blocked already incremented by resume function
        // do not enqueue
  return target.result
```

This avoids the problem of draining unrelated tasks: it stops as soon as
`target` is DONE, even if other tasks remain in the queue. Unrelated BLOCKED
tasks do not cause spurious deadlock traps — deadlock is only reported when
the target is not DONE and no progress can be made.

#### `scheduler_drain()`

Drains all remaining runnable tasks after top-level code completes. Called
at program shutdown to honor the "spawned task is a commitment to execute"
guarantee.

```
fn scheduler_drain():
  while sched_size > 0:
    task = dequeue()
    sched_size -= 1
    task.state = RUNNING
    action = call_ref task.resume(task)
    // ... same action handling as scheduler_run_until ...
  // NOTE: Phase 4 host-async must also check sched_pending_host == 0
  // before declaring deadlock here.
  if sched_blocked > 0:
    trap("task deadlock: blocked tasks remain at program exit")
```

#### Integration with top-level code

Top-level code is NOT transformed into a resume function in Phase 2. It runs
as ordinary Wasm code (the module init function). Only closures passed
directly to `Task.spawn` are eligible for the state-machine transform.

When top-level code calls `Task.await(target)`:

```
// Top-level Task.await(target):
if target.state == DONE:
  return unbox(target.result)        // fast path, no scheduling
result = scheduler_run_until(target) // run until target completes
return unbox(result)
```

After the init function returns, the runtime calls `scheduler_drain()` to
execute any remaining spawned-but-unawaited tasks.

### State-Machine Transform

#### Pipeline placement

The transform runs in `link_program` (codegen.tw) between closure conversion
and backend preparation:

```
1. closure conversion      →  captures identified, bodies self-contained
2. ** task body transform **→  frame types + resume functions generated
3. prepare_backend          →  slots, reprs assigned (including new frame types)
4. plan_wasm_types          →  layouts include frame structs
5. emit_module              →  resume functions emitted with pc-dispatch
```

After closure conversion, each task body is a self-contained `AnfFunctionDef`
with captured values as explicit prefix parameters. The transform replaces
each task body function with a resume function and emits a frame type.

#### Identifying task bodies

Scan the `AnfModule` for calls to `Task.spawn` (known FuncId). Each such call
has the form:

```
Let(task_local, ACall(AGlobalFunc(TASK_SPAWN), [AMakeClosure(body_fid, captures)]), rest)
```

Collect the set of `body_fid` values — these are the functions to transform.

#### Suspension classification: three kinds of Task.spawn

Phase 2 classifies each `Task.spawn` call site into one of three categories:

**1. Direct closure with suspension points → state-machine transform.**

```tw
Task.spawn(fn() Int {
  Task.yield()       // suspension point found
  compute()
})
```

The closure literal is passed directly to `Task.spawn` and its body contains
`Task.yield()` or `Task.await()`. The compiler applies the full state-machine
transform: generates a frame type and resume function. The task is created as
RUNNABLE and enqueued — it does not run until the scheduler picks it up.

**2. Direct closure without suspension points → non-suspending scheduled task.**

```tw
Task.spawn(fn() Int {
  compute()          // no suspension points
})
```

The closure literal contains no suspension points. The compiler generates a
simple **run-to-completion resume function**: a wrapper that calls the
original function body once, stores the boxed result in `task.result`, and
returns COMPLETE. The task is still created as RUNNABLE and enqueued — it is
NOT called immediately at the `Task.spawn` call site. This preserves the
eager-enqueue semantics: spawned tasks do not run until the current task
yields, awaits, or top-level code drains the scheduler.

**3. Indirect closure (variable, parameter, etc.) → non-suspending only.**

```tw
f := fn() Int { ... }
Task.spawn(f)          // f is a variable, not a literal
```

When the argument to `Task.spawn` is not a syntactically direct closure
literal (it may be a variable, function parameter, record field, conditional
result, etc.), the compiler cannot statically inspect its body for suspension
points. These are always treated as non-suspending: a run-to-completion
resume function wraps the closure call.

If the indirect closure's body happens to contain `Task.yield()` or
`Task.await()`, those calls will be rejected by the validation pass (which
only allows suspension points directly inside task bodies identified in
category 1). This is a compile-time error, not a silent semantic difference.

#### Run-to-completion frame and resume function

Non-suspending tasks (categories 2 and 3) need the closure value accessible
to the resume function. Since resume receives only `(ref $rt_task)`, the
closure is stored in `task.frame`:

```wasm
(type $rt_frame_rtc (struct
  (field $closure (mut anyref))   ;; the closure to call (nullable, nulled after use)
))
```

The generic run-to-completion resume function:

```
fn resume_rtc(task: ref $rt_task) -> i32:
  frame = task.frame as ref $rt_frame_rtc
  closure = frame.closure as ref $rt_types__Closure
  // Call closure with no args via universal convention
  env = closure.env
  result = call_ref closure.func_ref(env, null)  // returns anyref
  task.result = result
  task.frame = null               // release frame + closure for GC
  return 0                        // COMPLETE
```

**Closure ABI note:** Twinkle's universal closure convention
(`rt_types__ClosureFunc`) has signature `(anyref, anyref) → anyref`. The
second parameter is the packed args array; zero-arg closures pass `null`
(this is the existing convention — see `emit_closure_call` in
`codegen/emit/closures.tw`, which emits `RefNull(.None_)` for empty args,
and trampolines accept null without attempting to read elements). All
closure calls go through a trampoline that boxes the return value to
`anyref` before returning (see `emit_universal_trampoline`). This means
`resume_rtc` receives an already-boxed `anyref` result — no additional
boxing is needed. At `Task.await` sites, the result type `T` is statically
known, so codegen emits the appropriate `emit_unbox_from_anyref` to recover
the concrete value.

**Void-returning tasks:** `Task.await<Void>` must still drive the scheduler
to completion before dropping the result. Codegen emits the full
`scheduler_run_until` / suspend-park sequence first, then drops the unboxed
void value. The `Drop` is a post-completion cleanup, not an optimization
that skips scheduling work.

For direct non-suspending closure literals (category 2), the compiler may
alternatively generate a specialized resume function that inlines the body
and calls `emit_box_to_anyref` on the result directly (avoiding the closure
allocation + trampoline overhead). This is an optimization, not a semantic
difference — both produce the same boxed `anyref` result in `task.result`.

#### Task.await classification

`Task.await` has different lowering depending on where it appears:

| Context                          | Lowering                              |
|----------------------------------|---------------------------------------|
| Top-level code (init function)   | Blocking: calls `scheduler_run_until` |
| Transformed task body            | Suspending: fast-path/slow-path inline|
| Ordinary function / non-transformed closure | **Compile-time error** in Phase 2 |

The third case prevents scheduler reentrancy. If a non-transformed
run-to-completion task calls `Task.await` and the target is not yet done,
there is no way to suspend — the task has no frame or resume function. Using
`scheduler_run_until` from inside a RUNNING task would cause reentrant
scheduling (the scheduler calls a resume function which calls the scheduler).

Phase 2 avoids this entirely: `Task.await` is only valid in top-level code
(where it drives the scheduler as the root) or directly in a transformed task
body (where it suspends via the state machine). The validation pass rejects
`Task.await` in any other position.

> **Exception:** `Task.await` on an already-DONE target is always safe (just
> read the result). A future relaxation could allow `Task.await` in ordinary
> functions if the compiler can prove the target is already complete, but
> Phase 2 does not attempt this — use the uniform restriction for simplicity.

#### Transform algorithm

For each task body function `f` with body `B`:

**Step 1 — Find suspension points.**

Walk `B` linearly (it is a let-chain) and mark each `ACall` that targets a
known suspending intrinsic:
- `Task.yield` FuncId → YIELD suspension
- `Task.await` FuncId → AWAIT suspension

Record the `LocalId` of the `Let` binding at each suspension point and its
position in the let-chain. If no suspension points are found, skip the
transform for this function (it uses run-to-completion semantics).

**Step 2 — Split into segments.**

Cut the let-chain at each suspension point. Segment 0 is everything before
the first suspension. Segment N is the code between suspension point N-1 and
suspension point N (or the function return).

```
Original body:
  let a = ...
  let b = ...           ← segment 0
  let _ = Task.yield()  ← suspension point 0 (YIELD)
  let c = f(a, b)       ← segment 1
  let _ = Task.await(t) ← suspension point 1 (AWAIT)
  let d = g(c)          ← segment 2 (returns d)
```

**Step 3 — Liveness analysis.**

For each suspension point, compute which locals are **live across** it:
defined (bound in a `Let` or a parameter) before the suspension point and
referenced (used as an `ALocal`) after it.

These locals must be hoisted into the frame. Locals used only within a single
segment remain as ordinary Wasm locals in the resume function.

**Step 4 — Generate frame type.**

Create a record type (not a user-visible type — a compiler-internal type
registered with the environment) with fields:

```
type TaskFrame_<id> = .{
  pc: Int,
  <for each captured variable C>: <type of C>,
  <for each hoisted local L>: <type of L>,
  <for each AWAIT point N>: await_target_N: anyref,
}
```

The capture parameters from the original closure are always hoisted (they are
live from segment 0 onward). Each AWAIT suspension point gets a dedicated
`await_target_N` field to store the awaited task reference so the result can
be read on resume.

**Step 5 — Generate resume function.**

Create a new `AnfFunctionDef` with:
- One parameter: `task` (typed as the Task struct)
- Return type: `Int` (action code)

Body structure:

```
// Read frame from task
let frame = task.frame as TaskFrame_<id>
let pc = frame.pc
// Dispatch on pc
if pc == 0:
  // Restore captures from frame
  let a = frame.cap_a
  ...
  // Execute segment 0 code
  ...
  // At YIELD suspension point: save live locals to frame
  frame.loc_b = b
  frame.pc = 1
  return 1   // YIELD
elif pc == 1:
  // Restore hoisted locals from frame
  let a = frame.cap_a
  let b = frame.loc_b
  // Execute segment 1 code
  let c = f(a, b)
  // At AWAIT suspension point:
  let target = <awaited task expression>
  if target.state == DONE:
    // Fast path: target already complete, skip suspension
    let await_result = unbox(target.result)
    // Fall through to segment 2 code
    let d = g(c)
    task.result = box(d)
    return 0   // COMPLETE
  else:
    // Slow path: park as waiter
    frame.loc_c = c
    frame.await_target_1 = target
    frame.pc = 2
    task.state = BLOCKED
    sched_blocked += 1
    target.waiters = append_waiter(target.waiters, task)  // allocates if null
    return 2   // AWAIT
elif pc == 2:
  // Resuming after await — target is guaranteed DONE
  let target = frame.await_target_1 as ref $rt_task
  let await_result = unbox(target.result)
  frame.await_target_1 = null        // release reference
  let c = frame.loc_c
  let d = g(c)
  task.result = box(d)
  return 0   // COMPLETE
```

**Sequential awaits.** When multiple awaits appear in sequence (no
intervening yield), the fast path for each await inlines the subsequent
code up to the next suspension point or return. This means a fast-path
await does NOT return at the segment boundary — it falls through to
the next segment's code. Only the slow path (target not done) saves state
and returns AWAIT.

**Step 6 — Rewrite Task.spawn call site.**

The `AMakeClosure(body_fid, captures)` + `ACall(TASK_SPAWN, [closure])` is
rewritten to:

```
// Create frame with pc=0 and captured values
let frame = TaskFrame_<id>.{ pc: 0, cap_0: val_0, cap_1: val_1, ... }
// Create task object
let task = rt_task.{
  state: RUNNABLE,
  frame: frame,
  resume: resume_<id>,
  result: null,
  waiters: null,
}
// Enqueue
scheduler_enqueue(task)
sched_size += 1
```

The original closure and its trampoline are no longer needed for this
function.

#### Locals that don't need hoisting

Locals used only within a single segment (defined and last-used before the
next suspension point) remain ordinary let-bindings in the resume function.
They compile to normal Wasm locals, avoiding unnecessary frame reads/writes.

### Task.yield() Intrinsic

#### Type signature

```tw
fn Task.yield() Void
```

#### Frontend

Register `Task.yield` with a new FuncId (e.g., `TASK_YIELD = FuncId(1036)`).
Type: `fn() Void`. Add to builtin registry in both compilers.

#### Codegen

Inside a transformed task body, `Task.yield()` compiles to:
1. Save live locals to frame.
2. Set `frame.pc` to the next segment.
3. Return `1` (YIELD action).

Outside a task body (e.g., in an ordinary function called from a task),
`Task.yield()` is a **compile-time error** in Phase 2. The validation pass
rejects it.

### Suspension Effect Classification

The compiler internally classifies each function/closure body with a
suspension effect. This is not exposed as user-facing syntax — it is a
compiler-internal property used for validation and transform decisions.

```
SuspensionEffect:
  NonSuspending    — ordinary function; may not yield or park
  MaySuspend       — transformed task body; may yield or park via await
```

Classification rules:

| Context                                    | Effect          |
|--------------------------------------------|-----------------|
| Direct `Task.spawn` closure with yield/await | `MaySuspend`  |
| Direct `Task.spawn` closure without         | `NonSuspending` |
| Indirect `Task.spawn` closure              | `NonSuspending` |
| `Cell.update` callback                     | `NonSuspending` (enforced) |
| All other functions and closures           | `NonSuspending` |
| Top-level init code                        | n/a (scheduler root, not a task) |

Intrinsic requirements:

| Intrinsic       | Requires         | Error if violated                               |
|-----------------|------------------|-------------------------------------------------|
| `Task.yield()`  | `MaySuspend`     | "Task.yield only valid in transformed task body"|
| `Task.await()`  | `MaySuspend` or top-level | "Task.await not valid in this context"  |

In Phase 2, `MaySuspend` is not a general function effect — it is only
assigned to direct `Task.spawn` closure literals selected for state-machine
lowering. Ordinary functions cannot be `MaySuspend`; suspension through
nested calls requires a future CPS transform or Wasm stack switching.

This classification drives both validation error messages and the decision
of which `Task.spawn` call sites get the state-machine transform vs. the
run-to-completion adapter.

### Validation

#### Suspension-point restrictions

A validation pass runs before the state-machine transform and rejects
programs that use suspension points in unsupported positions:

1. **`Task.yield()` outside a transformed task body**: error. Yield is only
   valid directly inside a closure literal passed to `Task.spawn` whose body
   has been classified for state-machine transform.

2. **`Task.await()` outside top-level code or a transformed task body**:
   error. Await is only valid in top-level code (where it drives the
   scheduler) or directly inside a transformed task body (where it suspends
   via the state machine). Await inside ordinary functions, non-transformed
   closures, or indirect-closure task bodies is rejected to prevent scheduler
   reentrancy.

3. **`Task.yield()` or `Task.await()` inside `Cell.update` callback**: error.
   Cell updates must be non-suspending to preserve atomicity with respect to
   the cooperative scheduler.

4. **`Task.yield()` or `Task.await()` inside a nested function call from a
   task body**: error. Suspension *through* an ordinary call frame is not
   supported in Phase 2. The suspension point must appear directly in the
   task body's let-chain.

#### Implementation

Phase 2 only supports **flat top-level let-chain** suspension points. Walk
each task body's ANF and check that every suspension-point call
(`Task.yield`, `Task.await`) appears at the top level of the let-chain, not
nested inside:
- A closure body passed to any function other than `Task.spawn`
- An `if` branch or `case` arm
- A loop body

All three are rejected in Phase 2. Supporting suspension inside branches
requires per-branch segment chains; supporting loops requires loop iterations
to become pc-dispatch segments. Both are required for `Task<T>` to feel useful
in ordinary Twinkle programs, so they are the dedicated scope of Phase 3 rather
than host-async or helper-library work.

Additionally, scan all non-task-body functions for `Task.yield` and
`Task.await` calls and reject them (except `Task.await` in the top-level
init function, which uses the blocking `scheduler_run_until` path).

**Example of rejected code:**

```tw
fn wait_for<T>(task: Task<T>) T {
  task.await()   // ERROR: Task.await not valid in this context
}

// Even though wait_for is called from top-level code:
value := wait_for(my_task)
```

"Top-level await" means `Task.await` appearing syntactically in the module's
top-level let-chain (the init function), not merely called transitively from
top-level code. The restriction is structural, not call-graph-based.

## Phase 3 Detailed Design: Local Control-Flow State Machines

Phase 3 replaces the Phase 2 linear let-chain splitter with a local CFG-based
state-machine transform. The transform still applies only to direct
`Task.spawn` closure literals. It does not make ordinary functions or nested
closures suspending.

### CFG-based splitting

Instead of cutting only a flat let-chain, the compiler lowers the task body's
local expression tree into basic blocks. Each suspension point becomes a resume
block with a distinct `pc` value. The generated resume function dispatches on
`frame.pc`, restores the frame values needed by that block, executes until the
next suspension, branch, loop back-edge, or return, then either continues to
another block or returns a scheduler action.

Values live across a suspension point are hoisted into the frame just as in
Phase 2. Phase 3 extends liveness over CFG edges rather than over a single
linear sequence. This includes values defined before a branch and used after its
join, loop-carried values, iterator state, loop indices, and pattern bindings
from `case` arms that are needed after a suspension.

`Task.await` keeps the same fast-path/slow-path behavior in CFG form: if the
awaited task is already DONE, the result is read and execution continues along
the current CFG path without returning to the scheduler. If the target is not
DONE, the current continuation `pc` and live values are saved, the task is
parked as a waiter, and the resume function returns AWAIT. `Task.yield` always
saves the continuation and returns YIELD.

### Branches and joins

Suspension is allowed in `if` branches and `case` arms. Each branch or arm may
contain its own suspension points. A join point after the `if` or `case` gets a
`pc` block when control can resume there.

Expression-valued `if` and `case` forms use compiler-generated temporaries for
the branch result. If a branch suspends before producing the expression result,
any values needed to finish that branch are saved in the frame. Once a branch
produces the result, the result is stored in a temporary that is available at the
join block. If that temporary is live across a later suspension, it is hoisted
into the frame.

Suspending operations in condition, scrutinee, and iterable positions are also
lowered through compiler temporaries. For example, `if task.await() { ... }`
saves the awaited task on the slow path; on resume, the awaited result is read
into a temporary and condition dispatch continues. The same rule applies to
`case task.await() { ... }` scrutinees and `for item in task.await() { ... }`
iterator setup.

For `case`, variant payload bindings are ordinary arm-local locals. If a payload
binding is live across a suspension within the arm, it is stored in the frame and
restored when that arm resumes. Payload bindings do not exist outside their arm
unless their values are explicitly used to produce an expression result that
flows to the join.

### Loops

Suspension is allowed in `for` bodies, including collection iteration,
indexed iteration, and condition-style `for cond { ... }` loops. Loop back-edges
become CFG edges to the loop header. The task frame stores any loop-carried
state live across suspension points: iterator/cursor state, current element,
index variables, condition temporaries, accumulator locals, and user locals used
after the suspension.

A suspension inside a loop body resumes at the continuation block for that
iteration. After the resumed body reaches the loop back-edge, normal loop logic
runs again: advance the iterator or re-evaluate the condition, then either enter
the next iteration or continue after the loop.

Twinkle currently has no `break` or `continue` syntax. If those are added later,
they should lower to CFG edges: `continue` jumps to the loop back-edge/header,
and `break` jumps to the loop exit. Suspension before either jump uses the same
frame-hoisting rules as any other path.

### Allowed suspension positions

Phase 3 allows suspension anywhere in the transformed task body's local control
flow, including conditions, scrutinees, iterable expressions, branch/arm bodies,
and loop bodies:

```tw
Task.spawn(fn() Void {
  if should_wait().await() {
    Task.yield()
  }

  for item in load_items().await() {
    process(item)
    Task.yield()
  }

  case fetch_state().await() {
    .Ready(value) => use(value)
    .Waiting(task) => task.await()
  }
})
```

The key boundary is lexical, not call-graph based: the suspension point must be
inside the direct task-body expression tree that the compiler is transforming.
It must not be hidden inside an ordinary function or nested closure.

Allowed:

```tw
Task.spawn(fn() Void {
  if cond {
    Task.yield()
  }
})
```

Rejected:

```tw
Task.spawn(fn() Void {
  helper := fn() Void {
    Task.yield()
  }
  helper()
})
```

The second example remains invalid because suspension through the nested closure
would require that closure and its caller to be transformed as suspending call
frames. That remains future work.

### Phase 3 validation

Phase 3 replaces the Phase 2 "flat top-level let-chain" validation rule with a
local-CFG rule:

> Suspension points are allowed anywhere in a transformed task body's local
> control-flow graph, but still not inside ordinary function bodies or nested
> closure bodies within that task body.

The existing restrictions still apply:

* `Task.yield()` outside a transformed task body is invalid, except for the
  top-level scheduler-pump form added in Phase 3.
* `Task.await()` outside top-level code or a transformed task body is invalid.
* `Task.yield()` and `Task.await()` inside `Cell.update` callbacks are invalid.
* Indirect closures passed to `Task.spawn` are non-suspending and cannot contain
  suspension points.

### Top-level `Task.yield()`

Phase 3 adds top-level `Task.yield()` as a scheduler-pump operation. Top-level
module code is still not a task and is not parked or resumed. The operation runs
at most one scheduler turn:

1. If the runnable queue is non-empty, dequeue one task, decrement
   `sched_size`, mark it RUNNING, and call its resume function.
2. Handle exactly one returned scheduler action from that resumed task:
   - `COMPLETE`: mark the task DONE, store/release fields as usual, wake its
     waiters by enqueueing them, decrement `sched_blocked` for each woken waiter,
     increment `sched_size` for each enqueue, then return `Void`.
   - `YIELD`: re-enqueue that task, increment `sched_size`, then return `Void`.
   - `AWAIT`: leave that task BLOCKED, then return `Void`.
3. If the runnable queue is empty and `sched_blocked == 0`, return `Void`.
4. If the runnable queue is empty and `sched_blocked > 0`, trap with a task
   deadlock error.

"Exactly one returned scheduler action" means exactly one task is resumed.
Completion may still iterate over that task's full waiter list and enqueue all
waiters. Waiters woken by the one resumed task are only enqueued. They are not
run by the same top-level `Task.yield()` call; a later `Task.yield()`,
`Task.await()`, or program-exit drain may run them.

As with `scheduler_run_until` and `scheduler_drain`, the deadlock check changes
once host async exists: Phase 4 should only trap when no runnable tasks remain,
blocked tasks remain, and no host completions are pending.

## Backward Compatibility

Phase 2 is **source-compatible** with Phase 1 programs but introduces a
**timing change**: `Task.spawn` enqueues closures for later scheduling
instead of calling them synchronously.

```tw
Task.spawn(fn() Void { println("task") })
println("main")
```

Phase 1 prints `task` then `main`. Phase 2 prints `main` then `task`
(the spawned closure runs when the scheduler is driven, e.g., by a
subsequent `Task.await` or at program shutdown via `scheduler_drain`).

Specific compatibility notes:

- Programs that spawn and then immediately await still work, but Phase 2
  drives `scheduler_run_until` at the await site instead of returning a
  pre-completed result. The already-complete fast path only applies to tasks
  that were completed by earlier scheduler work (e.g., awaited a second
  time, or completed as a side effect of awaiting a different task).
- Non-suspending closures passed to `Task.spawn` (including indirect closure
  variables) use a run-to-completion resume function. Programs relying on
  synchronous side effects at spawn time may observe different ordering.
  This is intentional: Phase 2 establishes proper scheduling semantics where
  `Task.spawn` means "enqueue for later execution."
- The Task type changes representation (from 1-element array to `$rt_task`
  struct), which is a breaking ABI change. Recompilation is required.

### Implementation Order

1. Add `Task.yield()` to frontend (type signature, FuncId, builtin registry).
2. Define `$rt_task`, `$rt_resume_fn`, scheduler globals as runtime types.
3. Implement `scheduler_run_until` and `scheduler_drain` as runtime Wasm
   functions.
4. Implement the validation pass (reject unsupported positions).
5. Implement the state-machine transform (the core of Phase 2).
6. Update `Task.spawn` codegen: suspending bodies create frame + task +
   enqueue; non-suspending bodies use run-to-completion adapter.
7. Update `Task.await` lowering:
   - top-level: fast-path check + `scheduler_run_until` fallback;
   - transformed task body: inline fast-path / park slow-path;
   - all other contexts: validation error (rejected in step 4).
8. Add `Task.yield` codegen (frame save + return YIELD).
9. Write tests: basic yield, multiple tasks interleaving, await-after-yield,
   sequential awaits, fast-path await, deadlock detection, non-suspending
   spawn.
10. Update stage0 if the boot compiler source starts using tasks.

