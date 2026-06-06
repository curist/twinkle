// Browser entry for embedding Twinkle.
//
//   import { run } from "@twinkle-lang/twinkle/web";
//   await run(source, { stdout, stderr });
//
// Unlike index.mjs (Node: temp files, node:fs), this is browser-first: it
// self-loads the compiler wasm that ships beside it and uses an in-memory
// filesystem by default, so callers never touch boot.wasm/bridge.wasm or
// construct a host adapter.

import { runWasmBytesAsync, createMemoryHost } from "./runtime.mjs";

const textEncoder = new TextEncoder();

// boot.wasm / bridge.wasm are published next to this module. `new URL(...,
// import.meta.url)` lets the consumer's bundler emit them as assets (Vite,
// webpack, native ESM all understand this), so no `?url` import is needed.
let assetsPromise;

function loadAssets() {
  if (!assetsPromise) {
    assetsPromise = Promise.all([
      fetch(new URL("./boot.wasm", import.meta.url)).then((r) => r.arrayBuffer()),
      fetch(new URL("./bridge.wasm", import.meta.url)).then((r) => r.arrayBuffer()),
    ]).then(([boot, bridge]) => ({
      bootBytes: new Uint8Array(boot),
      bridgeBytes: new Uint8Array(bridge),
    }));
  }
  return assetsPromise;
}

/** Pre-fetch the compiler wasm so the first run() doesn't pay the load latency. */
export function load() {
  return loadAssets();
}

const defaultStream = (sink) => ({
  write(chunk) {
    sink(typeof chunk === "string" ? chunk : new TextDecoder().decode(chunk));
    return true;
  },
});

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
 * @param {Iterable<[string,Uint8Array]>} [opts.files]  Extra in-memory files (multi-file projects).
 * @param {object} [opts.host]     Override the host adapter entirely (advanced).
 * @returns {Promise<number>} exit code
 */
export async function run(source, opts = {}) {
  const { bootBytes, bridgeBytes } = await loadAssets();

  const files = new Map(opts.files ?? []);
  files.set("/input/main.tw", typeof source === "string" ? textEncoder.encode(source) : source);

  return runWasmBytesAsync(bootBytes, {
    programPath: "twk.wasm",
    guestArgs: ["run", "/input/main.tw"],
    cwd: "/",
    env: opts.env ?? {},
    stdout: opts.stdout ?? defaultStream((s) => console.log(s)),
    stderr: opts.stderr ?? defaultStream((s) => console.error(s)),
    bridgeBytes,
    host: opts.host ?? createMemoryHost(files),
    imports: opts.imports ?? {},
  });
}
