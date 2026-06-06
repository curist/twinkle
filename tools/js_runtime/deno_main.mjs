// Deno-compiled entry wrapper for the Twinkle CLI.
//
// `deno compile` embeds the verified boot compiler and bridge Wasm as data
// files. This wrapper loads those assets, adapts Deno's stdio handles to the
// shared host runtime interface, then delegates to the shared runtime.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { runWasmBytesAsync } from "./runtime.mjs";
import { nodeHost } from "./node_host.mjs";

const textEncoder = new TextEncoder();
const rootDir = resolve(import.meta.dirname, "../..");

function writeAllSync(stream, bytes) {
  let offset = 0;
  while (offset < bytes.byteLength) {
    const written = stream.writeSync(bytes.subarray(offset));
    if (written <= 0) {
      throw new Error("stdout write made no progress");
    }
    offset += written;
  }
}

function denoStream(stream) {
  return {
    write(chunk) {
      const bytes = typeof chunk === "string"
        ? textEncoder.encode(chunk)
        : new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
      writeAllSync(stream, bytes);
      return true;
    },
  };
}

function readFirst(paths) {
  let lastError = undefined;
  for (const path of paths) {
    try {
      return readFileSync(path);
    } catch (e) {
      lastError = e;
    }
  }
  throw lastError ?? new Error("no paths provided");
}

function loadBootWasm() {
  const override = Deno.env.get("BOOT_WASM");
  if (override) return readFileSync(resolve(override));

  try {
    return readFirst([
      // Embedded by tools/build_deno_cli.sh. The .bin suffix prevents Deno from
      // treating the compiler payload as a statically imported Wasm module.
      `${import.meta.dirname}/../../target/deno-assets/boot.wasm.bin`,
      // Direct `deno run` fallback for development.
      `${rootDir}/target/boot.wasm`,
    ]);
  } catch (e) {
    console.error(`Error: boot compiler wasm not found: ${e.message}`);
    console.error("Build the verified self-hosted payload with:");
    console.error("  cargo build --release");
    console.error("  make stage2");
    Deno.exit(1);
  }
}

function loadPackageVersion() {
  const override = Deno.env.get("TWK_VERSION");
  if (override) return override;
  try {
    const pkg = JSON.parse(readFileSync(`${import.meta.dirname}/../../target/deno-assets/package.json`, "utf8"));
    if (typeof pkg.version === "string") return pkg.version;
  } catch (_) {
    // Not running from the Deno standalone asset layout.
  }
  try {
    const pkg = JSON.parse(readFileSync(`${rootDir}/tools/npm/package.json`, "utf8"));
    if (typeof pkg.version === "string") return pkg.version;
  } catch (_) {
    // Development fallback failed; let the compiler report "dev".
  }
  return undefined;
}

function loadBridgeWasm() {
  const override = Deno.env.get("BRIDGE_WASM");
  if (override) return readFileSync(resolve(override));

  try {
    return readFirst([
      // Embedded by tools/build_deno_cli.sh.
      `${import.meta.dirname}/../../target/deno-assets/bridge.wasm.bin`,
      // Direct `deno run` fallback for development.
      `${rootDir}/tools/bridge.wasm`,
    ]);
  } catch (e) {
    console.error(`Error: bridge wasm not found: ${e.message}`);
    console.error("Regenerate with: ./target/release/twk run boot/tests/gen_bridge_wasm.tw");
    Deno.exit(1);
  }
}

async function main() {
  const bootOverride = Deno.env.get("BOOT_WASM");
  const bridgeBytes = loadBridgeWasm();
  const env = Deno.env.toObject();
  const version = loadPackageVersion();
  if (version !== undefined) env.TWK_VERSION = version;

  const exitCode = await runWasmBytesAsync(loadBootWasm(), {
    programPath: bootOverride ? resolve(bootOverride) : "twk.wasm",
    guestArgs: Deno.args,
    cwd: Deno.cwd(),
    env,
    stdout: denoStream(Deno.stdout),
    stderr: denoStream(Deno.stderr),
    bridgeBytes,
    host: nodeHost,
  });
  Deno.exit(exitCode);
}

main().catch((e) => {
  if (e.message?.startsWith("host.error:")) {
    Deno.exit(1);
  }
  console.error(e.stack || e.message || e);
  Deno.exit(1);
});
