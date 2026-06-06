# Twinkle npm Package (bin + lib)

## Goal

Distribute Twinkle through npm as a single package, `@twinkle-lang/twinkle`
(published under a `@twinkle-lang` npm organization, leaving room for related
packages later), providing two faces:

- **bin** — a `twk` CLI, a faithful Node.js equivalent of today's Deno
  standalone `target/twk` (all subcommands: `build/run/ir/fmt/check/lsp`).
- **lib** — an ESM module JS projects `import` to **compile** `.tw` source and
  **run** the result, with extern (JS host) functions wired in through a scoped
  per-run object instead of `globalThis` pollution — including auto-resolution
  of host globals. (CommonJS consumers can load it via Node's ESM interop, but
  CJS is not a primary target.)

This removes two frictions:

1. Nothing is published to npm today; embedding Twinkle means hand-copying
   `runtime.mjs` + `bridge.wasm` out of the repo.
2. Extern wiring is forced through `globalThis[module][name]`, polluting globals
   and offering no per-run scoping or clear error when a binding is missing.

## Non-Goals

- **No language changes.** JS→Twinkle (extern imports) is the only boundary.
  Calling Twinkle-exported functions *from* JS (bidirectional FFI) is a separate
  future project — Twinkle has no "export to host" syntax today.
- No prebuilt native binaries / per-platform packages. Pure-JS package.
- No TypeScript `.d.ts` stub generation from extern declarations (possible
  later).
- No rework of boundary type marshaling — the existing Int/Float/Bool/String
  heuristic is reused as-is.
- No Deno changes. The existing Deno standalone build path stays untouched.

## Decisions

| Topic | Decision |
|---|---|
| Package name | `@twinkle-lang/twinkle` (scoped under `@twinkle-lang` org) |
| Bin name | `twk` |
| Module format | ESM-only (`"type": "module"`). Use `import`; CJS via Node ESM interop only (not a primary target — `require(esm)` is unflagged only on Node ≥22.12). |
| Package version | Carried in `tools/npm/package.json`, bumped manually per release (no auto-sync). |
| `engines.node` | `>=22` (Wasm GC stable; JSPI auto-detected, degrades gracefully) |
| Lib capability | compile + run |
| Call direction | JS→Twinkle only (extern imports) |
| Extern wiring | scoped per-run `imports` object; missing modules fall back to `globalThis`; still-missing → strict error |
| Shadowing | `imports` wins over `globalThis` (explicit beats ambient) |
| Compiler payload | single full `boot.wasm` (from `make stage2`), shared by bin + lib |

## Key Architecture Insight

There are two independent payloads with very different roles:

- **`bridge.wasm`** (672 bytes) + `runtime.mjs` — everything needed to **run**
  any pre-compiled Twinkle `.wasm`. `run(wasmBytes, …)` loads *only* this.
- **`boot.wasm`** (~2.85 MB) — the self-hosted compiler, needed **only to
  compile** `.tw → wasm`. `compile()` / `runFile()` load this; `run()` never
  does.

The bin ships the full `boot.wasm` so the npm `twk` keeps every subcommand
(notably `fmt`, `check`, `lsp` for editor tooling). The lib's `compile` reuses
that same artifact; the lib's `run` is pure-JS.

## Single Source of Truth (no forked runtime)

`tools/js_runtime/runtime.mjs` remains the canonical host runtime (already
Node-compatible — uses `node:fs`, `process`, `Buffer`, `performance`,
`Atomics`). Two siblings are added next to it; nothing is duplicated:

- `tools/js_runtime/node_main.mjs` — Node CLI entry (the Node analogue of
  `deno_main.mjs`).
- `tools/js_runtime/index.mjs` — the lib API (`compile` / `run` / `runFile`).

A staging step assembles the publishable, self-contained package; see
*Packaging*.

## Runtime Change — Scoped Extern Imports (the wiring fix)

Extend `prepareWasm` + `autoBridgeExternImports` in `runtime.mjs` to accept an
optional `opts.imports` map (`{ moduleName: { fnName: jsFunction } }`).

Resolution order, per wasm import (`module`.`name`) that is not already a
provided host import and is of kind `function`:

1. `opts.imports[module]?.[name]` — scoped wiring (explicit).
2. `globalThis[module]?.[name]` — host-globals fallback (`Math`, `console`,
   `crypto`, …).
3. Otherwise: record as **missing**.

After scanning all imports, if any are missing, throw **one** error listing
every unsatisfied `module.name`, e.g.:

```
Missing host import(s): canvas.draw_rect, canvas.clear
Provide them via run(..., { imports: { canvas: { draw_rect, clear } } })
or define them on globalThis.
```

Marshaling of arguments/returns is unchanged. This is additive and backward
compatible: the Deno CLI and existing callers pass no `imports`, so behavior is
identical to today (globals-only, but now with a clear missing-import error
instead of a cryptic instantiate failure).

`opts.imports` is threaded through the public entry points
(`runWasmBytes`, `runWasmBytesAsync`, and their `*File` variants) into
`prepareWasm`.

**Nested `host.run_wasm` inheritance.** A running program can spawn a child via
`host.run_wasm`. To keep scoped wiring consistent, `imports` is stored on the
per-run `runtime` object and forwarded into the child runner (both the sync and
the JSPI async `run_wasm` paths), so child programs inherit the parent run's
extern imports. (The CLI passes no `imports`, so its children remain
globals-only, exactly as today.)

## Lib API (`tools/js_runtime/index.mjs`)

```js
import { compile, run, runFile } from "@twinkle-lang/twinkle";

// compile: path string | { source, path? }  ->  Uint8Array (wasm bytes)
const wasm = await compile("game.tw");

// run: wasm bytes + options. Loads only bridge.wasm (no compiler).
//   opts: { imports, args, cwd, env, stdout, stderr }
await run(wasm, { imports: { canvas } });   // Math/console auto-resolve from globalThis

// runFile: compile + run convenience
await runFile("game.tw", { imports: { canvas } });

// If a program only uses host-global externs (or none), no imports needed:
await runFile("calc.tw");
```

- `compile(input, opts?)` invokes the embedded `boot.wasm` compiler with
  `build`-equivalent arguments. Implementation: write input to a temp path if
  given as source, run the compiler with `-o <temp.wasm>` into the OS temp dir,
  read the bytes back, clean up, return `Uint8Array`. (The compiler is a CLI
  that writes to a file path; the lib hides that behind a bytes-returning API.)
  **Source-context limitation:** a path argument gets full project/import
  support (relative `use .sibling`, walk-up to `twinkle.toml`); `{ source }` is
  written to a temp dir and is single-file only — relative imports and project
  root won't resolve as they would at the original location. This is documented,
  not worked around.
- `run(wasmBytes, opts?)` calls the JSPI-aware async runner with the new
  `imports` plumbing and the bundled `bridge.wasm`.
- `runFile` / `runSource` are thin compile-then-run helpers.

Asset loading inside the lib resolves `boot.wasm` / `bridge.wasm` from
package-relative paths (`import.meta.dirname`), with `BOOT_WASM` / `BRIDGE_WASM`
env overrides for development.

## Node CLI (`tools/js_runtime/node_main.mjs`)

Mirrors `deno_main.mjs` using Node APIs:

- Shebang `#!/usr/bin/env node`.
- `guestArgs` = `process.argv.slice(2)`; `cwd` = `process.cwd()`;
  `env` = `process.env`.
- `stdout` / `stderr` adapters over `process.stdout` / `process.stderr`,
  exposing `fd` so `host.stdout_write_bytes` can use the fast `writeSync(fd, …)`
  path (matching the Deno entry's behavior).
- Loads `boot.wasm` + `bridge.wasm` from package-relative paths, with
  `BOOT_WASM` / `BRIDGE_WASM` overrides.
- Delegates to `runWasmBytesAsync` and `process.exit(code)`.

All subcommands (`build/run/ir/fmt/check/lsp`) already live inside `boot.wasm`;
the entry only forwards argv — no per-command logic in JS.

## Packaging

A new `tools/build_npm_pkg.sh`, driven by Makefile targets, stages a
self-contained package into `target/npm/` and (optionally) publishes it. The
package contains only flat files (no cross-directory relative imports):

```
target/npm/
  package.json      # copied from tools/npm/package.json (version lives there)
  README.md         # install + usage (lib & CLI)
  node.mjs          # copy of node_main.mjs (bin entry), chmod +x
  index.mjs         # copy of lib API
  runtime.mjs       # copy of canonical runtime
  boot.wasm         # freshly built by `make stage2`
  bridge.wasm       # copy of tools/bridge.wasm
```

`package.json`:

```json
{
  "name": "@twinkle-lang/twinkle",
  "version": "0.1.0",
  "publishConfig": { "access": "public" },
  "type": "module",
  "bin": { "twk": "./node.mjs" },
  "exports": { ".": "./index.mjs" },
  "files": ["node.mjs", "index.mjs", "runtime.mjs", "boot.wasm", "bridge.wasm", "README.md"],
  "engines": { "node": ">=22" },
  "license": "MIT"
}
```

Makefile targets:

- `make npm-pack` — depends on `stage2` (fresh `boot.wasm`); stages
  `target/npm/` and runs `npm pack` there. `boot.wasm` is gitignored and built
  at pack time, so the package always ships a verified self-hosted payload.
- `make npm-publish` — stage + `npm publish` from `target/npm/`.
- The package version lives in `tools/npm/package.json` and is bumped manually
  per release (no auto-sync from any other file).

## Publishing (manual, for now)

Publishing is a manual operation; there is no CI auto-publish in this scope.
One-time setup: create the `@twinkle-lang` organization on npmjs.com (free for
public packages — reserves the scope), then `npm login` locally. Release flow:
`npm whoami` to confirm auth → `make npm-pack` → inspect the tarball file list →
`cd target/npm && npm publish --dry-run` → `npm publish` (the manifest's
`publishConfig.access: public` makes the scoped package public). A future CI
publish would use an automation `NPM_TOKEN`; out of scope here.

## Tests (`make npm-test`)

Colocated `node:test` files under `tools/js_runtime/` (`runtime.test.mjs`,
`index.test.mjs`, `cli.test.mjs`), run via `make npm-test` (`node --test
tools/js_runtime/*.test.mjs`), asserting:

1. **Compile + run with scoped imports**: compile+run `examples/extern_ffi.tw`
   with `imports: { console, Math }` (or relying on globals) and check captured
   output.
2. **globalThis fallback**: omit `Math`/`console` from `imports` and confirm
   they auto-resolve.
3. **imports shadow globals**: pass a custom `console` capturing output and
   confirm it wins over the real one.
4. **Strict missing-import error**: run a program with an unsatisfied extern and
   assert the thrown error names the exact missing `module.fn`.

## Docs

- New `docs/js-embedding.md`: install (`npm i @twinkle-lang/twinkle`), CLI usage
  (`npx twk run file.tw`, full subcommand list), and the lib/extern-wiring guide
  (compile/run, `imports` object, globals fallback, missing-import errors).
- README section linking to it.

## Build / Implementation Order

1. Runtime: thread `opts.imports` through `prepareWasm` /
   `autoBridgeExternImports` (and into nested `host.run_wasm`); add globals
   fallback ordering + strict missing-import aggregation error. Keep Deno path
   behavior identical.
2. `node_main.mjs` — Node CLI entry.
3. `index.mjs` — lib API (`compile` / `run` / `runFile`).
4. `tools/build_npm_pkg.sh` + `package.json` template + Makefile targets
   (`npm-pack`, `npm-publish`).
5. Colocated `tools/js_runtime/*.test.mjs` + `make npm-test` + end-to-end
   tarball install verification.
6. `docs/js-embedding.md` + README section.
