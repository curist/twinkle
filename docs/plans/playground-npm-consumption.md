# Playground consumes published npm packages

**Status:** Design approved, pending implementation plan
**Date:** 2026-06-06

## Goal

Make the browser playground a plain Vite app that consumes the published
`@twinkle-lang/twinkle` compiler package and the published tree-sitter grammar
package, so it:

1. No longer builds a separate slim compiler (`boot/playground.tw` →
   `target/playground.wasm`).
2. Reuses the package's `runtime.mjs` instead of a hand-maintained browser fork
   of the same host/marshaling/JSPI logic (`playground/public/worker.js`).
3. Reaches into **no** sibling repo directories and runs **no** asset-copying
   step (`scripts/copy-assets.mjs` is deleted).

Net effect: the playground installs from npm and runs `vite build` — nothing
Twinkle-specific, no `make`, no monorepo path reaching.

## Background

What the playground does today (`playground/`):

- `make playground-wasm` builds `target/playground.wasm` from `boot/playground.tw`
  — a slim compiler that omits LSP / IR-debug to stay small (~2.0 MB).
- `scripts/copy-assets.mjs` copies `playground.wasm` (as `boot.wasm`),
  `tools/bridge.wasm`, the prelude/stdlib `.tw` sources, and the two tree-sitter
  wasms into `public/`.
- `public/worker.js` is a **classic** worker that reimplements, for the browser,
  what `tools/js_runtime/runtime.mjs` already does for Node/Deno: host imports,
  `autoBridgeExternImports`, JSPI, value marshaling, an in-memory VFS, plus a
  fetch-on-miss `read_file` that lazily pulls prelude/stdlib files.
- `src/main.js` imports the highlight query from
  `../../tree-sitter-twinkle/queries/highlights.scm?raw` and loads
  `./tree-sitter.wasm` + `./tree-sitter-twinkle.wasm` from `public/`.

Two facts that make this simplification possible:

- **The full `boot.wasm` (shipped on npm) embeds the prelude + stdlib** via the
  generated `boot/lib/module/core_lib.tw` under a virtual `/__twinkle_core`
  root. So a browser host filesystem only ever needs to hold the user's
  `/input/main.tw`; the prelude/stdlib fetching and the JSPI fetch-on-miss
  `read_file` are no longer needed.
- **The only reason `runtime.mjs` can't run in a browser** is its *static*
  `import { ... } from "node:fs"` / `node:path` at module top, plus `Buffer` and
  `process.stdin` usage inside. Remove those and the same module loads anywhere.

## Decisions

- **Sharing strategy: dependency-inject the host** (not Vite polyfills).
  Polyfilling would leave fs-population glue *and* an unsolved canvas-marshaling
  problem (see below) that forces a runtime change anyway; DI treats the
  node-dependence as the design issue it is and produces a lean bundle.
- **Browser adapter lives in the playground repo.** The npm package ships only
  the host-agnostic core + a Node adapter; browser/canvas concerns stay with the
  browser app rather than bloating a Node-focused package.
- **Resolution: published dependency with a local override.** The playground
  depends on the published `@twinkle-lang/twinkle`; a dev-only Vite alias
  (gated by `TWINKLE_LOCAL=1`) points at the in-repo build for development.
- **Use the full `boot.wasm`** (~2.86 MB) and drive it with `run /input/main.tw`
  — one call into the full compiler, no separate compile step. `boot.wasm`'s
  `run` compiles in-memory and invokes `host.run_wasm`, which recursively runs
  the compiled child with the same `imports`, so user `canvas`/`http`/`timer`
  externs reach the program.
- **Publish the tree-sitter grammar** so highlight assets come from a package
  too, eliminating `copy-assets.mjs`.
- **Testing: minimal.** Keep the existing colocated `tools/js_runtime/*.test.mjs`
  green and rely on the stage2 self-host rebuild as the bootstrap safety net;
  add a short manual smoke check. No new automated browser tests.

## The canvas-marshaling gap (why a configurable marshal spec is required)

`runtime.mjs`'s `autoBridgeExternImports` marshals every non-numeric extern arg
by assuming it is a string and calling `decodeString`. Canvas externs pass a
2D-context **externref**; `decodeString(ctx)` recurses on an opaque host object
and **stack-overflows Safari**. Today's `public/worker.js` avoids this with an
`EXTERN_ARG_MARSHAL` table that marks canvas args as `'raw'` (pass through
untouched). The shared core must absorb that as an optional, injectable marshal
spec, or the browser loses canvas support.

## Design

### 1. Runtime seam (`tools/js_runtime/`)

`runtime.mjs` loses **all** static `node:` imports and `Buffer`; node-specific
behavior moves behind an injected `host` adapter.

- New `tools/js_runtime/node_host.mjs` exports `nodeHost`, an object with:
  `readFile`, `writeFile`, `writeBytes`, `exists`, `listDir`, `mkdirp`,
  `resolvePath`, `readStdin`, and `stdinEof`. The current node stdin logic
  (`readSync(0, …)`, `process.stdin` events, `sleepSyncMs`) lives here.
- `runtime.mjs` core takes `opts.host` and routes all filesystem + stdin host
  imports through it. Byte arrays become `Uint8Array` everywhere (no `Buffer`).
- `autoBridgeExternImports` / `prepareWasm` gain an optional `marshalSpec`
  (`module → name → ('raw' | 'string')[]`). Absent ⇒ today's behavior (string
  fallback). The recursive `run_wasm` threads `host` + `marshalSpec` down, as it
  already threads `bridgeBytes` + `imports`.
- Node entry points (`index.mjs`, `node_main.mjs`, `deno_main.mjs`) import
  `nodeHost` and pass it. **Observable behavior is unchanged**; this is the only
  edit those files need.
- **Makefile dependency tracking:** because `deno_main.mjs` / `node_main.mjs`
  now depend on `node_host.mjs`, add `tools/js_runtime/node_host.mjs` to the
  `$(STAGE2_WASM)` prerequisite list (`Makefile:61`) and the `target/twk`
  prerequisite list (`Makefile:84`). Without this, the "stage2 self-host
  rebuild" safety net can silently link stale host code. (`npm-pack` already
  globs `tools/js_runtime/*.mjs`, so it needs no change.)

### 2. Package changes (`tools/npm/`)

- `package.json` `exports` gains subpaths so Vite can resolve assets:
  `"./runtime.mjs"`, `"./node_host.mjs"`, `"./boot.wasm"`, `"./bridge.wasm"`.
- `files` and `tools/build_npm_pkg.sh` add `node_host.mjs`.
- Bump version and publish (the browser consumer needs the host-agnostic
  runtime + subpath exports).

### 3. Tree-sitter grammar package

- Publish `tree-sitter-twinkle` (its `package.json` already lists `queries/*`
  and `*.wasm` in `files`, so the grammar wasm + highlight query ship as-is).
  Fix the placeholder `repository` URL before publishing.
- The package has no `exports` map, so subpath imports of `queries/highlights.scm`
  and the grammar wasm are allowed by default.
- **Native-binding install must be neutralized for the browser consumer.** The
  package's `"install": "node-gyp-build"` script compiles the native Node
  binding (`bindings/node/binding.cc`), which the browser never uses — and this
  repo currently ships **no** `prebuilds/`, so a plain `npm install` in the
  playground would attempt a node-gyp compile and can fail. The playground only
  needs `queries/*` + `*.wasm`. Chosen fix: **the playground installs with
  `--ignore-scripts`** (set `ignore-scripts=true` in `playground/.npmrc` so both
  local `npm install` and CI skip the native build). Alternatives, if we later
  decide the grammar package is wasm-only for all consumers: drop the `install`
  script + native deps from the published package, or publish real
  `prebuildify` prebuilds. The `.npmrc` route is chosen as the lowest-blast
  option that does not change the grammar package's external contract.

### 4. Playground changes (`playground/`)

- `package.json`: add `@twinkle-lang/twinkle` and `tree-sitter-twinkle` as
  dependencies (published versions), and a `playground/.npmrc` with
  `ignore-scripts=true` (see §3). A dev-only Vite alias block, gated by
  `TWINKLE_LOCAL=1`, must map **each subpath the playground imports** to the
  in-repo build — aliasing the bare package name alone does not cover subpaths:
  - `@twinkle-lang/twinkle/runtime.mjs` → `../tools/js_runtime/runtime.mjs`
  - `@twinkle-lang/twinkle/boot.wasm`   → `../target/boot.wasm`
  - `@twinkle-lang/twinkle/bridge.wasm` → `../tools/bridge.wasm`
- Worker moves `public/worker.js` → `src/worker.js` and becomes a **module
  worker**: `new Worker(new URL('./worker.js', import.meta.url), { type: 'module' })`
  in `src/main.js`. It imports `runWasmBytesAsync` from
  **`@twinkle-lang/twinkle/runtime.mjs`** (not the bare package — `index.mjs`
  stays Node/library-oriented and is not browser-loadable), loads `boot.wasm`
  and `bridge.wasm` bytes from the package via `?url` + `fetch`, builds the
  browser host adapter + externs object + canvas marshal spec, and calls:

  ```js
  runWasmBytesAsync(bootBytes, {
    guestArgs: ["run", "/input/main.tw"],
    bridgeBytes, host, imports, marshalSpec,
  })
  ```

  then forwards stdout/stderr/exit over `postMessage`. The bare `run()` /
  `runFile()` API in `index.mjs` is **not** used by the browser.
- The browser host adapter (in the playground) provides: an in-memory VFS host
  holding only `/input/main.tw`, an EOF stdin, and stdout/stderr that post
  messages. Externs (`canvas` via `OffscreenCanvas`, `http` via `fetch`,
  `timer` via `setTimeout`) are passed as `imports`; the canvas marshal spec is
  passed as `marshalSpec`.
- `boot.wasm` / `bridge.wasm` are imported as Vite `?url` assets from the
  package (`@twinkle-lang/twinkle/boot.wasm?url`,
  `@twinkle-lang/twinkle/bridge.wasm?url`) — no copying.
- Highlighting: `src/main.js` imports `highlights.scm?raw` and the grammar wasm
  (`?url`) from `tree-sitter-twinkle`, and `web-tree-sitter`'s `tree-sitter.wasm`
  (`?url`) from its own package, passing those URLs to `Parser.init` /
  `Language.load`.

### 5. What gets deleted

- `boot/playground.tw`; the `target/playground.wasm` build (Makefile
  `playground-wasm` target, `PLAYGROUND_WASM` / `PLAYGROUND_ENTRY` vars, and the
  `target/playground.wasm` recipe + its use in the `playground`/`playground-dev`
  targets).
- `playground/scripts/copy-assets.mjs` entirely, and the `copy-assets` /
  `build` wiring in `playground/package.json` that calls it.
- In the worker: the duplicated host-import block, `autoBridgeExternImports`,
  value marshaling, the prelude/stdlib `*_FILES` lists, the VFS prelude fetch,
  and the JSPI fetch-on-miss `read_file`.

### 6. Testing & risks

- **Parity:** existing `tools/js_runtime/*.test.mjs` stay green after the
  `nodeHost` injection; the stage2 self-host rebuild (runtime is a bootstrap
  prereq) is the integration check.
- **Smoke:** `make playground-dev` (or the local-override build) — run a plain
  program, a canvas program (Safari specifically, the marshal-spec regression
  target), and an `http.fetch` JSPI program.
- **Risks:**
  - Editing `runtime.mjs` triggers a full stage2 rebuild — slow, but it is the
    safety net.
  - +~820 KB wasm (2.04 MB → 2.86 MB); gzip mitigates wire cost.
  - The module-worker change relies on Vite emitting a correctly scoped worker
    chunk.
  - The playground builds against the **published** version, so the
    `@twinkle-lang/twinkle` (and grammar) publishes must land before the
    playground change can build in CI without the local override.

## Sequencing

1. Runtime DI refactor + `node_host.mjs` + marshal spec; add
   `tools/js_runtime/node_host.mjs` to the `$(STAGE2_WASM)` and `target/twk`
   Makefile prerequisite lists; keep Node/Deno paths green (tests + stage2
   rebuild).
2. Package `exports`/`files`/build-script updates; version bump; publish
   `@twinkle-lang/twinkle`.
3. Publish `tree-sitter-twinkle` (fix repository URL).
4. Playground: module worker + browser adapter + asset imports from packages;
   add `playground/.npmrc` (`ignore-scripts=true`); add the `TWINKLE_LOCAL`
   subpath aliases; delete `copy-assets.mjs` and worker duplication.
5. Makefile/repo cleanup: remove `boot/playground.tw` and the playground-wasm
   build.
