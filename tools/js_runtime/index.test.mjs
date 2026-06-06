import { test } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { compile, runFile } from "./index.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const fix = (name) => join(here, "fixtures", name);

test("compile returns wasm bytes", async () => {
  const wasm = await compile(fix("scoped_extern.tw"));
  assert.ok(wasm instanceof Uint8Array);
  assert.deepEqual([...wasm.slice(0, 4)], [0x00, 0x61, 0x73, 0x6d]); // "\0asm"
});

test("concurrent compiles do not collide on output path", async () => {
  const [a, b] = await Promise.all([
    compile(fix("scoped_extern.tw")),
    compile(fix("global_extern.tw")),
  ]);
  assert.deepEqual([...a.slice(0, 4)], [0x00, 0x61, 0x73, 0x6d]);
  assert.deepEqual([...b.slice(0, 4)], [0x00, 0x61, 0x73, 0x6d]);
});

test("scoped imports receive marshaled values", async () => {
  const seen = [];
  const code = await runFile(fix("scoped_extern.tw"), {
    imports: { host_app: { emit: (msg) => { seen.push(msg); } } },
  });
  assert.equal(code, 0);
  assert.deepEqual(seen, ["hello from twinkle"]);
});

test("missing import throws naming the symbol", async () => {
  await assert.rejects(
    () => runFile(fix("scoped_extern.tw")), // no imports; host_app not global
    /Missing host import\(s\): host_app\.emit/,
  );
});

test("host globals auto-resolve without wiring", async () => {
  const code = await runFile(fix("global_extern.tw"));
  assert.equal(code, 0);
});

test("imports shadow globals", async () => {
  const lines = [];
  const code = await runFile(fix("global_extern.tw"), {
    imports: { console: { log: (m) => lines.push(m) } },
  });
  assert.equal(code, 0);
  assert.equal(lines.length, 1);
  assert.match(lines[0], /sqrt 16 = 4/);
});
