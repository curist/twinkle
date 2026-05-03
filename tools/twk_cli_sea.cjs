#!/usr/bin/env node
// Node.js SEA entry for the Twinkle CLI.
//
// This file is intentionally self-contained: an injected SEA main script cannot
// load project-local modules with the normal file-based require(). The compiler
// and JS↔Wasm bridge are embedded as SEA assets by tools/build_node_sea_cli.sh.

const { readFileSync, writeFileSync, existsSync, readdirSync, mkdirSync, readSync, writeSync } = require("node:fs");
const { resolve, dirname } = require("node:path");
const { fileURLToPath } = require("node:url");
const sea = require("node:sea");

const RESULT_TYPE_ID = 1; // matches src/types/ty.rs RESULT_TYPE_ID
const RESULT_OK = 0;
const RESULT_ERR = 1;

class HostExit extends Error {
  constructor(code) {
    super(`host.exit(${code})`);
    this.name = "HostExit";
    this.code = code;
  }
}

const textDecoder = new TextDecoder();
const textEncoder = new TextEncoder();

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
    console.error("  tools/selfhost_loop.sh boot/main.tw");
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

function decodeString(b, ref) {
  if (!ref) return "";
  const len = b.string_len(ref);
  const bytes = new Uint8Array(len);
  for (let i = 0; i < len; i++) bytes[i] = b.string_get(ref, i);
  return textDecoder.decode(bytes);
}

function encodeString(b, str) {
  const bytes = textEncoder.encode(str);
  const ref = b.string_new(bytes.length);
  for (let i = 0; i < bytes.length; i++) b.string_set(ref, i, bytes[i]);
  return ref;
}

function makeResultOk(b, value) {
  const payload = b.array_new(1);
  b.array_set(payload, 0, value);
  return b.variant_new(RESULT_TYPE_ID, RESULT_OK, payload);
}

function makeResultErr(b, value) {
  const payload = b.array_new(1);
  b.array_set(payload, 0, value);
  return b.variant_new(RESULT_TYPE_ID, RESULT_ERR, payload);
}

function makeStringArray(b, strings) {
  const arr = b.array_new(strings.length);
  for (let i = 0; i < strings.length; i++) {
    b.array_set(arr, i, encodeString(b, strings[i]));
  }
  return arr;
}

function makeByteArray(b, bytes) {
  const arr = b.array_new(bytes.length);
  for (let i = 0; i < bytes.length; i++) {
    b.array_set(arr, i, b.i31_new(bytes[i]));
  }
  return arr;
}

function decodeStringArray(b, arrRef) {
  const len = b.array_len(arrRef);
  const out = new Array(len);
  for (let i = 0; i < len; i++) {
    out[i] = decodeString(b, b.array_get(arrRef, i));
  }
  return out;
}

function decodeByteArray(b, arrRef) {
  const len = b.array_len(arrRef);
  const out = Buffer.alloc(len);
  for (let i = 0; i < len; i++) {
    out[i] = b.i31_get(b.array_get(arrRef, i));
  }
  return out;
}

function verifyImports(module, hostImports) {
  try {
    for (const imp of WebAssembly.Module.imports(module)) {
      const mod = hostImports[imp.module];
      if (!mod || !(imp.name in mod)) {
        throw new Error(`Unsupported host import: ${imp.module}.${imp.name}`);
      }
    }
  } catch (e) {
    if (e.message?.startsWith("Unsupported host import:")) throw e;
    // Module.imports may fail on GC modules in some runtimes.
  }
}

function write(stream, text) {
  stream.write(text);
}

function makeHostImports(b, runtime, bridgeBytes) {
  return {
    host: {
      print: (s) => write(runtime.stdout, decodeString(b, s)),
      println: (s) => write(runtime.stdout, decodeString(b, s) + "\n"),
      error: (s) => {
        const msg = decodeString(b, s);
        write(runtime.stderr, msg + "\n");
        throw new Error("host.error: " + msg);
      },
      eprint: (s) => write(runtime.stderr, decodeString(b, s)),
      eprintln: (s) => write(runtime.stderr, decodeString(b, s) + "\n"),

      f64_to_string: (n) => encodeString(b, n.toString()),

      args: () => makeStringArray(b, runtime.programArgs),
      env: (keyRef) => {
        const key = decodeString(b, keyRef);
        const val = runtime.env[key];
        return val === undefined ? makeStringArray(b, []) : makeStringArray(b, [val]);
      },
      cwd: () => encodeString(b, runtime.cwd),
      exit: (code) => {
        const c = typeof code === "bigint" ? Number(code) : code;
        throw new HostExit(c);
      },
      now: () => performance.now(),
      run_wasm: (bytesRef, argvRef) => {
        const childBytes = decodeByteArray(b, bytesRef);
        const childArgv = decodeStringArray(b, argvRef);
        const [programPath, ...guestArgs] = childArgv;
        const exitCode = runWasmBytes(childBytes, {
          programPath: programPath ?? "<memory>.wasm",
          guestArgs,
          cwd: runtime.cwd,
          env: runtime.env,
          stdout: runtime.stdout,
          stderr: runtime.stderr,
          bridgeBytes,
        });
        return BigInt(exitCode);
      },

      read_file: (pathRef) => {
        const filePath = resolve(runtime.cwd, decodeString(b, pathRef));
        try {
          const bytes = readFileSync(filePath);
          return makeResultOk(b, makeByteArray(b, bytes));
        } catch (e) {
          const msg = `host.read_file failed for '${filePath}': ${e.message}`;
          return makeResultErr(b, encodeString(b, msg));
        }
      },
      write_file: (pathRef, contentRef) => {
        const filePath = resolve(runtime.cwd, decodeString(b, pathRef));
        writeFileSync(filePath, decodeString(b, contentRef));
      },
      write_bytes: (pathRef, bytesRef) => {
        const filePath = resolve(runtime.cwd, decodeString(b, pathRef));
        writeFileSync(filePath, decodeByteArray(b, bytesRef));
      },
      stdin_read_chunk: (maxBytes) => {
        const n = Number(maxBytes);
        if (n <= 0) return makeByteArray(b, []);
        const buf = Buffer.allocUnsafe(n);
        const read = readSync(0, buf, 0, n, null);
        return makeByteArray(b, buf.subarray(0, read));
      },
      stdout_write_bytes: (bytesRef) => {
        const bytes = decodeByteArray(b, bytesRef);
        if (runtime.stdout?.fd !== undefined) {
          writeSync(runtime.stdout.fd, bytes);
        } else {
          runtime.stdout.write(Buffer.from(bytes));
        }
      },
      mkdirp: (pathRef) => {
        const dirPath = resolve(runtime.cwd, decodeString(b, pathRef));
        mkdirSync(dirPath, { recursive: true });
      },
      list_dir: (pathRef) => {
        const dirPath = resolve(runtime.cwd, decodeString(b, pathRef));
        return makeStringArray(b, readdirSync(dirPath));
      },
      exists: (pathRef) => {
        const filePath = resolve(runtime.cwd, decodeString(b, pathRef));
        return existsSync(filePath) ? 1 : 0;
      },

      parse_int: (sRef) => {
        const s = decodeString(b, sRef);
        const n = parseInt(s, 10);
        return isNaN(n) ? 0n : BigInt(n);
      },
      parse_float: (sRef) => {
        const s = decodeString(b, sRef);
        const f = parseFloat(s);
        return isNaN(f) ? [0.0, 0] : [f, 1];
      },
    },
  };
}

function instantiateBridge(bridgeBytes) {
  const bridgeModule = new WebAssembly.Module(bridgeBytes);
  const bridgeInstance = new WebAssembly.Instance(bridgeModule);
  return bridgeInstance.exports;
}

function runWasmBytes(
  wasmBytes,
  {
    programPath = "<memory>.wasm",
    guestArgs = [],
    cwd = process.cwd(),
    env = process.env,
    stdout = process.stdout,
    stderr = process.stderr,
    bridgeBytes = loadBridgeWasm(),
  } = {},
) {
  const b = instantiateBridge(bridgeBytes);
  const runtime = {
    programArgs: [programPath, ...guestArgs],
    cwd,
    env,
    stdout,
    stderr,
  };

  const hostImports = makeHostImports(b, runtime, bridgeBytes);
  const mainModule = new WebAssembly.Module(wasmBytes);
  verifyImports(mainModule, hostImports);

  try {
    new WebAssembly.Instance(mainModule, hostImports);
    return 0;
  } catch (e) {
    if (e instanceof HostExit) {
      return e.code;
    }
    throw e;
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

function main() {
  const bridgeBytes = loadBridgeWasm();
  const exitCode = runWasmBytes(loadBootWasm(), {
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

try {
  main();
} catch (e) {
  if (e.message?.startsWith("host.error:")) {
    process.exit(1);
  }
  console.error(e.stack || e.message || e);
  process.exit(1);
}
