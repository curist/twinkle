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
import { nodeHost } from "./node_host.mjs";

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

function loadPackageVersion() {
  if (process.env.TWK_VERSION) return process.env.TWK_VERSION;
  try {
    const pkg = JSON.parse(readFileSync(`${here}/package.json`, "utf8"));
    if (typeof pkg.version === "string") return pkg.version;
  } catch (_) {
    // Not running from the packaged npm layout.
  }
  try {
    const pkg = JSON.parse(readFileSync(`${here}/../npm/package.json`, "utf8"));
    if (typeof pkg.version === "string") return pkg.version;
  } catch (_) {
    // Development fallback failed; let the compiler report "dev".
  }
  return undefined;
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
  const env = { ...process.env };
  const version = loadPackageVersion();
  if (version !== undefined) env.TWK_VERSION = version;

  const exitCode = await runWasmBytesAsync(loadBootWasm(), {
    programPath: bootOverride ? resolve(bootOverride) : "twk.wasm",
    guestArgs: process.argv.slice(2),
    cwd: process.cwd(),
    env,
    stdout: nodeStream(1),
    stderr: nodeStream(2),
    bridgeBytes: loadBridgeWasm(),
    host: nodeHost,
  });
  process.exit(exitCode);
}

main().catch((e) => {
  if (e.message?.startsWith("host.error:")) process.exit(1);
  console.error(e.stack || e.message || e);
  process.exit(1);
});
