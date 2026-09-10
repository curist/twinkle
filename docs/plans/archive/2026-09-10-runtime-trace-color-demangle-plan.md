# Runtime trace color + name demangling — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give source-mapped runtime trap traces the same ANSI color behavior as compile diagnostics, and render backtrace frame names as readable identifiers instead of mangled Wasm symbols.

**Architecture:** Renderer-only changes in `boot/lib/debug/` and `boot/runtime_trace_renderer.tw`. A new pure `lib.debug.demangle` module turns a mangled name into a label; `lib.debug.trace` calls it in `backtrace_lines`. Color flips from a hardcoded `.{ color: false }` to `report.default_config()`. No ABI change (the renderer's 4-arg entry point is untouched) and no bootstrap sequencing.

**Tech Stack:** Twinkle (`.tw`) self-hosted compiler sources; boot test suite via `target/twk run boot/tests/main.tw`; JS runtime tests via `deno test -A tools/js_runtime/`.

## Global Constraints

- Design spec: `docs/plans/2026-09-10-runtime-trace-color-demangle.md`.
- Import `demangle.tw` into `trace.tw` with a **dot-relative** module import: `use .demangle.{demangle_frame_name}` (both live under `lib.debug`).
- After editing any `.tw`: run `target/twk fmt <file>`, then `target/twk lint boot/main.tw` (~4 pre-existing findings in section.tw/builtins.tw/census.tw are unrelated — ignore only those).
- Renderer-only changes are picked up by `target/twk run boot/tests/main.tw` without a rebuild. `make bundle-cli` is needed only once, at the end, to ship `target/renderer.wasm` into the real `target/twk` for manual verification.
- Heavy commands (`make bundle-cli`) run sequentially, foreground, never backgrounded/concurrent. Never run full `cargo test`.
- Commits: imperative subject, what/why/how, no line/count metrics, with trailers:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01FmurxQ6UfDKcPb5T9fWjfY
  ```
- Twinkle notes: `for cond { ... }` is the while-loop form; `and`/`or` are logical; iterating a `String` (`for b in s` / `collect b in s`) yields `Byte`; `Byte.to_int()` gives the code point; `String` has `.index_of(needle) Int?`, `.slice(start, end) String`, `.len() Int`, `.starts_with(prefix) Bool`, `.contains(sub) Bool`, `.concat(s) String`, `.utf8_bytes() Vector<Byte>`; `String.from_char_code(c) String?`. Escape char in string literals is `\e`.

## File Structure

- **Create** `boot/lib/debug/demangle.tw` — pure `demangle_frame_name(String) String`. One responsibility: mangled Wasm name → readable label.
- **Create** `boot/tests/suites/demangle_suite.tw` — unit tests for the above.
- **Modify** `boot/lib/debug/trace.tw` — call `demangle_frame_name` in `backtrace_lines`.
- **Modify** `boot/tests/suites/trace_render_suite.tw` — add an integration case with mangled frame names.
- **Modify** `boot/runtime_trace_renderer.tw` — color via `report.default_config()`.
- **Modify** `boot/tests/suites/runtime_trace_renderer_suite.tw` — ANSI-robust content assertions + a color-parity assertion.
- **Modify** `boot/tests/main.tw` — register `demangle_suite`.

---

### Task 1: `demangle_frame_name` module + unit suite

Pure function, testable in complete isolation. This is Phase 4.2's core.

**Files:**
- Create: `boot/lib/debug/demangle.tw`
- Create: `boot/tests/suites/demangle_suite.tw`
- Modify: `boot/tests/main.tw` (register the suite)

**Interfaces:**
- Produces: `pub fn demangle_frame_name(raw: String) String` in module `lib.debug.demangle`.
  - `"user__$f350_cell"` → `"cell"`
  - `"user__$f352__init"` → `"<script>"`
  - `"user__$f7___lambda"` → `"<closure>"`, `"user__$f7___lambda__Int"` → `"<closure>"`
  - `"user__$f361_sort_by__Int"` → `"sort_by"`
  - `"user__$f5_my__thing"` → `"my__thing"` (a genuine double-underscore user name is preserved)
  - `"std_view__$f12_from"` → `"from"` (any namespace prefix is dropped)
  - `"rt_arr__get"` → `"rt_arr__get"` (no `$f<digits>_` marker → returned unchanged)

- [ ] **Step 1: Write the suite file (failing test)**

Create `boot/tests/suites/demangle_suite.tw`:

```tw
use @std.testing.assert as assert
use @std.testing as runner

use lib.debug.demangle

pub fn suite() runner.Suite {
  runner
    .suite("demangle")
    .test(
      "strips the namespace and $fN_ prefix from a regular function",
      fn() {
        try assert.equal(demangle.demangle_frame_name("user__$f350_cell"), "cell")
        .Ok({})
      },
    )
    .test(
      "maps the synthetic top-level body to <script>",
      fn() {
        try assert.equal(demangle.demangle_frame_name("user__$f352__init"), "<script>")
        .Ok({})
      },
    )
    .test(
      "maps a hoisted lambda to <closure>",
      fn() {
        try assert.equal(demangle.demangle_frame_name("user__$f7___lambda"), "<closure>")
        .Ok({})
      },
    )
    .test(
      "maps a monomorphized lambda to <closure>",
      fn() {
        try assert.equal(demangle.demangle_frame_name("user__$f7___lambda__Int"), "<closure>")
        .Ok({})
      },
    )
    .test(
      "strips a monomorphization suffix to the base name",
      fn() {
        try assert.equal(demangle.demangle_frame_name("user__$f361_sort_by__Int"), "sort_by")
        .Ok({})
      },
    )
    .test(
      "keeps a genuine double-underscore user name intact",
      fn() {
        try assert.equal(demangle.demangle_frame_name("user__$f5_my__thing"), "my__thing")
        .Ok({})
      },
    )
    .test(
      "drops a non-user namespace prefix too",
      fn() {
        try assert.equal(demangle.demangle_frame_name("std_view__$f12_from"), "from")
        .Ok({})
      },
    )
    .test(
      "leaves a name without the $f marker unchanged",
      fn() {
        try assert.equal(demangle.demangle_frame_name("rt_arr__get"), "rt_arr__get")
        .Ok({})
      },
    )
}
```

Register it in `boot/tests/main.tw`: add `use .suites.demangle_suite` alongside the other `use .suites.*` lines (kept alphabetical near the `d*` entries), and add `demangle_suite.suite(),` to the `runner.run_all([...])` list.

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -30`
Expected: FAIL — `lib.debug.demangle` does not resolve / `demangle_frame_name` unknown (module not created yet).

- [ ] **Step 3: Create the module (minimal implementation)**

Create `boot/lib/debug/demangle.tw`:

```tw
//! Demangle Wasm function names into readable backtrace labels.
//!
//! Codegen mangles every user function into `<ns>__$f<id>_<sanitized_name>`
//! (see `codegen/emit.tw` and `codegen/linker.tw`): a namespace prefix
//! (`user`, `std_view`, …), a `$f<func-id>_` marker, then the source name with
//! every non-`[A-Za-z0-9_]` byte mapped to `_`. This turns that back into a
//! name a human recognizes in a trap backtrace — dropping the prefix and id,
//! naming the synthetic top-level body (`$init` → `<script>`) and hoisted
//! lambdas (`__lambda` → `<closure>`), and stripping monomorphization suffixes
//! (`sort_by__Int` → `sort_by`).

fn is_digit(c: Int) Bool {
  c >= 48 and c <= 57
}

fn is_upper(c: Int) Bool {
  c >= 65 and c <= 90
}

// Strip a monomorphization suffix from a base name. Suffixes come from
// `monomorphize.tw` type keys and are always PascalCase-leading (`Int`,
// `Vec_Int`, `T7_Int`, `Fn_..`, …), joined onto the base with `__`. User names
// are snake_case, so we cut at the first `__` immediately followed by an
// uppercase ASCII letter; a `__` followed by lowercase (`my__thing`) is part of
// the name and left intact.
fn strip_mono_suffix(tail: String) String {
  bytes := tail.utf8_bytes()
  i := 0

  for i + 2 < bytes.len() {
    if bytes[i].to_int() == 95 and bytes[i + 1].to_int() == 95 and is_upper(bytes[i + 2].to_int()) {
      return tail.slice(0, i)
    }

    i = i + 1
  }

  tail
}

fn demangle_tail(tail: String) String {
  if tail == "_init" {
    "<script>"
  } else if tail.starts_with("__lambda") {
    "<closure>"
  } else {
    strip_mono_suffix(tail)
  }
}

/// Turn a mangled Wasm function name into a readable backtrace label. A name
/// without the `$f<digits>_` marker (externs, `rt_*` helpers, raw exports) is
/// returned unchanged.
pub fn demangle_frame_name(raw: String) String {
  case raw.index_of("$f") {
    .None => raw,
    .Some(marker) => {
      bytes := raw.utf8_bytes()
      digit_start := marker + 2
      i := digit_start

      for i < bytes.len() {
        if !is_digit(bytes[i].to_int()) {
          break
        }

        i = i + 1
      }

      // Require at least one digit after `$f` and a following `_` separator.
      if i == digit_start or i >= bytes.len() or bytes[i].to_int() != 95 {
        raw
      } else {
        demangle_tail(raw.slice(i + 1, raw.len()))
      }
    },
  }
}
```

- [ ] **Step 4: Format and lint**

Run: `target/twk fmt boot/lib/debug/demangle.tw boot/tests/suites/demangle_suite.tw && target/twk lint boot/main.tw`
Expected: fmt idempotent; lint reports only the ~4 known-unrelated findings.

- [ ] **Step 5: Run to verify it passes**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -20`
Expected: PASS — the `demangle` suite's 8 tests pass; no other suite regresses.

- [ ] **Step 6: Commit**

```bash
git add boot/lib/debug/demangle.tw boot/tests/suites/demangle_suite.tw boot/tests/main.tw
git commit -F - <<'EOF'
feat(debug): demangle Wasm frame names into readable labels

Backtrace lines print raw codegen symbols like user__$f350_cell. Add a pure
lib.debug.demangle module that strips the namespace/$fN_ prefix, names the
synthetic top-level body (<script>) and hoisted lambdas (<closure>), and
strips PascalCase-leading monomorphization suffixes to the base name. Names
without the $f marker pass through unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FmurxQ6UfDKcPb5T9fWjfY
EOF
```

---

### Task 2: Wire demangling into the backtrace

Route resolved frame names through `demangle_frame_name` where the backtrace is built, and prove it end to end through `render_trace`.

**Files:**
- Modify: `boot/lib/debug/trace.tw` (import + `backtrace_lines`)
- Modify: `boot/tests/suites/trace_render_suite.tw` (integration case)

**Interfaces:**
- Consumes: `demangle_frame_name(raw: String) String` from `lib.debug.demangle` (Task 1).
- Produces: no new exports. `backtrace_lines` now emits demangled `at <name> (...)` lines.

- [ ] **Step 1: Add the failing integration test**

In `boot/tests/suites/trace_render_suite.tw`, add a test inside the `suite()` chain (after the existing "appends an ordered backtrace of resolved frames" test). `frame_at` and `plain()` already exist in this file; `sample()` maps `frame_at(110,_)` → line 2 and `frame_at(100,_)` → line 1.

```tw
    .test(
      "demangles mangled frame names in the backtrace",
      fn() {
        try ensure_root()
        try write_fixture("${root}/${rel_path}", "a := 1\nb := a + 2\n")

        frames: Vector<Frame> = [
          frame_at(110, "user__$f350_cell"),
          frame_at(100, "user__$f352__init"),
        ]
        out := trace.render_trace(sample(), frames, "boom", plain(), root)
        try assert.str_contains(out, "at cell (${rel_path}:2:1)")
        try assert.str_contains(out, "at <script> (${rel_path}:1:1)")
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -30`
Expected: FAIL — output still contains `at user__$f350_cell (...)`, so `str_contains(out, "at cell (...)")` fails.

- [ ] **Step 3: Wire demangling into `backtrace_lines`**

In `boot/lib/debug/trace.tw`, add the dot-relative import next to the other `use .` lines (near `use .symbolicate`):

```tw
use .demangle.{demangle_frame_name}
```

Then in `backtrace_lines`, run the resolved name through the demangler:

```tw
        name := case rf.frame.name {
          .Some(n) => demangle_frame_name(n),
          .None => "<anonymous>",
        }
```

- [ ] **Step 4: Format and lint**

Run: `target/twk fmt boot/lib/debug/trace.tw boot/tests/suites/trace_render_suite.tw && target/twk lint boot/main.tw`
Expected: fmt idempotent; lint reports only the known-unrelated findings.

- [ ] **Step 5: Run to verify it passes**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -20`
Expected: PASS — the new case passes; the existing `trace render` cases (which use non-`$f` names like `deeper`/`main`, unaffected by demangling) still pass.

- [ ] **Step 6: Commit**

```bash
git add boot/lib/debug/trace.tw boot/tests/suites/trace_render_suite.tw
git commit -F - <<'EOF'
feat(debug): demangle frame names in runtime backtraces

Route each resolved frame's name through demangle_frame_name in
backtrace_lines so a trap backtrace shows `at cell` / `at <script>` instead
of `at user__$f350_cell`. The nameless-frame `<anonymous>` fallback is kept.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FmurxQ6UfDKcPb5T9fWjfY
EOF
```

---

### Task 3: ANSI color parity (Phase 4.4)

Flip the runtime-trace renderer to `report.default_config()` and make the integration suite robust to the now-present ANSI codes.

**Files:**
- Modify: `boot/runtime_trace_renderer.tw` (color config)
- Modify: `boot/tests/suites/runtime_trace_renderer_suite.tw` (ANSI-robust assertions + parity check)

**Interfaces:**
- Consumes: `default_config()` from `lib.source.report` (existing; returns `color: true` unless `NO_COLOR` is set non-empty).
- Produces: no signature change. `render_runtime_trace` output now carries ANSI codes when color is enabled.

- [ ] **Step 1: Make the suite ANSI-robust + add the parity test (write first)**

Edit `boot/tests/suites/runtime_trace_renderer_suite.tw`.

Add an import for `report` near the other `use` lines at the top:

```tw
use lib.source.report
```

Add a `strip_ansi` helper (the fixtures are ASCII, so per-byte reassembly is exact) beside the other fixture helpers:

```tw
// Remove ANSI SGR escape sequences (`\e[...m`) so content assertions hold
// whether or not color is enabled. Fixtures are ASCII, so per-byte reassembly
// is exact.
fn strip_ansi(s: String) String {
  out := ""
  in_escape := false

  for ch in s {
    c := ch.to_int()

    if in_escape {
      if c == 109 {
        in_escape = false
      }
    } else if c == 27 {
      in_escape = true
    } else {
      case String.from_char_code(c) {
        .Some(x) => out = out.concat(x),
        .None => {},
      }
    }
  }

  out
}
```

Rewrite the existing tests to assert against `strip_ansi(out)`. Concretely, in each test replace the `out := renderer.render_runtime_trace(...)` line's downstream assertions to run on a `stripped` binding. The four content-asserting tests become:

Test "renders a source-mapped trace ...":
```tw
        out := renderer.render_runtime_trace(module_bytes(), deno_stack(), "boom", root)
        stripped := strip_ansi(out)
        try assert.str_contains(stripped, "error: boom")
        try assert.str_contains(stripped, "--> ${rel_path}:2:1")
        try assert.str_contains(stripped, "b := a + 2")
        try assert.str_contains(stripped, "^^^^^^^^^^")

        try assert.str_contains(stripped, "at user__deeper (${rel_path}:2:1)")
        try assert.str_contains(stripped, "at user__main (${rel_path}:1:1)")
        deeper_at := try stripped.index_of("user__deeper").ok_or("deeper missing")
        main_at := try stripped.index_of("user__main").ok_or("main missing")
        try assert.is_true(deeper_at < main_at)
        .Ok({})
```

Test "degrades to location-only ... not on disk":
```tw
        out := renderer.render_runtime_trace(missing_module_bytes(), deno_stack(), "boom", root)
        stripped := strip_ansi(out)
        try assert.str_contains(stripped, "error: boom")
        try assert.is_false(stripped.contains("-->"))
        try assert.is_false(stripped.contains("b := a + 2"))
        try assert.str_contains(stripped, "at user__deeper (${missing_rel_path}:2:1)")
        try assert.str_contains(stripped, "at user__main (${missing_rel_path}:1:1)")
        try assert.str_contains(stripped, "source unavailable")
        .Ok({})
```

Test "degrades to location-only when source_root is empty ...":
```tw
        out := renderer.render_runtime_trace(module_bytes(), deno_stack(), "boom", "")
        stripped := strip_ansi(out)
        try assert.str_contains(stripped, "error: boom")
        try assert.is_false(stripped.contains("-->"))
        try assert.str_contains(stripped, "at user__deeper (${rel_path}:2:1)")
        try assert.str_contains(stripped, "source unavailable")
        .Ok({})
```

Test "falls back to a note when the module carries no debug section":
```tw
        garbage: Vector<Byte> = [b(1), b(2), b(3)]
        out := renderer.render_runtime_trace(garbage, deno_stack(), "boom", root)
        stripped := strip_ansi(out)
        try assert.str_contains(stripped, "error: boom")
        try assert.str_contains(stripped, "no Twinkle stack trace available")
        .Ok({})
```

Test "falls back to a note when the stack carries no Twinkle frames":
```tw
        host_only := "Error: boom\n    at run (file:///host.js:20:3)"
        out := renderer.render_runtime_trace(module_bytes(), host_only, "boom", root)
        stripped := strip_ansi(out)
        try assert.str_contains(stripped, "error: boom")
        try assert.str_contains(stripped, "no Twinkle stack trace available")
        .Ok({})
```

(The "extracts the debug section past other content" test calls `section.decode_module` directly, not the renderer — leave it unchanged.)

Add a new parity test to the chain:
```tw
    .test(
      "colors output per default_config (NO_COLOR parity)",
      fn() {
        out := renderer.render_runtime_trace(module_bytes(), deno_stack(), "boom", root)

        if report.default_config().color {
          try assert.is_true(out.contains("\e["))
        } else {
          try assert.is_false(out.contains("\e["))
        }
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify the parity test fails (color still forced off)**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -30`
Expected: In a normal shell (`NO_COLOR` unset), `default_config().color` is `true` but the renderer still forces `color: false`, so `out.contains("\e[")` is false → the parity test FAILS. (The rewritten content tests pass either way — `strip_ansi` is a no-op on already-monochrome output.)

- [ ] **Step 3: Switch the renderer to `default_config()`**

In `boot/runtime_trace_renderer.tw`, add `default_config` to the report import:

```tw
use lib.source.report.{RenderConfig, Report, default_config}
```

Replace the `render_config` function (delete the stale "swap this for" comment):

```tw
fn render_config() RenderConfig {
  default_config()
}
```

- [ ] **Step 4: Format and lint**

Run: `target/twk fmt boot/runtime_trace_renderer.tw boot/tests/suites/runtime_trace_renderer_suite.tw && target/twk lint boot/main.tw`
Expected: fmt idempotent; lint reports only the known-unrelated findings.

- [ ] **Step 5: Run to verify it passes**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -20`
Expected: PASS — parity test passes (color on → escape codes present); all content tests pass via `strip_ansi`.

To also confirm the `NO_COLOR` branch:
Run: `NO_COLOR=1 target/twk run boot/tests/main.tw 2>&1 | tail -20`
Expected: PASS — parity test takes the `is_false` branch; content tests still pass.

- [ ] **Step 6: Commit**

```bash
git add boot/runtime_trace_renderer.tw boot/tests/suites/runtime_trace_renderer_suite.tw
git commit -F - <<'EOF'
feat(debug): color runtime trap traces like compile diagnostics

Runtime traces rendered monochrome because the renderer forced color:false.
Use report.default_config() so traces follow the same NO_COLOR rule as every
compile diagnostic. Make the renderer suite's content assertions ANSI-robust
via a strip_ansi helper and add a parity check driven by default_config, so
it holds whether or not color is enabled.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01FmurxQ6UfDKcPb5T9fWjfY
EOF
```

---

### Task 4: Ship and verify end to end

No code changes — build the shipped renderer and confirm color + demangling in the real CLI and the JS runtime. This is a verification gate worth its own review.

**Files:** none (build + verification only).

- [ ] **Step 1: Rebuild the standalone CLI (ships `target/renderer.wasm`)**

Run: `make bundle-cli`
Expected: completes; `target/twk` and `target/renderer.wasm` refreshed. (Sequential, foreground.)

- [ ] **Step 2: Manual end-to-end trap check (color + demangled names)**

```bash
cat > /tmp/trap_demo.tw <<'TW'
fn cell(xs: Vector<Int>, i: Int) Int {
  xs[i]
}

fn go() Int {
  xs := [1, 2, 3]
  cell(xs, 7)
}

println(go())
TW
target/twk run /tmp/trap_demo.tw 2>&1 | cat -v
```
Expected: `error: index 7 out of bounds for length 3`, a source snippet + caret, and a backtrace reading `at cell (...)`, `at go (...)`, `at <script> (...)` — no `user__$f...` names. With `cat -v`, ANSI codes (`^[[...m`) are visible around the header/gutter/caret. Then confirm `NO_COLOR=1 target/twk run /tmp/trap_demo.tw 2>&1 | cat -v` shows the same text with no `^[[` sequences.

- [ ] **Step 3: JS runtime tests**

```bash
cp target/boot.wasm tools/js_runtime/boot.wasm
deno test -A tools/js_runtime/
```
Expected: pass. (A pre-existing `web.test.mjs` env-quirk failure is unrelated — confirm any failure is only that one.)

- [ ] **Step 4: Whole-branch review**

Use `superpowers:requesting-code-review` over the branch diff (`git diff main...HEAD`) before finishing. Address findings, then use `superpowers:finishing-a-development-branch` to decide integration.

---

## Self-Review

**Spec coverage:**
- Phase 4.4 color → Task 3. ✓
- Phase 4.2 demangle (module, algorithm, dot-relative import, readable-name choices) → Tasks 1 & 2. ✓
- Test plan: demangle unit suite → Task 1; trace_render integration → Task 2; runtime_trace_renderer ANSI-robustness + parity → Task 3. ✓
- Verification sequence (fmt/lint, boot suite, bundle-cli + manual, JS tests) → per-task + Task 4. ✓

**Placeholder scan:** No TBD/TODO/"handle edge cases"/"similar to Task N"; every code step shows the actual content. ✓

**Type consistency:** `demangle_frame_name(raw: String) String` defined in Task 1, consumed in Task 2 with matching signature; `strip_ansi(s: String) String` defined and used within Task 3; `default_config() RenderConfig` matches its existing definition in `report.tw`. ✓
