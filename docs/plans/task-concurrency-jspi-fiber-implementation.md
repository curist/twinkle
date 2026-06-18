# Task Concurrency — JSPI Backend Implementation Plan

Status: **In progress (2026-06-18). Checkpoints 0–17 done** (CP16 negative-path
tests — failure/deadlock/lifecycle — validated by smokes but deferred to a
subprocess harness; see CP16 note). Compiler side
(CP1–7): the stackless scheduler/transform/validation are removed, `Task<T>` is
an id-only handle, task ops lower through the `task_abi` suspension-intrinsic
binding table, `__task_run` is exported, and `Task.sleep`/`Task.read_stdin`
exist. Runtime side (CP8–15): a cooperative JS scheduler in `runtime.mjs`
implements the task host imports — eager-enqueue `task_create`,
`promising(__task_run)` task bodies, a race-free `scheduler.current` resume
discipline (strict one-task-per-microtask), FIFO `suspend_yield`, parking
`suspend_await`, drain + deadlock detection, and `suspend_sleep`/
`suspend_read_stdin` host readiness. `task_suite` is rewritten for the stackful
model (CP16) and re-enabled. Phase B scheduler benchmark (CP17) run on Node and
Deno — GO: `Task.yield()` ~0.16 µs, spawn+await ~0.7 µs, LSP-shaped dispatch
~1.8 µs (all far under budget); `sleep` is timer-floor host latency, not
switching. Recorded in the design doc's Evidence section. Remaining: stage0
parity (CP18, deferred until boot source uses tasks), LSP adoption (CP19).

This is the implementation plan for the stackful `Task<T>` design in
[task-concurrency-jspi-fiber.md](task-concurrency-jspi-fiber.md). Keep this plan
aligned with that design document: the public API is backend-independent, the
compiler lowers to abstract suspension intrinsics, and the current binding uses
JSPI in the JS host.

The implementation is split into concrete checkpoints so each commit can leave
the tree in a reviewable state. The early checkpoints intentionally separate
"remove retired stackless machinery" from "add the JSPI scheduler" so non-task
programs can stay green while task behavior is being swapped.

## Ground rules

- Implement in the boot compiler first. Stage0 only needs parity when boot source
  starts depending on tasks for self-hosting or LSP code.
- Keep the public `Task<T>` API stable; all churn is in lowering, runtime, and
  tests.
- Keep host imports id-based. JS receives and returns `i32 task_id`; Wasm owns the
  `Task<T>` struct.
- Preserve the backend-neutral seam. Codegen call sites target operation-named
  suspension intrinsics; only one binding table knows the current JSPI host names.
- Remove stackless-only diagnostics. Suspending from helpers, callbacks, and
  recursive code is valid under the stackful model.
- Keep task tests separate from non-task compiler regressions while the backend is
  being swapped, so the tree can stay green between checkpoints.

## Checkpoint 0 — spike gate is recorded

Status: complete.

- [x] Run the raw JSPI switching-cost probe on Node and Deno.
- [x] Record the result in the design document's Evidence section.
- [x] Decide whether the JSPI backend is still the right near-term binding.

Done when:

- Evidence says **GO**.
- The implementation plan proceeds without changing the public `Task<T>` design.

## Checkpoint 1 — remove stackless scheduler entry points

Goal: stop emitting calls to the old in-Wasm scheduler without yet requiring the
new JSPI scheduler to work.

Code changes:

- [x] Remove `boot/compiler/codegen/runtime/sched.tw` from runtime module assembly
  and stop importing `sched_enqueue`, `sched_run_until`, `sched_drain`, and
  waiter helpers into user modules.
- [x] Remove the top-level `sched_drain` call currently appended after program
  body emission. Draining will move to the JS scheduler after `__twinkle_start`
  settles.
- [x] Replace old task intrinsic lowering with explicit temporary stubs/traps, or
  with calls through the new intrinsic table if Checkpoint 2 is done in the same
  commit.
- [x] Keep non-task codegen unchanged.

Suggested searches:

```bash
rg -n "sched_|rt_sched|sched_drain|sched_run_until|sched_enqueue" boot/compiler
```

Done when:

- No boot codegen path emits old `sched_*` calls.
- Non-task programs still build and run.
- Task programs fail clearly if the new scheduler is not installed yet.

Validation:

```bash
make stage2
make rust-test
```

## Checkpoint 2 — remove stackless transform and validation

Goal: delete the compiler machinery whose only purpose was stackless suspension.

Code changes:

- [x] Remove `boot/compiler/codegen/emit/task_resume.tw` from the emit pipeline.
- [x] Remove `TaskResumeInfo`, `suspending_bodies`, `classify_task_bodies`,
  `analyze_task_body`, and resume-function emission from emit context/codegen.
- [x] Remove `boot/compiler/backend/task_validate.tw` from the codegen pipeline.
- [x] Update or delete direct validation helpers that exist only to assert old
  restrictions.
- [x] Convert old rejection tests into future positive tests, or quarantine them
  until the JSPI scheduler checkpoint lands.

Suggested searches:

```bash
rg -n "task_validate|validate_tasks|task_resume|TaskResumeInfo|suspending_bodies|classify_task_bodies" boot/compiler boot/tests
```

Done when:

- The boot compiler no longer classifies task bodies as suspending/non-suspending.
- There are no diagnostics that reject `Task.yield`/`Task.await` because they
  appear in ordinary helpers, callbacks, branches, or recursive code.
- Non-task suites remain green.

Validation:

```bash
make stage2
make rust-test
```

## Checkpoint 3 — shrink the Wasm `Task<T>` representation

Goal: make the runtime type match the JSPI ABI: `Task<T>` is only a Wasm-owned
handle carrying an integer id.

Code changes:

- [x] Change `rt_types__Task` to an id-carrying struct, e.g. one immutable or
  mutable `i32` field named `id`/`task_id`.
- [x] Remove frame/resume/result/waiter fields from `Task` and remove obsolete
  frame runtime structs that only served the stackless backend.
- [x] Update task equality/identity lowering to compare ids or preserve the
  current equality semantics via the new handle representation.
- [x] Update any tests or WAT snapshots that mention the old task struct layout.

Suggested searches:

```bash
rg -n "rt_types__Task|FrameRtc|FrameBase|Task\"" boot/compiler src tests
```

Done when:

- JS does not need to construct, inspect, or mutate a `Task<T>` GC struct.
- `Task<T>` values in Wasm can be wrapped/unwrapped around an `i32 task_id`.
- No old frame/resume fields remain in active runtime types.

Validation:

```bash
make stage2
```

## Checkpoint 4 — add the abstract suspension intrinsic table

Goal: establish the migration seam before the JSPI scheduler is implemented.

Code changes:

- [x] Add a small backend binding table for the abstract operations:
  - `task_create(closure: anyref) -> i32`
  - `suspend_await(task_id: i32) -> anyref`
  - `suspend_yield() -> Void`
  - `suspend_sleep(ms: Int) -> Void`
  - `suspend_read_stdin(max: Int) -> anyref`
- [x] Ensure call sites ask the table for symbols/imports. Avoid hard-coded host
  module/name strings outside the table.
- [x] Add imports for the current JSPI binding through the table. The concrete
  module/name choice is runtime policy; it should be centralized and easy to swap
  for a future continuations binding.
- [x] Keep temporary trap/stub implementations possible while the scheduler is
  not yet installed.

Done when:

- A code reviewer can find the complete JSPI binding name policy in one place.
- The compiler's task lowering mentions operation names, not host-specific names.
- A future continuations binding would replace the table/runtime binding, not the
  frontend or mid-pipeline.

Validation:

```bash
target/twk build /tmp/task_smoke.tw -o /tmp/task.wat
# Inspect the WAT for operation-named task/suspend imports.
make stage2
```

## Checkpoint 5 — add `Task.sleep` and `Task.read_stdin` surface signatures

Goal: expose the full MVP API before implementing host readiness.

Code changes:

- [x] Register builtin/prelude signatures for:
  - `Task.sleep(ms: Int) Void`
  - `Task.read_stdin(max: Int) Vector<Byte>`
- [x] Add API docs/comments in the generated/prelude module source where the
  current `Task.spawn`/`await`/`yield` comments live.
- [x] Ensure `Task.read_stdin` follows the existing `@std.io` convention: empty
  vector means EOF/empty read, with EOF distinguished by existing stdin state.
- [x] Add completion/typechecking coverage for the new methods if the test suite
  has task/API completion coverage nearby.

Done when:

- User code can typecheck calls to `Task.sleep` and `Task.read_stdin`.
- The backend may still trap if the runtime binding is not installed; that is
  acceptable until the host readiness checkpoint.

Validation:

```bash
target/twk build /tmp/task_api_smoke.tw -o /tmp/task_api.wat
make stage2
```

## Checkpoint 6 — lower core task operations to abstract intrinsics

Goal: make `Task.spawn`, `Task.await`, and `Task.yield` use the new ABI.

Code changes:

- [x] Lower `Task.spawn(f)` by emitting `f`, calling `task_create`, and wrapping
  the returned `i32` id in a `Task<T>` struct.
- [x] Lower `Task.await(t)` by unwrapping the `task_id`, calling
  `suspend_await`, and unboxing the returned `anyref` to the statically known
  result type.
- [x] Lower `Task.yield()` to `suspend_yield()`.
- [x] Do not box a spawn result at the spawn site. The result does not exist yet;
  result boxing belongs in `__task_run`.
- [x] Keep lowering valid at arbitrary call depth. No context check should depend
  on being inside a direct `Task.spawn` closure.

Done when:

- WAT for task programs shows id wrap/unwrap around `Task<T>` and calls through
  abstract operations.
- There are no stackless fast/slow await paths left.

Validation:

```bash
target/twk build /tmp/task_spawn_await_smoke.tw -o /tmp/task_spawn_await.wat
make stage2
```

## Checkpoint 7 — export `__task_run(closure) -> anyref`

Goal: provide the universal Wasm entry point the JS scheduler uses to run a task
body on its own JSPI stack.

Code changes:

- [x] Emit a function named/exported `__task_run` with one closure argument and an
  `anyref` result.
- [x] Inside `__task_run`, cast/load the closure as needed, call it through the
  universal closure path with no user arguments, and return the boxed result.
- [x] Ensure void-returning task bodies still return the canonical boxed/erased
  void value expected by existing `emit_box_to_anyref` / closure trampoline logic.
- [x] Add the export alongside `__twinkle_start`; do not turn it into a Wasm start
  function.

Done when:

- Built task programs export both `__twinkle_start` and `__task_run`.
- JS can wrap `instance.exports.__task_run` with `WebAssembly.promising`.

Validation:

```bash
target/twk build /tmp/task_spawn_await_smoke.tw -o /tmp/task_run.wat
rg -n "__task_run|__twinkle_start" /tmp/task_run.wat
make stage2
```

## Checkpoint 8 — install JSPI task imports in `runtime.mjs`

Goal: wire the abstract intrinsic imports to a scheduler object before module
instantiation.

Runtime changes:

- [x] Create a small scheduler object during `runWasmBytesAsync` preparation when
  JSPI is available.
- [x] Install imports for `task_create`, `suspend_await`, `suspend_yield`,
  `suspend_sleep`, and `suspend_read_stdin` before instantiation.
- [x] Let the import closures capture the scheduler before the Wasm instance is
  known; attach `instance.exports` after instantiation.
- [x] If a module imports task operations and `hasJspi` is false, throw a clear
  "Task requires JSPI" error.
- [x] Keep non-task async extern behavior unchanged.

Done when:

- Instantiation succeeds for modules importing task intrinsics.
- Non-task programs still run through both sync and async runtime APIs.
- A missing-JSPI path fails explicitly rather than silently using synchronous
  Phase 1 task behavior.

Validation:

```bash
target/twk run /tmp/non_task_smoke.tw
target/twk run /tmp/task_import_smoke.tw
```

## Checkpoint 9 — implement core scheduler state and task start

Goal: make spawned task bodies start through `promising(__task_run)`, but not yet
require all await/yield edge cases to be final.

Runtime model:

- Task record fields: `id`, `closure`, `state`, `result`, `error`, `waiters`, and
  an `awaited`/`observedFailure` marker for unawaited-failure reporting.
- Scheduler fields: `nextId`, `current`, `tasks`, `runnable`, `pendingHost`,
  `topLevelDone`, and a cached `promisingTaskRun`.
- Reserved task id `0` represents top-level.

Runtime changes:

- [x] `task_create(closure)` allocates an id, stores the closure, enqueues a
  start entry, and returns the id without running the body synchronously.
- [x] The pump starts runnable task entries FIFO. Before starting a task, set
  `scheduler.current` to the task id and call `promisingTaskRun(closure)`.
- [x] Task completion stores the boxed result and wakes waiters. Rejection stores
  failure and wakes/rejects waiters.
- [x] Keep JS references to closures/results until the task record is no longer
  needed; this provides the rooting expected by the design.

Done when:

- Spawned bodies run after the current stack yields/awaits or after top-level
  enters drain, not synchronously inside `Task.spawn`.
- Completed task records retain boxed results for later awaits.

Validation programs:

- spawn/await round trip;
- spawn ordering smoke where top-level observes that `spawn` does not run the
  body immediately;
- await-after-completion returns the stored result.

## Checkpoint 10 — implement `scheduler.current` resume discipline

Goal: ensure every suspending import knows the logical caller when a resumed Wasm
stack continues.

Runtime changes:

- [x] Capture `caller = scheduler.current` at the start of every suspending task
  import.
- [x] Return promises through a helper such as `resumeAs(caller, promise)` that
  sets `scheduler.current = caller` immediately before the JSPI continuation is
  allowed to resume Wasm code.
- [x] When enqueueing a suspended continuation, enqueue the resolver/rejecter for
  that import promise, not a direct call into Wasm.
- [x] Avoid relying on a global `current` value left over from whichever task ran
  most recently; set it deliberately for every start/resume path.

Done when:

- Nested awaits and task-to-task resumes do not confuse the caller id.
- Top-level `Task.await` works using pseudo-task id `0`.

Validation programs:

- top-level awaits a pending task;
- task awaits another task;
- nested task awaits another nested task;
- helpers called from tasks can suspend and resume with the correct caller.

## Checkpoint 11 — implement `suspend_yield`

Goal: provide cooperative interleaving.

Runtime changes:

- [x] `suspend_yield()` captures the current id.
- [x] It returns a Promise whose resolver is enqueued at the back of the runnable
  queue.
- [x] The scheduler pump resolves runnable entries FIFO.
- [x] The resumed continuation passes through the `resumeAs(currentId, promise)`
  discipline from Checkpoint 10.

Done when:

- Yielding task bodies interleave in FIFO order.
- Yield inside ordinary helpers/callbacks works because no compiler transform is
  involved.

Validation programs:

- multiple tasks append to a shared `Cell<Vector<Int>>` around yields and produce
  FIFO interleaving;
- a recursive helper yields and resumes;
- a higher-order callback yields and resumes.

## Checkpoint 12 — implement `suspend_await`

Goal: park callers until target tasks settle.

Runtime changes:

- [x] Validate target id and trap clearly on invalid task handles.
- [x] If target is done, return/resolve immediately with the boxed result.
- [x] If target failed, reject so the awaiter re-traps.
- [x] If target is pending, mark the target as awaited, append the caller's
  resolver/rejecter to the target waiters, and return a suspending Promise.
- [x] When a target settles, enqueue waiter resumes rather than recursively
  running them inline. This preserves cooperative FIFO behavior and avoids deep JS
  recursion.

Done when:

- Await before completion parks the caller and later returns the result.
- Await after completion does not unnecessarily enqueue work.
- Failed targets re-trap awaiters.

Validation programs:

- await-before-completion;
- await-after-completion;
- many waiters on one target;
- task-body trap propagates through one awaiter and a chain of awaiters.

## Checkpoint 13 — drain and deadlock detection

Goal: match the lifecycle semantics: spawned tasks are commitments to execute,
and true deadlock is reported.

Runtime changes:

- [x] After `__twinkle_start` resolves, run the scheduler drain until no runnable
  tasks remain.
- [x] If runnable is empty, pending-host is zero, and blocked tasks remain, throw
  a trap-equivalent deadlock error.
- [x] If runnable is empty but pending-host is nonzero, wait for the next host
  completion instead of reporting deadlock.
- [x] If an unawaited task failed, surface that failure during drain.
- [x] Ensure ordinary successful unawaited tasks complete without requiring the
  program to hold their handles.

Done when:

- Fire-and-forget success drains.
- Fire-and-forget failure is not swallowed.
- True await cycles or waits with no producer report deadlock.
- Pending timers/stdin suppress deadlock until they settle.

Validation programs:

- unawaited task mutates a cell before process exit;
- unawaited task traps and the run fails;
- two tasks await each other and deadlock;
- pending host wait does not deadlock prematurely.

## Checkpoint 14 — bind `suspend_sleep`

Goal: add timer readiness with correct pending-host accounting.

Runtime changes:

- [x] `suspend_sleep(ms)` captures the current id.
- [x] Increment `pendingHost` before scheduling the timer.
- [x] Decrement `pendingHost` exactly once when the timer fires or is cancelled by
  error handling.
- [x] Enqueue/resume the sleeping task through the normal resume discipline.
- [x] Decide and document behavior for negative durations; prefer trapping or
  clamping in one place, not per call site.

Done when:

- Sleeping task yields control to other runnable tasks.
- Sleep wakeups resume without false deadlock.

Validation programs:

- sleep wakes a task;
- another task runs while one sleeps;
- idle-with-only-sleeping-task waits instead of deadlocking;
- negative-duration behavior is covered.

## Checkpoint 15 — bind `suspend_read_stdin`

Goal: add stdin readiness for LSP-style input loops.

Runtime changes:

- [x] Reuse `runtime.host.readStdinAsync(max, timeout, runtime)` or the equivalent
  existing async stdin path.
- [x] Increment/decrement `pendingHost` around the read.
- [x] Convert returned bytes to the same `Vector<Byte>` representation used by
  existing `@std.io` functions, then return it as `anyref`.
- [x] Preserve EOF behavior and compatibility with `io.stdin_eof()`.
- [x] Avoid mixing stream and blocking fd reads on Node; keep the existing JSPI IO
  path's stream-buffering invariant.

Done when:

- A task can park waiting for stdin without busy-spinning.
- EOF returns an empty vector consistently with existing IO APIs.
- Deadlock detection treats an in-flight read as pending host work.

Validation programs:

- stdin data wakes a reader task;
- EOF wakes a reader task with an empty vector;
- a read with no data does not busy-spin;
- pending stdin read suppresses deadlock until it completes.

## Checkpoint 16 — update task test suite for stackful semantics

Goal: make tests describe the new model rather than the retired stackless model.

Test changes:

- [x] Replace old validation-rejection tests with positive stackful tests:
  - yield in ordinary helper;
  - await in ordinary helper;
  - yield/await in branches;
  - yield/await in recursive calls;
  - yield/await in higher-order callbacks.
- [ ] Keep `Cell.update` suspending-callback coverage, but mark it according to
  the design decision: discouraged/undefined for MVP unless a runtime guard is
  implemented.
- [ ] Add failure propagation tests: awaited failure, chained awaited failure, and
  unawaited failure during drain.
- [ ] Add lifecycle tests: eager-enqueue spawn, unawaited drain, deadlock, and
  pending-host-vs-deadlock.

> The three unchecked items above are **validated by standalone smoke programs**
> (spawn/await failure traps, two-task deadlock, sleep readiness, eager-enqueue
> ordering, read_stdin EOF — see the Phase B/runtime work), but not yet encoded
> as committed regression tests: a task trap or deadlock aborts the in-process
> test runner, so they need a **subprocess harness** (run a program via `twk`,
> assert exit code / output). Tracked as follow-up.

Done when:

- The task suite proves stackful suspension is allowed anywhere direct-style code
  can call a function.
- No tests assert old function-coloring or direct-closure restrictions.

Validation:

```bash
target/twk run boot/tests/main.tw
make boot-test
```

## Checkpoint 17 — Phase B scheduler benchmark

Goal: measure the real scheduler path after the implementation exists.

Benchmark work:

- [x] Extend `boot/bench/jspi/` or add a companion bench for:
  - repeated `Task.yield`;
  - await ping-pong;
  - sleep/readiness latency;
  - LSP-shaped reader/dispatcher/debounce smoke.
- [x] Run Node, Deno, and the bundled `target/twk` path.
- [x] Record summarized results in the design document's Evidence section.
- [x] Keep reusable benchmarks under `boot/bench/`; do not turn microbenchmarks
  into compiler correctness tests.

Decision:

- [x] Proceed unchanged if scheduler overhead stays within the design budget.
- [ ] Document guidance to avoid fine-grained yielding if overhead is visible but
  acceptable. *(N/A — overhead negligible.)*
- [ ] Revisit batching/yield coalescing before LSP adoption if overhead dominates
  the LSP-shaped smoke. *(N/A — overhead negligible.)*

## Checkpoint 18 — stage0 parity gate

Goal: defer stage0 work until it is actually needed, then keep it minimal.

Work:

- [ ] Add stage0 lowering for the same abstract intrinsic operations when boot
  source starts using tasks in code stage0 must compile.
- [ ] Keep stage0 free of scheduler/state-machine logic; it should only emit the
  same id-based intrinsic calls and `__task_run` export shape.
- [ ] Add focused parity tests or snapshots for the emitted imports/exports.

Done when:

- A stage0-built boot compiler can compile task-using boot source.
- Stage0 and boot agree on the public task ABI.

## Checkpoint 19 — LSP adoption gate

Goal: use tasks in real compiler workflows only after core semantics, readiness,
and scheduler overhead are proven.

Work:

- [ ] Track LSP migration in a separate plan/commit series.
- [ ] Move LSP internals onto cooperative tasks: input reader, dispatcher,
  debounce timers, diagnostics workers, and generation tokens for stale results.
- [ ] Keep parallel compilation out of scope. This plan gives cooperative
  single-threaded concurrency; worker-backed parallelism is a separate design.

Done when:

- The LSP uses tasks for responsiveness/readiness without changing compiler
  correctness semantics.
- There is no claim of CPU parallelism from this task backend.

## Final acceptance

The implementation is complete when:

- Task operations lower only through abstract suspension intrinsics.
- JSPI scheduler semantics match the design: eager enqueue, FIFO cooperative
  yield, await parking, top-level pseudo-task, drain, deadlock detection, and
  failure propagation.
- `Task.sleep` and `Task.read_stdin` work without busy-spinning and without false
  deadlock.
- Stackful suspension works at arbitrary call depth, including helpers,
  recursion, branches, and higher-order callbacks.
- Non-JSPI runtimes fail task programs clearly.
- Phase B evidence is recorded.
