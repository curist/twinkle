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
import { nodeHost } from "./node_host.mjs";

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

  // A dedicated temp dir per call: mkdtempSync's random suffix guarantees a
  // unique output path even for concurrent same-process compiles.
  const outDir = mkdtempSync(join(tmpdir(), "twinkle-out-"));
  const outPath = join(outDir, "out.wasm");
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
      host: nodeHost,
    });
    if (code !== 0) {
      throw new Error(`Twinkle compilation failed (exit ${code}):\n${err.text() || out.text()}`);
    }
    return new Uint8Array(readFileSync(outPath));
  } finally {
    try { rmSync(outDir, { recursive: true, force: true }); } catch {}
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
    host: nodeHost,
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
