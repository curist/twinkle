# Embedding Twinkle in JavaScript

`@twinkle-lang/twinkle` ships both the `twk` CLI and an embeddable library for
compiling and running Twinkle programs from Node.js (≥ 22).

## Install

```bash
npm install @twinkle-lang/twinkle
```

## CLI (`twk`)

The npm `twk` is the full self-hosted compiler — every subcommand works:

```bash
npx twk run program.tw            # compile + run
npx twk build program.tw -o out.wasm
npx twk check program.tw          # type-check only
npx twk fmt program.tw            # format in place
npx twk ir program.tw --opt       # print optimized IR
npx twk lsp                       # language server (stdio)
```

Install globally for a bare `twk`:

```bash
npm install -g @twinkle-lang/twinkle
twk run program.tw
```

## Library

The package is ESM-only — use `import`:

```js
import { compile, run, runFile, runSource } from "@twinkle-lang/twinkle";
```

CommonJS consumers can load it through Node's ESM interop (`require()` of ESM is
unflagged on Node ≥ 22.12), but CommonJS is not a primary target — prefer
`import` or a dynamic `await import("@twinkle-lang/twinkle")`.

### `compile(input, opts?) -> Promise<Uint8Array>`

`input` is either a **file path string** or `{ source, path? }` for in-memory
source. Returns the compiled wasm bytes. Throws with the compiler diagnostics on
error.

> **Source-context limitation:** a path argument gets full project support —
> relative imports (`use .sibling`) and walk-up `twinkle.toml` discovery resolve
> from the file's real location. `{ source }` is written to a temporary
> directory and compiled as a single file, so relative imports and project-root
> discovery will not resolve. Use a path for multi-file projects.

### `run(wasmBytes, opts?) -> Promise<number>`

Runs pre-compiled wasm and resolves to the program's exit code. Loads only the
tiny bridge module — no compiler. Options: `imports`, `args`, `cwd`, `env`,
`stdout`, `stderr`, `path`.

### `runFile(path, opts?)` / `runSource(source, opts?)`

Compile-then-run conveniences taking the same `opts` as `run`.

## Wiring extern (host) functions

Declare host functions in Twinkle with `extern`:

```tw
extern canvas {
  fn draw_rect(x: Float, y: Float, w: Float, h: Float)
  fn clear()
}
```

Wire them by passing a **scoped `imports` object** — keyed by extern module
name, then function name:

```js
await runFile("game.tw", {
  imports: {
    canvas: {
      draw_rect: (x, y, w, h) => ctx.fillRect(x, y, w, h),
      clear: () => ctx.clearRect(0, 0, W, H),
    },
  },
});
```

### Auto-wiring of host globals

Extern modules that already exist on `globalThis` — `Math`, `console`,
`crypto`, … — resolve automatically. You only wire what isn't ambient:

```js
await runFile("calc.tw");                       // uses extern Math/console, no imports
await runFile("game.tw", { imports: { canvas } }); // only canvas needs wiring
```

Resolution order per extern import: `imports[module][name]` → `globalThis[module][name]`.
Explicit `imports` therefore **shadow** globals — pass your own `console` to
capture output:

```js
const lines = [];
await runFile("program.tw", { imports: { console: { log: (m) => lines.push(m) } } });
```

### Missing imports

If an extern is satisfied by neither `imports` nor `globalThis`, `run` throws a
single error naming every unsatisfied symbol:

```
Missing host import(s): canvas.draw_rect, canvas.clear
Provide them via the run() "imports" option (e.g. { imports: { canvas: { draw_rect: fn } } }) or define them on globalThis.
```

## Boundary types

Extern parameter/return types are limited to `Int`, `Float`, `Bool`, `String`,
extern handle types, and `Void`. `Int` arrives in JS as a `number` (converted
from i64), `Float` as a `number`, `Bool` as `0`/`1`, `String` as a JS string.
See `docs/spec.md` §7.2 for the full extern rules.
