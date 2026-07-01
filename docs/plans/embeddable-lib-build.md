# Embeddable `lib` Build

**Status:** Implemented.

## Goal

Let a Twinkle project be **embedded into an existing Node or web application as a
library** — the host imports a built artifact and calls named Twinkle functions
(`lib.add(2, 3)`), rather than only triggering a run-once program.

Today `twk build` emits `target/<name>.wasm`, but that artifact:

* exports only `__twinkle_start` (run the top-level statements), `__task_run`
  (if tasks are used), and `memory` — **no user functions are callable by name**;
* is not self-contained — it instantiates only with the host imports from
  `runtime.mjs` + `bridge.wasm`, which live in the `@twinkle-lang/twinkle` npm
  package;
* ships with no guidance on how a host should load and drive it.

This plan introduces a second **build kind** that closes that gap for the
smallest feasible ABI, and surfaces a compiler-free loader so the built program
(not the megabyte compiler) is what gets embedded.

> **Depended on [bridge-in-runtime.md](archive/bridge-in-runtime.md)** — folding
> the JS↔Wasm-GC bridge into the runtime (removing the separate `bridge.wasm`
> asset) is what lets the embedded artifact be a single `<name>.lib.wasm` file.
> That is now landed, so this dependency is satisfied.

---

## Two Build Kinds

The project scaffold already encodes the distinction this plan formalizes:

| Kind | Entry (scaffold) | Wasm surface | Interaction |
|------|------------------|--------------|-------------|
| **command** (today) | `cmd/<name>.tw` | `__twinkle_start` runs top-level statements | program-drives: host supplies `extern` functions, the program calls out |
| **lib** (new) | root `<name>.tw` (`pub` module) | named exports per `pub` member | host-drives: host calls named Twinkle functions |

The command build is unchanged. The lib build reads the entry module's **`pub`
surface** and turns it into the wasm export surface.

---

## Scope (v1)

Deliberately minimal — see [Future Planned](#future-planned) for everything
intentionally deferred.

### Invocation & output

* `twinkle.toml` gains a `[lib]` section with a single `entry`:

  ```toml
  [lib]
  entry = "mathx.tw"
  ```

* `twk build --lib` builds the configured lib entry (project mode);
  `twk build --lib path/to/file.tw` builds an explicit file.
* Output goes to `target/<name>/<name>.lib.wasm` (grouped under the entry's
  bundle directory; see [platform-build-bundles.md](platform-build-bundles.md)).
  The `.lib` infix keeps it from colliding with a command build of the same stem
  (the scaffold's `cmd/<name>.tw` and root `<name>.tw` share a stem).

### Export surface — primitives only

* Every `pub fn` in the **entry module** whose parameters **and** return type are
  all primitive (`Int`, `Float`, `Bool`, `Void`) is exported under its bare name.
* Every `pub` value global of primitive type (e.g. `pub pi: Float = 3.14159`) is
  exported **as a synthesized zero-arg getter function** (`__get_pi() f64`). Its
  initializer still runs as part of `__twinkle_start`; the getter just reads the
  initialized global. This keeps the whole export surface uniformly functions —
  one metadata schema, one DCE-root kind, no new wasm global-export IR — and
  matches the loader's "value getter" shape (the loader calls it once after start
  and exposes it as a plain property).
* `pub` members that are **generic**, or that use `String` / records / arrays /
  dicts / function types, are **skipped with a build warning** that names each
  member and the reason (e.g. "`greet`: String parameters are not yet supported
  in lib exports"). The library still builds with its eligible surface.
* Non-`pub` functions are never exported.

### ABI & metadata

* Primitive identity mapping: `Bool` = i32, `Int` = i64, `Float` = f64,
  `Void` = no result.
* Codegen emits a **`twinkle.exports` custom section** mirroring the existing
  `twinkle.externs` metadata: for each export, its bare name and param/return
  primitive types (value-global getters carry an empty param list). It is built
  exactly like `twinkle.externs` is — `collect_extern_meta` in `codegen.tw`
  derives the externs section from `anf.extern_imports`; the exports section is
  the mirror, derived from the captured pub-export surface (see *Where the
  export surface is captured*). The host builds typed wrappers from this
  metadata — no hand-written ABI specs.
* **`Int` is i64**, which crosses to JavaScript as `BigInt`. The exports metadata
  lets the loader coerce `Number` ↔ `BigInt` at the boundary so hosts pass and
  receive plain numbers.
* Exported functions are **DCE roots automatically**: `compute_reachable` in
  `boot/compiler/codegen/linker_dce.tw` already seeds the worklist from
  `linked.exports`, and `retain_final_exports("user")` is already `true`, so the
  flat module keeps them. The only thing needed is to add the eligible pub
  members as `ExportDef`s on the user module in `emit.tw` — no change to
  `retain_final_exports`.

### Where the export surface is captured

The selection cannot happen in the linker or codegen: by then all user code is a
single merged module (`namespace: "user"`), `ExportDef` is only
`{ wasm_name, func_sym }` with no types, and monomorphization has already dropped
unused generics and renamed the specialized ones — so a generic `pub fn greet`
either no longer exists or is several clones, and can't be named in a warning.

Selection therefore happens **post-typecheck, where `pub` flags, source-module
identity, source names, and pre-mono signatures all still coexist** — the same
layer where `lower_core.tw` already collects `pub_value_globals` per module. For
the configured lib entry, walk its module's `pub` members, classify each:

* eligible (all-primitive `fn`, or primitive value global) → record
  `(bare_name, func_id_or_global_id, param/ret primitive kinds)`;
* ineligible (generic, or non-primitive type) → emit the warning and skip.

Carry that list down as a new field on `AnfModule` (e.g. `lib_exports`), the
mirror of `extern_imports`. Codegen then:

* in `emit.tw`, adds an `ExportDef { wasm_name: bare_name, func_sym }` per export
  (and synthesizes a getter func for each value global), making them DCE roots;
* in `codegen.tw` / `wasm.tw` / `wat.tw`, emits the `twinkle.exports` section
  from `lib_exports`, exactly as `collect_extern_meta` →
  `encode_extern_meta_section_payload` does for externs.

Bare export names are unique because v1 exports a single entry module's surface,
so no cross-module mangling is needed; the linker-qualified `func_sym` is only
used internally to point the export at the right function.

A command build leaves `lib_exports` empty, so this whole path is inert unless
`--lib` is set — the command artifact is byte-for-byte unchanged.

### Host runtime — compiler-free `loadLib` (Node + web)

A new helper instantiates a *prebuilt* program wasm with host externs and
returns a typed handle. It does **not** load the compiler.

* **Node** — in `tools/js_runtime/index.mjs` (staged as the npm `index.mjs`), beside the existing `run(wasm, …)`.
* **Web** — in `tools/js_runtime/web.mjs`. The web entry currently only exposes
  source-based `run`/`command` (which self-load `boot.wasm`); this surfaces
  `runtime.mjs`'s existing compiler-free `runWasmBytesAsync` path, closing the
  "ship the program, not the megabyte compiler" gap.

Shape:

```js
import { loadLib } from "@twinkle-lang/twinkle";          // Node
// import { loadLib } from "@twinkle-lang/twinkle/web";   // browser

const lib = await loadLib(new URL("./target/mathx/mathx.lib.wasm", import.meta.url), {
  imports: { /* externs the lib declares, if any */ },
});

lib.add(2, 3); // 5     — Number in, Number out (BigInt coerced internally)
lib.pi;        // 3.14159 — exported value global
```

`loadLib` instantiates with `imports`, calls `__twinkle_start` once (so value
globals and any module state are initialized), reads `twinkle.exports`, and
returns an object of typed wrapper functions plus value getters.

> **Depended on [bridge-in-runtime.md](archive/bridge-in-runtime.md).** For the
> embedded artifact to be a single file, the JS↔Wasm-GC bridge must ship inside
> the runtime rather than as a separate `bridge.wasm` asset. That folding has
> landed (the bridge is embedded in `runtime.mjs` via `bridge_bytes.mjs`), so the
> "only ship `<name>.lib.wasm`" guarantee here is now unblocked.

### Scaffold — Node + web harness

> **Superseded by [platform-build-bundles.md](platform-build-bundles.md).** The
> scaffold no longer emits `host.mjs` / `index.html` / `package.json`; it stays
> Twinkle-only. The runnable Node/web harness is now *build output*, generated by
> `twk build --node` / `--web` under `target/<name>/{node,web}/`. The `[lib]
> entry` line in the generated `twinkle.toml` stays.

---

## Future Planned

These are intentionally **out of v1**. The fuller ABI now has its own plan,
[lib-export-abi.md](lib-export-abi.md); the items below are the headline pieces
it covers (kept here so the direction is on record):

* **String args/returns** — marshal `String` across the boundary via the existing
  bridge. Covers the common "compute and return text" library.
* **Function-typed (callback) parameters** — `pub fn each_line(text, f: fn(String) Void)`,
  where the host passes a JS callback. This is essentially a *dynamic* `extern`:
  it reuses the runtime's existing host-function wrapping (`resolveExternImports`),
  bound per call instead of at instantiate time.

  > Together, **String + callback params** form what was discussed as
  > "approach C" during design. It is the intended next increment after this
  > primitives-only v1, but is **future planned**, not part of this plan's scope.

* **Returned closures** — handing JS a callable handle to a guest closure
  (`pub fn make_counter() fn() Int`), leaning on the typed-closure struct
  (`$closure_*`) codegen already emits, plus a JS `apply(closureRef, args)` helper.
* **Compound values** (records / arrays / dicts) crossing the boundary, e.g. via a
  JSON-ish encode/decode at the edge rather than live GC refs.
* ~~**Multiple lib entries** per project~~ — **shipped** in
  [platform-build-bundles.md](platform-build-bundles.md): `[lib] entries = [...]`
  with `--target` / `--all` selection.
* **`hard error` mode** for ineligible `pub` members, if warn-and-skip proves too
  permissive in practice (v1 warns and skips).

---

## Testing

* **Boot** — `boot/tests/suites/lib_export_suite.tw` drives `compile_source_lib`
  (the in-memory `--lib` build) and checks export-surface selection: primitive
  `pub` functions kept with their param/return kinds, primitive value globals
  exported as `__get_*` getters, and generic / `String` members skipped with the
  expected warnings (non-`pub` members neither exported nor warned). Lib-entry
  resolution from `[lib]` is covered by `project_config_suite` / `project_context_suite`.
* **Runtime** — `tools/js_runtime/runtime.test.mjs` builds a primitives lib via
  `compile(src, { lib: true })`, `loadLib`s it, and asserts `add(2, 3) === 5n`
  (`Int` returns are `BigInt`), a `Bool` round-trip, an exported value global
  (`pi`), and that a `String`-typed `pub fn` is absent from the surface.

---

## Affected Components

| Component | Change |
|-----------|--------|
| `boot/lib/project/config.tw` | parse `[lib] entry` |
| `boot/lib/project/context.tw` | resolve the lib entry as a build target |
| `boot/commands/build.tw` | `--lib` flag; lib build path → `target/<name>/<name>.lib.wasm` (grouped, per [platform-build-bundles.md](platform-build-bundles.md)); thread "this is a lib build of entry module M" into the pipeline |
| `boot/main.tw` | register the `--lib` flag on `build_cmd` |
| `boot/compiler/lower_core.tw` (+ checker/driver) | capture the entry module's eligible `pub` export surface (primitive `fn`s + value globals) with source names & signatures; warn-and-skip ineligible members. Beside the existing `pub_value_globals` capture |
| `boot/compiler/anf.tw` + `lower_anf.tw` | carry the captured surface as a new `AnfModule.lib_exports` field (mirror of `extern_imports`) |
| `boot/compiler/codegen/emit.tw` | add an `ExportDef` per eligible member (DCE root, automatic); synthesize a zero-arg getter func per value global |
| `boot/compiler/codegen/{codegen,wasm,wat}.tw` | emit the `twinkle.exports` custom section from `lib_exports`, mirroring `collect_extern_meta` / `encode_extern_meta_section_payload`. **No change to `linker.tw` / `retain_final_exports`** |
| `boot/lib/project/scaffold.tw` + `boot/commands/scaffold.tw` | `[lib]` entry only — the JS/web harness moved to `twk build --node`/`--web` output (see [platform-build-bundles.md](platform-build-bundles.md)) |
| `tools/js_runtime/index.mjs` | Node `loadLib` (staged to npm as `index.mjs`) |
| `tools/js_runtime/web.mjs` | web `loadLib` (compiler-free) |
| `src/` (stage0) | only if the lib build is exercised within `boot/` source itself (likely not — no stage0 parity needed) |
