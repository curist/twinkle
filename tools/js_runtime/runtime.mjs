// Shared Wasm GC runtime library for the Twinkle JavaScript host.
//
// Provides the "host" imports that Twinkle's compiler emits, using a small
// bridge Wasm module to create/read Wasm GC values (since JS cannot directly
// construct or inspect Wasm GC arrays/structs).
//
// Host-agnostic: all filesystem and stdin host imports are routed through an
// injected `host` adapter (see tools/js_runtime/node_host.mjs for Node/Deno,
// or the playground's browser adapter). This module contains no `node:`
// imports, so it loads unchanged in a browser/worker.
//
// Used by:
//   - tools/js_runtime/deno_main.mjs  (Deno standalone CLI, host: nodeHost)
//   - tools/js_runtime/node_main.mjs  (Node CLI, host: nodeHost)
//   - tools/js_runtime/index.mjs      (embeddable library, host: nodeHost)

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
  if (len === 0) return new Uint8Array(0);
  ensureMemory(b, len);
  b.bulk_bytes_read(arrRef);
  // ArrayBuffer.slice copies, so the returned view owns its bytes.
  return new Uint8Array(b.memory.buffer.slice(0, len));
}

// ---------------------------------------------------------------------------
// Extern auto-bridging
// ---------------------------------------------------------------------------

/**
 * Resolve each wasm extern import to a JS function.
 * Resolution order per import: scoped `imports[module][name]`, then
 * `globals[module][name]`. Imports already satisfied by `hostImports`, or that
 * are not functions, are skipped. Returns the resolved bindings plus a list of
 * unresolved "module.name" strings.
 */
export function resolveExternImports(importList, hostImports, imports = {}, globals = globalThis) {
  const found = [];
  const missing = [];
  for (const imp of importList) {
    if (hostImports[imp.module]?.[imp.name] !== undefined) continue;
    if (imp.kind !== "function") continue;

    const scopedRecv = imports[imp.module];
    const scoped = scopedRecv?.[imp.name];
    if (typeof scoped === "function") {
      found.push({ module: imp.module, name: imp.name, fn: scoped, recv: scopedRecv });
      continue;
    }

    const globalRecv = globals[imp.module];
    const global = globalRecv?.[imp.name];
    if (typeof global === "function") {
      found.push({ module: imp.module, name: imp.name, fn: global, recv: globalRecv });
      continue;
    }

    missing.push(`${imp.module}.${imp.name}`);
  }
  return { found, missing };
}

/**
 * Auto-bridge extern imports by resolving each to `imports[module][name]` (a
 * scoped per-run object) or `globalThis[module][name]`, wrapping with type
 * conversions for Twinkle's extern-safe types:
 *   - String params (GC refs) are decoded via bridge
 *   - String returns from JS are encoded via bridge
 *   - Int (bigint), Float (number), Bool (i32) pass through
 * Throws a single aggregated error if any extern import is unsatisfied.
 */
function autoBridgeExternImports(wasmModule, hostImports, b, jspi = false, imports = {}, marshalSpec = {}) {
  let importList;
  try {
    importList = WebAssembly.Module.imports(wasmModule);
  } catch {
    // Module.imports may fail on GC modules in some runtimes; nothing to bridge.
    return;
  }

  const { found, missing } = resolveExternImports(importList, hostImports, imports);

  if (missing.length > 0) {
    const [m0, f0] = missing[0].split(".");
    throw new Error(
      `Missing host import(s): ${missing.join(", ")}\n` +
      `Provide them via the run() "imports" option ` +
      `(e.g. { imports: { ${m0}: { ${f0}: fn } } }) or define them on globalThis.`,
    );
  }

  // Per-import arg marshaling honors an optional spec: `marshalSpec[module][name]`
  // is an array of `"raw" | "string"` keyed by arg position. `"raw"` passes the
  // value through untouched — essential for externref args (e.g. a canvas 2D
  // context), since calling decodeString on an opaque host object recurses until
  // a stack overflow in some engines (notably Safari). Without a spec entry, a
  // non-numeric arg is assumed to be a Wasm GC string and decoded.
  const makeMarshalArgs = (spec) => (args) => args.map((arg, i) => {
    if (typeof arg === "bigint") return Number(arg);
    if (typeof arg === "number") return arg;
    if (spec?.[i] === "raw") return arg;
    return decodeString(b, arg);
  });

  const marshalReturn = (result) => {
    if (result === undefined || result === null) return;
    if (typeof result === "string") return encodeString(b, result);
    if (typeof result === "number") return result;
    if (typeof result === "bigint") return result;
    return result;
  };

  for (const { module, name, fn, recv } of found) {
    const marshalArgs = makeMarshalArgs(marshalSpec[module]?.[name]);
    let bridgedFn;
    if (jspi) {
      // JSPI mode: async wrapper so Promise-returning JS functions suspend
      // Wasm. Non-Promise returns pass through without suspension.
      const asyncWrapper = async (...args) =>
        marshalReturn(await fn.apply(recv, marshalArgs(args)));
      bridgedFn = new WebAssembly.Suspending(asyncWrapper);
    } else {
      // Sync mode: detect and reject Promise returns
      bridgedFn = (...args) => {
        const result = fn.apply(recv, marshalArgs(args));
        if (result instanceof Promise) {
          throw new Error(
            `Extern ${module}.${name} returned a Promise, but JSPI is not available. ` +
            `Promise-returning externs require a runtime with WebAssembly.Suspending/promising support.`,
          );
        }
        return marshalReturn(result);
      };
    }
    if (!hostImports[module]) hostImports[module] = {};
    hostImports[module][name] = bridgedFn;
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
          imports: runtime.imports,
          marshalSpec: runtime.marshalSpec,
          host: runtime.host,
          bridgeBytes,
        });
        return BigInt(exitCode);
      },

      // --- File system (routed through the injected host adapter) ---
      read_file: (pathRef) => {
        const filePath = runtime.host.resolvePath(runtime.cwd, decodeString(b, pathRef));
        try {
          const bytes = runtime.host.readFile(filePath);
          return makeResultOk(b, makeByteArray(b, bytes));
        } catch (e) {
          const msg = `host.read_file failed for '${filePath}': ${e.message}`;
          return makeResultErr(b, encodeString(b, msg));
        }
      },
      write_file: (pathRef, contentRef) => {
        const filePath = runtime.host.resolvePath(runtime.cwd, decodeString(b, pathRef));
        runtime.host.writeFile(filePath, decodeString(b, contentRef));
      },
      write_bytes: (pathRef, bytesRef) => {
        const filePath = runtime.host.resolvePath(runtime.cwd, decodeString(b, pathRef));
        runtime.host.writeBytes(filePath, decodeByteArray(b, bytesRef));
      },
      stdin_read_chunk: (maxBytes) => makeByteArray(b, runtime.host.readStdin(maxBytes, 2147483647, runtime)),
      stdin_read_timeout: (maxBytes, timeoutMs) => makeByteArray(b, runtime.host.readStdin(maxBytes, timeoutMs, runtime)),
      stdin_eof: () => runtime.stdinEof ? 1 : 0,
      stdout_write_bytes: (bytesRef) => {
        // Streams accept a Uint8Array chunk; each adapter's write() handles the
        // platform write (fd write on Node, writeSync on Deno, postMessage in a
        // worker), so no Buffer/fd handling is needed here.
        runtime.stdout.write(decodeByteArray(b, bytesRef));
      },
      mkdirp: (pathRef) => {
        const dirPath = runtime.host.resolvePath(runtime.cwd, decodeString(b, pathRef));
        runtime.host.mkdirp(dirPath);
      },
      list_dir: (pathRef) => {
        const dirPath = runtime.host.resolvePath(runtime.cwd, decodeString(b, pathRef));
        return makeStringArray(b, runtime.host.listDir(dirPath));
      },
      exists: (pathRef) => {
        const filePath = runtime.host.resolvePath(runtime.cwd, decodeString(b, pathRef));
        return runtime.host.exists(filePath) ? 1 : 0;
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
    cwd = "/",
    env = {},
    stdout,
    stderr,
    bridgeBytes,
    host,
    imports = {},
    marshalSpec = {},
  } = opts;

  if (!bridgeBytes) {
    throw new Error("runWasmBytes: bridgeBytes is required");
  }
  if (!host) {
    throw new Error("runWasmBytes: host adapter is required (see node_host.mjs)");
  }

  const b = instantiateBridge(bridgeBytes);
  const runtime = {
    programArgs: [programPath, ...guestArgs],
    cwd,
    env,
    stdout,
    stderr,
    stdinEof: false,
    host,
    imports,
    marshalSpec,
  };

  const hostImports = makeHostImports(b, runtime, bridgeBytes);
  const mainModule = new WebAssembly.Module(wasmBytes);
  autoBridgeExternImports(mainModule, hostImports, b, jspi, imports, marshalSpec);

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

// ---------------------------------------------------------------------------
// Public API — async (JSPI-aware)
// ---------------------------------------------------------------------------

export async function runWasmBytesAsync(wasmBytes, opts = {}) {
  const { mainModule, hostImports, b, runtime } = prepareWasm(wasmBytes, opts, { jspi: hasJspi });

  if (hasJspi) {
    // Wrap stdin reads as suspending imports so the event loop stays free while
    // Twinkle waits for LSP input. Keep chunk and timeout reads on the same
    // stream-based path; mixing process.stdin.read() with fs.readSync(0, ...)
    // can strand bytes in Node's stream buffer.
    hostImports.host.stdin_read_chunk = new WebAssembly.Suspending(
      async (maxBytes) =>
        makeByteArray(b, await runtime.host.readStdinAsync(maxBytes, 2147483647, runtime)),
    );
    hostImports.host.stdin_read_timeout = new WebAssembly.Suspending(
      async (maxBytes, timeoutMs) =>
        makeByteArray(b, await runtime.host.readStdinAsync(maxBytes, timeoutMs, runtime)),
    );

    // Wrap run_wasm as a suspending import so child programs can themselves use
    // JSPI suspending imports.
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
          imports: runtime.imports,
          marshalSpec: runtime.marshalSpec,
          host: runtime.host,
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
