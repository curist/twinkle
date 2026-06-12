import { test, before } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { command } from "./web.mjs";

before(() => {
  globalThis.fetch = async (url) => {
    const buf = await readFile(fileURLToPath(url));
    return {
      arrayBuffer: async () => buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength),
    };
  };
});

test("web command formats source in the memory host", async () => {
  const result = await command(["fmt", "/input/main.tw"], {
    source: "x:=1\nprintln(x)\n",
    env: { NO_COLOR: "1" },
  });

  assert.equal(result.exitCode, 0);
  assert.equal(result.text("/input/main.tw"), "x := 1\nprintln(x)\n");
  assert.match(result.stdout, /Formatted: \/input\/main\.tw/);
});

test("web command returns non-zero compiler exits instead of throwing", async () => {
  const result = await command(["check", "/input/main.tw"], {
    source: "x := \n",
    env: { NO_COLOR: "1" },
  });

  assert.notEqual(result.exitCode, 0);
  assert.match(result.stderr, /expected expression/);
});
