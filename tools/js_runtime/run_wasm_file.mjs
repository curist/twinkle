// Minimal Deno runner: execute a stage0-emitted Twinkle Wasm module through the
// project's own JS<->Wasm-GC runtime (the same `runWasmBytesAsync` the boot CLI
// and boot test suite use), rather than shelling out to an external engine.
//
// Usage: deno run --allow-read --allow-env run_wasm_file.mjs <module.wasm>
//
// Exit code mirrors the guest: 0 on clean completion, the guest's `proc.exit`
// code when it exits explicitly, and non-zero when the module traps (e.g. an
// `error(...)` call). Used by tests/tuple_pattern_run_test.rs to assert on the
// actual runtime value a compiled program computes.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { runWasmBytesAsync } from "./runtime.mjs";
import { nodeHost } from "./node_host.mjs";

const textEncoder = new TextEncoder();

function denoStream(stream) {
  return {
    write(chunk) {
      const bytes = typeof chunk === "string"
        ? textEncoder.encode(chunk)
        : new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
      stream.writeSync(bytes);
      return true;
    },
  };
}

const modulePath = Deno.args[0];
if (!modulePath) {
  console.error("usage: run_wasm_file.mjs <module.wasm>");
  Deno.exit(2);
}

const bytes = readFileSync(resolve(modulePath));

try {
  const exitCode = await runWasmBytesAsync(bytes, {
    programPath: resolve(modulePath),
    guestArgs: Deno.args.slice(1),
    cwd: Deno.cwd(),
    env: Deno.env.toObject(),
    stdout: denoStream(Deno.stdout),
    stderr: denoStream(Deno.stderr),
    host: nodeHost,
  });
  Deno.exit(exitCode);
} catch (e) {
  // A guest trap (e.g. `error("mismatch")` or a failed cast) surfaces as a
  // thrown error here; report it and exit non-zero so callers can distinguish
  // "ran to completion" from "trapped".
  console.error(e?.stack || e?.message || String(e));
  Deno.exit(1);
}
