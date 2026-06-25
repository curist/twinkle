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

function importListFromExternMeta(externMeta) {
  const list = [];
  for (const [module, entries] of Object.entries(externMeta)) {
    for (const name of Object.keys(entries)) {
      list.push({ module, name, kind: "function" });
    }
  }
  return list;
}

function bridgeExternImports(importList, hostImports, b, jspi = false, imports = {}, externMeta = {}) {
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

function autoBridgeExternImports(wasmModule, hostImports, b, jspi = false, imports = {}, externMeta = {}) {
  let importList;
  try {
    importList = WebAssembly.Module.imports(wasmModule);
  } catch {
    // Some browsers (notably Safari on Wasm GC modules) can instantiate a module
    // but reject import introspection. Twinkle modules carry their extern ABI in
    // a custom section, so use that as the browser fallback instead of leaving
    // extern modules absent from the import object.
    importList = importListFromExternMeta(externMeta);
  }
  bridgeExternImports(importList, hostImports, b, jspi, imports, externMeta);
}

function missingImportFromError(e) {
  const msg = e?.message ?? "";
  const match = msg.match(/import\s+([^\s:]+):([^\s]+)\s+must be an object/)
    ?? msg.match(/Import #[0-9]+ module="([^"]+)" function="([^"]+)"/)
    ?? msg.match(/Import #[0-9]+ "([^"]+)" "([^"]+)"/);
  if (match) return { module: match[1], name: match[2], kind: "function" };
  const moduleOnly = msg.match(/Import #[0-9]+ "([^"]+)": module is not an object or function/);
  if (moduleOnly) return { module: moduleOnly[1], name: null, kind: "function" };
  return null;
}

function instantiateWithExternRetry(mainModule, hostImports, b, jspi, imports, externMeta) {
  // Last-ditch Safari fallback: if both Module.imports() and customSections()
  // are unavailable for a GC module, instantiate once, read the missing import
  // from the LinkError text, bridge it, and retry. This preserves globalThis
  // fallback for common browser globals such as performance/Math.
  for (let i = 0; i < 64; i++) {
    try {
      return new WebAssembly.Instance(mainModule, hostImports);
    } catch (e) {
      const imp = missingImportFromError(e);
      if (!imp) throw e;
      if (imp.name === null) {
        hostImports[imp.module] = {};
      } else {
        bridgeExternImports([imp], hostImports, b, jspi, imports, externMeta);
      }
    }
  }
  throw new Error("too many missing WebAssembly imports while instantiating");
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
      sleep: (_ms) => {
        throw new Error("host.sleep requires the async JSPI runtime");
      },
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
      write_buffer_raw: (pathRef, ptr, len) => {
        const filePath = runtime.host.resolvePath(runtime.cwd, decodeString(b, pathRef));
        const memory = runtime.instance?.exports?.memory;
        if (!memory) {
          throw new Error("host.write_buffer_raw requires the guest to export linear memory");
        }
        const start = Number(ptr);
        const size = Number(len);
        runtime.host.writeBytes(filePath, new Uint8Array(memory.buffer, start, size).slice());
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
// Cooperative task scheduler (JSPI binding of the abstract suspension intrinsics)
// ---------------------------------------------------------------------------

/**
 * The JS host side of Twinkle's stackful Task<T> model. The compiler lowers
 * task composition operations to operation-named intrinsics
 * (task_create / suspend_await / suspend_yield) imported from the "task"
 * module; this scheduler implements them and can also route suspending host
 * imports back through the scheduler.
 *
 * Each task body runs on its own JSPI stack via promising(__task_run). A body
 * suspends by calling a Suspending import that returns a Promise the scheduler
 * controls; resuming = resolving that Promise. All host imports speak plain
 * i32 task ids — JS never constructs or inspects a Task<T> GC struct.
 *
 * Concurrency invariant: when a task's Wasm code runs, scheduler.current is its
 * id. We keep this race-free by resuming strictly one task per microtask:
 * runOne sets current, then resolves exactly one parked Promise (or starts one
 * body); the resumed task runs until it next suspends or completes, and only
 * then does it call schedule() to advance. No two resumes interleave, so the
 * single global current is always valid for whichever stack is executing.
 */
function createTaskScheduler() {
  const TOP = 0;
  // Upper bound on how long continuous Task.yield()ing may withhold the host
  // event loop. suspend_yield normally re-schedules via a microtask (cheap), but
  // a task that yields in a tight loop would then keep the microtask queue
  // non-empty forever and starve the host's timer/IO (macrotask) phase — pending
  // stdin never gets read, setTimeout never fires. Forcing a macrotask hop at
  // least this often lets the event loop service timers and IO between yields.
  const YIELD_MACROTASK_MS = 4;
  const s = {
    nextId: 1,
    nextChannelId: 1,
    current: TOP,
    tasks: new Map(),
    channels: new Map(),
    runnable: [], // entries: {kind:'start', id} | {kind:'resume', id, fire}
    pendingHost: 0, // in-flight timers / stdin reads
    blockedOnTask: 0, // tasks parked awaiting another task
    blockedOnChannel: 0, // tasks parked sending/receiving on a channel
    pumping: false,
    promisingTaskRun: null,
    settled: false,
    topLevelDone: false,
    doneResolve: null,
    doneReject: null,
    lastYieldMacrotask: 0, // Date.now() of the last forced macrotask yield hop
  };
  s.done = new Promise((res, rej) => {
    s.doneResolve = res;
    s.doneReject = rej;
  });

  // Top-level is a pseudo-task so it can be parked as an await-waiter. It is
  // "awaited" by definition (its failure is the program's failure).
  s.tasks.set(TOP, {
    id: TOP, closure: null, state: "running", result: undefined, error: null,
    waiters: [], awaited: true,
  });

  const asError = (e) => (e instanceof Error ? e : new Error(String(e)));

  function settleDone(fn) {
    if (s.settled) return;
    s.settled = true;
    fn();
  }

  function checkQuiescence() {
    if (s.pumping || s.runnable.length > 0 || s.pendingHost > 0) return;
    // Nothing runnable and no task-host work outstanding. A top-level pseudo-task
    // parked on a channel is counted here too, so `ch.recv()` with no possible
    // sender reports a deadlock instead of hanging behind the top-level special
    // case below.
    if (s.blockedOnTask > 0 || s.blockedOnChannel > 0) {
      const msg = s.blockedOnChannel > 0
        ? "task deadlock: remaining tasks are all blocked on channels/awaits"
        : "task deadlock: remaining tasks are all blocked awaiting each other";
      settleDone(() => s.doneReject(new Error(msg)));
      return;
    }
    // An empty task scheduler does not mean the program is done while the
    // top-level pseudo-task is still suspended. Wait for explicit completion.
    if (!s.topLevelDone) return;
    // A spawned task that failed and was never awaited surfaces as the program's
    // failure rather than being swallowed.
    for (const t of s.tasks.values()) {
      if (t.state === "failed" && !t.awaited) {
        settleDone(() => s.doneReject(asError(t.error)));
        return;
      }
    }
    settleDone(() => s.doneResolve());
  }

  function schedule() {
    if (s.pumping) return;
    if (s.runnable.length === 0) {
      checkQuiescence();
      return;
    }
    s.pumping = true;
    queueMicrotask(runOne);
  }

  function runOne() {
    s.pumping = false;
    const entry = s.runnable.shift();
    if (!entry) {
      checkQuiescence();
      return;
    }
    if (entry.kind === "start") {
      const rec = s.tasks.get(entry.id);
      if (!rec || rec.state !== "new") {
        schedule();
        return;
      }
      rec.state = "running";
      s.current = entry.id;
      let p;
      try {
        p = s.promisingTaskRun(rec.closure);
      } catch (e) {
        onFail(entry.id, e);
        return;
      }
      p.then((r) => onComplete(entry.id, r), (e) => onFail(entry.id, e));
    } else {
      // resume: deliver a parked value/rejection with current set to the owner.
      s.current = entry.id;
      entry.fire();
    }
  }

  function wakeWaiters(rec) {
    const ws = rec.waiters;
    rec.waiters = [];
    for (const w of ws) {
      s.blockedOnTask--;
      if (rec.state === "failed") {
        const err = asError(rec.error);
        s.runnable.push({ kind: "resume", id: w.id, fire: () => w.reject(err) });
      } else {
        const result = rec.result;
        s.runnable.push({ kind: "resume", id: w.id, fire: () => w.resolve(result) });
      }
    }
  }

  function onComplete(id, result) {
    const rec = s.tasks.get(id);
    if (!rec) return;
    rec.state = "done";
    rec.result = result;
    wakeWaiters(rec);
    if (id === TOP) {
      // Top-level finished; drain spawned-but-unawaited tasks before exit.
      s.topLevelDone = true;
    }
    schedule();
  }

  function onFail(id, err) {
    const rec = s.tasks.get(id);
    if (!rec) return;
    rec.state = "failed";
    rec.error = err;
    if (id === TOP) {
      // A top-level trap/exit is the program's outcome; do not drain.
      settleDone(() => s.doneReject(asError(err)));
      return;
    }
    wakeWaiters(rec);
    schedule();
  }

  // --- abstract suspension intrinsics (host import implementations) ---

  // task_create(closure) -> i32 : eager-enqueue, do NOT run the body now.
  function taskCreate(closure) {
    const id = s.nextId++;
    s.tasks.set(id, {
      id, closure, state: "new", result: undefined, error: null,
      waiters: [], awaited: false,
    });
    s.runnable.push({ kind: "start", id });
    // The currently running stack keeps control until it suspends/completes,
    // at which point schedule() starts this body. No synchronous start here.
    return id;
  }

  // suspend_await(targetId) -> anyref
  async function suspendAwait(targetId) {
    const tid = Number(targetId);
    const target = s.tasks.get(tid);
    if (!target) throw new Error("Task.await: invalid task id " + tid);
    target.awaited = true;
    if (target.state === "done") return target.result;
    if (target.state === "failed") throw asError(target.error);
    const caller = s.current;
    s.blockedOnTask++;
    const p = new Promise((resolve, reject) => {
      target.waiters.push({ id: caller, resolve, reject });
    });
    schedule();
    return p;
  }

  // suspend_yield() -> void : re-enqueue at the back of the runnable queue.
  //
  // Fast path: re-schedule via a microtask so tasks round-robin without host
  // overhead. But to keep continuous yielding from starving the host event loop
  // (see YIELD_MACROTASK_MS), force a macrotask hop (setTimeout) at least that
  // often. The hop is accounted as pendingHost so quiescence/await detection
  // does not treat the yielding task as finished while the timer is in flight.
  function suspendYield() {
    const caller = s.current;
    return new Promise((resolve) => {
      const resume = () => {
        s.runnable.push({ kind: "resume", id: caller, fire: () => resolve() });
        schedule();
      };
      const now = Date.now();
      if (now - s.lastYieldMacrotask >= YIELD_MACROTASK_MS) {
        s.lastYieldMacrotask = now;
        s.pendingHost++;
        setTimeout(() => {
          s.pendingHost--;
          resume();
        }, 0);
      } else {
        resume();
      }
    });
  }

  function schedulerAwareHost(caller, op) {
    s.pendingHost++;
    const p = new Promise((resolve, reject) => {
      Promise.resolve()
        .then(op)
        .then(
          (value) => {
            s.pendingHost--;
            s.runnable.push({ kind: "resume", id: caller, fire: () => resolve(value) });
            schedule();
          },
          (err) => {
            s.pendingHost--;
            s.runnable.push({ kind: "resume", id: caller, fire: () => reject(asError(err)) });
            schedule();
          },
        );
    });
    schedule();
    return p;
  }

  function wrapHostSuspending(op) {
    return (...args) => {
      const caller = s.current;
      return schedulerAwareHost(caller, () => op(...args));
    };
  }

  function enqueueResume(id, resolve, value) {
    s.runnable.push({ kind: "resume", id, fire: () => resolve(value) });
  }

  function wakeChannelWaiter(waiter, value) {
    s.blockedOnChannel--;
    enqueueResume(waiter.id, waiter.resolve, value);
  }

  function requireChannel(id) {
    const cid = Number(id);
    const ch = s.channels.get(cid);
    if (!ch) throw new Error("Channel: invalid channel id " + cid);
    return ch;
  }

  function channelNew() {
    const id = s.nextChannelId++;
    s.channels.set(id, { capacity: 0, buffer: [], sendQ: [], recvQ: [], closed: false });
    return id;
  }

  function channelBounded(capacity) {
    const cap = Number(capacity);
    if (!Number.isInteger(cap) || cap < 1) {
      throw new Error("Channel.bounded: capacity must be >= 1");
    }
    const id = s.nextChannelId++;
    s.channels.set(id, { capacity: cap, buffer: [], sendQ: [], recvQ: [], closed: false });
    return id;
  }

  function channelSend(id, value) {
    const ch = requireChannel(id);
    if (ch.closed) return false;

    const receiver = ch.recvQ.shift();
    if (receiver) {
      wakeChannelWaiter(receiver, { kind: "value", value });
      return true;
    }

    if (ch.capacity > 0 && ch.buffer.length < ch.capacity) {
      ch.buffer.push(value);
      return true;
    }

    const caller = s.current;
    s.blockedOnChannel++;
    const p = new Promise((resolve) => {
      ch.sendQ.push({ id: caller, value, resolve });
    });
    schedule();
    return p;
  }

  function channelRecv(id) {
    const ch = requireChannel(id);

    if (ch.buffer.length > 0) {
      const value = ch.buffer.shift();
      const sender = ch.sendQ.shift();
      if (sender) {
        if (ch.closed) {
          wakeChannelWaiter(sender, false);
        } else {
          ch.buffer.push(sender.value);
          wakeChannelWaiter(sender, true);
        }
      }
      return { kind: "value", value };
    }

    const sender = ch.sendQ.shift();
    if (sender) {
      wakeChannelWaiter(sender, true);
      return { kind: "value", value: sender.value };
    }

    if (ch.closed) return { kind: "closed" };

    const caller = s.current;
    s.blockedOnChannel++;
    const p = new Promise((resolve) => {
      ch.recvQ.push({ id: caller, resolve });
    });
    schedule();
    return p;
  }

  function channelClose(id) {
    const ch = requireChannel(id);
    if (ch.closed) return;
    ch.closed = true;

    for (;;) {
      const receiver = ch.recvQ.shift();
      if (!receiver) break;
      wakeChannelWaiter(receiver, { kind: "closed" });
    }
    for (;;) {
      const sender = ch.sendQ.shift();
      if (!sender) break;
      wakeChannelWaiter(sender, false);
    }
  }

  function channelRecvIsValue(result) {
    return result?.kind === "value" ? 1 : 0;
  }

  function channelRecvValue(result) {
    if (result?.kind !== "value") throw new Error("Channel.recv: closed result has no value");
    return result.value;
  }

  s.imports = {
    task_create: taskCreate,
    suspend_await: new WebAssembly.Suspending(suspendAwait),
    suspend_yield: new WebAssembly.Suspending(suspendYield),
    channel_new: channelNew,
    channel_bounded: channelBounded,
    channel_send: new WebAssembly.Suspending(channelSend),
    channel_recv: new WebAssembly.Suspending(channelRecv),
    channel_recv_is_value: channelRecvIsValue,
    channel_recv_value: channelRecvValue,
    channel_close: channelClose,
  };

  s.wrapHostSuspending = (op) => new WebAssembly.Suspending(wrapHostSuspending(op));

  s.onTopLevelComplete = () => onComplete(TOP, undefined);
  s.onTopLevelFail = (e) => onFail(TOP, e);
  s.kick = () => schedule();
  return s;
}

/** True if the module imports any operation from the "task" module. */
function moduleNeedsTasks(wasmModule) {
  try {
    return WebAssembly.Module.imports(wasmModule).some((imp) => imp.module === "task");
  } catch {
    return false;
  }
}

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

  // Install the cooperative task scheduler before auto-bridging so the task
  // intrinsic imports are recognized as host-provided rather than treated as
  // unresolved externs. Only when the module actually imports task operations.
  const needsTasks = moduleNeedsTasks(mainModule);
  let scheduler = null;
  if (needsTasks && jspi) {
    scheduler = createTaskScheduler();
    hostImports.task = scheduler.imports;
  }

  autoBridgeExternImports(mainModule, hostImports, b, jspi, imports, externMeta);

  return { mainModule, hostImports, b, runtime, imports, externMeta, jspi, needsTasks, scheduler };
}

// ---------------------------------------------------------------------------
// Public API — synchronous
// ---------------------------------------------------------------------------

export function runWasmBytes(wasmBytes, opts = {}) {
  const { mainModule, hostImports, b, runtime, imports, externMeta, jspi } = prepareWasm(wasmBytes, opts);
  try {
    const instance = instantiateWithExternRetry(mainModule, hostImports, b, jspi, imports, externMeta);
    runtime.instance = instance;
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
  const { mainModule, hostImports, b, runtime, imports, externMeta, jspi, needsTasks, scheduler } = prepareWasm(wasmBytes, opts, { jspi: hasJspi });

  if (needsTasks && !hasJspi) {
    throw new Error(
      "Task concurrency requires a JSPI-capable runtime " +
      "(WebAssembly.Suspending/promising). This engine does not provide it.",
    );
  }

  if (hasJspi) {
    const suspendHost = needsTasks
      ? (op) => scheduler.wrapHostSuspending(op)
      : (op) => new WebAssembly.Suspending(op);

    hostImports.host.sleep = suspendHost(
      (ms) => new Promise((resolve) => setTimeout(resolve, Number(ms) > 0 ? Number(ms) : 0)),
    );

    // Wrap stdin reads as suspending imports so the event loop stays free while
    // Twinkle waits for LSP input. Keep chunk and timeout reads on the same
    // stream-based path; mixing process.stdin.read() with fs.readSync(0, ...)
    // can strand bytes in Node's stream buffer.
    hostImports.host.stdin_read_chunk = suspendHost(
      async (maxBytes) =>
        makeByteArray(b, await runtime.host.readStdinAsync(maxBytes, 2147483647, runtime)),
    );
    hostImports.host.stdin_read_timeout = suspendHost(
      async (maxBytes, timeoutMs) =>
        makeByteArray(b, await runtime.host.readStdinAsync(maxBytes, timeoutMs, runtime)),
    );

    // Wrap run_wasm as a suspending import so child programs can themselves use
    // JSPI suspending imports. In task-enabled programs this also preserves the
    // scheduler's single-resume discipline.
    const childBridgeBytes = opts.bridgeBytes;
    hostImports.host.run_wasm = suspendHost(
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
    const instance = instantiateWithExternRetry(mainModule, hostImports, b, jspi, imports, externMeta);
    runtime.instance = instance;
    if (instance.exports.__twinkle_start) {
      if (needsTasks) {
        // Stackful task path: drive top-level (pseudo-task 0) and the spawned
        // task bodies through the cooperative scheduler.
        scheduler.promisingTaskRun = WebAssembly.promising(instance.exports.__task_run);
        const start = WebAssembly.promising(instance.exports.__twinkle_start);
        scheduler.current = 0;
        start().then(scheduler.onTopLevelComplete, scheduler.onTopLevelFail);
        scheduler.kick();
        await scheduler.done;
      } else if (hasJspi) {
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
