// Web Worker — Twinkle Wasm runtime for the browser playground.
//
// Receives: { type: 'run', code: string }
// Posts:    { type: 'status' | 'stdout' | 'stderr' | 'done' | 'error', ... }

// Base URL: same directory as worker.js (works for both local dev and GitHub Pages)
const BASE_URL = new URL('./', self.location.href).href;

const textDecoder = new TextDecoder();
const textEncoder = new TextEncoder();

// ---------------------------------------------------------------------------
// Cached resources (loaded once)
// ---------------------------------------------------------------------------

let cachedBridgeBytes = null;
let cachedBootBytes   = null;
let cachedPrelude     = null; // Map<path, Uint8Array>

const PRELUDE_FILES = [
  'byte.tw', 'dict.tw', 'float.tw', 'int.tw', 'iterator.tw',
  'option.tw', 'result.tw', 'string.tw', 'vector.tw',
];
const PRELUDE_SIG_FILES = [
  'bool.tw', 'byte.tw', 'cell.tw', 'dict.tw', 'float.tw',
  'int.tw', 'iterator.tw', 'range.tw', 'string.tw', 'task.tw', 'vector.tw',
];
const STDLIB_FILES = ['date.tw', 'fs.tw', 'io.tw', 'path.tw', 'proc.tw'];

let loadResourcesPromise = null;

function loadResources() {
  if (!loadResourcesPromise) loadResourcesPromise = doLoadResources();
  return loadResourcesPromise;
}

async function doLoadResources() {
  if (cachedBridgeBytes && cachedBootBytes && cachedPrelude) return;

  self.postMessage({ type: 'status', text: 'Loading compiler…' });

  const [bridgeResp, bootResp] = await Promise.all([
    fetch(BASE_URL + 'bridge.wasm'),
    fetch(BASE_URL + 'boot.wasm'),
  ]);

  if (!bridgeResp.ok) throw new Error(`Failed to fetch bridge.wasm: ${bridgeResp.status}`);
  if (!bootResp.ok)   throw new Error(`Failed to fetch boot.wasm: ${bootResp.status}`);

  [cachedBridgeBytes, cachedBootBytes] = await Promise.all([
    bridgeResp.arrayBuffer().then(b => new Uint8Array(b)),
    bootResp.arrayBuffer().then(b  => new Uint8Array(b)),
  ]);

  // Pre-compile the boot module (large; cache the WebAssembly.Module object)
  self.postMessage({ type: 'status', text: 'Compiling boot module…' });
  cachedBootModule = new WebAssembly.Module(cachedBootBytes);

  self.postMessage({ type: 'status', text: 'Loading prelude…' });
  // TWINKLE_ROOT="/", so prelude lives at /prelude and stdlib at /stdlib.
  // This keeps prelude_root == parent_prelude ("/prelude") so canonical_module_path
  // does NOT remap paths — linker and reader both see /prelude/... consistently.
  cachedPrelude = new Map();

  const fetches = [];
  for (const name of PRELUDE_FILES) {
    fetches.push(
      fetch(BASE_URL + `prelude/${name}`)
        .then(r => { if (!r.ok) throw new Error(`fetch ${name}: ${r.status}`); return r.arrayBuffer(); })
        .then(buf => cachedPrelude.set(`/prelude/${name}`, new Uint8Array(buf)))
    );
  }
  for (const name of PRELUDE_SIG_FILES) {
    fetches.push(
      fetch(BASE_URL + `prelude/signatures/${name}`)
        .then(r => { if (!r.ok) throw new Error(`fetch signatures/${name}: ${r.status}`); return r.arrayBuffer(); })
        .then(buf => cachedPrelude.set(`/prelude/signatures/${name}`, new Uint8Array(buf)))
    );
  }
  for (const name of STDLIB_FILES) {
    fetches.push(
      fetch(BASE_URL + `stdlib/${name}`)
        .then(r => { if (!r.ok) throw new Error(`fetch ${name}: ${r.status}`); return r.arrayBuffer(); })
        .then(buf => cachedPrelude.set(`/stdlib/${name}`, new Uint8Array(buf)))
    );
  }
  await Promise.all(fetches);
}

let cachedBootModule = null;

// ---------------------------------------------------------------------------
// Virtual filesystem
// ---------------------------------------------------------------------------

function makeVfs(initial) {
  const files = new Map(initial);

  function norm(p) {
    if (!p.startsWith('/')) p = '/' + p;
    return p.replace(/\/+/g, '/');
  }

  return {
    read(path) { return files.get(norm(path)); },
    write(path, data) { files.set(norm(path), data instanceof Uint8Array ? data : textEncoder.encode(data)); },
    exists(path) {
      const np = norm(path);
      if (files.has(np)) return true;
      const prefix = np.endsWith('/') ? np : np + '/';
      for (const k of files.keys()) if (k.startsWith(prefix)) return true;
      return false;
    },
    listDir(path) {
      const np = norm(path);
      const prefix = np.endsWith('/') ? np : np + '/';
      const names = new Set();
      for (const k of files.keys()) {
        if (k.startsWith(prefix)) {
          const name = k.slice(prefix.length).split('/')[0];
          if (name) names.add(name);
        }
      }
      return [...names].sort();
    },
    mkdirp(_path) { /* virtual dirs are implicit */ },
  };
}

// ---------------------------------------------------------------------------
// Bridge helpers
// ---------------------------------------------------------------------------

const RESULT_TYPE_ID = 1;
const RESULT_OK  = 0;
const RESULT_ERR = 1;

class HostExit extends Error {
  constructor(code) { super(`exit(${code})`); this.code = code; }
}

function instantiateBridge() {
  return new WebAssembly.Instance(new WebAssembly.Module(cachedBridgeBytes)).exports;
}

function decodeString(b, ref) {
  if (!ref) return '';
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

function makeByteArray(b, bytes) {
  const arr = b.array_new(bytes.length);
  for (let i = 0; i < bytes.length; i++) b.array_set(arr, i, b.i31_new(bytes[i]));
  return arr;
}

function decodeByteArray(b, arrRef) {
  const len = b.array_len(arrRef);
  const out = new Uint8Array(len);
  for (let i = 0; i < len; i++) out[i] = b.i31_get(b.array_get(arrRef, i));
  return out;
}

function makeStringArray(b, strings) {
  const arr = b.array_new(strings.length);
  for (let i = 0; i < strings.length; i++) b.array_set(arr, i, encodeString(b, strings[i]));
  return arr;
}

function decodeStringArray(b, arrRef) {
  const len = b.array_len(arrRef);
  const out = new Array(len);
  for (let i = 0; i < len; i++) out[i] = decodeString(b, b.array_get(arrRef, i));
  return out;
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

// ---------------------------------------------------------------------------
// Auto-bridge extern imports
// ---------------------------------------------------------------------------

/**
 * Parse the import section from raw Wasm bytes.
 * Fallback for runtimes where WebAssembly.Module.imports() fails on GC modules.
 */
function parseWasmImports(bytes) {
  const imports = [];
  let offset = 8; // skip magic + version

  const readLEB128 = () => {
    let result = 0, shift = 0, byte;
    do {
      byte = bytes[offset++];
      result |= (byte & 0x7f) << shift;
      shift += 7;
    } while (byte & 0x80);
    return result >>> 0;
  };
  const readString = () => {
    const len = readLEB128();
    const str = textDecoder.decode(bytes.subarray(offset, offset + len));
    offset += len;
    return str;
  };

  while (offset < bytes.length) {
    const sectionId = bytes[offset++];
    const sectionSize = readLEB128();
    const sectionEnd = offset + sectionSize;

    if (sectionId === 2) { // Import section
      const count = readLEB128();
      for (let i = 0; i < count; i++) {
        const module = readString();
        const name = readString();
        const kind = bytes[offset++];
        if (kind === 0) { // function import: skip type index
          readLEB128();
        } else {
          // Non-function imports: skip to section end
          // (Twinkle extern only generates function imports)
          break;
        }
        imports.push({ module, name, kind: 'function' });
      }
      break;
    }

    offset = sectionEnd;
  }

  return imports;
}

/**
 * Auto-bridge extern imports by resolving `globalThis[module][name]` and
 * wrapping with type conversions for Twinkle's extern-safe types:
 *   - String params (GC refs) are decoded via bridge
 *   - String returns from JS are encoded via bridge
 *   - Int (bigint), Float (number), Bool (i32) pass through
 */
const EXTERN_ARG_MARSHAL = {
  console: {
    log: ['string'],
    warn: ['string'],
    error: ['string'],
    info: ['string'],
  },
  http: {
    fetch: ['string'],
    fetch_bytes: ['string'],
  },
  timer: {
    sleep_ms: ['raw'],
  },
  canvas: {
    get_context: ['string'],
    get_width: [],
    get_height: [],
    set_fill_style: ['raw', 'string'],
    fill_rect: ['raw', 'raw', 'raw', 'raw', 'raw'],
    clear_rect: ['raw', 'raw', 'raw', 'raw', 'raw'],
    set_stroke_style: ['raw', 'string'],
    stroke_rect: ['raw', 'raw', 'raw', 'raw', 'raw'],
    begin_path: ['raw'],
    close_path: ['raw'],
    move_to: ['raw', 'raw', 'raw'],
    line_to: ['raw', 'raw', 'raw'],
    arc: ['raw', 'raw', 'raw', 'raw', 'raw', 'raw'],
    fill: ['raw'],
    stroke: ['raw'],
    set_line_width: ['raw', 'raw'],
    set_global_alpha: ['raw', 'raw'],
    set_font: ['raw', 'string'],
    fill_text: ['raw', 'string', 'raw', 'raw'],
  },
};

function autoBridgeExternImports(wasmModule, hostImports, b, jspi = false, wasmBytes = null) {
  let importList;
  try {
    importList = WebAssembly.Module.imports(wasmModule);
  } catch (_e) {
    // Module.imports fails on GC modules in some runtimes (e.g. mobile Safari).
    // Fall back to parsing the binary import section directly.
    importList = wasmBytes ? parseWasmImports(wasmBytes) : [];
  }

  for (const imp of importList) {
      // Already provided (host module, etc.) — skip
      if (hostImports[imp.module]?.[imp.name] !== undefined) continue;
      if (imp.kind !== 'function') continue;

      // Try globalThis[module][name]
      const mod = globalThis[imp.module];
      const fn = mod?.[imp.name];
      if (typeof fn !== 'function') continue; // silently skip in playground

      const argSpec = EXTERN_ARG_MARSHAL[imp.module]?.[imp.name];
      const marshalArg = (arg, kind) => {
        if (typeof arg === 'bigint') return Number(arg);
        if (typeof arg === 'number') return arg;
        if (kind === 'raw') return arg;
        if (kind === 'string') return decodeString(b, arg);
        // Unknown extern signatures use the legacy best-effort path.
        // Known externref-heavy APIs (notably canvas) are listed above so we
        // never probe opaque host objects with bridge string helpers; Safari can
        // recurse until stack overflow when a non-string externref is passed to
        // those Wasm GC helpers.
        try { return decodeString(b, arg); } catch { return arg; }
      };
      const marshalArgs = (args) => args.map((arg, i) => marshalArg(arg, argSpec?.[i]));

      const marshalReturn = (result) => {
        if (result === undefined || result === null) return;
        if (typeof result === 'string') return encodeString(b, result);
        if (typeof result === 'number') return result;
        if (typeof result === 'bigint') return result;
        return result;
      };

      let bridgedFn;
      if (jspi) {
        const asyncWrapper = async (...args) => {
          const result = await fn.apply(mod, marshalArgs(args));
          return marshalReturn(result);
        };
        bridgedFn = new WebAssembly.Suspending(asyncWrapper);
      } else {
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
}

// ---------------------------------------------------------------------------
// JSPI feature detection
// ---------------------------------------------------------------------------

const hasJspi =
  typeof WebAssembly.Suspending === "function" &&
  typeof WebAssembly.promising === "function";

// ---------------------------------------------------------------------------
// Browser-provided extern globals for JSPI-capable programs
// ---------------------------------------------------------------------------

// timer.sleep_ms(ms) — suspends Wasm for N milliseconds via setTimeout.
// Usage: extern timer { fn sleep_ms(ms: Int) }
globalThis.timer = {
  sleep_ms: (ms) => new Promise(resolve => setTimeout(resolve, ms)),
};

// http.fetch(url) — fetches a URL and returns the response body as a string.
// http.fetch_bytes(url) — fetches a URL and returns the response body as bytes.
// Usage: extern http { fn fetch(url: String) String }
globalThis.http = {
  fetch: async (url) => {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    return await response.text();
  },
  fetch_bytes: async (url) => {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    const buf = await response.arrayBuffer();
    return new Uint8Array(buf);
  },
};

// ---------------------------------------------------------------------------
// Core runner
// ---------------------------------------------------------------------------

function runWasmBytes(wasmBytes, { programArgs = [], env = {}, vfs, emit, wasmModule = null }) {
  const b = instantiateBridge();

  const hostImports = {
    host: {
      // I/O
      print:   (s) => emit('stdout', decodeString(b, s)),
      println: (s) => emit('stdout', decodeString(b, s) + '\n'),
      error:   (s) => { const msg = decodeString(b, s); emit('stderr', msg + '\n'); throw new Error('host.error: ' + msg); },
      eprint:   (s) => emit('stderr', decodeString(b, s)),
      eprintln: (s) => emit('stderr', decodeString(b, s) + '\n'),

      // Numeric
      f64_to_string: (n) => encodeString(b, n.toString()),

      // Process
      args: () => makeStringArray(b, programArgs),
      env:  (keyRef) => {
        const key = decodeString(b, keyRef);
        const val = env[key];
        return val === undefined ? makeStringArray(b, []) : makeStringArray(b, [val]);
      },
      cwd:  () => encodeString(b, '/'),
      exit: (code) => { throw new HostExit(typeof code === 'bigint' ? Number(code) : code); },
      now:  () => performance.now(),

      run_wasm: (bytesRef, argvRef) => {
        const childBytes = decodeByteArray(b, bytesRef);
        const childArgv  = decodeStringArray(b, argvRef);
        const exitCode   = runWasmBytes(childBytes, { programArgs: childArgv, env, vfs, emit });
        return BigInt(exitCode);
      },

      // Filesystem
      read_file: (pathRef) => {
        const path = decodeString(b, pathRef);
        const data = vfs.read(path);
        if (data === undefined) return makeResultErr(b, encodeString(b, `file not found: ${path}`));
        return makeResultOk(b, makeByteArray(b, data));
      },
      write_file: (pathRef, contentRef) => {
        vfs.write(decodeString(b, pathRef), textEncoder.encode(decodeString(b, contentRef)));
      },
      write_bytes: (pathRef, bytesRef) => {
        vfs.write(decodeString(b, pathRef), decodeByteArray(b, bytesRef));
      },
      mkdirp:   (pathRef) => vfs.mkdirp(decodeString(b, pathRef)),
      list_dir: (pathRef) => makeStringArray(b, vfs.listDir(decodeString(b, pathRef))),
      exists:   (pathRef) => vfs.exists(decodeString(b, pathRef)) ? 1 : 0,
      stdin_read_chunk: (_maxBytes) => makeByteArray(b, []),
      stdin_read_timeout: (_maxBytes, _timeoutMs) => makeByteArray(b, []),
      stdin_eof: () => 1,
      stdout_write_bytes: (bytesRef) => {
        const bytes = decodeByteArray(b, bytesRef);
        emit('stdout', textDecoder.decode(bytes));
      },

      // Parsing
      parse_int: (sRef) => {
        const n = parseInt(decodeString(b, sRef), 10);
        return isNaN(n) ? 0n : BigInt(n);
      },
      parse_float: (sRef) => {
        const f = parseFloat(decodeString(b, sRef));
        return isNaN(f) ? [0.0, 0] : [f, 1];
      },
    },
  };

  const mod = wasmModule ?? new WebAssembly.Module(wasmBytes);
  autoBridgeExternImports(mod, hostImports, b, false, wasmBytes);
  try {
    const instance = new WebAssembly.Instance(mod, hostImports);
    // Boot-compiled modules export __twinkle_start instead of using a Wasm
    // start section. Stage0-compiled modules still use start and run during
    // instantiation.
    if (instance.exports.__twinkle_start) {
      instance.exports.__twinkle_start();
    }
    return 0;
  } catch (e) {
    if (e instanceof HostExit) return e.code;
    throw e;
  }
}

// ---------------------------------------------------------------------------
// Async runner (JSPI-aware, top-level only)
// ---------------------------------------------------------------------------

async function runWasmBytesAsync(wasmBytes, { programArgs = [], env = {}, vfs, emit, wasmModule = null }) {
  const b = instantiateBridge();

  const hostImports = {
    host: {
      // I/O
      print:   (s) => emit('stdout', decodeString(b, s)),
      println: (s) => emit('stdout', decodeString(b, s) + '\n'),
      error:   (s) => { const msg = decodeString(b, s); emit('stderr', msg + '\n'); throw new Error('host.error: ' + msg); },
      eprint:   (s) => emit('stderr', decodeString(b, s)),
      eprintln: (s) => emit('stderr', decodeString(b, s) + '\n'),

      // Numeric
      f64_to_string: (n) => encodeString(b, n.toString()),

      // Process
      args: () => makeStringArray(b, programArgs),
      env:  (keyRef) => {
        const key = decodeString(b, keyRef);
        const val = env[key];
        return val === undefined ? makeStringArray(b, []) : makeStringArray(b, [val]);
      },
      cwd:  () => encodeString(b, '/'),
      exit: (code) => { throw new HostExit(typeof code === 'bigint' ? Number(code) : code); },
      now:  () => performance.now(),

      run_wasm: (bytesRef, argvRef) => {
        const childBytes = decodeByteArray(b, bytesRef);
        const childArgv  = decodeStringArray(b, argvRef);
        const exitCode   = runWasmBytes(childBytes, { programArgs: childArgv, env, vfs, emit });
        return BigInt(exitCode);
      },

      // Filesystem
      read_file: (pathRef) => {
        const path = decodeString(b, pathRef);
        const data = vfs.read(path);
        if (data === undefined) return makeResultErr(b, encodeString(b, `file not found: ${path}`));
        return makeResultOk(b, makeByteArray(b, data));
      },
      write_file: (pathRef, contentRef) => {
        vfs.write(decodeString(b, pathRef), textEncoder.encode(decodeString(b, contentRef)));
      },
      write_bytes: (pathRef, bytesRef) => {
        vfs.write(decodeString(b, pathRef), decodeByteArray(b, bytesRef));
      },
      mkdirp:   (pathRef) => vfs.mkdirp(decodeString(b, pathRef)),
      list_dir: (pathRef) => makeStringArray(b, vfs.listDir(decodeString(b, pathRef))),
      exists:   (pathRef) => vfs.exists(decodeString(b, pathRef)) ? 1 : 0,
      stdin_read_chunk: (_maxBytes) => makeByteArray(b, []),
      stdin_read_timeout: (_maxBytes, _timeoutMs) => makeByteArray(b, []),
      stdin_eof: () => 1,
      stdout_write_bytes: (bytesRef) => {
        const bytes = decodeByteArray(b, bytesRef);
        emit('stdout', textDecoder.decode(bytes));
      },

      // Parsing
      parse_int: (sRef) => {
        const n = parseInt(decodeString(b, sRef), 10);
        return isNaN(n) ? 0n : BigInt(n);
      },
      parse_float: (sRef) => {
        const f = parseFloat(decodeString(b, sRef));
        return isNaN(f) ? [0.0, 0] : [f, 1];
      },
    },
  };

  const mod = wasmModule ?? new WebAssembly.Module(wasmBytes);
  autoBridgeExternImports(mod, hostImports, b, hasJspi, wasmBytes);

  if (hasJspi) {
    // Fetch-backed read_file: check VFS first, then try fetching from server.
    // This enables lazy loading of prelude/stdlib files on demand.
    hostImports.host.read_file = new WebAssembly.Suspending(
      async (pathRef) => {
        const path = decodeString(b, pathRef);
        const cached = vfs.read(path);
        if (cached !== undefined) return makeResultOk(b, makeByteArray(b, cached));
        try {
          // Strip leading / to make it relative to BASE_URL
          const url = BASE_URL + path.replace(/^\/+/, '');
          const resp = await fetch(url);
          if (!resp.ok) return makeResultErr(b, encodeString(b, `file not found: ${path}`));
          const bytes = new Uint8Array(await resp.arrayBuffer());
          vfs.write(path, bytes);
          return makeResultOk(b, makeByteArray(b, bytes));
        } catch (e) {
          return makeResultErr(b, encodeString(b, `file not found: ${path}`));
        }
      },
    );

    // Wrap run_wasm as suspending so child programs can use JSPI imports
    hostImports.host.run_wasm = new WebAssembly.Suspending(
      async (bytesRef, argvRef) => {
        const childBytes = decodeByteArray(b, bytesRef);
        const childArgv  = decodeStringArray(b, argvRef);
        const exitCode   = await runWasmBytesAsync(childBytes, { programArgs: childArgv, env, vfs, emit });
        return BigInt(exitCode);
      },
    );
  }

  try {
    const instance = new WebAssembly.Instance(mod, hostImports);
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
    if (e instanceof HostExit) return e.code;
    throw e;
  }
}

// ---------------------------------------------------------------------------
// Canvas extern support (OffscreenCanvas from main thread)
// ---------------------------------------------------------------------------

function setupCanvasGlobals(offscreen) {
  const ctx = offscreen.getContext('2d');
  globalThis.canvas = {
    get_context:       (_id) => ctx ?? null,
    get_width:         () => offscreen.width,
    get_height:        () => offscreen.height,
    set_fill_style:    (c, color) => { c.fillStyle = color; },
    fill_rect:         (c, x, y, w, h) => c.fillRect(x, y, w, h),
    clear_rect:        (c, x, y, w, h) => c.clearRect(x, y, w, h),
    set_stroke_style:  (c, color) => { c.strokeStyle = color; },
    stroke_rect:       (c, x, y, w, h) => c.strokeRect(x, y, w, h),
    begin_path:        (c) => c.beginPath(),
    close_path:        (c) => c.closePath(),
    move_to:           (c, x, y) => c.moveTo(x, y),
    line_to:           (c, x, y) => c.lineTo(x, y),
    arc:               (c, x, y, r, start, end) => c.arc(x, y, r, start, end),
    fill:              (c) => c.fill(),
    stroke:            (c) => c.stroke(),
    set_line_width:    (c, w) => { c.lineWidth = w; },
    set_global_alpha:  (c, a) => { c.globalAlpha = a; },
    set_font:          (c, font) => { c.font = font; },
    fill_text:         (c, text, x, y) => c.fillText(text, x, y),
  };
}

// ---------------------------------------------------------------------------
// Message handler
// ---------------------------------------------------------------------------

self.postMessage({ type: 'ready' });
loadResources();

self.onmessage = async (event) => {
  const { type, code, offscreenCanvas } = event.data;
  if (type !== 'run') return;

  try {
    if (offscreenCanvas) setupCanvasGlobals(offscreenCanvas);

    await loadResources();

    // Build virtual FS: prelude/stdlib + user input
    const vfs = makeVfs(cachedPrelude);
    vfs.write('/input/main.tw', code);

    const output = { stdout: '', stderr: '' };
    function emit(stream, text) {
      output[stream] += text;
      self.postMessage({ type: stream, text });
    }

    self.postMessage({ type: 'status', text: 'Running…' });

    const exitCode = await runWasmBytesAsync(null, {
      programArgs: ['playground.wasm', '/input/main.tw'],
      env: { TWINKLE_ROOT: '/', NO_COLOR: '1' },
      vfs,
      emit,
      wasmModule: cachedBootModule,
    });

    self.postMessage({ type: 'done', exitCode });
  } catch (e) {
    self.postMessage({ type: 'error', message: e.message ?? String(e) });
  }
};
