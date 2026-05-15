# Unify JavaScript Wasm Runtime

## Goal

Stop maintaining two independent copies of the Node.js Twinkle Wasm runtime:

* `tools/wasm_runner_lib.mjs`
* `tools/twk_cli_sea.cjs`

Create one shared runtime implementation that supports both ordinary Node.js use
and the Node SEA standalone CLI.

This must happen before the JSPI migration. JSPI will add async entry invocation,
suspending imports, async stdin, nested async `host.run_wasm`, and extern
wrapping policy; implementing that twice would be error-prone.

## Current State

The runtime logic is duplicated today:

* bridge instantiation;
* Twinkle string/vector/result marshaling;
* host imports for I/O, process, filesystem, parsing, and nested `run_wasm`;
* import verification;
* Wasm module instantiation and host-exit handling.

`tools/wasm_runner_lib.mjs` is a normal ESM library used from file-based Node
execution. `tools/twk_cli_sea.cjs` is intentionally self-contained because a Node
SEA main script cannot rely on project-local runtime files being available via
normal `require()` after injection.

The SEA file also contains extern FFI auto-bridging logic that should not remain
SEA-only. Extern auto-wiring belongs in the shared runtime, especially because
JSPI will extend that logic to wrap auto-wired externs with
`WebAssembly.Suspending`.

## Constraints

* The SEA main script must remain runnable as an injected single executable.
* The SEA path needs access to embedded assets via `node:sea`:
  * `boot.wasm`
  * `bridge.wasm`
* The normal Node library should remain easy to import for tests/tools.
* esbuild is acceptable as the SEA bundling tool because it is a small,
  zero-dependency, single-binary dev/build dependency.
* The runtime implementation should not need to know whether it is running in
  SEA. Environment differences should be expressed through injected callbacks
  and options.
* The shared runtime should be the place where JSPI support is added.

## Design

Use one shared ESM runtime and bundle a tiny SEA wrapper with esbuild into a
self-contained CJS entry file under `target/`.

### Source layout

Proposed files:

```text
tools/js_runtime/runtime.mjs   # shared implementation; source of truth
tools/js_runtime/sea_main.mjs  # tiny SEA wrapper: asset loading + CLI invocation
target/twk_cli_sea.cjs         # generated esbuild bundle, not hand-edited
```

`sea_main.mjs` should stay tiny, roughly asset loading via `sea.getAsset()`,
argv/process setup, and a call to `runBootCli()`. Almost all behavior belongs in
`runtime.mjs`.

During migration, `tools/wasm_runner_lib.mjs` may briefly re-export from the
shared runtime. After call sites are updated, delete the facade rather than
keeping a permanent compatibility layer for an internal tool.

### Runtime API

The shared runtime should expose environment-neutral entrypoints. Initially these
can preserve today's synchronous API. As JSPI lands, prefer a single Promise-
returning runtime path for CLI execution rather than maintaining parallel sync
and async implementations long term.

Possible shape:

```js
export function createRuntimeHost(options) { ... }
export function runWasmBytes(bytes, options) { ... }       // sync during extraction
export function runWasmFile(path, options) { ... }         // sync during extraction
export async function runBootCli(options) { ... }          // CLI-facing path
```

After JSPI migration, `runWasmBytes` may become Promise-returning, or the public
API can be renamed to make async behavior explicit. The important invariant is
that the implementation is shared, not duplicated.

Options should provide environment-specific pieces without an `isSea` flag:

```js
{
  loadBridgeBytes,
  loadBootBytes,
  programPath,
  guestArgs,
  cwd,
  env,
  stdout,
  stderr,
  stdin,
}
```

The shared runtime should not directly call `sea.getAsset`; that belongs in
`sea_main.mjs`. If the shared runtime needs bytes, paths, streams, or process
state, those should be supplied through options.

### SEA bundling strategy

Use esbuild from the start. `tools/build_node_sea_cli.sh` can invoke it via a
local dev dependency if present, or via `npx --yes esbuild` as a fallback:

```bash
esbuild tools/js_runtime/sea_main.mjs \
  --bundle \
  --platform=node \
  --format=cjs \
  --outfile=target/twk_cli_sea.cjs
```

Rationale:

* the shared runtime is ESM while Node SEA expects a self-contained main file;
* esbuild handles import/export conversion and dependency graph bundling;
* the generated file is a build artifact, not source;
* this avoids fragile text concatenation/templates.

`tools/build_node_sea_cli.sh` should run the bundle step and point the Node SEA
config at `target/twk_cli_sea.cjs`.

### Shared extern auto-bridging

Move `autoBridgeExternImports` into the shared runtime. It is runtime behavior,
not SEA adapter behavior.

The shared function should:

* inspect `WebAssembly.Module.imports(module)`;
* skip imports already provided by core host modules;
* resolve externs from `globalThis[module][name]` where supported;
* insert the current marshaling wrapper for strings, numbers, booleans, and
  void-like returns;
* later, in JSPI mode, wrap that marshaling wrapper with
  `new WebAssembly.Suspending(...)`.

This behavior should be available to both ordinary Node/file-based execution and
the SEA CLI.

### Normal Node path

Update internal call sites to import from `tools/js_runtime/runtime.mjs`.

`tools/wasm_runner_lib.mjs` can temporarily be:

```js
export { runWasmBytes, runWasmFile } from "./js_runtime/runtime.mjs";
```

Delete it after migration unless a concrete external consumer appears.

### JSPI ownership

All JSPI-related runtime behavior should be added only to the shared runtime:

* `WebAssembly.Suspending` wrapping policy;
* `WebAssembly.promising(instance.exports.__twinkle_start)` invocation;
* async `stdin_read_timeout` implementation;
* async-capable nested `host.run_wasm`;
* extern auto-wrapping.

The SEA wrapper should only choose asset loading and process integration.

## Implementation Plan

### Phase 1: Extract shared runtime and bundle SEA entry

* Move common runtime logic out of `tools/wasm_runner_lib.mjs` into
  `tools/js_runtime/runtime.mjs`.
* Move SEA-specific asset loading and CLI startup into
  `tools/js_runtime/sea_main.mjs`.
* Move extern auto-bridging into the shared runtime.
* Add an esbuild step in `tools/build_node_sea_cli.sh` that writes
  `target/twk_cli_sea.cjs`.
* Point the Node SEA config at `target/twk_cli_sea.cjs`.
* Replace `tools/wasm_runner_lib.mjs` with a temporary re-export shim or update
  call sites directly.
* Stop editing `tools/twk_cli_sea.cjs` by hand; remove it or replace it with a
  short note pointing to the generated target file.

### Phase 2: Regression test the unified runtime

* Run boot compiler commands through the normal Node library path.
* Build `target/twk` and run representative CLI commands through the SEA path.
* Verify host imports, file I/O, stdin/stdout byte I/O, nested `run_wasm`, and
  `host.exit` behavior remain unchanged.
* Test extern auto-bridging through both the SEA path and the normal Node runtime
  path. Moving it into the shared runtime should preserve SEA behavior and make
  the same behavior available outside SEA.

### Phase 3: Delete compatibility shims

* Update any remaining imports of `tools/wasm_runner_lib.mjs`.
* Delete the facade if it has no real consumer.
* Ensure the generated SEA bundle lives only under `target/` and is not checked
  in.

### Phase 4: Implement JSPI on the shared runtime

After the runtime is unified, implement the JSPI plan in one place:

* entry-export invocation;
* suspending timed stdin;
* async `host.run_wasm`;
* extern auto-wrapping.

## Testing Strategy

* Add a small Node test script that imports the shared runtime and runs a smoke
  Wasm program.
* Add a SEA smoke command to CI or `make test` if SEA build time is acceptable;
  otherwise keep it as an explicit `make quick-bundle-cli && target/twk ...`
  developer check.
* Compare behavior of the library runner and SEA runner on:
  * printing;
  * reading/writing files;
  * raw byte stdout;
  * stdin timeout/eof;
  * nested `run_wasm`;
  * extern auto-bridging.

## Decisions

* The source of truth is `tools/js_runtime/runtime.mjs`, not the SEA entry file.
* Use esbuild from the start for the SEA bundle.
* The generated SEA file lives under `target/` and is not checked in.
* Do not pass an `isSea` flag into the runtime. SEA-specific behavior is captured
  by asset-loader callbacks and process options.
* `autoBridgeExternImports` belongs in the shared runtime.
* `tools/wasm_runner_lib.mjs` should be deleted after migration unless a concrete
  external compatibility need appears.
* JSPI should be implemented after unification so async runtime behavior is not
  duplicated.
