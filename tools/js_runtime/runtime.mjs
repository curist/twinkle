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
 * Resolve each wasm extern import to a JS function (and its optional arg spec).
 *
 * A scoped `imports[module][name]` entry is either:
 *   - a function — the implementation, with default arg marshaling, or
 *   - `{ fn?, args? }` — `fn` is the implementation (omit it to fall back to
 *     `globals[module][name]`), and `args` is the per-arg marshal spec, an array
 *     of `"raw" | "string"` keyed by position.
 *
 * Resolution order per import: scoped `imports[module][name]`, then
 * `globals[module][name]`. Imports already satisfied by `hostImports`, or that
 * are not functions, are skipped. Returns the resolved bindings (each with
 * `fn`, `recv`, and `args`) plus a list of unresolved "module.name" strings.
 */
export function resolveExternImports(importList, hostImports, imports = {}, globals = globalThis) {
  const found = [];
  const missing = [];
  for (const imp of importList) {
    if (hostImports[imp.module]?.[imp.name] !== undefined) continue;
    if (imp.kind !== "function") continue;

    const scoped = imports[imp.module]?.[imp.name];
    let fn;
    let recv;
    let args;

    if (typeof scoped === "function") {
      fn = scoped;
      recv = imports[imp.module];
    } else if (scoped && typeof scoped === "object") {
      args = scoped.args;
      if (typeof scoped.fn === "function") {
        fn = scoped.fn;
        recv = imports[imp.module];
      } else {
        fn = globals[imp.module]?.[imp.name];
        recv = globals[imp.module];
      }
    } else {
      fn = globals[imp.module]?.[imp.name];
      recv = globals[imp.module];
    }

    if (typeof fn === "function") {
      found.push({ module: imp.module, name: imp.name, fn, recv, args });
    } else {
      missing.push(`${imp.module}.${imp.name}`);
    }
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
/**
 * Read the compiler-emitted `twinkle.externs` custom section into a
 * `module → name → { args, ret }` map. Best-effort: `customSections` may throw
 * on GC modules in some engines (same risk class as `Module.imports`), and the
 * section is absent for non-Twinkle wasm — either way we return `{}` and the
 * manual override / string-default path takes over.
 */
function readExternMeta(wasmModule) {
  let sections;
  try {
    sections = WebAssembly.Module.customSections(wasmModule, "twinkle.externs");
  } catch {
    return {};
  }
  if (!sections || sections.length === 0) return {};
  try {
    const list = JSON.parse(new TextDecoder().decode(new Uint8Array(sections[0])));
    const map = {};
    for (const e of list) {
      (map[e.module] ??= {})[e.name] = { args: e.args, ret: e.ret };
    }
    return map;
  } catch {
    return {};
  }
}

function autoBridgeExternImports(wasmModule, hostImports, b, jspi = false, imports = {}, externMeta = {}) {
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

  // Per-import arg marshaling honors a per-position kind spec. Two vocabularies
  // are accepted and treated identically: the compiler-emitted twinkle.externs
  // kinds ("str" | "ref" | "i64" | "f64" | "i32") and the manual override's
  // ("raw" | "string"). Numbers pass through (handled before the spec). "ref" /
  // "raw" pass the value untouched — essential for externref args (e.g. a canvas
  // 2D context), since decodeString on an opaque host object recurses until a
  // stack overflow in some engines (notably Safari). Anything else (incl. no
  // entry) is assumed to be a Wasm GC string and decoded.
  const makeMarshalArgs = (spec) => (args) => args.map((arg, i) => {
    if (typeof arg === "bigint") return Number(arg);
    if (typeof arg === "number") return arg;
    const k = spec?.[i];
    if (k === "ref" || k === "raw") return arg;
    return decodeString(b, arg);
  });

  // Return marshaling uses the compiler-emitted `ret` kind when available, so
  // we never guess from the JS value's type. Falls back to a generic guess when
  // there is no section (manual-only callers).
  const marshalReturn = (result, ret) => {
    if (result === undefined || result === null) return;
    switch (ret) {
      case "ref": return result;
      case "str": return typeof result === "string" ? encodeString(b, result) : result;
      case "i64": return typeof result === "number" ? BigInt(result) : result;
      case "f64": case "i32": return result;
      case "void": return undefined;
    }
    if (typeof result === "string") return encodeString(b, result);
    if (typeof result === "number") return result;
    if (typeof result === "bigint") return result;
    return result;
  };

  for (const { module, name, fn, recv, args } of found) {
    // Precedence: a manual `args` override wins over the section's kinds.
    const meta = externMeta[module]?.[name];
    const marshalArgs = makeMarshalArgs(args ?? meta?.args);
    const ret = meta?.ret;
    let bridgedFn;
    if (jspi) {
      // JSPI mode: async wrapper so Promise-returning JS functions suspend
      // Wasm. Non-Promise returns pass through without suspension.
      const asyncWrapper = async (...args) =>
        marshalReturn(await fn.apply(recv, marshalArgs(args)), ret);
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
        return marshalReturn(result, ret);
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
// In-memory host adapter
// ---------------------------------------------------------------------------

/**
 * A host adapter backed by an in-memory file map — the default for browsers and
 * other environments without a real filesystem. Reads/writes a `Map<string,
 * Uint8Array>`; stdin is always EOF. Pure JS (no `node:` deps), so it is safe to
 * load anywhere.
 *
 * @param {Map<string,Uint8Array> | Iterable<[string,Uint8Array]>} [initialFiles]
 */
export function createMemoryHost(initialFiles) {
  const files = initialFiles instanceof Map ? initialFiles : new Map(initialFiles ?? []);
  const norm = (p) => (p.startsWith("/") ? p : "/" + p).replace(/\/+/g, "/");
  return {
    files,
    resolvePath(cwd, p) {
      if (p.startsWith("/")) return norm(p);
      return norm((cwd.endsWith("/") ? cwd : cwd + "/") + p);
    },
    readFile(path) {
      const data = files.get(norm(path));
      if (data === undefined) throw new Error(`file not found: ${path}`);
      return data;
    },
    writeFile(path, text) { files.set(norm(path), textEncoder.encode(text)); },
    writeBytes(path, bytes) { files.set(norm(path), bytes); },
    exists(path) {
      const np = norm(path);
      if (files.has(np)) return true;
      const prefix = np.endsWith("/") ? np : np + "/";
      for (const k of files.keys()) if (k.startsWith(prefix)) return true;
      return false;
    },
    listDir(path) {
      const prefix = norm(path).replace(/\/?$/, "/");
      const names = new Set();
      for (const k of files.keys()) {
        if (k.startsWith(prefix)) {
          const name = k.slice(prefix.length).split("/")[0];
          if (name) names.add(name);
        }
      }
      return [...names].sort();
    },
    mkdirp() { /* virtual dirs are implicit */ },
    readStdin(_maxBytes, _timeoutMs, runtime) {
      runtime.stdinEof = true;
      return new Uint8Array(0);
    },
    readStdinAsync(_maxBytes, _timeoutMs, runtime) {
      runtime.stdinEof = true;
      return Promise.resolve(new Uint8Array(0));
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
  };

  const hostImports = makeHostImports(b, runtime, bridgeBytes);
  const mainModule = new WebAssembly.Module(wasmBytes);
  const externMeta = readExternMeta(mainModule);
  autoBridgeExternImports(mainModule, hostImports, b, jspi, imports, externMeta);

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
