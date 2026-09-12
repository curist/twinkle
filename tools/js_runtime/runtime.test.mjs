import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { resolveExternImports, instantiateBridge } from "./runtime.mjs";
import { bridgeBytes } from "./bridge_bytes.mjs";
import { compile, loadLib, run } from "./index.mjs";

const here = dirname(fileURLToPath(import.meta.url));

test("embedded bridge bytes match tools/bridge.wasm (guard against a stale embed)", () => {
  const onDisk = readFileSync(join(here, "..", "bridge.wasm"));
  assert.equal(
    Buffer.compare(Buffer.from(bridgeBytes), onDisk),
    0,
    "bridge_bytes.mjs is stale; regenerate with `node tools/generate_bridge_bytes.mjs`",
  );
  // The embedded module must be instantiable on its own (it imports nothing).
  new WebAssembly.Instance(new WebAssembly.Module(bridgeBytes));
});

test("scoped imports win over globals", () => {
  const scopedFn = () => "scoped";
  const globalFn = () => "global";
  const { found, missing } = resolveExternImports(
    [{ module: "m", name: "f", kind: "function" }],
    {},
    { m: { f: scopedFn } },
    { m: { f: globalFn } },
  );
  assert.deepEqual(missing, []);
  assert.equal(found.length, 1);
  assert.equal(found[0].fn, scopedFn);
  assert.equal(found[0].recv.f, scopedFn);
});

test("falls back to globals when not scoped", () => {
  const globalFn = () => 1;
  const { found, missing } = resolveExternImports(
    [{ module: "Math", name: "sqrt", kind: "function" }],
    {},
    {},
    { Math: { sqrt: globalFn } },
  );
  assert.deepEqual(missing, []);
  assert.equal(found[0].fn, globalFn);
});

test("aggregates missing imports", () => {
  const { found, missing } = resolveExternImports(
    [
      { module: "a", name: "x", kind: "function" },
      { module: "a", name: "y", kind: "function" },
    ],
    {},
    {},
    {},
  );
  assert.equal(found.length, 0);
  assert.deepEqual(missing, ["a.x", "a.y"]);
});

test("skips already-provided host imports", () => {
  const { found, missing } = resolveExternImports(
    [{ module: "host", name: "print", kind: "function" }],
    { host: { print: () => {} } },
    {},
    {},
  );
  assert.deepEqual(missing, []);
  assert.equal(found.length, 0);
});

test("skips non-function imports", () => {
  const { found, missing } = resolveExternImports(
    [{ module: "env", name: "memory", kind: "memory" }],
    {},
    {},
    {},
  );
  assert.deepEqual(missing, []);
  assert.equal(found.length, 0);
});

test("bridge round-trips boxed int and float", () => {
  const b = instantiateBridge();
  const bi = b.boxed_int_new(9007199254740993n); // > 2^53, must stay exact
  assert.equal(b.boxed_int_get(bi), 9007199254740993n);
  const bf = b.boxed_float_new(3.5);
  assert.equal(b.boxed_float_get(bf), 3.5);
});

test("loadLib exposes primitive and String pub exports and skips ineligible ones", async () => {
  const src = [
    "pub fn add(a: Int, b: Int) Int {",
    "  a + b",
    "}",
    "",
    "pub fn is_positive(n: Int) Bool {",
    "  n > 0",
    "}",
    "",
    "pub pi: Float = 3.14159",
    "",
    // String args and returns cross the boundary via the embedded bridge.
    "pub fn greet(name: String) String {",
    "  \"hello, ${name}\"",
    "}",
    "",
    "pub greeting: String = \"hi\"",
    "",
    // Non-pub functions are never exported.
    "fn secret() Int {",
    "  42",
    "}",
  ].join("\n");

  const wasm = await compile({ source: src }, { lib: true });
  const lib = await loadLib(wasm);

  // Int args accept plain numbers; Int returns come back as BigInt (no downcast).
  assert.equal(lib.add(2, 3), 5n);
  // Bool round-trips as a JS boolean.
  assert.equal(lib.is_positive(5), true);
  assert.equal(lib.is_positive(-1), false);
  // Value globals are read once after start and exposed as a property.
  assert.ok(Math.abs(lib.pi - 3.14159) < 1e-9);
  // String args (JS string → guest String) and returns (guest String → JS string).
  assert.equal(lib.greet("world"), "hello, world");
  // A String value global reads back as a plain JS string.
  assert.equal(lib.greeting, "hi");
  // Non-pub members are absent from the surface.
  assert.equal(lib.secret, undefined);
});

test("loadLib round-trips Vector args and returns", async () => {
  const src = [
    "pub fn dbl(xs: Vector<Int>) Vector<Int> {",
    "  collect x in xs { x * 2 }",
    "}",
    "pub fn shout(ws: Vector<String>) Vector<String> {",
    "  collect w in ws { \"${w}!\" }",
    "}",
  ].join("\n");
  const lib = await loadLib(await compile({ source: src }, { lib: true }));
  assert.deepEqual(lib.dbl([1n, 2n, 3n]), [2n, 4n, 6n]);
  assert.deepEqual(lib.shout(["a", "b"]), ["a!", "b!"]);
});

test("loadLib round-trips Vector<Byte> args and returns via bulk copy", async () => {
  const src = [
    "pub fn total(bytes: Vector<Byte>) Int {",
    "  sum := 0",
    "  for b in bytes { sum = sum + b.to_int() }",
    "  sum",
    "}",
    "pub fn echo(bytes: Vector<Byte>) Vector<Byte> {",
    "  bytes",
    "}",
  ].join("\n");
  const lib = await loadLib(await compile({ source: src }, { lib: true }));
  // Accepts a Uint8Array as the Vector<Byte> argument.
  assert.equal(lib.total(new Uint8Array([1, 2, 3, 250])), 256n);
  // A returned Vector<Byte> decodes to a Uint8Array preserving the bytes.
  const out = lib.echo(new Uint8Array([0, 127, 255]));
  assert.ok(out instanceof Uint8Array);
  assert.deepEqual(Array.from(out), [0, 127, 255]);
});

test("loadLib round-trips a record", async () => {
  const src = [
    "pub type Pt = .{ x: Int, y: Int }",
    "pub fn mk(a: Int, b: Int) Pt { Pt.{ x: a, y: b } }",
    "pub fn swap(p: Pt) Pt { Pt.{ x: p.y, y: p.x } }",
  ].join("\n");
  const lib = await loadLib(await compile({ source: src }, { lib: true }));
  assert.deepEqual(lib.mk(1n, 2n), { x: 1n, y: 2n });
  assert.deepEqual(lib.swap({ x: 3n, y: 4n }), { x: 4n, y: 3n });
});

test("loadLib round-trips a Dict", async () => {
  const src = [
    "pub fn inc_all(m: Dict<String, Int>) Dict<String, Int> {",
    "  out := m",
    "  for k, v in m { out[k] = v + 1 }",
    "  out",
    "}",
  ].join("\n");
  const lib = await loadLib(await compile({ source: src }, { lib: true }));
  assert.deepEqual(lib.inc_all({ a: 1n, b: 2n }), { a: 2n, b: 3n });
});

test("loadLib round-trips nested compounds", async () => {
  const src = [
    "pub type Row = .{ id: Int, tags: Vector<String> }",
    "pub fn rows() Vector<Row> {",
    "  collect i in range(2) { Row.{ id: i, tags: [\"t${i}\"] } }",
    "}",
    "pub fn group(xs: Vector<Int>) Dict<String, Vector<Int>> {",
    "  out: Dict<String, Vector<Int>> = Dict.new()",
    "  out[\"all\"] = xs",
    "  out",
    "}",
  ].join("\n");
  const lib = await loadLib(await compile({ source: src }, { lib: true }));
  assert.deepEqual(lib.rows(), [
    { id: 0n, tags: ["t0"] },
    { id: 1n, tags: ["t1"] },
  ]);
  assert.deepEqual(lib.group([1n, 2n, 3n]), { all: [1n, 2n, 3n] });
});

test("loadLib returns a callable closure", async () => {
  const src = [
    "pub fn adder(n: Int) fn(Int) Int {",
    "  fn(x: Int) { x + n }",
    "}",
    "pub fn greeter(prefix: String) fn(String) String {",
    "  fn(name: String) { \"${prefix}${name}\" }",
    "}",
  ].join("\n");
  const lib = await loadLib(await compile({ source: src }, { lib: true }));
  const add5 = lib.adder(5n);
  assert.equal(add5(10n), 15n);
  assert.equal(add5(1n), 6n);
  const hi = lib.greeter("hi, ");
  assert.equal(hi("bob"), "hi, bob");
});

test("loadLib drives host callbacks (Void and value-returning)", async () => {
  const src = [
    "pub fn each_word(text: String, f: fn(String) Void) Void {",
    "  for w in text.split(\" \") {",
    "    f(w)",
    "  }",
    "}",
    "",
    "pub fn transform(n: Int, f: fn(Int) Int) Int {",
    "  f(n)",
    "}",
  ].join("\n");
  const wasm = await compile({ source: src }, { lib: true });
  const lib = await loadLib(wasm);

  const seen = [];
  lib.each_word("a b c", (w) => seen.push(w));
  assert.deepEqual(seen, ["a", "b", "c"]);

  // Value-returning callback: JS return marshalled back into the guest.
  assert.equal(lib.transform(21n, (n) => n * 2n), 42n);
});

test("loadLib returned closures can call escaped host callbacks", async () => {
  const src = [
    "pub fn keep(f: fn(Int) Int) fn(Int) Int {",
    "  f",
    "}",
  ].join("\n");
  const lib = await loadLib(await compile({ source: src }, { lib: true }));

  const kept = lib.keep((n) => n + 1n);

  assert.equal(kept(41n), 42n);
});

test("task scheduler runs other tasks while a Promise-returning extern is pending", async () => {
  const src = [
    "extern host {",
    "  fn delay(ms: Int) Void",
    "}",
    "Task.spawn(fn() {",
    "  println(\"a\")",
    "  host.delay(20)",
    "  println(\"b\")",
    "})",
    "Task.spawn(fn() {",
    "  println(\"c\")",
    "})",
  ].join("\n");
  const wasm = await compile({ source: src });
  let stdout = "";

  await run(wasm, {
    stdout: { write(chunk) { stdout += chunk; return true; } },
    imports: {
      host: {
        delay: (ms) => new Promise((resolve) => setTimeout(resolve, Number(ms))),
      },
    },
  });

  assert.equal(stdout, "a\nc\nb\n");
});

test("try_await returns Ok with the value for a successful task", async () => {
  const src = [
    "t := Task.spawn(fn() Int { 21 * 2 })",
    "case t.try_await() {",
    "  .Ok(v) => println(\"ok ${v}\"),",
    "  .Err(msg) => println(\"err ${msg}\"),",
    "}",
  ].join("\n");
  const wasm = await compile({ source: src });
  let stdout = "";

  await run(wasm, {
    stdout: { write(chunk) { stdout += chunk; return true; } },
  });

  assert.equal(stdout, "ok 42\n");
});

test("try_await recovers a failed task as Err with the message", async () => {
  const src = [
    "t := Task.spawn(fn() Int { error(\"boom\") })",
    "case t.try_await() {",
    "  .Ok(v) => println(\"ok ${v}\"),",
    "  .Err(msg) => println(\"err ${msg}\"),",
    "}",
  ].join("\n");
  const wasm = await compile({ source: src });
  let stdout = "";
  let stderr = "";

  await run(wasm, {
    stdout: { write(chunk) { stdout += chunk; return true; } },
    stderr: { write(chunk) { stderr += chunk; return true; } },
  });

  assert.equal(stdout, "err boom\n");
  assert.equal(stderr, "");
});

test("try_await recovers a task runtime trap as Err", async () => {
  const src = [
    "t := Task.spawn(fn() Int {",
    "  xs := collect i in range(1) { i }",
    "  xs[2]",
    "})",
    "case t.try_await() {",
    "  .Ok(v) => println(\"ok ${v}\"),",
    "  .Err(msg) => println(\"err ${msg}\"),",
    "}",
  ].join("\n");
  const wasm = await compile({ source: src });
  let stdout = "";
  let stderr = "";

  await run(wasm, {
    stdout: { write(chunk) { stdout += chunk; return true; } },
    stderr: { write(chunk) { stderr += chunk; return true; } },
  });

  assert.match(stdout, /^err index 2 out of bounds for length 1\n$/);
  assert.equal(stderr, "");
});

test("try_await recovers a native Wasm trap as Err", async () => {
  const src = [
    "fn divide(a: Int, b: Int) Int { a / b }",
    "t := Task.spawn(fn() Int { divide(1, 0) })",
    "case t.try_await() {",
    "  .Ok(v) => println(\"ok ${v}\"),",
    "  .Err(msg) => println(\"err ${msg}\"),",
    "}",
  ].join("\n");
  const wasm = await compile({ source: src });
  let stdout = "";
  let stderr = "";

  await run(wasm, {
    stdout: { write(chunk) { stdout += chunk; return true; } },
    stderr: { write(chunk) { stderr += chunk; return true; } },
  });

  assert.match(stdout, /^err .*divide by zero\n$/);
  assert.equal(stderr, "");
});

test("auto-bridged extern Int arguments arrive as precise BigInts", async () => {
  const src = [
    "extern host {",
    "  fn take(n: Int) Void",
    "}",
    "host.take(9007199254740993)",
  ].join("\n");
  const wasm = await compile({ source: src });
  let seen;

  await run(wasm, {
    imports: {
      host: { take: (n) => { seen = n; } },
    },
  });

  assert.equal(typeof seen, "bigint");
  assert.equal(seen, 9007199254740993n);
});

test("Float.from_string rejects strings with trailing junk", async () => {
  const src = [
    "case Float.from_string(\"1x\") {",
    "  .Some(_) => println(\"some\"),",
    "  .None => println(\"none\"),",
    "}",
  ].join("\n");
  const wasm = await compile({ source: src });
  let stdout = "";

  await run(wasm, {
    stdout: { write(chunk) { stdout += chunk; return true; } },
  });

  assert.equal(stdout, "none\n");
});
