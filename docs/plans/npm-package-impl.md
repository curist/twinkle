# @twinkle-lang/twinkle npm Package — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship Twinkle on npm as `@twinkle-lang/twinkle` — a Node.js `twk` CLI plus an importable compile/run library that wires JS extern functions through a scoped per-run `imports` object (with `globalThis` fallback and clear missing-import errors).

**Architecture:** Keep `tools/js_runtime/runtime.mjs` as the single canonical host runtime. Add two thin siblings — `node_main.mjs` (CLI entry) and `index.mjs` (lib API) — plus a staging script that flattens runtime + entries + `boot.wasm` + `bridge.wasm` into `target/npm/` for publishing. The only behavioral change to the runtime is scoped extern-import resolution. `run()` loads only `bridge.wasm` (672 B); only `compile()` loads the 2.85 MB `boot.wasm`.

**Tech Stack:** Node.js ≥22 (ESM, Wasm GC, optional JSPI), the existing self-hosted `boot.wasm` compiler, `node:test`, bash + Make for packaging.

**Design spec:** `docs/plans/npm-package.md`

---

## File Structure

**Create:**
- `tools/js_runtime/node_main.mjs` — Node CLI entry (forwards argv into `boot.wasm`).
- `tools/js_runtime/index.mjs` — lib API: `compile` / `run` / `runFile` / `runSource`.
- `tools/js_runtime/runtime.test.mjs` — unit tests for `resolveExternImports`.
- `tools/js_runtime/index.test.mjs` — integration tests for the lib (compile/run/wiring).
- `tools/js_runtime/cli.test.mjs` — subprocess test for the Node CLI entry.
- `tools/js_runtime/fixtures/scoped_extern.tw` — program with a non-global extern.
- `tools/js_runtime/fixtures/global_extern.tw` — program using `Math` + `console` externs.
- `tools/npm/package.json` — package manifest template (carries the version).
- `tools/npm/README.md` — package README (install + usage).
- `tools/build_npm_pkg.sh` — stages a self-contained package into `target/npm/`.
- `docs/js-embedding.md` — user docs for the CLI + lib.

**Modify:**
- `tools/js_runtime/runtime.mjs` — add `resolveExternImports`, thread `opts.imports`, strict missing-import error.
- `Makefile` — add `npm-pack`, `npm-publish`, `npm-test` targets (+ `.PHONY`).
- `README.md` — add an "Install from npm" section linking to `docs/js-embedding.md`.

Asset/import-path convention (works identically in the dev tree and the flattened package):
- Entries import the runtime as `./runtime.mjs` (same directory in both layouts).
- `boot.wasm` resolves `./boot.wasm` (packaged) then `../../target/boot.wasm` (dev).
- `bridge.wasm` resolves `./bridge.wasm` (packaged) then `../bridge.wasm` (dev, = `tools/bridge.wasm`).

---

## Task 1: Runtime — scoped extern-import resolution

**Files:**
- Modify: `tools/js_runtime/runtime.mjs`
- Test: `tools/js_runtime/runtime.test.mjs`

- [ ] **Step 1: Write the failing unit tests**

Create `tools/js_runtime/runtime.test.mjs`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { resolveExternImports } from "./runtime.mjs";

test("scoped imports win over globals", () => {
  const scopedFn = () => "scoped";
  const globalFn = () => "global";
  const { found, missing } = resolveExternImports(
    [{ module: "m", name: "f", kind: "function" }],
    {},
    { m: { f: scopedFn } },
    { m: { f: globalFn } },
  );
  assert.deepEqual(missing, []);
  assert.equal(found.length, 1);
  assert.equal(found[0].fn, scopedFn);
  assert.equal(found[0].recv.f, scopedFn);
});

test("falls back to globals when not scoped", () => {
  const globalFn = () => 1;
  const { found, missing } = resolveExternImports(
    [{ module: "Math", name: "sqrt", kind: "function" }],
    {},
    {},
    { Math: { sqrt: globalFn } },
  );
  assert.deepEqual(missing, []);
  assert.equal(found[0].fn, globalFn);
});

test("aggregates missing imports", () => {
  const { found, missing } = resolveExternImports(
    [
      { module: "a", name: "x", kind: "function" },
      { module: "a", name: "y", kind: "function" },
    ],
    {},
    {},
    {},
  );
  assert.equal(found.length, 0);
  assert.deepEqual(missing, ["a.x", "a.y"]);
});

test("skips already-provided host imports", () => {
  const { found, missing } = resolveExternImports(
    [{ module: "host", name: "print", kind: "function" }],
    { host: { print: () => {} } },
    {},
    {},
  );
  assert.deepEqual(missing, []);
  assert.equal(found.length, 0);
});

test("skips non-function imports", () => {
  const { found, missing } = resolveExternImports(
    [{ module: "env", name: "memory", kind: "memory" }],
    {},
    {},
    {},
  );
  assert.deepEqual(missing, []);
  assert.equal(found.length, 0);
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `node --test tools/js_runtime/runtime.test.mjs`
Expected: FAIL — `resolveExternImports` is not exported (`SyntaxError`/`undefined is not a function`).

- [ ] **Step 3: Add `resolveExternImports` and rewrite `autoBridgeExternImports`**

In `tools/js_runtime/runtime.mjs`, replace the entire `autoBridgeExternImports` function (currently in the "Extern auto-bridging" section) with this exported helper plus the rewritten bridger:

```js
/**
 * Resolve each wasm extern import to a JS function.
 * Resolution order per import: scoped `imports[module][name]`, then
 * `globals[module][name]`. Imports already satisfied by `hostImports`, or that
 * are not functions, are skipped. Returns the resolved bindings plus a list of
 * unresolved "module.name" strings.
 */
export function resolveExternImports(importList, hostImports, imports = {}, globals = globalThis) {
  const found = [];
  const missing = [];
  for (const imp of importList) {
    if (hostImports[imp.module]?.[imp.name] !== undefined) continue;
    if (imp.kind !== "function") continue;

    const scopedRecv = imports[imp.module];
    const scoped = scopedRecv?.[imp.name];
    if (typeof scoped === "function") {
      found.push({ module: imp.module, name: imp.name, fn: scoped, recv: scopedRecv });
      continue;
    }

    const globalRecv = globals[imp.module];
    const global = globalRecv?.[imp.name];
    if (typeof global === "function") {
      found.push({ module: imp.module, name: imp.name, fn: global, recv: globalRecv });
      continue;
    }

    missing.push(`${imp.module}.${imp.name}`);
  }
  return { found, missing };
}

/**
 * Auto-bridge extern imports by resolving each to `imports[module][name]` (a
 * scoped per-run object) or `globalThis[module][name]`, wrapping with type
 * conversions for Twinkle's extern-safe types:
 *   - String params (GC refs) are decoded via bridge
 *   - String returns from JS are encoded via bridge
 *   - Int (bigint), Float (number), Bool (i32) pass through
 * Throws a single aggregated error if any extern import is unsatisfied.
 */
function autoBridgeExternImports(wasmModule, hostImports, b, jspi = false, imports = {}) {
  let importList;
  try {
    importList = WebAssembly.Module.imports(wasmModule);
  } catch {
    // Module.imports may fail on GC modules in some runtimes; nothing to bridge.
    return;
  }

  const { found, missing } = resolveExternImports(importList, hostImports, imports);

  if (missing.length > 0) {
    const [m0, f0] = missing[0].split(".");
    throw new Error(
      `Missing host import(s): ${missing.join(", ")}\n` +
      `Provide them via the run() "imports" option ` +
      `(e.g. { imports: { ${m0}: { ${f0}: fn } } }) or define them on globalThis.`,
    );
  }

  const marshalArgs = (args) => args.map((arg) => {
    if (typeof arg === "bigint") return Number(arg);
    if (typeof arg === "number") return arg;
    // GC ref — assume string
    return decodeString(b, arg);
  });

  const marshalReturn = (result) => {
    if (result === undefined || result === null) return;
    if (typeof result === "string") return encodeString(b, result);
    if (typeof result === "number") return result;
    if (typeof result === "bigint") return result;
    return result;
  };

  for (const { module, name, fn, recv } of found) {
    let bridgedFn;
    if (jspi) {
      const asyncWrapper = async (...args) =>
        marshalReturn(await fn.apply(recv, marshalArgs(args)));
      bridgedFn = new WebAssembly.Suspending(asyncWrapper);
    } else {
      bridgedFn = (...args) => {
        const result = fn.apply(recv, marshalArgs(args));
        if (result instanceof Promise) {
          throw new Error(
            `Extern ${module}.${name} returned a Promise, but JSPI is not available. ` +
            `Promise-returning externs require a runtime with WebAssembly.Suspending/promising support.`,
          );
        }
        return marshalReturn(result);
      };
    }
    if (!hostImports[module]) hostImports[module] = {};
    hostImports[module][name] = bridgedFn;
  }
}
```

- [ ] **Step 4: Thread `opts.imports` through `prepareWasm`**

In `tools/js_runtime/runtime.mjs`, in `prepareWasm`, add `imports` to the destructured opts and pass it to the bridger. Change the opts destructure block to include:

```js
    bridgeBytes,
    imports = {},
  } = opts;
```

and change the bridger call from:

```js
  autoBridgeExternImports(mainModule, hostImports, b, jspi);
```

to:

```js
  autoBridgeExternImports(mainModule, hostImports, b, jspi, imports);
```

Also store `imports` on the per-run `runtime` object so nested `host.run_wasm`
children can inherit it. In `prepareWasm`, change the `runtime` object literal from:

```js
  const runtime = {
    programArgs: [programPath, ...guestArgs],
    cwd,
    env,
    stdout,
    stderr,
    stdinEof: false,
  };
```

to:

```js
  const runtime = {
    programArgs: [programPath, ...guestArgs],
    cwd,
    env,
    stdout,
    stderr,
    stdinEof: false,
    imports,
  };
```

(`runWasmBytes`, `runWasmBytesAsync`, `runWasmFile`, `runWasmFileAsync` already forward their full `opts` into `prepareWasm`, so `imports` flows through automatically — no further edits there.)

- [ ] **Step 5: Forward inherited imports through nested `host.run_wasm`**

So a Twinkle program that spawns a child via `host.run_wasm` passes its scoped
imports down. Two call sites in `tools/js_runtime/runtime.mjs`.

In `makeHostImports`, the synchronous `run_wasm`, change the child-run options from:

```js
        const exitCode = runWasmBytes(childBytes, {
          programPath: programPath ?? "<memory>.wasm",
          guestArgs,
          cwd: runtime.cwd,
          env: runtime.env,
          stdout: runtime.stdout,
          stderr: runtime.stderr,
          bridgeBytes,
        });
```

to (add the `imports` line):

```js
        const exitCode = runWasmBytes(childBytes, {
          programPath: programPath ?? "<memory>.wasm",
          guestArgs,
          cwd: runtime.cwd,
          env: runtime.env,
          stdout: runtime.stdout,
          stderr: runtime.stderr,
          imports: runtime.imports,
          bridgeBytes,
        });
```

In `runWasmBytesAsync`, the JSPI `run_wasm` override, change:

```js
        const exitCode = await runWasmBytesAsync(childBytes, {
          programPath: programPath ?? "<memory>.wasm",
          guestArgs,
          cwd: runtime.cwd,
          env: runtime.env,
          stdout: runtime.stdout,
          stderr: runtime.stderr,
          bridgeBytes: childBridgeBytes,
        });
```

to (add the `imports` line):

```js
        const exitCode = await runWasmBytesAsync(childBytes, {
          programPath: programPath ?? "<memory>.wasm",
          guestArgs,
          cwd: runtime.cwd,
          env: runtime.env,
          stdout: runtime.stdout,
          stderr: runtime.stderr,
          imports: runtime.imports,
          bridgeBytes: childBridgeBytes,
        });
```

(`runtime.imports` defaults to `{}` from the `prepareWasm` destructure, so the
CLI and other no-imports callers keep globals-only child behavior.)

- [ ] **Step 6: Run the tests to verify they pass**

Run: `node --test tools/js_runtime/runtime.test.mjs`
Expected: PASS (5 tests).

- [ ] **Step 7: Verify the Deno CLI path is unaffected**

Run: `BOOT_WASM=target/boot.wasm deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs run examples/extern_ffi.tw`
Expected: prints the extern_ffi output (e.g. "Hello from Twinkle via extern FFI!") and exits 0 — `console`/`Math` still resolve from `globalThis` with no `imports` passed.

- [ ] **Step 8: Commit**

```bash
git add tools/js_runtime/runtime.mjs tools/js_runtime/runtime.test.mjs
git commit -m "Add scoped extern-import resolution to JS runtime

Resolve wasm extern imports from a scoped per-run imports object first,
then globalThis, aggregating any unsatisfied imports into one clear error.
Imports are stored on the run context and inherited by nested host.run_wasm
children. Backward compatible: callers passing no imports keep globals-only
behavior."
```

---

## Task 2: Node CLI entry (`node_main.mjs`)

**Files:**
- Create: `tools/js_runtime/node_main.mjs`
- Create: `tools/js_runtime/cli.test.mjs`

- [ ] **Step 1: Write the failing subprocess test**

Create `tools/js_runtime/cli.test.mjs`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const entry = join(here, "node_main.mjs");
const repoRoot = join(here, "..", "..");

test("twk CLI runs a Twinkle program", () => {
  const out = execFileSync("node", [entry, "run", join(repoRoot, "examples", "fizzbuzz.tw")], {
    encoding: "utf8",
  });
  assert.match(out, /Fizz/);
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `node --test tools/js_runtime/cli.test.mjs`
Expected: FAIL — `node_main.mjs` does not exist (`Cannot find module`).

- [ ] **Step 3: Create the Node CLI entry**

Create `tools/js_runtime/node_main.mjs`:

```js
#!/usr/bin/env node
// Node.js entry wrapper for the Twinkle CLI (twk).
//
// Mirrors tools/js_runtime/deno_main.mjs using Node APIs. The full self-hosted
// compiler (boot.wasm) handles every subcommand (build/run/ir/fmt/check/lsp);
// this wrapper only loads the embedded payloads, adapts stdio, and forwards
// process.argv into the shared runtime.

import { readFileSync, writeSync } from "node:fs";
import { resolve } from "node:path";
import { runWasmBytesAsync } from "./runtime.mjs";

const textEncoder = new TextEncoder();
const here = import.meta.dirname;

function writeAllFd(fd, bytes) {
  let offset = 0;
  while (offset < bytes.byteLength) {
    const written = writeSync(fd, bytes, offset, bytes.byteLength - offset);
    if (written <= 0) throw new Error("stdout write made no progress");
    offset += written;
  }
}

function nodeStream(fd) {
  return {
    fd,
    write(chunk) {
      const bytes = typeof chunk === "string"
        ? textEncoder.encode(chunk)
        : new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
      writeAllFd(fd, bytes);
      return true;
    },
  };
}

function readFirst(paths) {
  let lastError;
  for (const p of paths) {
    try { return readFileSync(p); } catch (e) { lastError = e; }
  }
  throw lastError ?? new Error("no paths provided");
}

function loadBootWasm() {
  const override = process.env.BOOT_WASM;
  if (override) return readFileSync(resolve(override));
  try {
    return readFirst([
      `${here}/boot.wasm`,              // packaged (flat layout)
      `${here}/../../target/boot.wasm`, // dev fallback
    ]);
  } catch (e) {
    console.error(`Error: boot compiler wasm not found: ${e.message}`);
    console.error("Build it with: make stage2");
    process.exit(1);
  }
}

function loadBridgeWasm() {
  const override = process.env.BRIDGE_WASM;
  if (override) return readFileSync(resolve(override));
  try {
    return readFirst([
      `${here}/bridge.wasm`,    // packaged
      `${here}/../bridge.wasm`, // dev fallback (tools/bridge.wasm)
    ]);
  } catch (e) {
    console.error(`Error: bridge wasm not found: ${e.message}`);
    console.error("Regenerate with: ./target/release/twk run boot/tests/gen_bridge_wasm.tw");
    process.exit(1);
  }
}

async function main() {
  const bootOverride = process.env.BOOT_WASM;
  const exitCode = await runWasmBytesAsync(loadBootWasm(), {
    programPath: bootOverride ? resolve(bootOverride) : "twk.wasm",
    guestArgs: process.argv.slice(2),
    cwd: process.cwd(),
    env: process.env,
    stdout: nodeStream(1),
    stderr: nodeStream(2),
    bridgeBytes: loadBridgeWasm(),
  });
  process.exit(exitCode);
}

main().catch((e) => {
  if (e.message?.startsWith("host.error:")) process.exit(1);
  console.error(e.stack || e.message || e);
  process.exit(1);
});
```

Then mark the source executable so the shebang is usable directly and the bit
is preserved through git:

```bash
chmod +x tools/js_runtime/node_main.mjs
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `node --test tools/js_runtime/cli.test.mjs`
Expected: PASS (uses the dev `target/boot.wasm` fallback).

- [ ] **Step 5: Smoke-check a few subcommands manually**

Run: `node tools/js_runtime/node_main.mjs check examples/point.tw`
Expected: type-checks cleanly, exit 0.

Run: `node tools/js_runtime/node_main.mjs build examples/fizzbuzz.tw -o /tmp/fb.wasm && node tools/js_runtime/node_main.mjs run /tmp/fb.wasm | head -1`
Expected: builds, then runs the compiled wasm and prints the first line.

- [ ] **Step 6: Commit**

```bash
git add tools/js_runtime/node_main.mjs tools/js_runtime/cli.test.mjs
git commit -m "Add Node.js CLI entry for the twk compiler

Node analogue of deno_main.mjs: loads boot.wasm + bridge.wasm, adapts
stdio via writeSync, forwards process.argv into the shared runtime. All
subcommands continue to live inside boot.wasm."
```

---

## Task 3: Library API (`index.mjs`)

**Files:**
- Create: `tools/js_runtime/index.mjs`
- Create: `tools/js_runtime/fixtures/scoped_extern.tw`
- Create: `tools/js_runtime/fixtures/global_extern.tw`
- Create: `tools/js_runtime/index.test.mjs`

- [ ] **Step 1: Create the test fixtures**

Create `tools/js_runtime/fixtures/scoped_extern.tw`:

```tw
// host_app is NOT on globalThis — must be wired via the imports option.
extern host_app {
  fn emit(msg: String)
}

host_app.emit("hello from twinkle")
```

Create `tools/js_runtime/fixtures/global_extern.tw`:

```tw
// Math and console resolve from globalThis with zero wiring.
extern Math {
  fn sqrt(x: Float) Float
}

extern console {
  fn log(msg: String)
}

console.log("sqrt 16 = ${Math.sqrt(16.0)}")
```

- [ ] **Step 2: Write the failing integration tests**

Create `tools/js_runtime/index.test.mjs`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { compile, runFile } from "./index.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const fix = (name) => join(here, "fixtures", name);

test("compile returns wasm bytes", async () => {
  const wasm = await compile(fix("scoped_extern.tw"));
  assert.ok(wasm instanceof Uint8Array);
  assert.deepEqual([...wasm.slice(0, 4)], [0x00, 0x61, 0x73, 0x6d]); // "\0asm"
});

test("scoped imports receive marshaled values", async () => {
  const seen = [];
  const code = await runFile(fix("scoped_extern.tw"), {
    imports: { host_app: { emit: (msg) => { seen.push(msg); } } },
  });
  assert.equal(code, 0);
  assert.deepEqual(seen, ["hello from twinkle"]);
});

test("missing import throws naming the symbol", async () => {
  await assert.rejects(
    () => runFile(fix("scoped_extern.tw")), // no imports; host_app not global
    /Missing host import\(s\): host_app\.emit/,
  );
});

test("host globals auto-resolve without wiring", async () => {
  const code = await runFile(fix("global_extern.tw"));
  assert.equal(code, 0);
});

test("imports shadow globals", async () => {
  const lines = [];
  const code = await runFile(fix("global_extern.tw"), {
    imports: { console: { log: (m) => lines.push(m) } },
  });
  assert.equal(code, 0);
  assert.equal(lines.length, 1);
  assert.match(lines[0], /sqrt 16 = 4/);
});
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `node --test tools/js_runtime/index.test.mjs`
Expected: FAIL — `index.mjs` does not exist (`Cannot find module`).

- [ ] **Step 4: Create the lib API**

Create `tools/js_runtime/index.mjs`:

```js
// Library API for embedding Twinkle in JavaScript.
//
//   import { compile, run, runFile } from "@twinkle-lang/twinkle";
//
// compile(input)        -> Uint8Array  (loads boot.wasm)
// run(wasmBytes, opts)  -> exitCode    (loads only bridge.wasm)
// runFile(path, opts)   -> exitCode    (compile + run)

import { readFileSync, writeFileSync, rmSync, mkdtempSync } from "node:fs";
import { resolve, dirname, join, basename } from "node:path";
import { tmpdir } from "node:os";
import { runWasmBytesAsync } from "./runtime.mjs";

const here = import.meta.dirname;

function readFirst(paths) {
  let lastError;
  for (const p of paths) {
    try { return readFileSync(p); } catch (e) { lastError = e; }
  }
  throw lastError ?? new Error("no paths provided");
}

function loadBootWasm() {
  const override = process.env.BOOT_WASM;
  if (override) return readFileSync(resolve(override));
  return readFirst([
    `${here}/boot.wasm`,
    `${here}/../../target/boot.wasm`,
  ]);
}

function loadBridgeWasm() {
  const override = process.env.BRIDGE_WASM;
  if (override) return readFileSync(resolve(override));
  return readFirst([
    `${here}/bridge.wasm`,
    `${here}/../bridge.wasm`,
  ]);
}

function collectingStream() {
  const chunks = [];
  // Stream-decode so a multi-byte UTF-8 sequence split across writes is not
  // corrupted; flush the decoder in text().
  const dec = new TextDecoder();
  return {
    text() {
      return chunks.join("") + dec.decode();
    },
    write(chunk) {
      chunks.push(typeof chunk === "string" ? chunk : dec.decode(chunk, { stream: true }));
      return true;
    },
  };
}

/**
 * Compile Twinkle source to wasm bytes.
 * @param {string | {source: string, path?: string}} input
 *   A file path string — full project/import support (relative `use .sibling`,
 *   walk-up to `twinkle.toml`). Or `{ source, path? }` — written to a temp dir
 *   and compiled single-file only; relative imports and project-root discovery
 *   will NOT resolve as they would at the original location.
 * @returns {Promise<Uint8Array>}
 */
export async function compile(input, opts = {}) {
  const bootBytes = loadBootWasm();
  const bridgeBytes = loadBridgeWasm();

  let srcPath;
  let cleanupDir;
  if (typeof input === "string") {
    srcPath = resolve(input);
  } else if (input && typeof input.source === "string") {
    cleanupDir = mkdtempSync(join(tmpdir(), "twinkle-"));
    srcPath = join(cleanupDir, basename(input.path ?? "main.tw"));
    writeFileSync(srcPath, input.source);
  } else {
    throw new TypeError("compile: input must be a path string or { source, path? }");
  }

  const outPath = join(cleanupDir ?? tmpdir(), `twinkle-out-${process.pid}-${Date.now()}.wasm`);
  const out = collectingStream();
  const err = collectingStream();
  try {
    const code = await runWasmBytesAsync(bootBytes, {
      programPath: "twk.wasm",
      guestArgs: ["build", srcPath, "-o", outPath],
      cwd: opts.cwd ?? dirname(srcPath),
      env: process.env,
      stdout: out,
      stderr: err,
      bridgeBytes,
    });
    if (code !== 0) {
      throw new Error(`Twinkle compilation failed (exit ${code}):\n${err.text() || out.text()}`);
    }
    return new Uint8Array(readFileSync(outPath));
  } finally {
    try { rmSync(outPath, { force: true }); } catch {}
    if (cleanupDir) { try { rmSync(cleanupDir, { recursive: true, force: true }); } catch {} }
  }
}

/**
 * Run pre-compiled wasm bytes with optional scoped extern imports.
 * @param {Uint8Array} wasmBytes
 * @param {{imports?, args?, cwd?, env?, stdout?, stderr?, path?}} opts
 * @returns {Promise<number>} exit code
 */
export async function run(wasmBytes, opts = {}) {
  return runWasmBytesAsync(wasmBytes, {
    programPath: opts.path ?? "<memory>.wasm",
    guestArgs: opts.args ?? [],
    cwd: opts.cwd ?? process.cwd(),
    env: opts.env ?? process.env,
    stdout: opts.stdout ?? process.stdout,
    stderr: opts.stderr ?? process.stderr,
    bridgeBytes: loadBridgeWasm(),
    imports: opts.imports ?? {},
  });
}

/** Compile a file then run it. */
export async function runFile(path, opts = {}) {
  const wasm = await compile(path, opts);
  return run(wasm, { ...opts, path: resolve(path) });
}

/** Compile source text then run it. */
export async function runSource(source, opts = {}) {
  const wasm = await compile({ source, path: opts.path }, opts);
  return run(wasm, opts);
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `node --test tools/js_runtime/index.test.mjs`
Expected: PASS (5 tests). Note: the "host globals auto-resolve" test prints `sqrt 16 = 4` to the real stdout — that is expected.

- [ ] **Step 6: Commit**

```bash
git add tools/js_runtime/index.mjs tools/js_runtime/index.test.mjs tools/js_runtime/fixtures
git commit -m "Add embeddable compile/run library API

index.mjs exposes compile (source/path -> wasm bytes via boot.wasm) and
run/runFile/runSource (bridge.wasm only) with scoped extern imports.
Integration tests cover scoped wiring, globals fallback, shadowing, and
the missing-import error."
```

---

## Task 4: Packaging — staging script, manifest, README

**Files:**
- Create: `tools/npm/package.json`
- Create: `tools/npm/README.md`
- Create: `tools/build_npm_pkg.sh`
- Modify: `Makefile`

- [ ] **Step 1: Create the package manifest template**

Create `tools/npm/package.json` (version is carried here and bumped manually on release):

```json
{
  "name": "@twinkle-lang/twinkle",
  "version": "0.1.0",
  "description": "Twinkle — a statically typed language targeting WebAssembly GC. CLI (twk) plus an embeddable compile/run library.",
  "type": "module",
  "bin": { "twk": "./node.mjs" },
  "exports": { ".": "./index.mjs" },
  "files": ["node.mjs", "index.mjs", "runtime.mjs", "boot.wasm", "bridge.wasm", "README.md"],
  "engines": { "node": ">=22" },
  "license": "MIT",
  "publishConfig": { "access": "public" },
  "repository": { "type": "git", "url": "git+https://github.com/curist/twinkle.git" }
}
```

- [ ] **Step 2: Create the package README**

Create `tools/npm/README.md`:

```markdown
# @twinkle-lang/twinkle

Twinkle is a statically typed language targeting WebAssembly GC. This package
ships both the `twk` command-line compiler and an embeddable JS library for
compiling and running Twinkle programs from Node.js.

## Install

```bash
npm install @twinkle-lang/twinkle
```

## CLI

```bash
npx twk run path/to/program.tw
npx twk build path/to/program.tw -o out.wasm
npx twk fmt path/to/program.tw
```

## Library

```js
import { compile, run, runFile } from "@twinkle-lang/twinkle";

// Host functions declared in Twinkle as `extern canvas { fn draw_rect(...) }`
// are wired by passing a scoped imports object — no globalThis pollution.
await runFile("game.tw", {
  imports: {
    canvas: { draw_rect: (x, y, w, h) => { /* ... */ } },
  },
});

// Host globals (Math, console, crypto, ...) resolve automatically:
await runFile("calc.tw");

// Compile once, run many times:
const wasm = await compile("game.tw");
await run(wasm, { imports: { canvas } });
```

A missing extern import produces a clear error naming the exact `module.fn`.

Requires Node.js ≥ 22.
```

- [ ] **Step 3: Create the staging script**

Create `tools/build_npm_pkg.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUT_DIR="${OUT_DIR:-target/npm}"
SRC="tools/js_runtime"
BOOT_WASM="${BOOT_WASM:-target/boot.wasm}"
BRIDGE_WASM="${BRIDGE_WASM:-tools/bridge.wasm}"

if [[ ! -f "$BOOT_WASM" ]]; then
  printf 'error: missing compiler payload: %s\n' "$BOOT_WASM" >&2
  printf 'build it with:\n  make stage2\n' >&2
  exit 1
fi
if [[ ! -f "$BRIDGE_WASM" ]]; then
  printf 'error: missing bridge module: %s\n' "$BRIDGE_WASM" >&2
  exit 1
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

cp "$SRC/runtime.mjs"   "$OUT_DIR/runtime.mjs"
cp "$SRC/node_main.mjs" "$OUT_DIR/node.mjs"
cp "$SRC/index.mjs"     "$OUT_DIR/index.mjs"
cp "$BOOT_WASM"         "$OUT_DIR/boot.wasm"
cp "$BRIDGE_WASM"       "$OUT_DIR/bridge.wasm"
cp tools/npm/package.json "$OUT_DIR/package.json"
cp tools/npm/README.md    "$OUT_DIR/README.md"

# Ensure the bin is executable in the published tarball.
chmod +x "$OUT_DIR/node.mjs"

VERSION="$(node -p "require('./$OUT_DIR/package.json').version")"
printf 'Staged @twinkle-lang/twinkle v%s in %s\n' "$VERSION" "$OUT_DIR"
```

- [ ] **Step 4: Make the staging script executable**

Run: `chmod +x tools/build_npm_pkg.sh`
Expected: no output, exit 0.

- [ ] **Step 5: Add Makefile targets**

In `Makefile`, add `npm-pack npm-publish npm-test` to the `.PHONY` line (first line of the file). Then append this section to the end of the file:

```makefile
# ---------------------------------------------------------------------------
# npm package (@twinkle-lang/twinkle)
# ---------------------------------------------------------------------------

# Stage a self-contained npm package into target/npm/ and build the tarball.
# Depends on a fresh self-hosted payload.
npm-pack: $(STAGE2_WASM) tools/build_npm_pkg.sh tools/npm/package.json tools/npm/README.md $(wildcard tools/js_runtime/*.mjs)
	tools/build_npm_pkg.sh
	cd target/npm && npm pack

# Publish the staged package to npm (requires `npm login` and the
# @twinkle-lang organization to exist).
npm-publish: $(STAGE2_WASM) tools/build_npm_pkg.sh tools/npm/package.json tools/npm/README.md $(wildcard tools/js_runtime/*.mjs)
	tools/build_npm_pkg.sh
	cd target/npm && npm publish

# Run the JS runtime/lib/CLI test suite (needs target/boot.wasm present).
npm-test: $(STAGE2_WASM)
	node --test tools/js_runtime/*.test.mjs
```

- [ ] **Step 6: Stage the package and verify its contents**

Run: `make npm-pack`
Expected: builds the self-host payload (if stale), stages `target/npm/`, and `npm pack` prints a tarball name `twinkle-lang-twinkle-0.1.0.tgz` with a file list including `node.mjs`, `index.mjs`, `runtime.mjs`, `boot.wasm`, `bridge.wasm`, `README.md`, `package.json`.

- [ ] **Step 7: Verify the staged CLI runs from the flat layout**

Run: `node target/npm/node.mjs run examples/fizzbuzz.tw | head -1`
Expected: prints the first FizzBuzz line (confirms `./boot.wasm` + `./bridge.wasm` + `./runtime.mjs` resolve correctly in the packaged flat layout).

- [ ] **Step 8: Commit**

```bash
git add tools/npm/package.json tools/npm/README.md tools/build_npm_pkg.sh Makefile
git commit -m "Add npm packaging for @twinkle-lang/twinkle

Stage runtime + Node entry + lib + boot.wasm + bridge.wasm into a flat,
self-contained target/npm/ package. Add make npm-pack/npm-publish/npm-test."
```

---

## Task 5: Verify the published tarball end-to-end

**Files:** none (verification task; produces no source changes).

- [ ] **Step 1: Pack and install the tarball into a throwaway project**

Run:
```bash
make npm-pack
TARBALL="$(ls -t target/npm/twinkle-lang-twinkle-*.tgz | head -1)"
WORK="$(mktemp -d)"
cd "$WORK" && npm init -y >/dev/null && npm install "$OLDPWD/$TARBALL"
```
Expected: installs `@twinkle-lang/twinkle` into `$WORK/node_modules` with no errors.

- [ ] **Step 2: Verify the bin works from the installed package**

Run (still in `$WORK`):
```bash
printf 'println("hello from installed twk")\n' > hi.tw
npx twk run hi.tw
./node_modules/.bin/twk run hi.tw
```
Expected: both invocations print `hello from installed twk`. The direct
`.bin/twk` call exercises the executable bin shim/mode (catches a missing
executable bit or broken shebang that `npx` might paper over).

- [ ] **Step 3: Verify the lib import works from the installed package**

Run (still in `$WORK`):
```bash
cat > use.mjs <<'EOF'
import { runFile } from "@twinkle-lang/twinkle";
import { writeFileSync } from "node:fs";
writeFileSync("ext.tw", 'extern host_app { fn emit(msg: String) }\nhost_app.emit("wired!")\n');
const code = await runFile("ext.tw", { imports: { host_app: { emit: (m) => console.log("JS got:", m) } } });
process.exit(code);
EOF
node use.mjs
```
Expected: prints `JS got: wired!` and exits 0.

- [ ] **Step 4: Clean up**

Run: `rm -rf "$WORK"` and `cd` back to the repo root.
Expected: no output.

- [ ] **Step 5: Commit (if any incidental fixes were needed)**

If steps 1–3 surfaced a packaging bug and you fixed it in `tools/`, commit the fix:

```bash
git add -A
git commit -m "Fix packaging issue found during end-to-end install verification"
```

If no changes were needed, skip this step.

---

## Task 6: Documentation

**Files:**
- Create: `docs/js-embedding.md`
- Modify: `README.md`

- [ ] **Step 1: Write the embedding guide**

Create `docs/js-embedding.md`:

```markdown
# Embedding Twinkle in JavaScript

`@twinkle-lang/twinkle` ships both the `twk` CLI and an embeddable library for
compiling and running Twinkle programs from Node.js (≥ 22).

## Install

```bash
npm install @twinkle-lang/twinkle
```

## CLI (`twk`)

The npm `twk` is the full self-hosted compiler — every subcommand works:

```bash
npx twk run program.tw            # compile + run
npx twk build program.tw -o out.wasm
npx twk check program.tw          # type-check only
npx twk fmt program.tw            # format in place
npx twk ir program.tw --opt       # print optimized IR
npx twk lsp                       # language server (stdio)
```

Install globally for a bare `twk`:

```bash
npm install -g @twinkle-lang/twinkle
twk run program.tw
```

## Library

The package is ESM-only — use `import`:

```js
import { compile, run, runFile, runSource } from "@twinkle-lang/twinkle";
```

CommonJS consumers can load it through Node's ESM interop (`require()` of ESM is
unflagged on Node ≥ 22.12), but CommonJS is not a primary target — prefer
`import` or a dynamic `await import("@twinkle-lang/twinkle")`.

### `compile(input, opts?) -> Promise<Uint8Array>`

`input` is either a **file path string** or `{ source, path? }` for in-memory
source. Returns the compiled wasm bytes. Throws with the compiler diagnostics on
error.

> **Source-context limitation:** a path argument gets full project support —
> relative imports (`use .sibling`) and walk-up `twinkle.toml` discovery resolve
> from the file's real location. `{ source }` is written to a temporary
> directory and compiled as a single file, so relative imports and project-root
> discovery will not resolve. Use a path for multi-file projects.

### `run(wasmBytes, opts?) -> Promise<number>`

Runs pre-compiled wasm and resolves to the program's exit code. Loads only the
tiny bridge module — no compiler. Options: `imports`, `args`, `cwd`, `env`,
`stdout`, `stderr`, `path`.

### `runFile(path, opts?)` / `runSource(source, opts?)`

Compile-then-run conveniences taking the same `opts` as `run`.

## Wiring extern (host) functions

Declare host functions in Twinkle with `extern`:

```tw
extern canvas {
  fn draw_rect(x: Float, y: Float, w: Float, h: Float)
  fn clear()
}
```

Wire them by passing a **scoped `imports` object** — keyed by extern module
name, then function name:

```js
await runFile("game.tw", {
  imports: {
    canvas: {
      draw_rect: (x, y, w, h) => ctx.fillRect(x, y, w, h),
      clear: () => ctx.clearRect(0, 0, W, H),
    },
  },
});
```

### Auto-wiring of host globals

Extern modules that already exist on `globalThis` — `Math`, `console`,
`crypto`, … — resolve automatically. You only wire what isn't ambient:

```js
await runFile("calc.tw");                       // uses extern Math/console, no imports
await runFile("game.tw", { imports: { canvas } }); // only canvas needs wiring
```

Resolution order per extern import: `imports[module][name]` → `globalThis[module][name]`.
Explicit `imports` therefore **shadow** globals — pass your own `console` to
capture output:

```js
const lines = [];
await runFile("program.tw", { imports: { console: { log: (m) => lines.push(m) } } });
```

### Missing imports

If an extern is satisfied by neither `imports` nor `globalThis`, `run` throws a
single error naming every unsatisfied symbol:

```
Missing host import(s): canvas.draw_rect, canvas.clear
Provide them via the run() "imports" option (e.g. { imports: { canvas: { draw_rect: fn } } }) or define them on globalThis.
```

## Boundary types

Extern parameter/return types are limited to `Int`, `Float`, `Bool`, `String`,
extern handle types, and `Void`. `Int` arrives in JS as a `number` (converted
from i64), `Float` as a `number`, `Bool` as `0`/`1`, `String` as a JS string.
See `docs/spec.md` §7.2 for the full extern rules.
```

- [ ] **Step 2: Add an install section to the top-level README**

In `README.md`, add this section (place it after the project intro / before or after the existing build instructions, matching the surrounding heading style):

```markdown
## Install from npm

Twinkle ships on npm as [`@twinkle-lang/twinkle`](https://www.npmjs.com/package/@twinkle-lang/twinkle),
providing both the `twk` CLI and an embeddable compile/run library:

```bash
npm install -g @twinkle-lang/twinkle   # CLI
npm install @twinkle-lang/twinkle      # library
```

See [docs/js-embedding.md](docs/js-embedding.md) for CLI usage and the
JavaScript embedding/extern-wiring guide.
```

- [ ] **Step 3: Commit**

```bash
git add docs/js-embedding.md README.md
git commit -m "Document npm install, twk CLI, and JS embedding/extern wiring"
```

---

## Final Verification

- [ ] **Step 1: Run the full JS test suite**

Run: `make npm-test`
Expected: all tests in `runtime.test.mjs`, `index.test.mjs`, and `cli.test.mjs` PASS.

- [ ] **Step 2: Confirm the Deno path still self-hosts (no regression)**

Run: `make boot-test`
Expected: the boot compiler test suite passes — confirms the runtime change did not break the Deno-driven self-host flow.

- [ ] **Step 3: Confirm a clean package stage**

Run: `make npm-pack && node target/npm/node.mjs run examples/extern_ffi.tw`
Expected: the staged package's CLI runs the extern_ffi example (console/Math via globalThis fallback), exit 0.

---

## Appendix: Publishing (manual)

Publishing is **manual** in this scope — there is no CI auto-publish. These are
operator steps, run only when cutting a release; they are not part of the
task-by-task implementation.

**One-time setup:**
1. Create the `@twinkle-lang` organization on npmjs.com (Account → Add
   Organization). Free for public packages; reserves the scope.
2. `npm login` locally (or configure a granular/automation token).

**Each release:**
1. Bump `"version"` in `tools/npm/package.json`.
2. `npm whoami` — confirm you are authenticated.
3. `make npm-pack` — build the verified payload and stage `target/npm/`.
4. Inspect the `npm pack` file list (expect `node.mjs`, `index.mjs`,
   `runtime.mjs`, `boot.wasm`, `bridge.wasm`, `README.md`, `package.json`).
5. `cd target/npm && npm publish --dry-run` — verify what would be published.
6. `npm publish` — `publishConfig.access: "public"` (already in the manifest)
   publishes the scoped package publicly.

A future CI publish would authenticate with an automation `NPM_TOKEN` and run
`make npm-publish`; that automation is out of scope here.
```
