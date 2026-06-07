import { test } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { compile, runFile } from "./index.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const fix = (name) => join(here, "fixtures", name);

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
