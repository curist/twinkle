# Disk-Backed Debug Info — Milestone 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drop inline source text from the `twinkle.debug` section — keeping absolute paths, byte spans, and precomputed line/col — and make the `twk run` trap renderer read source snippets from disk, so `boot.wasm` sheds its ~2× debug bloat while `twk run` still prints source-mapped traces.

**Architecture:** The section format goes to v2: the file table stores `{file_id, path}` only (no source), and each line-program entry gains four **absolute** line/col fields alongside its byte span. The compiler computes line/col at section-emit time from the source it already holds, then discards the source. The renderer (a `--lib` module) reads each frame's absolute path via `@std.fs` for the snippet+caret, and derives every frame's `file:line:col` from the embedded line/col — so an unreadable file degrades to location-only, never a crash. The host boundary is unchanged (3-arg `render_runtime_trace`, existing `childTrapHandler`).

**Tech Stack:** Twinkle (`.tw`, boot compiler), `@std.fs` host extern (`twinkle_runtime.read_file_string`), Deno/Node JS runtime host (`tools/js_runtime`), the self-host loop (`make stage2` / `make bundle-cli`).

## Global Constraints

- Boot compiler in `boot/` is the primary implementation; touch Rust stage0 (`src/`) only if the self-host loop demands it (it should not — no new language feature is used).
- After editing any `.tw`, run `target/twk fmt <file>` then `target/twk lint <entry>`; the formatter is idempotent.
- Run heavy verification (`make stage2`, `make bundle-cli`, full suites) **sequentially, never concurrently or backgrounded**.
- Do **not** run full `cargo test`; use targeted filters only. Boot suite via `target/twk run boot/tests/main.tw`.
- Commit messages: imperative subject, what/why/how body, no line/count metrics. Trailers:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01FmurxQ6UfDKcPb5T9fWjfY
  ```
- The fresh compiler after a stage rebuild is `target/twk` only once `make bundle-cli` runs; before that, drive the just-built compiler with `BOOT_WASM=target/boot.wasm deno run -A tools/js_runtime/deno_main.mjs <args>`, and copy `target/boot.wasm` to `tools/js_runtime/boot.wasm` before running JS tests.
- Line/col are **codepoint** columns produced by `registry.line_col`; the producer must reuse that exact function so producer and renderer never disagree on a column.
- All line/col fields are stored **absolute** (only `offset` stays delta-encoded); source positions are non-monotonic across entries and `push_uleb` is unsigned-only.

## Execution note (task boundaries)

Tasks 2 (A), 3 (B), and 4 (C) form **one atomic compile unit** and are dispatched to a single implementer. Reason: the boot test entry (`boot/tests/main.tw`) transitively compiles `compiler.codegen.wasm` and the renderer suites, so `wasm.tw`/`module_compiler.tw` (which *construct* the section types) and `symbolicate.tw`/`trace.tw` (which *consume* them) must migrate in the same change or the tree won't compile — Twinkle records have no field defaults, so there is no green-at-each-step path. The per-step "run suite to confirm it fails/passes" gates inside Tasks 2–4 therefore apply **after the whole A+B+C migration**, not between them. Task 1 (C0) already landed (gate PASS). Task 5 (D) remains a separate verify task.

## File Structure

- `boot/lib/debug/section.tw` — **modify.** Format v2: `FileEntry` loses `source`; `LineEntry` gains `start_line/start_col/end_line/end_col`; `encode`/`decode`/`decode_module` updated; `span_at`→`entry_at` and `lookup` return `LineEntry?`; drop `file_source`; add `file_path(section, file_id) String?`.
- `boot/tests/suites/debug_section_suite.tw` — **modify.** Rewrite fixtures/assertions for v2.
- `boot/compiler/codegen/wasm.tw` — **modify.** Capture raw lines; at section-emit build a registry from threaded source, fill line/col, emit source-less file table.
- `boot/compiler/module_compiler.tw` — **modify.** Thread a source-carrying `DebugFile` (not the source-less section `FileEntry`) as `debug_files`.
- `boot/compiler/pipeline.tw` — **modify.** `debug_files` element type change ripples through `PipelineArtifacts`/`codegen_wasm` signatures.
- `boot/lib/debug/symbolicate.tw` — **modify.** Locations from embedded line/col (no registry); add a disk-source registry builder for snippets; `ResolvedFrame` unchanged in shape.
- `boot/lib/debug/trace.tw` — **modify.** Use disk registry for the snippet; append `source unavailable` when the primary frame's file isn't readable.
- `boot/runtime_trace_renderer.tw` — **unchanged signature**; imports flow through unchanged (it delegates to `trace`/`symbolicate`).
- `boot/tests/suites/runtime_trace_renderer_suite.tw` — **modify.** Point fixtures at a temp-dir source file; cover present/missing/location-only.
- `tools/js_runtime/cli.test.mjs` — **modify/confirm.** `twk run` shows a snippet; a pre-built artifact run later shows location-only.
- `docs/plans/disk-backed-debug-info.md` — the design (already written); this plan implements its **Milestone 1** only.

---

## Task 1 (C0): `@std.fs`-in-`--lib` spike (gate)

The whole milestone rests on one unproven capability: a `--lib` artifact importing `@std.fs` and reading a file through `loadLibSync`'s host imports. `prepareWasm` already returns `twinkle_runtime` (including `read_file_string`, runtime.mjs:536) in `hostImports`, so this *should* just work — but `instantiateWithExternRetry` + extern-meta wiring for a lib that declares an extern is untested. Prove it before touching the format.

**Files:**
- Create (throwaway): `/private/tmp/claude-501/.../scratchpad/fs_spike.tw`
- Reference: `tools/js_runtime/runtime.mjs:1272` (`loadLibSync`), `:522`/`:536` (fs adapters), `boot/stdlib/fs.tw:40` (`read_text`)

**Interfaces:**
- Consumes: `@std.fs.read_text(path: String) String!FsError`, `loadLibSync(wasmBytes, opts)`.
- Produces: confidence (documented in the task's commit or the plan's notes) that a lib may import `@std.fs`. No shipped code.

- [ ] **Step 1: Write a minimal fs-reading lib**

`fs_spike.tw`:
```tw
use @std.fs

pub fn file_len(path: String) Int {
  case fs.read_text(path) {
    .Ok(text) => text.char_len(),
    .Err(_) => -1,
  }
}
```

- [ ] **Step 2: Build it as a lib and load it synchronously**

Build (using the current shipped compiler):
```bash
target/twk build --lib /private/tmp/claude-501/.../scratchpad/fs_spike.tw -o /private/tmp/claude-501/.../scratchpad/fs_spike.wasm
```
Then a throwaway Node/Deno script that `loadLibSync`s it and calls `file_len` on a known file (e.g. the spike's own path), printing the result. Run it.
Expected: prints the file's codepoint length (a positive int), **not** `-1` and **not** an instantiation error about a missing import.

- [ ] **Step 3: Decide the gate**

If it prints the length → capability confirmed; proceed. If it errors on the extern import or returns `-1` for a readable file → stop and fix the lib-export extern wiring (extend `loadLibSync`/`prepareWasm` extern handling) here, before any format change. Record the outcome in the Task C (renderer) commit body.

- [ ] **Step 4: Clean up**

Delete the scratchpad spike files. No commit (throwaway), or a docs-only note if wiring had to change.

---

## Task 2 (A): Format v2 codec

**Files:**
- Modify: `boot/lib/debug/section.tw`
- Test: `boot/tests/suites/debug_section_suite.tw`

**Interfaces:**
- Produces (consumed by Tasks B and C):
  - `pub type FileEntry = .{ file_id: Int, path: String }`
  - `pub type LineEntry = .{ offset: Int, span: Span, start_line: Int, start_col: Int, end_line: Int, end_col: Int }`
  - `pub type FuncDebug = .{ func_idx: Int, body_start: Int, lines: Vector<LineEntry> }`
  - `pub type DebugSection = .{ files: Vector<FileEntry>, funcs: Vector<FuncDebug> }`
  - `pub fn encode(section: DebugSection) Vector<Byte>`
  - `pub fn decode(bytes: Vector<Byte>) DebugSection?`
  - `pub fn decode_module(module_bytes: Vector<Byte>) DebugSection?`
  - `pub fn entry_at(fn_dbg: FuncDebug, body_rel: Int) LineEntry?` (was `span_at`, returned `Span?`)
  - `pub fn lookup(section: DebugSection, pc: Int) LineEntry?` (was `Span?`)
  - `pub fn file_path(section: DebugSection, file_id: Int) String?` (replaces `file_source`)

- [ ] **Step 1: Rewrite the suite fixtures/assertions for v2 (failing test)**

Replace `sample()` and the assertions in `boot/tests/suites/debug_section_suite.tw`. New `sample()` (no `source`; line/col on each entry):
```tw
fn sample() DebugSection {
  files: Vector<FileEntry> = [
    .{ file_id: 0, path: "main.tw" },
    .{ file_id: 3, path: "util.tw" },
  ]
  funcs: Vector<FuncDebug> = [
    .{
      func_idx: 5,
      body_start: 200,
      lines: [
        .{ offset: 0, span: .{ file_id: 0, start: 0, end: 6 }, start_line: 1, start_col: 1, end_line: 1, end_col: 7 },
        .{ offset: 7, span: .{ file_id: 0, start: 7, end: 17 }, start_line: 2, start_col: 1, end_line: 2, end_col: 11 },
        .{ offset: 19, span: .{ file_id: 3, start: 19, end: 20 }, start_line: 3, start_col: 1, end_line: 3, end_col: 2 },
      ],
    },
    .{
      func_idx: 6,
      body_start: 260,
      lines: [.{ offset: 0, span: .{ file_id: 3, start: 19, end: 20 }, start_line: 3, start_col: 1, end_line: 3, end_col: 2 }],
    },
  ]
  .{ files, funcs }
}
```
Update the four tests: the round-trip test drops the `source` assertion and adds `try assert.equal(decoded.funcs[0].lines[1].start_line, 2)` and `try assert.equal(decoded.funcs[0].lines[1].start_col, 1)`; `entry_at`/`lookup` assertions read `.span.start`/`.span.file_id` off the returned `LineEntry` (rename `span_at`→`entry_at`; e.g. `f.entry_at(3)` then `s0.span.start == 0`). Keep the version-reject test.

- [ ] **Step 2: Run the suite to confirm it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — `debug section` compile error (unknown field `source`, missing `start_line`, unknown `span_at`).

- [ ] **Step 3: Update the model + version**

In `section.tw`: change `FileEntry` to `.{ file_id: Int, path: String }`; change `LineEntry` to add `start_line/start_col/end_line/end_col: Int`; bump `version()` to `2`; update the module doc comment (drop "source text").

- [ ] **Step 4: Update `encode`**

File-table loop drops the flag byte and the source string:
```tw
for f in section.files {
  out = push_uleb(out, f.file_id)
  out = push_str(out, f.path)
}
```
Line-program loop keeps `offset` delta-encoded, keeps span fields absolute, adds the four line/col fields absolute:
```tw
for entry in fn_dbg.lines {
  out = push_uleb(out, entry.offset - prev)
  out = push_uleb(out, entry.span.file_id)
  out = push_uleb(out, entry.span.start)
  out = push_uleb(out, entry.span.end)
  out = push_uleb(out, entry.start_line)
  out = push_uleb(out, entry.start_col)
  out = push_uleb(out, entry.end_line)
  out = push_uleb(out, entry.end_col)
  prev = entry.offset
}
```

- [ ] **Step 5: Update `decode`**

File-table loop: read `file_id`, then `path` (no flag byte, no source):
```tw
idr := read_uleb(bytes, pos)
file_id := idr.value
pos = idr.pos
pr := read_str(bytes, pos)
pos = pr.pos
files = .append(.{ file_id, path: pr.value })
```
Line-program loop: after reading `offset` (delta), `file_id`, `start`, `end`, read the four line/col ulebs and build the entry:
```tw
slr := read_uleb(bytes, pos)
pos = slr.pos
scr := read_uleb(bytes, pos)
pos = scr.pos
elr := read_uleb(bytes, pos)
pos = elr.pos
ecr := read_uleb(bytes, pos)
pos = ecr.pos
lines = .append(.{
  offset,
  span: span.new(fidr.value, startr.value, endr.value),
  start_line: slr.value, start_col: scr.value,
  end_line: elr.value, end_col: ecr.value,
})
```
(`decode_module` needs no change beyond calling the updated `decode`.)

- [ ] **Step 6: Update lookup + accessors**

Rename `span_at` → `entry_at` returning `LineEntry?` (return `.Some(lines[lo - 1])` instead of `.Some(lines[lo - 1].span)`). Change `lookup` to return `LineEntry?` (`chosen.entry_at(pc - chosen.body_start)`). Replace `file_source` with:
```tw
pub fn file_path(section: DebugSection, file_id: Int) String? {
  for f in section.files {
    if f.file_id == file_id {
      return .Some(f.path)
    }
  }
  .None
}
```

- [ ] **Step 7: Run the suite to confirm it passes**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS for `debug section` (other suites may still fail — Tasks B/C fix them). If the whole run is red only on `runtime trace renderer` / codegen suites, that's expected until later tasks.

- [ ] **Step 8: fmt + lint + commit**

```bash
target/twk fmt boot/lib/debug/section.tw boot/tests/suites/debug_section_suite.tw
target/twk lint boot/tests/main.tw
git add boot/lib/debug/section.tw boot/tests/suites/debug_section_suite.tw
git commit -m "feat(debug): twinkle.debug v2 — source-less file table + embedded line/col"
```

---

## Task 3 (B): Producer — drop source, embed line/col

**Files:**
- Modify: `boot/compiler/codegen/wasm.tw`, `boot/compiler/module_compiler.tw`, `boot/compiler/pipeline.tw`
- Test: the boot suite (`boot/tests/main.tw`) + a direct decode assertion on a compiled fixture (below)

**Interfaces:**
- Consumes: Task A's `FileEntry`/`LineEntry`/`DebugSection`, `registry.empty`/`add_file`/`line_col` (`boot/lib/source/registry.tw:13,30,95`), `span.new`.
- Produces (consumed by codegen only): a producer-local `DebugFile = .{ file_id: Int, path: String, source: String }` threaded as `debug_files`; the emitted section has source-less `FileEntry`s and line/col-filled `LineEntry`s.

- [ ] **Step 1: Introduce the source-carrying producer type**

In `boot/compiler/module_compiler.tw`, define near the other debug uses:
```tw
pub type DebugFile = .{ file_id: Int, path: String, source: String }
```
Change the `debug_files` builder (module_compiler.tw:301) from `section.FileEntry` to `DebugFile`:
```tw
debug_files: Vector<DebugFile> = collect r in cur_cache.file_records_list() {
  .{ file_id: r.file_id, path: r.path, source: r.source }
}
```
Remove the now-unused `use lib.debug.section.{FileEntry}` if nothing else references it (keep `section` if still used elsewhere).

- [ ] **Step 2: Thread the new type through the pipeline signatures**

In `boot/compiler/pipeline.tw`: change `PipelineArtifacts.debug_files` and the `codegen_wasm`/`codegen_wasm_buffer` parameter types from `Vector<section.FileEntry>` to `Vector<module_compiler.DebugFile>` (import the type as needed). In `boot/compiler/codegen/wasm.tw`, change the `debug_files` parameter type on the code-emitting entry (near wasm.tw:1587) to `Vector<DebugFile>` and add the import. Build to surface every mismatched signature and fix each.

Run: `BOOT_WASM=target/boot.wasm deno run -A tools/js_runtime/deno_main.mjs build boot/main.tw -o /tmp/probe.wasm`
Expected at this point: a type error only if a signature was missed; iterate until it compiles. (Behavior still old — line/col not yet embedded.)

- [ ] **Step 3: Capture raw lines (no line/col at capture)**

In `boot/compiler/codegen/wasm.tw`, add a local raw type and switch capture to it:
```tw
type RawLine = .{ offset: Int, span: Span }
type RawFunc = .{ func_idx: Int, body_start: Int, lines: Vector<RawLine> }
```
Change `CodeSectionCache.lines` to `Vector<RawLine>`, `record_line` to append `.{ offset, span }` (already its shape), and `CapturedFunc.lines` / `encode_code_section_payload` to produce `RawFunc`. Update `module_debug` (wasm.tw:1691) to `Vector<RawFunc>` and its append (1698-1702) to build `RawFunc`.

- [ ] **Step 4: Enrich to `section.FuncDebug` at emit, filling line/col from a registry**

At the section-emit block (wasm.tw:1723), before `encode`, build a registry from the threaded source and convert each `RawFunc` to a `section.FuncDebug` with line/col-filled `LineEntry`s, and build the source-less file table:
```tw
if module_debug.len() > 0 {
  // Registry over referenced source, for line/col precompute only.
  reg := registry.empty()
  for df in debug_files {
    reg = reg.add_file_with_id(df.file_id, df.path, df.source)
  }

  fn line_col_of(reg2: registry.FileRegistry, file_id: Int, at: Int) registry.LineCol {
    case reg2.line_col(span.new(file_id, at, at)) {
      .Some(lc) => lc,
      .None => .{ line: 0, column: 0 },
    }
  }

  enriched: Vector<section.FuncDebug> = collect rf in module_debug {
    lines: Vector<section.LineEntry> = collect e in rf.lines {
      sc := line_col_of(reg, e.span.file_id, e.span.start)
      ec := line_col_of(reg, e.span.file_id, e.span.end)
      .{
        offset: e.offset, span: e.span,
        start_line: sc.line, start_col: sc.column,
        end_line: ec.line, end_col: ec.column,
      }
    }
    .{ func_idx: rf.func_idx, body_start: rf.body_start, lines }
  }

  referenced: Dict<Int, Bool> = Dict.new()
  for fd in enriched {
    for entry in fd.lines {
      referenced[entry.span.file_id] = true
    }
  }
  used_files: Vector<section.FileEntry> = collect df in debug_files.filter(fn(d) { referenced.has(d.file_id) }) {
    .{ file_id: df.file_id, path: df.path }
  }
  dbg_bytes := section.encode(.{ files: used_files, funcs: enriched })
  dbg_payload := empty_wasm_buf().emit_name(section.section_name()).emit_bytes(dbg_bytes)
  buf = .emit_section_into(0x00, dbg_payload)
}
```
(If a nested `fn` inside this block is not permitted here, inline the two `line_col` lookups instead of the helper.)

- [ ] **Step 5: Add a producer round-trip assertion (test)**

Add a test to `boot/tests/suites/debug_section_suite.tw` (or a small dedicated check in `runtime_trace_renderer_suite.tw`) that compiles a tiny program to wasm bytes and asserts the decoded section carries no source but correct line/col. If compiling-to-bytes-in-suite isn't ergonomic, assert via the CLI path in `cli.test.mjs` instead (Step 6). Prefer the CLI path if in doubt.

- [ ] **Step 6: Rebuild the compiler and verify a compiled section decodes correctly**

Rebuild and inspect a real program's section (use a `.wat` or a decode probe):
```bash
BOOT_WASM=target/boot.wasm deno run -A tools/js_runtime/deno_main.mjs build boot/tests/main.tw -o /tmp/probe.wasm
```
Then a throwaway decode (reuse the renderer's `section.decode_module`, or a Node script reading `/tmp/probe.wasm`) and confirm: file entries have a `path`, no source; a known line-program entry has plausible `start_line`/`start_col`. Also confirm the file shrank materially vs. a v1 build (sanity for the size goal — full confirmation is Task D).
Expected: section decodes; `start_line`/`start_col` are nonzero for real spans.

- [ ] **Step 7: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/codegen/wasm.tw boot/compiler/module_compiler.tw boot/compiler/pipeline.tw
target/twk lint boot/main.tw
git add boot/compiler/codegen/wasm.tw boot/compiler/module_compiler.tw boot/compiler/pipeline.tw boot/tests/suites/debug_section_suite.tw
git commit -m "feat(debug): emit source-less v2 section with precomputed line/col"
```

---

## Task 4 (C): Renderer — disk-backed snippets, embedded-line/col locations

**Files:**
- Modify: `boot/lib/debug/symbolicate.tw`, `boot/lib/debug/trace.tw`
- Test: `boot/tests/suites/runtime_trace_renderer_suite.tw`, `tools/js_runtime/cli.test.mjs`

**Interfaces:**
- Consumes: Task A's `lookup(section, pc) LineEntry?`, `file_path(section, file_id) String?`; `@std.fs.read_text(path) String!FsError`; `registry.empty`/`add_file_with_id`/`line_col`/`snippet`; `render.render`.
- Produces: `render_runtime_trace(bytes, stack, message) String` (signature unchanged) that renders a snippet when the innermost frame's file is readable, else location-only + `source unavailable`.

- [ ] **Step 1: Renderer suite — temp-dir source present ⇒ snippet (failing test)**

In `boot/tests/suites/runtime_trace_renderer_suite.tw`, add a test that writes a small source file to a temp dir via `@std.fs`, builds a `DebugSection` whose file table points at that absolute path with a span/line-col over a known line, synthesizes a matching V8 stack string, and asserts the rendered output contains the source line text and the caret. Add a companion test pointing the file table at a **non-existent** path and assert the output contains `source unavailable` and the `at <name> (path:line:col)` backtrace line (location-only). Use the embedded line/col so the backtrace renders even for the missing file.

(Exact fixture: mirror the existing suite's stack-string helper; write the temp file under the scratchpad or `@std.fs`-created temp dir, use its absolute path as `FileEntry.path`.)

- [ ] **Step 2: Run the suite to confirm it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — `runtime trace renderer` (symbolicate still expects inline source; no disk read yet).

- [ ] **Step 3: Rework `symbolicate.tw` — locations from embedded line/col, disk registry for snippets**

Replace the inline-source registry reconstruction with two pieces. Locations come straight from the embedded entry (no registry):
```tw
use @std.fs

fn resolve(sec: DebugSection, frame: Frame) ResolvedFrame {
  entry := sec.lookup(frame.code_offset)
  location: FrameLoc? = case entry {
    .Some(e) => case sec.file_path(e.span.file_id) {
      .Some(path) => .Some(.{ file: path, line: e.start_line, column: e.start_col }),
      .None => .None,
    },
    .None => .None,
  }
  span: Span? = case entry {
    .Some(e) => .Some(e.span),
    .None => .None,
  }
  .{ frame, span, location }
}

pub fn symbolicate(sec: DebugSection, frames: Vector<Frame>) Vector<ResolvedFrame> {
  resolved: Vector<ResolvedFrame> = []
  for fr in frames {
    resolved = .append(resolve(sec, fr))
  }
  resolved
}
```
Add a disk-source registry builder used only for the snippet (readable files only):
```tw
pub fn build_disk_registry(sec: DebugSection) FileRegistry {
  reg := registry.empty()
  for f in sec.files {
    case fs.read_text(f.path) {
      .Ok(src) => reg = reg.add_file_with_id(f.file_id, f.path, src),
      .Err(_) => {},
    }
  }
  reg
}
```
Remove `reconstruct_registry` (and its `f.source` use). Keep `ResolvedFrame`/`FrameLoc` shapes.

- [ ] **Step 4: Rework `trace.tw` — snippet from disk registry, `source unavailable` fallback**

`render_trace` builds the disk registry, renders with it, and appends the note when the primary frame's file is not in the registry:
```tw
pub fn render_trace(
  section: DebugSection,
  frames: Vector<Frame>,
  message: String,
  config: RenderConfig,
) String {
  reg := symbolicate.build_disk_registry(section)
  resolved := symbolicate.symbolicate(section, frames)

  labels := case primary_label(resolved) {
    .Some(l) => [l],
    .None => [],
  }
  report: Report = .{ severity: .Error, title: message, labels, help_lines: [] }
  head := render.render(report, reg, config)
  bt := backtrace_lines(resolved)

  snippet_ok := case primary_label(resolved) {
    .Some(l) => case reg.line_col(l.span) { .Some(_) => true, .None => false },
    .None => false,
  }

  if bt.len() == 0 {
    "${head}\n  (no Twinkle stack trace available)"
  } else if snippet_ok {
    "${head}\n\n${bt.join("\n")}"
  } else {
    "${head}\n\n${bt.join("\n")}\n\n  source unavailable"
  }
}
```
(`render.render` already draws the snippet only when `reg.line_col(pl.span)` resolves, so with a disk registry the source block appears exactly when the file was readable — `snippet_ok` mirrors that predicate for the note.)

- [ ] **Step 5: Run the suite to confirm it passes**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS for `runtime trace renderer` (present ⇒ snippet; missing ⇒ `source unavailable` + backtrace). `debug section` still green.

- [ ] **Step 6: Rebuild renderer + boot, refresh CLI test fixtures**

The renderer ships as `target/renderer.wasm` beside `boot.wasm`. Rebuild both via the Makefile rules, then confirm the `twk run` e2e:
```bash
make stage2          # sequential; rebuilds target/boot.wasm to fixed point
make bundle-cli      # rebuilds target/twk (+ target/renderer.wasm)
cp target/boot.wasm tools/js_runtime/boot.wasm
```
Update `tools/js_runtime/cli.test.mjs`: `twk run` on a trapping fixture shows the source snippet (file present); a **pre-built** artifact (`twk build fixture.tw -o f.wasm` then run the wasm) shows location-only. Run the JS tests (targeted):
```bash
deno test -A tools/js_runtime/cli.test.mjs
```
Expected: PASS.

- [ ] **Step 7: fmt + lint + commit**

```bash
target/twk fmt boot/lib/debug/symbolicate.tw boot/lib/debug/trace.tw boot/tests/suites/runtime_trace_renderer_suite.tw
target/twk lint boot/main.tw
git add boot/lib/debug/symbolicate.tw boot/lib/debug/trace.tw boot/tests/suites/runtime_trace_renderer_suite.tw tools/js_runtime/cli.test.mjs
git commit -m "feat(debug): render trap snippets from disk; degrade to location-only"
```

---

## Task 5 (D): Verify — self-host, size, trap fixtures

**Files:** none (verification), plus memory/plan-doc bookkeeping.

**Interfaces:** consumes the fully-built `target/twk` and `target/renderer.wasm`.

- [ ] **Step 1: Self-host fixed point**

Run sequentially:
```bash
make stage2
make bundle-cli
```
Expected: stage2 reaches its fixed point; `target/twk` builds.

- [ ] **Step 2: Confirm the size win**

Compare `target/boot.wasm` size against the pre-change (v1) build — it should drop back toward its pre-debug ~4.5 MB (from ~9 MB). Record the actual before/after in the commit body (this is a "why it matters" fact, not a metrics-for-metrics count).
```bash
ls -l target/boot.wasm
```
Expected: materially smaller than the v1 debug build.

- [ ] **Step 3: Full boot + JS suites**

```bash
target/twk run boot/tests/main.tw
cp target/boot.wasm tools/js_runtime/boot.wasm
deno test -A tools/js_runtime/
```
Expected: green.

- [ ] **Step 4: Trap-fixture e2e (error / div0 / OOB) via shipped `target/twk`**

Run each trapping fixture through `target/twk run` and confirm a source-mapped snippet + backtrace, single print, exit 1:
```bash
target/twk run <error-fixture>.tw ; echo "exit=$status"
target/twk run <div0-fixture>.tw  ; echo "exit=$status"
target/twk run <oob-fixture>.tw   ; echo "exit=$status"
```
(Reuse the fixtures the Phase-2 work already added; `$status` is fish's exit-code variable.)
Expected: each prints one source-mapped trace, `exit=1`; a normal program still exits 0 with no trace.

- [ ] **Step 5: Bookkeeping commit**

Mark Milestone 1 done: update `docs/plans/disk-backed-debug-info.md` status note, adjust the `docs/plans/README.md` row (M1 complete, M2 still deferred), and refresh the `project_runtime_stack_traces.md` memory pointer.
```bash
git add docs/plans/disk-backed-debug-info.md docs/plans/README.md
git commit -m "docs(debug): mark disk-backed debug info Milestone 1 complete"
```

---

## Self-Review

**Spec coverage** (against `docs/plans/disk-backed-debug-info.md`, Milestone 1):
- Drop source text, keep absolute paths → Task A (FileEntry) + Task B (source-less emit). ✓
- Embedded absolute line/col → Task A (LineEntry fields) + Task B (precompute via `registry.line_col`). ✓
- 3-arg renderer, unchanged host boundary → Task C keeps `render_runtime_trace(bytes, stack, message)`; no `run_wasm`/`childTrapHandler` edit. ✓
- Disk-read snippet + location-only degradation + `source unavailable` → Task C (`build_disk_registry`, `snippet_ok`). ✓
- `@std.fs`-in-lib gate first → Task C0 before format change. ✓
- Reuse `registry.line_col` for column parity → Task B Step 4. ✓
- All line/col absolute, only offset delta → Task A Steps 4–5. ✓
- Staleness documented not detected → no code; the design doc already states it (no tripwire, no hash). ✓
- Size win verified → Task D Step 2. ✓
- Milestone 2 (relativization/safe-join/`find_project_root`/`source_root`) → **out of scope**, correctly absent.

**Placeholder scan:** No "TBD"/"handle edge cases"/"similar to Task N" — each code step carries real code. The two soft spots (Task B Step 5 "assert via CLI if in-suite is awkward"; Task C Step 1 "mirror the existing stack-string helper") are deliberate test-authoring latitude, not implementation gaps, and both name the concrete fallback.

**Type consistency:** `LineEntry` fields (`start_line/start_col/end_line/end_col`) are identical across Tasks A (define), B (fill), C (read). `entry_at`/`lookup` return `LineEntry?` consistently (A defines, C consumes `.span`/`.start_line`/`.start_col`). `file_path(section, file_id) String?` defined in A, used in C. `DebugFile` (producer, source-carrying) is distinct from `FileEntry` (on-wire, source-less) — never conflated. `build_disk_registry`/`symbolicate` names match between symbolicate.tw and trace.tw.
