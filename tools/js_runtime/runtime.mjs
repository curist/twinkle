// Shared Wasm GC runtime library for the Twinkle Node.js host.
//
// Provides the "host" imports that Twinkle's compiler emits, using a small
// bridge Wasm module to create/read Wasm GC values (since JS cannot directly
// construct or inspect Wasm GC arrays/structs).
//
// Used by:
//   - tools/js_runtime/sea_main.mjs  (Node SEA standalone CLI, bundled via esbuild)

import { readFileSync, writeFileSync, existsSync, readdirSync, mkdirSync, readSync, writeSync } from "node:fs";
import { resolve } from "node:path";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const RESULT_TYPE_ID = 1; // matches src/types/ty.rs RESULT_TYPE_ID
const RESULT_OK = 0;
const RESULT_ERR = 1;

// ---------------------------------------------------------------------------
// HostExit
// ---------------------------------------------------------------------------

export class HostExit extends Error {
  constructor(code) {
    super(`host.exit(${code})`);
    this.name = "HostExit";
    this.code = code;
  }
}

// ---------------------------------------------------------------------------
// String / array marshaling
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Stdin helpers
// ---------------------------------------------------------------------------

function sleepSyncMs(ms) {
  if (ms <= 0) return;
  const sab = new SharedArrayBuffer(4);
  Atomics.wait(new Int32Array(sab), 0, 0, ms);
}

function readStdinTimeout(maxBytes, timeoutMs, runtime) {
  const n = Number(maxBytes);
  const timeout = Number(timeoutMs);
  if (n <= 0) return Buffer.alloc(0);

  // Accessing process.stdin asks Node/libuv to put fd 0 in non-blocking mode
  // for pipes/ttys, which lets fs.readSync report EAGAIN instead of blocking
  // forever when no LSP bytes are currently available.
  void process.stdin;

  const deadline = performance.now() + Math.max(0, timeout);
  const buf = Buffer.allocUnsafe(n);
  while (true) {
    try {
      const read = readSync(0, buf, 0, n, null);
      if (read === 0) runtime.stdinEof = true;
      return buf.subarray(0, read);
    } catch (e) {
      if (e?.code === "EAGAIN" || e?.code === "EWOULDBLOCK") {
        const remaining = deadline - performance.now();
        if (remaining <= 0) return Buffer.alloc(0);
        sleepSyncMs(Math.min(10, remaining));
        continue;
      }
      throw e;
    }
  }
}

// ---------------------------------------------------------------------------
// Extern auto-bridging
// ---------------------------------------------------------------------------

/**
 * Auto-bridge extern imports by resolving `globalThis[module][name]` and
 * wrapping with type conversions for Twinkle's extern-safe types:
 *   - String params (GC refs) are decoded via bridge
 *   - String returns from JS are encoded via bridge
 *   - Int (bigint), Float (number), Bool (i32) pass through
 */
function autoBridgeExternImports(wasmModule, hostImports, b) {
  try {
    for (const imp of WebAssembly.Module.imports(wasmModule)) {
      // Already provided (host module, etc.) — skip
      if (hostImports[imp.module]?.[imp.name] !== undefined) continue;
      if (imp.kind !== "function") continue;

      // Try globalThis[module][name]
      const mod = globalThis[imp.module];
      const fn = mod?.[imp.name];
      if (typeof fn !== "function") {
        throw new Error(`Unsupported host import: ${imp.module}.${imp.name} (not found on globalThis.${imp.module})`);
      }

      // Create a bridged wrapper that auto-converts args and return values
      const bridgedFn = (...args) => {
        const jsArgs = args.map((arg) => {
          if (typeof arg === "bigint") return Number(arg);
          if (typeof arg === "number") return arg;
          // GC ref — assume string
          return decodeString(b, arg);
        });
        const result = fn.apply(mod, jsArgs);
        if (result === undefined || result === null) return;
        if (typeof result === "string") return encodeString(b, result);
        if (typeof result === "number") return result;
        if (typeof result === "bigint") return result;
        return result;
      };

      if (!hostImports[imp.module]) hostImports[imp.module] = {};
      hostImports[imp.module][imp.name] = bridgedFn;
    }
  } catch (e) {
    if (e.message?.startsWith("Unsupported host import:")) throw e;
    // Module.imports may fail on GC modules in some runtimes.
  }
}

// ---------------------------------------------------------------------------
// Host imports
// ---------------------------------------------------------------------------

function write(stream, text) {
  stream.write(text);
}

function makeHostImports(b, runtime, bridgeBytes) {
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
      stdin_read_chunk: (maxBytes) => makeByteArray(b, readStdinTimeout(maxBytes, 2147483647, runtime)),
      stdin_read_timeout: (maxBytes, timeoutMs) => makeByteArray(b, readStdinTimeout(maxBytes, timeoutMs, runtime)),
      stdin_eof: () => runtime.stdinEof ? 1 : 0,
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

// ---------------------------------------------------------------------------
// Bridge instantiation
// ---------------------------------------------------------------------------

function instantiateBridge(bridgeBytes) {
  const bridgeModule = new WebAssembly.Module(bridgeBytes);
  const bridgeInstance = new WebAssembly.Instance(bridgeModule);
  return bridgeInstance.exports;
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export function runWasmBytes(
  wasmBytes,
  {
    programPath = "<memory>.wasm",
    guestArgs = [],
    cwd = process.cwd(),
    env = process.env,
    stdout = process.stdout,
    stderr = process.stderr,
    bridgeBytes,
  } = {},
) {
  if (!bridgeBytes) {
    throw new Error("runWasmBytes: bridgeBytes is required");
  }

  const b = instantiateBridge(bridgeBytes);
  const runtime = {
    programArgs: [programPath, ...guestArgs],
    cwd,
    env,
    stdout,
    stderr,
    stdinEof: false,
  };

  const hostImports = makeHostImports(b, runtime, bridgeBytes);
  const mainModule = new WebAssembly.Module(wasmBytes);
  autoBridgeExternImports(mainModule, hostImports, b);

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
    bridgeBytes,
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
