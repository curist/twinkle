# Embeddable `lib` Build

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
* Output goes to `target/<name>.lib.wasm`. The `.lib` infix keeps it from
  colliding with a command build of the same stem (the scaffold's `cmd/<name>.tw`
  and root `<name>.tw` share a stem).

### Export surface — primitives only

* Every `pub fn` in the **entry module** whose parameters **and** return type are
  all primitive (`Int`, `Float`, `Bool`, `Void`) is exported under its bare name.
* Every `pub` value global of primitive type (e.g. `pub pi: Float = 3.14159`) is
  exported. Its initializer runs as part of `__twinkle_start`.
* `pub` members that are **generic**, or that use `String` / records / arrays /
  dicts / function types, are **skipped with a build warning** that names each
  member and the reason (e.g. "`greet`: String parameters are not yet supported
  in lib exports"). The library still builds with its eligible surface.
* Non-`pub` functions are never exported.

### ABI & metadata

* Primitive identity mapping: `Bool` = i32, `Int` = i64, `Float` = f64,
  `Void` = no result.
* The linker emits a **`twinkle.exports` custom section** mirroring the existing
  `twinkle.externs` metadata: for each export, its param/return primitive types.
  The host builds typed wrappers from this metadata — no hand-written ABI specs.
* **`Int` is i64**, which crosses to JavaScript as `BigInt`. The exports metadata
  lets the loader coerce `Number` ↔ `BigInt` at the boundary so hosts pass and
  receive plain numbers.
* Exported functions are **DCE roots**: they are not reachable from
  `__twinkle_start`, so the linker must retain them. The existing
  `retain_final_exports` machinery in `boot/compiler/codegen/linker.tw` is the
  hook.

### Host runtime — compiler-free `loadLib` (Node + web)

A new helper instantiates a *prebuilt* program wasm with host externs and
returns a typed handle. It does **not** load the compiler.

* **Node** — in `tools/npm` `index.mjs`, beside the existing `run(wasm, …)`.
* **Web** — in `tools/js_runtime/web.mjs`. The web entry currently only exposes
  source-based `run`/`command` (which self-load `boot.wasm`); this surfaces
  `runtime.mjs`'s existing compiler-free `runWasmBytesAsync` path, closing the
  "ship the program, not the megabyte compiler" gap.

Shape:

```js
import { loadLib } from "@twinkle-lang/twinkle";          // Node
// import { loadLib } from "@twinkle-lang/twinkle/web";   // browser

const lib = await loadLib(new URL("./target/mathx.lib.wasm", import.meta.url), {
  imports: { /* externs the lib declares, if any */ },
});

lib.add(2, 3); // 5     — Number in, Number out (BigInt coerced internally)
lib.pi;        // 3.14159 — exported value global
```

`loadLib` instantiates with `imports`, calls `__twinkle_start` once (so value
globals and any module state are initialized), reads `twinkle.exports`, and
returns an object of typed wrapper functions plus value getters.

### Scaffold — Node + web harness

`twk new` / `twk init` additionally:

* add the `[lib] entry = "<name>.tw"` to the generated `twinkle.toml`;
* generate a Node `host.mjs` example that calls `loadLib("./target/<name>.lib.wasm")`
  and invokes an exported function;
* generate a web `index.html` + loader that does the same in the browser;
* generate a minimal `package.json` depending on `@twinkle-lang/twinkle`, so
  `node host.mjs` runs out of the box.

---

## Future Planned

These are intentionally **out of v1**. The headline follow-up is the fuller ABI
(noted here so the direction is on record):

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
* **Multiple lib entries** per project (v1 supports a single `[lib] entry`).
* **`hard error` mode** for ineligible `pub` members, if warn-and-skip proves too
  permissive in practice (v1 warns and skips).

---

## Testing

* **Boot** — a suite (in the style of `boot/tests/suites/project_scaffold_suite.tw`)
  covering: lib-entry resolution from `[lib]`, export-surface selection (eligible
  primitives kept, ineligible members skipped with the expected warnings),
  `twinkle.exports` metadata content, and export retention through DCE.
* **Runtime** — a Node test that instantiates a built primitives lib via
  `loadLib` and asserts `add(2, 3) === 5`, reads an exported value global, and
  confirms a `String`-typed `pub fn` was skipped (warning surfaced, not exported).

---

## Affected Components

| Component | Change |
|-----------|--------|
| `boot/lib/project/config.tw` | parse `[lib] entry` |
| `boot/lib/project/context.tw` | resolve the lib entry as a build target |
| `boot/commands/build.tw` | `--lib` flag; lib build path → `target/<name>.lib.wasm` |
| `boot/main.tw` | register the `--lib` flag on `build_cmd` |
| `boot/compiler/codegen/linker.tw` | export entry `pub` primitives; retain as DCE roots |
| `boot/compiler/codegen/{wasm,wat}.tw` | emit `twinkle.exports` custom section |
| `boot/lib/project/scaffold.tw` + `boot/commands/scaffold.tw` | `[lib]` entry, `host.mjs`, `index.html`, `package.json` templates |
| `tools/npm/index.mjs` | Node `loadLib` |
| `tools/js_runtime/web.mjs` | web `loadLib` (compiler-free) |
| `src/` (stage0) | only if the lib build is exercised within `boot/` source itself (likely not — no stage0 parity needed) |
