# Runtime Trace: User Frame as Primary (Phase 4.1) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a runtime trap trace's primary snippet/caret (and its backtrace) land on the user's call site instead of a prelude/stdlib internal, so `error(...)` and any stdlib-internal trap point at user code.

**Architecture:** All rendering-policy logic lives in `boot/lib/debug/trace.tw`. Using M2's path convention (a resolved frame is a user frame iff its file path does **not** start with `@`), `render_trace` computes the user-frame subset once and drives both the primary snippet and the backtrace from it — falling back to all resolved frames when no user frame exists, so an all-stdlib trap still shows a location. No ABI change, no `symbolicate.tw`/section change.

**Tech Stack:** Twinkle (`.tw`, boot compiler), the boot test suites, `tools/js_runtime/cli.test.mjs` (Node), `make bundle-cli` (ships `target/renderer.wasm`).

**Design:** `docs/plans/runtime-trace-user-frame-primary.md` (read it — this plan implements it).

## Global Constraints

- Boot compiler in `boot/` is primary; this phase touches only `boot/lib/debug/trace.tw` + its boot suite, and `tools/js_runtime/cli.test.mjs`. No Rust, no `symbolicate.tw`, no `section.tw`, no host/ABI change.
- After editing any `.tw`: `target/twk fmt <file>` then `target/twk lint boot/main.tw`.
- Never run full `cargo test`. Boot suite: `target/twk run boot/tests/main.tw` (ends with `Ran N tests: N passed`).
- Heavy commands (`make bundle-cli`) run **sequentially, foreground, never backgrounded/concurrent**.
- The boot suite and `cli.test.mjs`'s `buildRenderer()` both compile the renderer/tests from **source** with the current compiler, so Task 1's logic is exercised **without** any rebuild. `make bundle-cli` is only needed once (Task 2) to ship `target/renderer.wasm` for the real `target/twk`.
- **User frame predicate (load-bearing):** a resolved frame is a user frame iff `rf.location` is `.Some(loc)` and `!loc.file.starts_with("@")`. Non-`@` = user; `@std/…`/`@extern/…` = not user; unresolved (no location) = not user.
- Commit style: imperative subject, what/why/how, no line/count metrics. Trailers on every commit:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01FmurxQ6UfDKcPb5T9fWjfY
  ```

## File Structure

- `boot/lib/debug/trace.tw` — **modify.** Add `is_user_frame` + `user_frames`; in `render_trace` compute `chosen` (user frames, or all resolved as fallback) and pass it to `primary_label` and `backtrace_lines`; fix the module doc comment. `primary_label`/`backtrace_lines` keep their `Vector<ResolvedFrame>` signatures and bodies.
- `boot/tests/suites/trace_render_suite.tw` — **modify.** Add a mixed user/`@std` section helper and three cases (user-primary, interior-`@`-dropped, all-`@` fallback). Existing cases stay green.
- `tools/js_runtime/cli.test.mjs` — **modify.** Flip the existing prelude-`error()` test (it encodes the old deferred behavior) to assert the user-call-site snippet.

Not touched (confirmed still-green): `symbolicate.tw`, `section.tw`, `runtime_trace_renderer_suite.tw` (its only mapped-frame case uses a user-only section → primary unchanged), and all other suites.

---

## Task 1: User-frame primary + user-only backtrace in `trace.tw`

**Files:**
- Modify: `boot/lib/debug/trace.tw`
- Test: `boot/tests/suites/trace_render_suite.tw`

**Interfaces:**
- Consumes: `symbolicate.ResolvedFrame` (`.{ frame: Frame, span: Span?, location: FrameLoc? }`) and `FrameLoc` (`.{ file: String, line: Int, column: Int }`) — both already imported into `trace.tw`.
- Produces: private `fn is_user_frame(rf: ResolvedFrame) Bool` and `fn user_frames(resolved: Vector<ResolvedFrame>) Vector<ResolvedFrame>`; `render_trace`'s public signature is unchanged.

- [ ] **Step 1: Add the failing tests** — in `boot/tests/suites/trace_render_suite.tw`, add a mixed-section helper just after the existing `sample()` (around line 68):

```tw
// A section with a user file (id 0) and an @-logical stdlib file (id 1), each
// owning its own function body, so different frames resolve to different files.
// frame_at(200,_) -> @std/io.tw:19:3 ; frame_at(110,_) -> src/a.tw:2:1 ;
// frame_at(100,_) -> src/a.tw:1:1.
fn sample_mixed() DebugSection {
  files: Vector<FileEntry> = [.{ file_id: 0, path: rel_path }, .{ file_id: 1, path: "@std/io.tw" }]
  funcs: Vector<FuncDebug> = [
    .{
      func_idx: 6,
      body_start: 100,
      lines: [
        .{ offset: 0, span: .{ file_id: 0, start: 0, end: 6 }, start_line: 1, start_col: 1, end_line: 1, end_col: 7 },
        .{ offset: 8, span: .{ file_id: 0, start: 7, end: 17 }, start_line: 2, start_col: 1, end_line: 2, end_col: 11 },
      ],
    },
    .{
      func_idx: 7,
      body_start: 200,
      lines: [
        .{ offset: 0, span: .{ file_id: 1, start: 0, end: 5 }, start_line: 19, start_col: 3, end_line: 19, end_col: 8 },
      ],
    },
  ]
  .{ files, funcs }
}
```

Then add these three `.test(...)` cases inside `suite()` (append after the last existing case, before the closing `}`):

```tw
    .test(
      "points the primary caret at the first user frame when the innermost frame is @-logical stdlib",
      fn() {
        try ensure_root()
        try write_fixture("${root}/${rel_path}", "a := 1\nb := a + 2\n")

        // Innermost frame resolves to @std/io.tw; the next resolves to the user
        // file. Primary + backtrace must prefer the user frame.
        frames: Vector<Frame> = [frame_at(200, "error"), frame_at(110, "deeper")]
        out := trace.render_trace(sample_mixed(), frames, "boom", plain(), root)
        try assert.str_contains(out, "--> ${rel_path}:2:1")
        try assert.str_contains(out, "b := a + 2")
        try assert.str_contains(out, "^^^^^^^^^^")
        try assert.str_contains(out, "at deeper (${rel_path}:2:1)")
        try assert.is_false(out.contains("@std/io.tw"))
        try assert.is_false(out.contains("at error"))
        .Ok({})
      },
    )
    .test(
      "keeps both user frames and drops an interior @-logical frame from the backtrace",
      fn() {
        try ensure_root()
        try write_fixture("${root}/${rel_path}", "a := 1\nb := a + 2\n")

        // user(deeper) -> @std(error) -> user(main): both user frames stay, the
        // interior stdlib frame is dropped.
        frames: Vector<Frame> = [frame_at(110, "deeper"), frame_at(200, "error"), frame_at(100, "main")]
        out := trace.render_trace(sample_mixed(), frames, "boom", plain(), root)
        try assert.str_contains(out, "--> ${rel_path}:2:1")
        try assert.str_contains(out, "at deeper (${rel_path}:2:1)")
        try assert.str_contains(out, "at main (${rel_path}:1:1)")
        try assert.is_false(out.contains("@std/io.tw"))
        .Ok({})
      },
    )
    .test(
      "falls back to all resolved frames when every frame is @-logical, never empty",
      fn() {
        // An all-stdlib trap (no user frame): must still show the @ location(s),
        // not the empty "(no Twinkle stack trace available)" note.
        frames: Vector<Frame> = [frame_at(200, "error"), frame_at(200, "inner")]
        out := trace.render_trace(sample_mixed(), frames, "boom", plain(), root)
        try assert.str_contains(out, "error: boom")
        try assert.is_false(out.contains("no Twinkle stack trace available"))
        try assert.is_false(out.contains("-->"))
        try assert.str_contains(out, "at error (@std/io.tw:19:3)")
        try assert.str_contains(out, "source unavailable")
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run, verify the new cases fail** — `target/twk run boot/tests/main.tw`. Expected: the first two new cases FAIL (current code makes the innermost `@std/io.tw` frame primary → snippet points at `@std`/location-only, and the backtrace still contains `@std/io.tw` / `at error`). The all-`@` fallback case may already pass (it matches current behavior) — that's fine; it is a guard against regressing the fallback in Step 3.

- [ ] **Step 3: Implement** — edit `boot/lib/debug/trace.tw`.

  (a) Add the two helpers just above `primary_label` (after the `use` block, ~line 25):

```tw
/// A frame counts as a user frame when it resolved to a source location whose
/// file is a real project-relative path — i.e. not an `@`-logical stdlib,
/// prelude, or extern name (M2's path convention). Unresolved frames (no
/// location) are never user frames.
fn is_user_frame(rf: ResolvedFrame) Bool {
  case rf.location {
    .Some(loc) => !loc.file.starts_with("@"),
    .None => false,
  }
}

/// The user-frame subset of `resolved`, preserving stack order.
fn user_frames(resolved: Vector<ResolvedFrame>) Vector<ResolvedFrame> {
  out: Vector<ResolvedFrame> = []

  for rf in resolved {
    if is_user_frame(rf) {
      out = .append(rf)
    }
  }

  out
}
```

  (b) In `render_trace`, replace the two lines that currently read

```tw
  resolved := symbolicate.symbolicate(section, frames)
  primary := primary_label(resolved)
```

  with

```tw
  resolved := symbolicate.symbolicate(section, frames)
  // Prefer user frames for both the primary snippet and the backtrace so an
  // `error()` (or any stdlib-internal) trap points at the user's call site, not
  // the prelude. Fall back to all resolved frames when none are user frames, so
  // an all-stdlib trap still shows a location instead of nothing.
  users := user_frames(resolved)
  chosen := if users.len() > 0 {
    users
  } else {
    resolved
  }
  primary := primary_label(chosen)
```

  (c) Change the backtrace line from `bt := backtrace_lines(resolved)` to `bt := backtrace_lines(chosen)`. Leave `primary_label`, `backtrace_lines`, `snippet_ok`, and the three output branches exactly as they are.

  (d) Update the module `//!` doc comment (currently lines ~7–8 claiming "Frames that do not map to Twinkle source ... are dropped") to describe the actual behavior, e.g.:

```tw
//! renders only the user-meaningful frames: the primary snippet/caret and the
//! backtrace are taken from the frames that resolve to a real project source
//! file (a non-`@` path), so a trap through the prelude `error` shim or a
//! stdlib internal points at the user's call site. When a trap resolves to no
//! user frame at all (an all-stdlib stack), it falls back to every resolved
//! frame so a location still renders; frames that resolve to nothing are
//! dropped from the backtrace as before.
```

- [ ] **Step 4: Run, verify all pass** — `target/twk run boot/tests/main.tw`. Expected: the three new cases PASS and every pre-existing case in `trace_render_suite`, `runtime_trace_renderer_suite`, and the rest of the suite still PASS (the existing user-only and single-`@`-frame cases are unaffected by the user-frame preference). Confirm the final line reports all tests passed.

- [ ] **Step 5: fmt + lint** — `target/twk fmt boot/lib/debug/trace.tw boot/tests/suites/trace_render_suite.tw` then `target/twk lint boot/main.tw` (the ~4 pre-existing findings in `section.tw`/`builtins.tw`/`census.tw` are unrelated; introduce no new ones in the touched files).

- [ ] **Step 6: Commit**

```bash
git add boot/lib/debug/trace.tw boot/tests/suites/trace_render_suite.tw
git commit -m "feat(debug): render the user call site as the trap trace primary

Prefer user frames (non-@ resolved path) for the primary snippet/caret and
the backtrace, so error() and stdlib-internal traps point at the user's call
site instead of the prelude. Fall back to all resolved frames when no user
frame exists, so an all-stdlib trap still shows a location."
# (append the two required trailers)
```

---

## Task 2: Flip the CLI e2e and ship the renderer

**Files:**
- Modify: `tools/js_runtime/cli.test.mjs`
- Build artifact: `target/renderer.wasm` (via `make bundle-cli`)

**Interfaces:**
- Consumes: Task 1's `trace.tw` change (already committed). `buildRenderer(dir)` in `cli.test.mjs` compiles `boot/runtime_trace_renderer.tw --lib` from source with the current bundled compiler, so it includes Task 1's logic.

- [ ] **Step 1: Flip the failing e2e** — in `tools/js_runtime/cli.test.mjs`, find the test titled `"twk run degrades to a location-only trace when the trap surfaces through the prelude error() shim"` (~line 77). Replace that whole `test(...)` block with the one below. It keeps the same `boom`/`error("kaboom")` fixture (the `error(...)` call is on line 2) but asserts the new user-call-site behavior:

```js
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
```

- [ ] **Step 2: Run the e2e, verify it passes** — `deno test -A tools/js_runtime/cli.test.mjs`. Expected: the flipped test PASSES (its `buildRenderer` picks up Task 1's `trace.tw` from source), and the other cli.test cases still pass. (`buildRenderer` uses the current `tools/js_runtime/boot.wasm`; it compiles `trace.tw` from disk, so no rebuild is needed for this step.)

- [ ] **Step 3: Ship the renderer** — sequentially, foreground:
```bash
make bundle-cli
cp target/boot.wasm tools/js_runtime/boot.wasm
```
`make bundle-cli` rebuilds `target/renderer.wasm` (and `target/twk`) from the updated source so the shipped compiler renders the new primary. (`bundle-cli` runs the self-host loop; do not background it.) Gate: `target/twk run boot/tests/main.tw` green.

- [ ] **Step 4: Manual confirmation through the shipped `target/twk`** — create a scratch `error(...)` program in a temp dir with no `twinkle.toml` and run it:
```bash
printf 'fn boom(n: Int) Int {\n  if n <= 0 { error("kaboom") }\n  boom(n - 1)\n}\n\nx := boom(2)\n' > /tmp/twk_p41_trap.tw
target/twk run /tmp/twk_p41_trap.tw; echo "exit=$?"
```
Expected: stderr shows `error: kaboom`, a `--> …trap…:2:` snippet header with a caret, the backtrace lists only `boom` (no `@std`), and `exit=1`. Also confirm a normal program still exits 0 (`target/twk run examples/fizzbuzz.tw`). Capture this output for the report.

- [ ] **Step 5: Commit**
```bash
git add tools/js_runtime/cli.test.mjs
# also add any tracked rebuild byproducts bundle-cli regenerated (e.g. tools/bridge.wasm,
# tools/js_runtime/bridge_bytes.mjs) — check `git status --short`; do NOT add gitignored
# artifacts (target/*, tools/js_runtime/boot.wasm, boot/lib/module/core_lib.tw).
git commit -m "test(debug): assert twk run renders the user error() call site

Flip the prelude-error() e2e from the old location-only-on-prelude behavior
to the Phase 4.1 behavior: a full snippet + caret on the user's error() call
site, stdlib frame suppressed. Rebuild target/renderer.wasm to ship it."
# (append the two required trailers)
```

---

## Self-Review

**Spec coverage** (against `docs/plans/runtime-trace-user-frame-primary.md`):
- User-frame predicate (non-`@` resolved path) → Task 1 `is_user_frame`. ✓
- Two-tier selection (user frames, else all resolved) driving primary AND backtrace → Task 1 `chosen` wired into `primary_label`/`backtrace_lines`. ✓
- All-`@` fallback never empty, location-only → Task 1 test "falls back to all resolved frames…". ✓
- Policy stays in `trace.tw`; `symbolicate.tw` untouched → Task 1 File Structure. ✓
- Doc-comment correction → Task 1 Step 3(d). ✓
- Boot test cases (user-primary, interior-`@`, all-`@`, plus existing user-only & nothing-resolves regressions) → Task 1 Steps 1/4. ✓
- Flip the existing CLI e2e to user-call-site snippet → Task 2 Step 1. ✓
- Ship via one `make bundle-cli`; no ABI/bootstrap sequencing → Task 2 Step 3. ✓
- Out-of-scope items (demangling, "runtime" prefix, ANSI parity) → correctly absent. ✓

**Placeholder scan:** None — every code and test block is concrete; the two helpers, the `render_trace` edits, all three boot cases, and the full flipped e2e are spelled out.

**Type consistency:** `is_user_frame(rf: ResolvedFrame) Bool` and `user_frames(Vector<ResolvedFrame>) Vector<ResolvedFrame>` match `ResolvedFrame`/`FrameLoc` as defined in `symbolicate.tw`. `chosen` is a `Vector<ResolvedFrame>`, the exact type `primary_label` and `backtrace_lines` already accept — their signatures are unchanged. `frame_at`/`sample_mixed`/`ensure_root`/`write_fixture`/`plain`/`rel_path`/`root` are the existing helpers in `trace_render_suite.tw`. `sample_mixed` returns `DebugSection` built from `FileEntry`/`FuncDebug` exactly as `sample_at` does.
