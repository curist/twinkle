# Channels — implementation handoff

Start-of-session note for implementing `Channel<T>`. The full design is
`channels.md` (read it first — this note assumes it). Status: **Phase 1
(core primitive) and Phase 2 (LSP migration) are complete.** During Phase 1 a latent
codegen bug was found and fixed: closure free-variable analysis
(`lower_core/closures.tw`) didn't descend into `ContractCall`, so an IntoIterator
iterable (e.g. a channel) consumed via `for v in ch` inside a `Task.spawn` closure
degraded to a bogus module-global reference. Stage0's Rust closure-capture pass may
have the same gap, but it is not triggered (boot/main.tw has no such pattern); fix
it there too if Phase 2 introduces one.

## Repo state

- Branch `main`. Everything below is **committed but NOT pushed**:
  - `e711c28` LSP per-module progress reporting
  - `6efb805` LSP cold-analysis responsiveness (sink yields)
  - `6aca841` `Task.yield` macrotask-hop fix (scheduler fairness)
  - `211ce65` / `3051ebe` / `26ddfe5` / `8bc53ac` channels design spec (+ reviews)
- Confirm green before starting: `make bundle-cli` reaches a self-host fixed point;
  `target/twk run boot/tests/main.tw` passes.

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
- `boot/compiler/base_env.tw`: register `Channel<T>` as a builtin named type.
- `boot/compiler/codegen/runtime/task_abi.tw`: `host_module()` returns `"task"`;
  add the channel `ImportDef`s in `imports()` (i32 ids, anyref payload/result,
  plus sync tagged-result accessors `channel_recv_is_value` /
  `channel_recv_value`). Runtime import names are `channel_new()` and
  `channel_bounded(capacity)`, not a magic-capacity `channel_new(cap)`.
- `boot/compiler/builtins.tw`: register `intr(...)` entries for
  `Channel.new/bounded/send/recv/close` (see the Task intrinsics block).
- `boot/compiler/codegen/runtime/types.tw`: add `rt_types__Channel`, an immutable
  one-field struct wrapping the i32 channel id.
- `boot/compiler/codegen/wasm_layout.tw` and `boot/compiler/codegen/emit/anyref.tw`:
  handle `Channel<T>` like `Task<T>` (opaque GC ref for layout/boxing).
- `boot/compiler/codegen/emit.tw`: add `IntrinsicTag` variants +
  `emit_intrinsic_*` for each channel op (cf. `emit_intrinsic_task_spawn/await/yield`);
  register the method-id → op mapping. **Extend `program_uses_tasks`** to also
  include the `Channel` method ids so `__task_run` + scheduler support get emitted.
- `Channel.recv` lowering: `channel_recv` returns an opaque tagged JS object;
  Wasm decodes it with `channel_recv_is_value(raw)` and `channel_recv_value(raw)`
  and constructs the typed `Option<T>` itself. Do not rely on Twinkle/wasm
  inspecting JS object properties directly.
- `Channel<T>` is a GC struct `rt_types__Channel` wrapping an `i32` id (mirror
  `Task<T>`); JS only ever sees the i32.
- `for v in ch`: give `Channel` an inherent `iter(self) Iterator<T>` (the
  `IntoIterator` contract is satisfied via `iter()` — see `checker.tw`, "satisfied
  via inherent"). Implement `iter` with `Iterator.unfold` whose step calls `recv`
  (`.Some`→yield+continue, `.None`→stop).
- After adding `boot/prelude/channel.tw` / `boot/prelude/signatures/channel.tw`,
  regenerate `boot/lib/module/core_lib.tw` (`python3 tools/generate_core_lib.py`,
  then format it).

**Thin Twinkle wrapper module** (prelude): user-facing signatures and the
`iter()` satisfier. The `recv`→`Option` / `send`→`Bool` shaping is compiler-emitted
around the runtime intrinsics/accessors; concurrency stays in the runtime.

**Stage0 (Rust `src/`) — emit-only, needed from Phase 1:** mirror Task —
`CHANNEL_TYPE_ID` (`src/types/ty.rs`, cf. `TASK_TYPE_ID`), `FuncId`s
(`src/ir/lower.rs`, cf. `TASK_SPAWN`), `src/types/{resolve,check,env}.rs`,
`src/intrinsics/{registry,signatures}.rs`, `src/codegen/{prelude,emit}.rs`, and
`src/runtime/types.rs`. Needed already in Phase 1 because
`boot/prelude/channel.tw` is auto-imported into `boot/main.tw`, which `make
stage2` builds from Rust stage0 — without it the prelude won't compile under
stage0 and self-host breaks. (Stage0 only *emits* channels; it never runs them.)

## Phases

1. **Core primitive** — runtime ops + intrinsics + boot builtin + prelude wrapper
   + `iter()` + `boot/tests/suites/channel_suite.tw` + the emit-only Rust stage0
   support (the prelude is auto-imported into `boot/main.tw`). Gate:
   `make bundle-cli` fixed point + boot suite green.
2. **LSP migration — complete** — replaced `ChunkQueue`/`DiagnosticsQueue` + the
   `time.sleep(1)` poll loops (`wait_for_dispatcher`, `diagnostics_loop`, the
   `pending` counter) in `boot/commands/lsp.tw` with channels; exit is folded into
   channel close. The dispatcher uses `recv()` loops rather than `for v in ch` so
   Rust stage0 can still bootstrap `boot/main.tw`; the runtime behavior still parks
   on the channel instead of polling.

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
- Keep the implementation notes in this file and `channels.md` as the
  source of truth; avoid relying on ephemeral session memory.
- After any `.tw` edit: `target/twk fmt` then `target/twk lint <entry>`. Boot
  changes ⇒ `make bundle-cli` (full self-host), not `quick-bundle-cli`.
