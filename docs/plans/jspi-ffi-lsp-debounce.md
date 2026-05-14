# JSPI + FFI for Twinkle-Owned LSP Debounce

## Goal

Implement LSP diagnostics debounce with Twinkle owning the debounce policy and
freshness checks, while the JavaScript host provides only minimal async/event-loop
primitives.

The host should not know when diagnostics are fresh, which documents need
checking, or how debounce tokens are interpreted. It should only provide a way to
schedule a later callback into the Twinkle LSP server.

## Desired Behavior

1. `textDocument/didChange` records the new content and version in Twinkle state.
2. Twinkle creates a debounce token for that document/version.
3. Twinkle asks the host to deliver a timer event after the debounce delay.
4. The LSP server keeps processing incoming messages while the timer is pending.
5. When the host delivers the timer event, Twinkle checks whether the token still
   matches the latest known document version.
6. Only fresh timer events run diagnostics and publish results.

## Design: Host Timer Event, Twinkle Policy

Avoid blocking the LSP message loop on a synchronous-looking sleep. A direct
`host_sleep_ms(ms)` call inside `didChange` would suspend the current Wasm export
under JSPI; if the LSP loop is a single long-running call, that can also stop the
server from reading newer messages.

Instead, use host-owned timers as event delivery only:

```tw
extern fn host_schedule_lsp_timer(token: Int, delay_ms: Int) Void
```

Twinkle owns token generation and validation:

```tw
fn on_did_change(uri: String, version: Int, text: String) Void {
  documents = documents.change_full_text(uri, text, .Some(version))
  token := next_debounce_token()
  debounce_by_uri[uri] = .{ token, version }
  host_schedule_lsp_timer(token, 150)
}

fn on_lsp_timer(token: Int) Void {
  case find_debounce_by_token(token) {
    .Some(entry) => {
      if latest_version(entry.uri) == entry.version {
        publish_workspace_diagnostics()
      }
    },
    .None => {},
  }
}
```

The host later injects an internal notification back into the normal LSP handling
path, for example:

```json
{
  "jsonrpc": "2.0",
  "method": "$/twinkle/timer",
  "params": { "token": 42 }
}
```

That keeps policy centralized in `boot/lib/lsp/server_core.tw`, while the host
only knows how to call `setTimeout` and enqueue a message.

## Host Responsibilities

The host still needs changes, but they are intentionally narrow:

* expose `host.schedule_lsp_timer(token, delay_ms)` as a Twinkle extern import;
* implement it with `setTimeout`;
* when the timer fires, enqueue or directly deliver the internal
  `$/twinkle/timer` notification;
* keep the LSP transport loop active while timers are pending.

The host does **not** decide whether to run diagnostics, publish diagnostics, or
cancel stale work.

## JSPI Role

This timer-event design does not require JSPI for the debounce path because the
extern import can return immediately after scheduling a host timer.

JSPI remains useful for future host APIs where Twinkle genuinely wants a
synchronous-looking wait or Promise-returning import:

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

Do not make debounce depend on this unless a concrete need appears. Timer events
are simpler and avoid suspending the LSP message loop.

## Import Discovery

Use a hard-coded tooling import for the first implementation:

* Twinkle extern: `host_schedule_lsp_timer(token: Int, delay_ms: Int) Void`
* Wasm import: `host.schedule_lsp_timer`

Avoid language syntax changes or an async-import manifest for the initial pass.
A manifest can be added later if more host tooling imports need configuration.

## Duplicate Publish Guard

Token/version checks should prevent stale timers from publishing. If multiple
timer events for the same current version are delivered, duplicate diagnostics
are not corrupting but are noisy.

Keep a per-document published-version cache in Twinkle state or the LSP
server-core state:

```tw
last_published_by_uri[uri] = version
```

Before publishing, skip diagnostics for a document if the same version was
already published.

## Implementation Steps

1. Add debounce state to the boot LSP server core:
   * next timer token;
   * pending debounce entries by document URI/token;
   * last-published version by document URI.
2. Change `textDocument/didChange` to update document state and schedule a timer
   instead of immediately publishing diagnostics.
3. Add handling for the internal `$/twinkle/timer` notification.
4. On timer notification, validate the token/version and run diagnostics only for
   fresh work.
5. Add the `host_schedule_lsp_timer` extern signature and call site.
6. Extend the Node host harness to implement `host.schedule_lsp_timer` with
   `setTimeout` and feed the timer notification back into the LSP message path.
7. Keep `didOpen` behavior immediate unless editor behavior suggests it should
   share the debounce path too.
8. Add tests for stale timer suppression, latest-version publishing, and duplicate
   publish suppression.

## Open Questions

* Should `didOpen` publish immediately or go through the same debounce path?
* Should timer events be represented as internal JSON-RPC notifications or as a
  smaller host callback into a dedicated exported Twinkle function?
* Does the current SEA host need a small event queue abstraction so stdin frames
  and timer events share one dispatch path?
* Should debounce delay be fixed initially or configurable via initialization
  options?
