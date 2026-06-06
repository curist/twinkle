import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const entry = join(here, "node_main.mjs");
const repoRoot = join(here, "..", "..");

test("twk CLI runs a Twinkle program", () => {
  const out = execFileSync("node", [entry, "run", join(repoRoot, "examples", "fizzbuzz.tw")], {
    encoding: "utf8",
  });
  assert.match(out, /Fizz/);
});
