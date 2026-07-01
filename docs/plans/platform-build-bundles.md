# Platform Build Bundles

## Goal

Make a built Twinkle library **immediately runnable** on a target platform, and
stop scattering JS/web host files across the project root. `twk build` grows
`--node` / `--web` flags that emit movable, runnable **npm/Vite project bundles**
under `target/<name>/`, each with a printed "how to run" recipe. The scaffold
reverts to Twinkle-only.

Here "bundle" means a small npm project that contains the user's `.lib.wasm` and
host files but resolves the Twinkle **runtime** via npm — it is *movable and
runnable* (copy it anywhere, `npm install && run`), not *self-contained* in the
sense of vendoring the runtime. This is a deliberate trade (see Toolchains):
letting npm/Vite resolve `@twinkle-lang/twinkle` is what keeps `twk` from having
to emit `runtime.mjs`.

This builds directly on the embeddable lib build
([embeddable-lib-build.md](embeddable-lib-build.md)): that plan added
`twk build --lib` (emit `target/<name>.lib.wasm`) and a compiler-free `loadLib`.
The gap it left is that the raw `.lib.wasm` is not runnable on its own — the user
still has to assemble a host, and the scaffold dropped `host.mjs` / `index.html`
/ `package.json` at the root with no clear serving story. This plan closes that
by making `twk build` produce the runnable artifact.

---

## Motivation

Two problems with the current shape:

* **Cluttered root.** `twk new` writes `host.mjs`, `index.html`, and
  `package.json` beside the `.tw` sources, mixing languages at the top level.
* **No clear serving path.** The generated `index.html` imports the bare
  specifier `@twinkle-lang/twinkle/web` (needs a bundler/import map) and
  references `target/<name>.lib.wasm` (needs the lib built and the directory
  served over HTTP) — none of which is spelled out.

The fix is to move the host harness from *scaffold output the user owns* to
*build output the tool generates* — `target/` is your `dist/`. Generated bundles
are regenerated each build and not hand-edited.

### Design note: why build-time-into-`target/`

This is deliberately the create-react-app model: a *managed*, regenerated bundle
you don't hand-edit, plus an escape hatch to take ownership. We considered two
"cleaner" corners and rejected them for this project's priority (one command →
runnable):

* **Emit the runtime → static, npm-free bundle.** Dissolves version-skew and the
  regeneration dance, but `twk build` runs inside the boot compiler, which can't
  emit large JS without embedding the whole runtime — a real cost — and loses the
  bundler DX.
* **Vite as owned source (committed `platforms/`, opt-in scaffold).** Fully
  customizable, but not "runnable from one build" — it's scaffold-once-then-build.

We keep build-time-into-`target/` with eyes open: the npm/Vite resolution is what
creates the version-skew (see Runtime version pinning) and regeneration (see
Regeneration is non-destructive) constraints, and both are accepted and mitigated
below rather than designed away.

### Owning a generated bundle (escape hatch)

A bundle under `target/` is managed output. To take ownership:

* **Copy it out (v1).** Move the bundle into your source tree; `twk` only manages
  `target/`, so a copy elsewhere is yours and never regenerated. Zero
  implementation, always available — this is the documented path.
* **`twk eject` (future).** A create-react-app-style command that relocates a
  bundle to a committed path and stops `twk` managing it. Nice affordance;
  deferred.
* **Un-gitignoring in place is discouraged** — the directory is still regenerated
  on the next `twk build`, so a committed-in-place bundle fights the tool.

---

## Scaffold: revert to Twinkle-only

`twk new <name>` / `twk init` stop emitting JS/web files. The scaffold is:

```
demo/
  twinkle.toml
  .gitignore
  demo.tw          # lib entry (pub surface)
  cmd/demo.tw      # command entry
  tests/main.tw
```

Remove `node_host` / `web_host` / `package_json` template generation from
`scaffold.tw` (and their assertions from `project_scaffold_suite`). The `[lib]`
entry in `twinkle.toml` stays — the lib build is core; platforms are optional
consumers produced by `twk build`.

---

## `twk build --node` / `--web`

Two additive, combinable flags. Each builds the lib entry first (no need to also
pass `--lib`), then writes a bundle. They work in project mode (using the `[lib]`
entry) or with an explicit file, exactly like `--lib`.

### CLI contract

The three lib-family flags — `--lib`, `--node`, `--web` — are **additive output
selectors** and combine freely; each adds its output. Any lib-family flag builds
the lib and writes the raw wasm at `target/<name>/<name>.lib.wasm`; `--node` /
`--web` additionally write their bundle directory (with its own wasm copy). So:

| Invocation | Produces |
|---|---|
| `twk build --lib` | `target/<name>/<name>.lib.wasm` only |
| `twk build --node` | raw wasm + `target/<name>/node/` |
| `twk build --node --web` | raw wasm + `node/` + `web/` |
| `twk build --lib --node --web` | same as above (`--lib` is implied by the bundle flags) |

**`-o`** maps to a single file, so it is only meaningful for a plain command
build or `--lib` **alone** (it overrides the wasm path). It is **rejected with
`--node` / `--web`** (those emit directories) — error:
`-o/--output cannot be combined with --node/--web`.

**`--target` / `--all`** select across the lib family exactly as they do for
project builds: `--all` builds every lib entry, `--target <name>` the named one,
and with a sole `[lib]` entry neither is required. They only become *necessary*
once multiple lib entries are configured (see Multi-entry, shipped). The
explicit-file form (`twk build --node path/to/entry.tw`) bypasses selection.

### Output layout — grouped, collision-safe

Everything for a lib entry lives under `target/<name>/`, so multiple entries
never collide:

```
target/
  demo/
    demo.lib.wasm
    node/
      package.json      # dep @twinkle-lang/twinkle; scripts: { start }
      main.mjs          # import { loadLib } from "@twinkle-lang/twinkle"
      demo.lib.wasm
    web/
      package.json      # dep @twinkle-lang/twinkle; devDep vite; scripts: { dev, build }
      vite.config.js
      index.html
      main.mjs          # import { loadLib } from "@twinkle-lang/twinkle/web"
      demo.lib.wasm
```

* **`main.mjs`** is the program entry for both platforms (replaces the odd
  `host.mjs`). Web's entry is `index.html` → `main.mjs`; node runs `main.mjs`.
* Each bundle carries **its own copy of `demo.lib.wasm`**, so it is movable and
  Vite needs no `server.fs.allow` escape for a wasm outside its root.
* **`--lib`'s output moves under `target/<name>/`** (from today's
  `target/<name>.lib.wasm`) so all artifacts for an entry are grouped and
  collision-safe. This is a small behavior change to the young `--lib` feature;
  `default_lib_output_path` / `default_lib_build_output` in `build.tw` /
  `context.tw` change accordingly.

### Toolchains

* **Node** is a minimal npm project — `package.json` (dep `@twinkle-lang/twinkle`)
  + `main.mjs`. Run: `npm install && npm start`.
* **Web** is a minimal Vite project — `package.json` (dep
  `@twinkle-lang/twinkle`, devDep `vite`), a tiny `vite.config.js`, `index.html`,
  `main.mjs`. Run: `npm install && npm run dev`. Vite resolves the bare
  `@twinkle-lang/twinkle/web` import and serves the wasm as an asset.

Because both platforms resolve the runtime via npm, **`twk` never emits
`runtime.mjs`** — it only writes small text templates and copies the `.lib.wasm`
in. This is the key simplification over an "emit the runtime into the bundle"
approach.

### Generated `main.mjs` is library-agnostic

The templates cannot assume any particular export exists — an arbitrary lib may
export different names, or nothing eligible. So generated `main.mjs` is generic:
it `loadLib`s `./<name>.lib.wasm` and **discovers** the surface via the object
`loadLib` returns (its enumerable keys are the exports), then reports it rather
than calling a hard-coded `lib.add`:

* **Node** — load, then print each export with its kind (`typeof lib[k]` →
  function vs value) and eagerly show zero-arg value getters; end with a one-line
  "edit this file to call your exports" hint.
* **Web** — load, then render that same export list into the page (`<pre>`), so
  opening the page shows what is callable.

The scaffold's demo lib happens to export `add` / `pi`, but demonstrating a
concrete call belongs in docs (the embeddable-lib handbook), **not** the generic
bundle template.

### Runtime version pinning

`package.json` pins `@twinkle-lang/twinkle` to the **compiler's own package
version** (as printed by `twk version`), exact-matched — the runtime reads the
`twinkle.exports` metadata this compiler emits, so a floating range risks
metadata/runtime ABI skew. For a dev/unpublished `twk` whose version has no
matching published release, emit `"latest"` and print a caution that the bundle
pins an unreleased-matching runtime and may skew until `twk` is released.

### Regeneration is non-destructive

Because users run `npm install` inside `target/<name>/{node,web}`, a rebuild must
not clobber their install. `twk build` **overwrites only the generated files it
owns** — `package.json`, `main.mjs`, `index.html`, `vite.config.js`, and the
copied `<name>.lib.wasm` — writing them in place. It never recreates the bundle
directory wholesale and never deletes `node_modules/` or a lockfile
(`package-lock.json`). Re-pinning the runtime version in `package.json` may
require the user to re-run `npm install`, which is expected; nothing is removed
on their behalf.

### Serving guidance

After a bundle build, `twk` prints the run recipe, e.g.:

```
Wrote target/demo/node →  cd target/demo/node && npm install && npm start
Wrote target/demo/web  →  cd target/demo/web && npm install && npm run dev
```

`twk build --help` documents `--lib` / `--node` / `--web` and this workflow.

---

## Multi-entry resolution (shipped)

The layout is keyed by `<name>`, so multiple lib entries slot in without churn.

### Config — a list, consistent with `[project]` / `[test]`

```toml
[lib]
entries = ["math.tw", "text.tw"]   # canonical (like project/test)
# entry = "demo.tw"                 # accepted shorthand → normalized to a 1-element list
```

Config normalizes both forms to `lib_entries: Vector<String>`. Each entry's
target name is its file stem (`derive_target_name`, as for project entries),
producing `target/math/`, `target/text/`. Two lib entries with the same stem are
rejected by the shared `reject_conflicts` check (now run over lib entries too) —
lib names must be unique among themselves, though a lib entry may still share a
stem with a project entry (the scaffold's `demo.tw` and `cmd/demo.tw` do, and
their outputs — `target/demo/` vs `target/demo.wasm` — don't collide).

### Selection — mirror project-build semantics, across the whole lib family

`--target` / `--all` / default apply uniformly to `--lib`, `--node`, `--web` via
`ProjectContext.select_lib_targets` (a lib analog of `select_build_targets`):

| Invocation | Behavior |
|---|---|
| `twk build --web` (one lib entry) | builds that entry |
| `twk build --web` (several) | error: "multiple lib entries; pass --target <name> or --all" |
| `twk build --web --target math` | builds the `math` entry's web bundle |
| `twk build --web --all` | builds web bundles for every lib entry |
| `twk build --web path/to/x.tw` | builds that explicit file (bypasses selection) |

Platform flags still combine (`--web --node --all` → both bundles for all
entries). `-o` is rejected when the selection resolves to more than one lib
target, mirroring project mode.

---

## Migration: `--lib` output path change

Moving `--lib`'s default output from `target/<name>.lib.wasm` to
`target/<name>/<name>.lib.wasm` is a breaking change to the young `--lib`
feature. Required follow-through:

* Update `default_lib_output_path` (`context.tw`) and `default_lib_build_output`
  (`build.tw`) to the grouped path.
* Update doc references — [embeddable-lib-build.md](embeddable-lib-build.md) and
  [lib-export-abi.md](lib-export-abi.md) both cite `target/<name>.lib.wasm`.
* Add/adjust tests for the new default path in **both** project mode and the
  explicit-file form (`twk build --lib file.tw`), and confirm `-o` still
  overrides for `--lib` alone.
* Since the scaffold's JS host (which referenced `./target/<name>.lib.wasm`) is
  being removed anyway, no scaffolded file needs the old path.

---

## Affected Components

| Component | Change |
|-----------|--------|
| `boot/lib/project/scaffold.tw` + `project_scaffold_suite` | drop `node_host`/`web_host`/`package_json` templates and assertions; scaffold is Twinkle-only |
| `boot/commands/build.tw` | `--node` / `--web` flags; reject `-o` with them; select lib targets via `--target`/`--all`/default and loop; grouped `target/<name>/` output (incl. moved `--lib` path); build lib then write bundles non-destructively; print run recipes |
| `boot/main.tw` | register `--node` / `--web` on `build_cmd`; help text |
| Bundle templates (new, in a `boot/lib/project/bundle.tw`) | library-agnostic `main.mjs` (node + web), `index.html`, `vite.config.js`, version-pinned `package.json` generators |
| version source | read the compiler version (as `twk version` prints) to pin `@twinkle-lang/twinkle` |
| `boot/lib/project/config.tw` | `lib_entries: Vector<String>`; parse `[lib] entries` list + `entry` shorthand |
| `boot/lib/project/context.tw` | `lib_entries: Vector<EntryTarget>`; `select_lib_targets`; `reject_conflicts` over lib entries |

No stage0 change expected (the bundle emission is boot-only file writing).

---

## Testing

* **Boot** — extend the project/build suites:
  * `--lib` writes the new grouped path `target/<name>/<name>.lib.wasm` (project
    mode and explicit file); `-o` still overrides for `--lib` alone.
  * `--node` writes `target/<name>/node/{package.json,main.mjs,<name>.lib.wasm}`;
    `--web` writes the Vite set; both copy the wasm in.
  * `-o` with `--node`/`--web`, and `--target`/`--all` with any lib-family flag,
    are rejected with the specified errors.
  * `package.json` pins the compiler version; `main.mjs` contains no hard-coded
    export name.
  * the scaffold no longer emits JS files.

  Assert file presence and key template content (bare-import lines, scripts,
  pinned version, export-discovery), not full file bodies.
* **Runtime** — the existing `loadLib` coverage in `runtime.test.mjs` already
  exercises the marshalling the bundles rely on; a light test can build a node
  bundle and `import`/run its `main.mjs` against the copied wasm. Serving the
  Vite web bundle end to end is left to manual verification (needs `npm install`).
