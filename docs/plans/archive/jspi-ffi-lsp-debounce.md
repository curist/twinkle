# Twinkle-Owned LSP Debounce via Timed Stdin Polling

## Goal

Implement LSP diagnostics debounce with Twinkle owning the debounce policy and
freshness checks, while the JavaScript host provides the smallest useful runtime
primitive: a stdin read that can time out.

This avoids host-side debounce policy, host timer callbacks, internal timer
notifications, and JSPI for the initial implementation.

## Desired Behavior

1. `textDocument/didChange` records the new content and version in Twinkle state.
2. Twinkle records a diagnostics debounce deadline for the changed document or
   workspace.
3. The LSP main loop reads stdin with a timeout equal to the next debounce
   deadline.
4. If bytes arrive first, Twinkle processes all complete LSP frames and updates
   debounce state.
5. If the timeout expires, Twinkle runs diagnostics only for fresh pending work.
6. Stale or already-published document versions do not publish diagnostics.

## Design: Timed Stdin Read Host Primitive

Add a host import with timeout semantics:

```tw
extern fn host_read_stdin_timeout(max_bytes: Int, timeout_ms: Int) Vector<Byte>
extern fn host_stdin_eof() Bool
```

Semantics:

* return as soon as stdin has bytes;
* wait for at most `timeout_ms` milliseconds;
* return `[]` on timeout or EOF;
* set `host_stdin_eof()` to true after an actual EOF;
* drain currently available bytes up to `max_bytes` when possible;
* `timeout_ms <= 0` means non-blocking/immediate poll.

`[]` is ambiguous by itself, so the LSP loop must check `host_stdin_eof()`
before deciding to exit.

Twinkle owns the event loop policy:

```tw
for !state.should_exit {
  timeout_ms := lsp_next_poll_timeout_ms(state)
  chunk := io.read_stdin_timeout(4096, timeout_ms)

  if chunk.len() > 0 {
    buffer = buffer.concat(chunk)
    process_all_complete_frames()
  }

  if lsp_debounce_due(state) {
    state = publish_fresh_pending_diagnostics(state)
  }
}
```

This keeps the host generic: it does not know about diagnostics, document
versions, debounce tokens, or publishing.

## LSP State Changes

Extend boot LSP state with debounce metadata:

* pending diagnostics by document URI or workspace root;
* latest changed version for each pending document;
* next debounce deadline in host time milliseconds;
* last-published version by document URI to suppress duplicate publishes.

`textDocument/didOpen` may continue publishing immediately at first.
`textDocument/didChange` should stop publishing immediately and instead mark
pending diagnostics with a deadline.

When the deadline is due, Twinkle runs workspace diagnostics and publishes only
if the document version still matches the latest known version and has not
already been published.

## Host Responsibilities

The Node host change should be limited to stdin polling:

* expose `host.stdin_read_timeout(max_bytes, timeout_ms)`;
* expose `host.stdin_eof()` so timeout and EOF can be distinguished;
* implement both without debounce-specific policy;
* keep existing `host.stdin_read_chunk(max_bytes)` for compatibility or make it a
  wrapper around the timeout read with an infinite/blocking timeout.

The host should not schedule diagnostics, decide freshness, or publish anything.

## Node Implementation Options to Research

Plain `fs.readSync(0, ...)` blocks indefinitely when stdin is a pipe with no
available data, so a timed poll needs care.

Candidate approaches:

1. Put fd `0` in non-blocking mode and call `fs.readSync` in a loop until data,
   EOF, or deadline. Sleep briefly between `EAGAIN` attempts.
2. Use `Atomics.wait` on a local `SharedArrayBuffer` for short synchronous sleeps
   between non-blocking reads. This blocks the current thread but does not spin.
3. Use a worker thread to perform blocking stdin reads and push chunks into a
   shared/queued buffer; the main thread polls the queue with a timeout.
4. If Node exposes a reliable stream/readable API that can be integrated with a
   synchronous Wasm import, evaluate it, but avoid requiring JSPI for the initial
   debounce solution.

Prefer the simplest SEA-compatible implementation that works for LSP stdio.

## JSPI Role

JSPI is not required for this debounce design. It remains a possible future tool
for Promise-returning host APIs where Twinkle wants synchronous-looking waits:

```tw
extern fn host_sleep_ms(ms: Int) Void
```

with JavaScript integration like:

```js
const imports = {
  host: {
    sleep_ms: new WebAssembly.Suspending(ms =>
      new Promise(resolve => setTimeout(resolve, Number(ms)))
    ),
  },
}

const run = WebAssembly.promising(instance.exports.some_entry)
await run()
```

Do not make debounce depend on JSPI unless timed stdin polling proves unsuitable.

## Implementation Steps

1. Research and locally test timed stdin polling in Node/SEA-compatible code.
2. Add `host.stdin_read_timeout(max_bytes, timeout_ms)` and `host.stdin_eof()`
   to the Node host harness.
3. Add Twinkle stdlib/host wrappers, e.g. `io.read_stdin_timeout` and
   `io.stdin_eof`.
4. Extend `boot/commands/lsp.tw` to compute the next timeout and check debounce
   deadlines after each read/process cycle.
5. Extend `boot/lib/lsp/server_core.tw` so `didChange` records pending diagnostics
   instead of publishing immediately.
6. Add a server-core function that publishes due fresh diagnostics and updates
   the last-published version cache.
7. Preserve immediate diagnostics on `didOpen` initially unless editor behavior
   suggests opening should be debounced too.
8. Add tests for repeated changes, stale pending diagnostics, and duplicate
   publish suppression.

## Open Questions

* Should debounce be per document, per project root, or a single workspace-wide
  pending deadline?
* Should `didOpen` be immediate or share the debounce path?
* What timeout should be used by default?
* Is `host_stdin_eof()` enough, or should the timeout read eventually return a
  richer status type?
* Is a nonblocking fd approach portable enough across the Node versions used for
  SEA builds?
