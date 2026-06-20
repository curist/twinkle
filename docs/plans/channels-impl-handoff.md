# Channels — implementation handoff

Start-of-session note for implementing `Channel<T>`. The full design is
`docs/plans/channels.md` (read it first — this note assumes it). Status: **design
final + reviewed, no implementation yet.**

## Repo state

- Branch `main`. Everything below is **committed but NOT pushed**:
  - `e711c28` LSP per-module progress reporting
  - `6efb805` LSP cold-analysis responsiveness (sink yields)
  - `6aca841` `Task.yield` macrotask-hop fix (scheduler fairness)
  - `211ce65` / `3051ebe` / `26ddfe5` / `8bc53ac` channels design spec (+ reviews)
- Confirm green before starting: `make bundle-cli` reaches a self-host fixed point;
  `target/twk run boot/tests/main.tw` (2764 tests) passes.

## Final API (see spec for semantics)

```tw
Channel.new()                Channel<T>   // unbuffered (rendezvous)
Channel.bounded(capacity)    Channel<T>   // buffered, capacity >= 1 (else traps)
ch.send(v)  Bool      // false = closed (not delivered)
ch.recv()   T?        // .None = closed AND drained (normal termination)
ch.close()            // idempotent
for v in ch { }       // drains until closed (IntoIterator via inherent iter())
```
Decisions locked: unbuffered + bounded only (no unbounded); `recv`→`Option`,
`send`→`Bool` (closed is termination, not an error); `select` deferred; producer
owns close (convention, unenforced); multi-consumer = work queue, not broadcast.

## Implement by mirroring `Task` (it is the template for everything)

The cleanest path is "do what `Task` does." Concrete pointers:

**Runtime scheduler — `tools/js_runtime/runtime.mjs`** (`createTaskScheduler`):
- Task intrinsics are `task_create` / `suspend_await` / `suspend_yield` on
  `s.imports` (~line 795). Add `channel_new` / `channel_bounded` / `channel_send`
  / `channel_recv` / `channel_close` there. `send`/`recv` are
  `new WebAssembly.Suspending(...)` like `suspendAwait`; `close`/`new` are plain.
- Keep `channels: Map<i32, channel>` + an id counter; channel object
  `{ capacity, buffer:[], sendQ:[], recvQ:[], closed:false }`. Park/wake by
  pushing `{kind:"resume", id, fire}` onto `s.runnable` and calling `schedule()`
  (study `suspendAwait`/`schedulerAwareHost` for the exact discipline + the
  `pendingHost`/`blockedOnTask` accounting).
- Add `blockedOnChannel`; update `checkQuiescence` to count it for spawned AND the
  top-level pseudo-task (don't let a top-level `recv` silently exit/hang).
- `recv` resolves to a tagged result `{kind:"value",value}` | `{kind:"closed"}`
  (NOT null sentinel). Detection (`moduleNeedsTasks`) keys on
  `imp.module === "task"`, so register channel imports under module `"task"` and
  nothing else changes.

**Compiler — boot:**
- `boot/compiler/codegen/runtime/task_abi.tw`: `host_module()` returns `"task"`;
  add the channel `ImportDef`s in `imports()` (i32 ids, anyref payload).
- `boot/compiler/builtins.tw`: register the `Channel` builtin type + `intr(...)`
  entries for `Channel.new/bounded/send/recv/close` (see the Task intrinsics block
  ~line 213/499).
- `boot/compiler/codegen/emit.tw`: add `BuiltinOp` variants + `emit_intrinsic_*`
  for each channel op (cf. `emit_intrinsic_task_spawn/await/yield`, ~line 1453,
  2109); register the method-id → op mapping (~line 1383). **Extend
  `program_uses_tasks` (line 2167)** to also include the `Channel` method ids so
  `__task_run` + scheduler support get emitted.
- `Channel<T>` is a GC struct `rt_types__Channel` wrapping an `i32` id (mirror
  `Task<T>`); JS only ever sees the i32.
- `for v in ch`: give `Channel` an inherent `iter(self) Iterator<T>` (the
  `IntoIterator` contract is satisfied via `iter()` — see `checker.tw`, "satisfied
  via inherent"). Implement `iter` with `Iterator.unfold` whose step calls `recv`
  (`.Some`→yield+continue, `.None`→stop).

**Thin Twinkle wrapper module** (e.g. `boot/stdlib/...` or prelude): the
`recv`→`Option` / `send`→`Bool` shaping + the `iter()` satisfier. Concurrency
stays in the runtime.

**Stage0 (Rust `src/`) — Phase 2 only, emit-only:** mirror Task —
`CHANNEL_TYPE_ID` (`src/types/ty.rs`, cf. `TASK_TYPE_ID`), `FuncId`s
(`src/ir/lower.rs`, cf. `TASK_SPAWN`), and `src/types/{resolve,check,env}.rs`
entries. Needed because Phase 2 puts channels in `boot/main.tw`, which `make
stage2` compiles from Rust stage0. Phase 1 needs no `src/` changes.

## Phases

1. **Core primitive** — runtime ops + intrinsics + boot builtin + wrapper +
   `iter()` + `boot/tests/suites/channel_suite.tw`. No stage0. Gate:
   `make bundle-cli` fixed point + boot suite green. (Channel usage stays in tests
   + wrapper, off `boot/main.tw`.)
2. **LSP migration** — replace `ChunkQueue`/`DiagnosticsQueue` + the
   `time.sleep(1)` poll loops (`wait_for_dispatcher`, `diagnostics_loop`, the
   `pending` counter) in `boot/commands/lsp.tw` with channels; fold exit into
   close. Needs the emit-only stage0 support. Re-verify the cold-analysis
   format-latency driver + full boot suite.

## Verification

- Boot suite: `target/twk run boot/tests/main.tw`. Self-host: `make bundle-cli`
  (must reach stage3==stage4). JS runtime tests:
  `deno test --allow-read --allow-write --allow-env --allow-run tools/js_runtime/`.
- `channel_suite.tw` cases: unbuffered rendezvous; bounded buffer + backpressure;
  `for v in ch` drains to close; close drains-then-`.None`; parked senders →
  `false`; send-after-close → `false`; double close no-op; fan-in/out (each value
  once); `try ch.recv()`; send/recv a `.None`/Void payload (tagged-result
  distinctness); deadlock integration test (`target/twk run` a deadlocking `.tw`,
  expect nonzero exit + deadlock message).
- LSP format-latency driver (Phase 2): recreate the stdio driver that opens
  `boot/main.tw` (cold analysis) and sends a `textDocument/formatting` request
  ~300–600ms in, measuring response latency (baseline before this work was
  ~1194ms; should stay ≤ a few hundred ms). The earlier session kept it at
  `/tmp/lsp_format_race.py` (ephemeral — recreate from this description).

## Gotchas / references

- Don't reintroduce polling — the whole point is parking. `recv`/`send` suspend.
- `Task.yield` is now event-loop-fair (commit 6aca841) — see
  `reference_task_scheduler_microtask_macrotask` memory; channels rely on the same
  scheduler discipline (one-task-per-microtask, `pendingHost` accounting).
- Memory `project_channels` has the decision log; `reference_intrinsic_builtin_wiring`
  / `reference_runtime_builtin_wiring` cover the builtin-wiring mechanics.
- After any `.tw` edit: `target/twk fmt` then `target/twk lint <entry>`. Boot
  changes ⇒ `make bundle-cli` (full self-host), not `quick-bundle-cli`.
