// Node SEA entry wrapper for the Twinkle CLI.
//
// This file is bundled by esbuild into a self-contained CJS file for Node SEA
// injection. It provides SEA-specific asset loading and argv handling, then
// delegates to the shared runtime.

import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import sea from "node:sea";
import { runWasmBytesAsync } from "./runtime.mjs";

function assetBytes(key, fallbackPath) {
  if (sea.isSea()) {
    return Buffer.from(sea.getAsset(key));
  }
  return readFileSync(fallbackPath);
}

function defaultRootDir() {
  if (sea.isSea()) {
    return dirname(process.execPath);
  }
  // When bundled to CJS by esbuild, __dirname is the output directory (target/).
  return resolve(__dirname, "..");
}

function loadBootWasm() {
  const fallback = resolve(defaultRootDir(), "target/boot.wasm");
  try {
    return assetBytes("boot.wasm", process.env.BOOT_WASM ? resolve(process.env.BOOT_WASM) : fallback);
  } catch (e) {
    console.error(`Error: boot compiler wasm not found: ${e.message}`);
    console.error("Build the verified self-hosted payload with:");
    console.error("  cargo build --release");
    console.error("  make stage2");
    process.exit(1);
  }
}

function loadBridgeWasm() {
  const fallback = resolve(defaultRootDir(), "tools/bridge.wasm");
  try {
    return assetBytes("bridge.wasm", process.env.BRIDGE_WASM ? resolve(process.env.BRIDGE_WASM) : fallback);
  } catch (e) {
    console.error(`Error: bridge wasm not found: ${e.message}`);
    console.error("Regenerate with: ./target/release/twk run boot/tests/gen_bridge_wasm.tw");
    process.exit(1);
  }
}

function guestArgs() {
  const argv1 = process.argv[1] ?? "";
  // Node SEA preserves the executable path as argv[1], while direct `node
  // tools/twk_cli_sea.cjs ...` preserves the script path there.
  const hasScriptArg = sea.isSea() || argv1.endsWith(".cjs") || argv1.endsWith(".mjs") || argv1.endsWith(".js");
  const args = hasScriptArg ? process.argv.slice(2) : process.argv.slice(1);
  if (process.env.TWINKLE_DEBUG_ARGV === "1") {
    console.error(JSON.stringify({ argv: process.argv, guestArgs: args, isSea: sea.isSea() }));
  }
  return args;
}

async function main() {
  const bridgeBytes = loadBridgeWasm();
  const exitCode = await runWasmBytesAsync(loadBootWasm(), {
    programPath: sea.isSea() ? "twk.wasm" : resolve(defaultRootDir(), "target/boot.wasm"),
    guestArgs: guestArgs(),
    cwd: process.cwd(),
    env: process.env,
    stdout: process.stdout,
    stderr: process.stderr,
    bridgeBytes,
  });
  process.exit(exitCode);
}

main().catch((e) => {
  if (e.message?.startsWith("host.error:")) {
    process.exit(1);
  }
  console.error(e.stack || e.message || e);
  process.exit(1);
});
