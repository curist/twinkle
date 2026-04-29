#!/usr/bin/env bun
// Bun-compiled Twinkle CLI entry.
//
// This file expects ../target/twk_cli_payload.mjs to be generated first:
//   tools/gen_bundled_cli_payload.mjs
//
// Then build a standalone executable with:
//   bun build --compile tools/twk_cli_bundled.mjs --outfile target/twk

import { runWasmBytes } from "./wasm_runner_lib.mjs";
import { BOOT_WASM_BASE64, BRIDGE_WASM_BASE64 } from "../target/twk_cli_payload.mjs";

function decodeBase64(base64) {
  return Uint8Array.from(Buffer.from(base64, "base64"));
}

function guestArgs() {
  const argv1 = process.argv[1] ?? "";
  const hasScriptArg = argv1.endsWith(".mjs") || argv1.endsWith(".js") || argv1.startsWith("/$bunfs/");
  const args = hasScriptArg ? process.argv.slice(2) : process.argv.slice(1);
  if (process.env.TWINKLE_DEBUG_ARGV === "1") {
    console.error(JSON.stringify({ argv: process.argv, guestArgs: args }));
  }
  return args;
}

function main() {
  const bootBytes = decodeBase64(BOOT_WASM_BASE64);
  const bridgeBytes = decodeBase64(BRIDGE_WASM_BASE64);

  const exitCode = runWasmBytes(bootBytes, {
    programPath: "twk.wasm",
    guestArgs: guestArgs(),
    cwd: process.cwd(),
    env: process.env,
    stdout: process.stdout,
    stderr: process.stderr,
    bridgeBytes,
  });
  process.exit(exitCode);
}

main();
