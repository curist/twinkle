# LSP Task Migration Plan

Status: **In progress**.

This plan tracks CP19 from `task-concurrency-jspi-fiber-implementation.md`: moving the LSP transport and background work onto Twinkle `Task` primitives after the JSPI scheduler and Task ABI are in place.

## Scope

- Use cooperative tasks to keep stdin readiness, JSON-RPC dispatch, debounce timers, and diagnostics publication from being tied to one inline polling loop.
- Preserve LSP protocol semantics and existing server-core behavior.
- Keep stale diagnostics suppressed with generation tokens.
- Do **not** claim or depend on CPU parallelism. The current JSPI backend is cooperative and single-threaded; synchronous compiler analysis runs until it reaches an explicit suspension point.

Out of scope:

- Worker-thread or process-backed parallel compilation.
- Preemptive cancellation of an already-running diagnostic analysis.
- Public Task ABI changes.

## Current checkpoint

Already done:

- The LSP command uses `Task.read_stdin(4096)` instead of timeout polling.
- Shared LSP state is stored in `Cell<server_core.State>` so task callbacks observe the same state.
- Diagnostics debounce uses `Task.spawn` and `Task.sleep`.
- Diagnostics scheduling uses a generation token so stale debounce tasks do not publish.
- `didOpen` now marks diagnostics pending instead of publishing synchronously, so opening a document does not block the dispatcher on workspace analysis.
- Diagnostics debounce waits for an idle window before running the synchronous workspace pass.
- Diagnostics publication emits LSP work-done progress when the client advertises `window.workDoneProgress`; percentages are coarse because the current compiler analysis is synchronous once started.
- Expensive automatic editor refresh requests (`semanticTokens/full`, document symbols, folding ranges, inlay hints) return empty results while diagnostics are pending so save-time refreshes do not block behind compiler work; formatting remains parse/print-only and still returns edits during the diagnostics debounce window.
- The end-game non-blocking direction is captured below in [Non-blocking end-game](#non-blocking-end-game); the key idea is isolating heavy analysis from the foreground dispatcher rather than sprinkling cooperative yields through compiler internals.
- Stage0 can compile task-using boot source via the id-only Task ABI.

## Migration checkpoints

### CP19.1 — Dedicated migration plan

- [x] Keep this document as the LSP-specific task migration checklist.
- [x] State explicitly that this is cooperative single-threaded concurrency, not CPU parallelism.

### CP19.2 — Reader/dispatcher split

Goal: keep stdin readiness in the input reader and move JSON-RPC handling into a dispatcher task.

- [x] Add an internal reader-to-dispatcher handoff structure.
- [x] Keep the reader focused on `Task.read_stdin` and queueing chunks or frames.
- [x] Have a dispatcher task decode/handle messages and write responses.
- [x] Avoid busy-spinning while no input is ready; use cooperative yielding or short sleeps deliberately until a channel abstraction exists.
- [x] Preserve shutdown/exit behavior without leaving the reader parked on stdin after an exit notification has been dispatched.

Implementation note: `boot/commands/lsp.tw` currently uses a local `Cell`-backed chunk queue. The reader waits for queued chunks to drain before parking on stdin again, which lets an `exit` notification take effect promptly even without a first-class channel wakeup primitive.

### CP19.3 — Diagnostics worker model

Goal: make debounce tasks schedule diagnostics work instead of doing the publish inline.

- [x] Keep debounce tasks timer-only: sleep, check generation, then enqueue a diagnostics job.
- [x] Run diagnostics publication from a worker-style task.
- [x] Check the generation before starting diagnostics and again before publishing responses.
- [x] Document that a heavy synchronous analysis is not preemptively cancelled once started.

Implementation note: generation checks suppress stale diagnostics before work starts and before responses are written. Because the scheduler is cooperative, analysis that does not yield still runs synchronously once started; this is why the stronger non-blocking design needs real worker isolation.

### CP19.4 — Validation

- [x] Format edited Twinkle sources.
- [x] Run `target/twk lint boot/main.tw`.
- [x] Build `boot/main.tw` to WAT to catch compile/type errors.
- [x] Run `target/twk run boot/tests/main.tw`.
- [x] Confirm `cargo test --release` is not required for the current slice because stage0/Rust were not changed.
- [x] Keep task behavior tests that can deadlock or trap in a subprocess harness rather than the in-process boot runner.

## Non-blocking end-game

CP19 is cooperative and in-process, so a CPU-heavy compiler phase still monopolizes
the process until it yields. A true non-blocking *guarantee* needs the points below.
They are not yet implemented; this captures the direction so it isn't lost.

**Prerequisite that does not exist yet.** Real isolation needs a worker process (or
Web Worker / `worker_threads`) whose OS-level preemption keeps the foreground
responsive even across a non-yielding compiler phase. But `@std.proc` exposes only
`args/env/cwd/exit/run_wasm`, and the JS host has no child-process/worker wiring.
So the first real work item is **host primitives**: spawn, kill, and a framed
bidirectional channel with crash/EOF signalling, with stage0 parity. Everything
below is gated on this.

**Two tiers.** Most of the *felt* responsiveness is backend-neutral and can ship on
the current cooperative scheduler (snapshots, freshness policy, ordering, single
writer, save-vs-edit gating). Only the airtight guarantee against a non-yielding
phase needs the worker. Sequence the cheap tier first so the design doesn't stall on
the hard part.

**Guiding invariants** (these keep the design race-free as the LSP grows):

- *Sole-owner actor.* One foreground loop is the only writer of LSP state and the
  only emitter of client JSON-RPC. Workers send internal framed messages
  (`ready`/`heartbeat`/`result`/`log`); they never write to the wire directly.
- *Per-root state.* Snapshots and job/worker control state are keyed by project
  root, and kept separate from each other — a global generation over-cancels
  unrelated roots. (Generalizes the existing single `gen` token.)
- *Message ordering.* State-mutating notifications (`didOpen/didChange/didClose/
  didSave`) apply in strict receipt order; priority may reorder responses/internal
  events but never a request ahead of an unapplied notification.
- *Stale-but-consistent + version gating.* Features answer from the latest immutable
  snapshot rather than blocking. Positional results (semantic tokens, inlay hints,
  folding, symbols) are keyed by `(URI, version)` and never remapped onto newer text.
- *Cancellation = discard + supervise.* A CPU-bound worker won't read a cancel
  message promptly, so cancellation means the foreground discards stale-generation
  results (always works), plus a heartbeat-driven supervisor that kills/restarts a
  worker that won't quiesce. No cooperative cancel-polling in the compiler.
- *Progress reflects foreground intent*, not worker call-stack state: end progress
  immediately on supersede/crash so spinners can't hang.

**Serialization boundary (v1).** Keep live compiler objects off the channel: in =
source overlays + roots + versions + target generation; out = diagnostics + snapshot
+ logs, all generation-tagged. A reusable cache crosses only if it has an explicit
stable serializable schema; otherwise it stays worker-local and is rebuilt.

**Highest-value near-term lever — save-vs-edit gating.** Whole-workspace type
checking on every debounced keystroke is the CPU sink. Route edits to lightweight
analysis (parse/local names) and full workspace analysis (type check, cross-file
diagnostics, index rebuild) to `didSave`. This captures most of the savings a tiered
incremental query engine would give, on the current batch compiler, with no
compiler-architecture change.

**End-game shape (not near-term).** A layered snapshot
(`{ parse, symbols, types, index }`) lets each feature read only the tier it needs.
Serving tiers *cheaply*, though, needs a demand-driven query engine in the compiler
(Twinkle is whole-program batch today) — a separate, large epic. Anticipate the
snapshot *shape* now; defer the engine. Also avoid hard-coding "exactly one worker"
so a slow index job can later run beside fast diagnostics.

**Acceptance targets.** Format-on-save p99 < ~20 ms while diagnostics run (CP19
already reached ~0.013 s); hover/completion answer from snapshot < ~20 ms or return
empty immediately; no stuck progress after supersede/crash; no stale positional
result rendered against newer text.

## Notes for future work

The queueing in CP19 can remain local to the LSP command. If other compiler commands need similar coordination, introduce a small stdlib channel/queue abstraction with explicit readiness semantics instead of duplicating polling loops.
