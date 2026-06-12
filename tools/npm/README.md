# @twinkle-lang/twinkle

Twinkle is a statically typed, value-oriented language targeting Wasm GC. This
package ships both the `twk` command-line compiler and an embeddable JS library
for compiling and running Twinkle programs from Node.js.

## Install

```bash
npm install @twinkle-lang/twinkle
```

## CLI

```bash
npx twk run path/to/program.tw
npx twk build path/to/program.tw -o out.wasm
npx twk fmt path/to/program.tw
```

## Node library

```js
import { compile, run, runFile } from "@twinkle-lang/twinkle";

// Host functions declared in Twinkle as `extern canvas { fn draw_rect(...) }`
// are wired by passing a scoped imports object — no globalThis pollution.
await runFile("game.tw", {
  imports: {
    canvas: { draw_rect: (x, y, w, h) => { /* ... */ } },
  },
});

// Host globals (Math, console, crypto, ...) resolve automatically:
await runFile("calc.tw");

// Compile once, run many times:
const wasm = await compile("game.tw");
await run(wasm, { imports: { canvas } });
```

A missing extern import produces a clear error naming the exact `module.fn`.

## Browser library

```js
import { command, run, load } from "@twinkle-lang/twinkle/web";

await load(); // optional prefetch

const result = await command(["fmt", "/input/main.tw"], {
  source: editor.getValue(),
  env: { NO_COLOR: "1" },
});

if (result.exitCode === 0) {
  editor.setValue(result.text("/input/main.tw"));
}
```

`command(args, opts)` runs the shipped compiler payload against an in-memory
filesystem and returns `{ exitCode, stdout, stderr, files, text(path), bytes(path) }`.
Compiler failures are returned as non-zero exit codes; host/runtime failures throw.
The existing browser `run(source, opts)` helper is still available and returns an
exit code.

Requires Node.js ≥ 22 for the Node APIs. Browser APIs require WebAssembly GC.
