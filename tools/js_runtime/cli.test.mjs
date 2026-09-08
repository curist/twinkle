import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { runWasmBytesAsync, loadLibBytes } from "./runtime.mjs";
import { nodeHost } from "./node_host.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const entry = join(here, "node_main.mjs");
const repoRoot = join(here, "..", "..");

test("twk CLI runs a Twinkle program", () => {
  const out = execFileSync("node", [entry, "run", join(repoRoot, "examples", "fizzbuzz.tw")], {
    encoding: "utf8",
  });
  assert.match(out, /Fizz/);
});

// Build the renderer lib into `dir` so tests do not depend on a stray
// target/renderer.wasm being present, and return its path for RENDERER_WASM.
function buildRenderer(dir) {
  const rendererPath = join(dir, "renderer.wasm");
  execFileSync(
    "node",
    [entry, "build", join(repoRoot, "boot", "runtime_trace_renderer.tw"), "--lib", "-o", rendererPath],
    { encoding: "utf8" },
  );
  return rendererPath;
}

test("twk run renders a full source snippet for a trap in user code (file present on disk)", () => {
  // Divide-by-zero traps directly on the `a / b` instruction inside the
  // user's own function — no prelude frame sits between the trap and the
  // user's source, so the primary frame resolves to a real, on-disk file and
  // the renderer's disk-backed snippet path (build_disk_registry / snippet_ok
  // in boot/lib/debug/trace.tw) renders a full snippet + caret.
  const root = mkdtempSync(join(tmpdir(), "twk-trace-"));
  try {
    const rendererPath = buildRenderer(root);

    const trapPath = join(root, "trap.tw");
    writeFileSync(
      trapPath,
      "fn divide(a: Int, b: Int) Int {\n  a / b\n}\n\nx := divide(10, 0)\nprintln(\"never ${x}\")\n",
    );

    let status = 0;
    let stderr = "";
    try {
      execFileSync("node", [entry, "run", trapPath], {
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
        env: { ...process.env, RENDERER_WASM: rendererPath },
      });
    } catch (e) {
      status = e.status ?? 1;
      stderr = e.stderr?.toString() ?? "";
    }

    assert.notEqual(status, 0);
    // The trap message headline, a source location + caret into the user
    // file, and a backtrace frame — printed exactly once.
    assert.match(stderr, /error: divide by zero/);
    assert.match(stderr, /trap\.tw:2:\d+/);
    assert.match(stderr, /\^\^/);
    assert.match(stderr, /a \/ b/);
    assert.match(stderr, /at .*\(.*trap\.tw:/);
    assert.equal(stderr.match(/error: divide by zero/g).length, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("twk run degrades to a location-only trace when the trap surfaces through the prelude error() shim", () => {
  // `error(...)` is itself a prelude function (@std prelude/io.tw), so the
  // innermost resolved frame is a `/__twinkle_core/...` logical path that
  // never exists on disk — the renderer's documented degrade path (choosing
  // the first resolved frame as primary, `@std`/prelude paths "never resolve
  // to a real file" per docs/plans/disk-backed-debug-info.md). Preferring the
  // nearest *user* frame instead is explicitly deferred rendering polish
  // (runtime-stack-traces.md Phase 4, "prelude-frame suppression") — a
  // separate, later change, not part of this milestone.
  const root = mkdtempSync(join(tmpdir(), "twk-trace-"));
  try {
    const rendererPath = buildRenderer(root);

    const trapPath = join(root, "trap.tw");
    writeFileSync(
      trapPath,
      'fn boom(n: Int) Int {\n  if n <= 0 { error("kaboom") }\n  boom(n - 1)\n}\n\nx := boom(2)\nprintln("never ${x}")\n',
    );

    let status = 0;
    let stderr = "";
    try {
      execFileSync("node", [entry, "run", trapPath], {
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
        env: { ...process.env, RENDERER_WASM: rendererPath },
      });
    } catch (e) {
      status = e.status ?? 1;
      stderr = e.stderr?.toString() ?? "";
    }

    assert.notEqual(status, 0);
    assert.match(stderr, /error: kaboom/);
    assert.match(stderr, /at .*\(.*trap\.tw:/);
    assert.match(stderr, /source unavailable/);
    assert.equal(stderr.includes("^^"), false);
    assert.equal(stderr.match(/error: kaboom/g).length, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

// Minimal writable stream collector for the direct-runtime test below: no
// child process is spawned, so stdout/stderr are plain in-memory sinks rather
// than pipes read back from a subprocess.
function collector() {
  const state = { text: "" };
  state.stream = {
    write(chunk) {
      state.text += typeof chunk === "string" ? chunk : chunk.toString();
      return true;
    },
  };
  return state;
}

test("running a pre-built artifact directly degrades to a location-only trace once its source is gone", async () => {
  // This exercises the disk-backed-debug-info degradation path: `twinkle.debug`
  // (v2) carries absolute paths + line/col but no embedded source text, so a
  // compiled artifact run away from its original source can only recover
  // `file:line:col` + backtrace, not a snippet. Reuses the divide-by-zero
  // fixture (traps directly in user code, not through the prelude `error()`
  // shim) so this isolates the disk-unavailable degrade path specifically,
  // distinct from the separate prelude-frame case above. Unlike the tests
  // above, this goes straight through tools/js_runtime's runtime.mjs (no
  // `twk run` recompile of the fixture) to mirror "run a pre-built .wasm"
  // directly.
  const srcRoot = mkdtempSync(join(tmpdir(), "twk-artifact-src-"));
  const outRoot = mkdtempSync(join(tmpdir(), "twk-artifact-out-"));
  try {
    const rendererPath = buildRenderer(outRoot);

    const fixturePath = join(srcRoot, "trap.tw");
    writeFileSync(
      fixturePath,
      "fn divide(a: Int, b: Int) Int {\n  a / b\n}\n\nx := divide(10, 0)\nprintln(\"never ${x}\")\n",
    );

    const artifactPath = join(outRoot, "trap.wasm");
    execFileSync("node", [entry, "build", fixturePath, "-o", artifactPath], { encoding: "utf8" });

    const artifactBytes = new Uint8Array(readFileSync(artifactPath));
    const rendererBytes = new Uint8Array(readFileSync(rendererPath));

    // Delete the source directory entirely so the disk registry's read of the
    // (still-absolute) embedded path fails at render time — the point of this
    // case. The artifact itself lives under outRoot, untouched.
    rmSync(srcRoot, { recursive: true, force: true });

    const out = collector();
    const err = collector();

    const exitCode = await runWasmBytesAsync(artifactBytes, {
      programPath: artifactPath,
      guestArgs: [],
      cwd: outRoot,
      env: process.env,
      stdout: out.stream,
      stderr: err.stream,
      host: nodeHost,
      imports: {},
      // Render the trap ourselves rather than going through `twk run`'s
      // compile step (which is what "no compile" rules out): load the
      // renderer lib and call the same 3-arg entry point the CLI's own
      // childTrapHandler uses.
      childTrapHandler: async (trapInfo, childBytes) => {
        const lib = await loadLibBytes(rendererBytes, {
          programPath: "<renderer>.wasm",
          guestArgs: [],
          cwd: outRoot,
          env: process.env,
          stdout: { write: () => true },
          stderr: { write: () => true },
          host: nodeHost,
          imports: {},
        });
        const rendered = lib.render_runtime_trace(childBytes, trapInfo.stack, trapInfo.message);
        err.stream.write(rendered + "\n");
        return 1;
      },
    });

    assert.equal(exitCode, 1);
    assert.match(err.text, /error: divide by zero/);
    assert.match(err.text, /trap\.tw:2:\d+/);
    assert.match(err.text, /at .*\(.*trap\.tw:/);
    assert.match(err.text, /source unavailable/);
    // No snippet: neither the caret underline nor the source line text itself.
    assert.equal(err.text.includes("^^"), false);
    assert.equal(err.text.includes("a / b"), false);
  } finally {
    rmSync(srcRoot, { recursive: true, force: true });
    rmSync(outRoot, { recursive: true, force: true });
  }
});

// Run `twk <args...>` inside a project directory. Returns { status, stdout,
// stderr } without throwing so rejection paths (nonzero exit) can be asserted.
function twk(cwd, args) {
  try {
    // Pipe stderr (rather than the default inherit) so the negative-path tests
    // below can assert on it without the child's diagnostics leaking into the
    // test runner's own output.
    const stdout = execFileSync("node", [entry, ...args], {
      cwd,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
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
