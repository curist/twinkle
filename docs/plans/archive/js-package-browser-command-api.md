# JS Package Browser Command API Plan

Status: planned.

## Goal

Expose the full shipped Twinkle compiler payload from the JavaScript package's
browser entry, not just the current `run(source)` convenience wrapper.

The published `@twinkle-lang/twinkle` package already ships `boot.wasm`, which is
the boot compiler CLI. The browser entry should let embedders invoke supported
`twk` subcommands against an in-memory project and inspect stdout, stderr, exit
status, and generated files.

This plan only covers the JS package interface and runtime behavior. It does not
specify any playground UI or editor integration.

---

## Current Baseline

`@twinkle-lang/twinkle/web` currently exports:

```js
import { run, load } from '@twinkle-lang/twinkle/web'
```

`run(source, opts)` seeds `/input/main.tw` in an in-memory filesystem and invokes
the compiler payload as:

```js
guestArgs: ['run', '/input/main.tw']
```

That means browser embedders can compile and run source, but cannot directly use
other already-available compiler commands such as `fmt`, `check`, `ir`, or
`build`.

---

## Proposed Public API

Add a generic command API to `@twinkle-lang/twinkle/web`:

```js
import { command, run, load } from '@twinkle-lang/twinkle/web'

const result = await command(['check', '/input/main.tw'], {
  source,
  env: { NO_COLOR: '1' },
})
```

### `command(args, opts)`

```ts
type WebCommandOptions = {
  source?: string | Uint8Array
  path?: string
  files?: Iterable<[string, string | Uint8Array]>
  cwd?: string
  env?: Record<string, string>
  stdout?: { write(chunk: string | Uint8Array): boolean | void }
  stderr?: { write(chunk: string | Uint8Array): boolean | void }
  imports?: Record<string, Record<string, Function | { fn?: Function, args?: string[] }>>
  host?: MemoryHost
}

type WebCommandResult = {
  exitCode: number
  stdout: string
  stderr: string
  files: Map<string, Uint8Array>
  text(path: string): string | undefined
  bytes(path: string): Uint8Array | undefined
}

async function command(args: string[], opts?: WebCommandOptions): Promise<WebCommandResult>
```

Behavior:

* Loads the same `boot.wasm` and `bridge.wasm` assets as `run()`.
* Creates an in-memory host unless `opts.host` is supplied.
* If `opts.source` is provided, writes it to `opts.path ?? '/input/main.tw'`.
* Seeds any `opts.files` into the same in-memory filesystem.
* Runs the compiler payload with `guestArgs: args`.
* Collects stdout and stderr while still forwarding to optional caller-provided
  streams.
* Returns the final memory filesystem so callers can read rewritten source,
  generated WAT, or generated Wasm.
* Does not throw for non-zero compiler exit codes; those are represented by
  `result.exitCode`.
* Throws only for host/runtime failures: missing wasm assets, malformed options,
  unsupported host behavior, or unexpected JS/Wasm runtime errors.

The `files` map should use normalized absolute paths, matching the existing
`createMemoryHost` behavior.

### Convenience Helpers

Keep the existing `run(source, opts)` API stable, but implement it on top of
`command()` conceptually:

```js
export async function run(source, opts = {}) {
  const result = await command(['run', opts.path ?? '/input/main.tw'], {
    ...opts,
    source,
    path: opts.path ?? '/input/main.tw',
  })
  return result.exitCode
}
```

Add small helpers only if they remove boilerplate without hiding the command
model:

```js
async function check(source, opts?)
async function format(source, opts?)
async function ir(source, opts?)
async function build(source, opts?)
```

These should be thin wrappers around `command()`, not separate runtime paths.
They can be deferred; the generic API is the important part.

---

## Example Usage

### Type-check source

```js
const result = await command(['check', '/input/main.tw'], {
  source,
  env: { NO_COLOR: '1' },
})

if (result.exitCode !== 0) {
  console.error(result.stderr || result.stdout)
}
```

### Format source

```js
const result = await command(['fmt', '/input/main.tw'], {
  source,
  env: { NO_COLOR: '1' },
})

const formatted = result.text('/input/main.tw')
```

### Print optimized IR

```js
const result = await command(['ir', '--opt', '/input/main.tw'], {
  source,
  env: { NO_COLOR: '1' },
})

console.log(result.stdout)
```

### Build WAT

```js
const result = await command([
  'build',
  '/input/main.tw',
  '-o',
  '/output/main.wat',
], { source })

const wat = result.text('/output/main.wat')
```

### Build Wasm bytes

```js
const result = await command([
  'build',
  '/input/main.tw',
  '-o',
  '/output/main.wasm',
], { source })

const wasm = result.bytes('/output/main.wasm')
```

---

## Command Argument Shape

`command()` should accept normalized CLI arguments only, without trying to parse
or reinterpret them in JavaScript.

Good:

```js
command(['ir', '--wat', '/input/main.tw'], { source })
command(['build', '/input/main.tw', '-o', '/out/main.wasm'], { source })
```

Avoid adding command-specific option objects to the generic API:

```js
// Not the generic API shape
command({ subcommand: 'ir', file: '/input/main.tw', wat: true })
```

Rationale: the compiler CLI remains the source of truth for flags, defaults,
validation, help text, and future command behavior.

---

## Filesystem Seeding

`opts.files` should accept both text and bytes:

```js
await command(['check', '/project/main.tw'], {
  files: [
    ['/project/main.tw', mainSource],
    ['/project/math.tw', mathSource],
    ['/project/twinkle.toml', ''],
  ],
  cwd: '/project',
})
```

Rules:

* Text values are UTF-8 encoded.
* Byte values are stored as-is.
* `opts.source` is applied after `opts.files`, so it can override the entry path.
* Relative paths in `opts.files` are normalized through the memory host.
* `cwd` defaults to `/`.

---

## Streams and Output Capture

`command()` should always capture stdout and stderr into strings for the result.
If callers also pass `stdout` or `stderr`, writes should be tee'd:

```js
const result = await command(['check', '/input/main.tw'], {
  source,
  stderr: { write: chunk => appendToPanel(chunk) },
})

result.stderr // still contains the full stderr text
```

Use stream-decoding so split UTF-8 byte chunks are not corrupted, matching the
existing worker/runtime pattern.

---

## Error Model

Compiler command failures are normal results:

```js
const result = await command(['check', '/input/main.tw'], { source: badSource })
result.exitCode // non-zero
result.stderr   // rendered diagnostics
```

Runtime/host failures throw:

* failed asset fetch
* invalid `args`
* invalid file payload type
* missing required bridge wasm
* unexpected WebAssembly or JS host exception

This matches CLI expectations while making command failures easy to present in
browser UIs.

---

## Compatibility

Keep these existing exports and behaviors stable:

* `load()` prefetches compiler assets.
* `run(source, opts)` compiles and runs source and returns an exit code.
* Existing extern import handling continues to work for `run()` and for command
  invocations that eventually execute user Wasm.

The new API is additive. No package export-map change is required if it lives in
`./web`.

---

## Implementation Tasks

1. Add internal helpers in `tools/js_runtime/web.mjs`:
   * normalize text/byte file inputs
   * create a teeing capture stream
   * expose result helpers `text(path)` and `bytes(path)`
2. Implement `command(args, opts)` using `runWasmBytesAsync` and
   `createMemoryHost`.
3. Reimplement `run(source, opts)` in terms of the shared command machinery while
   preserving its return type.
4. Add browser-oriented tests for:
   * `check` success and failure
   * `fmt` rewriting `/input/main.tw`
   * `ir --opt` returning IR on stdout
   * `build -o /output/main.wat` producing text output in memory
   * `build -o /output/main.wasm` producing Wasm bytes in memory
   * non-zero compiler exits returning a result instead of throwing
5. Update `tools/npm/README.md` with the new browser command API.

---

## Non-goals

* No playground UI plan in this document.
* No editor integration plan in this document.
* No command-specific browser protocol beyond the CLI-shaped `args` array.
* No LSP-over-worker design here.
* No replacement for the Node package API; this plan is scoped to
  `@twinkle-lang/twinkle/web`.
