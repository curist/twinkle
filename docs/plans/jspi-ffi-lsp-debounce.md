# JSPI + FFI for LSP Debounce

## Goal

Implement LSP diagnostics debounce with the smallest reliable mechanism first:
host-owned JavaScript debounce around the existing synchronous Twinkle
checker/compiler entrypoints.

JSPI + extern FFI is a follow-up option only if we later need debounce policy or
other host waits to live inside Twinkle code.

## Use Case

The LSP receives frequent document changes. Diagnostics should not rerun on every
keystroke; they should run after a short quiet period and only for the latest
document version.

Desired behavior:

1. `textDocument/didChange` records the new content and version.
2. A debounce delay is scheduled or reset.
3. When the delay completes, diagnostics run for the latest version.
4. Stale scheduled checks do not publish diagnostics.

## Phase 1: Host-Owned Debounce

Keep debounce scheduling in the JavaScript LSP host. This is the deliverable that
solves the immediate problem.

```js
let debounceTimer = null
let latestDocument = null

function onDidChange(document) {
  latestDocument = document
  clearTimeout(debounceTimer)
  debounceTimer = setTimeout(() => {
    runDiagnostics(latestDocument)
  }, 150)
}
```

The Twinkle checker/compiler remains synchronous from its own point of view. The
host decides when to call it.

Benefits:

* no compiler changes;
* no Wasm/runtime changes;
* easy cancellation with `clearTimeout`;
* natural integration with the JS LSP server event loop;
* works regardless of JSPI availability.

### Version guard

Even with host-owned debounce, diagnostics should carry the document version they
were computed for. Before publishing, the host should check that the result still
matches the latest known version. This prevents stale diagnostics if a check takes
longer than expected.

```js
async function runDiagnostics(document) {
  const version = document.version
  const result = checkDocument(document)
  if (latestDocument?.version === version) {
    publishDiagnostics(document.uri, version, result)
  }
}
```

## Scope Gate

Ship Phase 1 and close the LSP debounce issue if it is sufficient.

Only pursue JSPI if a concrete follow-up need appears, such as:

* debounce policy must be shared/tested in Twinkle code;
* Twinkle tooling needs synchronous-looking waits for host timers;
* future host APIs need to await JavaScript Promises from inside Wasm.

## Phase 2 Option: Twinkle-Owned Delay via JSPI

If debounce policy needs to live in Twinkle code, expose a host timer import:

```tw
extern fn host_sleep_ms(ms: Int) Void
extern fn host_now_ms() Float
```

The JavaScript implementation of `host_sleep_ms` returns a Promise:

```js
function sleep_ms(ms) {
  return new Promise(resolve => setTimeout(resolve, Number(ms)))
}
```

With JSPI, V8 can suspend the Wasm call while the Promise is pending and resume
it when the timer fires. Twinkle code can remain synchronous-looking:

```tw
fn debounce_check(version: Int, delay_ms: Int) Void {
  host_sleep_ms(delay_ms)
  if current_document_version() == version {
    publish_diagnostics(version)
  }
}
```

Use version checks rather than cancellation for the first Twinkle-owned design:

* each edit increments a document version;
* each scheduled check captures the version it was created for;
* after `host_sleep_ms`, the check exits if a newer version exists.

### Duplicate publish race

If multiple suspended debounce calls resume in the same JavaScript turn, more
than one call can observe the same current version and publish duplicate
diagnostics. This is not corrupting, but it is noisy.

For the first JSPI version, guard publishes with a per-document published-version
cache:

```js
const lastPublished = new Map()

function publishIfFresh(uri, version, diagnostics) {
  if (latestVersion(uri) !== version) return
  if (lastPublished.get(uri) === version) return
  lastPublished.set(uri, version)
  publishDiagnostics(uri, version, diagnostics)
}
```

## JSPI Host Integration Sketch

V8's JSPI API wraps Promise-returning imports and exports that may suspend.
Conceptually:

```js
const imports = {
  host: {
    sleep_ms: new WebAssembly.Suspending(ms =>
      new Promise(resolve => setTimeout(resolve, Number(ms)))
    ),
  },
}

const { instance } = await WebAssembly.instantiate(wasmBytes, imports)
const run = WebAssembly.promising(instance.exports.run_lsp_entry)
await run()
```

### Node/V8 compatibility

JSPI availability depends on the exact Node/V8 runtime used by the LSP host.
Verify at startup rather than assuming support:

```js
const hasJspi =
  "Suspending" in WebAssembly && "promising" in WebAssembly
```

Observed local behavior:

* Node v23.11.0: JSPI requires `--experimental-wasm-jspi`.
* Node v26.0.0/v26.1.0: JSPI is enabled by default; the old
  `--experimental-wasm-jspi` flag is rejected.

If JSPI is unavailable, disable Twinkle-owned JSPI debounce and use Phase 1
host-owned debounce.

SEA concern: `target/twk` is built as a Node SEA executable. With Node v23.11.0,
JSPI required a flag and that flag did not carry through the tested SEA paths:

* passing `--experimental-wasm-jspi` to the SEA executable left JSPI disabled;
* `NODE_OPTIONS=--experimental-wasm-jspi` was rejected by Node;
* adding `execArgv: ["--experimental-wasm-jspi"]` to the SEA config was ignored
  by Node v23.11.0.

With Node v26.1.0 from the official `node` npm package, a SEA built via
`--build-sea` has JSPI enabled by default. That is the preferred SEA path for
JSPI experiments.

Caveat: the Homebrew Node v26.0.0 binary tested locally did not contain the SEA
sentinel used by `--build-sea`/`postject`, so it could not be used to produce a
SEA in the current build flow. For JSPI + SEA experiments, use a Node binary that
both supports SEA generation and has JSPI enabled by default.

## FFI Requirements for the JSPI Option

### Import discovery

The compiler already supports extern imports. The host harness needs to know
which imports should be wrapped with `WebAssembly.Suspending`.

Possible first-pass approaches:

* hard-code tooling imports such as `host.sleep_ms` in the LSP runner;
* use a host manifest listing async imports;
* later, introduce a convention such as imports from `host.async`.

Prefer hard-coded tooling imports for the first experiment to avoid language
syntax changes.

### Export invocation

Any exported Twinkle function that may call a JSPI-suspending import must be
called through `WebAssembly.promising` by the JS host.

The Wasm function signature can remain ordinary for the first version; the host
controls whether the call is promise-aware.

### Type mapping

Start with timer-only imports:

* `Int` delay in milliseconds;
* `Void` result.

Promise rejection policy for the first JSPI pass: **trap**. Timer Promises should
not reject, and trapping keeps the initial host adapter simple. If future host
async APIs need recoverable failures, add explicit `Result<T, String>` adapters
then.

## Implementation Steps

1. Implement host-owned debounce in the LSP server.
2. Add version guards before publishing diagnostics.
3. Ship this as the initial LSP debounce solution.
4. If Twinkle-owned waits become necessary, add a JSPI smoke experiment in
   `tools/`:
   * import `host.sleep_ms`;
   * call it from Wasm;
   * invoke the Wasm export via `WebAssembly.promising`.
5. Verify Node/SEA JSPI compatibility and required flags.
6. Add an internal tooling extern signature for `host_sleep_ms(ms: Int) Void`.
7. Extend the LSP Node host harness to wrap `host.sleep_ms` with
   `WebAssembly.Suspending`.
8. Add smoke tests that run only when JSPI is available in the host runtime.

## Open Questions

* Is host-owned debounce sufficient for the LSP long term?
* If JSPI is needed, should the LSP host run under normal Node instead of SEA?
* Should async imports remain hard-coded tooling hooks or move to a manifest?
