# Channels Phase 2 — LSP migration handoff

Start-of-session note for **Phase 2**: replace the LSP server's hand-rolled
`Cell`-backed queues and 1ms poll loops with the `Channel<T>` primitive shipped in
Phase 1. Read `docs/plans/channels.md` (design) and `channels-impl-handoff.md`
(Phase 1 map) first.

## State going in

- **Phase 1 done + committed on `main`** (unpushed): codegen fix `0579db04`,
  feature `b5264ff1`. `Channel<T>` works: `Channel.new()` / `Channel.bounded(n)`,
  `ch.send(v) Bool`, `ch.recv() T?`, `ch.close()`, `for v in ch` (drains until
  close, including **inside a `Task.spawn` closure** — the Phase 1 codegen fix).
- Gates to re-confirm before starting: `make bundle-cli` (self-host fixed point) +
  `target/twk run boot/tests/main.tw` (2776) green.
- Whole series (LSP progress, responsiveness, Task.yield fix, channels) is still
  **unpushed**.

## Goal / payoff

`boot/commands/lsp.tw` currently moves data between three tasks (stdin reader =
top-level, dispatcher, diagnostics worker) via two `Cell`-backed queues plus
`time.sleep(1)` poll loops. Replace them with channels so consumers **park**
instead of polling: removes the busy 1ms loops (lower idle CPU, instant wake),
deletes shared `Cell` state + the `pending` counter, and folds exit into channel
close. No new programming model — just the Phase 1 primitive.

## What to replace (file: `boot/commands/lsp.tw`)

Current structures:
- `ChunkQueue = .{ items: Cell<Vector<Vector<Byte>>>, closed: Cell<Bool>, pending: Cell<Int> }`
  with `push_chunk` / `pop_chunk` / `finish_chunk` / `has_pending` / `close_chunks`.
- `DiagnosticsQueue = .{ items: Cell<Vector<Int>>, closed: Cell<Bool> }` with
  `push_job` / `pop_job` / `close_diagnostics`.
- `LoopCtx.{ chunks: ChunkQueue, diagnostics: DiagnosticsQueue, ... }`.
- `wait_for_dispatcher(ctx)` — polls `has_pending()` with `time.sleep(idle_sleep_ms)`.
- `dispatch_loop` — `case pop_chunk() { .Some => process_buffer + finish_chunk + Task.yield, .None => if closed break else time.sleep(1) }`.
- `diagnostics_loop` — `case pop_job() { .Some => run_diagnostics_job, .None => if closed/should_exit break else time.sleep(1) }`.
- `run_lsp_command` reader loop: `read_stdin_chunk` → `push_chunk` → `wait_for_dispatcher`; on EOF/exit `close_chunks` + `dispatcher.await()`, `close_diagnostics` + `diagnostics.await()`.

Target:
- `ctx.chunks: Channel<Vector<Byte>>`, `ctx.diagnostics: Channel<Int>` (job gens).
- Reader: `ctx.chunks.send(chunk)` (drop `push_chunk` + `wait_for_dispatcher`).
- Dispatcher task body: `for chunk in ctx.chunks { buffer = process_buffer(buffer.concat(chunk)) }` (drop `pop_chunk`/`finish_chunk`/`pending`/the `.None` sleep; `Task.yield` after each chunk is optional — keep if you want to interleave, but channel recv already parks).
- Diagnostics worker: `for gen in ctx.diagnostics { run_diagnostics_job(gen) }`.
- `schedule_diagnostics` debounce task: `ctx.diagnostics.send(my_gen)` instead of `push_job` (keep the `time.sleep(deadline-now)` debounce timer and the `ctx.gen` generation gate — those are unchanged; the timer is a timer, not a poll).
- Delete `wait_for_dispatcher`, the queue types + helpers, `has_pending`, `pending`, and `idle_sleep_ms` if nothing else uses it.

## Design decisions to make

1. **Chunk channel capacity.** Unbuffered (`Channel.new()`) makes `send(chunk)`
   block until the dispatcher `recv`s — a rendezvous that faithfully reproduces
   today's `wait_for_dispatcher` (the reader already waits for the dispatcher to
   drain before reading the next chunk). A small `bounded(n)` adds pipelining.
   Recommend **unbuffered** first (closest to current semantics, simplest to
   reason about); revisit if input throughput matters.
2. **Diagnostics channel capacity.** Multiple debounce tasks may `send` gens.
   With the generation gate, only the latest gen matters, so a small
   `bounded(n)` (e.g. 8) avoids debounce tasks parking on a full channel; or
   unbuffered if the worker keeps up. Decide based on whether a parked debounce
   task is acceptable.
3. **Exit / close ordering (the tricky part).** On EOF/`exit`: close the chunk
   channel so the dispatcher's `for chunk in ch` ends, `await` it; then close the
   diagnostics channel, `await` the worker. Ensure the reader (top-level) is not
   parked on `chunks.send` when the dispatcher has already exited — that would be
   a real deadlock now that the scheduler **reports channel deadlocks**
   (`blockedOnChannel`). Drive the shutdown from the side that won't be blocked
   (close from the reader after it stops reading; the dispatcher only recvs).

## What stays unchanged

- `run_diagnostics_job` and its progress sink (the `Task.yield`-based cold-analysis
  responsiveness from commits `6efb805`/`6aca841`) — independent of channels.
- The generation (`ctx.gen`) debounce/supersede logic.
- `process_buffer`, message handling, `write_message`, progress notifications.

## Verification

- `make bundle-cli` self-host fixed point + `target/twk run boot/tests/main.tw`.
- LSP suites: `lsp_server_core_suite`, `lsp_completion_suite`, etc. — these drive
  `publish_due_diagnostics` directly and shouldn't care about the transport change,
  but run them.
- Recreate the cold-analysis format-latency driver (a Python LSP stdio client that
  opens `boot/main.tw`, waits ~300–600ms, sends `textDocument/formatting`, measures
  response latency; baseline ≤ a few hundred ms) and confirm no regression.
- Manual: open the repo in an editor, confirm responsiveness and that idle CPU is
  lower (no 1ms polling).

## Gotchas

- Channel deadlock detection is now live — a mis-ordered close that leaves a task
  parked on `send`/`recv` with nothing else runnable will **abort the server** with
  a deadlock error, not hang. Test the exit path (`shutdown`/`exit`, EOF, and exit
  mid-analysis).
- `for v in ch` inside the dispatcher/worker `Task.spawn` relies on the Phase 1
  codegen fix (`lower_core/closures.tw` ContractCall capture) — already in.
- Boot-only change (no stage0, no runtime change). After editing: `target/twk fmt`
  + `target/twk lint boot/main.tw`, then `make bundle-cli`.
- Optional follow-up unblocked by channels: the dispatcher↔reader handshake and the
  `should_exit` checks may simplify further once the queues are gone — but keep the
  diff focused on the queue→channel swap first, verify, then simplify.
