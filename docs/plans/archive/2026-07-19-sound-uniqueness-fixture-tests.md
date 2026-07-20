# Sound Uniqueness Fixture Tests Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add on-disk, multi-module CFG ownership tests that ground the completed sound-uniqueness analysis claim in realistic source fixtures.

**Architecture:** Create a dedicated boot test suite that compiles fixture entry files from `boot/tests/fixtures/cfg/sound_uniqueness/`, runs the optimized ANF through CFG pruning, summary computation, and ownership analysis, then asserts rendered facts/verdicts. Keep this test-only; no production compiler behavior changes are planned.

**Tech Stack:** Twinkle boot test runner, `compiler.pipeline`, `compiler.cfg`, `compiler.summary`, `compiler.ownership`, relative fixture imports.

## Global Constraints

- Boot compiler remains the primary implementation path.
- Generated code should remain unchanged; these tests inspect analysis/debug output only.
- Missing proof must remain conservative: `Unknown` or `Shared`, not speculative mutation.
- Prefer on-disk fixtures over synthetic ANF for these grounding tests.

---

### Task 1: Add failing fixture-backed CFG suite

**Files:**
- Create: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`
- Modify: `boot/tests/main.tw`
- Later fixture paths consumed by tests: `boot/tests/fixtures/cfg/sound_uniqueness/*.tw`

**Interfaces:**
- Consumes: `pipeline.compile_entry_path(path)`, `cfg.build_view(module, builtins)`, `ownership.prune_dead_merge(view)`, `summary.compute(view, builtins, semantics)`, `ownership.analyze_with_summaries(view, builtins, semantics, summaries)`, `summary.render_cfg(view, summaries)`.
- Produces: A suite function `pub fn suite() runner.Suite` registered from `boot/tests/main.tw`.

- [ ] **Step 1: Write the failing test suite**

Create `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw` with helpers to locate `boot/tests/fixtures/cfg/sound_uniqueness`, compile entries, render analyzed CFG, and assert substrings for:

```tw
use @std.testing.assert as assert
use @std.testing as runner

use compiler.builtins
use compiler.cfg
use compiler.opt.semantics as semantics
use compiler.opt.semantics.{make_prelude_optimizer_semantics}
use compiler.ownership
use compiler.pipeline
use compiler.summary
use lib.module.loader

fn fixtures_dir() String {
  root := loader.find_project_root("boot")
  "${root}/tests/fixtures/cfg/sound_uniqueness"
}

fn render_entry(name: String) Result<String, String> {
  artifacts := try pipeline.compile_entry_path("${fixtures_dir()}/${name}.tw")
  b := artifacts.builtins
  view := cfg.build_view(artifacts.opt, b)
  view = ownership.prune_dead_merge(view)
  sem := semantics.make_prelude_optimizer_semantics(b)
  table := summary.compute(view, b, sem)
  analyzed := ownership.analyze_with_summaries(view, b, sem, table)
  .Ok(summary.render_cfg(analyzed, table))
}

fn assert_has_all(out: String, parts: Vector<String>) Result<Void, String> {
  for p in parts {
    try assert.str_contains(out, p)
  }
  .Ok({})
}

pub fn suite() runner.Suite {
  runner
    .suite("cfg sound uniqueness fixture coverage")
    .test("multi-module env callers render owned and generic decisions", fn() {
      out := try render_entry("env_main")
      try assert_has_all(out, ["function build_owned", "function branch_shared", "selected owned variant", "record_update"])
      .Ok({})
    })
    .test("multi-module Result state transport keeps handled payload paths visible", fn() {
      out := try render_entry("result_main")
      try assert_has_all(out, ["function run_ok", "ret_paths", "V", "selected owned variant"])
      .Ok({})
    })
    .test("Cell-backed dict update stays conservative", fn() {
      out := try render_entry("cell_main")
      try assert_has_all(out, ["function mark", "summary:", "Published"])
      try assert.is_false(out.contains("function mark") and out.contains("selected owned variant"))
      .Ok({})
    })
    .test("visit-like threaded state exercises loops branches and field decisions", fn() {
      out := try render_entry("visit_main")
      try assert_has_all(out, ["function run_visit", "function visit", "loop", "record_update"])
      .Ok({})
    })
}
```

- [ ] **Step 2: Wire the suite into `boot/tests/main.tw`**

Add `use .suites.cfg_sound_uniqueness_fixtures_suite` near the existing CFG suite imports, and add `cfg_sound_uniqueness_fixtures_suite.suite()` near the CFG suite entries.

- [ ] **Step 3: Run test to verify red**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL because the fixture entry files do not exist yet.

---

### Task 2: Add realistic on-disk fixture modules

**Files:**
- Create directory: `boot/tests/fixtures/cfg/sound_uniqueness/`
- Create fixture files:
  - `env_lib.tw`
  - `env_main.tw`
  - `result_lib.tw`
  - `result_main.tw`
  - `cell_lib.tw`
  - `cell_main.tw`
  - `visit_lib.tw`
  - `visit_main.tw`

**Interfaces:**
- Produces compilable multi-module source fixtures using relative imports.
- Fixtures should be non-trivial enough to lower through real parser/resolver/checker/ANF/optimizer.

- [ ] **Step 1: Add env specialization fixture**

`env_lib.tw` exports `Env` and `add_type`; `env_main.tw` imports both and defines one owned-threading caller and one alias-preserving caller.

- [ ] **Step 2: Add Result transport fixture**

`result_lib.tw` exports state/result record types and a helper returning `Result<Load, LoadErr>` with state in payloads; `result_main.tw` handles both arms locally and continues threading state.

- [ ] **Step 3: Add Cell conservative fixture**

`cell_lib.tw` exports a `mark` shape using `Cell<Dict<String, Bool>>`, `get`, dict update, and `set`; `cell_main.tw` calls it.

- [ ] **Step 4: Add visit-like state fixture**

`visit_lib.tw` exports a threaded `State` with dict/vector fields, a loop, branch joins, and record field updates; `visit_main.tw` calls it repeatedly.

- [ ] **Step 5: Run fixture tests and inspect actual rendered CFG**

Run focused command: `target/twk run boot/tests/main.tw`.
Expected: tests may fail on exact substring assertions while fixtures compile. Use `target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/<entry>.tw --cfg` to inspect actual render text.

---

### Task 3: Tighten assertions to actual analysis evidence

**Files:**
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes actual render strings from Task 2.
- Produces stable assertions that prove the intended analysis behavior without depending on unstable FuncIds.

- [ ] **Step 1: Replace broad substrings with stable evidence**

Use stable function names, summary role strings, `ret_paths`, and verdict phrases such as `selected owned variant`, `generic`, `record_update`, or persistent/in-place field verdict fragments observed in the render.

- [ ] **Step 2: Run focused tests**

Run: `target/twk run boot/tests/main.tw`.
Expected: PASS.

- [ ] **Step 3: Format and lint Twinkle files**

Run: `target/twk fmt boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw boot/tests/fixtures/cfg/sound_uniqueness/*.tw boot/tests/main.tw`
Run: `target/twk lint boot/main.tw`
Expected: formatter is idempotent; linter reports no blocking compile errors. If report-only style findings appear, evaluate them and fix relevant ones.

---

### Task 4: Final verification

**Files:**
- No new files beyond Tasks 1-3.

**Interfaces:**
- Consumes the complete test-only diff.
- Produces final evidence for user report.

- [ ] **Step 1: Run full boot test suite**

Run: `target/twk run boot/tests/main.tw`.
Expected: PASS.

- [ ] **Step 2: Report grounded coverage**

Summarize which worked-example gaps are now covered by on-disk fixtures and which gaps remain, if any.
