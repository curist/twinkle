# Boot JSPI Runtime Integration

## Goal

Use WebAssembly JavaScript Promise Integration (JSPI) in the boot compiler's
JavaScript hosts so Twinkle programs can call selected Promise-returning host or
extern functions as if they were synchronous Twinkle calls.

The first concrete migration target is the current LSP diagnostics debounce
loop: keep the debounce policy in Twinkle, but replace the Node-side synchronous
poll/sleep implementation of timed stdin reads with an event-loop-friendly JSPI
host import.

This is a boot-compiler/runtime plan only. Stage0 remains unchanged and does not
need to learn this entry ABI, host wrapping policy, or async runtime path.

This plan depends on [unify-js-runtime.md](unify-js-runtime.md). Unify the Node
runtime implementation first so JSPI entry invocation, suspending stdin, nested
`run_wasm`, and extern wrapping are implemented once rather than separately in
`tools/wasm_runner_lib.mjs` and `tools/twk_cli_sea.cjs`.

## Motivation

Twinkle targets modern Wasm GC hosts. Latest Chrome and recent/latest Node.js
support the standardized JSPI boundary API:

* `new WebAssembly.Suspending(jsFunction)` marks an imported JavaScript function
  as capable of suspending Wasm when it returns a Promise.
* `WebAssembly.promising(wasmExport)` wraps a Wasm export so JavaScript receives
  a Promise representing the full suspended/resumed execution.

This lets Twinkle preserve direct-style code for browser APIs and async Node
APIs without introducing `async` syntax or lowering whole call chains into
manual continuations. For LSP debounce specifically, Twinkle can continue to
write direct-style code such as `io.read_stdin_timeout(4096, timeout_ms)`, while
Node implements the wait as an async race between stdin data and a timer.

JSPI should also improve host integration responsiveness when the Twinkle program
is waiting on host work. Instead of blocking the JavaScript thread with sync
filesystem, network, timer, or virtual-file operations, the host can return a
Promise, let the Wasm stack suspend, and allow the Node.js or browser event loop
to continue processing other work until the Promise resolves.

This is not preemption or parallelism. CPU-bound Twinkle code still runs until it
returns, traps, or reaches a suspending host import. In the playground, execution
already happens in a worker, so JSPI mainly keeps that worker responsive and
allows async browser APIs to compose naturally; it does not make long-running
compute stop blocking that worker by itself.

## Current State

LSP diagnostics debounce has already been implemented in Twinkle without JSPI.
The current design is intentionally simple and correct for a stdio server:

* `boot/commands/lsp.tw` computes the next poll timeout from Twinkle LSP state.
* `boot/lib/lsp/server_core.tw` owns debounce deadlines and freshness checks.
* `stdlib/io.tw` exposes `read_stdin_timeout` and `stdin_eof`.
* the Node host implements timed reads with synchronous `fs.readSync`, retrying
  on `EAGAIN` and sleeping briefly with `Atomics.wait`.

That means the policy is in the right place, but the Node main thread is still
blocked while Twinkle is waiting for stdin or a debounce deadline. It is not a
busy spin and it is acceptable for the current stdio LSP, but it prevents the
Node event loop from running other timers, promises, file watchers, or future
host integrations during the wait.

JSPI lets us keep the Twinkle-side debounce design while making the host wait
properly async: `read_stdin_timeout` can become a suspending import that returns
a Promise resolved by stdin data, timeout, or EOF.

The boot-generated programs currently execute through a Wasm `start` section.
Both JavaScript hosts instantiate synchronously:

* `tools/wasm_runner_lib.mjs`
* `tools/twk_cli_sea.cjs`
* `playground/public/worker.js`

That is incompatible with suspending imports: JSPI requires a suspending import
to be reached from a matching `WebAssembly.promising(...)` export call. A Wasm
`start` function is invoked during instantiation and is not an exported function
that the host can wrap. If a Promise-returning `WebAssembly.Suspending` import is
called from `start`, the host should expect a trap rather than useful suspension.

The first required change is therefore a boot backend entry ABI change for JSPI
mode, not just a runtime wrapper change.

## Non-goals

* No stage0 implementation work. Stage0 can keep emitting and running the
  existing start-section ABI.
* No new Twinkle `async` / `await` syntax.
* No general `Task<T>` redesign as part of this plan.
* No Asyncify fallback.
* No support for arbitrary Promise values crossing the Twinkle value boundary.
  JSPI initially applies to extern-safe scalar/string-shaped APIs where existing
  host bridging already knows how to marshal values.
* No Safari compatibility requirement for the initial implementation.

## Target Hosts

The initial target hosts are:

* latest Chrome for the playground worker;
* latest Node.js for `target/twk` and local runtime tools.

The runtime should feature-detect JSPI:

```js
const hasJspi =
  typeof WebAssembly.Suspending === "function" &&
  typeof WebAssembly.promising === "function";
```

When JSPI is unavailable, the runtime should fail clearly if async imports were
requested. Purely synchronous programs may continue to use the existing sync
path where practical.

## Design

### Boot codegen entry ABI

Add a boot-only backend mode that emits an exported entry function instead of a
Wasm `start` section.

Current shape:

```wat
(start $__linked_init)
```

JSPI mode shape:

```wat
(export "__twinkle_start" (func $__linked_init))
```

The linked init function still runs the same Twinkle top-level code and runtime
initializers. Only the host-visible invocation changes.

Once the JavaScript hosts have been migrated, the boot compiler should always
emit this entry-export ABI. Do not add a long-lived user-facing mode switch for
JSPI vs start-section output in the boot compiler. Stage0 remains the
compatibility/reference implementation for the old start-section path.

### Runtime invocation

In JSPI mode the JavaScript host does:

```js
const instance = new WebAssembly.Instance(module, imports);
const start = WebAssembly.promising(instance.exports.__twinkle_start);
await start();
```

The runtime entrypoint that may execute JSPI code must therefore become async.
For Node, this means adding async variants around `runWasmBytes` / `runWasmFile`
and updating the SEA CLI main path to await boot execution. For the browser
worker, the message handler is already async and can await program execution.

### Import wrapping policy

Host-owned imports should be wrapped deliberately: imports that may return
Promises should use `WebAssembly.Suspending`, while permanently synchronous
imports can remain plain functions.

Initial host candidates:

* Node `host.stdin_read_timeout`, used by the LSP debounce loop;
* Node `host.run_wasm`, once child programs may themselves use JSPI;
* future browser VFS/host APIs that become fetch-backed;
* future Node host APIs where an async implementation is desirable.

Extern FFI is different: today's browser playground already auto-wires extern
imports by resolving `globalThis[module][name]` and inserting a marshaling
wrapper. In JSPI mode, auto-wired externs should also be auto-wrapped with
`WebAssembly.Suspending`. If the underlying JS function returns a non-Promise,
JSPI passes it through synchronously; if it returns a Promise, Wasm suspends and
resumes with the resolved value. This keeps extern FFI low-ceremony and matches
the existing auto-wiring model.

Existing synchronous core host imports such as `print`, `println`, string
conversion, simple virtual-file reads, and bridge helpers should remain plain
unless they intentionally become async. `stdin_read_chunk` may remain a blocking
compatibility import; the LSP loop should prefer the suspending
`stdin_read_timeout` path when running under JSPI.

For auto-bridged externs, the wrapper order should be:

1. build the Twinkle-to-JS marshaling wrapper;
2. if JSPI mode is active and the import is allowed to suspend, pass that wrapper
   through `new WebAssembly.Suspending(wrapper)`.

The marshaling wrapper may be an `async` function or may return a Promise
provided by the target JS API. If it returns a non-Promise, JSPI passes the value
through without suspension.

### JSPI LSP debounce stdin

The Twinkle LSP debounce logic should remain essentially as it is today:

```tw
for !state.should_exit {
  timeout_ms := server_core.next_poll_timeout_ms(state)
  chunk := case timeout_ms {
    .Some(ms) => io.read_stdin_timeout(4096, ms),
    .None => io.read_stdin_chunk(4096),
  }
  process_frames_and_publish_due_diagnostics()
}
```

The host implementation changes. Instead of a synchronous loop around
`fs.readSync` and `Atomics.wait`, the JSPI host import should return a Promise
that resolves with bytes when either stdin produces data, the timeout expires, or
EOF is observed:

```js
host.stdin_read_timeout = new WebAssembly.Suspending(
  async (maxBytes, timeoutMs) => makeByteArray(b,
    await readStdinChunkOrTimeout(Number(maxBytes), Number(timeoutMs), runtime)
  )
);
```

The exact Node stdin implementation can use stream events, an internal queued
byte buffer, and `setTimeout` for the deadline. The important property is that
while Twinkle is waiting, the Wasm stack is suspended and Node's event loop is
free to process other work.

`stdin_eof` can remain a plain synchronous import reading host runtime state. The
empty-vector result remains ambiguous by itself, so Twinkle should continue to
check `io.stdin_eof()` to distinguish timeout from EOF.

### Nested `host.run_wasm`

`host.run_wasm` currently calls child Wasm synchronously and returns an integer
exit code. In a JSPI runtime, child execution may need to suspend too.

Options:

1. Keep `host.run_wasm` synchronous-only initially. If a child requires JSPI,
   return a clear host error.
2. Add an async-capable internal runtime path and wrap `host.run_wasm` itself as
   suspending so boot code can call it as a direct-style host import.

The preferred implementation is option 2 once the basic entry-export ABI works,
because the boot compiler's `run` command uses `proc.run_wasm` to execute the
compiled user program.

### Extern return/value constraints

The initial extern bridge should keep today's simple conversions:

* Twinkle `String` refs decode to JS strings for parameters.
* JS string returns encode to Twinkle strings.
* Twinkle `Int` / `Float` / `Bool` map to JS number/bigint/number according to
  the existing ABI expectations.

If a Promise resolves, it must resolve to one of those supported JS values. A
rejected Promise propagates as a JSPI exception/trap through the promised export;
the host should report it as a runtime error. Recoverable errors remain explicit
Twinkle values such as `Result<T, E>`.

## Implementation Plan

### Phase 0: Unify the JavaScript runtime

Complete [unify-js-runtime.md](unify-js-runtime.md) first. The shared runtime
must be the single place that owns host imports, bridge marshaling, SEA asset
loading adapters, nested `run_wasm`, and future async/JSPI behavior.

Do not start the JSPI migration while the Node library runtime and SEA CLI still
contain separate copies of the runtime logic.

### Phase 1: Entry-export ABI in the boot backend

* Change the boot backend so linked start functions are exported as
  `__twinkle_start` instead of emitted through a Wasm `start` section.
* Do this as the boot compiler's normal output ABI; do not add a persistent
  user-facing flag to switch back to start-section output.
* Leave stage0 unchanged.
* Add boot-side tests that inspect WAT for the expected export and absence of
  the start section.

### Phase 2: Async runtime path

* Add async `runWasmBytes` / `runWasmFile` variants in the Node runtime library.
* Mirror the change in the SEA CLI entry script.
* Update the browser worker runner to invoke `__twinkle_start` through
  `WebAssembly.promising` when running entry-export modules.
* Preserve a synchronous runtime path for non-JSPI modules while the migration is
  in progress.

### Phase 3: Migrate LSP timed stdin to JSPI

* Replace the Node synchronous polling/sleep implementation of
  `stdin_read_timeout` with an event-loop-based Promise implementation in the
  JSPI runtime path.
* Wrap `stdin_read_timeout` with `WebAssembly.Suspending` when running an
  entry-export module under JSPI.
* Keep `stdin_eof` as plain synchronous state inspection.
* Keep the existing synchronous implementation as a fallback for non-JSPI runtime
  paths during the transition.
* Validate that debounce behavior remains Twinkle-owned: deadlines, freshness,
  duplicate suppression, and publishing stay in `boot/lib/lsp/server_core.tw`.

### Phase 4: Boot `run` integration

* Make the boot compiler's `run` command use the entry-export ABI for user
  programs when executing through the JS runtime.
* Make `host.run_wasm` async-capable under JSPI so a boot compiler running under
  Node or the playground can compile and execute a JSPI-capable child program.
* Keep CLI behavior direct: `twk run file.tw` still exits with the child program's
  exit code after awaited completion.

### Phase 5: Suspended extern FFI

* Extend browser `autoBridgeExternImports` so auto-wired extern imports are
  wrapped with `WebAssembly.Suspending` in JSPI mode.
* Keep the existing marshaling behavior for strings, numbers, booleans, and
  void-like returns.
* Add examples that call Promise-returning JS functions from Twinkle direct-style
  code.
* Report a clear diagnostic/runtime error if a Promise-returning extern is used
  without JSPI support.

### Phase 6: Optional async host APIs

After LSP stdin and extern FFI work, evaluate moving selected host operations to
async implementations where it improves host behavior:

* browser file/module loading backed by `fetch`;
* Node filesystem operations if blocking becomes a practical problem;
* future network or timer APIs.

## Testing Strategy

* WAT-level tests for entry-export ABI generation.
* Node LSP debounce smoke test proving `read_stdin_timeout` can resolve by data,
  timeout, and EOF through a suspending import.
* Node event-loop responsiveness test proving a JS timer can fire while Twinkle
  is waiting in `io.read_stdin_timeout`.
* Browser worker smoke test using an extern that returns `Promise.resolve(...)`.
* Node smoke test using a Promise-returning host/extern function.
* Rejection-path test confirming Promise rejection is surfaced as a runtime
  failure.
* Regression tests confirming sync programs still run through the existing path.

## Decisions

* JSPI/entry-export is the boot compiler's normal JavaScript-host ABI. Use it
  always in the boot path once the runtime is migrated; stage0 keeps the old
  start-section behavior.
* Twinkle source does not need to declare that an extern may suspend initially.
  Suspending behavior is a host/runtime policy.
* Auto-wired extern FFI should also be auto-wrapped for JSPI. This mirrors the
  current low-ceremony extern model: if a JS function returns a Promise, Wasm
  suspends; if it returns a plain value, execution stays synchronous.
* The public entry export is `__twinkle_start`.
