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
  'int.tw', 'iterator.tw', 'range.tw', 'string.tw', 'vector.tw',
];
const STDLIB_FILES = ['date.tw', 'fs.tw', 'path.tw', 'proc.tw'];

async function loadResources() {
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
      if (imp.kind !== 'function') continue;

      // Try globalThis[module][name]
      const mod = globalThis[imp.module];
      const fn = mod?.[imp.name];
      if (typeof fn !== 'function') continue; // silently skip in playground

      // Create a bridged wrapper that auto-converts args and return values
      const bridgedFn = (...args) => {
        const jsArgs = args.map((arg) => {
          if (typeof arg === 'bigint') return Number(arg);
          if (typeof arg === 'number') return arg;
          // GC ref — assume string
          return decodeString(b, arg);
        });
        const result = fn.apply(mod, jsArgs);
        if (result === undefined || result === null) return;
        if (typeof result === 'string') return encodeString(b, result);
        if (typeof result === 'number') return result;
        if (typeof result === 'bigint') return result;
        return result;
      };

      if (!hostImports[imp.module]) hostImports[imp.module] = {};
      hostImports[imp.module][imp.name] = bridgedFn;
    }
  } catch (_e) {
    // Module.imports may fail on GC modules in some runtimes.
  }
}

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
  autoBridgeExternImports(mod, hostImports, b);
  try {
    new WebAssembly.Instance(mod, hostImports);
    return 0;
  } catch (e) {
    if (e instanceof HostExit) return e.code;
    throw e;
  }
}

// ---------------------------------------------------------------------------
// Message handler
// ---------------------------------------------------------------------------

self.onmessage = async (event) => {
  const { type, code } = event.data;
  if (type !== 'run') return;

  try {
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

    const exitCode = runWasmBytes(null, {
      programArgs: ['boot.wasm', 'run', '/input/main.tw'],
      env: { TWINKLE_ROOT: '/' },
      vfs,
      emit,
      wasmModule: cachedBootModule,
    });

    self.postMessage({ type: 'done', exitCode });
  } catch (e) {
    self.postMessage({ type: 'error', message: e.message ?? String(e) });
  }
};
