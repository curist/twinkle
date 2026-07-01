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
are regenerated each build and not hand-edited; a user who wants to own the host
copies the bundle out.

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

**`--target` / `--all`** are **rejected for all lib-family builds in the first
cut**, exactly as `--lib` already rejects them today — there is only one `[lib]
entry`, so there is nothing to select. The only override is the explicit-file
form (`twk build --node path/to/entry.tw`). `--target` / `--all` gain meaning
only when multiple lib entries land (see Multi-entry, deferred).

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

## Multi-entry resolution (designed; implementation deferred)

The layout is keyed by `<name>`, so multiple lib entries slot in without churn.
The shape below is decided so the first implementation doesn't paint us into a
corner; only single-entry ships first.

### Config — a list, consistent with `[project]` / `[test]`

```toml
[lib]
entries = ["math.tw", "text.tw"]   # canonical (like project/test)
# entry = "demo.tw"                 # accepted shorthand → normalized to a 1-element list
```

Config normalizes both forms to `Vector<String>`. Each entry's target name is its
file stem (`derive_target_name`, as for project entries), producing
`target/math/`, `target/text/`. Two lib entries with the same stem are rejected
by the existing conflict check, extended to cover lib entries — names must be
unique.

### Selection — mirror project-build semantics, across the whole lib family

Once multiple entries exist, `--target` / `--all` / default apply uniformly to
`--lib`, `--node`, `--web`. **This is the deferred behavior** — in the first cut
`--target` / `--all` are rejected (see CLI contract):

| Invocation | Behavior (deferred) |
|---|---|
| `twk build --web` (one lib entry) | builds that entry |
| `twk build --web` (several) | error: "multiple lib entries; pass --target <name> or --all" |
| `twk build --web --target math` | builds the `math` entry's web bundle |
| `twk build --web --all` | builds web bundles for every lib entry |
| `twk build --web path/to/x.tw` | builds that explicit file (works in the first cut too) |

Platform flags still combine (`--web --node --all` → both bundles for all
entries).

### Phasing

1. **First cut** — single `[lib] entry` (string, as today) + `--node` / `--web`
   + explicit-file selection + printed recipes + scaffold revert. `--target` /
   `--all` are **rejected** for lib-family builds.
2. **Deferred** — `entries` list parsing, `--target` / `--all` selection over lib
   entries, and the multi-entry conflict check. No layout or CLI redesign needed
   to add them — config parsing + a selection loop.

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
| `boot/commands/build.tw` | `--node` / `--web` flags; reject `-o` and `--target`/`--all` with them; grouped `target/<name>/` output (incl. moved `--lib` path); build lib then write bundles non-destructively; print run recipes |
| `boot/main.tw` | register `--node` / `--web` on `build_cmd`; help text |
| Bundle templates (new, in a `boot/lib/project/bundle.tw`) | library-agnostic `main.mjs` (node + web), `index.html`, `vite.config.js`, version-pinned `package.json` generators |
| version source | read the compiler version (as `twk version` prints) to pin `@twinkle-lang/twinkle` |
| `boot/lib/project/config.tw` (deferred) | parse `[lib] entries` list; keep `entry` shorthand |
| `boot/lib/project/context.tw` (deferred) | resolve multiple lib entries; extend conflict rejection |

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
