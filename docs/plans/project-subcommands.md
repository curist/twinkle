# Project-Aware Subcommands Plan

## Goal

Make `twinkle.toml` entries the shared project model for CLI tooling while
preserving the current one-off file workflows. Commands should behave
predictably in two modes:

* **Explicit mode:** when the user supplies file paths or a target, operate only
  on that request.
* **Project mode:** when no file/target is supplied, load the nearest
  `twinkle.toml` and operate on the configured project/test entries.

This plan builds on the shipped constrained config reader in
`boot/lib/project/config.tw`, the `twk test` command, and the LSP's configured
entry seeding.

---

## Current Baseline

Available today:

* `twinkle.toml` can contain legacy `name = "..."`, `[project].name`,
  `[project].entries`, and `[test].entries`.
* Entries are validated as project-local `.tw` paths.
* `twk test` runs `[test].entries` as normal Twinkle programs, falling back to
  `tests/main.tw` when present.
* LSP diagnostics can seed workspace analysis from configured entries.
* Most CLI commands still require an explicit file path and ignore configured
  project entries.

The main migration is therefore command selection and traversal policy, not the
manifest parser itself.

---

## Core Semantics

### Entry groups

Use entry groups consistently:

* **Project entries** (`[project].entries`) are buildable/runnable program roots.
* **Test entries** (`[test].entries`) are executable test programs.
* **All configured entries** means project entries followed by test entries.

Tool defaults:

| Command | No-argument project mode should use |
|---|---|
| `fmt` | project + test entries |
| `lint` | project + test entries |
| `check` | project entries |
| `build` | project entries |
| `run` | project entries |
| `test` | test entries |
| `parse` | no project mode for now |
| `ir` | no project mode for now, except maybe one target later |

Rationale: source-maintenance commands should include tests by default;
build/run commands should not build or run test entrypoints unless the user asks
for `twk test`.

### Explicit mode wins

If the user provides explicit file paths, command behavior should remain a
one-off operation over those files/entries. Project config may still provide the
root for import resolution, but it should not add extra entries.

Examples:

```bash
twk fmt foo.tw          # format exactly foo.tw
twk lint boot/main.tw   # lint exactly boot/main.tw as today
twk build demo.tw       # one-off build of demo.tw
twk run scratch.tw      # one-off run of scratch.tw
```

### Project-root discovery

All project-mode commands should discover the nearest project root by walking up
from the current working directory. Use an absolute current directory from the
host (`proc.cwd()`), not the literal `.`; otherwise nested directories can fail
to find their parent `twinkle.toml`.

If no `twinkle.toml` is found, project-mode commands should either use their
small conventional fallback or emit a clear error. They should not silently pick
an arbitrary file.

### Target names

For build/run project entries, derive a target name from the entry path:

* `cmd/server.tw` -> `server`
* `cmd/server/main.tw` -> `server`
* `main.tw` -> project name when present, otherwise `main`

If two project entries derive the same target name, report a configuration error
and ask the user to choose distinct entry paths for now. Do not introduce a
richer target table until conflicts become common enough to justify it.

Future extension, intentionally deferred:

```toml
[[target]]
name = "server"
entry = "cmd/server/main.tw"
```

---

## Desired Command Behavior

### `twk fmt`

Current: requires one or more file paths.

Target behavior:

```bash
twk fmt                 # format modules reachable from project + test entries
twk fmt --check         # check formatting for that reachable set
twk fmt a.tw b.tw       # format exactly these files
twk fmt --check a.tw    # check exactly this file
```

Notes:

* Project mode should traverse imports from configured entries and format the
  reachable project-local modules. Orphan files (reachable from no entry) are
  handled via explicit file/glob args, not project mode; a dedicated tree/glob
  mode is deferred until a concrete need appears.
* Keep deterministic output order.
* Dedupe modules reached through multiple entries.
* Keep explicit file mode parse-only enough to work on standalone files.
* Prefer continuing across independent entries after an error and reporting all
  reachable failures.

### `twk lint`

Current: requires one entry/file path.

Target behavior:

```bash
twk lint                # lint modules reachable from project + test entries
twk lint --fix          # apply safe fixes across that reachable set
twk lint boot/main.tw   # lint exactly this entry/file
twk lint --explain      # project-mode explanations plus findings
```

Notes:

* Dedupe findings for modules reached through multiple entries.
* For `--fix`, collect non-overlapping edits per file and write each file once.
* If any module has parse/resolve/check errors, still report syntactic lints for
  modules that can be parsed.
* Keep fix precedence behavior (`--fix-unused-imports`, `--fix-inherent-calls`)
  unchanged in explicit mode.

### `twk check`

Current: requires one entry file.

Target behavior:

```bash
twk check               # check all [project].entries
twk check app.tw        # one-off check
twk check --all         # project + test entries
```

Notes:

* If no project entries are configured, report a clear message. A later
  convention such as `cmd/*.tw` can be added after project scaffolding exists.
* Dedupe diagnostics by canonical URI when entries share dependencies.
* Keep command-line file mode exactly as today.

### `twk build`

Current: requires one entry file and optional `-o`.

Target behavior:

```bash
twk build                       # single entry: build it; multiple: error, ask for --all/--target
twk build --all                 # build every [project].entry
twk build --target server       # build one configured entry
twk build app.tw -o app.wasm    # one-off build
twk build --target server -o target/server.wasm
```

Notes:

* With one project entry, no-arg `twk build` builds it. With multiple, no-arg
  `twk build` errors and lists target names, asking for `--all` or `--target`.
  This keeps build symmetric with `run` (which selects exactly one) while still
  offering an explicit "build the world" via `--all`.
* In project mode, default output is `<project-root>/target/<name>.wasm`, where
  `<name>` is the derived target name and `<project-root>` is the directory
  holding `twinkle.toml` (not the cwd) — so building from a subdirectory still
  lands artifacts in the same place. Explicit mode keeps its current behavior
  (honor `-o`, or the existing one-off default).
* If multiple entries exist and `-o` is supplied without `--target`, report an
  error rather than writing multiple artifacts to one path.
* Keep `.wat` output behavior for explicit `-o something.wat`; for project mode,
  consider a later `--emit wat` or require `--target -o` for WAT debugging.
* Validate target-name conflicts before building anything.

### `twk run`

Current: requires one entry file.

Target behavior:

```bash
twk run                 # run the only configured project entry
twk run --target server # run one configured entry
twk run app.tw          # one-off run
```

Notes:

* If no project entries are configured, report a clear error.
* If multiple project entries exist and no target is supplied, list available
  target names and ask for `--target`.
* Preserve `-i/--interpreter` for explicit and project modes.
* Reserve `twk run --target server -- <args>` for future program-argument
  forwarding.

### `twk test`

Current: runs `[test].entries`, falling back to `tests/main.tw`.

Target behavior refinements:

```bash
twk test                      # run configured test entries
twk test tests/smoke.tw       # optional explicit one-off test entry
twk test --filter parser      # optional CLI spelling for TWK_TEST_FILTER
```

Notes:

* Fix project-root discovery so running from subdirectories finds the parent
  project config.
* Keep environment-variable compatibility (`TWK_TEST_FILTER`,
  `TWK_TEST_REPORT=verbose`, `NO_COLOR`).
* First release ships env-var control only; CLI flags (`--filter`, etc.) are
  deferred to Phase 4 once the base project-mode behavior is stable, so flag/env
  precedence and naming aren't locked in prematurely.

### `twk parse` and `twk ir`

Keep explicit-file-first for now:

```bash
twk parse file.tw
twk ir file.tw --opt
```

Rationale:

* `parse` is mainly a debugging command for one file.
* `ir` output for multiple entries would need a clear display/file-output model.

Possible later extension: `twk ir --target server --opt`.

---

## Shared Implementation Work

### Project command context

Add a shared helper under `boot/commands/` or `boot/lib/project/` that computes:

```tw
type ProjectContext = .{
  root: String,
  config: Config,
  project_entries: Vector<EntryTarget>,
  test_entries: Vector<EntryTarget>,
}

type EntryTarget = .{
  name: String,
  rel_path: String,
  abs_path: String,
}
```

Responsibilities:

* discover root from `proc.cwd()`;
* load config;
* resolve relative paths;
* derive target names;
* reject target-name conflicts;
* provide entry sets for each command;
* provide the reachable project-local module set for a given entry group, by
  running `analyze.discover_closure` over those entries (see below) and filtering
  to project-local paths. This is the single discovery entry point the rest of
  the tooling consumes; commands do not walk imports themselves.

### Reachable module discovery

`fmt` and `lint` need a reusable way to turn entries into project-local source
files:

* parse each entry;
* follow project-local imports;
* include relative imports;
* include stdlib/prelude for analysis but do not format/lint bundled sources;
* dedupe by canonical path;
* continue across independent entries after an error where possible.

Reuse the existing shared frontend — do not write a second scanner. The module
graph is already unified:

* `boot/compiler/imports.tw::plan_dependencies` resolves one module's `use`
  declarations into canonical dependency paths.
* `boot/compiler/query/analyze.tw` drives the transitive closure over that, and
  is the single path used by *both* the batch compiler
  (`module_compiler.tw::compile_entry`, behind `build`/`run`/`check`) and the
  LSP diagnostics. They diverge only downstream of analysis (LSP → editor
  queries over an overlay of unsaved buffers; batch → lowering/codegen).

For `fmt`/`lint`, reuse `analyze.discover_closure`, the env-independent Phase 1
discovery that builds the dependency adjacency graph by load/parse/plan only (no
resolve, no `ResolvedEnv`). It returns `Discovery { order, edges, failed,
diagnostics }`; `order` is the reachable canonical module set, including
stdlib/prelude (they are traversed so the graph closes). The remaining work is
just: run it per entry, then filter `order` to project-local paths for the
rewrite set (stdlib/prelude stay as traversed dependencies but are never
formatted/linted), dedupe across entries, and surface `failed`/`diagnostics`
without aborting the other entries. `fmt` needs nothing past discovery; `lint`
adds its own analysis only for the type-dependent rules.

### Diagnostics and output

For multi-entry commands:

* report which entry/target is being processed when useful;
* group diagnostics by file;
* dedupe shared dependency diagnostics;
* return non-zero if any entry fails;
* keep output deterministic for CI.

---

## Migration Phases

### Phase 1 — Shared context and root discovery

* Add `ProjectContext` helper.
* ~~Fix `twk test` root discovery from nested directories.~~ Already done:
  `test.tw` discovers via `find_project_root(proc.cwd())` and walks up
  correctly (verified from a subdirectory). No change needed.
* Build the reachable-module helper on `ProjectContext` over
  `analyze.discover_closure`: run discovery per entry, take its `order` as the
  reachable canonical set, filter to project-local paths (drop stdlib/prelude
  via `imports.make_canonical_roots` / `canonical_module_path`), dedupe across
  entries, and carry `failed`/`diagnostics` through without aborting siblings.
  This is env-independent (load/parse/plan only) and lands here so Phase 2's
  `fmt`/`lint` consume it instead of writing a second scanner.
* Add tests for root discovery, configured entries, derived target names,
  conflict errors, and the reachable-module set (project-local only, deduped
  across overlapping entries, resilient to a failing entry).

### Phase 2 — Source-maintenance commands

* Relax `fmt` CLI so file arguments are optional.
* Implement `twk fmt` project mode over the Phase 1 reachable-module helper
  (`ProjectContext`), formatting each project-local module from `order`.
* Relax `lint` CLI so file arguments are optional.
* Implement `twk lint` project mode over the same helper, with deduped findings
  and safe multi-file fixes; run analysis only for the type-dependent rules.

### Phase 3 — Check/build/run project mode

* Relax `check`, `build`, and `run` required file arguments.
* Add project-entry selection and target-name validation.
* Add `--target` for build/run.
* Add default project build outputs.

### Phase 4 — Polish and docs

* Document command behavior in user-facing docs/help output.
* Add examples for single-entry and multi-entry projects.
* Add CLI flags for test filtering/reporting (`--filter`, etc.), settling
  flag/env precedence against the existing `TWK_TEST_*` variables.

### Phase 5 — Makefile migration (once everything is done)

Once project mode is stable, collapse the repo's own `Makefile` invocations onto
it so the manifest is the single source of truth:

* Replace the `fmt` target's `find boot -name '*.tw' ... | xargs target/twk fmt`
  with project-mode `target/twk fmt`, relying on `[project].entries` /
  `[test].entries` for coverage. The generated `boot/lib/module/core_lib.tw`
  must stay excluded — project mode skips bundled/generated sources, but verify
  it is not reachable-and-rewritten before deleting the explicit `find` filter.
* `boot-test` already uses project-mode `target/twk test`; once root discovery
  from subdirectories is fixed (Phase 1), confirm it still resolves the boot
  project root.
* Audit other targets (`core_lib.tw` formatting step, any future `lint`/`check`
  hooks) for the same `find | xargs` pattern and migrate them too.
* Keep the migration behavior-preserving: the set of files formatted/linted
  before and after must match, so diff the file lists during cutover.

Artifact-output migration (driven by the `target/<name>.wasm` convention):

* **Bootstrap builds stay explicit.** The self-host loop builds to exact paths
  (`target/boot-stage1.wasm`, `target/boot.wasm`, the stage3/stage4 temps) and
  downstream rules key off them. These keep their explicit `-o` flags — do not
  route them through project mode. Note the derived name for `main.tw` is the
  *project* name (root project is `twinkle` → `target/twinkle.wasm`), so it does
  not collide with the `target/boot.wasm` payload today; the collision is only a
  theoretical caution if a project is ever named `boot`.
* **`.gitignore`:** the current `/target` rule is repo-root-anchored, so it does
  NOT cover sub-project artifact dirs — `boot/target/`, `examples/*/target/`,
  etc. Project roots are wherever a `twinkle.toml` sits (root, `boot/`, each
  `examples/*`), and a project-mode build there emits to `<that-root>/target/`.
  Migrate `/target` to an unanchored `target/` pattern (matches a `target/`
  directory at any depth) so every project root's artifacts are ignored, or add
  per-project rules. For user projects, scaffold the same `target/` ignore via
  any future `twk init`.
* **GitHub Actions:** `test.yml` runs `make test` and caches `target`; it makes
  no per-artifact path assertions, so it needs no change unless a future CI step
  references a specific built `.wasm` path. `deploy-playground.yml` builds from
  published npm packages and is unaffected. Re-audit if CI starts invoking
  project-mode `build`/`run` directly.

---

## Compatibility Rules

* Existing explicit file invocations must keep working.
* Existing legacy `twinkle.toml` files with only `name = "..."` remain valid.
* Empty `twinkle.toml` remains valid.
* No command should recursively scan the whole tree unless explicitly designed
  later; MVP project behavior is entry-based.
* Stdlib/prelude modules are analyzed as dependencies but not rewritten by
  project formatting/linting.

---

## Resolved Decisions

These were the open questions; all are now settled and reflected in the command
sections above.

* **`check` scope:** no-arg `twk check` covers `[project].entries` only; test
  entries are opt-in via `--all`.
* **`build` default:** with one project entry, no-arg `twk build` builds it; with
  multiple it errors and asks for `--all` or `--target`. Symmetric with `run`.
* **Project build output:** `<project-root>/target/<name>.wasm`, derived from the
  target name and the `twinkle.toml` location (not cwd). Explicit mode unchanged.
* **`fmt`/`lint` scope:** reachable-from-entries only; orphan files use explicit
  file/glob args. A dedicated tree/glob mode is deferred.
* **Test filter/report:** first release ships env-var control only
  (`TWK_TEST_FILTER`, `TWK_TEST_REPORT`, `NO_COLOR`); CLI flags land in Phase 4.

---

## Exit Criteria

* `twk fmt`, `twk lint`, `twk check`, `twk build`, `twk run`, and `twk test` have
  documented explicit-mode and project-mode behavior.
* Running commands from a nested project subdirectory finds the parent
  `twinkle.toml`.
* Shared modules reached from multiple entries are processed once per command.
* Target-name conflicts are reported before build/run work begins.
* Current explicit-file workflows and boot validation commands continue to work.
* Boot tests cover project config parsing, target derivation, command selection,
  and representative project-mode command behavior.
