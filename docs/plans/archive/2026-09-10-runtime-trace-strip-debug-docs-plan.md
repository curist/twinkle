# Runtime trace `--strip-debug` + docs — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an opt-in `twk build --strip-debug` flag that omits the `name` and `twinkle.debug` custom sections from the emitted `.wasm`, and document the shipped runtime trap-trace behavior.

**Architecture:** Emission-time gate. A new `strip_debug: Bool` field on `PipelineArtifacts` is set by the build command from the flag and threaded down `emit_wasm → codegen_wasm → emit_linked_wasm → emit_linked_wasm_buf → emit_wasm_parts`, where the two `emit_section_into` blocks are gated on `!strip_debug`. Default off — current behavior unchanged. Docs are a separate task.

**Tech Stack:** Twinkle (`.tw`) self-hosted compiler; boot test suite (`target/twk run boot/tests/main.tw`); JS runtime tests (`deno test -A tools/js_runtime/`).

## Global Constraints

- Design spec: `docs/plans/2026-09-10-runtime-trace-strip-debug-docs.md`. Roadmap: `docs/plans/runtime-stack-traces.md`.
- Default ships debug info; `--strip-debug` is strictly opt-in. Never change the default output.
- The boot/bundle build (`make bundle-cli`, Makefile) is **not** changed — `boot.wasm` keeps its debug sections. Do not add `--strip-debug` to the Makefile.
- The public `pipeline.emit_wasm(artifacts)` / `emit_wasm_buffer(artifacts)` signatures must stay 1-arg (read the flag off `artifacts`), so `run.tw`, `test.tw`, and `boot/tests/gen_bridge_wasm.tw` are untouched.
- `rt.panic` stays internal — do not document it as public API.
- After editing any `.tw`: `target/twk fmt <file>` then `target/twk lint boot/main.tw` (~4 pre-existing findings in section.tw/builtins.tw/census.tw are unrelated — ignore only those).
- Never run full `cargo test`. Boot suite = `target/twk run boot/tests/main.tw`; JS = `deno test -A tools/js_runtime/` (a pre-existing `web.test.mjs` JSPI env-quirk failure is unrelated). After a rebuild, `cp target/boot.wasm tools/js_runtime/boot.wasm` before JS tests.
- Twinkle notes: records have no field defaults (every constructor must set every field) and functions have no default args (every new param must be passed at every call site); `for cond { }` is the while form; record field update `r.field = v` rebinds; `parsed.has_flag("name") Bool`; command builders chain `.add_flag(name, help)` returning the command.
- Commits: imperative subject, what/why/how, no line/count metrics, with trailers:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01FmurxQ6UfDKcPb5T9fWjfY
  ```

## File Structure

- **Modify** `boot/compiler/artifacts.tw` — add `strip_debug: Bool` to `PipelineArtifacts`.
- **Modify** `boot/compiler/module_compiler.tw` — set `strip_debug: false` in the one artifacts constructor.
- **Modify** `boot/compiler/pipeline.tw` — `emit_wasm`/`emit_wasm_buffer` pass `artifacts.strip_debug` down.
- **Modify** `boot/compiler/codegen/codegen.tw` — `codegen_wasm`/`codegen_wasm_buffer` take + forward the flag.
- **Modify** `boot/compiler/codegen/wasm.tw` — `emit_linked_wasm`/`emit_linked_wasm_buffer`/`emit_linked_wasm_buf`/`emit_wasm_parts` take + forward the flag; gate the two section blocks.
- **Modify** `boot/commands/build.tw` — read the flag, set it on artifacts.
- **Modify** `boot/main.tw` — register the `--strip-debug` flag.
- **Create** `boot/tests/suites/strip_debug_suite.tw` — unit test the gate; register in `boot/tests/main.tw`.
- **Modify** `tools/js_runtime/cli.test.mjs` — e2e: stripped artifact falls back to no-trace.
- **Modify** `docs/spec.md`, `docs/internals/host-abi.md`, `docs/API.md` — Part B docs.
- **Modify** `docs/plans/runtime-stack-traces.md` — mark 4.5 + 4.6 complete (final task).

---

### Task 1: Thread `strip_debug` and gate section emission

End-to-end wiring with a unit test proving the gate. The CLI flag + e2e come in Task 2; the docs in Task 3.

**Files:**
- Modify: `boot/compiler/artifacts.tw:25`, `boot/compiler/module_compiler.tw:316`, `boot/compiler/pipeline.tw:169-175`, `boot/compiler/codegen/codegen.tw:504-535`, `boot/compiler/codegen/wasm.tw:1488-1522,1586,1731-1787`
- Create: `boot/tests/suites/strip_debug_suite.tw`
- Modify: `boot/tests/main.tw` (register suite)

**Interfaces:**
- Produces: `PipelineArtifacts` gains `strip_debug: Bool`. Internal chain functions gain a trailing `strip_debug: Bool` param: `codegen_wasm`, `codegen_wasm_buffer`, `emit_linked_wasm`, `emit_linked_wasm_buffer`, `emit_linked_wasm_buf`, `emit_wasm_parts`. Public `pipeline.emit_wasm(artifacts)`/`emit_wasm_buffer(artifacts)` signatures unchanged.

- [ ] **Step 1: Write the failing unit test**

Create `boot/tests/suites/strip_debug_suite.tw`. It compiles a tiny source to bytes with and without stripping, and checks both the `twinkle.debug` section (via `section.decode_module`) and the standard `name` section (via a local byte scan). Match the existing suite style (look at `boot/tests/suites/runtime_trace_renderer_suite.tw` for the `b(n)`/byte helpers and `section` usage, and `trace_render_suite.tw` for assert style).

```tw
use @std.testing.assert as assert
use @std.testing as runner

use compiler.pipeline
use lib.debug.section

// True if `bytes` is a valid wasm module (magic `\0asm` + version 1) that
// contains an id-0 custom section whose name equals `want`. Mirrors the
// section-framing walk in `section.decode_module`, but matches any custom
// section name (not just "twinkle.debug").
fn has_custom_section(bytes: Vector<Byte>, want: String) Bool {
  // Minimal header check: 8-byte header, then a sequence of (id, uleb(len), payload).
  if bytes.len() < 8 {
    return false
  }

  want_bytes := want.utf8_bytes()
  i := 8

  for i < bytes.len() {
    id := bytes[i].to_int()
    i = i + 1

    // Decode uleb section length.
    len := 0
    shift := 0

    for i < bytes.len() {
      byte := bytes[i].to_int()
      i = i + 1
      len = len | (byte & 0x7f) << shift
      shift = shift + 7

      if byte & 0x80 == 0 {
        break
      }
    }

    payload_start := i
    payload_end := i + len

    if id == 0 {
      // Custom section: payload begins with uleb name length + name bytes.
      j := payload_start
      name_len := 0
      nshift := 0

      for j < bytes.len() {
        byte := bytes[j].to_int()
        j = j + 1
        name_len = name_len | (byte & 0x7f) << nshift
        nshift = nshift + 7

        if byte & 0x80 == 0 {
          break
        }
      }

      if name_len == want_bytes.len() {
        matches := true
        k := 0

        for k < name_len {
          if bytes[j + k].to_int() != want_bytes[k].to_int() {
            matches = false
            break
          }

          k = k + 1
        }

        if matches {
          return true
        }
      }
    }

    i = payload_end
  }

  false
}

fn emit(src: String, strip: Bool) Vector<Byte> {
  artifacts := case pipeline.compile_source(src) {
    .Ok(a) => a,
    .Err(_) => error("strip_debug_suite: compile failed"),
  }
  artifacts.strip_debug = strip
  pipeline.emit_wasm(artifacts)
}

pub fn suite() runner.Suite {
  src: String = "fn cell(xs: Vector<Int>, i: Int) Int {\n  xs[i]\n}\n\nprintln(cell([1, 2, 3], 0))\n"

  runner
    .suite("strip debug")
    .test(
      "default build keeps the name and twinkle.debug sections",
      fn() {
        bytes := emit(src, false)
        try assert.is_true(has_custom_section(bytes, "name"))
        try assert.is_true(has_custom_section(bytes, "twinkle.debug"))
        try assert.is_true(case section.decode_module(bytes) {
          .Some(_) => true,
          .None => false,
        })
        .Ok({})
      },
    )
    .test(
      "strip_debug omits the name and twinkle.debug sections but stays valid wasm",
      fn() {
        bytes := emit(src, true)
        try assert.is_false(has_custom_section(bytes, "name"))
        try assert.is_false(has_custom_section(bytes, "twinkle.debug"))
        try assert.is_true(case section.decode_module(bytes) {
          .Some(_) => false,
          .None => true,
        })
        // A non-debug custom section (twinkle.externs/exports) or a core
        // section still present proves we emitted a real module, not nothing.
        try assert.is_true(bytes.len() > 8)
        .Ok({})
      },
    )
}
```

Register in `boot/tests/main.tw`: add `use .suites.strip_debug_suite` alphabetically among the `s*` entries and `strip_debug_suite.suite(),` in the `runner.run_all([...])` list.

NOTE on `pipeline.compile_source`: confirm a source-string compile entry exists in `pipeline.tw` (alongside `compile_entry_path`). If the exposed entry is named differently (e.g. `compile_source`/`compile_string`), use that name; if only a path-based entry exists, write the tiny source to a scratch temp file with `@std.fs` and use `compile_entry_path`, mirroring how other suites obtain artifacts. Do not invent an API — grep `pipeline.tw` first.

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -30`
Expected: FAIL — `PipelineArtifacts` has no `strip_debug` field (compile error on `artifacts.strip_debug = strip`), or, once the field exists but the gate is not wired, the strip test fails because the sections are still present.

- [ ] **Step 3: Add the field and set it at construction**

In `boot/compiler/artifacts.tw`, add to the `PipelineArtifacts` record (after `debug_files`):

```tw
  // When true, the emitter omits the `name` and `twinkle.debug` custom
  // sections (opt-in via `twk build --strip-debug`).
  strip_debug: Bool,
```

In `boot/compiler/module_compiler.tw:316`, add `strip_debug: false,` to the artifacts literal (the compile stage never strips; the build command opts in later):

```tw
      artifacts: .{
        env,
        builtins,
        core,
        mono,
        anf,
        opt,
        warnings: compile_warnings,
        project_root: abs_root,
        debug_files,
        strip_debug: false,
      },
```

- [ ] **Step 4: Thread the flag through the emit chain**

`boot/compiler/pipeline.tw` — pass the flag from artifacts:

```tw
pub fn emit_wasm(artifacts: PipelineArtifacts) Vector<Byte> {
  codegen_wasm(artifacts.opt, artifacts.env, artifacts.builtins, artifacts.debug_files, artifacts.strip_debug)
}

pub fn emit_wasm_buffer(artifacts: PipelineArtifacts) Buffer {
  codegen_wasm_buffer(artifacts.opt, artifacts.env, artifacts.builtins, artifacts.debug_files, artifacts.strip_debug)
}
```

`boot/compiler/codegen/codegen.tw` — add the param to both `codegen_wasm` and `codegen_wasm_buffer` (trailing `strip_debug: Bool,` after `debug_files`) and forward it:

```tw
  bytes := emit_linked_wasm(linked, strip_debug)
```
```tw
  bytes := emit_linked_wasm_buffer(linked, strip_debug)
```

`boot/compiler/codegen/wasm.tw` — add the param and forward:

```tw
pub fn emit_linked_wasm(linked: LinkedModule, strip_debug: Bool) Vector<Byte> {
  emit_linked_wasm_buf(linked, strip_debug).wasm_buf_to_bytes()
}

pub fn emit_linked_wasm_buffer(linked: LinkedModule, strip_debug: Bool) Buffer {
  emit_linked_wasm_buf(linked, strip_debug).wasm_buf_to_buffer()
}

fn emit_linked_wasm_buf(linked: LinkedModule, strip_debug: Bool) WasmBuf {
  buf := emit_wasm_parts(
    linked.types,
    linked.imports,
    linked.funcs,
    linked.globals,
    linked.tables,
    linked.elems,
    linked.exports,
    linked.memory_exports,
    linked.memories,
    linked.data,
    linked.start,
    linked.debug_files,
    strip_debug,
  )
  ...
}
```

And `emit_wasm_parts` gains a trailing `strip_debug: Bool,` parameter (after `debug_files: Vector<DebugFile>,`).

- [ ] **Step 5: Gate the two section blocks**

In `emit_wasm_parts` (`boot/compiler/codegen/wasm.tw`), gate the name section (currently `wasm.tw:1731`) and the twinkle.debug section (`wasm.tw:1740`) on `!strip_debug`:

```tw
  if !strip_debug and (imports.len() > 0 or funcs.len() > 0) {
    buf = .emit_section_into(0x00, encode_name_section_payload(imports, funcs, ctx))
  }
```
```tw
  if !strip_debug and module_debug.len() > 0 {
    ... // unchanged twinkle.debug construction + emit
  }
```

- [ ] **Step 6: Format, lint, and run tests**

Run: `target/twk fmt boot/compiler/artifacts.tw boot/compiler/module_compiler.tw boot/compiler/pipeline.tw boot/compiler/codegen/codegen.tw boot/compiler/codegen/wasm.tw boot/tests/suites/strip_debug_suite.tw boot/tests/main.tw && target/twk lint boot/main.tw`
Then: `target/twk run boot/tests/main.tw 2>&1 | tail -20`
Expected: fmt idempotent; lint only the known-unrelated findings; boot suite PASS including the two `strip debug` tests.

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/artifacts.tw boot/compiler/module_compiler.tw boot/compiler/pipeline.tw boot/compiler/codegen/codegen.tw boot/compiler/codegen/wasm.tw boot/tests/suites/strip_debug_suite.tw boot/tests/main.tw
git commit -F - <<'EOF'
feat(codegen): gate name + twinkle.debug sections behind strip_debug

Add a strip_debug flag carried on PipelineArtifacts and threaded through the
emit chain to emit_wasm_parts, where the name and twinkle.debug custom
sections are now gated on !strip_debug. Default false, so normal builds are
byte-for-byte unchanged; the build command wires the flag next.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FmurxQ6UfDKcPb5T9fWjfY
EOF
```

---

### Task 2: `twk build --strip-debug` flag + CLI e2e

Wire the flag into the build command and prove it end to end through the shipped CLI.

**Files:**
- Modify: `boot/main.tw` (register flag), `boot/commands/build.tw` (read + set on artifacts)
- Modify: `tools/js_runtime/cli.test.mjs` (e2e)

**Interfaces:**
- Consumes: `PipelineArtifacts.strip_debug` (Task 1) and the gated emitter.

- [ ] **Step 1: Register the flag**

In `boot/main.tw`, on the `build` command chain (after the `--web` flag, around line 85), add:

```tw
  .add_flag("strip-debug", "Omit the name and twinkle.debug custom sections from the .wasm output")
```

- [ ] **Step 2: Set the flag on artifacts in the build command**

In `boot/commands/build.tw`, in `run_build_command` (the non-lib project/single-file path) and `run_build_lib_project`, read the flag once and set it on each compiled artifacts value before it is emitted. After each `pipeline.compile_entry_path(file)` / `compile_entry_path_lib(file)` that yields `artifacts`, add:

```tw
      artifacts.strip_debug = parsed.has_flag("strip-debug")
```

Apply it at every site where an `artifacts` is obtained and later handed to `write_wasm`/`write_wasm_copies` (the single-file `-o` path around build.tw:240-247, and the lib project path). Grep `build.tw` for `compile_entry_path` to find all sites; each gets the one-line rebind immediately after a successful compile.

- [ ] **Step 3: Add the CLI e2e test**

In `tools/js_runtime/cli.test.mjs`, add a test near the other `twk run` trace tests. Follow the existing pattern (`buildRenderer(root)`, `execFileSync("node", [entry, ...], { env: { ...process.env, RENDERER_WASM: rendererPath, NO_COLOR: "1" } })`). Build a trapping program with `--strip-debug`, then run the produced artifact and assert the no-trace fallback:

```javascript
test("twk build --strip-debug produces an artifact that traps without a source-mapped trace", () => {
  const root = mkdtempSync(join(tmpdir(), "twk-strip-"));
  try {
    const rendererPath = buildRenderer(root);
    const trapPath = join(root, "trap.tw");
    writeFileSync(
      trapPath,
      "fn divide(a: Int, b: Int) Int {\n  a / b\n}\n\nx := divide(10, 0)\nprintln(\"never ${x}\")\n",
    );
    const outPath = join(root, "trap.wasm");

    // Build with --strip-debug (should succeed).
    execFileSync("node", [entry, "build", trapPath, "--strip-debug", "-o", outPath], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, RENDERER_WASM: rendererPath, NO_COLOR: "1" },
    });

    // Run the stripped artifact: it traps, but with no source-mapped trace.
    let status = 0;
    let stderr = "";
    try {
      execFileSync("node", [entry, "run", outPath], {
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
        env: { ...process.env, RENDERER_WASM: rendererPath, NO_COLOR: "1" },
      });
    } catch (e) {
      status = e.status ?? 1;
      stderr = e.stderr?.toString() ?? "";
    }

    assert.notEqual(status, 0);
    assert.match(stderr, /no Twinkle stack trace available/);
    assert.doesNotMatch(stderr, /trap\.tw:\d+:\d+/); // no source-mapped location
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
```

Confirm `mkdtempSync`, `join`, `tmpdir`, `writeFileSync`, `rmSync`, `entry`, and `buildRenderer` are already imported/defined in `cli.test.mjs` (they are used by the existing trace tests); reuse them.

- [ ] **Step 4: Format, lint, and build the CLI**

Run: `target/twk fmt boot/main.tw boot/commands/build.tw && target/twk lint boot/main.tw`
Then (sequential, foreground): `make bundle-cli`
This ships a `target/twk` that knows `--strip-debug`. Then refresh the JS copy: `cp target/boot.wasm tools/js_runtime/boot.wasm`.

- [ ] **Step 5: Run tests**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -20` (still green) and `deno test -A tools/js_runtime/ 2>&1 | tail -20`.
Expected: boot suite PASS; JS suite PASS except the one known-unrelated `web.test.mjs` JSPI quirk.

Manual sanity (optional): `target/twk build /tmp/trap.tw --strip-debug -o /tmp/s.wasm && target/twk run /tmp/s.wasm 2>&1` shows the native trap + "no Twinkle stack trace available"; without `--strip-debug`, a full trace.

- [ ] **Step 6: Commit**

```bash
git add boot/main.tw boot/commands/build.tw tools/js_runtime/cli.test.mjs tools/js_runtime/boot.wasm
git commit -F - <<'EOF'
feat(build): add twk build --strip-debug

Register an opt-in --strip-debug flag that sets strip_debug on the compiled
artifacts, so the emitter omits the name and twinkle.debug sections for
smaller production .wasm. A stripped artifact traps natively and the renderer
falls back to the headline plus "no Twinkle stack trace available". Covered by
a CLI e2e building then running a stripped trapping program.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FmurxQ6UfDKcPb5T9fWjfY
EOF
```

---

### Task 3: Docs (Phase 4.6)

Document the shipped trap-trace behavior and fix the stale `run_wasm` signature. Docs-only.

**Files:**
- Modify: `docs/spec.md` (§6 Traps), `docs/internals/host-abi.md` (`run_wasm` row + render boundary), `docs/API.md` (trap behavior note), `docs/plans/runtime-stack-traces.md` (mark 4.5 + 4.6 complete)

- [ ] **Step 1: spec.md §6 Traps**

In `docs/spec.md`, the "### Traps (unrecoverable)" subsection (around line 244), after the existing sentence about out-of-bounds/division/`error`, add a short paragraph:

```markdown
On `twk run`, an unrecoverable trap prints a source-mapped trace: a severity
headline with the trap message, the `file:line:col` of the failing expression,
a source snippet with a caret, and a backtrace of readable frame names — colored
like compile diagnostics (and honoring `NO_COLOR`). Out-of-bounds reports
`index N out of bounds for length L`. `twk build --strip-debug` omits the debug
sections, so a stripped artifact reports only the trap message with no
source-mapped trace.
```

- [ ] **Step 2: host-abi.md — fix run_wasm + document the render boundary**

In `docs/internals/host-abi.md`, correct the `run_wasm` table row (`:165`) from the 2-arg form to 3 args. New row:

```markdown
| `twinkle_runtime.run_wasm` | `(ref null $Array, ref null $Array, ref null $Array) → (i64)` | Run a child Wasm module (bytes, argv, source_root); returns its exit code |
```

Add a sentence after the table noting the third arg: `source_root` is the absolute directory that the child's project-relative `twinkle.debug` paths are safe-joined against when the renderer reads a source snippet (empty string disables snippet reads).

Then add a short subsection (near the existing run_wasm/JSPI notes) documenting the trap-render boundary:

```markdown
### Runtime trap traces

When a module launched via `run_wasm` traps, the host captures the trap message
and the V8 `.stack` string and renders a source-mapped trace via the renderer
library's `render_runtime_trace(bytes, stack, message, source_root)` export — a
dedicated library instance loaded beside `boot.wasm`, so rendering never
re-enters the suspended child. The trace is printed once to stderr and the run
exits nonzero. When no renderer is available (embeddable/web), the trap
re-throws unchanged. `twk build --strip-debug` drops the `name`/`twinkle.debug`
sections, so a stripped module renders only the trap headline.
```

(Verify the export name `render_runtime_trace` and its 4 args against `boot/runtime_trace_renderer.tw`, and the host wiring against `tools/js_runtime/runtime.mjs`.)

- [ ] **Step 3: API.md — trap behavior note**

In `docs/API.md`, at the indexing / error surface (the `vec[i]` indexing entry and/or the `error` builtin), add a one-line note that under `twk run` an out-of-bounds access, division by zero, or `error(msg)` traps with a source-mapped trace (headline + snippet + backtrace). Do not mention `rt.panic`. Keep it consistent with the spec §6 wording. Grep `docs/API.md` for the existing `error`/indexing entries and attach the note there.

- [ ] **Step 4: Mark the roadmap complete**

In `docs/plans/runtime-stack-traces.md`, update the top `Status:`/`Remaining:` lines: Phase 4.5 (`--strip-debug`) and 4.6 (docs) are now complete, so the feature is fully done — remove the "Remaining:" clause (or state "Remaining: none — feature complete"). Update the `### Phase 4 — Polish` checklist items for `--strip-debug` and docs to done.

- [ ] **Step 5: Verify + commit**

No automated test (docs-only). Re-read each edited section for accuracy against the source. Then:

```bash
git add docs/spec.md docs/internals/host-abi.md docs/API.md docs/plans/runtime-stack-traces.md
git commit -F - <<'EOF'
docs: document runtime trap traces and --strip-debug; fix run_wasm arity

Spec §6 now describes the source-mapped trap trace on twk run; host-abi.md
corrects the stale 2-arg run_wasm signature to the 3-arg (bytes, argv,
source_root) form and documents the render_runtime_trace boundary; API.md
notes the trap-trace behavior at the indexing/error surface. Mark roadmap
Phases 4.5 and 4.6 complete.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FmurxQ6UfDKcPb5T9fWjfY
EOF
```

---

## Self-Review

**Spec coverage:**
- Phase 4.5 `--strip-debug` (gate, flag threading, CLI flag, artifacts flag) → Tasks 1 & 2. ✓
- Default-off / behavior-unchanged → Task 1 Step 3 (`strip_debug: false`) + gate on `!strip_debug`. ✓
- Every emit path honors it (set on artifacts) → Task 2 Step 2. ✓
- Public `emit_wasm` signature unchanged → Task 1 Step 4 (reads off artifacts). ✓
- Boot/bundle build not changed → no Makefile task; Global Constraints. ✓
- Phase 4.6 docs (spec §6, host-abi run_wasm + render boundary, API.md, rt.panic excluded) → Task 3. ✓
- Stripped-artifact fallback proven e2e → Task 2 Step 3. ✓

**Placeholder scan:** No TBD/"handle edge cases"/"similar to Task N"; code steps show actual content. The one conditional ("if the source-compile entry is named differently") names the exact fallback (temp file + `compile_entry_path`) rather than leaving it open. ✓

**Type consistency:** `strip_debug: Bool` added in Task 1 and consumed in Task 2 via `artifacts.strip_debug`; the trailing-`strip_debug` param order is consistent across `codegen_wasm`/`emit_linked_wasm`/`emit_wasm_parts`; `has_custom_section(bytes, want) Bool` and `emit(src, strip) Vector<Byte>` defined and used within Task 1's suite. ✓
