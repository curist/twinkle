#!/usr/bin/env node
// Node.js / Bun runner for Twinkle-emitted Wasm GC modules.
//
// Usage:
//   node tools/run_wasm_node.mjs <file.wasm> [program args...]
//   node tools/run_wasm_node.mjs <file.wasm> -- [program args...]
//   bun  tools/run_wasm_node.mjs <file.wasm> [program args...]
//   bun  tools/run_wasm_node.mjs <file.wasm> -- [program args...]
//
// The optional `--` is consumed by this runner and not forwarded to the Wasm
// program. This is useful when the guest program itself expects command-like
// args, e.g. running a compiled `boot/main.tw` as:
//
//   node tools/run_wasm_node.mjs out/boot-main.wasm -- build boot/main.tw
//
// Provides the "host" imports that Twinkle's stage0 compiler emits, using a
// small bridge Wasm module to create/read Wasm GC values (since JS cannot
// directly construct or inspect Wasm GC arrays/structs).

import { resolve } from "node:path";
import { runWasmBytes, runWasmFile } from "./wasm_runner_lib.mjs";

export { runWasmBytes, runWasmFile };

function parseCliArgs(argv) {
  if (argv.length === 0) {
    console.error("Usage: node tools/run_wasm_node.mjs <file.wasm> [args...]");
    console.error("   or: node tools/run_wasm_node.mjs <file.wasm> -- [args...]");
    process.exit(1);
  }

  const wasmPath = resolve(argv[0]);
  const sepIndex = argv.indexOf("--", 1);
  const guestArgs = sepIndex >= 0 ? argv.slice(sepIndex + 1) : argv.slice(1);
  return { wasmPath, guestArgs };
}

function main() {
  const { wasmPath, guestArgs } = parseCliArgs(process.argv.slice(2));
  const exitCode = runWasmFile(wasmPath, { guestArgs });
  process.exit(exitCode);
}

try {
  main();
} catch (e) {
  if (e.message?.startsWith("host.error:")) {
    process.exit(1);
  }
  console.error(e.message || e);
  process.exit(1);
}
