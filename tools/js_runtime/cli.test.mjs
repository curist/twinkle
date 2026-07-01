import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const entry = join(here, "node_main.mjs");
const repoRoot = join(here, "..", "..");

test("twk CLI runs a Twinkle program", () => {
  const out = execFileSync("node", [entry, "run", join(repoRoot, "examples", "fizzbuzz.tw")], {
    encoding: "utf8",
  });
  assert.match(out, /Fizz/);
});

// Run `twk <args...>` inside a project directory. Returns { status, stdout,
// stderr } without throwing so rejection paths (nonzero exit) can be asserted.
function twk(cwd, args) {
  try {
    const stdout = execFileSync("node", [entry, ...args], { cwd, encoding: "utf8" });
    return { status: 0, stdout, stderr: "" };
  } catch (e) {
    return { status: e.status ?? 1, stdout: e.stdout?.toString() ?? "", stderr: e.stderr?.toString() ?? "" };
  }
}

// Scaffold a minimal lib project in a fresh temp dir and return its root.
function makeLibProject(toml, files) {
  const root = mkdtempSync(join(tmpdir(), "twk-build-"));
  writeFileSync(join(root, "twinkle.toml"), toml);
  for (const [name, content] of Object.entries(files)) {
    writeFileSync(join(root, name), content);
  }
  return root;
}

const ADD_LIB = "pub fn add(a: Int, b: Int) Int { a + b }\n";

test("twk build --lib writes the grouped raw wasm", () => {
  const root = makeLibProject('[lib]\nentry = "demo.tw"\n', { "demo.tw": ADD_LIB });
  try {
    const r = twk(root, ["build", "--lib"]);
    assert.equal(r.status, 0, r.stderr);
    assert.ok(existsSync(join(root, "target", "demo", "demo.lib.wasm")));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("twk build --node emits a bundle with a copied wasm and pinned package.json", () => {
  const root = makeLibProject('[lib]\nentry = "demo.tw"\n', { "demo.tw": ADD_LIB });
  try {
    const r = twk(root, ["build", "--node"]);
    assert.equal(r.status, 0, r.stderr);
    const nodeDir = join(root, "target", "demo", "node");
    assert.ok(existsSync(join(nodeDir, "demo.lib.wasm")), "wasm copied into bundle");
    assert.ok(existsSync(join(nodeDir, "main.mjs")));
    const pkg = readFileSync(join(nodeDir, "package.json"), "utf8");
    assert.match(pkg, /@twinkle-lang\/twinkle/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("twk build rejects -o combined with --node", () => {
  const root = makeLibProject('[lib]\nentry = "demo.tw"\n', { "demo.tw": ADD_LIB });
  try {
    const r = twk(root, ["build", "--node", "-o", "out.wasm"]);
    assert.equal(r.status, 1);
    assert.match(r.stderr, /-o\/--output cannot be combined with --node\/--web/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("twk build --all builds every lib entry; bare build is ambiguous", () => {
  const root = makeLibProject('[lib]\nentries = ["math.tw", "text.tw"]\n', {
    "math.tw": ADD_LIB,
    "text.tw": "pub fn twice(n: Int) Int { n * 2 }\n",
  });
  try {
    const ambiguous = twk(root, ["build", "--lib"]);
    assert.equal(ambiguous.status, 1);
    assert.match(ambiguous.stderr, /multiple lib entries/);

    const all = twk(root, ["build", "--lib", "--all"]);
    assert.equal(all.status, 0, all.stderr);
    assert.ok(existsSync(join(root, "target", "math", "math.lib.wasm")));
    assert.ok(existsSync(join(root, "target", "text", "text.lib.wasm")));

    const one = twk(root, ["build", "--lib", "--target", "text"]);
    assert.equal(one.status, 0, one.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("twk build regeneration preserves unrelated bundle files", () => {
  const root = makeLibProject('[lib]\nentry = "demo.tw"\n', { "demo.tw": ADD_LIB });
  try {
    assert.equal(twk(root, ["build", "--node"]).status, 0);

    // Simulate a user's install: a lockfile and a node_modules dir.
    const nodeDir = join(root, "target", "demo", "node");
    writeFileSync(join(nodeDir, "package-lock.json"), "{}\n");
    mkdirSync(join(nodeDir, "node_modules"));
    writeFileSync(join(nodeDir, "node_modules", "sentinel"), "keep\n");

    assert.equal(twk(root, ["build", "--node"]).status, 0, "rebuild");

    assert.ok(existsSync(join(nodeDir, "package-lock.json")), "lockfile survives");
    assert.ok(existsSync(join(nodeDir, "node_modules", "sentinel")), "node_modules survives");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
