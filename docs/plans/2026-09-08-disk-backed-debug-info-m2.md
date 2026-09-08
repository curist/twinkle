# Disk-Backed Debug Info — Milestone 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the `twinkle.debug` section portable — store **project-relative** paths (with an `@`-prefix marking logical stdlib/prelude/extern names), and give the `twk run` renderer a **`source_root`** to safe-join them from disk — so shipped artifacts carry no absolute build paths and relocate cleanly.

**Architecture:** Absolutize the project root once, at the shared producer layer (`module_compiler`), and classify each source path against it into a project-relative or `@`-logical string stored in the section (format v3). The `twk run` command threads that absolute root through the `run_wasm` host boundary as a 4th `render_runtime_trace` argument; the renderer safe-joins non-`@` paths under it (rejecting traversal) and reads snippets from disk, degrading to location-only whenever the root is absent, the path is logical, the join escapes, or the file is unreadable.

**Tech Stack:** Twinkle (`.tw`, boot compiler), `@std.fs`, the `run_wasm` registered host builtin (`builtins.tw` + Rust stage0 + `tools/js_runtime`), the self-host loop (`make stage2` / `bundle-cli`).

**Design:** `docs/plans/disk-backed-debug-info-m2.md` (read it — this plan implements it).

## Global Constraints

- Boot compiler in `boot/` is primary; touch Rust stage0 (`src/`) only where the `run_wasm` arity bump requires it to keep `make stage2` bootstrapping (Task 5).
- After editing any `.tw`: `target/twk fmt <file>` then `target/twk lint boot/main.tw` (and `boot/tests/main.tw` for suites).
- Heavy verification (`make stage2`, `make bundle-cli`, full suites) runs **sequentially, never concurrent/backgrounded**.
- Never run full `cargo test`; targeted filters only. Boot suite: `target/twk run boot/tests/main.tw`.
- Fresh compiler after a stage rebuild is `target/twk` only after `make bundle-cli`; before that drive the just-built compiler with `BOOT_WASM=target/boot.wasm deno run -A tools/js_runtime/deno_main.mjs <args>`, and `cp target/boot.wasm tools/js_runtime/boot.wasm` before JS tests.
- Commit style: imperative subject, what/why/how, no line/count metrics. Trailers on every commit:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01FmurxQ6UfDKcPb5T9fWjfY
  ```
- **Bootstrap discipline (Task 5):** `run_wasm` is a registered builtin; the old `target/twk` cannot compile source that *uses* a wider `run_wasm` until a compiler carrying the new abi exists. Teach the toolchain the new arity and rebuild **before** changing any `.tw` that calls it. Details in Task 5.
- Paths are **absolute end-to-end** for classification and safe-join; `@`-prefixed = logical (never disk-read); only `offset` stays delta-encoded (unchanged from M1).

## File Structure

- `boot/lib/module/loader.tw` — **modify.** `find_project_root` no-manifest fallback → entry dir.
- `boot/lib/source/pathclass.tw` — **create.** Shared `PathClass` classifier (extracted from `analyze.tw`).
- `boot/compiler/query/analyze.tw` — **modify.** `display_module_name` uses the shared classifier (display output unchanged).
- `boot/compiler/module_compiler.tw` — **modify.** Absolutize roots + canonicals, classify, store relative/`@` path in `DebugFile.path`; put absolute `project_root` on `PipelineArtifacts`.
- `boot/compiler/artifacts.tw` — **modify.** `PipelineArtifacts.project_root: String`.
- `boot/lib/debug/section.tw` — **modify.** `version()` 2 → 3.
- `boot/lib/debug/symbolicate.tw`, `boot/lib/debug/trace.tw`, `boot/runtime_trace_renderer.tw` — **modify.** 4-arg `source_root`; safe-join; `@`-skip; no-root ⇒ location-only.
- `boot/lib/debug/pathjoin.tw` (or inside `symbolicate.tw`) — **create/modify.** `safe_join`.
- `boot/stdlib/proc.tw` + `boot/lib/module/core_lib.tw` (generated) — **modify.** `run_wasm` gains `source_root`.
- `boot/compiler/builtins.tw` — **modify.** `host_run_wasm` abi gains `str_n()`.
- `src/` (Rust stage0) — **modify if required.** `run_wasm` extern arity (Task 5 verifies).
- `boot/commands/run.tw` — **modify.** Pass `artifacts.project_root` as `source_root`.
- `tools/js_runtime/runtime.mjs` — **modify.** `run_wasm` import reads the 3rd arg; thread to `childTrapHandler` → `render_runtime_trace` 4th arg.
- Test suites: `module_loader_suite.tw`, a classifier suite, `debug_section_suite.tw`, `symbolicate_suite.tw`/`trace_render_suite.tw`/`runtime_trace_renderer_suite.tw`, `tools/js_runtime/cli.test.mjs`.

---

## Task 1: `find_project_root` no-manifest fallback

**Files:** Modify `boot/lib/module/loader.tw`; Test `boot/tests/suites/module_loader_suite.tw`.

**Interfaces:**
- Produces: `find_project_root(start) String` now returns the normalized start dir (not `"."`) when no manifest is found.

- [ ] **Step 1: Failing test** — in `module_loader_suite.tw`, add a case: create a temp dir with **no** `twinkle.toml` (e.g. `/tmp/twk_m2_nomanifest_<unique>/`), assert `find_project_root("/tmp/twk_m2_nomanifest_<unique>")` returns that absolute dir, not `"."`. (Use `@std.fs.create_dir`; mirror `module_loader_suite`'s existing style.)

- [ ] **Step 2: Run, verify it fails** — `target/twk run boot/tests/main.tw` → FAIL (returns `"."`).

- [ ] **Step 3: Implement** — in `boot/lib/module/loader.tw`, preserve the origin and return it on the no-manifest path:
```tw
pub fn find_project_root(start: String) String {
  origin := path.normalize(start)
  dir := origin
  for true {
    if fs.exists(path.join(dir, "twinkle.toml")) {
      return dir
    }
    parent := path.dirname(dir)
    if parent == dir {
      return origin
    }
    dir = parent
  }
  origin
}
```

- [ ] **Step 4: Run, verify it passes** — `target/twk run boot/tests/main.tw` → the new case PASS; existing `module_loader_suite` cases (nearest-manifest) still PASS.

- [ ] **Step 5: fmt + lint + commit**
```bash
target/twk fmt boot/lib/module/loader.tw boot/tests/suites/module_loader_suite.tw
target/twk lint boot/main.tw
git add boot/lib/module/loader.tw boot/tests/suites/module_loader_suite.tw
git commit -m "fix(loader): find_project_root falls back to the entry directory"
```

---

## Task 2: Shared `PathClass` classifier

**Files:** Create `boot/lib/source/pathclass.tw`; Modify `boot/compiler/query/analyze.tw`; Test a new `boot/tests/suites/pathclass_suite.tw` (register in `boot/tests/main.tw`).

**Interfaces:**
- Produces (consumed by Tasks 2-analyze and 3-producer):
  - `pub type PathClass = { Project(String), Std(String), Prelude(String), Extern(String) }`
  - `pub fn classify(canonical: String, project_root: String, stdlib_root: String, prelude_root: String) PathClass` — checks stdlib, then prelude, then project via `strip_root`; else `Extern(basename(canonical))`. Returns the relative remainder for the first three.
  - `pub fn strip_root(canonical: String, root: String) String?` — moved here from `analyze.tw` (same logic).

- [ ] **Step 1: Failing test** — `pathclass_suite.tw`:
```tw
try assert.equal(pathclass.classify("/p/src/a.tw", "/p", "/std", "/prel"), PathClass.Project("src/a.tw"))
try assert.equal(pathclass.classify("/std/vector.tw", "/p", "/std", "/prel"), PathClass.Std("vector.tw"))
try assert.equal(pathclass.classify("/prel/io.tw", "/p", "/std", "/prel"), PathClass.Prelude("io.tw"))
try assert.equal(pathclass.classify("/other/x.tw", "/p", "/std", "/prel"), PathClass.Extern("x.tw"))
```
- [ ] **Step 2: Run, verify it fails** — module not found / function undefined.

- [ ] **Step 3: Implement `pathclass.tw`** — move `strip_root` (currently `analyze.tw:336-350`) here verbatim and add:
```tw
pub type PathClass = { Project(String), Std(String), Prelude(String), Extern(String) }

pub fn classify(canonical: String, project_root: String, stdlib_root: String, prelude_root: String) PathClass {
  case strip_root(canonical, stdlib_root) {
    .Some(rel) => return .Std(rel),
    .None => {},
  }
  case strip_root(canonical, prelude_root) {
    .Some(rel) => return .Prelude(rel),
    .None => {},
  }
  case strip_root(canonical, project_root) {
    .Some(rel) => return .Project(rel),
    .None => {},
  }
  .Extern(path.basename(canonical))
}
```

- [ ] **Step 4: Refactor `analyze.tw` to use it (display unchanged)** — replace the private `strip_root` + the inline branches in `display_module_name` (`analyze.tw:316-332`) with calls into the shared classifier, formatting the **same** display strings it produces today:
```tw
fn display_module_name(state: AnalysisState, canonical: String) String {
  case pathclass.classify(canonical, path.normalize(state.project_root),
                          state.canonical_roots.stdlib_root, state.canonical_roots.prelude_root) {
    .Std(rel) => "@std/${rel}",
    .Prelude(rel) => rel,
    .Project(rel) => rel,
    .Extern(base) => base,
  }
}
```
(Import `pathclass`; drop the now-moved private `strip_root`.)

- [ ] **Step 5: Run, verify passes** — `target/twk run boot/tests/main.tw` → `pathclass` suite PASS; existing `analyze`/diagnostic/progress tests still PASS (display output is byte-identical).

- [ ] **Step 6: fmt + lint + commit**
```bash
target/twk fmt boot/lib/source/pathclass.tw boot/compiler/query/analyze.tw boot/tests/suites/pathclass_suite.tw boot/tests/main.tw
target/twk lint boot/main.tw
git add boot/lib/source/pathclass.tw boot/compiler/query/analyze.tw boot/tests/suites/pathclass_suite.tw boot/tests/main.tw
git commit -m "refactor(source): extract shared PathClass classifier from analyze"
```

---

## Task 3: Producer relativization + format v3

**Files:** Modify `boot/compiler/module_compiler.tw`, `boot/compiler/artifacts.tw`, `boot/lib/debug/section.tw`, `boot/tests/suites/debug_section_suite.tw`; add a producer compiled-section assertion.

**Interfaces:**
- Consumes: Task 2's `pathclass.classify`.
- Produces: `PipelineArtifacts.project_root: String` (absolute); `DebugFile.path` now holds the relative/`@`-logical string; section `version() == 3`.

- [ ] **Step 1: Bump version + update `debug_section_suite`** — `section.tw`: `version()` returns `3`. Update the suite's version assertions (round-trip is version-internal; the "rejects unknown version byte" test still uses a non-3 byte). Run → the version test drives the bump.

- [ ] **Step 2: Add absolute `project_root` to artifacts** — `artifacts.tw`: add `project_root: String` to `PipelineArtifacts`.

- [ ] **Step 3: Relativize at the producer (failing producer test first)** — add a test that compiles a small fixture project (with a `twinkle.toml`) via the pipeline and decodes the emitted `twinkle.debug`, asserting the entry file's stored path is project-relative (e.g. `main.tw`), never absolute, never `@extern`. Include the **`project_root == "."`** case (fixture compiled from its own root) and a **`twk build`-style** compile (relative entry, no `run_file`). Run → FAIL (paths still absolute from M1).

- [ ] **Step 4: Implement** — in `module_compiler.tw` where `debug_files` are built (`:300-302`), with `project_root` (`:58`) and `canonical_roots` (`:59`) in scope, absolutize and classify:
```tw
abspath := fn(p: String) String {
  if path.is_absolute(p) { path.normalize(p) } else { path.normalize(path.join(proc.cwd(), p)) }
}
abs_root := abspath(project_root)
abs_std := abspath(canonical_roots.stdlib_root)
abs_prel := abspath(canonical_roots.prelude_root)

debug_files: Vector<DebugFile> = collect r in cur_cache.file_records_list() {
  rel := case pathclass.classify(abspath(r.path), abs_root, abs_std, abs_prel) {
    .Project(p) => p,
    .Std(p) => "@std/${p}",
    .Prelude(p) => "@std/${p}",
    .Extern(b) => "@extern/${b}",
  }
  .{ file_id: r.file_id, path: rel, source: r.source }
}
```
(If a locally-bound `fn` isn't accepted, inline `abspath` as a top-level helper in `module_compiler.tw`.) Then set `project_root: abs_root` on the returned `PipelineArtifacts`. `DebugFile.source` is still carried for M1's line/col precompute — unchanged. `wasm.tw`'s emit/precompute are untouched.

- [ ] **Step 5: Run, verify passes** — `target/twk run boot/tests/main.tw` → the producer test + `debug_section` PASS; the renderer suites still PASS (they build fixtures manually with absolute paths + version-internal encode/decode, unaffected by producer changes). No other suite regresses.

- [ ] **Step 6: fmt + lint + commit**
```bash
target/twk fmt boot/compiler/module_compiler.tw boot/compiler/artifacts.tw boot/lib/debug/section.tw boot/tests/suites/debug_section_suite.tw
target/twk lint boot/main.tw
git add -A
git commit -m "feat(debug): store project-relative/@-logical paths in v3 section"
```

---

## Task 4: Renderer — source_root, safe-join, degradation

**Files:** Modify `boot/lib/debug/symbolicate.tw`, `boot/lib/debug/trace.tw`, `boot/runtime_trace_renderer.tw`; add `safe_join`; update `symbolicate_suite.tw`, `trace_render_suite.tw`, `runtime_trace_renderer_suite.tw`.

**Interfaces:**
- Produces: `render_runtime_trace(bytes, stack, message, source_root: String) String`; `render_trace(section, frames, message, config, source_root)`; `build_disk_registry(section, source_root)`; `safe_join(source_root, rel) String?`.

- [ ] **Step 1: `safe_join` unit test (failing)** — table test: `safe_join("/p", "src/a.tw") == .Some("/p/src/a.tw")`; `safe_join("/p", "../etc/passwd") == .None`; `safe_join("/p", "@std/x.tw") == .None`; `safe_join("", "src/a.tw") == .None`; `safe_join("/p", "/abs/x") == .None`.

- [ ] **Step 2: Implement `safe_join`** (in `symbolicate.tw` or a small `pathjoin.tw`):
```tw
pub fn safe_join(source_root: String, rel: String) String? {
  if source_root == "" or rel.starts_with("@") {
    return .None
  }
  joined := path.normalize(path.join(source_root, rel))
  root := path.normalize(source_root)
  prefix := if root.ends_with("/") { root } else { "${root}/" }
  if joined == root or joined.starts_with(prefix) {
    .Some(joined)
  } else {
    .None
  }
}
```

- [ ] **Step 3: Renderer suites (failing)** — update the three suites to the new signatures and M2 behavior, using stable `/tmp/twk_m2_*` fixtures (per M1's portable-path rule, NOT session scratchpad): write a real file at `/tmp/twk_m2_render_<unique>/src/a.tw`, build a section whose `FileEntry.path` is the **relative** `"src/a.tw"`, and:
  - `source_root = /tmp/twk_m2_render_<unique>`, file present ⇒ snippet text + caret rendered.
  - same, file removed ⇒ location-only (`source unavailable`) + backtrace line from embedded line/col.
  - `source_root = ""` though the file exists ⇒ location-only (proves no-root never disk-reads).
  - a `FileEntry.path = "@std/vector.tw"` frame ⇒ location-only.

- [ ] **Step 4: Implement renderer changes**
  - `symbolicate.tw`: `build_disk_registry(sec, source_root)` — for each file, `case safe_join(source_root, f.path) { .Some(abs) => read+add ; .None => skip }` (was: read `f.path` directly). Location resolution still comes from embedded line/col + the stored path string (unchanged).
  - `trace.tw`: `render_trace(section, frames, message, config, source_root)` — pass `source_root` to `build_disk_registry`; `snippet_ok` unchanged in spirit (mirror `reg.line_col(primary.span)` on the disk registry).
  - `runtime_trace_renderer.tw`: `render_runtime_trace(bytes, stack, message, source_root: String)` — forward `source_root` to `render_trace`. `source.decode_module` unchanged.

- [ ] **Step 5: Run, verify passes** — `target/twk run boot/tests/main.tw` → renderer suites PASS; nothing else regresses. (Do **not** `bundle-cli` here — the shipped `target/renderer.wasm` stays M1 until Task 5; twk-run e2e is validated there.)

- [ ] **Step 6: fmt + lint + commit**
```bash
target/twk fmt boot/lib/debug/symbolicate.tw boot/lib/debug/trace.tw boot/runtime_trace_renderer.tw boot/tests/suites/symbolicate_suite.tw boot/tests/suites/trace_render_suite.tw boot/tests/suites/runtime_trace_renderer_suite.tw
target/twk lint boot/main.tw
git add -A
git commit -m "feat(debug): renderer safe-joins relative paths under source_root"
```

---

## Task 5: Root plumbing across the `run_wasm` boundary (bootstrap-sequenced)

**Files:** `boot/compiler/builtins.tw`, `src/` (Rust stage0, if required), `tools/js_runtime/runtime.mjs`, `boot/stdlib/proc.tw` + `boot/lib/module/core_lib.tw`, `boot/commands/run.tw`, `tools/js_runtime/cli.test.mjs`.

**Interfaces:**
- Produces: `run_wasm(bytes, argv, source_root) Int`; host forwards `source_root` to `render_runtime_trace`'s 4th arg; `run_file` supplies `artifacts.project_root`.

**Why two rebuilds:** `run_wasm` is a registered builtin (`builtins.tw:371`,`:750`). The current `target/twk` has the 2-arg abi; it cannot compile a 3-arg *use* of it. So first make a compiler that knows the 3-arg abi (Step A + rebuild), then change the callers (Step E + rebuild).

- [ ] **Step A: Teach the compiler + host the 3-arg abi (no caller change yet).**
  - `builtins.tw:371`: `"host_run_wasm" => abi([arr_n(), arr_n(), str_n()], [.I64])`.
  - `tools/js_runtime/runtime.mjs`: the `run_wasm` host import (sync ~`:500`, async `suspendHost` ~`:1687`) accepts a 3rd wasm arg; `decodeString` it (empty ⇒ `""`); capture in `makeChildTrapHandler(runtime, sourceRoot)` (~`:516`/`:1704`); `renderChildTrace` passes it as the 4th arg of `render_runtime_trace` (~`:1316`). Nested `run_wasm` launches pass `""`.
  - Leave `proc.tw`/`run.tw` at 2 args for now.

- [ ] **Step B: Rust stage0 arity — verify then fix if needed.** Check whether Rust stage0 (`src/`) rejects a 3-arg `run_wasm` extern (grep its builtin/extern table for `run_wasm`). If it enforces arity, bump it to accept the 3rd `String` arg. If it treats `twinkle_runtime` externs generically, no change.
  Run: `cargo build --release` (targeted, not test).

- [ ] **Step C: First rebuild.** Sequentially:
```bash
make stage2
make bundle-cli
cp target/boot.wasm tools/js_runtime/boot.wasm
```
Gate: `target/twk run boot/tests/main.tw` green; `deno test -A tools/js_runtime/` green. (twk-run traces are location-only here — `source_root` isn't passed yet; expected, not a regression to fix.)

- [ ] **Step D: Widen `proc.tw` + regen `core_lib`.** `boot/stdlib/proc.tw`: extern `fn run_wasm(bytes, argv, source_root: String) Int` and the `pub fn run_wasm(bytes, argv, source_root) { twinkle_runtime.run_wasm(bytes, argv, source_root) }` wrapper. Regenerate `boot/lib/module/core_lib.tw` (do not hand-edit the embedded literal — run the generator the repo uses).

- [ ] **Step E: `run_file` passes the root.** `boot/commands/run.tw`: `proc.run_wasm(wasm_bytes, argv, artifacts.project_root)`. Other `proc.run_wasm` callers (`test.tw:100`, nested) pass `""`.

- [ ] **Step F: CLI e2e (failing → passing).** In `cli.test.mjs`: `twk run src/main.tw` from a fixture project root shows a **relative-path** snippet; the same artifact run later without a root shows location-only; `twk run ./foo.tw` (no manifest) resolves via absolutization + entry-dir fallback. (Reuse the M1 e2e harness + `RENDERER_WASM` override pattern.)

- [ ] **Step G: Second rebuild + verify e2e.** Sequentially: `make stage2 && make bundle-cli && cp target/boot.wasm tools/js_runtime/boot.wasm`. Run `deno test -A tools/js_runtime/cli.test.mjs` → PASS. Manually confirm `target/twk run` on a project fixture prints a source-mapped snippet with a relative path.

- [ ] **Step H: fmt + lint + commit**
```bash
target/twk fmt boot/stdlib/proc.tw boot/commands/run.tw
target/twk lint boot/main.tw
git add -A
git commit -m "feat(debug): thread source_root through run_wasm to the renderer"
```
(If Step A/B were committed separately before the first rebuild — recommended so the bootstrap step is bisectable — this is the second commit.)

---

## Task 6: Verify — self-host, size, portability, fixtures

**Files:** none (verification) + bookkeeping.

- [ ] **Step 1: Self-host fixed point** — `make stage2` then `make bundle-cli` (sequential). Confirm convergence.

- [ ] **Step 2: Portability check** — build a fixture project artifact, then confirm its `twinkle.debug` contains **no absolute paths** (all entries relative or `@`-prefixed). Decode via the section decoder or a `.wat`/byte scan.

- [ ] **Step 3: Size** — `ls -l target/boot.wasm`; confirm it is unchanged-or-smaller vs M1 (relative paths are shorter). Record bytes.

- [ ] **Step 4: Suites** — `target/twk run boot/tests/main.tw`; `cp target/boot.wasm tools/js_runtime/boot.wasm`; `deno test -A tools/js_runtime/`. All green (modulo the known pre-existing `web.test.mjs` env quirk).

- [ ] **Step 5: Trap fixtures** — through shipped `target/twk`: a project-fixture div0/OOB prints a **relative-path** source-mapped snippet, exit 1; the same pre-built artifact run later prints location-only; a normal program exits 0. (`error()` staying location-only is the deferred Phase-4 non-goal, unchanged.)

- [ ] **Step 6: Bookkeeping** — mark M2 complete in `docs/plans/disk-backed-debug-info-m2.md`, adjust the `docs/plans/README.md` row (M2 done), refresh the `project_runtime_stack_traces.md` memory pointer. Commit `docs(debug): mark disk-backed debug info Milestone 2 complete`.

---

## Self-Review

**Spec coverage** (against `docs/plans/disk-backed-debug-info-m2.md`):
- `find_project_root` fix → Task 1. ✓
- One classifier, two formatters → Task 2 (classifier + analyze formatter) / Task 3 (producer formatter). ✓
- Absolute-root caller-independent relativization → Task 3 (absolutize in `module_compiler`, run+build). ✓
- v3 bump → Task 3. ✓
- 4-arg renderer + safe-join + `@`-skip + no-root⇒location-only → Task 4. ✓
- `source_root` threaded via `run_wasm` import + closure (not `prepareWasm`) → Task 5. ✓
- Registered-builtin bootstrap (two rebuilds) + Rust stage0 arity → Task 5 Steps A–C, G. ✓
- Degradation table (present/missing/empty-root/@) → Task 4 Step 3 + Task 5 Step F + Task 6 Step 5. ✓
- Portability (no absolute paths in artifacts) → Task 6 Step 2. ✓
- v2→v3 cross-version degradation → covered by the version bump; no code beyond `version()`. ✓
- Non-goals (hashes, --strip-debug, prelude-frame suppression) → correctly absent. ✓

**Placeholder scan:** No TBD/vague steps. Two deliberate latitudes, each with a named fallback: Task 3 Step 4's locally-bound `fn abspath` (inline as a top-level helper if not accepted), and Task 5 Step B (Rust arity is verify-then-fix — may be a no-op if externs are generic). Test-fixture authoring reuses the M1 stable-`/tmp` pattern explicitly.

**Type consistency:** `PathClass`/`classify`/`strip_root` defined in Task 2, consumed identically in Tasks 2-analyze and 3. `PipelineArtifacts.project_root` defined Task 3, read Task 5. `safe_join(source_root, rel) String?` defined Task 4, used in `build_disk_registry` (Task 4) — signature stable. `render_runtime_trace`'s 4th arg name `source_root` matches across renderer (Task 4) and host call (Task 5). `run_wasm(bytes, argv, source_root)` arity matches across `builtins.tw` abi, `proc.tw`, `run.tw`, and the JS import (Task 5).
