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

test("twk run points the caret at the user's error() call site (not the prelude shim)", () => {
  // `error(...)` traps through the prelude `error` frame (an @std/... logical
  // path with no on-disk file), but its caller is user code. Phase 4.1 makes
  // the primary snippet/caret land on the first USER frame — the user's
  // `error("kaboom")` call site (trap.tw line 2) — with a full snippet, and
  // suppresses the stdlib frame from the backtrace. (This replaces the earlier
  // test that asserted the deferred location-only-on-prelude behavior.)
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
    // Primary snippet + caret on the user's error() call line (line 2), a real
    // file readable under the (no-manifest, entry-dir) project root.
    assert.match(stderr, /-->[^\n]*trap\.tw:2:/);
    assert.equal(stderr.includes("^^"), true);
    assert.doesNotMatch(stderr, /source unavailable/);
    // The prelude/stdlib frame is suppressed from the backtrace.
    assert.equal(stderr.includes("@std"), false);
    // Printed exactly once.
    assert.equal(stderr.match(/error: kaboom/g).length, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

// Runs `twk run <trap.tw>` for `src` in a fresh temp dir with the built
// renderer, returning { status, stderr }. Shared by the out-of-bounds cases.
function runTrap(src) {
  const root = mkdtempSync(join(tmpdir(), "twk-trace-"));
  try {
    const rendererPath = buildRenderer(root);
    const trapPath = join(root, "trap.tw");
    writeFileSync(trapPath, src);

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

    return { status, stderr };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test("twk run renders a rich out-of-bounds message for an indexed read", () => {
  // Reading past the end of a length-3 vector traps with the rich
  // `index N out of bounds for length L` message (from the emit-site
  // __panic_oob guard) plus a source-mapped snippet + caret on the xs[i] site.
  const { status, stderr } = runTrap('xs := [10, 20, 30]\nprintln(xs[5])\n');
  assert.notEqual(status, 0);
  assert.match(stderr, /index 5 out of bounds for length 3/);
  assert.match(stderr, /-->[^\n]*trap\.tw:2:/);
  assert.equal(stderr.includes("^^"), true);
});

test("twk run renders a rich out-of-bounds message for an indexed write", () => {
  // Writing past the end reaches the rt.arr set/set_in_place bounds guard added
  // in this phase; the write site (line 2) carries the rich message + caret.
  const { status, stderr } = runTrap(
    "fn w(xs: Vector<Int>, i: Int) Vector<Int> {\n  xs[i] = 1\n  xs\n}\n\nprintln(w([1, 2, 3], 5))\n",
  );
  assert.notEqual(status, 0);
  assert.match(stderr, /index 5 out of bounds for length 3/);
  assert.match(stderr, /-->[^\n]*trap\.tw:2:/);
  assert.equal(stderr.includes("^^"), true);
});

test("twk run catches a negative index via the unsigned bounds guard", () => {
  // A negative index arrives as a large unsigned i32; the single I32GeU compare
  // catches it, and the raw index renders as `-1` (sign-extended in __panic_oob).
  const { status, stderr } = runTrap('xs := [10, 20, 30]\nprintln(xs[-1])\n');
  assert.notEqual(status, 0);
  assert.match(stderr, /index -1 out of bounds for length 3/);
});

test("twk run exits 0 for an in-bounds index (guard does not false-fire)", () => {
  // Sanity: a valid indexing program still runs to completion.
  const root = mkdtempSync(join(tmpdir(), "twk-trace-"));
  try {
    const okPath = join(root, "ok.tw");
    writeFileSync(okPath, 'xs := [10, 20, 30]\nprintln(xs[1])\n');
    const stdout = execFileSync("node", [entry, "run", okPath], { encoding: "utf8" });
    assert.match(stdout, /20/);
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
  // (v3) carries project-relative/@-logical paths + line/col but no embedded
  // source text, and a pre-built artifact run directly gets NO `source_root`
  // (the 4th `render_runtime_trace` arg is omitted below) — so per the M2
  // "no source_root ⇒ location-only for every frame" rule it can only recover
  // `file:line:col` + backtrace, not a snippet. Deleting the source dir first
  // makes the degrade unconditional. Reuses the divide-by-zero fixture (traps
  // directly in user code, not through the prelude `error()` shim) so this
  // isolates the no-root/disk-unavailable degrade path specifically, distinct
  // from the separate prelude-frame case above. Unlike the tests above, this
  // goes straight through tools/js_runtime's runtime.mjs (no `twk run`
  // recompile of the fixture) to mirror "run a pre-built .wasm" directly.
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
      // renderer lib and call render_runtime_trace WITHOUT a source_root
      // (the CLI's childTrapHandler passes one only for `twk run`; a raw
      // pre-built artifact gets none) — the omitted 4th arg forces the
      // location-only path.
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

test("twk run resolves a project-relative snippet path when launched from the project root", () => {
  // The B1 regression case: a project whose `twinkle.toml` sits at the launch
  // CWD makes `find_project_root("src")` return `"."`. The producer's
  // absolutization (module_compiler.abspath) turns that into the absolute CWD
  // so files still classify as in-root RELATIVE ("src/main.tw"), and `run_file`
  // forwards that absolute root as `source_root` so the renderer safe-joins and
  // reads the snippet — while the *rendered* frame path stays project-relative
  // and no absolute build path leaks into the trace.
  const projectDir = mkdtempSync(join(tmpdir(), "twk-proj-"));
  const rendererDir = mkdtempSync(join(tmpdir(), "twk-renderer-"));
  try {
    const rendererPath = buildRenderer(rendererDir);
    writeFileSync(join(projectDir, "twinkle.toml"), '[project]\nname = "demo"\n');
    mkdirSync(join(projectDir, "src"));
    writeFileSync(
      join(projectDir, "src", "main.tw"),
      "fn divide(a: Int, b: Int) Int {\n  a / b\n}\n\nx := divide(10, 0)\nprintln(\"never ${x}\")\n",
    );

    let status = 0;
    let stderr = "";
    try {
      execFileSync("node", [entry, "run", join("src", "main.tw")], {
        cwd: projectDir,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
        env: { ...process.env, RENDERER_WASM: rendererPath },
      });
    } catch (e) {
      status = e.status ?? 1;
      stderr = e.stderr?.toString() ?? "";
    }

    assert.notEqual(status, 0);
    assert.match(stderr, /error: divide by zero/);
    // Frame path is the project-relative "src/main.tw", not an absolute path.
    assert.match(stderr, /src\/main\.tw:2:\d+/);
    assert.match(stderr, /\^\^/);
    assert.match(stderr, /a \/ b/);
    // The absolute project directory must never appear in the trace.
    assert.equal(stderr.includes(projectDir), false);
  } finally {
    rmSync(projectDir, { recursive: true, force: true });
    rmSync(rendererDir, { recursive: true, force: true });
  }
});

test("twk run ./foo.tw with no manifest resolves its snippet via the entry-dir fallback", () => {
  // No `twinkle.toml` anywhere: `find_project_root(".")` returns `"."`, the
  // producer absolutizes it to the CWD (which contains the file), and the
  // relative entry "foo.tw" resolves from disk. Exercises the manifest-less
  // relative-entry path (distinct from the absolute-entry no-manifest case).
  const dir = mkdtempSync(join(tmpdir(), "twk-nomanifest-"));
  const rendererDir = mkdtempSync(join(tmpdir(), "twk-renderer-"));
  try {
    const rendererPath = buildRenderer(rendererDir);
    writeFileSync(
      join(dir, "foo.tw"),
      "fn divide(a: Int, b: Int) Int {\n  a / b\n}\n\nx := divide(10, 0)\nprintln(\"never ${x}\")\n",
    );

    let status = 0;
    let stderr = "";
    try {
      execFileSync("node", [entry, "run", "./foo.tw"], {
        cwd: dir,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
        env: { ...process.env, RENDERER_WASM: rendererPath },
      });
    } catch (e) {
      status = e.status ?? 1;
      stderr = e.stderr?.toString() ?? "";
    }

    assert.notEqual(status, 0);
    assert.match(stderr, /error: divide by zero/);
    assert.match(stderr, /foo\.tw:2:\d+/);
    assert.match(stderr, /\^\^/);
    assert.match(stderr, /a \/ b/);
    assert.equal(stderr.includes(dir), false);
  } finally {
    rmSync(dir, { recursive: true, force: true });
    rmSync(rendererDir, { recursive: true, force: true });
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
