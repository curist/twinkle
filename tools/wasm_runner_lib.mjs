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

import { readFileSync, writeFileSync, existsSync, readdirSync, mkdirSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

// ---------------------------------------------------------------------------
// Bridge module
// ---------------------------------------------------------------------------

const BRIDGE_WASM_PATH = resolve(__dirname, "bridge.wasm");
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

function loadBridgeWasm() {
  try {
    return readFileSync(BRIDGE_WASM_PATH);
  } catch {
    console.error(`Error: missing bridge wasm at ${BRIDGE_WASM_PATH}`);
    console.error("Regenerate with: cargo run --release -- run boot/tests/gen_bridge_wasm.tw");
    process.exit(1);
  }
}

const textDecoder = new TextDecoder();
const textEncoder = new TextEncoder();

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

function makeHostImports(b, runtime) {
  return {
    host: {
      // --- I/O ---
      print: (s) => write(runtime.stdout, decodeString(b, s)),
      println: (s) => write(runtime.stdout, decodeString(b, s) + "\n"),
      error: (s) => {
        const msg = decodeString(b, s);
        write(runtime.stderr, msg + "\n");
        throw new Error("host.error: " + msg);
      },
      eprint: (s) => write(runtime.stderr, decodeString(b, s)),
      eprintln: (s) => write(runtime.stderr, decodeString(b, s) + "\n"),

      // --- String conversion ---
      f64_to_string: (n) => encodeString(b, n.toString()),

      // --- Process ---
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

      // --- File system ---
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

      // --- Parsing ---
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

function instantiateBridge(bridgeBytes = loadBridgeWasm()) {
  const bridgeModule = new WebAssembly.Module(bridgeBytes);
  const bridgeInstance = new WebAssembly.Instance(bridgeModule);
  return bridgeInstance.exports;
}

export function runWasmBytes(
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

  const hostImports = makeHostImports(b, runtime);
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

export function runWasmFile(
  wasmPath,
  {
    guestArgs = [],
    cwd = process.cwd(),
    env = process.env,
    stdout = process.stdout,
    stderr = process.stderr,
    bridgeBytes = loadBridgeWasm(),
  } = {},
) {
  return runWasmBytes(readFileSync(wasmPath), {
    programPath: resolve(wasmPath),
    guestArgs,
    cwd,
    env,
    stdout,
    stderr,
    bridgeBytes,
  });
}
