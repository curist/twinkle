import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { resolveExternImports } from "./runtime.mjs";
import { bridgeBytes } from "./bridge_bytes.mjs";

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
