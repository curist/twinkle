#!/usr/bin/env node
// Thin Node.js wrapper that runs the self-hosted boot compiler like `twk`.
//
// Usage:
//   node tools/twk_boot.mjs <twk args...>
//
// Environment:
//   BOOT_WASM=/path/to/boot.wasm   Override the compiler Wasm payload.
//
// Default compiler payload:
//   target/boot.wasm
//
// That payload is expected to be the verified stage2 compiler produced by:
//   tools/selfhost_loop.sh boot/main.tw

import { existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { runWasmFile } from "./wasm_runner_lib.mjs";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const ROOT_DIR = resolve(__dirname, "..");
const DEFAULT_BOOT_WASM = resolve(ROOT_DIR, "target/boot.wasm");
const BOOT_ENTRY = resolve(ROOT_DIR, "boot/main.tw");

function resolveBootWasm() {
  return resolve(process.env.BOOT_WASM || DEFAULT_BOOT_WASM);
}

function ensureBootWasm(bootWasm) {
  if (existsSync(bootWasm)) {
    return;
  }

  console.error(`Error: boot compiler wasm not found at ${bootWasm}`);
  console.error("Build the verified self-hosted payload with:");
  console.error(`  cargo build --release`);
  console.error(`  tools/selfhost_loop.sh ${BOOT_ENTRY}`);
  process.exit(1);
}

function main() {
  const twkArgs = process.argv.slice(2);

  const bootWasm = resolveBootWasm();
  ensureBootWasm(bootWasm);

  const exitCode = runWasmFile(bootWasm, {
    guestArgs: twkArgs,
    cwd: process.cwd(),
    env: process.env,
    stdout: process.stdout,
    stderr: process.stderr,
  });
  process.exit(exitCode);
}

main();
