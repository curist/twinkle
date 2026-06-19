# Task API Separation of Concerns

Status: **Implemented**

## Goal

Keep `Task<T>` focused on task composition and scheduling, while moving time and
I/O suspension points to the modules that own those domains.

The current branch exposes:

```tw
Task.spawn(f)
Task.await(task)
Task.yield()
Task.sleep(ms)
Task.read_stdin(max)
```

`spawn` / `await` / `yield` are task primitives. `sleep` and `read_stdin` are not:
they are timer and I/O operations that happen to suspend under the current JSPI
backend. If we keep adding effectful host operations under `Task`, the namespace
becomes a catch-all for "async host things" instead of a task abstraction.

## Target public API

### `Task<T>` stays about task composition

```tw
Task.spawn<T>(f: fn() T) Task<T>
Task.await<T>(task: Task<T>) T
Task.yield() Void
```

No `Task.sleep`, no `Task.read_stdin`, and no future `Task.read_file` /
`Task.accept_socket` / `Task.wait_process` pattern.

### Time owns timers

Introduce `@std.time`:

```tw
use @std.time

time.now() Float          // monotonic-ish milliseconds, same semantics as current date.now
time.sleep(ms: Int) Void  // suspends when the runtime supports suspension
```

Keep `@std.date.now()` as a compatibility alias for now, but prefer documenting
`@std.time.now()` for elapsed/runtime time. `date` can later be reserved for real
calendar/date APIs.

### I/O owns stdin

Keep stdin under `@std.io`:

```tw
use @std.io

io.read_stdin_chunk(max_bytes: Int) Vector<Byte>
io.read_stdin_timeout(max_bytes: Int, timeout_ms: Int) Vector<Byte>
io.stdin_eof() Bool
```

These functions may suspend cooperatively when called in a task-enabled JSPI
program, but that is an implementation detail. Their namespace remains I/O.

## Runtime / ABI direction

The public API cleanup should also clean up the ABI layering.

### Task ABI should contain only task operations

Keep the task import module for task primitives only:

```text
task.task_create(anyref) -> i32
task.suspend_await(i32) -> anyref
task.suspend_yield() -> void
```

Remove these from the task ABI:

```text
task.suspend_sleep(i64) -> void
task.suspend_read_stdin(i64) -> Array
```

### Host effect imports should be scheduler-aware when tasks are active

Timer and I/O imports belong under their domain host module:

```text
host.now() -> f64
host.sleep(i64) -> void
host.stdin_read_chunk(i32) -> Array
host.stdin_read_timeout(i32, i32) -> Array
host.stdin_eof() -> i32
```

When a program uses tasks, the JS runtime must wrap suspending host imports through
the task scheduler instead of letting their Promises resume Wasm directly. The
wrapper should:

1. capture the currently running task id;
2. increment scheduler-tracked host work;
3. start the host operation;
4. when it completes, enqueue a scheduler resume entry for that task;
5. resolve/reject the import's Promise only from the scheduler resume path, with
   `scheduler.current` set to the resumed task.

This preserves the scheduler invariant that only one Wasm stack resumes at a time
and that `current` always names the stack that is actually executing.

Non-task programs can keep the existing direct JSPI wrappers for stdin/run-wasm
host imports. In task-enabled programs, replace those direct JSPI wrappers for
suspending host imports with scheduler-aware wrappers; do not let stdin/sleep
Promise completion resume Wasm outside the scheduler pump.

`time.sleep` can require the async/JSPI runner initially; a sync fallback is not
required for this cleanup. If `host.sleep` is called from the sync runner, it
should fail with a clear "requires async/JSPI runtime" error rather than busy
waiting or silently returning.

The stdin ABI keeps the current runtime width (`i32` for byte counts and
timeouts). Twinkle-level `Int` arguments continue to be adapted at the host
boundary the same way existing stdin wrappers are.

## Implementation plan

### 1. Add `@std.time`

Files:

- Add: `boot/stdlib/time.tw`
- Modify: `boot/stdlib/date.tw`
- Modify: builtin registries/signatures in boot and stage0 as needed:
  - `boot/compiler/base_env.tw`
  - `boot/compiler/builtins.tw`
  - `src/types/env.rs`
  - `src/intrinsics/registry.rs`
  - `src/intrinsics/signatures.rs`
  - `src/ir/lower.rs`
  - `src/codegen/prelude.rs`
  - `tools/js_runtime/runtime.mjs`

Planned shape:

```tw
// boot/stdlib/time.tw
pub fn now() Float {
  __host_now()
}

pub fn sleep(ms: Int) Void {
  __host_sleep(ms)
}
```

Then make `date.now()` delegate to `time.now()` or keep its direct `__host_now()`
implementation while docs steer new code to `@std.time`.

Register `__host_sleep` as an internal runtime builtin lowering to the `host.sleep`
import. Do not expose it as a prelude/public builtin.

### 2. Move public stdin use back to `@std.io`

Files:

- Modify: `boot/stdlib/io.tw` only if wrapper names need adjustment
- Modify: JS runtime host import wrapping
- Modify: stage0/boot intrinsic registries if stdin lowering metadata changes

The preferred public surface is unchanged: `io.read_stdin_chunk` and
`io.read_stdin_timeout`. The change is that, in task-enabled programs, those host
imports resume through the task scheduler. This applies to both chunk reads and
timeout reads; the direct async JSPI stdin wrappers remain only for non-task
programs.

If implementation pressure makes a distinct internal import necessary, name it as
an internal host/domain operation (for example `__host_stdin_read_chunk`), not as a
`Task.*` public API.

### 3. Shrink the public `Task` signature

Files:

- Modify: `boot/prelude/signatures/task.tw`
- Modify: `boot/compiler/builtins.tw`
- Modify: `src/intrinsics/registry.rs`
- Modify: `src/intrinsics/signatures.rs`
- Modify: `src/ir/lower.rs` prelude ids
- Modify: boot/stage0 codegen task intrinsic dispatch

Remove public registration and lowering for:

```tw
Task.sleep
Task.read_stdin
```

Keep:

```tw
Task.spawn
Task.await
Task.yield
```

Update docs and builtin hover docs so completion/hover does not advertise the
removed `Task` methods.

### 4. Simplify the task ABI binding

Files:

- Modify: `boot/compiler/codegen/runtime/task_abi.tw`
- Modify: `boot/compiler/codegen/emit.tw`
- Modify: `src/codegen/emit.rs`
- Modify: `tools/js_runtime/runtime.mjs`

Remove `sym_suspend_sleep`, `sym_suspend_read_stdin`, and their imports from the
task ABI table. Sleep/stdin should appear as `host` imports, not `task` imports.

Keep task-use detection based on `Task.spawn` / `Task.await` / `Task.yield`; using
`time.sleep` or `io.read_stdin_chunk` alone should not force a program to import
`task.task_create` or export `__task_run` unless it also uses tasks.

### 5. Update LSP command and examples

Files:

- Modify: `boot/commands/lsp.tw`
- Modify: task/LSP tests and benchmarks that reference the old API
- Modify: `src/cli/build.rs` WAT/import smoke tests that currently expect
  `task.suspend_sleep` and `task.suspend_read_stdin`

Replace:

```tw
chunk := Task.read_stdin(4096)
Task.sleep(idle_sleep_ms)
```

with:

```tw
use @std.time

chunk := io.read_stdin_chunk(4096)
time.sleep(idle_sleep_ms)
```

Keep `Task.spawn` / `Task.await` for dispatcher and diagnostics worker tasks.
Update comments that describe the reader as parking on `Task.read_stdin`.

### 6. Update API docs

Files:

- Modify: `docs/API.md`

Document `Task<T>` with only task composition methods. Add `@std.time` and ensure
`@std.io` is the documented home for stdin reads. Mention that some stdlib host
operations may suspend cooperatively under a task-capable runtime without moving
into the `Task` namespace.

### 7. Tests and validation

Add/update coverage for:

- `Task` completion/hover/API docs expose only `spawn`, `await`, and `yield`.
- `time.sleep` emits/imports `host.sleep`, not `task.suspend_sleep`.
- `io.read_stdin_chunk` and `io.read_stdin_timeout` emit/import
  `host.stdin_read_chunk` / `host.stdin_read_timeout`, not
  `task.suspend_read_stdin`.
- A task that sleeps and a task that reads stdin still resume through the scheduler
  and do not trip scheduler quiescence early.
- LSP command builds and uses `io`/`time` for domain operations.

Validation commands:

```bash
target/twk fmt boot/commands/lsp.tw boot/stdlib/time.tw boot/stdlib/date.tw boot/stdlib/io.tw boot/prelude/signatures/task.tw
target/twk lint boot/main.tw
target/twk build boot/main.tw -o /tmp/check.wasm
target/twk run boot/tests/main.tw
cargo test --release
make bundle-cli
```

`cargo test --release` is included because this cleanup touches stage0 intrinsic
registration/lowering and runtime ABI behavior.

## Non-goals

- No new `async`/`await` syntax.
- No trait/effect system for marking suspending functions.
- No CPU parallelism or worker isolation; this is API/ABI layering cleanup only.
- No attempt to make every host operation cancellable.

## Open questions

1. Should `@std.date.now()` remain indefinitely as an alias, or should it be
   deprecated after `@std.time.now()` exists?
2. Should `time.sleep` be available in the sync runner with a blocking fallback,
   or should it require the async/JSPI runtime from day one?
3. Should task-aware host wrapping cover all Promise-returning extern imports, or
   only built-in host imports (`sleep`, stdin, `run_wasm`) for now?
