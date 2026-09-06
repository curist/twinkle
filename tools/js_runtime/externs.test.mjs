import { test } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { compile, runFile } from "./index.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const fix = (name) => join(here, "fixtures", name);

function decodeFunctionNames(section) {
  const bytes = new Uint8Array(section);
  let offset = 0;
  const readU32 = () => {
    let value = 0;
    let shift = 0;
    while (true) {
      const byte = bytes[offset++];
      value |= (byte & 0x7f) << shift;
      if ((byte & 0x80) === 0) return value;
      shift += 7;
    }
  };
  const subsectionId = bytes[offset++];
  assert.equal(subsectionId, 1, "first name subsection describes functions");
  readU32();
  const count = readU32();
  const names = new Map();
  for (let i = 0; i < count; i++) {
    const index = readU32();
    const length = readU32();
    names.set(index, new TextDecoder().decode(bytes.subarray(offset, offset + length)));
    offset += length;
  }
  return names;
}

test("compiler emits the twinkle.externs section with per-arg kinds", async () => {
  const wasm = await compile(fix("extern_ref.tw"));
  const mod = new WebAssembly.Module(wasm);
  const sections = WebAssembly.Module.customSections(mod, "twinkle.externs");
  assert.equal(sections.length, 1);
  const meta = JSON.parse(new TextDecoder().decode(new Uint8Array(sections[0])));
  const byName = Object.fromEntries(meta.map((e) => [`${e.module}.${e.name}`, e]));

  assert.deepEqual(byName["canvas.get_context"], {
    module: "canvas", name: "get_context", args: ["str"], ret: "ref",
  });
  assert.deepEqual(byName["canvas.fill_rect"], {
    module: "canvas", name: "fill_rect", args: ["ref", "f64", "f64", "f64", "f64"], ret: "void",
  });
  assert.deepEqual(byName["probe.record"], {
    module: "probe", name: "record", args: ["str"], ret: "void",
  });
});

test("compiler emits the standard WebAssembly function name section", async () => {
  const wasm = await compile(fix("extern_ref.tw"));
  const mod = new WebAssembly.Module(wasm);
  const sections = WebAssembly.Module.customSections(mod, "name");

  assert.equal(sections.length, 1);
  const names = decodeFunctionNames(sections[0]);
  assert.match(names.get(0), /\$extern_/, "imported function occupies index zero");
  assert.ok(
    [...names.entries()].some(
      ([index, name]) => index > 0 && name.startsWith("user__") && !name.includes("$extern_"),
    ),
    "defined Twinkle functions follow imported functions in index space",
  );
});

test("compiler emits the twinkle.debug section with a versioned line program", async () => {
  const wasm = await compile(fix("extern_ref.tw"));
  const mod = new WebAssembly.Module(wasm);
  const sections = WebAssembly.Module.customSections(mod, "twinkle.debug");

  assert.equal(sections.length, 1);
  const u = new Uint8Array(sections[0]);
  let p = 0;
  const uleb = () => {
    let v = 0;
    let s = 0;
    let x;
    do {
      x = u[p++];
      v |= (x & 0x7f) << s;
      s += 7;
    } while (x & 0x80);
    return v;
  };

  const readStr = () => {
    const n = uleb();
    const s = Buffer.from(u.slice(p, p + n)).toString("utf8");
    p += n;
    return s;
  };

  assert.equal(u[p++], 1, "version byte");
  const fileCount = uleb();
  assert.ok(fileCount > 0, "file table carries the referenced files");
  const fileIds = new Set();
  for (let i = 0; i < fileCount; i++) {
    const id = uleb();
    p++; // flag byte
    readStr(); // path
    const src = readStr();
    assert.ok(src.length > 0, "inline source is present");
    fileIds.add(id);
  }

  const funcCount = uleb();
  assert.ok(funcCount > 0, "at least one function has debug info");

  let sawLine = false;
  for (let i = 0; i < funcCount; i++) {
    uleb(); // func_idx
    const bodyStart = uleb();
    assert.ok(bodyStart > 0 && bodyStart < wasm.length, "plausible body_start");
    const lineCount = uleb();
    for (let j = 0; j < lineCount; j++) {
      uleb(); // delta offset
      const fileId = uleb();
      uleb(); // span start
      uleb(); // span end
      assert.ok(fileIds.has(fileId), "line-program file_id is present in the file table");
      sawLine = true;
    }
  }
  assert.ok(sawLine, "at least one line-program entry maps an offset to a span");
});

test("runtime auto-marshals from the section: externref raw, strings decoded", async () => {
  const CTX = { tag: "the-real-ctx" };
  let drewWith;
  const labels = [];
  // Plain functions, no per-arg spec — the section drives marshaling.
  const code = await runFile(fix("extern_ref.tw"), {
    imports: {
      canvas: {
        get_context: (id) => {
          assert.equal(id, "2d"); // String arg decoded to a JS string
          return CTX;
        },
        fill_rect: (ctx, x, y, w, h) => {
          drewWith = ctx; // externref passed through untouched
          labels.push(`${x},${y},${w},${h}`);
        },
      },
      probe: { record: (l) => labels.push(l) },
    },
  });

  assert.equal(code, 0);
  assert.equal(drewWith, CTX); // same object, not decoded as a string
  assert.deepEqual(labels, ["1,2,3,4", "ok"]);
});
