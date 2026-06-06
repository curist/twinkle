# @twinkle-lang/twinkle

Twinkle is a statically typed language targeting WebAssembly GC. This package
ships both the `twk` command-line compiler and an embeddable JS library for
compiling and running Twinkle programs from Node.js.

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

## Library

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

Requires Node.js ≥ 22.
