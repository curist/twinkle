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

test("web run falls back to extern metadata when import introspection is unavailable", async () => {
  const originalImports = WebAssembly.Module.imports;
  WebAssembly.Module.imports = () => { throw new Error("import introspection unavailable"); };
  try {
    const result = await command(["run", "/input/main.tw"], {
      source: "extern Math fn floor(x: Float) Float\nprintln(Math.floor(2.7).to_string())\n",
      env: { NO_COLOR: "1" },
    });

    assert.equal(result.exitCode, 0);
    assert.match(result.stdout, /2/);
  } finally {
    WebAssembly.Module.imports = originalImports;
  }
});

test("web run retries missing externs when all import metadata APIs are unavailable", async () => {
  const originalImports = WebAssembly.Module.imports;
  const originalCustomSections = WebAssembly.Module.customSections;
  WebAssembly.Module.imports = () => { throw new Error("import introspection unavailable"); };
  WebAssembly.Module.customSections = () => { throw new Error("custom sections unavailable"); };
  try {
    const result = await command(["run", "/input/main.tw"], {
      source: "extern performance fn now() Float\nprintln((performance.now() >= 0.0).to_string())\n",
      env: { NO_COLOR: "1" },
    });

    assert.equal(result.exitCode, 0);
    assert.match(result.stdout, /true/);
  } finally {
    WebAssembly.Module.imports = originalImports;
    WebAssembly.Module.customSections = originalCustomSections;
  }
});
