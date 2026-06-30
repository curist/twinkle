// Browser entry for embedding Twinkle.
//
//   import { command, run } from "@twinkle-lang/twinkle/web";
//   await command(["fmt", "/input/main.tw"], { source });
//   await run(source, { stdout, stderr });
//
// Unlike index.mjs (Node: temp files, node:fs), this is browser-first: it
// self-loads the compiler wasm that ships beside it and uses an in-memory
// filesystem by default, so callers never touch boot.wasm or construct a host
// adapter. The JS<->Wasm-GC bridge is embedded in runtime.mjs.

import { loadLibBytes, runWasmBytesAsync, createMemoryHost } from "./runtime.mjs";

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

// boot.wasm is published next to this module. `new URL(..., import.meta.url)`
// lets the consumer's bundler emit it as an asset (Vite, webpack, native ESM
// all understand this), so no `?url` import is needed. The bridge ships
// embedded in runtime.mjs, so only boot.wasm is fetched here.
let assetsPromise;

function loadAssets() {
  if (!assetsPromise) {
    assetsPromise = fetch(new URL("./boot.wasm", import.meta.url))
      .then((r) => r.arrayBuffer())
      .then((boot) => ({ bootBytes: new Uint8Array(boot) }));
  }
  return assetsPromise;
}

/** Pre-fetch the compiler wasm so the first run() doesn't pay the load latency. */
export function load() {
  return loadAssets();
}

const defaultStream = (sink) => ({
  write(chunk) {
    sink(typeof chunk === "string" ? chunk : textDecoder.decode(chunk));
    return true;
  },
});

function assertPath(path, label) {
  if (typeof path !== "string" || path.length === 0) {
    throw new TypeError(`${label} must be a non-empty string`);
  }
}

function bytesFor(value, label) {
  if (typeof value === "string") return textEncoder.encode(value);
  if (value instanceof Uint8Array) return value;
  throw new TypeError(`${label} must be a string or Uint8Array`);
}

function createCaptureStream(forward) {
  const decoder = new TextDecoder();
  let text = "";

  return {
    stream: {
      write(chunk) {
        if (typeof chunk === "string") {
          text += decoder.decode();
          text += chunk;
        } else {
          text += decoder.decode(chunk, { stream: true });
        }
        if (forward) forward.write(chunk);
        return true;
      },
    },
    finish() {
      text += decoder.decode();
      return text;
    },
  };
}

function seedFile(host, cwd, path, value, label) {
  assertPath(path, label);
  if (typeof host.resolvePath !== "function" || typeof host.writeBytes !== "function") {
    throw new TypeError("command: host must provide resolvePath(cwd, path) and writeBytes(path, bytes)");
  }
  host.writeBytes(host.resolvePath(cwd, path), bytesFor(value, label));
}

function normalizeArgs(args) {
  if (!Array.isArray(args) || !args.every((arg) => typeof arg === "string")) {
    throw new TypeError("command: args must be an array of strings");
  }
  return args.slice();
}

function commandResult(exitCode, stdout, stderr, host, cwd) {
  const files = host.files instanceof Map ? host.files : new Map();
  return {
    exitCode,
    stdout,
    stderr,
    files,
    text(path) {
      const data = this.bytes(path);
      return data === undefined ? undefined : textDecoder.decode(data);
    },
    bytes(path) {
      assertPath(path, "path");
      const normalized = typeof host.resolvePath === "function" ? host.resolvePath(cwd, path) : path;
      return files.get(normalized);
    },
  };
}

/**
 * Run a compiler command in the browser against an in-memory project.
 *
 * @param {string[]} args CLI arguments, e.g. `["fmt", "/input/main.tw"]`.
 * @param {object} [opts]
 * @param {string | Uint8Array} [opts.source]  Entry source written to opts.path.
 * @param {string} [opts.path]     Path for opts.source; defaults to /input/main.tw.
 * @param {Iterable<[string,string | Uint8Array]>} [opts.files]  Files to seed first.
 * @param {string} [opts.cwd]      Working directory; defaults to /.
 * @param {object} [opts.stdout]   Optional stream with write(chunk), tee'd with capture.
 * @param {object} [opts.stderr]   Optional stream with write(chunk), tee'd with capture.
 * @param {object} [opts.env]      Environment map exposed to the compiler/program.
 * @param {object} [opts.imports]  Extern imports for commands that execute user Wasm.
 * @param {object} [opts.host]     Override the host adapter entirely (advanced).
 * @returns {Promise<{exitCode:number, stdout:string, stderr:string, files:Map<string,Uint8Array>, text:function, bytes:function}>}
 */
export async function command(args, opts = {}) {
  const guestArgs = normalizeArgs(args);
  const cwd = opts.cwd ?? "/";
  assertPath(cwd, "cwd");

  const { bootBytes } = await loadAssets();
  const host = opts.host ?? createMemoryHost();

  if (opts.files !== undefined) {
    for (const entry of opts.files) {
      if (!Array.isArray(entry) || entry.length !== 2) {
        throw new TypeError("command: files must be an iterable of [path, contents] pairs");
      }
      seedFile(host, cwd, entry[0], entry[1], "file path");
    }
  }

  if (opts.source !== undefined) {
    seedFile(host, cwd, opts.path ?? "/input/main.tw", opts.source, "path");
  }

  const stdoutCapture = createCaptureStream(opts.stdout);
  const stderrCapture = createCaptureStream(opts.stderr);
  const exitCode = await runWasmBytesAsync(bootBytes, {
    programPath: "twk.wasm",
    guestArgs,
    cwd,
    env: opts.env ?? {},
    stdout: stdoutCapture.stream,
    stderr: stderrCapture.stream,
    host,
    imports: opts.imports ?? {},
  });

  return commandResult(exitCode, stdoutCapture.finish(), stderrCapture.finish(), host, cwd);
}

/**
 * Compile and run Twinkle source in the browser.
 *
 * @param {string | Uint8Array} source  Twinkle source for the entry module.
 * @param {object} [opts]
 * @param {object} [opts.stdout]   Stream with write(chunk); defaults to console.log.
 * @param {object} [opts.stderr]   Stream with write(chunk); defaults to console.error.
 * @param {object} [opts.env]      Environment map exposed to the program.
 * @param {object} [opts.imports]  Extern imports — `module → fn | { fn?, args? }`
 *   (else resolved via globalThis). `args` is the per-arg spec, e.g.
 *   `{ canvas: { fill_rect: { fn, args: ['raw','raw','raw','raw','raw'] } } }`.
 * @param {Iterable<[string,string | Uint8Array]>} [opts.files]  Extra in-memory files (multi-file projects).
 * @param {string} [opts.path]     Entry path; defaults to /input/main.tw.
 * @param {object} [opts.host]     Override the host adapter entirely (advanced).
 * @returns {Promise<number>} exit code
 */
async function libBytes(input) {
  if (input instanceof Uint8Array) return input;
  if (input instanceof ArrayBuffer) return new Uint8Array(input);
  if (typeof input === "string" || input instanceof URL) {
    const response = await fetch(input);
    if (!response.ok) throw new Error(`failed to fetch ${input}: ${response.status}`);
    return new Uint8Array(await response.arrayBuffer());
  }
  throw new TypeError("wasm must be a URL, path string, Uint8Array, or ArrayBuffer");
}

export async function loadLib(wasm, opts = {}) {
  const wasmBytes = await libBytes(wasm);
  return loadLibBytes(wasmBytes, {
    programPath: opts.path ?? "<library>.wasm",
    guestArgs: [],
    cwd: opts.cwd ?? "/",
    env: opts.env ?? {},
    stdout: opts.stdout ?? defaultStream((s) => console.log(s)),
    stderr: opts.stderr ?? defaultStream((s) => console.error(s)),
    host: opts.host ?? createMemoryHost(),
    imports: opts.imports ?? {},
  });
}

export async function run(source, opts = {}) {
  const path = opts.path ?? "/input/main.tw";
  const result = await command(["run", path], {
    ...opts,
    source,
    path,
    stdout: opts.stdout ?? defaultStream((s) => console.log(s)),
    stderr: opts.stderr ?? defaultStream((s) => console.error(s)),
  });
  return result.exitCode;
}
