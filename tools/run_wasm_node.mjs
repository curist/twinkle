#!/usr/bin/env node
// Node.js / Bun runner for Twinkle-emitted Wasm GC modules.
//
// Usage:
//   node tools/run_wasm_node.mjs <file.wasm> [program args...]
//   bun  tools/run_wasm_node.mjs <file.wasm> [program args...]
//
// Provides the "host" imports that Twinkle's stage0 compiler emits, using a
// small bridge Wasm module to create/read Wasm GC values (since JS cannot
// directly construct or inspect Wasm GC arrays/structs).

import { readFileSync, writeFileSync, existsSync, readdirSync, mkdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

// ---------------------------------------------------------------------------
// Bridge module
// ---------------------------------------------------------------------------
// Provides encode/decode helpers for Twinkle's GC types.  Uses the same
// structural types as the compiled module — Wasm GC structural subtyping
// makes references interchangeable across module boundaries.
//
// Types mirrored:
//   $String  = (array (mut i8))
//   $Array   = (array (mut anyref))
//   $Variant = (struct (field $type_id i32)
//                      (field $variant_id i32)
//                      (field $payload (ref null $Array)))

const BRIDGE_WAT = `
(module
  (type $String  (array (mut i8)))
  (type $Array   (array (mut anyref)))
  (type $Variant (struct (field $type_id i32)
                         (field $variant_id i32)
                         (field $payload (ref null $Array))))

  ;; --- String helpers ---
  (func (export "string_len") (param (ref null $String)) (result i32)
    local.get 0  array.len)
  (func (export "string_get") (param $s (ref null $String)) (param $i i32) (result i32)
    local.get $s  local.get $i  array.get_u $String)
  (func (export "string_new") (param $len i32) (result (ref $String))
    i32.const 0  local.get $len  array.new $String)
  (func (export "string_set") (param $s (ref null $String)) (param $i i32) (param $v i32)
    local.get $s  local.get $i  local.get $v  array.set $String)

  ;; --- Array helpers (anyref tuple) ---
  (func (export "array_new") (param $len i32) (result (ref $Array))
    ref.null any  local.get $len  array.new $Array)
  (func (export "array_get") (param $a (ref null $Array)) (param $i i32) (result anyref)
    local.get $a  local.get $i  array.get $Array)
  (func (export "array_set") (param $a (ref null $Array)) (param $i i32) (param $v anyref)
    local.get $a  local.get $i  local.get $v  array.set $Array)
  (func (export "array_len") (param (ref null $Array)) (result i32)
    local.get 0  array.len)

  ;; --- Variant constructor ---
  (func (export "variant_new") (param $type_id i32) (param $variant_id i32) (param $payload (ref null $Array)) (result (ref $Variant))
    local.get $type_id  local.get $variant_id  local.get $payload
    struct.new $Variant)

  ;; --- i31 helpers (used for boxing bytes) ---
  (func (export "i31_new") (param $v i32) (result (ref i31))
    local.get $v  ref.i31)
  (func (export "i31_get") (param (ref i31)) (result i32)
    local.get 0  i31.get_s)
)`;

function compileBridgeWat() {
  try {
    return execFileSync("wasm-tools", ["parse", "-"], {
      input: BRIDGE_WAT,
      maxBuffer: 1024 * 1024,
    });
  } catch (e) {
    console.error(
      "Error: 'wasm-tools' is required to compile the bridge module.",
    );
    console.error("Install: cargo install wasm-tools  (or brew install wasm-tools)");
    process.exit(1);
  }
}

// ---------------------------------------------------------------------------
// String encode / decode
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

// ---------------------------------------------------------------------------
// Runtime value constructors
// ---------------------------------------------------------------------------
// Twinkle's Result<T,E> is encoded as a Variant with type_id for Result,
// variant_id 0 = Ok, 1 = Err, payload = single-element Array wrapping the value.

const RESULT_TYPE_ID = 1; // matches src/types/ty.rs RESULT_TYPE_ID
const RESULT_OK = 0;
const RESULT_ERR = 1;

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

// Encode bytes as an Array of i31-boxed values (matches Twinkle's byte representation)
function makeByteArray(b, bytes) {
  const arr = b.array_new(bytes.length);
  for (let i = 0; i < bytes.length; i++) {
    b.array_set(arr, i, b.i31_new(bytes[i]));
  }
  return arr;
}

// ---------------------------------------------------------------------------
// Host imports
// ---------------------------------------------------------------------------
function makeHostImports(b, programArgs, cwd) {
  return {
    host: {
      // --- I/O ---
      print: (s) => process.stdout.write(decodeString(b, s)),
      println: (s) => process.stdout.write(decodeString(b, s) + "\n"),
      error: (s) => {
        const msg = decodeString(b, s);
        process.stderr.write(msg + "\n");
        throw new Error("host.error: " + msg);
      },
      eprint: (s) => process.stderr.write(decodeString(b, s)),
      eprintln: (s) => process.stderr.write(decodeString(b, s) + "\n"),

      // --- String conversion ---
      f64_to_string: (n) => encodeString(b, n.toString()),

      // --- Process ---
      args: () => makeStringArray(b, programArgs),
      env: (keyRef) => {
        const key = decodeString(b, keyRef);
        const val = process.env[key];
        // Returns Array<String>: [value] if found, [] if not
        if (val === undefined) return makeStringArray(b, []);
        return makeStringArray(b, [val]);
      },
      cwd: () => encodeString(b, cwd),
      exit: (code) => {
        // Use BigInt conversion for i64 values
        const c = typeof code === "bigint" ? Number(code) : code;
        process.exit(c);
      },

      // --- File system ---
      read_file: (pathRef) => {
        const filePath = resolve(cwd, decodeString(b, pathRef));
        try {
          const bytes = readFileSync(filePath);
          const byteArr = makeByteArray(b, bytes);
          return makeResultOk(b, byteArr);
        } catch (e) {
          const msg = `host.read_file failed for '${filePath}': ${e.message}`;
          return makeResultErr(b, encodeString(b, msg));
        }
      },
      write_file: (pathRef, contentRef) => {
        const filePath = resolve(cwd, decodeString(b, pathRef));
        const content = decodeString(b, contentRef);
        writeFileSync(filePath, content);
      },
      write_bytes: (pathRef, bytesRef) => {
        const filePath = resolve(cwd, decodeString(b, pathRef));
        const len = b.array_len(bytesRef);
        const buf = Buffer.alloc(len);
        for (let i = 0; i < len; i++) {
          buf[i] = b.i31_get(b.array_get(bytesRef, i));
        }
        writeFileSync(filePath, buf);
      },
      mkdirp: (pathRef) => {
        const dirPath = resolve(cwd, decodeString(b, pathRef));
        mkdirSync(dirPath, { recursive: true });
      },
      list_dir: (pathRef) => {
        const dirPath = resolve(cwd, decodeString(b, pathRef));
        const entries = readdirSync(dirPath);
        return makeStringArray(b, entries);
      },
      exists: (pathRef) => {
        const filePath = resolve(cwd, decodeString(b, pathRef));
        return existsSync(filePath) ? 1 : 0;
      },

      // --- Parsing ---
      parse_int: (sRef) => {
        const s = decodeString(b, sRef);
        const n = parseInt(s, 10);
        return isNaN(n) ? 0n : BigInt(n);
      },
      // Returns (f64, i32) — multi-value: [value, success_flag]
      parse_float: (sRef) => {
        const s = decodeString(b, sRef);
        const f = parseFloat(s);
        return isNaN(f) ? [0.0, 0] : [f, 1];
      },
    },
  };
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------
async function main() {
  const args = process.argv.slice(2);
  if (args.length === 0) {
    console.error("Usage: node tools/run_wasm_node.mjs <file.wasm> [args...]");
    process.exit(1);
  }

  const wasmPath = resolve(args[0]);
  // Match twk run behavior: argv[0] is the program path, rest are user args
  const programArgs = [wasmPath, ...args.slice(1)];
  const cwd = process.cwd();

  const wasmBytes = readFileSync(wasmPath);
  const bridgeBytes = compileBridgeWat();

  const bridgeModule = await WebAssembly.compile(bridgeBytes);
  const bridgeInstance = await WebAssembly.instantiate(bridgeModule);
  const b = bridgeInstance.exports;

  const hostImports = makeHostImports(b, programArgs, cwd);
  const mainModule = await WebAssembly.compile(wasmBytes);

  // Verify imports if the runtime supports it (Bun/JSC may not)
  try {
    for (const imp of WebAssembly.Module.imports(mainModule)) {
      const mod = hostImports[imp.module];
      if (!mod || !(imp.name in mod)) {
        console.error(`Unsupported host import: ${imp.module}.${imp.name}`);
        process.exit(1);
      }
    }
  } catch {
    // Module.imports may fail on GC modules in some runtimes
  }

  await WebAssembly.instantiate(mainModule, hostImports);
}

main().catch((e) => {
  if (e.message?.startsWith("host.error:")) {
    process.exit(1);
  }
  console.error(e.message || e);
  process.exit(1);
});
