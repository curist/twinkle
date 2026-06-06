// Node/Deno host adapter for the shared Twinkle runtime.
//
// runtime.mjs is host-agnostic: it routes every filesystem and stdin host
// import through an injected `host` object. This module provides that object
// for Node and Deno (Deno satisfies node:fs / node:path via its node-compat
// layer). A browser supplies its own adapter with the same shape.
//
// Host interface:
//   resolvePath(cwd, p) -> string
//   readFile(path)      -> Uint8Array   (throws on missing)
//   writeFile(path, text)
//   writeBytes(path, bytes: Uint8Array)
//   exists(path)        -> boolean
//   listDir(path)       -> string[]
//   mkdirp(path)
//   readStdin(maxBytes, timeoutMs, runtime)      -> Uint8Array   (sync)
//   readStdinAsync(maxBytes, timeoutMs, runtime) -> Promise<Uint8Array>
//
// The stdin helpers set runtime.stdinEof when the stream reaches EOF.

import { readFileSync, writeFileSync, existsSync, readdirSync, mkdirSync, readSync } from "node:fs";
import { resolve } from "node:path";

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
// Adapter
// ---------------------------------------------------------------------------

export const nodeHost = {
  resolvePath(cwd, p) { return resolve(cwd, p); },
  readFile(path) { return readFileSync(path); },
  writeFile(path, text) { writeFileSync(path, text); },
  writeBytes(path, bytes) { writeFileSync(path, bytes); },
  exists(path) { return existsSync(path); },
  listDir(path) { return readdirSync(path); },
  mkdirp(path) { mkdirSync(path, { recursive: true }); },
  readStdin: readStdinTimeout,
  readStdinAsync: readStdinTimeoutAsync,
};
