# Task Concurrency — Stackful Tasks via JSPI Backend

Status: **Design approved (2026-06-18); implementation not started.**

This document defines Twinkle's **stackful** implementation direction for
`Task<T>`. The public API remains backend-independent. The near-term backend uses
JSPI because Node/Deno already provide stack switching through
`WebAssembly.promising` / `WebAssembly.Suspending`. The compiler lowers task
operations to **abstract suspension intrinsics**, which the JSPI backend
currently binds to host imports. A future Wasm continuations backend can satisfy
the same intrinsic interface without changing user code.

The thesis: **Twinkle commits to a backend-neutral, stackful `Task<T>` model;
JSPI is the current backend, not the semantic foundation.**

The earlier stackless design rationale lives in
[task-concurrency.md](task-concurrency.md); the public `Task<T>` API and the
"Why Task instead of Fiber" discussion there still apply.

## Layered model

The design is four layers, of which only the first is user-visible:

1. **Stable surface / API** (backend-independent) — `Task.spawn`, `Task.await`,
   `Task.yield`, `Task.sleep`, `Task.read_stdin`.
2. **Abstract suspension backend** (compiler lowering target) — `task_create`,
   `suspend_await`, `suspend_yield`, `suspend_sleep`, `suspend_read_stdin`. The
   compiler lowers the surface to these and *never* to a named host import.
3. **Current binding: JSPI** — the intrinsics bind to JS host imports; the
   scheduler lives in JS; task bodies run through
   `WebAssembly.promising(__task_run)`.
4. **Future binding: Wasm continuations** — the same intrinsics bind to an
   in-Wasm continuation runtime; the scheduler can move back into Wasm; the JSPI
   per-suspension cost disappears for pure task scheduling, leaving only true
   host-readiness operations (`sleep`, stdin, future file/network I/O) crossing
   the host boundary.

Layers 1–2 are the stable contract. Layers 3–4 are interchangeable
implementations behind the same intrinsic interface.

## Why stackful, and why JSPI now

The stackless Milestones A–C built a large amount of machinery whose only purpose
was to *emulate* stack switching on an engine that lacked it: a CFG-based
state-machine splitter, per-body frame structs and generated resume functions, a
transitive suspension-effect analysis (function coloring), and the associated
boundary diagnostics (rejecting yield-capable functions used as first-class
values, rejecting suspending callbacks to higher-order functions). All of it is
accidental complexity relative to the concurrency model itself.

The host runtime already ships JSPI: `runtime.mjs` wraps `__twinkle_start` with
`WebAssembly.promising` and several host imports with `WebAssembly.Suspending`,
and `twk run` goes through that async path when `hasJspi` is true
(`typeof WebAssembly.Suspending === "function" && typeof WebAssembly.promising
=== "function"`). Stack switching is therefore available *today* in the target
host. With real fibers, `Task.yield` is a stack switch: any function may suspend
at any call depth, including inside arbitrary higher-order callbacks and
recursion, with no compiler transform and no coloring. The entire Milestone B/C
problem space disappears.

### Position of the stackless plan

The stackless plan remains useful as a **portability fallback and reference**,
but it is not the preferred implementation, because it makes ordinary
direct-style suspension impossible without function coloring or whole-program
transforms. JSPI gives us the semantics we want *now*; Wasm continuations give us
the portability/performance story *later*. Both are bindings of the same
intrinsic interface, so neither is the semantic foundation.

### Backend cost (current binding)

JSPI imposes a host-boundary/Promise cost at each suspension. This is acceptable
for the current LSP-oriented workload and avoids the compiler state-machine
complexity entirely. If Wasm continuations become available, pure scheduler
operations (`yield`, `await`, task wakeups) can move to native Wasm stack
switching, leaving only true host-readiness operations (`sleep`, stdin,
file/network I/O) to cross the host boundary.

### Accepted tradeoff (current binding only)

With the JSPI binding, tasks are **JSPI-host-only**: on a non-JSPI Wasm engine,
task programs are unsupported. This is acceptable because the near-term consumer
is the boot compiler's LSP, which runs on Node/Deno (JSPI-capable), and because
the constraint is a property of the *binding*, not the model — the continuations
binding lifts it. The stackless implementation is preserved on the
`archive/stackless-task-concurrency` branch (tip `4a83356`).

`main` was reset to `310a4ec` (just before Milestone A), keeping the Phase 1/2
baseline: the `Task<T>` type, `spawn`/`await`/`yield` builtin signatures, and
the older in-Wasm scheduler + straight-line transform. The frontend scaffolding
is reused; the backend is replaced.

## Architecture (current JSPI binding)

Everything in this section describes layer 3 — the JSPI binding of the abstract
intrinsics. It is the current implementation, not the semantic foundation; a
continuations binding would replace this section while leaving layers 1–2 intact.

### Core idea

Under the JSPI binding the scheduler lives in the JS host, and each task body
runs on its own JSPI suspendable stack. The suspension intrinsics
(`suspend_await`/`yield`/`sleep`/`read_stdin`) bind to `WebAssembly.Suspending`
host imports that return a Promise the JS scheduler controls. There is **no
compiler suspension transform** — task bodies are ordinary Twinkle closures.

### Execution mechanism

- `__twinkle_start` is already `promising`; top-level code runs on a suspendable
  stack and may itself await tasks.
- A single exported trampoline `__task_run(closure) -> anyref` is wrapped with
  `promising`. To start (or drive) a task, the JS scheduler calls
  `promising(__task_run)(closure)`, obtaining a completion Promise `p` that
  resolves with the task's boxed result when the body finishes.
- When a body calls a Suspending import, its stack suspends, `p` stays pending,
  and control returns to the JS event loop. The scheduler then runs other
  runnable tasks.

### The JS scheduler

A small cooperative scheduler in `runtime.mjs`, holding:

- a **runnable queue** of pending resolvers (tasks ready to resume),
- a **blocked set** of await-waiters keyed by target task id,
- **pending-host accounting** (in-flight timers / stdin reads),
- a **task registry** keyed by integer `task_id` (state, completion promise,
  result-or-failure),
- a **current-context id** (`scheduler.current`), see below.

#### Handle ABI: integer ids only

JS can hold an opaque Wasm reference, but it **cannot** construct a
module-defined `rt_types__Task` GC struct or read its fields. So all host imports
operate on a plain **`i32 task_id`**, never on the `Task<T>` struct. The Wasm
side wraps a returned id into the `Task<T>` struct (and unwraps the id when
passing a task back to `suspend_await`). The only opaque ref that crosses the
boundary is the spawned **closure** (passed to `task_create`, handed back to
`__task_run`), which JS holds without inspecting.

#### Current-execution-context model

A Suspending import must know *which* logical task is calling it. We do not
thread a self-id through Twinkle code (that would reintroduce coloring). Instead
the scheduler maintains `scheduler.current`: the id of the task (or top-level)
whose stack is executing. It is set immediately before the scheduler resumes a
stack and, because JS is single-threaded, stays valid throughout that stack's
synchronous run until the next suspend. Every suspension intrinsic consults
`scheduler.current` to identify its caller.

**Top-level is a pseudo-task** with reserved id `0`, present in the registry so
it can be parked as an await-waiter. `__twinkle_start` runs on its own
`promising` stack; `scheduler.current` is `0` until the first task resume. This
is what makes top-level `Task.await` work without special-casing.

The JSPI binding implements one host import per abstract intrinsic (layer 3
binding of layer 2):

```text
task_create(closure: anyref) -> i32
suspend_await(task_id: i32)  -> anyref
suspend_yield()              -> void
suspend_sleep(ms: i64)       -> void
suspend_read_stdin(max: i64) -> anyref   ;; boxed Vector<Byte>
```

| Import | Kind | Behavior |
|---|---|---|
| `task_create(closure) -> i32` | sync | Register the closure, allocate a `task_id`, enqueue the task to start on the next scheduler tick, and return the id. Does **not** run the body synchronously (eager-enqueue semantics). |
| `suspend_await(target_id) -> anyref` | Suspending | Caller = `scheduler.current`. If the target is done, resolve immediately with its boxed result; if it failed, re-trap the caller; otherwise park the caller as a waiter on the target and resolve when the target settles. |
| `suspend_yield()` | Suspending | Re-enqueue `scheduler.current` at the back of the runnable queue; resolve on the next scheduler tick. |
| `suspend_sleep(ms)` | Suspending | `setTimeout(resolve, ms)`; counts as pending host while in flight; resumes `scheduler.current`. |
| `suspend_read_stdin(max) -> anyref` | Suspending | Reuse the existing async stdin read; resolve with the boxed chunk (empty `Vector<Byte>` at EOF). Counts as pending host while in flight. |

Cooperative semantics fall out of the event loop: because JS is single-threaded
and the currently-running Wasm stack holds control until it suspends or
completes, a `Task.spawn` issued by a running task does not start the spawned
body until the current stack next yields control to the event loop.

### `Task<T>` representation and marshaling

`Task<T>` remains a Wasm-owned GC struct containing an integer `task_id`. **JS
never constructs or inspects `Task<T>` structs directly.** Lowering unwraps the
id before calling scheduler imports and wraps returned ids back into `Task<T>`
values; the JS scheduler keys task records by that id. Task identity is id
equality (consistent with the existing reference-equality design decision).
Closures and boxed task results cross the boundary as opaque refs — the spawned
closure (JS holds it, hands it back to `__task_run`) and the boxed result — both
already marshaled by the existing bridge.

Boxing is asymmetric:

- **`Task.spawn`** passes only the closure (and receives the `task_id`). No
  result exists yet, so there is nothing to box here.
- **Result boxing** happens inside `__task_run` / the universal closure-return
  trampoline, which boxes the body's return value to `anyref` once.
- **Unboxing** happens at the **await** site, where the static result type `T`
  is known, via `emit_unbox_from_anyref`.

## Compiler changes

### Remove (backend)

- `boot/compiler/codegen/runtime/sched.tw` — the in-Wasm scheduler trampoline,
  queue, and run-to-completion resume function.
- `boot/compiler/codegen/emit/task_resume.tw` and the CFG-based state-machine
  splitter — the suspension transform.
- `boot/compiler/backend/task_validate.tw` — suspension-position validation.
- `boot/compiler/backend/task_effect.tw` (when re-added by A–C; absent on the
  current baseline) — suspension-effect analysis / function coloring.
- The frame-stack runtime types (`FrameBase`, per-body frames) in
  `runtime/types.tw`, and the await fast/slow-path lowering in `emit.tw`.

### Keep / adjust (frontend + thin backend)

- `Task<T>` type, builtin registration, and type-checking remain.
- Builtin signatures: keep `Task.spawn`/`Task.await`/`Task.yield`; add
  `Task.sleep(ms: Int) Void` and `Task.read_stdin(max: Int) Vector<Byte>`.
- Backend lowering becomes trivial. **Normative rule: the compiler must not lower
  `Task.*` directly to JS host import names. It lowers to backend-neutral
  suspension intrinsics.** The JSPI backend binds those intrinsics to host
  imports; a future continuations backend can bind them to in-Wasm runtime
  functions. Each of `Task.spawn`/`await`/`yield`/`sleep`/`read_stdin` lowers to
  its intrinsic (`task_create`, `suspend_await`, `suspend_yield`,
  `suspend_sleep`, `suspend_read_stdin`), with the `Task` struct wrap/unwrap
  around the `i32` id and result unboxing at the await site. A single backend
  binding table is the only place that knows a concrete binding exists. No
  transform pass; no validation pass.

A direct consequence: suspension is legal **anywhere** — arbitrary call depth,
inside higher-order callbacks, recursive helpers — and needs no diagnostics.

## API surface

```tw
type Task<T>

fn Task.spawn<T>(f: fn() T) Task<T>
fn Task.await<T>(t: Task<T>) T
fn Task.yield() Void
fn Task.sleep(ms: Int) Void
fn Task.read_stdin(max: Int) Vector<Byte>
```

`Task.read_stdin` follows the existing `io.read_stdin_chunk` convention: an empty
`Vector<Byte>` signals EOF (distinguishable via `io.stdin_eof()`).

`sleep` and `read_stdin` are the host-readiness primitives that motivated the
milestone: `sleep` for LSP debounce windows, `read_stdin` to feed an LSP framing
buffer by parking until input (or EOF) arrives.

**Non-goal: no user-facing Fiber API.** "Fiber" here names only the
implementation mechanism (the per-task JSPI suspendable stack). The public
surface is `Task<T>` and the operations above — there is no `Fiber.resume`,
`Fiber.yield`-with-value, exposed fiber state, or manual control transfer.
Keeping the surface at the `Task<T>` altitude is also what keeps the
forward-migration seam free: the backend can change because the public API never
commits to coroutine semantics.

## Lifecycle and deadlock

The process stays alive while the top-level promise is pending or while tasks or
host operations are outstanding. At top-level completion the scheduler drains
unawaited runnable tasks (preserving "a spawned task is a commitment to
execute"). Deadlock is detected in JS: when the runnable queue is empty, no host
operations are pending, and blocked tasks remain, the scheduler throws a
trap-equivalent error (mirroring the old in-Wasm `sched_blocked > 0` check).

## Failure and trap propagation

A task body can fail two ways under JSPI: the body traps (its `promising` call
rejects), or a suspending import rejects (e.g. a host I/O error). Either way the
completion promise rejects. This is consistent with the existing decision that a
task-body trap is an unrecoverable trap; recoverable failure stays explicit in
the value via `Result<T, E>` and is *not* a separate task-failure channel.

Scheduler rules:

- On rejection, the task record is marked **failed** (storing the error), the
  task leaves the runnable/blocked sets, and pending-host counters are
  decremented as for completion.
- **Awaiting a failed task re-traps the awaiter**: `suspend_await` on a failed
  target rejects the awaiter's resume promise, which propagates the trap up that
  task's stack (and recursively to *its* awaiters). A chain of awaiters all trap,
  matching synchronous trap semantics.
- An **unawaited failed task surfaces as a program-level error**: during drain,
  if a task failed and no one awaited it, the scheduler propagates the failure as
  the program's failure rather than swallowing it. (It does not silently
  disappear the way a fire-and-forget success would.)
- No `Task.try_await`/`Result`-returning await in the MVP. A future
  trap-catching design can add one; this matches the deferred decision in the
  superseded doc.

## Decisions

- **`Cell.update` with a suspending callback** (no static rejection is cheap
  without coloring): documented as discouraged/undefined for the MVP. Revisit
  with a runtime reentrancy guard only if it bites in practice.
- **JSPI per-suspend overhead** (stack switch cost): noted, not optimized. Tasks
  target coarse-grained (LSP-scale) concurrency. No yield-coalescing or fast
  paths until a real workload demonstrates a problem.

## Portability and stage0

Tasks require a JSPI host (`hasJspi`). On a non-JSPI engine, task programs are
unsupported and must fail with a clear error rather than silently misbehaving.
Stage0's responsibility shrinks to emitting the host import calls — no scheduler,
no transform. Because boot source does not yet use tasks, stage0 stays clean;
when boot source (the LSP) adopts tasks, stage0 must emit the same host import
calls, which is trivial codegen.

## Future migration: native Wasm stack switching

JSPI is a host-coupled fiber mechanism. The truly portable fiber primitive is the
WebAssembly **stack-switching / typed-continuations** proposal (`cont` types,
`cont.new` / `resume` / `suspend`), which keeps the scheduler *in Wasm* with no
JS dependency. When it is broadly available in the target engines, Twinkle should
be able to migrate the backend without changing the public `Task<T>` API. This
section records how to keep that migration cheap.

### The seam: an abstract suspension interface

The single most important design rule for migratability is to **not** let the
compiler lower `Task.yield`/`await`/`sleep`/`read_stdin` to *named JS host
imports directly*. Instead lower them to a small set of **abstract suspension
intrinsics** (e.g. `suspend_yield`, `suspend_await`, `suspend_sleep`,
`suspend_read_stdin`, `task_create`). A backend then *binds* those intrinsics to
a concrete implementation:

- **JSPI backend (this design):** bind to `WebAssembly.Suspending` host imports;
  the scheduler is the JS event loop.
- **Continuations backend (future):** bind to in-Wasm runtime functions that use
  `cont.new`/`resume`/`suspend`; the scheduler is portable Wasm again (close to
  the structure of the archived stackless scheduler, but with real stacks instead
  of generated frames).

Because the compiler's front-to-mid pipeline only ever sees the abstract
intrinsics, swapping backends is a localized change in codegen + runtime, not a
recompile of the whole task feature. Keep the intrinsic set minimal and stable.

### What changes, what stays

- **Stays:** `Task<T>` type and id-based identity; the `spawn`/`await`/`yield`/
  `sleep`/`read_stdin` surface; result boxing/unboxing; "spawn is a commitment"
  drain semantics; FIFO + deadlock policy.
- **Moves back into Wasm:** the scheduler (runnable queue, blocked set,
  pending-host accounting, deadlock check), now driving real continuations rather
  than calling promising/Suspending.
- **Still host-backed:** `sleep` and `read_stdin` are inherently host operations.
  Under continuations they integrate the way the old "scheduler self-suspends"
  model described: a parked task increments a pending-host counter and registers a
  completion; a host callback re-enqueues the continuation; when idle-but-pending,
  the scheduler suspends *itself* (one continuation) to the host event loop and
  resumes on the callback. JSPI may still be the mechanism for *that single
  scheduler-level wait* even after task suspension uses native continuations.

### Migration shape (incremental, dual-backend)

1. Land the abstract suspension intrinsic boundary as part of the JSPI backend so
   the seam exists from day one (no extra cost — it is just naming discipline).
2. When continuations ship behind a flag, add a second backend binding behind a
   capability check, leaving JSPI as the fallback for engines without
   continuations. Both backends satisfy the same intrinsic interface.
3. Re-home the scheduler into portable Wasm for the continuations backend, reusing
   the archived stackless scheduler's queue/deadlock structure (the data
   structures are backend-agnostic; only the suspend/resume primitive differs).
4. Restore portability: with the continuations backend, task programs run on any
   engine that implements the proposal, including non-Node/Deno targets and
   potentially stage0-emitted Wasm without a JS scheduler.

### Implication for this milestone

Adopt the abstract-intrinsic discipline now (Slice 1/2): name the lowering
targets after the *operation*, not the *host*, and route them through one binding
table. This is the only forward-migration work that belongs in the current
effort; everything else is deferred until the proposal is real.

## Spike: JSPI switching cost

Measure the cost of JSPI suspension in the shapes this design actually uses. The
goal is not to micro-optimize; it is to decide whether the JSPI backend is
comfortably good enough for LSP-scale cooperative concurrency, and to separate
JSPI stack-switching cost from host-readiness latency. The spike has two phases
with different prerequisites — they cannot all run "before the scheduler exists,"
since most of the interesting numbers *are* scheduler numbers.

- **Phase A — gate (before Slice 2).** Raw JSPI cost with no scheduler. Decides
  whether to build the JSPI backend at all.
- **Phase B — validation (after Slices 2–3).** Scheduler overhead and the
  LSP-shaped decision benchmark, on the real `Task.*` lowering.

### Budget (decide the threshold up front)

Set the bar before seeing numbers, to keep the decision honest. The intended
workload is coarse-grained: a debounce window is ~50–250 ms, stdin/diagnostics
waits are milliseconds. So:

- **Proceed unchanged** if a `yield`/`await` round-trip is on the order of **tens
  of microseconds or less** (≥3 orders of magnitude below a debounce window).
- **Proceed but avoid fine-grained yielding in library code** if it is hundreds
  of microseconds.
- **Reconsider** (batching/yield-coalescing, or defer task adoption to native
  continuations) if a single round-trip approaches **~1 ms** or the LSP-shaped
  smoke spends a meaningful fraction of wall time in suspension overhead rather
  than useful work or real host waits.

Express raw JSPI cost as a multiple of a plain host-import call (Phase A,
step A0) so "good enough" is interpretable rather than absolute.

### Questions to answer

- What is the lower-bound cost of a Wasm stack suspending through a
  `WebAssembly.Suspending` import and resuming through a `promising` export —
  both when the Promise is already resolved and when it resolves on a later tick?
- How much overhead does the task scheduler add on top of raw JSPI suspension for
  `Task.yield()` and blocked/resumed `Task.await()`?
- Is the cost small relative to the intended workload (stdin readiness, debounce
  timers, request dispatch, diagnostics work) per the budget above?
- Are Node and Deno (and the bundled `twk`) close enough for the CLI target, or
  do we need a runtime/version guard beyond `hasJspi`?

### Phase A — gate (no scheduler)

A0. **Plain host call anchor:** a non-suspending extern called in a loop. This is
   the denominator for expressing JSPI cost as a multiple.
A1. **Baseline loop:** an empty counted Wasm loop, so host timing overhead and
   loop code are visible.
A2. **Raw JSPI, already-resolved:** call a temporary extern (e.g.
   `bench.suspend_now()`) wrapped in `WebAssembly.Suspending` returning an
   **already-resolved** Promise. Caveat: an engine may take a fast path here that
   does *not* fully unwind/switch the stack, under-reporting true cost.
A3. **Raw JSPI, next-tick:** same, but the Promise resolves on a later microtask
   /macrotask (`queueMicrotask` or `setTimeout(0)`). The A3−A2 gap reveals whether
   the engine fast-paths already-resolved promises; **A3 is the honest floor** for
   a suspension that actually had to wait.

If A2/A3 already blow the budget, stop here and revisit the backend before
building the scheduler.

### Phase B — validation (after Slices 2–3)

B1. **Scheduler yield:** one task repeatedly calling `Task.yield()` — queue
   bookkeeping plus JSPI stack switching.
B2. **Scheduler ping-pong:** tasks alternating yield/await on one another, so
   waiter registration and completion wakeups are included.
B3. **Host readiness:** measure short `Task.sleep` and `Task.read_stdin(max)`
   separately. Note `setTimeout(_, 0)` is clamped (~1 ms in Node), so `sleep(0)`
   measures the timer floor, not switching; include a `queueMicrotask`-style
   "yield to the loop" as a cheaper readiness baseline distinct from timers. These
   numbers include timer/stream latency and are not pure switching cost.
B4. **LSP-shaped smoke (the decision benchmark):** input-reader, dispatcher, and
   debounce tasks doing small units of work. The backend is acceptable if this
   shape spends its time in useful work or real host waits, not in suspension
   overhead, per the budget.

### Cross-runtime step

Run Phase A (and, once it exists, B4) on **Node, Deno, and the bundled `twk`**,
and compare. This is what actually answers the parity question; record per-runtime
numbers, not a single representative run.

### Harness shape

- Put the temporary harness under `boot/bench/` (the existing bench convention)
  or another throwaway bench directory — not in the compiler test suite.
- Use the same `tools/js_runtime/runtime.mjs` async path as `twk run`, and fail
  clearly when `hasJspi` is false.
- Time from the JS host with `performance.now()`; avoid printing inside measured
  loops.
- Warm the module before measuring and run each case enough times to smooth out
  JIT and event-loop noise.
- Each suspend allocates a Promise + resolver + queue entry; for the tight
  yield/ping-pong cases, note GC pressure (or declare it out of scope). It is
  negligible at LSP scale but can distort a microbenchmark.
- Record runtime details with the result: Node/Deno version, platform, whether
  the CLI bundle or direct runtime was used, and the Twinkle commit.
- Store summarized results in this document's Evidence section when the spike is
  complete.

### Interpreting results

Apply the budget above. If `yield`/`await` overhead is insignificant next to LSP
debounce windows, stdin waits, and diagnostics work, proceed with the JSPI
backend unchanged. If it is visible but still acceptable, keep the backend and
avoid fine-grained yielding in library code. If it dominates the LSP-shaped smoke,
revisit batching, yield-coalescing, or delaying task adoption until native Wasm
continuations are available.

## Risks and open items

- **JSPI availability across target Node/Deno versions** must be confirmed for
  the CLI bundle; `hasJspi` gates it at runtime today.
- **Eager-enqueue timing**: verify the spawn-then-suspend ordering matches the
  documented backward-compatibility note (main prints before a spawned task).
- **GC rooting**: JS holds task closures and boxed results as opaque refs; confirm
  the engine keeps them alive for the task lifetime.
- **Reentrancy of the trampoline**: nested `promising` stacks (top-level awaiting
  a task that awaits another) must compose correctly under JSPI.

## Slice breakdown

1. **Strip the stackless backend + establish the intrinsic seam.** Remove the
   scheduler/transform/validation. Re-lower `Task.*` to the **abstract suspension
   intrinsics** (`task_create`, `suspend_await`/`yield`/`sleep`/`read_stdin`)
   routed through one backend binding table — the migration seam, in place from
   day one. Intrinsics may be temporarily stubbed/rejected at runtime. `make
   stage2` green; existing non-task suites green. Mark the stackless design doc
   superseded.
2. **JS scheduler + core fibers.** Bind `task_create`/`suspend_await`/
   `suspend_yield` to host imports operating on **`i32 task_id`** (not the `Task`
   struct), add the `promising`-wrapped `__task_run(closure)` trampoline, the
   registry, the runnable/blocked sets, the `scheduler.current` context, and the
   top-level pseudo-task id `0`. Result boxing in `__task_run`, unboxing at await.
   Tests via `twk run`: spawn/await round-trip, interleaving, await-after-yield,
   unawaited drain, deadlock detection, and **failure propagation** (awaiting a
   trapped task re-traps; unawaited failure surfaces at drain).
3. **Host readiness.** Bind `suspend_sleep` and `suspend_read_stdin`; tests for a
   timer waking an awaiting task, stdin readiness waking a reader task without
   busy-spinning, and deadlock-vs-pending-host distinction.
4. **(Later) LSP migration.** Move `twk lsp` onto cooperative tasks (input
   reader, dispatcher, debounce, diagnostics), with generation tokens for stale
   results. Tracked separately.

## Evidence

### Phase A spike — GO (2026-06-18)

Raw JSPI suspend/resume cost, no scheduler. Harness:
`boot/bench/jspi/{phase_a.tw,run.mjs}` (throwaway), run through the same
`runWasmBytesAsync` async path as `twk run`, providing the `bench` externs so we
control whether each suspends. `N=100000` per loop, 7 runs (median), 2 warmup.
Platform: darwin arm64. Commit: `f3c4e9c` + uncommitted bench.

| Measurement | Node 26.0.0 | Deno 2.8.3 |
|---|---|---|
| A1 baseline loop (no suspend) | 0.0009 µs/iter | 0.0002 µs/iter |
| A2 suspend, already-resolved promise | 0.085 µs/iter | 0.081 µs/iter |
| A3 suspend, microtask-deferred (real) | 0.112 µs/iter | 0.118 µs/iter |
| A3 / A2 (resolved-promise fast-path tell) | 1.31× | 1.45× |
| A3 − A1 (real per-suspend cost) | 0.111 µs/iter | 0.117 µs/iter |

Findings:

- A real JSPI suspend costs **~0.11–0.12 µs (≈110–120 ns)** on both runtimes —
  2–3 orders of magnitude below the "tens of µs → proceed" budget and ~4 below
  the "~1 ms → reconsider" threshold. **Proceed with the JSPI backend unchanged.**
- The engine **does** fast-path already-resolved promises (A3/A2 ≈ 1.3–1.45×), so
  A2 alone would under-report a true suspend — A3 is the honest floor. Both were
  measured; the gap is small in absolute terms.
- **A0 note (recorded):** the runtime wraps *every* extern in
  `WebAssembly.Suspending` under JSPI, so a truly non-suspending host call is not
  reachable on the real `twk run` path. The realistic "plain extern" cost is the
  already-resolved suspend (A2 ≈ 0.08 µs). A1 (a bare Wasm loop iteration,
  ≈1–2 ns) is the Wasm-call-scale denominator.
- **Node/Deno parity:** within ~6% on the real-suspend number. No runtime/version
  guard beyond `hasJspi` is needed. (Both are V8-derived; a non-V8 JSPI engine
  would warrant a re-check, but none is a current target.)
- Phase B (scheduler overhead + LSP-shaped decision benchmark) runs after Slices
  2–3 on the real `Task.*` lowering.

Commands:
`target/twk build boot/bench/jspi/phase_a.tw -o boot/bench/jspi/phase_a.wasm`,
`node boot/bench/jspi/run.mjs`, `deno run -A boot/bench/jspi/run.mjs`.
