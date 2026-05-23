// Shared Wasm GC runtime library for the Twinkle JavaScript host.
//
// Provides the "host" imports that Twinkle's compiler emits, using a small
// bridge Wasm module to create/read Wasm GC values (since JS cannot directly
// construct or inspect Wasm GC arrays/structs).
//
// Used by:
//   - tools/js_runtime/deno_main.mjs  (Deno standalone CLI)

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

const PAGE_SIZE = 65536;

// Ensure the bridge's linear memory can hold at least `needed` bytes.
// Returns a Uint8Array view of the memory (may be invalidated by future grows).
function ensureMemory(b, needed) {
  const buf = b.memory.buffer;
  if (buf.byteLength >= needed) return new Uint8Array(buf);
  const pages = Math.ceil((needed - buf.byteLength) / PAGE_SIZE);
  b.memory.grow(pages);
  return new Uint8Array(b.memory.buffer);
}

function decodeString(b, ref) {
  if (!ref) return "";
  const len = b.string_len(ref);
  if (len === 0) return "";
  ensureMemory(b, len);
  b.bulk_string_read(ref);
  const view = new Uint8Array(b.memory.buffer, 0, len);
  return textDecoder.decode(view);
}

function encodeString(b, str) {
  const bytes = textEncoder.encode(str);
  if (bytes.length === 0) return b.string_new(0);
  const mem = ensureMemory(b, bytes.length);
  mem.set(bytes);
  return b.bulk_string_new(bytes.length);
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
  if (bytes.length === 0) return b.array_new(0);
  const mem = ensureMemory(b, bytes.length);
  mem.set(bytes);
  return b.bulk_bytes_new(bytes.length);
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
  if (len === 0) return Buffer.alloc(0);
  ensureMemory(b, len);
  b.bulk_bytes_read(arrRef);
  // ArrayBuffer.slice copies (unlike Buffer.slice which creates a view)
  return Buffer.from(b.memory.buffer.slice(0, len));
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

function readStdinTimeoutAsync(maxBytes, timeoutMs, runtime) {
  const n = Number(maxBytes);
  const timeout = Number(timeoutMs);
  if (n <= 0) return Promise.resolve(Buffer.alloc(0));

  return new Promise((resolve) => {
    let timer = null;

    let settled = false;

    const finish = (chunk) => {
      if (settled) return;
      settled = true;
      if (timer !== null) { clearTimeout(timer); timer = null; }
      process.stdin.removeListener("readable", onReadable);
      process.stdin.removeListener("end", onEnd);
      resolve(chunk);
    };

    const tryRead = () => {
      // read() with no argument returns whatever is buffered (1..any bytes),
      // matching the sync path's "read up to n" semantics.
      const chunk = process.stdin.read();
      if (chunk !== null) {
        // If the stream returned more than n bytes, push the excess back.
        if (chunk.length > n) {
          process.stdin.unshift(chunk.subarray(n));
          finish(chunk.subarray(0, n));
        } else {
          finish(chunk);
        }
        return true;
      }
      return false;
    };

    const onReadable = () => { tryRead(); };

    const onEnd = () => {
      runtime.stdinEof = true;
      finish(Buffer.alloc(0));
    };

    // Try immediate read from the stream buffer
    if (tryRead()) return;

    if (process.stdin.readableEnded) {
      runtime.stdinEof = true;
      resolve(Buffer.alloc(0));
      return;
    }

    timer = setTimeout(() => finish(Buffer.alloc(0)), Math.max(0, timeout));

    process.stdin.once("readable", onReadable);
    process.stdin.once("end", onEnd);
  });
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
function autoBridgeExternImports(wasmModule, hostImports, b, jspi = false) {
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

      const marshalArgs = (args) => args.map((arg) => {
        if (typeof arg === "bigint") return Number(arg);
        if (typeof arg === "number") return arg;
        // GC ref — assume string
        return decodeString(b, arg);
      });

      const marshalReturn = (result) => {
        if (result === undefined || result === null) return;
        if (typeof result === "string") return encodeString(b, result);
        if (typeof result === "number") return result;
        if (typeof result === "bigint") return result;
        return result;
      };

      let bridgedFn;
      if (jspi) {
        // JSPI mode: async wrapper so Promise-returning JS functions suspend
        // Wasm. Non-Promise returns pass through without suspension.
        const asyncWrapper = async (...args) => {
          const result = await fn.apply(mod, marshalArgs(args));
          return marshalReturn(result);
        };
        bridgedFn = new WebAssembly.Suspending(asyncWrapper);
      } else {
        // Sync mode: detect and reject Promise returns
        bridgedFn = (...args) => {
          const result = fn.apply(mod, marshalArgs(args));
          if (result instanceof Promise) {
            throw new Error(
              `Extern ${imp.module}.${imp.name} returned a Promise, but JSPI is not available. ` +
              `Promise-returning externs require a runtime with WebAssembly.Suspending/promising support.`,
            );
          }
          return marshalReturn(result);
        };
      }

      if (!hostImports[imp.module]) hostImports[imp.module] = {};
      hostImports[imp.module][imp.name] = bridgedFn;
    }
  } catch (e) {
    if (e.message?.startsWith("Unsupported host import:") || e.message?.startsWith("Extern ")) throw e;
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
// JSPI feature detection
// ---------------------------------------------------------------------------

export const hasJspi =
  typeof WebAssembly.Suspending === "function" &&
  typeof WebAssembly.promising === "function";

// ---------------------------------------------------------------------------
// Wasm preparation (shared by sync and async paths)
// ---------------------------------------------------------------------------

function prepareWasm(wasmBytes, opts, { jspi = false } = {}) {
  const {
    programPath = "<memory>.wasm",
    guestArgs = [],
    cwd = process.cwd(),
    env = process.env,
    stdout = process.stdout,
    stderr = process.stderr,
    bridgeBytes,
  } = opts;

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
  autoBridgeExternImports(mainModule, hostImports, b, jspi);

  return { mainModule, hostImports, b, runtime };
}

// ---------------------------------------------------------------------------
// Public API — synchronous
// ---------------------------------------------------------------------------

export function runWasmBytes(wasmBytes, opts = {}) {
  const { mainModule, hostImports } = prepareWasm(wasmBytes, opts);
  try {
    const instance = new WebAssembly.Instance(mainModule, hostImports);
    // Boot-compiled modules export __twinkle_start instead of using a Wasm
    // start section. Stage0-compiled modules still use the start section and
    // run during instantiation above.
    if (instance.exports.__twinkle_start) {
      instance.exports.__twinkle_start();
    }
    return 0;
  } catch (e) {
    if (e instanceof HostExit) {
      return e.code;
    }
    throw e;
  }
}

export function runWasmFile(wasmPath, opts = {}) {
  return runWasmBytes(readFileSync(wasmPath), {
    programPath: resolve(wasmPath),
    ...opts,
  });
}

// ---------------------------------------------------------------------------
// Public API — async (JSPI-aware)
// ---------------------------------------------------------------------------

export async function runWasmBytesAsync(wasmBytes, opts = {}) {
  const { mainModule, hostImports, b, runtime } = prepareWasm(wasmBytes, opts, { jspi: hasJspi });

  if (hasJspi) {
    // Phase 3: wrap stdin_read_timeout as a suspending import so the Node
    // event loop stays free while Twinkle waits for stdin data or a timeout.
    hostImports.host.stdin_read_timeout = new WebAssembly.Suspending(
      async (maxBytes, timeoutMs) =>
        makeByteArray(b, await readStdinTimeoutAsync(maxBytes, timeoutMs, runtime)),
    );

    // Phase 4: wrap run_wasm as a suspending import so child programs can
    // themselves use JSPI suspending imports.
    const childBridgeBytes = opts.bridgeBytes;
    hostImports.host.run_wasm = new WebAssembly.Suspending(
      async (bytesRef, argvRef) => {
        const childBytes = decodeByteArray(b, bytesRef);
        const childArgv = decodeStringArray(b, argvRef);
        const [programPath, ...guestArgs] = childArgv;
        const exitCode = await runWasmBytesAsync(childBytes, {
          programPath: programPath ?? "<memory>.wasm",
          guestArgs,
          cwd: runtime.cwd,
          env: runtime.env,
          stdout: runtime.stdout,
          stderr: runtime.stderr,
          bridgeBytes: childBridgeBytes,
        });
        return BigInt(exitCode);
      },
    );
  }

  try {
    const instance = new WebAssembly.Instance(mainModule, hostImports);
    if (instance.exports.__twinkle_start) {
      if (hasJspi) {
        const start = WebAssembly.promising(instance.exports.__twinkle_start);
        await start();
      } else {
        instance.exports.__twinkle_start();
      }
    }
    return 0;
  } catch (e) {
    if (e instanceof HostExit) {
      return e.code;
    }
    throw e;
  }
}

export async function runWasmFileAsync(wasmPath, opts = {}) {
  return runWasmBytesAsync(readFileSync(wasmPath), {
    programPath: resolve(wasmPath),
    ...opts,
  });
}
