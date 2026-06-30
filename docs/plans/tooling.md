# Tooling Active TODO Plan

## Goal

Track active, user-facing Twinkle tooling work now that the formatter, boot LSP
baseline, self-hosted compiler, and standalone `twk` workflow are in place.

Historical tooling roadmap context is archived in
[archive/tooling.md](archive/tooling.md).

---

## Current Baseline

Available today:

* `twk fmt <file>` and `twk fmt --check <file>` for canonical source formatting.
* `twk lsp` with diagnostics, hover, go to definition, completion, unused-import
  code actions, and whole-document formatting.
* `twk check`, `twk parse`, `twk ir`, `twk build`, and `twk run` in the boot CLI.
* Twinkle-native boot test suite under `boot/tests/`.

---

## Active Tooling Work Index

| Area | Priority | Status | Details |
|------|----------|--------|---------|
| Project configuration | High | Planned | This document |
| LSP enhancements | High | Planned | [lsp-enhancements.md](lsp-enhancements.md) |
| Additional LSP code actions | Medium | Planned | [lsp-code-actions.md](lsp-code-actions.md) |
| Whole-project formatter UX | Medium | Planned | This document |
| Linter | Medium | Planned | This document |
| Test runner UX | Medium | Planned | This document |
| Documentation generator | Low | Planned | This document |
| Package/project tooling | Low | Planned | This document |

---

## T0 — Project Configuration via `twinkle.toml`

`twinkle.toml` currently acts mostly as a project-root marker. Tooling should
turn it into the shared source of truth for project-level behavior while keeping
an empty file valid.

Configuration should stay deliberately small. Most tooling behavior should be
convention-driven, and every section/key should be optional. An empty
`twinkle.toml` remains valid and means "use defaults".

Possible minimal shape:

```toml
[project]
name = "my_project"
entries = ["cmd/compiler.tw", "cmd/playground.tw"]

[test]
entries = ["tests/main.tw"]
```

`project.entries` are the package's buildable entry points. They also serve as
the default roots for project-level `fmt`, `check`, docs reachability, LSP
workspace analysis, and other whole-project tooling. Tool-specific entries
should be added only when the tool really needs a separate root set; tests are
the first likely case.

`test.entries` are executable test entry points, not per-test discovery globs.
Each listed file is a normal Twinkle program that is expected to run a test
harness and exit non-zero on failure. This matches the existing boot test model
(`boot/tests/main.tw` imports suite modules and calls `runner.run_all([...])`) and
keeps the MVP free of reflection, macros, or test annotations.

LSP features that need a whole-workspace view should treat the configured
project as the union of `project.entries` and `test.entries`, plus open-document
overlays. This is especially important for rename: references in tests should be
updated when tests are configured. When a project has no configured entries, the
LSP keeps the current fallback behavior of analyzing from open documents.

Planned work:

* Add a small TOML reader or constrained config parser in the boot stdlib/CLI.
* Define defaults so existing empty `twinkle.toml` files keep working.
* Resolve all configured paths relative to the project root.
* Validate configured entries: paths must be project-local `.tw` files,
  canonicalized paths must be unique, and derived artifact names must not
  conflict.
* Preserve current root-marker files such as `name = "twinkle-boot"` as valid
  legacy shorthand; prefer `[project].name` for new projects.
* Share config loading across commands and LSP workspace analysis.
* Keep command-line flags higher precedence than config values.
* Avoid per-tool knobs unless they solve a real workflow problem.
* Report unknown or malformed config keys with source spans once the parser can
  provide them.

Near-term consumers:

* `twk fmt`: with no file arguments, format modules reachable from project and
  test entries; with file arguments, format those files as one-offs.
* `twk check`: with no file arguments, check configured buildable entries; with
  file arguments, check those entries as one-offs.
* `twk build`: with no file arguments, build all configured entries; with a file
  argument, build that entry as a one-off; with `--target <name>`, build one
  configured entry by derived target name.
* `twk run`: with no file arguments, run the only configured entry; with a file
  path, run that entry as a one-off; with `--target <name>`, run a configured
  entry by derived target name.
* `twk test`: run configured test entries as normal Twinkle programs using the
  standard test harness; later may accept explicit test entry paths or filters.
* future package tooling: project name, package metadata, and dependency data.

Non-goals for now:

* LSP feature toggles in `twinkle.toml`; editors already own most LSP behavior.
* Style configuration for the formatter; Twinkle should keep one official style.
* Broad include/exclude configuration unless entry-based discovery proves
  insufficient.
* Configurable module roots or source-path remapping; imports should continue
  to mirror paths under the project root.

### Formatter discovery options

There are two plausible models for project formatting:

**A. File discovery:** traverse the project tree and format every `.tw` file.

Pros:

* Formats orphaned modules, examples, experiments, and future entry files.
* Simple mental model: every source file under the project is formatted.
* Does not require imports to parse/resolve successfully.

Cons:

* Needs ignore/exclude rules for generated/vendor/build directories.
* Can format scratch files that are not part of the project.
* Directory traversal behavior becomes another policy surface.

**B. Entry-based module traversal:** configure `project.entries`, parse those
entry files, follow imports, and format the reachable module graph.

Pros:

* Matches Twinkle's module system and avoids broad filesystem policy.
* Naturally ignores unreachable scratch files.
* Gives package/test/app roots explicit meaning that can be reused by `check`,
  `build`, docs, and package tooling.
* Works naturally with a scaffolded `cmd/`, root-level library modules, and
  `tests/` convention.

Cons:

* Misses orphaned source files unless they are listed as entries or imported.
* Requires import scanning to work; syntax errors in an entry or import may
  limit discovery.
* Test/example modules may need explicit entries even when they are not imported
  by the main program.

Current leaning: start with entry-based traversal because it composes with the
compiler's module model. Keep explicit file arguments as one-off mode for `fmt`,
`check`, `build`, and `run`, and consider a later explicit tree-discovery command
only if reachable-only formatting proves inconvenient. Avoid a separate `--all`
flag; in a project context, no file arguments should mean "operate on the
configured project".

### Entry naming, build artifacts, `build`, and `run`

For MVP, configured project entries should also be the build/run targets. Avoid
artifact-name configuration initially; derive names from the entry path instead.
Establish an MVP project convention so `twk new` can scaffold predictable
projects:

```text
my_project/
  twinkle.toml
  .gitignore
  cmd/
    my_project.tw
  foo/
    bar.tw
  tests/
    main.tw
```

Use `cmd/<name>.tw` for executable entry points. This keeps commands grouped
without forcing one directory per command. Library/support modules live directly
under the project root using their real module paths. Avoid a required `src/`
wrapper because Twinkle imports are path-based: a file at `src/foo/bar.tw` would
be imported as `use src.foo.bar`, which makes `src` part of the public module
name. `twk new` should also write a `.gitignore` containing at least `target/`.
The current Twinkle repository's `boot/` directory is a bootstrap/compiler
implementation detail, not a layout recommendation for new projects.

Suggested convention:

* Derive the default artifact name from the entry file stem, so
  `cmd/compiler.tw` builds `compiler`.
* If the entry file stem is too generic, such as `main`, derive from the parent
  directory instead, so `cmd/compiler/main.tw` also builds `compiler`.
* If two entries derive the same artifact name, report a config/build error and
  ask the user to choose distinct entry paths for now.

`twk build` and `twk run` should follow the same target-selection model, using
an explicit `--target` flag for named project entries so target selection is
never confused with file paths or future program arguments:

* If the project has one configured entry, `twk run` runs it.
* Plain `twk build` builds all configured entries.
* `twk run <path.tw>` and `twk build <path.tw>` remain available for one-off
  source files.
* If the project has multiple entries, `twk run` without a target reports the
  available targets and asks the user to pass `--target <name>`.
* `twk run --target server` runs the configured entry whose derived target name
  is `server`.
* `twk build --target server` builds that configured entry.
* `twk run --target server -- <args...>` is reserved for passing program args
  once argument forwarding exists.

This intentionally defers explicit artifact naming. If conflicts or release
workflows become common, add a richer target table later, for example:

```toml
[[target]]
name = "server"
entry = "cmd/server/main.tw"
```

Open questions:

* What default entries should be when `project.entries` is absent? Current
  leaning: discover `cmd/*.tw`, then fall back to `main.tw` for tiny scripts.
* Should project-level `twk fmt` include `[test].entries` automatically?
  Current leaning: yes, because tests are project source too.
* Should non-buildable examples have their own conventional entry location, such
  as `examples/*.tw`, or wait until users need it?
* Should `cmd/*/main.tw` be supported as a secondary convention, or should MVP
  only scaffold and auto-discover `cmd/*.tw`?
* Should build output default to `target/<name>.wasm`, or should it keep using
  the current nearby/default output convention until package tooling exists?

---

## T1 — Whole-Project Formatter UX

`twk fmt` currently formats explicitly provided files. The next formatter UX
step is project-level behavior backed by `twinkle.toml` defaults.

Planned work:

* Make `twk fmt` with no file arguments format modules reachable from
  configured/default project entries and test entries.
* Keep deterministic module traversal and output ordering.
* Report per-file read/parse/write failures while continuing other files.
* Return a non-zero exit when any file needs changes in `--check` mode or any
  formatting failure occurs.

Open questions:

* Whether `twk fmt` with explicit file arguments should ignore project entries
  entirely or treat the files as additional entry roots. Current leaning: ignore
  project entries and format exactly the requested files.
* Whether project-level `twk fmt` should include test entries automatically.

---

## T2 — Linter

Add `twk lint` for lightweight style and correctness diagnostics that do not
belong in the type checker.

Initial rule candidates:

* Rebinding-without-use patterns that look like mutation mistakes.
* Unreachable code after `return`, `break`, or `error` where syntactically clear.
* Redundant wildcard case arms after exhaustive explicit arms.
* Suspicious shadowing in the same small scope.
* Public declarations without doc comments once the stdlib/project style needs
  it.

Design notes:

* Syntactic lint rules should run after parse only.
* Semantic lint rules can reuse the query pipeline.
* Lint diagnostics should use the same source diagnostic and LSP diagnostic
  rendering paths as compiler diagnostics where practical.

---

## T3 — Test Runner UX

The boot test suite already has Twinkle-native suite infrastructure:
`runner.suite(name)`, `.test(name, fn() Result<Void, String>)`,
`runner.run_all([...])`, assertion helpers that return `Result<Void, String>`,
compact/verbose reporting, and substring filtering through `TWK_TEST_FILTER`.
The next step is making that model available to user projects instead of
inventing a separate test-discovery mechanism.

MVP decisions:

* `[test].entries` lists executable test entry programs. `twk test` runs those
  entries; it does not scan for annotated test functions or auto-discover suite
  files.
* Test entry programs manually aggregate suites, just like `boot/tests/main.tw`:
  suite modules export `pub fn suite() testing.Suite`, and the entry calls
  `testing.run_all([suite_a.suite(), suite_b.suite()])`.
* Promote the existing boot-only harness into the standard library, initially as
  `@std.testing` plus assertion helpers (for example `@std.testing.assert`). The
  API should be a conservative copy of `boot/tests/runner.tw` and
  `boot/tests/assert.tw` unless user-project needs justify changes.
* Keep environment-variable compatibility for the first CLI layer
  (`TWK_TEST_FILTER`, `TWK_TEST_REPORT`, `NO_COLOR`), then add explicit CLI flags
  only if the workflow needs them.

Planned work:

* Add `@std.testing` with `Suite`, `Test`, `RunOptions`, `run_all`, and related
  runner helpers.
* Add standard assertion helpers matching the current boot helpers:
  equality, boolean, option/result, string containment/prefix/suffix, vector
  length/contains, and explicit failure.
* Move the boot test suite toward the standard harness or keep a thin compatibility
  wrapper so project tests and compiler tests exercise the same API.
* Add a `twk test` command that loads `twinkle.toml`, resolves `[test].entries`,
  and runs each configured entry as a normal Twinkle program.
* If `[test].entries` is absent, either report a clear "no test entries
  configured" message or use a tiny convention such as `tests/main.tw`; choose
  before shipping the command.
* Support filtering by suite/test name using the existing runner semantics.
* Produce concise human output and CI-friendly non-zero exit behavior.

---

## T4 — Documentation Generator

Add a documentation tool for public module APIs.

Planned work:

* Extract public functions, types, variants, fields, contracts, and doc comments.
* Render Markdown or static HTML from compiler query artifacts.
* Link references between modules where possible.
* Include signatures using the same user-facing type rendering as hover.

This can share much of the symbol extraction needed by LSP document/workspace
symbols.

---

## T5 — Package and Project Tooling

Project metadata should grow out of the same `twinkle.toml` config path rather
than introducing a second manifest format.

Potential work:

* `twk new <name>` for creating a new project directory following the standard
  convention.
* Defer `twk init` until there is a clear need to retrofit an existing
  directory.
* Dependency declaration and lockfile design in `twinkle.toml` plus a separate
  lockfile.
* Standard library/package documentation integration.
* Build/test/fmt/check orchestration for packages.

This area should stay low priority until the language and stdlib stabilize.

---

## Cross-Cutting Requirements

* Prefer boot compiler implementations under `boot/` for new tooling behavior.
* Keep CLI behavior deterministic and CI-friendly.
* Reuse query artifacts rather than adding duplicate parser/typechecker paths.
* Preserve UTF-16 position correctness at LSP boundaries.
* Add boot tests for new CLI/LSP behavior.
